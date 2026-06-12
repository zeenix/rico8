//! RICO-8: a PICO-8-like fantasy console for Rust games.
//!
//! `rico8` opens the console; `rico8 <dir|cart.png>` opens it with a cart
//! loaded. A few headless subcommands (`new`, `build`, `export`,
//! `extract`) support the external-editor workflow and CI.

mod builder;
mod editor;
mod gpu;
mod shell;
mod ui;

use anyhow::{anyhow, bail, Context, Result};
use rico8_runtime::cart::{self, Cart};
use rico8_runtime::project::Project;
use shell::{Key, Mods, Shell};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// Where the `rico8` SDK crate lives, for generated project manifests.
/// The SDK is this very package, so default to our own source directory;
/// override with RICO8_SDK for installed binaries.
fn sdk_path() -> PathBuf {
    if let Ok(p) = std::env::var("RICO8_SDK") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
        ["verify", png] => headless_verify(Path::new(png)),
        ["snap", project, outdir] => headless_snap(Path::new(project), Path::new(outdir)),
        [] => run_windowed(None),
        [path] => run_windowed(Some(path.to_string())),
        _ => {
            print_help();
            bail!("unrecognized arguments: {args:?}");
        }
    }
}

fn print_help() {
    println!(
        "rico-8 {} - a fantasy console for rust\n\n\
         usage:\n\
         \x20 rico8                      boot the console\n\
         \x20 rico8 <dir|cart.png>       boot with a cart loaded\n\
         \x20 rico8 new <dir>            create a project (headless)\n\
         \x20 rico8 build <dir>          compile a project to wasm (headless)\n\
         \x20 rico8 export <dir> <out.png> [--no-source]\n\
         \x20                            build + export a png cart (headless)\n\
         \x20 rico8 extract <cart.png> <dir>\n\
         \x20                            turn an editable cart into a project\n\
         \x20 rico8 verify <cart.png>    load a cart and run 60 frames headless",
        shell::VERSION
    );
}

// ---------------------------------------------------------------------------
// Headless subcommands
// ---------------------------------------------------------------------------

fn headless_new(dir: &Path) -> Result<()> {
    let name = dir
        .file_name()
        .ok_or_else(|| anyhow!("bad directory name"))?
        .to_string_lossy()
        .into_owned();
    Project::create(dir, &name, &sdk_path())?;
    println!("created {}", dir.display());
    Ok(())
}

fn headless_build(dir: &Path) -> Result<()> {
    let project = Project::load(dir)?;
    let result = builder::run_build(dir, Instant::now());
    if !result.success {
        for line in &result.errors {
            eprintln!("{line}");
        }
        bail!("build failed");
    }
    println!(
        "built {} ({:.1}s)",
        project.wasm_path().display(),
        result.duration.as_secs_f32()
    );
    Ok(())
}

fn headless_export(dir: &Path, out: &Path, include_source: bool) -> Result<()> {
    let project = Project::load(dir)?;
    let result = builder::run_build(dir, Instant::now());
    if !result.success {
        for line in &result.errors {
            eprintln!("{line}");
        }
        bail!("build failed");
    }
    let wasm = std::fs::read(project.wasm_path()).context("reading built wasm")?;
    let cart = Cart {
        wasm,
        assets: project.assets.clone(),
        source: include_source.then(|| project.code.clone()),
    };
    cart::save_png(&cart, out)?;
    println!("exported {}", out.display());
    Ok(())
}

fn headless_extract(png: &Path, dir: &Path) -> Result<()> {
    let cart = cart::load_png(png)?;
    let source = cart
        .source
        .ok_or_else(|| anyhow!("cart has no embedded source (playable-only cart)"))?;
    let mut project = Project::create(dir, &cart.assets.meta.name, &sdk_path())?;
    project.code = source;
    project.assets = cart.assets;
    project.save()?;
    println!("extracted into {}", dir.display());
    Ok(())
}

/// Load a cart and run a second of frames without a window — a smoke
/// test for carts and for the console itself (used by CI).
fn headless_verify(png: &Path) -> Result<()> {
    use rico8_runtime::audio::AudioHandle;
    use rico8_runtime::vm::GameVm;
    let cart = cart::load_png(png)?;
    let mut vm = GameVm::load(&cart.wasm, &cart.assets, AudioHandle::dummy())
        .context("loading cart into the vm")?;
    for frame in 0..60 {
        vm.call_update()
            .and_then(|()| vm.call_draw())
            .map_err(|e| anyhow!("frame {frame}: {e}"))?;
    }
    let drew_something = vm.state().fb.pixels().iter().any(|&p| p != 0);
    println!(
        "ok: {} ran 60 frames{}",
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
    use rico8_runtime::audio::AudioHandle;
    use rico8_runtime::cart::encode_screen_png;
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
    println!("wrote screenshots to {}", outdir.display());
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
        next_tick: Instant::now(),
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
    next_tick: Instant,
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
                eprintln!("rico8: could not open a window: {e}");
                event_loop.exit();
                return;
            }
        };
        match gpu::Gpu::new(window.clone(), event_loop.owned_display_handle()) {
            Ok(g) => {
                self.gpu = Some(g);
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("rico8: graphics init failed: {e:#}");
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
                let shell = &mut self.shell;
                let fb = shell.draw();
                if let Some(g) = &mut self.gpu {
                    if let Err(e) = g.render(fb) {
                        eprintln!("rico8: render error: {e:#}");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut ticked = false;
        while Instant::now() >= self.next_tick {
            self.shell.tick();
            self.next_tick += FRAME;
            ticked = true;
            // Don't death-spiral after a long stall.
            if now > self.next_tick + FRAME * 10 {
                self.next_tick = now + FRAME;
            }
        }
        if self.shell.want_exit {
            event_loop.exit();
            return;
        }
        if ticked {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}
