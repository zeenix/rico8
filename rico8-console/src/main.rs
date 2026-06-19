//! RICO-8: a PICO-8-like fantasy console for Rust games.
//!
//! `rico8` opens the console; `rico8 <dir|cart.png>` opens it with a cart
//! loaded. A few headless subcommands (`new`, `build`, `export`,
//! `extract`, `import-pico8`) support the external-editor workflow and CI.

mod builder;
mod editor;
mod gpu;
mod shell;
mod ui;
mod watch;
mod webexport;

use anyhow::{anyhow, bail, Context, Result};
use rico8_runtime::{
    cart::{self, Cart},
    project::Project,
};
use shell::{Key, Mods, Shell};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

/// One tick's wall-clock budget at a given rate (30 normally, 60 while a
/// 60 fps cart runs).
fn frame_duration(fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000 / fps.max(1) as u64)
}

/// Where the `rico8` SDK crate lives, for generated project manifests.
/// Defaults to this source tree; override with RICO8_SDK for installs.
fn sdk_path() -> PathBuf {
    if let Ok(p) = std::env::var("RICO8_SDK") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../rico8")
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    match strs.as_slice() {
        ["help" | "--help" | "-h"] => {
            print_help();
            Ok(())
        }
        ["new", dir] => headless_new(Path::new(dir)),
        ["build", dir] => headless_build(Path::new(dir)),
        ["export", dir, out, rest @ ..] => headless_export(
            Path::new(dir),
            Path::new(out),
            !rest.contains(&"--no-source"),
        ),
        ["extract", png, dir] => headless_extract(Path::new(png), Path::new(dir)),
        ["import-pico8", src] => {
            let src = Path::new(src);
            let dir = rico8_runtime::pico8::default_dir_name(src);
            headless_import_pico8(src, Path::new(&dir))
        }
        ["import-pico8", src, dir] => headless_import_pico8(Path::new(src), Path::new(dir)),
        ["export-web", input, out] => headless_export_web(Path::new(input), Path::new(out)),
        ["verify", png] => headless_verify(Path::new(png)),
        ["snap", project, outdir] => headless_snap(Path::new(project), Path::new(outdir)),
        [] => run_windowed(None),
        [path] => run_windowed(Some(path.to_string())),
        _ => {
            print_help();
            bail!("Unrecognized arguments: {args:?}");
        }
    }
}

fn print_help() {
    println!(
        "RICO-8 {} - A fantasy console for Rust\n\n\
         Usage:\n\
         \x20 rico8                      Boot the console\n\
         \x20 rico8 <dir|cart.png>       Boot with a cart loaded\n\
         \x20 rico8 new <dir>            Create a project (headless)\n\
         \x20 rico8 build <dir>          Compile a project to wasm (headless)\n\
         \x20 rico8 export <dir> <out.png> [--no-source]\n\
         \x20                            Build + export a PNG cart (headless)\n\
         \x20 rico8 extract <cart.png> <dir>\n\
         \x20                            Turn an editable cart into a project\n\
         \x20 rico8 import-pico8 <cart.p8|.p8.png> [dir]\n\
         \x20                            Import a PICO-8 cart's assets into a project\n\
         \x20                            (dir defaults to the cart's name)\n\
         \x20 rico8 export-web <dir|cart.png> <out.html>\n\
         \x20                            Export a self-contained playable web page\n\
         \x20 rico8 verify <cart.png>    Load a cart and run 60 frames headless",
        shell::VERSION
    );
}

// ---------------------------------------------------------------------------
// Headless subcommands
// ---------------------------------------------------------------------------

fn headless_new(dir: &Path) -> Result<()> {
    let name = dir
        .file_name()
        .ok_or_else(|| anyhow!("Bad directory name"))?
        .to_string_lossy()
        .into_owned();
    Project::create(dir, &name, &sdk_path())?;
    println!("Created {}", dir.display());
    Ok(())
}

fn headless_build(dir: &Path) -> Result<()> {
    let project = Project::load(dir)?;
    let result = builder::run_build(dir, Instant::now());
    if !result.success {
        for line in &result.errors {
            eprintln!("{line}");
        }
        bail!("Build failed");
    }
    println!(
        "Built {} ({:.1}s)",
        project.wasm_path().display(),
        result.duration.as_secs_f32()
    );
    for line in &result.warnings {
        eprintln!("{line}");
    }
    Ok(())
}

fn headless_export(dir: &Path, out: &Path, include_source: bool) -> Result<()> {
    let project = Project::load(dir)?;
    let result = builder::run_build(dir, Instant::now());
    if !result.success {
        for line in &result.errors {
            eprintln!("{line}");
        }
        bail!("Build failed");
    }
    let wasm = std::fs::read(project.wasm_path()).context("Reading built wasm")?;
    let cart = Cart {
        wasm,
        assets: project.assets.clone(),
        source: include_source.then(|| project.code.clone()),
    };
    cart::save_png(&cart, out)?;
    println!("Exported {}", out.display());
    Ok(())
}

