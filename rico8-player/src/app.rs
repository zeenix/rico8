//! The player's mode logic: the cart shelf, running a cart, and the error screen. Written
//! against the `Platform` trait so it runs identically on KMS, on a TTY, or headless in tests.

use crate::{
    picker,
    platform::{InputSnapshot, Platform},
};
use anyhow::Result;
use rico8_runtime::{
    audio::AudioHandle,
    cart,
    fb::{Framebuffer, HEIGHT},
    palette::col,
    ui,
    vm::{GameVm, UI_FPS},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Folds per-frame input snapshots into high-level actions: the universal hold-O+X exit,
/// Select/Start handling, and the fps toggle. Shared by every backend, tested headless.
#[derive(Default)]
pub struct Controls {
    combo_frames: u32,
}

/// What `Controls` decided this frame.
pub enum ControlAction {
    None,
    BackToPicker,
    Quit,
    ToggleFps,
}

impl Controls {
    /// `fps` is the current logical frame rate, so the ~1s hold scales with it.
    pub fn update(&mut self, snap: &InputSnapshot, fps: u32) -> ControlAction {
        if snap.quit_requested {
            return ControlAction::Quit;
        }
        if snap.select && snap.start {
            return ControlAction::Quit;
        }
        if snap.select {
            return ControlAction::BackToPicker;
        }
        if snap.fps_toggle {
            return ControlAction::ToggleFps;
        }
        if snap.buttons[4] && snap.buttons[5] {
            self.combo_frames += 1;
            if self.combo_frames >= fps.max(1) {
                self.combo_frames = 0;
                return ControlAction::BackToPicker;
            }
        } else {
            self.combo_frames = 0;
        }
        ControlAction::None
    }
}

/// What a finished game/picker loop wants to happen next.
pub enum Flow {
    Quit,
    BackToPicker,
}

/// One frame's wall-clock budget at a given logical frame rate.
fn frame_duration(fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000 / fps.max(1) as u64)
}

pub struct App {
    platform: Box<dyn Platform>,
    /// The synth the running cart writes and the platform's audio thread reads. Held here so a
    /// single handle is shared; `KmsPlatform` is constructed from a clone of it.
    audio: AudioHandle,
    /// Run only this many frames, then exit (CI smoke mode).
    smoke: Option<u32>,
}

impl App {
    pub fn new(platform: Box<dyn Platform>, audio: AudioHandle, smoke: Option<u32>) -> App {
        App {
            platform,
            audio,
            smoke,
        }
    }

    /// The cart shelf: list carts in a directory, pick one, play it.
    pub fn picker(&mut self, dir: &Path) -> Result<()> {
        loop {
            let carts = picker::scan_carts(dir)?;
            match self.picker_loop(dir, &carts)? {
                Some(path) => match self.play(&path) {
                    Ok(Flow::BackToPicker) => continue,
                    Ok(Flow::Quit) => return Ok(()),
                    Err(e) => {
                        eprintln!("rico8-player: {e:#}");
                        continue;
                    }
                },
                None => return Ok(()),
            }
        }
    }

    /// Run one cart until the player backs out or quits.
    pub fn play(&mut self, path: &Path) -> Result<Flow> {
        eprintln!("rico8-player: loading {}", path.display());
        let cart = match cart::load_png(path) {
            Ok(c) => c,
            Err(e) => return self.show_error(&format!("load failed\n{e}")),
        };
        // Stop any audio from a previous cart before loading the new VM.
        self.audio.stop_all();
        // The VM writes into the same synth the platform's audio thread reads.
        let mut vm = match GameVm::load(&cart.wasm, &cart.assets, self.audio.clone()) {
            Ok(vm) => Some(vm),
            Err(e) => return self.show_error(&format!("boot failed\n{e}")),
        };
        let fps = vm.as_ref().map(GameVm::fps).unwrap_or(UI_FPS);
        let frame = frame_duration(fps);
        eprintln!("rico8-player: running {}", path.display());

        let mut controls = Controls::default();
        let mut error_fb: Option<Framebuffer> = None;
        let mut next = Instant::now();
        let mut frames = 0u32;
        let mut show_fps = false;
        let mut fps_frames = 0u32;
        let mut fps_t0 = Instant::now();
        let mut fps_val = 0.0f32;

        loop {
            let snap = self.platform.poll();
            match controls.update(&snap, fps) {
                ControlAction::Quit => return Ok(Flow::Quit),
                ControlAction::BackToPicker => return Ok(Flow::BackToPicker),
                ControlAction::ToggleFps => show_fps = !show_fps,
                ControlAction::None => {}
            }
            if let Some(v) = vm.as_mut() {
                let input = &mut v.state_mut().input;
                for (b, pressed) in snap.buttons.iter().enumerate() {
                    input.set_button(b, *pressed);
                }
            }

            if let Some(v) = vm.as_mut() {
                if let Err(e) = v.call_update().and_then(|()| v.call_draw()) {
                    eprintln!("rico8-player: runtime error: {e}");
                    self.audio.stop_all();
                    let mut fb = ui::error_screen(&e.to_string());
                    fb.print("hold o+x to exit", 2, HEIGHT - 7, col::LIGHT_GREY);
                    error_fb = Some(fb);
                    vm = None;
                }
            }
            if show_fps {
                if let Some(v) = vm.as_mut() {
                    picker::draw_fps_overlay(&mut v.state_mut().fb, fps_val, fps);
                }
            }
            if let Some(v) = &vm {
                self.platform.present(&v.state().fb)?;
            } else if let Some(fb) = &error_fb {
                self.platform.present(fb)?;
            }

            frames += 1;
            if self.smoke.is_some_and(|n| frames >= n) {
                return Ok(Flow::Quit);
            }
            self.pace(&mut next, frame, &mut fps_frames, &mut fps_t0, &mut fps_val);
        }
    }

