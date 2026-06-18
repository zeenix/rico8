//! RICO-8 cart player for handhelds (and any SDL2 machine).
//!
//! A third frontend over `rico8-runtime`, sized for devices like the
//! PowKiddy RGB10S / Anbernic RG351 family running ArkOS or ROCKNIX:
//! SDL2 handles KMS/DRM video, the built-in gamepad and ALSA audio, and
//! this binary handles nothing else. Point it at a cart or a directory
//! of carts:
//!
//! ```text
//! rico8-player cart.png          play one cart
//! rico8-player /roms/rico8       cart picker over a directory
//! rico8-player                   picker over the current directory
//! ```
//!
//! Controls: d-pad moves, the two action buttons = O / X. On pads SDL
//! recognizes as GameControllers, Select returns to the picker and
//! Start+Select quits; on any pad, holding both action buttons for ~1s
//! returns to the picker. Keyboard works too (arrows + Z/X, Esc = back)
//! so the same binary doubles as a desktop cart player.

use anyhow::{anyhow, Context, Result};
use rico8_runtime::{
    audio::AudioHandle,
    cart,
    fb::{Framebuffer, HEIGHT, WIDTH},
    palette::col,
    ui,
    vm::{GameVm, UI_FPS},
};
use sdl2::{
    controller::{Button as CButton, GameController},
    event::Event,
    joystick::{HatState, Joystick},
    keyboard::Keycode,
    pixels::PixelFormatEnum,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SAMPLE_RATE: i32 = 44100;
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / UI_FPS as u64);

/// One frame's wall-clock budget at a given logical frame rate.
fn frame_duration(fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000 / fps.max(1) as u64)
}