fn headless_extract(png: &Path, dir: &Path) -> Result<()> {
    let cart = cart::load_png(png)?;
    let source = cart
        .source
        .ok_or_else(|| anyhow!("Cart has no embedded source (playable-only cart)"))?;
    let mut project = Project::create(dir, &cart.assets.meta.name, &sdk_path())?;
    project.code = source;
    project.assets = cart.assets;
    project.save()?;
    println!("Extracted into {}", dir.display());
    Ok(())
}

/// Import a PICO-8 cart (`.p8` text or `.p8.png`) into a new project. Only
/// the assets — graphics, map, sound and music — transfer; the cart's Lua
/// code is ignored.
fn headless_import_pico8(src: &Path, dir: &Path) -> Result<()> {
    rico8_runtime::pico8::import_project(src, dir, &sdk_path())?;
    println!("Imported {} into {}", src.display(), dir.display());
    Ok(())
}

/// Export a project or cart as a single playable HTML file.
fn headless_export_web(input: &Path, out: &Path) -> Result<()> {
    let cart = if input.extension().is_some_and(|e| e == "png") {
        cart::load_png(input)?
    } else {
        let project = Project::load(input)?;
        let result = builder::run_build(input, Instant::now());
        if !result.success {
            for line in &result.errors {
                eprintln!("{line}");
            }
            bail!("Build failed");
        }
        let wasm = std::fs::read(project.wasm_path()).context("Reading built wasm")?;
        Cart {
            wasm,
            assets: project.assets.clone(),
            // Web players can't edit; keep the page lean.
            source: None,
        }
    };
    webexport::export_html(&cart, out, &webexport::web_crate_dir(&sdk_path()))?;
    println!("Exported {}", out.display());
    Ok(())
}

/// Load a cart and run a second of frames without a window — a smoke
/// test for carts and for the console itself (used by CI).
fn headless_verify(png: &Path) -> Result<()> {
    use rico8_runtime::{audio::AudioHandle, vm::GameVm};
    let cart = cart::load_png(png)?;
    let mut vm = GameVm::load(&cart.wasm, &cart.assets, AudioHandle::dummy())
        .context("Loading cart into the VM")?;
    for frame in 0..60 {
        vm.call_update()
            .and_then(|()| vm.call_draw())
            .map_err(|e| anyhow!("Frame {frame}: {e}"))?;
    }
    let drew_something = vm.state().fb.pixels().iter().any(|&p| p != 0);
    println!(
        "OK: {} ran 60 frames{}",
        cart.assets.meta.name,
        if drew_something {
            ""
        } else {
            " (blank screen)"
        }
    );
    Ok(())
}