    /// Show a RICO-8 error screen until the player presses back.
    fn show_error(&mut self, message: &str) -> Result<Flow> {
        eprintln!("rico8-player: {}", message.replace('\n', ": "));
        self.audio.stop_all();
        let mut fb = ui::error_screen(message);
        fb.print("select/b: back", 2, HEIGHT - 7, col::LIGHT_GREY);
        let mut controls = Controls::default();
        let mut next = Instant::now();
        let mut shown = 0u32;
        loop {
            let snap = self.platform.poll();
            match controls.update(&snap, UI_FPS) {
                ControlAction::Quit => return Ok(Flow::Quit),
                ControlAction::BackToPicker => return Ok(Flow::BackToPicker),
                _ => {}
            }
            // Any face button also leaves the error screen, back to the picker.
            if snap.buttons[4] || snap.buttons[5] {
                return Ok(Flow::BackToPicker);
            }
            self.platform.present(&fb)?;
            shown += 1;
            if self.smoke.is_some_and(|n| shown >= n) {
                return Ok(Flow::Quit);
            }
            Self::sleep_until(&mut next, frame_duration(UI_FPS));
        }
    }

    fn picker_loop(&mut self, dir: &Path, carts: &[PathBuf]) -> Result<Option<PathBuf>> {
        let mut sel = 0usize;
        let mut frame = 0u32;
        let mut next = Instant::now();
        // `None` until the first frame establishes a baseline, so a button still held from the
        // previous screen (e.g. the in-game hold-O+X exit) is not read as a fresh press here.
        let mut prev: Option<InputSnapshot> = None;
        loop {
            let snap = self.platform.poll();
            if snap.quit_requested || snap.select {
                return Ok(None);
            }
            if let Some(p) = &prev {
                // Edge-detect d-pad up/down and any face button (launch).
                let edge = |i: usize| snap.buttons[i] && !p.buttons[i];
                if edge(2) {
                    sel = sel.saturating_sub(1);
                }
                if edge(3) {
                    sel = (sel + 1).min(carts.len().saturating_sub(1));
                }
                if (edge(4) || edge(5)) && !carts.is_empty() {
                    return Ok(Some(carts[sel].clone()));
                }
            }
            prev = Some(snap);

            let fb = picker::draw_picker(dir, carts, sel, frame);
            self.platform.present(&fb)?;
            frame += 1;
            if self.smoke.is_some_and(|n| frame >= n) {
                return Ok(None);
            }
            Self::sleep_until(&mut next, frame_duration(UI_FPS));
        }
    }

    fn pace(
        &self,
        next: &mut Instant,
        frame: Duration,
        fps_frames: &mut u32,
        fps_t0: &mut Instant,
        fps_val: &mut f32,
    ) {
        let now = Instant::now();
        *fps_frames += 1;
        let elapsed = now.duration_since(*fps_t0);
        if elapsed >= Duration::from_millis(500) {
            *fps_val = *fps_frames as f32 / elapsed.as_secs_f32();
            *fps_frames = 0;
            *fps_t0 = now;
        }
        Self::sleep_until(next, frame);
    }

    fn sleep_until(next: &mut Instant, frame: Duration) {
        *next += frame;
        let now = Instant::now();
        if *next > now {
            std::thread::sleep(*next - now);
        } else {
            *next = now;
        }
    }
}

#[cfg(test)]
mod controls_tests {
    use super::*;
    use crate::platform::InputSnapshot;

