//! The console shell: boot screen, command prompt, mode switching, the
//! run loop, build orchestration and error screens. If RICO-8 has a
//! personality, it lives here.

use crate::{
    builder::{spawn_build, BuildJob},
    editor::{
        code::CodeEditor, map::MapEditor, music::MusicEditor, sfx::SfxEditor, sprite::SpriteEditor,
    },
    ui::{self, Mouse},
    watch::{FileChange, FileWatch, SourceTreeWatch},
};
use anyhow::{anyhow, bail, Result};
use rico8_runtime::{
    assets::Assets,
    audio::AudioHandle,
    cart::{self, Cart},
    fb::Framebuffer,
    font,
    palette::col,
    project::{decode_assets, encode_assets, Project},
    vm::{GameVm, RuntimeError, UI_FPS},
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Keys as the shell sees them, decoupled from the windowing library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Left,
    Right,
    Up,
    Down,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Home,
    End,
    PageUp,
    PageDown,
    /// F6: capture the screen as the cart label while running.
    CaptureLabel,
    /// F1: toggle the wall-clock fps meter.
    ToggleFps,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Console,
    Run,
    Code,
    Sprite,
    Map,
    Sfx,
    Music,
}

/// The editor tabs, in tab-bar order.
pub const EDITOR_MODES: [Mode; 5] = [Mode::Code, Mode::Sprite, Mode::Map, Mode::Sfx, Mode::Music];

/// What is currently loaded into the console.
enum Loaded {
    None,
    /// A project directory: full edit/build/run workflow.
    Project(Project),
    /// A PNG cart loaded directly: runs as-is; source (if any) is shown
    /// read-only until imported into a project with `import`.
    Cart {
        cart: Cart,
        path: PathBuf,
    },
}

fn assets_of(loaded: &mut Loaded) -> Option<&mut Assets> {
    match loaded {
        Loaded::None => None,
        Loaded::Project(p) => Some(&mut p.assets),
        Loaded::Cart { cart, .. } => Some(&mut cart.assets),
    }
}

fn assets_ref(loaded: &Loaded) -> Option<&Assets> {
    match loaded {
        Loaded::None => None,
        Loaded::Project(p) => Some(&p.assets),
        Loaded::Cart { cart, .. } => Some(&cart.assets),
    }
}

/// Disk watchers for the currently-loaded *project*: the two files rico8
/// mirrors in memory plus the crate's source tree for build triggering.
struct ProjectWatch {
    code: FileWatch,
    assets: FileWatch,
    source_tree: SourceTreeWatch,
}

impl ProjectWatch {
    fn new(p: &Project) -> Self {
        let assets_baseline = encode_assets(&p.assets).unwrap_or_default();
        Self {
            code: FileWatch::new(p.dir.join("src/lib.rs"), p.code.clone().into_bytes()),
            assets: FileWatch::new(p.dir.join("assets.rico8"), assets_baseline),
            source_tree: SourceTreeWatch::new(&p.dir),
        }
    }

    /// Re-baseline every watcher to the project's current in-memory state and
    /// the current source tree (after rico8 saved the files itself).
    fn sync(&mut self, p: &Project) {
        self.code.mark_synced(p.code.clone().into_bytes());
        self.assets
            .mark_synced(encode_assets(&p.assets).unwrap_or_default());
        self.source_tree.sync();
    }
}

/// Disk watcher for a loaded PNG cart: re-parses on external change and
/// reconciles its assets against any in-console edits.
struct CartWatch {
    path: PathBuf,
    synced_mtime: Option<SystemTime>,
    /// Encoded assets as of the last sync (the editable, comparable part).
    baseline: Vec<u8>,
}

impl CartWatch {
    fn new(path: PathBuf, baseline: Vec<u8>) -> Self {
        let synced_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Self {
            path,
            synced_mtime,
            baseline,
        }
    }

    /// Re-baseline after rico8 wrote the cart itself (save), so our own write
    /// is not seen as an external change.
    fn mark_synced(&mut self, baseline: Vec<u8>) {
        self.baseline = baseline;
        self.synced_mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
    }

    /// The new mtime if the file advanced past the last sync, else `None`.
    fn advanced(&mut self) -> Option<SystemTime> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok()?;
        let advanced = self.synced_mtime.map(|prev| mtime > prev).unwrap_or(true);
        if advanced {
            // Absorb the mtime now; a transiently-corrupt PNG mid-write is
            // ignored until the next write rather than retried every poll.
            self.synced_mtime = Some(mtime);
            Some(mtime)
        } else {
            None
        }
    }
}

/// Draw the fps meter in the top-left: measured frames per second over the
/// cart's target rate, e.g. `60/60`.
fn fps_overlay(fb: &mut Framebuffer, measured: f32, target: u32) {
    let text = format!("{}/{}", measured.round() as u32, target);
    let w = text.len() as i32 * 4 + 1;
    fb.rectfill(0, 0, w, 6, col::BLACK);
    fb.print(&text, 1, 1, col::YELLOW);
}

/// How long the F6 camera-flash overlay lasts, in frames (~0.1s at 60fps).
const CAPTURE_FLASH_FRAMES: u32 = 6;

/// Paint the camera-flash feedback over the running cart's screen: a bright
/// full-screen white pop, like a camera shutter. `cls` is used deliberately —
/// it ignores any camera offset or clip the cart left active, so the flash
/// always covers the whole screen and touches no cart-visible state.
fn capture_flash_overlay(fb: &mut Framebuffer) {
    fb.cls(col::WHITE);
}

enum ConsoleLine {
    Text {
        text: String,
        color: u8,
    },
    /// Decorative palette stripe shown at boot.
    Stripe,
}

pub struct Shell {
    pub mode: Mode,
    last_editor: Mode,
    loaded: Loaded,
    vm: Option<GameVm>,
    audio: AudioHandle,
    fb: Framebuffer,
    frame: u64,

    // Console state.
    lines: VecDeque<ConsoleLine>,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    scroll_back: usize,

    // Build state.
    build: Option<BuildJob>,
    run_after_build: bool,
    /// Transient feedback shown in the editor bottom bar:
    /// (text, color, frame it expires at).
    toast: Option<(String, u8, u64)>,

    // Hot reload.
    wasm_mtime: Option<SystemTime>,

    // Disk watching for external-edit live-reload.
    project_watch: Option<ProjectWatch>,
    cart_watch: Option<CartWatch>,

    // Mouse, shared with editors.
    pub mouse: Mouse,

    // Editors.
    code_ed: CodeEditor,
    sprite_ed: SpriteEditor,
    map_ed: MapEditor,
    sfx_ed: SfxEditor,
    music_ed: MusicEditor,

    pub want_exit: bool,
    /// Where `new` creates projects and `ls` looks: the host working dir.
    cwd: PathBuf,
    sdk_path: PathBuf,

    // Wall-clock fps meter (F1 toggles it), measured over a moving window.
    show_fps: bool,
    fps_frames: u32,
    fps_t0: Instant,
    fps_val: f32,

    /// Frames remaining of the camera-flash overlay shown after an F6 capture.
    capture_flash: u32,

    /// Display magnification applied to the framebuffers (1 = logical 128²).
    scale: i32,
}

const TEXT_COLS: usize = 31;
const PROMPT_COL: u8 = col::WHITE;