/// Render the console and each editor headless and save screenshots.
/// Undocumented helper for docs and visual checks.
fn headless_snap(project: &Path, outdir: &Path) -> Result<()> {
    use rico8_runtime::{audio::AudioHandle, cart::encode_screen_png};
    std::fs::create_dir_all(outdir)?;
    let mut shell = Shell::new(AudioHandle::dummy(), sdk_path());
    shell.startup_load(&project.to_string_lossy());
    let shots = [
        (shell::Mode::Console, "console"),
        (shell::Mode::Code, "code"),
        (shell::Mode::Sprite, "sprite"),
        (shell::Mode::Map, "map"),
        (shell::Mode::Sfx, "sfx"),
        (shell::Mode::Music, "music"),
    ];
    for (mode, name) in shots {
        if mode == shell::Mode::Console {
            shell.mode = mode;
        } else {
            shell.switch_editor(mode);
        }
        for _ in 0..3 {
            shell.tick();
        }
        let png = encode_screen_png(shell.draw(), 3);
        std::fs::write(outdir.join(format!("{name}.png")), png)?;
    }
    println!("Wrote screenshots to {}", outdir.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Windowed console
// ---------------------------------------------------------------------------

fn run_windowed(load: Option<String>) -> Result<()> {
    #[cfg(feature = "audio")]
    let audio_out = rico8_runtime::audio::AudioOutput::start();
    #[cfg(feature = "audio")]
    let audio = audio_out
        .as_ref()
        .map(|a| a.handle())
        .unwrap_or_else(rico8_runtime::audio::AudioHandle::dummy);
    #[cfg(not(feature = "audio"))]
    let audio = rico8_runtime::audio::AudioHandle::dummy();

    let mut shell = Shell::new(audio, sdk_path());
    if let Some(path) = load {
        shell.startup_load(&path);
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
    let mut app = App {
        window: None,
        gpu: None,
        shell,
        mods: Mods::default(),
        last_tick: Instant::now(),
        vsync_paced: true,
        #[cfg(feature = "audio")]
        _audio_out: audio_out,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    shell: Shell,
    mods: Mods,
    /// When the last logical frame was ticked. Updates are phase-locked to the
    /// vsync'd redraw instead of a free-running wall clock, so a 60 fps cart on
    /// a ~60 Hz panel advances exactly once per refresh (no beating/judder).
    last_tick: Instant,
    /// Whether the last present blocked on vblank. When it did, the panel paces
    /// the loop; when it didn't (window occluded/minimized), we fall back to a
    /// wall-clock cap so the loop never busy-spins.
    vsync_paced: bool,
    #[cfg(feature = "audio")]
    _audio_out: Option<rico8_runtime::audio::AudioOutput>,
}

impl App {
    /// Map physical keys to the six game buttons (active in run mode).
    fn game_button(code: KeyCode) -> Option<usize> {
        Some(match code {
            KeyCode::ArrowLeft => 0,
            KeyCode::ArrowRight => 1,
            KeyCode::ArrowUp => 2,
            KeyCode::ArrowDown => 3,
            KeyCode::KeyZ | KeyCode::KeyC | KeyCode::KeyN => 4,
            KeyCode::KeyX | KeyCode::KeyV | KeyCode::KeyM => 5,
            _ => return None,
        })
    }

    fn shell_key(logical: &winit::keyboard::Key) -> Option<Key> {
        use winit::keyboard::Key as WKey;
        Some(match logical {
            WKey::Named(NamedKey::ArrowLeft) => Key::Left,
            WKey::Named(NamedKey::ArrowRight) => Key::Right,
            WKey::Named(NamedKey::ArrowUp) => Key::Up,
            WKey::Named(NamedKey::ArrowDown) => Key::Down,
            WKey::Named(NamedKey::Backspace) => Key::Backspace,
            WKey::Named(NamedKey::Delete) => Key::Delete,
            WKey::Named(NamedKey::Enter) => Key::Enter,
            WKey::Named(NamedKey::Tab) => Key::Tab,
            WKey::Named(NamedKey::Escape) => Key::Escape,
            WKey::Named(NamedKey::Home) => Key::Home,
            WKey::Named(NamedKey::End) => Key::End,
            WKey::Named(NamedKey::PageUp) => Key::PageUp,
            WKey::Named(NamedKey::PageDown) => Key::PageDown,
            WKey::Named(NamedKey::Space) => Key::Char(' '),
            WKey::Named(NamedKey::F1) => Key::ToggleFps,
            WKey::Named(NamedKey::F6) => Key::CaptureLabel,
            WKey::Character(s) => Key::Char(s.chars().next()?),
            _ => return None,
        })
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("RICO-8")
            .with_inner_size(LogicalSize::new(512.0, 512.0))
            .with_min_inner_size(LogicalSize::new(128.0, 128.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("rico8: Could not open a window: {e}");
                event_loop.exit();
                return;
            }
        };
        match gpu::Gpu::new(window.clone(), event_loop.owned_display_handle()) {
            Ok(g) => {
                self.gpu = Some(g);
                // Kick the vsync-driven render loop; each frame re-arms it.
                window.request_redraw();
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("rico8: Graphics init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gpu {
                    g.resize(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.mods = Mods {
                    ctrl: s.control_key(),
                    shift: s.shift_key(),
                    alt: s.alt_key(),
                };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(b) = Self::game_button(code) {
                        self.shell
                            .set_button(b, event.state == ElementState::Pressed);
                    }
                }
                if event.state == ElementState::Pressed {
                    if let Some(key) = Self::shell_key(&event.logical_key) {
                        self.shell.key(key, self.mods);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(g) = &self.gpu {
                    let (x, y) = g.viewport().window_to_screen(position.x, position.y);
                    self.shell.mouse.x = x;
                    self.shell.mouse.y = y;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        if down {
                            self.shell.mouse.left_pressed = true;
                        }
                        self.shell.mouse.left = down;
                    }
                    MouseButton::Right => {
                        if down {
                            self.shell.mouse.right_pressed = true;
                        }
                        self.shell.mouse.right = down;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                // One logical tick per vsync'd frame. The 3/4-frame threshold
                // makes a 60 fps cart on a ~60 Hz panel advance exactly once
                // per refresh (phase-locked, no 60-vs-59.97 beating) while
                // capping logical speed on high-refresh panels.
                let now = Instant::now();
                let frame = frame_duration(self.shell.tick_fps());
                if now.duration_since(self.last_tick) >= frame * 3 / 4 {
                    self.shell.tick();
                    self.last_tick = now;
                }
                if self.shell.want_exit {
                    event_loop.exit();
                    return;
                }
                let fb = self.shell.draw();
                if let Some(g) = &mut self.gpu {
                    let present = Instant::now();
                    if let Err(e) = g.render(fb) {
                        eprintln!("rico8: Render error: {e:#}");
                    }
                    // A real Fifo present waits for vblank (milliseconds); an
                    // instant return means the surface isn't presenting (the
                    // window is occluded), so the loop must self-pace instead.
                    self.vsync_paced = present.elapsed() >= Duration::from_millis(3);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.shell.want_exit {
            event_loop.exit();
            return;
        }
        // Re-arm the redraw every iteration; the game advances in the
        // RedrawRequested handler, one frame per refresh.
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        // When the present blocks on vblank it paces the loop, so just wait for
        // the re-armed redraw. When it doesn't (occluded window), cap to the
        // frame interval so the loop never busy-spins.
        if self.vsync_paced {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            let frame = frame_duration(self.shell.tick_fps());
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + frame));
        }
    }
}