/// Draw the fps meter in the top-left: measured frames per second over the
/// cart's target rate, e.g. `60/60`.
fn draw_fps_overlay(fb: &mut Framebuffer, measured: f32, target: u32) {
    let text = format!("{}/{}", measured.round() as u32, target);
    let w = text.len() as i32 * 4 + 1;
    fb.rectfill(0, 0, w, 6, col::BLACK);
    fb.print(&text, 1, 1, col::YELLOW);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--probe` runs the binary just far enough to prove it executes in
    // this environment (right CPU arch, loader and glibc all resolve)
    // without opening SDL. The launcher uses it to pick the binary that
    // actually runs, instead of guessing the arch from uname/loaders —
    // on a 64-bit kernel with a 32-bit ports runtime, an aarch64 binary
    // gets routed to qemu and fails, and this is how we find that out.
    if args.first().map(String::as_str) == Some("--probe") {
        println!("rico8-player ok arch={}", std::env::consts::ARCH);
        return Ok(());
    }

    // Hidden smoke-test mode for CI: run N frames headless and exit.
    // (SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy rico8-player --smoke 60 cart.png)
    let (smoke, args) = match args.split_first() {
        Some((flag, rest)) if flag == "--smoke" => {
            let (n, rest) = rest
                .split_first()
                .ok_or_else(|| anyhow!("--smoke <frames> <cart>"))?;
            (Some(n.parse::<u32>()?), rest.to_vec())
        }
        _ => (None, args),
    };
    let target = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut app = App::new(smoke).map_err(|e| anyhow!("sdl init failed: {e}"))?;
    if target.is_file() {
        app.play(&target)?;
    } else {
        app.picker(&target)?;
    }
    Ok(())
}

/// Mono audio callback: pulls samples straight from the shared synth.
struct SynthCallback(AudioHandle);

impl sdl2::audio::AudioCallback for SynthCallback {
    type Channel = f32;
    fn callback(&mut self, out: &mut [f32]) {
        // SDL calls this from a C thread; a panic unwinding across that
        // FFI boundary is undefined behavior and would abort the whole
        // process (looking like "the game failed to launch"). Contain
        // it and emit silence for this buffer instead.
        let handle = &self.0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle.with_synth(|s| {
                for sample in out.iter_mut() {
                    *sample = s.next_sample();
                }
            });
        }));
        if result.is_err() {
            for sample in out.iter_mut() {
                *sample = 0.0;
            }
        }
    }
}

/// What a finished game/picker loop wants to happen next.
enum Flow {
    Quit,
    BackToPicker,
}

struct App {
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    events: sdl2::EventPump,
    _audio_dev: Option<sdl2::audio::AudioDevice<SynthCallback>>,
    audio: AudioHandle,
    /// Opened controllers; kept alive so they keep sending events.
    _controllers: Vec<GameController>,
    /// Raw joysticks opened as a fallback for unmapped pads.
    _joysticks: Vec<Joystick>,
    /// Instance ids covered by the GameController API, whose duplicate
    /// raw joystick events we must ignore.
    gc_ids: HashSet<u32>,
    /// Raw-joystick button indices to treat as Select / Start, for pads
    /// SDL doesn't recognize as GameControllers (so it can't name them).
    /// Set via RICO8_SELECT / RICO8_START. None = unmapped.
    joy_select: Option<u8>,
    joy_start: Option<u8>,
    /// Run only this many frames, then exit (CI smoke mode).
    smoke: Option<u32>,
    /// Device-pixel scale used for cart framebuffers (see `player_scale`).
    scale: i32,
}

/// Device-pixel scale for the handheld canvas, capped to protect the RGB20S.
///
/// Reads `RICO8_MAX_SCALE` from the environment (default 3). Only the cheap
/// block-fill path scales, not per-pixel logic, so 2–3 is usually fine on
/// low-power handhelds. Scale 1 is always the minimum.
fn player_scale(canvas: &sdl2::render::WindowCanvas) -> i32 {
    let max = std::env::var("RICO8_MAX_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let (w, h) = canvas
        .output_size()
        .unwrap_or((WIDTH as u32, HEIGHT as u32));
    ((w.min(h) as i32) / WIDTH).clamp(1, max)
}

impl App {
    fn new(smoke: Option<u32>) -> std::result::Result<App, String> {
        let sdl = sdl2::init()?;
        let video = sdl.video()?;
        let window = video
            .window("RICO-8", 512, 512)
            .fullscreen_desktop()
            .position_centered()
            .build()
            .map_err(|e| e.to_string())?;
        // Prefer an accelerated vsynced renderer; fall back to whatever
        // SDL can give us (e.g. the software renderer on dummy video).
        let window2 = window.clone();
        let mut canvas = match window.into_canvas().accelerated().present_vsync().build() {
            Ok(c) => c,
            Err(_) => window2.into_canvas().build().map_err(|e| e.to_string())?,
        };
        // Square logical screen at the device scale; SDL letterboxes and
        // scales with the nearest-neighbor hint below. A scale > 1 gives
        // the cart framebuffer room for sub-pixel motion without zigzag.
        sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "0");
        let scale = player_scale(&canvas);
        canvas
            .set_logical_size((WIDTH * scale) as u32, (HEIGHT * scale) as u32)
            .ok();

        // Open every connected controller (the handheld's built-ins
        // enumerate as one). Extra mappings can be supplied for exotic
        // pads via a SDL_GameControllerDB-format file.
        let gc = sdl.game_controller()?;
        for path in [
            std::env::var("RICO8_GCDB").unwrap_or_default(),
            "gamecontrollerdb.txt".into(),
        ] {
            if !path.is_empty() && Path::new(&path).is_file() {
                let _ = gc.load_mappings(&path);
            }
        }
        let joystick = sdl.joystick()?;
        let mut controllers = Vec::new();
        let mut joysticks = Vec::new();
        let mut gc_ids = HashSet::new();
        for i in 0..gc.num_joysticks()? {
            if gc.is_game_controller(i) {
                if let Ok(c) = gc.open(i) {
                    gc_ids.insert(c.instance_id());
                    controllers.push(c);
                }
            } else if let Ok(j) = joystick.open(i) {
                // Unmapped pad: fall back to raw hat + button events.
                joysticks.push(j);
            }
        }
        // Tells us, in the log, which input path is active: recognized
        // GameControllers get named buttons (Select/Start); raw joysticks
        // only get the d-pad + face buttons plus the hold-to-exit combo.
        eprintln!(
            "rico8-player: {} controller(s), {} raw joystick(s)",
            controllers.len(),
            joysticks.len()
        );

        // Audio is best-effort: a device that won't open (or RICO8_NOAUDIO
        // set, as a diagnostic lever) just means a silent but fully
        // playable console. A larger buffer keeps these low-power devices
        // from underrunning.
        let audio = AudioHandle::dummy();
        let audio_dev = if std::env::var_os("RICO8_NOAUDIO").is_some() {
            None
        } else {
            sdl.audio()
                .ok()
                .and_then(|a| {
                    a.open_playback(
                        None,
                        &sdl2::audio::AudioSpecDesired {
                            freq: Some(SAMPLE_RATE),
                            channels: Some(1),
                            samples: Some(2048),
                        },
                        |_| SynthCallback(audio.clone()),
                    )
                    .ok()
                })
                .inspect(|dev| {
                    dev.resume();
                })
        };

        Ok(App {
            canvas,
            events: sdl.event_pump()?,
            _audio_dev: audio_dev,
            audio,
            _controllers: controllers,
            _joysticks: joysticks,
            gc_ids,
            joy_select: env_button("RICO8_SELECT"),
            joy_start: env_button("RICO8_START"),
            smoke,
            scale,
        })
    }

    /// Map a controller button to a console button (0..6).
    fn pad_button(b: CButton) -> Option<usize> {
        Some(match b {
            CButton::DPadLeft => 0,
            CButton::DPadRight => 1,
            CButton::DPadUp => 2,
            CButton::DPadDown => 3,
            CButton::A | CButton::Y => 4,
            CButton::B | CButton::X => 5,
            _ => return None,
        })
    }

    /// Raw-joystick fallback for pads without a controller mapping:
    /// Nintendo-style cross (0/3 = O, 1/2 = X).
    fn joy_button(b: u8) -> Option<usize> {
        Some(match b {
            0 | 3 => 4,
            1 | 2 => 5,
            _ => return None,
        })
    }

    fn key_button(k: Keycode) -> Option<usize> {
        Some(match k {
            Keycode::Left => 0,
            Keycode::Right => 1,
            Keycode::Up => 2,
            Keycode::Down => 3,
            Keycode::Z | Keycode::C | Keycode::N => 4,
            Keycode::X | Keycode::V | Keycode::M => 5,
            _ => return None,
        })
    }

    fn present(&mut self, fb: &Framebuffer, rgba: &mut Vec<u8>) -> Result<()> {
        let (dw, dh) = (fb.device_width() as u32, fb.device_height() as u32);
        rgba.resize((dw * dh * 4) as usize, 0);
        fb.write_rgba(rgba);
        let creator = self.canvas.texture_creator();
        let mut tex = creator
            .create_texture_streaming(PixelFormatEnum::ABGR8888, dw, dh)
            .map_err(|e| anyhow!("texture: {e}"))?;
        tex.update(None, rgba, (dw * 4) as usize)
            .map_err(|e| anyhow!("texture update: {e}"))?;
        self.canvas.set_draw_color(sdl2::pixels::Color::BLACK);
        self.canvas.clear();
        self.canvas
            .copy(&tex, None, None)
            .map_err(|e| anyhow!("blit: {e}"))?;
        self.canvas.present();
        Ok(())
    }

    /// Show a RICO-8 error screen until the player presses back. Used
    /// for load/boot/runtime failures so the device itself reports why,
    /// instead of silently bouncing to the picker.
    fn show_error(&mut self, message: &str) -> Result<Flow> {
        eprintln!("rico8-player: {}", message.replace('\n', ": "));
        let mut fb = ui::error_screen(message, self.scale);
        fb.print("select/b: back", 2, HEIGHT - 7, col::LIGHT_GREY);
        let mut rgba = Vec::new();
        self.audio.stop_all();
        let mut next = Instant::now();
        let mut shown = 0u32;
        loop {
            for event in self.events.poll_iter() {
                match event {
                    Event::Quit { .. } => return Ok(Flow::Quit),
                    Event::ControllerButtonDown {
                        button: CButton::Back | CButton::B | CButton::A | CButton::Start,
                        ..
                    } => return Ok(Flow::BackToPicker),
                    Event::JoyButtonDown { which, .. } if !self.gc_ids.contains(&which) => {
                        return Ok(Flow::BackToPicker)
                    }
                    Event::KeyDown { .. } => return Ok(Flow::BackToPicker),
                    _ => {}
                }
            }
            self.present(&fb, &mut rgba)?;
            shown += 1;
            if self.smoke.is_some_and(|n| shown >= n) {
                return Ok(Flow::Quit);
            }
            next += FRAME;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }

    /// Run one cart until the player backs out or quits.
    fn play(&mut self, path: &Path) -> Result<Flow> {
        eprintln!("rico8-player: loading {}", path.display());
        let cart = match cart::load_png(path) {
            Ok(c) => c,
            Err(e) => return self.show_error(&format!("load failed\n{e}")),
        };
        self.audio.stop_all();
        let mut vm = match GameVm::load(&cart.wasm, &cart.assets, self.audio.clone()) {
            Ok(vm) => Some(vm),
            Err(e) => return self.show_error(&format!("boot failed\n{e}")),
        };
        // Set the cart's framebuffer scale before the first frame so carts
        // benefit from sub-pixel motion immediately on load.
        if let Some(vm) = vm.as_mut() {
            vm.state_mut().fb.set_scale(self.scale);
        }
        // Carts run at their selected rate (30 or 60); the picker and error
        // screens stay at 30.
        let fps = vm.as_ref().map(GameVm::fps).unwrap_or(UI_FPS);
        let frame = frame_duration(fps);
        eprintln!("rico8-player: running {}", path.display());
        let mut error_fb: Option<Framebuffer> = None;
        let mut rgba = Vec::new();
        let mut next = Instant::now();
        let mut frames = 0u32;
        let mut select_held = false;
        let mut start_held = false;
        // Mirror of the six console buttons, kept regardless of whether
        // input arrives as GameController or raw-joystick events, so a
        // universal "hold both action buttons to exit" works even when
        // the pad isn't a recognized GameController (no Select/Start).
        let mut held = [false; 6];
        let mut combo_frames = 0u32;
        // Wall-clock fps meter (F1 toggles it). The cart can't measure this
        // itself — `time()` is a logical clock — so we count real presented
        // frames over a moving window here on the host.
        let mut show_fps = false;
        let mut fps_frames = 0u32;
        let mut fps_t0 = Instant::now();
        let mut fps_val = 0.0f32;

        loop {
            for event in self.events.poll_iter() {
                match event {
                    Event::Quit { .. } => return Ok(Flow::Quit),
                    Event::ControllerButtonDown { button, .. } => {
                        match button {
                            CButton::Back | CButton::Guide => select_held = true,
                            CButton::Start => start_held = true,
                            _ => {}
                        }
                        if select_held && start_held {
                            return Ok(Flow::Quit);
                        }
                        if let Some(b) = Self::pad_button(button) {
                            held[b] = true;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, true);
                            }
                        }
                    }
                    Event::ControllerButtonUp { button, .. } => {
                        match button {
                            CButton::Back | CButton::Guide if select_held => {
                                return Ok(Flow::BackToPicker)
                            }
                            CButton::Back | CButton::Guide => select_held = false,
                            CButton::Start => start_held = false,
                            _ => {}
                        }
                        if let Some(b) = Self::pad_button(button) {
                            held[b] = false;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, false);
                            }
                        }
                    }
                    Event::JoyButtonDown {
                        which, button_idx, ..
                    } if !self.gc_ids.contains(&which) => {
                        if Some(button_idx) == self.joy_select {
                            select_held = true;
                            if start_held {
                                return Ok(Flow::Quit);
                            }
                        } else if Some(button_idx) == self.joy_start {
                            start_held = true;
                            if select_held {
                                return Ok(Flow::Quit);
                            }
                        } else if let Some(b) = Self::joy_button(button_idx) {
                            held[b] = true;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, true);
                            }
                        }
                    }
                    Event::JoyButtonUp {
                        which, button_idx, ..
                    } if !self.gc_ids.contains(&which) => {
                        if Some(button_idx) == self.joy_select {
                            if select_held {
                                return Ok(Flow::BackToPicker);
                            }
                        } else if Some(button_idx) == self.joy_start {
                            start_held = false;
                        } else if let Some(b) = Self::joy_button(button_idx) {
                            held[b] = false;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, false);
                            }
                        }
                    }
                    Event::JoyHatMotion { which, state, .. } if !self.gc_ids.contains(&which) => {
                        let (l, r, u, d) = hat_dirs(state);
                        held[0] = l;
                        held[1] = r;
                        held[2] = u;
                        held[3] = d;
                        if let Some(vm) = vm.as_mut() {
                            let input = &mut vm.state_mut().input;
                            input.set_button(0, l);
                            input.set_button(1, r);
                            input.set_button(2, u);
                            input.set_button(3, d);
                        }
                    }
                    Event::KeyDown {
                        keycode: Some(k), ..
                    } => {
                        if k == Keycode::Escape {
                            return Ok(Flow::BackToPicker);
                        }
                        if k == Keycode::F1 {
                            show_fps = !show_fps;
                        }
                        if let Some(b) = Self::key_button(k) {
                            held[b] = true;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, true);
                            }
                        }
                    }
                    Event::KeyUp {
                        keycode: Some(k), ..
                    } => {
                        if let Some(b) = Self::key_button(k) {
                            held[b] = false;
                            if let Some(vm) = vm.as_mut() {
                                vm.state_mut().input.set_button(b, false);
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Universal escape: hold both action buttons (~1s) to return
            // to the picker. The named Select/Start route above only
            // works on recognized GameControllers; this works on any pad.
            if held[4] && held[5] {
                combo_frames += 1;
                if combo_frames >= fps {
                    return Ok(Flow::BackToPicker);
                }
            } else {
                combo_frames = 0;
            }

            if let Some(v) = vm.as_mut() {
                if let Err(e) = v.call_update().and_then(|()| v.call_draw()) {
                    eprintln!("rico8-player: runtime error: {e}");
                    let mut fb = ui::error_screen(&e.to_string(), self.scale);
                    fb.print("hold o+x to exit", 2, HEIGHT - 7, col::LIGHT_GREY);
                    error_fb = Some(fb);
                    self.audio.stop_all();
                    vm = None;
                }
            }
            if show_fps {
                if let Some(v) = vm.as_mut() {
                    draw_fps_overlay(&mut v.state_mut().fb, fps_val, fps);
                }
            }
            if let Some(v) = &vm {
                self.present(&v.state().fb, &mut rgba)?;
            } else if let Some(fb) = &error_fb {
                self.present(fb, &mut rgba)?;
            }

            frames += 1;
            if let Some(n) = self.smoke {
                if frames >= n {
                    return Ok(Flow::Quit);
                }
            }
            next += frame;
            let now = Instant::now();

            // Measure presented frames over ~0.5 s windows.
            fps_frames += 1;
            let elapsed = now.duration_since(fps_t0);
            if elapsed >= Duration::from_millis(500) {
                fps_val = fps_frames as f32 / elapsed.as_secs_f32();
                fps_frames = 0;
                fps_t0 = now;
            }
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }

    /// The cart shelf: list carts in a directory, pick one, play it.
    fn picker(&mut self, dir: &Path) -> Result<()> {
        loop {
            let carts = scan_carts(dir)?;
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

    fn picker_loop(&mut self, dir: &Path, carts: &[PathBuf]) -> Result<Option<PathBuf>> {
        let mut sel = 0usize;
        let mut frame = 0u32;
        let mut rgba = Vec::new();
        let mut next = Instant::now();
        loop {
            for event in self.events.poll_iter() {
                match event {
                    Event::Quit { .. } => return Ok(None),
                    Event::ControllerButtonDown { button, .. } => match button {
                        CButton::DPadUp => sel = sel.saturating_sub(1),
                        CButton::DPadDown => sel = (sel + 1).min(carts.len().saturating_sub(1)),
                        // Only Select leaves the picker. Any face button or
                        // Start launches -- which physical button reports as
                        // A vs B varies wildly across these handhelds, so
                        // accept them all rather than guess wrong.
                        CButton::Back => return Ok(None),
                        CButton::A | CButton::B | CButton::X | CButton::Y | CButton::Start => {
                            if let Some(p) = carts.get(sel) {
                                return Ok(Some(p.clone()));
                            }
                        }
                        _ => {}
                    },
                    Event::JoyHatMotion { which, state, .. } if !self.gc_ids.contains(&which) => {
                        let (_, _, u, d) = hat_dirs(state);
                        if u {
                            sel = sel.saturating_sub(1);
                        }
                        if d {
                            sel = (sel + 1).min(carts.len().saturating_sub(1));
                        }
                    }
                    Event::JoyButtonDown {
                        which, button_idx, ..
                    } if !self.gc_ids.contains(&which) => {
                        // Raw pad with no SDL mapping: the four face buttons
                        // are indices 0-3. Any of them launches; there's no
                        // reliable Select, so use the firmware's exit hotkey
                        // to leave the picker.
                        if button_idx <= 3 {
                            if let Some(p) = carts.get(sel) {
                                return Ok(Some(p.clone()));
                            }
                        }
                    }
                    Event::KeyDown {
                        keycode: Some(k), ..
                    } => match k {
                        Keycode::Up => sel = sel.saturating_sub(1),
                        Keycode::Down => sel = (sel + 1).min(carts.len().saturating_sub(1)),
                        Keycode::Escape => return Ok(None),
                        _ => {
                            if let Some(p) = carts.get(sel) {
                                return Ok(Some(p.clone()));
                            }
                        }
                    },
                    _ => {}
                }
            }

            let fb = draw_picker(dir, carts, sel, frame);
            self.present(&fb, &mut rgba)?;

            frame += 1;
            if let Some(n) = self.smoke {
                if frame >= n {
                    return Ok(None);
                }
            }
            next += FRAME;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }
}

/// Decompose an SDL hat state into (left, right, up, down).
/// Parse a raw-joystick button index from an environment variable.
fn env_button(var: &str) -> Option<u8> {
    std::env::var(var).ok()?.trim().parse().ok()
}

fn hat_dirs(state: HatState) -> (bool, bool, bool, bool) {
    use HatState::*;
    let l = matches!(state, Left | LeftUp | LeftDown);
    let r = matches!(state, Right | RightUp | RightDown);
    let u = matches!(state, Up | LeftUp | RightUp);
    let d = matches!(state, Down | LeftDown | RightDown);
    (l, r, u, d)
}

/// All RICO-8 carts in a directory, sorted by file name.
fn scan_carts(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut carts: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .filter(|p| {
            std::fs::read(p)
                .map(|bytes| cart::is_cart(&bytes))
                .unwrap_or(false)
        })
        .collect();
    carts.sort();
    Ok(carts)
}

/// Render the cart shelf in console style.
fn draw_picker(dir: &Path, carts: &[PathBuf], sel: usize, frame: u32) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.cls(col::BLACK);
    for (i, c) in [8u8, 9, 10, 11, 12, 13, 14, 15].iter().enumerate() {
        fb.rectfill(2 + i as i32 * 6, 2, 6 + i as i32 * 6, 5, *c);
    }
    fb.print("rico-8 carts", 2, 10, col::WHITE);

    if carts.is_empty() {
        fb.print("no carts found in", 2, 30, col::LIGHT_GREY);
        let dir = dir.to_string_lossy();
        let tail: String = dir
            .chars()
            .rev()
            .take(30)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .collect();
        fb.print(&tail, 2, 38, col::LIGHT_GREY);
        fb.print("copy .png carts here!", 2, 54, col::ORANGE);
        return fb;
    }

    // A window of entries around the selection (leaving room for the
    // controls footer).
    let rows = 13usize;
    let top = sel
        .saturating_sub(rows / 2)
        .min(carts.len().saturating_sub(rows));
    for (row, cart_path) in carts.iter().skip(top).take(rows).enumerate() {
        let i = top + row;
        let y = 22 + row as i32 * 7;
        let name = cart_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if i == sel {
            fb.rectfill(0, y - 1, 127, y + 5, col::DARK_BLUE);
            // Blinking chevron, console style.
            if (frame / 8).is_multiple_of(2) {
                fb.print(">", 2, y, col::RED);
            }
            fb.print(&name, 8, y, col::WHITE);
        } else {
            fb.print(&name, 8, y, col::LIGHT_GREY);
        }
    }
    // Controls, along the bottom of the shelf.
    fb.print("up/dn:pick   any btn:play", 2, 114, col::DARK_GREY);
    fb.print("in game: hold o+x to exit", 2, 121, col::DARK_GREY);
    fb
}

#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::{assets::Assets, cart::Cart};

    #[test]
    fn scan_finds_only_carts_sorted() {
        let dir = std::env::temp_dir().join(format!("rico8_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cart = Cart {
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            assets: Assets::default(),
            source: None,
        };
        let png = cart::encode(&cart).unwrap();
        std::fs::write(dir.join("b_game.png"), &png).unwrap();
        std::fs::write(dir.join("a_game.png"), &png).unwrap();
        // Decoys: a non-cart png and a text file.
        std::fs::write(dir.join("photo.png"), b"\x89PNG\r\n\x1a\nnotacart").unwrap();
        std::fs::write(dir.join("readme.txt"), b"hi").unwrap();

        let found = scan_carts(&dir).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a_game.png", "b_game.png"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn picker_draws_selection() {
        let carts = vec![PathBuf::from("one.png"), PathBuf::from("two.png")];
        let fb = draw_picker(Path::new("."), &carts, 1, 0);
        // Second row has the selection bar (dark blue).
        assert_eq!(fb.pget(0, 29), col::DARK_BLUE);
    }
}