impl Shell {
    pub fn new(audio: AudioHandle, sdk_path: PathBuf) -> Self {
        let mut shell = Self {
            mode: Mode::Console,
            last_editor: Mode::Code,
            loaded: Loaded::None,
            vm: None,
            audio,
            fb: Framebuffer::new(),
            frame: 0,
            lines: VecDeque::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            scroll_back: 0,
            build: None,
            run_after_build: false,
            toast: None,
            wasm_mtime: None,
            project_watch: None,
            cart_watch: None,
            mouse: Mouse::default(),
            code_ed: CodeEditor::new(),
            sprite_ed: SpriteEditor::new(),
            map_ed: MapEditor::new(),
            sfx_ed: SfxEditor::new(),
            music_ed: MusicEditor::new(),
            want_exit: false,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            sdk_path,
            show_fps: false,
            fps_frames: 0,
            fps_t0: Instant::now(),
            fps_val: 0.0,
            capture_flash: 0,
            scale: 1,
        };
        shell.boot();
        shell
    }

    fn boot(&mut self) {
        self.lines.push_back(ConsoleLine::Stripe);
        self.say(&format!("RICO-8 {VERSION}"), col::WHITE);
        self.say("A fantasy console for Rust", col::LIGHT_GREY);
        self.say("", col::WHITE);
        self.say("Type help for help", col::LIGHT_GREY);
        self.say("", col::WHITE);
    }

    /// Print a (wrapped) line to the console.
    pub fn say(&mut self, text: &str, color: u8) {
        if text.is_empty() {
            self.push_line(String::new(), color);
            return;
        }
        for raw in text.split('\n') {
            let mut rest = raw;
            loop {
                let take = rest
                    .char_indices()
                    .nth(TEXT_COLS)
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                self.push_line(rest[..take].to_string(), color);
                rest = &rest[take..];
                if rest.is_empty() {
                    break;
                }
            }
        }
    }

    /// Flash a message in the editor bottom bar for `secs` seconds.
    fn toast(&mut self, text: &str, color: u8, secs: f32) {
        self.toast = Some((text.to_string(), color, self.frame + (secs * 30.0) as u64));
    }

    fn push_line(&mut self, text: String, color: u8) {
        self.lines.push_back(ConsoleLine::Text { text, color });
        while self.lines.len() > 300 {
            self.lines.pop_front();
        }
        self.scroll_back = 0;
    }

    // -----------------------------------------------------------------
    // Loaded-state helpers
    // -----------------------------------------------------------------

    pub fn assets(&self) -> Option<&Assets> {
        match &self.loaded {
            Loaded::None => None,
            Loaded::Project(p) => Some(&p.assets),
            Loaded::Cart { cart, .. } => Some(&cart.assets),
        }
    }

    pub fn assets_mut(&mut self) -> Option<&mut Assets> {
        match &mut self.loaded {
            Loaded::None => None,
            Loaded::Project(p) => Some(&mut p.assets),
            Loaded::Cart { cart, .. } => Some(&mut cart.assets),
        }
    }

    fn code(&self) -> Option<&str> {
        match &self.loaded {
            Loaded::None => None,
            Loaded::Project(p) => Some(&p.code),
            Loaded::Cart { cart, .. } => cart.source.as_deref(),
        }
    }

    fn set_code(&mut self, code: String) {
        match &mut self.loaded {
            Loaded::None => {}
            Loaded::Project(p) => p.code = code,
            Loaded::Cart { cart, .. } => cart.source = Some(code),
        }
    }

    fn cart_name(&self) -> String {
        self.assets()
            .map(|a| a.meta.name.clone())
            .unwrap_or_else(|| "no cart".into())
    }

    // -----------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------