    fn snap(buttons: [bool; 6]) -> InputSnapshot {
        InputSnapshot {
            buttons,
            ..Default::default()
        }
    }

    #[test]
    fn hold_o_and_x_returns_to_picker_after_one_second() {
        let mut c = Controls::default();
        let held = snap([false, false, false, false, true, true]); // O + X
        for _ in 0..59 {
            assert!(matches!(c.update(&held, 60), ControlAction::None));
        }
        assert!(matches!(c.update(&held, 60), ControlAction::BackToPicker));
    }

    #[test]
    fn releasing_o_or_x_resets_the_combo() {
        let mut c = Controls::default();
        let both = snap([false, false, false, false, true, true]);
        let one = snap([false, false, false, false, true, false]);
        for _ in 0..30 {
            c.update(&both, 60);
        }
        c.update(&one, 60); // release X
        for _ in 0..59 {
            assert!(matches!(c.update(&both, 60), ControlAction::None));
        }
        assert!(matches!(c.update(&both, 60), ControlAction::BackToPicker));
    }

    #[test]
    fn select_plus_start_quits() {
        let mut c = Controls::default();
        let s = InputSnapshot {
            select: true,
            start: true,
            ..Default::default()
        };
        assert!(matches!(c.update(&s, 60), ControlAction::Quit));
    }

    #[test]
    fn select_alone_returns_to_picker() {
        let mut c = Controls::default();
        let s = InputSnapshot {
            select: true,
            ..Default::default()
        };
        assert!(matches!(c.update(&s, 60), ControlAction::BackToPicker));
    }

    #[test]
    fn fps_toggle_is_an_edge() {
        let mut c = Controls::default();
        let on = InputSnapshot {
            fps_toggle: true,
            ..Default::default()
        };
        assert!(matches!(c.update(&on, 60), ControlAction::ToggleFps));
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use crate::platform::{null::NullPlatform, InputSnapshot};

    #[test]
    fn smoke_picker_presents_then_quits() {
        // Empty dir: picker_loop should present `smoke` frames and return None.
        let dir = std::env::temp_dir().join(format!("rico8_app_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(Box::new(NullPlatform::new()), AudioHandle::dummy(), Some(3));
        app.picker(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn quit_requested_leaves_picker_immediately() {
        let dir = std::env::temp_dir().join(format!("rico8_app2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = InputSnapshot {
            quit_requested: true,
            ..Default::default()
        };
        let platform = Box::new(NullPlatform::scripted(vec![snap]));
        let mut app = App::new(platform, AudioHandle::dummy(), None);
        app.picker(&dir).unwrap(); // returns without hanging
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A button still held when re-entering the picker (e.g. the in-game hold-O+X exit) must
    /// NOT be treated as a fresh press. The first frame establishes the baseline; held buttons
    /// only act on a subsequent rising edge.
    #[test]
    fn held_face_button_on_entry_does_not_launch() {
        let dir = std::env::temp_dir().join(format!("rico8_app3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Create a dummy cart so carts list is non-empty.
        std::fs::write(dir.join("test.png"), b"dummy").unwrap();

        // Button 4 (O) held on every frame — no release, no rising edge.
        let held = InputSnapshot {
            buttons: [false, false, false, false, true, false],
            ..Default::default()
        };
        let frames = vec![held, held, held];
        let platform = Box::new(NullPlatform::scripted(frames));
        // smoke=3 so the loop exits via the smoke limit, not a launch.
        let mut app = App::new(platform, AudioHandle::dummy(), Some(3));
        let carts = vec![dir.join("test.png")];
        // With the OLD code this returns Ok(Some(..)) on frame 0 (false positive launch).
        // With the fix it returns Ok(None) (exits via smoke, no cart launched).
        let result = app.picker_loop(&dir, &carts).unwrap();
        assert!(
            result.is_none(),
            "held button on entry must not launch a cart (got {result:?})"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A genuine button press that happens AFTER a full release cycle still launches.
    #[test]
    fn fresh_press_after_release_launches() {
        let dir = std::env::temp_dir().join(format!("rico8_app4_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.png"), b"dummy").unwrap();

        let held = InputSnapshot {
            buttons: [false, false, false, false, true, false],
            ..Default::default()
        };
        let released = InputSnapshot::default();
        // Sequence: held, held, released, held (fresh press).
        let frames = vec![held, held, released, held];
        let platform = Box::new(NullPlatform::scripted(frames));
        let mut app = App::new(platform, AudioHandle::dummy(), Some(10));
        let carts = vec![dir.join("test.png")];
        let result = app.picker_loop(&dir, &carts).unwrap();
        assert_eq!(
            result,
            Some(carts[0].clone()),
            "genuine press after release must launch the selected cart"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