    /// Feed a game button (host already mapped keys to buttons 0..6).
    pub fn set_button(&mut self, b: usize, down: bool) {
        if let Some(vm) = &mut self.vm {
            vm.state_mut().input.set_button(b, down);
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods) {
        // Global shortcuts.
        if key == Key::ToggleFps {
            self.show_fps = !self.show_fps;
            return;
        }
        if mods.ctrl {
            match key {
                Key::Char('r') => {
                    self.cmd_run();
                    return;
                }
                Key::Char('s') => {
                    self.cmd_save_quiet();
                    return;
                }
                _ => {}
            }
        }

        match self.mode {
            Mode::Run => {
                if key == Key::Escape {
                    self.stop_run("");
                } else if key == Key::CaptureLabel {
                    self.capture_label();
                }
            }
            Mode::Console => self.console_key(key, mods),
            _ => self.editor_key(key, mods),
        }
    }

    fn editor_key(&mut self, key: Key, mods: Mods) {
        if key == Key::Escape {
            self.mode = Mode::Console;
            return;
        }
        // Alt+Left/Right cycles editor tabs.
        if mods.alt {
            let cur = EDITOR_MODES
                .iter()
                .position(|m| *m == self.mode)
                .unwrap_or(0);
            match key {
                Key::Left => {
                    self.switch_editor(
                        EDITOR_MODES[(cur + EDITOR_MODES.len() - 1) % EDITOR_MODES.len()],
                    );
                    return;
                }
                Key::Right => {
                    self.switch_editor(EDITOR_MODES[(cur + 1) % EDITOR_MODES.len()]);
                    return;
                }
                _ => {}
            }
        }
        if self.loaded_none() {
            return;
        }
        let audio = self.audio.clone();
        match self.mode {
            Mode::Code => {
                let mut code = self.code().unwrap_or_default().to_string();
                self.code_ed.key(key, mods, &mut code);
                self.set_code(code);
            }
            Mode::Sprite => {
                if let Some(a) = assets_of(&mut self.loaded) {
                    self.sprite_ed.key(key, mods, a);
                }
            }
            Mode::Map => {
                if let Some(a) = assets_of(&mut self.loaded) {
                    self.map_ed.key(key, mods, a);
                }
            }
            Mode::Sfx => {
                if let Some(a) = assets_of(&mut self.loaded) {
                    self.sfx_ed.key(key, mods, a, &audio);
                }
            }
            Mode::Music => {
                if let Some(a) = assets_of(&mut self.loaded) {
                    self.music_ed.key(key, mods, a, &audio);
                }
            }
            _ => {}
        }
    }

    pub fn switch_editor(&mut self, mode: Mode) {
        // Abandon any in-progress map-editor drag so it can't commit a stale
        // selection or move once the editor regains focus.
        self.map_ed.cancel_drag();
        if self.loaded_none() {
            self.say("No cart loaded. Try: new mygame", col::RED);
            self.mode = Mode::Console;
            return;
        }
        if mode == Mode::Code {
            let code = self.code().unwrap_or_default().to_string();
            self.code_ed.set_text(&code);
        }
        self.mode = mode;
        self.last_editor = mode;
    }

    fn loaded_none(&self) -> bool {
        matches!(self.loaded, Loaded::None)
    }

    fn console_key(&mut self, key: Key, _mods: Mods) {
        match key {
            Key::Char(c) => {
                self.input.insert(self.byte_at(self.cursor), c);
                self.cursor += 1;
            }
            Key::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let at = self.byte_at(self.cursor);
                    self.input.remove(at);
                }
            }
            Key::Delete => {
                if self.cursor < self.input.chars().count() {
                    let at = self.byte_at(self.cursor);
                    self.input.remove(at);
                }
            }
            Key::Left => self.cursor = self.cursor.saturating_sub(1),
            Key::Right => self.cursor = (self.cursor + 1).min(self.input.chars().count()),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = self.input.chars().count(),
            Key::Up => {
                if !self.history.is_empty() {
                    let pos = match self.history_pos {
                        None => self.history.len() - 1,
                        Some(p) => p.saturating_sub(1),
                    };
                    self.history_pos = Some(pos);
                    self.input = self.history[pos].clone();
                    self.cursor = self.input.chars().count();
                }
            }
            Key::Down => {
                if let Some(p) = self.history_pos {
                    if p + 1 < self.history.len() {
                        self.history_pos = Some(p + 1);
                        self.input = self.history[p + 1].clone();
                    } else {
                        self.history_pos = None;
                        self.input.clear();
                    }
                    self.cursor = self.input.chars().count();
                }
            }
            Key::PageUp => self.scroll_back = (self.scroll_back + 5).min(self.lines.len()),
            Key::PageDown => self.scroll_back = self.scroll_back.saturating_sub(5),
            Key::Enter => {
                let cmd = self.input.clone();
                self.say(&format!("> {cmd}"), col::LIGHT_GREY);
                if !cmd.trim().is_empty() {
                    self.history.push(cmd.clone());
                }
                self.history_pos = None;
                self.input.clear();
                self.cursor = 0;
                self.exec(&cmd);
            }
            Key::Escape => {
                if !self.loaded_none() {
                    self.switch_editor(self.last_editor);
                }
            }
            Key::Tab | Key::CaptureLabel | Key::ToggleFps => {}
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    // -----------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------

    fn exec(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let Some(&verb) = parts.first() else { return };
        let args = &parts[1..];
        let result = match verb.to_ascii_lowercase().as_str() {
            "help" => {
                self.cmd_help(args);
                Ok(())
            }
            "new" => self.cmd_new(args),
            "load" => self.cmd_load(args),
            "reload" => self.cmd_reload(),
            "save" => self.cmd_save(args),
            "run" => {
                self.cmd_run();
                Ok(())
            }
            "export" => self.cmd_export(args),
            "import" => self.cmd_import(args),
            "import-pico8" | "importp8" => self.cmd_import_pico8(args),
            "info" => {
                self.cmd_info();
                Ok(())
            }
            "ls" | "dir" => self.cmd_ls(),
            "cls" => {
                self.lines.clear();
                Ok(())
            }
            "title" => self.cmd_meta(args, |a, v| a.meta.name = v),
            "author" => self.cmd_meta(args, |a, v| a.meta.author = v),
            "code" => {
                self.switch_editor(Mode::Code);
                Ok(())
            }
            "sprite" | "gfx" => {
                self.switch_editor(Mode::Sprite);
                Ok(())
            }
            "map" => {
                self.switch_editor(Mode::Map);
                Ok(())
            }
            "sfx" => {
                self.switch_editor(Mode::Sfx);
                Ok(())
            }
            "music" => {
                self.switch_editor(Mode::Music);
                Ok(())
            }
            "keys" => {
                self.cmd_keys();
                Ok(())
            }
            "reboot" => {
                self.vm = None;
                self.audio.stop_all();
                self.loaded = Loaded::None;
                self.lines.clear();
                self.boot();
                Ok(())
            }
            "exit" | "quit" | "shutdown" => {
                self.want_exit = true;
                Ok(())
            }
            other => Err(anyhow!("Syntax error: {other}\nType help for help")),
        };
        if let Err(e) = result {
            self.say(&e.to_string(), col::RED);
        }
    }

    fn cmd_help(&mut self, args: &[&str]) {
        if args.first() == Some(&"keys") {
            self.cmd_keys();
            return;
        }
        for (c, d) in [
            ("new <name>", "Create a project"),
            ("load <dir|cart.png>", "Load a cart"),
            ("reload", "Re-read from disk, drop edits"),
            ("save", "Save project to disk"),
            ("run", "Build + run (esc stops)"),
            ("export <f.png|f.html>", "Export cart (PNG or web)"),
            ("import <f.png> <dir>", "Cart -> project"),
            ("import-pico8 <f> [dir]", "PICO-8 cart -> project"),
            ("info", "Cart metadata"),
            ("title/author <text>", "Set metadata"),
            ("code/sprite/map/sfx/music", "Editors (esc)"),
            ("ls, cls, keys, reboot, exit", ""),
        ] {
            self.say(c, col::WHITE);
            if !d.is_empty() {
                self.say(&format!("  {d}"), col::LIGHT_GREY);
            }
        }
    }

    fn cmd_keys(&mut self) {
        for (k, d) in [
            ("esc", "Console <-> editor / stop"),
            ("ctrl+r", "Run cart"),
            ("ctrl+s", "Save + build check"),
            ("alt+left/right", "Switch editor"),
            ("arrows + z/x", "Game buttons"),
            ("f1", "Toggle FPS meter"),
            ("f6", "Capture label (running)"),
        ] {
            self.say(&format!("{k:14} {d}"), col::LIGHT_GREY);
        }
    }

    fn cmd_new(&mut self, args: &[&str]) -> Result<()> {
        let Some(name) = args.first() else {
            bail!("Usage: new <name>");
        };
        let dir = self.cwd.join(name);
        let project = Project::create(&dir, name, &self.sdk_path)?;
        self.say(&format!("Created ./{name}"), col::GREEN);
        self.code_ed.set_text(&project.code);
        self.project_watch = Some(ProjectWatch::new(&project));
        self.cart_watch = None;
        self.loaded = Loaded::Project(project);
        Ok(())
    }

    /// Load a cart/project given on the command line at boot.
    pub fn startup_load(&mut self, path: &str) {
        if let Err(e) = self.cmd_load(&[path]) {
            self.say(&e.to_string(), col::RED);
        }
    }

    fn cmd_load(&mut self, args: &[&str]) -> Result<()> {
        let Some(path) = args.first() else {
            bail!("Usage: load <dir|cart.png>");
        };
        let path = self.cwd.join(path);
        if path.extension().is_some_and(|e| e == "png") {
            let cart = cart::load_png(&path)?;
            let name = cart.assets.meta.name.clone();
            let has_src = cart.source.is_some();
            self.code_ed.set_text(
                cart.source
                    .as_deref()
                    .unwrap_or("// No source in this cart"),
            );
            self.project_watch = None;
            let cart_baseline = encode_assets(&cart.assets).unwrap_or_default();
            self.cart_watch = Some(CartWatch::new(path.clone(), cart_baseline));
            self.loaded = Loaded::Cart { cart, path };
            self.say(&format!("Loaded cart: {name}"), col::GREEN);
            if !has_src {
                self.say("(Playable cart, no source)", col::LIGHT_GREY);
            }
        } else {
            let project = Project::load(&path)?;
            self.code_ed.set_text(&project.code);
            self.say(&format!("Loaded {}", project.name), col::GREEN);
            self.project_watch = Some(ProjectWatch::new(&project));
            self.cart_watch = None;
            self.loaded = Loaded::Project(project);
        }
        Ok(())
    }

    /// Re-read the current project/cart from disk, discarding in-console edits.
    /// Resolves a conflict in favour of the external version.
    fn cmd_reload(&mut self) -> Result<()> {
        let path = match &self.loaded {
            Loaded::None => bail!("Nothing loaded"),
            Loaded::Project(p) => p.dir.clone(),
            Loaded::Cart { path, .. } => path.clone(),
        };
        let path_str = path.to_string_lossy().into_owned();
        self.cmd_load(&[&path_str])
    }

    fn cmd_save(&mut self, _args: &[&str]) -> Result<()> {
        let message = match &mut self.loaded {
            Loaded::None => bail!("Nothing to save"),
            Loaded::Project(p) => {
                p.save()?;
                "Saved".to_string()
            }
            Loaded::Cart { cart, path } => {
                cart::save_png(cart, path)?;
                format!("Saved {}", path.display())
            }
        };
        // After saving a project, rico8's own write must not look external.
        if let (Loaded::Project(p), Some(w)) = (&self.loaded, &mut self.project_watch) {
            w.sync(p);
        }
        // After saving a PNG cart, rico8's own write must not look external.
        if let (Loaded::Cart { cart, .. }, Some(w)) = (&self.loaded, &mut self.cart_watch) {
            w.mark_synced(encode_assets(&cart.assets).unwrap_or_default());
        }
        self.say(&message, col::GREEN);
        Ok(())
    }

    /// Ctrl+S: save, flash feedback where the user is looking, and (for
    /// projects) start a background build so compile errors show up
    /// while editing instead of at `run` time.
    fn cmd_save_quiet(&mut self) {
        if let Err(e) = self.cmd_save(&[]) {
            let msg = e.to_string();
            self.say(&msg, col::RED);
            self.toast(&msg, col::RED, 3.0);
            return;
        }
        self.toast("Saved", col::GREEN, 1.5);
        if self.build.is_none() {
            if let Loaded::Project(p) = &self.loaded {
                let dir = p.dir.clone();
                self.build = Some(spawn_build(&dir));
                self.run_after_build = false;
            }
        }
    }

    pub fn cmd_run(&mut self) {
        self.audio.stop_all();
        self.vm = None;
        match &self.loaded {
            Loaded::None => self.say("No cart loaded", col::RED),
            Loaded::Cart { .. } => match self.start_vm_from_loaded() {
                Ok(()) => {}
                Err(e) => self.show_error("boot", &e.to_string()),
            },
            Loaded::Project(p) => {
                let dir = p.dir.clone();
                if self.build.is_some() {
                    self.say("Already compiling...", col::ORANGE);
                    self.toast("Already building...", col::ORANGE, 1.5);
                    return;
                }
                // Pick up external edits and flush in-console edits without
                // clobbering either. Abort the run on an unresolved conflict.
                if !self.reconcile_for_build() {
                    self.say("Disk & editor both changed", col::ORANGE);
                    self.say("Save or reload to resolve", col::ORANGE);
                    self.toast("Conflict: save or reload", col::ORANGE, 3.0);
                    return;
                }
                self.mode = Mode::Console;
                self.say("Compiling...", col::LIGHT_GREY);
                self.build = Some(spawn_build(&dir));
                self.run_after_build = true;
            }
        }
    }

    /// Reconcile a project's disk and in-memory copies in preparation for a
    /// build. Adopts clean external changes, flushes in-console edits to disk,
    /// and returns `false` (build should abort) on an unresolved conflict.
    fn reconcile_for_build(&mut self) -> bool {
        let Loaded::Project(_) = &self.loaded else {
            return true;
        };
        // Snapshot the in-memory bytes to feed the watchers.
        let (code_mem, assets_mem) = match &self.loaded {
            Loaded::Project(p) => (
                p.code.clone().into_bytes(),
                encode_assets(&p.assets).unwrap_or_default(),
            ),
            _ => return true,
        };
        let Some(w) = &mut self.project_watch else {
            return true;
        };
        // Adopt external changes first (clean memory), or bail on conflict.
        let code_change = w.code.poll(&code_mem);
        let assets_change = w.assets.poll(&assets_mem);
        // Absorb the source-tree high-water mark so the flush below + the build
        // it triggers are not re-detected as an external change next poll.
        w.source_tree.poll();
        // Bail on a conflict — including a *standing* one from an earlier poll.
        // A later poll returns `None` once the mtime is absorbed, so the latch
        // is what keeps `run` from flushing the stale copy over disk until the
        // user resolves it with `save` (keep mine) or `reload` (take disk).
        if matches!(code_change, FileChange::Conflict)
            || matches!(assets_change, FileChange::Conflict)
            || w.code.in_conflict()
            || w.assets.in_conflict()
        {
            return false;
        }
        if let FileChange::Adopt(bytes) = code_change {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            self.code_ed.set_text(&text);
            if let Loaded::Project(p) = &mut self.loaded {
                p.code = text;
            }
        }
        if let FileChange::Adopt(bytes) = assets_change {
            match decode_assets(&bytes) {
                Ok(assets) => {
                    if let Loaded::Project(p) = &mut self.loaded {
                        p.assets = assets;
                    }
                }
                // Malformed disk file: re-sync the watcher to the current
                // in-memory encoding so we do not flush over it.
                Err(_) => self.resync_assets_watcher(),
            }
        }
        // Flush any in-console edits that are not yet on disk, so cargo builds
        // exactly what the editors show. (No-op when nothing is dirty.)
        let needs_flush = match (&self.loaded, &self.project_watch) {
            (Loaded::Project(p), Some(w)) => {
                p.code.as_bytes() != w.code.baseline()
                    || encode_assets(&p.assets).unwrap_or_default() != w.assets.baseline()
            }
            _ => false,
        };
        if needs_flush {
            if let Loaded::Project(p) = &self.loaded {
                let _ = p.save();
            }
            if let (Loaded::Project(p), Some(w)) = (&self.loaded, &mut self.project_watch) {
                w.sync(p);
            }
        }
        true
    }

    /// Re-baseline the assets watcher to the current in-memory encoding. Used
    /// when an external `assets.rico8` is unreadable, so we neither flush over
    /// it nor keep re-detecting it.
    fn resync_assets_watcher(&mut self) {
        if let (Loaded::Project(p), Some(w)) = (&self.loaded, &mut self.project_watch) {
            w.assets
                .mark_synced(encode_assets(&p.assets).unwrap_or_default());
        }
    }

    fn start_vm_from_loaded(&mut self) -> Result<()> {
        let (wasm, assets) = match &self.loaded {
            Loaded::None => bail!("No cart loaded"),
            Loaded::Cart { cart, .. } => (cart.wasm.clone(), cart.assets.clone()),
            Loaded::Project(p) => {
                let path = p.wasm_path();
                let wasm = std::fs::read(&path)
                    .map_err(|_| anyhow!("Cart not built yet ({})", path.display()))?;
                self.wasm_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                (wasm, p.assets.clone())
            }
        };
        let vm = GameVm::load(&wasm, &assets, self.audio.clone())?;
        self.vm = Some(vm);
        self.mode = Mode::Run;
        Ok(())
    }

    fn cmd_export(&mut self, args: &[&str]) -> Result<()> {
        let mut include_source = true;
        let mut file = None;
        for a in args {
            if *a == "-nosrc" {
                include_source = false;
            } else {
                file = Some(*a);
            }
        }
        let file = file
            .map(|f| f.to_string())
            .unwrap_or_else(|| format!("{}.png", self.cart_name()));
        let out = self.cwd.join(&file);
        if file.ends_with(".html") {
            // Web export: one self-contained playable page.
            let cart = self.make_cart(false)?;
            self.say("Exporting for web...", col::LIGHT_GREY);
            let web_dir = crate::webexport::web_crate_dir(&self.sdk_path);
            crate::webexport::export_html(&cart, &out, &web_dir)?;
        } else {
            let cart = self.make_cart(include_source)?;
            cart::save_png(&cart, &out)?;
        }
        self.say(&format!("Exported {file}"), col::GREEN);
        Ok(())
    }

    fn make_cart(&self, include_source: bool) -> Result<Cart> {
        match &self.loaded {
            Loaded::None => bail!("No cart loaded"),
            Loaded::Cart { cart, .. } => Ok(Cart {
                wasm: cart.wasm.clone(),
                assets: cart.assets.clone(),
                source: if include_source {
                    cart.source.clone()
                } else {
                    None
                },
            }),
            Loaded::Project(p) => {
                let wasm = std::fs::read(p.wasm_path())
                    .map_err(|_| anyhow!("Cart not built yet. Type run first"))?;
                Ok(Cart {
                    wasm,
                    assets: p.assets.clone(),
                    source: include_source.then(|| p.code.clone()),
                })
            }
        }
    }

    fn cmd_import(&mut self, args: &[&str]) -> Result<()> {
        let (Some(png), Some(dir)) = (args.first(), args.get(1)) else {
            bail!("Usage: import <cart.png> <dir>");
        };
        let cart = cart::load_png(&self.cwd.join(png))?;
        let Some(source) = &cart.source else {
            bail!("Cart has no source (playable-only)");
        };
        let dir = self.cwd.join(dir);
        let mut project = Project::create(&dir, &cart.assets.meta.name, &self.sdk_path)?;
        project.code = source.clone();
        project.assets = cart.assets.clone();
        project.save()?;
        self.say(&format!("Imported into {}", dir.display()), col::GREEN);
        self.code_ed.set_text(&project.code);
        self.loaded = Loaded::Project(project);
        if let Loaded::Project(p) = &self.loaded {
            self.project_watch = Some(ProjectWatch::new(p));
        }
        self.cart_watch = None;
        Ok(())
    }

    /// Import a PICO-8 cart's assets into a fresh project. Only the graphics,
    /// map, sound and music transfer; the cart's Lua code is ignored.
    fn cmd_import_pico8(&mut self, args: &[&str]) -> Result<()> {
        let Some(src) = args.first() else {
            bail!("Usage: import-pico8 <cart.p8|cart.p8.png> [dir]");
        };
        let src = self.cwd.join(src);
        // The destination defaults to the cart's name when omitted.
        let dir = match args.get(1) {
            Some(d) => self.cwd.join(d),
            None => self.cwd.join(rico8_runtime::pico8::default_dir_name(&src)),
        };
        let project = rico8_runtime::pico8::import_project(&src, &dir, &self.sdk_path)?;
        self.say(
            &format!("Imported assets into {}", dir.display()),
            col::GREEN,
        );
        self.code_ed.set_text(&project.code);
        self.loaded = Loaded::Project(project);
        if let Loaded::Project(p) = &self.loaded {
            self.project_watch = Some(ProjectWatch::new(p));
        }
        self.cart_watch = None;
        Ok(())
    }

    fn cmd_info(&mut self) {
        match self.assets() {
            None => self.say("No cart loaded", col::RED),
            Some(a) => {
                let (name, author, version) = (
                    a.meta.name.clone(),
                    a.meta.author.clone(),
                    a.meta.version.clone(),
                );
                let label = if a.label.is_some() {
                    "Captured"
                } else {
                    "Default"
                };
                let kind = match &self.loaded {
                    Loaded::Project(p) => format!("Project {}", p.dir.display()),
                    Loaded::Cart { path, .. } => format!("Cart {}", path.display()),
                    Loaded::None => unreachable!(),
                };
                self.say(&format!("Title:   {name}"), col::WHITE);
                self.say(&format!("Author:  {author}"), col::WHITE);
                self.say(&format!("Version: {version}"), col::WHITE);
                self.say(&format!("Label:   {label}"), col::LIGHT_GREY);
                self.say(&kind, col::LIGHT_GREY);
            }
        }
    }

    fn cmd_ls(&mut self) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(&self.cwd)?
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (name, is_dir)
            })
            .filter(|(n, _)| !n.starts_with('.'))
            .collect();
        entries.sort();
        for (name, is_dir) in entries.into_iter().take(40) {
            if is_dir {
                self.say(&format!("{name}/"), col::BLUE);
            } else if name.ends_with(".png") {
                self.say(&name, col::PINK);
            } else {
                self.say(&name, col::LIGHT_GREY);
            }
        }
        Ok(())
    }

    fn cmd_meta(&mut self, args: &[&str], set: impl FnOnce(&mut Assets, String)) -> Result<()> {
        if args.is_empty() {
            bail!("Missing text");
        }
        let value = args.join(" ");
        match self.assets_mut() {
            None => bail!("No cart loaded"),
            Some(a) => {
                set(a, value);
                self.say("OK", col::GREEN);
                Ok(())
            }
        }
    }

    fn capture_label(&mut self) {
        let Some(vm) = &self.vm else { return };
        // Use logical_pixels so the label is always 128² regardless of scale.
        let pixels = vm.state().fb.logical_pixels();
        if let Some(a) = self.assets_mut() {
            a.label = Some(pixels);
        }
        // A brief on-screen camera flash, plus a console line for the record.
        self.capture_flash = CAPTURE_FLASH_FRAMES;
        self.say("Label captured", col::GREEN);
    }

    fn stop_run(&mut self, message: &str) {
        self.vm = None;
        self.audio.stop_all();
        self.mode = Mode::Console;
        if !message.is_empty() {
            self.say(message, col::RED);
        }
    }

    fn show_error(&mut self, phase: &str, message: &str) {
        self.stop_run("");
        self.say("", col::WHITE);
        self.say(&format!("** Error in {phase} **"), col::RED);
        for line in message.lines().take(12) {
            self.say(line, col::ORANGE);
        }
    }

    fn runtime_error(&mut self, e: RuntimeError) {
        self.show_error(e.phase, &e.message);
    }

    /// Set the display magnification. The scale is applied lazily to each
    /// framebuffer before the next draw. The logical 128² coordinate space
    /// for carts and the UI is unchanged.
    pub fn set_scale(&mut self, scale: i32) {
        self.scale = scale.max(1);
    }

    // -----------------------------------------------------------------
    // Per-frame logic
    // -----------------------------------------------------------------

    /// The rate the host should tick at: a running cart's frame rate (30 or
    /// 60), else 30. Running the whole Run-mode tick at the cart's rate is
    /// what gets the display to refresh at 60 too.
    pub fn tick_fps(&self) -> u32 {
        match (self.mode, &self.vm) {
            (Mode::Run, Some(vm)) => vm.fps(),
            _ => UI_FPS,
        }
    }

    pub fn tick(&mut self) {
        self.frame += 1;

        // Poll the background build.
        if let Some(job) = &self.build {
            if let Some(result) = job.poll() {
                self.build = None;
                if result.success {
                    let msg = format!("Build ok ({:.1}s)", result.duration.as_secs_f32());
                    self.say(&msg, col::GREEN);
                    self.toast(&msg, col::GREEN, 2.0);
                    for w in &result.warnings {
                        self.say(w, col::ORANGE);
                    }
                    if self.run_after_build {
                        self.run_after_build = false;
                        if let Err(e) = self.start_vm_from_loaded() {
                            self.show_error("boot", &e.to_string());
                        }
                    }
                } else {
                    self.run_after_build = false;
                    let n = result
                        .errors
                        .iter()
                        .filter(|l| l.starts_with("error"))
                        .count();
                    self.toast(
                        &format!("Build failed ({n} errors) - press esc"),
                        col::RED,
                        5.0,
                    );
                    for line in &result.errors {
                        let color = if line.starts_with("error") {
                            col::RED
                        } else {
                            col::ORANGE
                        };
                        self.say(line, color);
                    }
                }
            }
        }

        self.poll_project_watch();
        self.poll_cart_watch();
        self.check_hot_reload();

        match self.mode {
            Mode::Run => {
                if self.vm.is_some() {
                    let (logs, result) = {
                        let vm = self.vm.as_mut().unwrap();
                        if vm.state().fb.scale() != self.scale {
                            vm.state_mut().fb.set_scale(self.scale);
                        }
                        let logs = std::mem::take(&mut vm.state_mut().logs);
                        let r = vm.call_update().and_then(|()| vm.call_draw());
                        (logs, r)
                    };
                    for l in logs {
                        self.say(&l, col::LIGHT_GREY);
                    }
                    if let Err(e) = result {
                        self.runtime_error(e);
                    }
                } else {
                    self.mode = Mode::Console;
                }
            }
            Mode::Console => {}
            _ => {
                // Tab bar clicks work in every editor.
                if let Some(target) = ui::tab_bar_click(&self.mouse) {
                    self.switch_editor(EDITOR_MODES[target]);
                }
                let mouse = self.mouse;
                let audio = self.audio.clone();
                match self.mode {
                    Mode::Code => {
                        if !self.loaded_none() {
                            let code = self.code().unwrap_or_default().to_string();
                            self.code_ed.tick(&mouse, &code);
                        }
                    }
                    Mode::Sprite => {
                        if let Some(a) = assets_of(&mut self.loaded) {
                            self.sprite_ed.tick(&mouse, a);
                        }
                    }
                    Mode::Map => {
                        if let Some(a) = assets_of(&mut self.loaded) {
                            self.map_ed.tick(&mouse, a);
                        }
                    }
                    Mode::Sfx => {
                        if let Some(a) = assets_of(&mut self.loaded) {
                            self.sfx_ed.tick(&mouse, a, &audio);
                        }
                    }
                    Mode::Music => {
                        if let Some(a) = assets_of(&mut self.loaded) {
                            self.music_ed.tick(&mouse, a, &audio);
                        }
                        // The pencil on a channel jumps to that SFX for editing.
                        if let Some(n) = self.music_ed.take_edit_request() {
                            self.sfx_ed.select(n);
                            self.switch_editor(Mode::Sfx);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.mouse.end_frame();
    }

    fn check_hot_reload(&mut self) {
        if !self.frame.is_multiple_of(30) {
            return;
        }
        let Loaded::Project(p) = &self.loaded else {
            return;
        };
        let Ok(meta) = std::fs::metadata(p.wasm_path()) else {
            return;
        };
        let Ok(mtime) = meta.modified() else {
            return;
        };
        match self.wasm_mtime {
            Some(prev) if mtime > prev => {
                self.wasm_mtime = Some(mtime);
                // Only swap the running VM; in other modes the fresh wasm is
                // simply ready for the next run.
                if self.mode == Mode::Run {
                    match self.start_vm_from_loaded() {
                        Ok(()) => self.say("Hot reloaded", col::GREEN),
                        Err(e) => self.show_error("reload", &e.to_string()),
                    }
                }
            }
            Some(_) => {}
            None => self.wasm_mtime = Some(mtime),
        }
    }

    /// Poll project watchers and react to external edits: adopt clean changes,
    /// warn on conflicts, and kick off a rebuild (code/source) or VM reload
    /// (assets). Runs on a 30-frame cadence; skipped while a build is in flight.
    fn poll_project_watch(&mut self) {
        if !self.frame.is_multiple_of(30) || self.build.is_some() {
            return;
        }
        let (code_mem, assets_mem) = match &self.loaded {
            Loaded::Project(p) => (
                p.code.clone().into_bytes(),
                encode_assets(&p.assets).unwrap_or_default(),
            ),
            _ => return,
        };
        let Some(w) = &mut self.project_watch else {
            return;
        };
        let assets_change = w.assets.poll(&assets_mem);
        let code_change = w.code.poll(&code_mem);
        let source_changed = w.source_tree.poll();
        let code_conflicted = w.code.in_conflict();

        // Assets: no rebuild needed; adopt and reload the running VM.
        match assets_change {
            FileChange::Adopt(bytes) => match decode_assets(&bytes) {
                Ok(assets) => {
                    if let Loaded::Project(p) = &mut self.loaded {
                        p.assets = assets;
                    }
                    self.say("Assets reloaded from disk", col::GREEN);
                    if self.mode == Mode::Run {
                        if let Err(e) = self.start_vm_from_loaded() {
                            self.show_error("reload", &e.to_string());
                        }
                    }
                }
                // Malformed disk file: re-sync so we do not loop on it.
                Err(_) => {
                    self.resync_assets_watcher();
                    self.say("assets.rico8 on disk is unreadable", col::ORANGE);
                }
            },
            FileChange::Conflict => {
                self.say("assets.rico8 changed on disk;", col::ORANGE);
                self.say("You have unsaved edits", col::ORANGE);
            }
            FileChange::None => {}
        }

        // Code: adopt into the editor; build is driven by source_changed below.
        match code_change {
            FileChange::Adopt(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.code_ed.set_text(&text);
                if let Loaded::Project(p) = &mut self.loaded {
                    p.code = text;
                }
            }
            FileChange::Conflict => {
                self.say("src/lib.rs changed on disk;", col::ORANGE);
                self.say("Save or reload to resolve", col::ORANGE);
            }
            FileChange::None => {}
        }

        // Any source change (lib.rs or another module) rebuilds — unless the
        // mirrored code is in an unresolved conflict (we must not build a state
        // the user has not chosen).
        if source_changed && !code_conflicted {
            let dir = match &self.loaded {
                Loaded::Project(p) => p.dir.clone(),
                _ => return,
            };
            self.say("Source changed, rebuilding...", col::LIGHT_GREY);
            self.toast("Rebuilding...", col::LIGHT_GREY, 1.5);
            self.build = Some(spawn_build(&dir));
            // Re-run from the console or while already running; stay put if the
            // user is in an editor.
            self.run_after_build = matches!(self.mode, Mode::Run | Mode::Console);
        }
    }

    /// Poll a loaded PNG cart's file: on external change re-parse and adopt it
    /// (when there are no in-console asset edits), else warn about a conflict.
    fn poll_cart_watch(&mut self) {
        if !self.frame.is_multiple_of(30) {
            return;
        }
        let (path, in_memory, baseline) = match (&self.loaded, &mut self.cart_watch) {
            (Loaded::Cart { cart, .. }, Some(w)) => {
                if w.advanced().is_none() {
                    return;
                }
                (
                    w.path.clone(),
                    encode_assets(&cart.assets).unwrap_or_default(),
                    w.baseline.clone(),
                )
            }
            _ => return,
        };
        let new_cart = match cart::load_png(&path) {
            Ok(c) => c,
            Err(e) => {
                self.show_error("reload", &e.to_string());
                return;
            }
        };
        let disk = encode_assets(&new_cart.assets).unwrap_or_default();
        match crate::watch::reconcile(&baseline, &disk, &in_memory) {
            crate::watch::Reconcile::Unchanged => {}
            crate::watch::Reconcile::Adopt(_) => {
                self.code_ed.set_text(
                    new_cart
                        .source
                        .as_deref()
                        .unwrap_or("// No source in this cart"),
                );
                if let Some(w) = &mut self.cart_watch {
                    w.baseline = disk;
                }
                self.loaded = Loaded::Cart {
                    cart: new_cart,
                    path,
                };
                self.say("Cart reloaded from disk", col::GREEN);
                if self.mode == Mode::Run {
                    if let Err(e) = self.start_vm_from_loaded() {
                        self.show_error("reload", &e.to_string());
                    }
                }
            }
            crate::watch::Reconcile::Conflict => {
                // No latch: each new external write re-warns. The `reload`
                // command (next task) takes the disk version to resolve this.
                self.say("Cart changed on disk;", col::ORANGE);
                self.say("You have unsaved edits", col::ORANGE);
            }
        }
    }

    // -----------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------

    /// Draw the current mode and return the framebuffer to present.
    /// Count presented frames over ~0.5 s windows for the fps meter. The
    /// cart can't measure this itself — `time()` is a logical clock — so the
    /// host counts real draws against the wall clock.
    fn meter_fps(&mut self) {
        self.fps_frames += 1;
        let elapsed = self.fps_t0.elapsed();
        if elapsed >= Duration::from_millis(500) {
            self.fps_val = self.fps_frames as f32 / elapsed.as_secs_f32();
            self.fps_frames = 0;
            self.fps_t0 = Instant::now();
        }
    }

    pub fn draw(&mut self) -> &Framebuffer {
        self.meter_fps();
        // Keep the shell framebuffer's scale in sync before drawing into it.
        if self.fb.scale() != self.scale {
            self.fb.set_scale(self.scale);
        }
        match self.mode {
            Mode::Run => {
                if self.show_fps {
                    if let Some(vm) = self.vm.as_mut() {
                        let target = vm.fps();
                        fps_overlay(&mut vm.state_mut().fb, self.fps_val, target);
                    }
                }
                if self.capture_flash > 0 {
                    if let Some(vm) = self.vm.as_mut() {
                        capture_flash_overlay(&mut vm.state_mut().fb);
                    }
                    self.capture_flash -= 1;
                }
                if let Some(vm) = &self.vm {
                    return &vm.state().fb;
                }
                &self.fb
            }
            Mode::Console => {
                self.draw_console();
                &self.fb
            }
            _ => {
                self.fb.reset_state();
                self.fb.cls(col::DARK_GREY);
                let mouse = self.mouse;
                match self.mode {
                    Mode::Code => {
                        let code = self.code().unwrap_or_default().to_string();
                        self.code_ed.draw(&mut self.fb, &code);
                    }
                    Mode::Sprite => {
                        if let Some(a) = assets_ref(&self.loaded) {
                            self.sprite_ed.draw(&mut self.fb, a);
                        }
                    }
                    Mode::Map => {
                        if let Some(a) = assets_ref(&self.loaded) {
                            self.map_ed.draw(&mut self.fb, a);
                        }
                    }
                    Mode::Sfx => {
                        if let Some(a) = assets_ref(&self.loaded) {
                            self.sfx_ed.draw(&mut self.fb, a, &self.audio);
                        }
                    }
                    Mode::Music => {
                        if let Some(a) = assets_ref(&self.loaded) {
                            self.music_ed.draw(&mut self.fb, a, &self.audio);
                        }
                    }
                    _ => {}
                }
                ui::draw_tab_bar(&mut self.fb, self.mode);
                // The audio editors show PICO-8's pitch/tracker mode buttons in
                // the top-left (the music editor's are decorative).
                match self.mode {
                    Mode::Music => ui::mode_buttons(&mut self.fb, true),
                    Mode::Sfx => ui::mode_buttons(&mut self.fb, self.sfx_ed.is_pitch()),
                    _ => {}
                }
                self.draw_toast();
                ui::draw_cursor(&mut self.fb, &mouse);
                &self.fb
            }
        }
    }

    /// Bottom-bar feedback in editor modes: a live "building..." while a
    /// build runs, otherwise the most recent toast until it expires.
    fn draw_toast(&mut self) {
        let msg = if self.build.is_some() {
            let dots = ".".repeat(1 + (self.frame as usize / 10) % 3);
            Some((format!("Building{dots}"), col::ORANGE))
        } else {
            match &self.toast {
                Some((text, color, expires)) if self.frame < *expires => {
                    Some((text.clone(), *color))
                }
                _ => {
                    self.toast = None;
                    None
                }
            }
        };
        if let Some((text, color)) = msg {
            self.fb.rectfill(0, 120, 127, 127, col::BLACK);
            self.fb.print(&text, 2, 121, color);
        }
    }

    fn draw_console(&mut self) {
        self.fb.reset_state();
        self.fb.cls(col::BLACK);
        // Lines fit between the top margin and the bottom status line, scaled
        // to the font's line height.
        let rows = ((120 - 2) / font::GLYPH_H) as usize;

        // Gather visible lines: history tail + prompt line.
        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_back);
        let start = end.saturating_sub(rows);
        let mut y = 2;
        for line in self.lines.iter().skip(start).take(end - start) {
            match line {
                ConsoleLine::Text { text, color } => {
                    self.fb.print(text, 2, y, *color);
                }
                ConsoleLine::Stripe => {
                    for (i, c) in [8u8, 9, 10, 11, 12, 13, 14, 15].iter().enumerate() {
                        self.fb
                            .rectfill(2 + i as i32 * 6, y, 2 + i as i32 * 6 + 4, y + 3, *c);
                    }
                }
            }
            y += font::GLYPH_H;
        }

        // Prompt with blinking cursor (skipped while compiling).
        if self.build.is_some() {
            let dots = ".".repeat(1 + (self.frame as usize / 10) % 3);
            self.fb
                .print(&format!("Compiling{dots}"), 2, y, col::ORANGE);
            return;
        }
        let prompt = format!("> {}", self.input);
        self.fb.print(&prompt, 2, y, PROMPT_COL);
        if (self.frame / 8).is_multiple_of(2) {
            let cx = 2 + (2 + self.cursor as i32) * 4;
            self.fb
                .rectfill(cx, y, cx + 3, y + font::GLYPH_H - 2, col::RED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_shell() -> Shell {
        let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rico8");
        Shell::new(AudioHandle::dummy(), sdk)
    }

    /// Ctrl+S in an editor saves, flashes feedback, kicks off a real
    /// background build, and reports the result in the bottom bar.
    #[test]
    fn ctrl_s_saves_and_builds_with_feedback() {
        let dir = std::env::temp_dir().join(format!("rico8_shell_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        Project::create(&project_dir, "game", &shell.sdk_path).unwrap();

        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load project");
        shell.switch_editor(Mode::Code);

        // Add a comment line at the top (keeping the code valid), then Ctrl+S.
        for c in "//x".chars() {
            shell.key(Key::Char(c), Mods::default());
        }
        shell.key(Key::Enter, Mods::default());
        shell.key(
            Key::Char('s'),
            Mods {
                ctrl: true,
                ..Default::default()
            },
        );

        // Saved to disk, toast shown, build started.
        let code = std::fs::read_to_string(project_dir.join("src/lib.rs")).unwrap();
        assert!(code.starts_with("//x\n"), "edit was saved");
        assert_eq!(shell.toast.as_ref().unwrap().0, "Saved");
        assert!(shell.build.is_some(), "background build spawned");

        // While building, the editor bottom bar shows progress.
        shell.draw();
        assert_eq!(shell.fb.pget(0, 120), col::BLACK, "toast bar drawn");

        // Wait for the real cargo build (template code must compile).
        for _ in 0..(120 * 30) {
            shell.tick();
            if shell.build.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        assert!(shell.build.is_none(), "build finished in time");
        let (text, color, _) = shell.toast.as_ref().unwrap();
        assert!(text.starts_with("Build ok"), "got: {text}");
        assert_eq!(*color, col::GREEN);
        assert!(
            shell.mode == Mode::Code,
            "stays in the editor; no mode switch"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A broken cart reports a failing build without leaving the editor.
    #[test]
    fn save_build_failure_is_reported() {
        let dir = std::env::temp_dir().join(format!("rico8_shell_fail_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        let mut project = Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        project.code = "fn broken( {".into();
        project.save().unwrap();

        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load project");
        shell.switch_editor(Mode::Code);
        shell.key(
            Key::Char('s'),
            Mods {
                ctrl: true,
                ..Default::default()
            },
        );
        assert!(shell.build.is_some());
        for _ in 0..(120 * 30) {
            shell.tick();
            if shell.build.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        let (text, color, _) = shell.toast.as_ref().unwrap();
        assert!(text.starts_with("Build failed"), "got: {text}");
        assert_eq!(*color, col::RED);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `run` after an external edit (clean in-console state) builds the
    /// external version and does NOT overwrite it with the stale in-memory copy.
    #[test]
    fn run_does_not_clobber_external_edits() {
        let dir = std::env::temp_dir().join(format!("rico8_run_noclobber_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load");

        // Simulate an external editor changing src/lib.rs to a still-valid file.
        let lib = project_dir.join("src/lib.rs");
        let original = std::fs::read_to_string(&lib).unwrap();
        let edited = format!("// EXTERNAL EDIT\n{original}");
        // Bump mtime so the watcher sees it as newer than load time.
        std::fs::write(&lib, &edited).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&lib)
            .unwrap()
            .set_modified(later)
            .unwrap();

        shell.cmd_run();

        // The on-disk file must still contain the external edit — not be
        // reverted to the stale in-memory copy.
        let after = std::fs::read_to_string(&lib).unwrap();
        assert!(
            after.starts_with("// EXTERNAL EDIT\n"),
            "external edit survived run; got:\n{after}"
        );

        // Let the build finish so we leave no thread dangling.
        for _ in 0..(120 * 30) {
            shell.tick();
            if shell.build.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `new` must arm the disk watcher, otherwise in-console edits are dropped
    /// before the build (reconcile_for_build skips the flush when unwatched).
    #[test]
    fn new_arms_the_project_watcher() {
        let dir = std::env::temp_dir().join(format!("rico8_new_watch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        shell.cwd = dir.clone();
        shell.cmd_new(&["game"]).expect("new");
        assert!(shell.project_watch.is_some(), "new should arm the watcher");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Editing project source externally while idle at the console triggers an
    /// automatic build and starts the cart running.
    #[test]
    fn external_edit_auto_builds_and_runs() {
        let dir = std::env::temp_dir().join(format!("rico8_autobuild_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load");
        assert_eq!(shell.mode, Mode::Console);

        // External edit, mtime bumped so the watcher sees it.
        let lib = project_dir.join("src/lib.rs");
        let original = std::fs::read_to_string(&lib).unwrap();
        std::fs::write(&lib, format!("// auto\n{original}")).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&lib)
            .unwrap()
            .set_modified(later)
            .unwrap();

        // Drive ticks: poll fires on a 30-frame cadence, then the build runs.
        let mut entered_run = false;
        for _ in 0..(180 * 30) {
            shell.tick();
            if shell.mode == Mode::Run {
                entered_run = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        assert!(entered_run, "external edit should auto-build and run");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Re-exporting a loaded PNG cart on disk reloads it (assets adopted) when
    /// there are no in-console edits.
    #[test]
    fn external_png_change_reloads_cart() {
        use rico8_runtime::cart::{self, Cart};
        let dir = std::env::temp_dir().join(format!("rico8_pngreload_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut shell = test_shell();

        let project_dir = dir.join("game");
        let project = Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        let png = dir.join("game.png");

        // A valid cart needs the 4-byte wasm magic (plus version); the codec
        // checks for it on save and load. The VM never runs here.
        let mut cart = Cart {
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            assets: project.assets.clone(),
            source: Some("// v1".into()),
        };
        cart::save_png(&cart, &png).unwrap();
        shell.cmd_load(&[png.to_str().unwrap()]).expect("load png");
        let before = shell.cart_name();

        // Re-export with a different cart name, bump mtime.
        cart.assets.meta.name = "renamed".into();
        cart::save_png(&cart, &png).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&png)
            .unwrap()
            .set_modified(later)
            .unwrap();

        for _ in 0..(60 * 30) {
            shell.tick();
            if shell.cart_name() != before {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        assert_eq!(shell.cart_name(), "renamed", "external PNG change adopted");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Saving a PNG cart re-baselines its watcher, so the next poll does not
    /// mistake rico8's own write for an external change (no false conflict).
    #[test]
    fn saving_png_does_not_self_conflict() {
        use rico8_runtime::{
            cart::{self, Cart},
            project::encode_assets,
        };
        let dir = std::env::temp_dir().join(format!("rico8_pngsave_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut shell = test_shell();
        let project = Project::create(&dir.join("game"), "game", &shell.sdk_path).unwrap();
        let png = dir.join("game.png");
        let cart = Cart {
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            assets: project.assets.clone(),
            source: Some("// v1".into()),
        };
        cart::save_png(&cart, &png).unwrap();
        shell.cmd_load(&[png.to_str().unwrap()]).expect("load png");

        // Edit the loaded cart's assets in-console, then save.
        if let Some(a) = shell.assets_mut() {
            a.meta.name = "edited".into();
        }
        shell.cmd_save(&[]).expect("save");

        // The watcher baseline must now match the saved in-memory assets.
        let in_mem = encode_assets(shell.assets().unwrap()).unwrap_or_default();
        assert_eq!(
            shell.cart_watch.as_ref().unwrap().baseline,
            in_mem,
            "save re-baselined the cart watcher"
        );

        // Ticking must not flip into a conflict / reload state.
        for _ in 0..(2 * 30) {
            shell.tick();
        }
        assert_eq!(shell.cart_name(), "edited");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `reload` discards in-console edits and re-reads the project from disk,
    /// resolving a conflict in favour of the external version.
    #[test]
    fn reload_takes_disk_version() {
        let dir = std::env::temp_dir().join(format!("rico8_reload_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load");

        // External edit on disk.
        let lib = project_dir.join("src/lib.rs");
        std::fs::write(&lib, "// DISK VERSION\n").unwrap();

        shell.cmd_reload().expect("reload");
        let code = shell.code().unwrap_or_default().to_string();
        assert!(
            code.starts_with("// DISK VERSION"),
            "reload took disk; got:\n{code}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A conflict (both disk and editor changed) must abort `run` and keep
    /// aborting on a *second* `run` until resolved — never flushing the stale
    /// in-console copy over the external edit.
    #[test]
    fn run_aborts_on_conflict_and_does_not_clobber() {
        let dir = std::env::temp_dir().join(format!("rico8_run_conflict_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut shell = test_shell();
        let project_dir = dir.join("game");
        Project::create(&project_dir, "game", &shell.sdk_path).unwrap();
        shell
            .cmd_load(&[project_dir.to_str().unwrap()])
            .expect("load");

        // In-console edit: make the in-memory copy dirty.
        if let Loaded::Project(p) = &mut shell.loaded {
            p.code = "// IN-CONSOLE EDIT\n".into();
        }
        // External edit on disk, mtime bumped so the watcher sees it.
        let lib = project_dir.join("src/lib.rs");
        std::fs::write(&lib, "// EXTERNAL EDIT\n").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&lib)
            .unwrap()
            .set_modified(later)
            .unwrap();

        // First run: conflict → abort, no build, disk keeps the external edit.
        shell.cmd_run();
        assert_eq!(shell.mode, Mode::Console, "first run aborts on conflict");
        assert!(shell.build.is_none(), "no build started on conflict");
        assert_eq!(
            std::fs::read_to_string(&lib).unwrap(),
            "// EXTERNAL EDIT\n",
            "disk untouched after the first run"
        );

        // Second run without resolving: must STILL abort and STILL not clobber.
        shell.cmd_run();
        assert_eq!(shell.mode, Mode::Console, "second run still aborts");
        assert!(shell.build.is_none(), "second run starts no build");
        assert_eq!(
            std::fs::read_to_string(&lib).unwrap(),
            "// EXTERNAL EDIT\n",
            "disk still untouched after the second run"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
