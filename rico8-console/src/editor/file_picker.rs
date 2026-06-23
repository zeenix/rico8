//! A small centered overlay for choosing which source file to edit, or
//! creating a new one. It knows nothing about projects: it renders the list it
//! is handed and returns a [`PickerAction`] for the shell to carry out.

use crate::{
    shell::{Key, Mods},
    ui::Mouse,
};
use rico8_runtime::{
    fb::{Framebuffer, HEIGHT, WIDTH},
    font,
    palette::col,
    ui as rui,
};

/// What the user asked for when confirming in the picker.
pub enum PickerAction {
    /// Open this existing file.
    Switch(String),
    /// Create a new file with this (un-normalized) name.
    Create(String),
}

#[derive(PartialEq)]
enum Mode {
    List,
    NewFile,
}

/// Row height: the font line height plus one pixel of leading.
const LINE_H: i32 = font::GLYPH_H + 1;
/// Overlay width in pixels.
const BOX_W: i32 = 90;

pub struct FilePicker {
    open: bool,
    sel: usize,
    mode: Mode,
    input: String,
}

impl FilePicker {
    pub fn new() -> Self {
        Self {
            open: false,
            sel: 0,
            mode: Mode::List,
            input: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the picker, pre-selecting the most likely target: the previously
    /// edited file (alt-tab style), or the first file that isn't the current
    /// one, so confirming immediately switches away rather than to itself.
    pub fn open(&mut self, files: &[&str], current: &str, previous: Option<&str>) {
        self.open = true;
        self.mode = Mode::List;
        self.input.clear();
        self.sel = Self::initial_sel(files, current, previous);
    }

    fn initial_sel(files: &[&str], current: &str, previous: Option<&str>) -> usize {
        if let Some(prev) = previous {
            if prev != current {
                if let Some(i) = files.iter().position(|f| *f == prev) {
                    return i;
                }
            }
        }
        files.iter().position(|f| *f != current).unwrap_or(0)
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Handle a keypress. `files` is the current file list (the trailing
    /// "+ new file" entry is implicit).
    pub fn key(&mut self, key: Key, mods: Mods, files: &[&str]) -> Option<PickerAction> {
        match self.mode {
            Mode::List => {
                let n = files.len() + 1;
                match key {
                    Key::Up => {
                        self.sel = (self.sel + n - 1) % n;
                        None
                    }
                    Key::Down => {
                        self.sel = (self.sel + 1) % n;
                        None
                    }
                    Key::Escape => {
                        self.close();
                        None
                    }
                    Key::Enter => self.confirm_selection(files),
                    _ => None,
                }
            }
            Mode::NewFile => match key {
                Key::Escape => {
                    self.mode = Mode::List;
                    None
                }
                Key::Backspace => {
                    self.input.pop();
                    None
                }
                Key::Enter => {
                    let name = self.input.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }
                    self.close();
                    Some(PickerAction::Create(name))
                }
                Key::Char(c)
                    if !mods.ctrl && (c.is_ascii_alphanumeric() || c == '_' || c == '.') =>
                {
                    self.input.push(c);
                    None
                }
                _ => None,
            },
        }
    }

    /// Handle mouse input. Clicking outside the box closes it.
    pub fn tick(&mut self, mouse: &Mouse, files: &[&str]) -> Option<PickerAction> {
        if !mouse.left_pressed {
            return None;
        }
        let n = files.len() + 1;
        let (x0, y0, x1, y1) = Self::geom(n);
        if !mouse.over(x0, y0, x1, y1) {
            self.close();
            return None;
        }
        // Inside the box, row selection only applies in the list.
        if self.mode != Mode::List {
            return None;
        }
        let row = (mouse.y - (y0 + 2)) / LINE_H;
        if (0..n as i32).contains(&row) {
            self.sel = row as usize;
            return self.confirm_selection(files);
        }
        None
    }

    /// Confirm the highlighted row: switch to a file, or enter new-file mode.
    fn confirm_selection(&mut self, files: &[&str]) -> Option<PickerAction> {
        if self.sel == files.len() {
            self.mode = Mode::NewFile;
            self.input.clear();
            None
        } else {
            let name = files[self.sel].to_string();
            self.close();
            Some(PickerAction::Switch(name))
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, files: &[&str], current: &str) {
        let n = files.len() + 1;
        let (x0, y0, x1, y1) = Self::geom(n);
        rui::panel(fb, x0, y0, x1, y1, col::DARK_BLUE, col::LIGHT_GREY);

        if self.mode == Mode::NewFile {
            fb.print("new file:", x0 + 3, y0 + 3, col::LIGHT_GREY);
            fb.print(
                &format!("{}_", self.input),
                x0 + 3,
                y0 + 3 + LINE_H,
                col::WHITE,
            );
            return;
        }

        // Each file row, then the trailing synthetic "+ new file" entry.
        for i in 0..n {
            let y = y0 + 2 + i as i32 * LINE_H;
            let selected = i == self.sel;
            if selected {
                fb.rectfill(x0 + 1, y - 1, x1 - 1, y + LINE_H - 2, col::DARK_PURPLE);
            }
            let (text, color) = if i == files.len() {
                ("+ new file", if selected { col::WHITE } else { col::GREEN })
            } else {
                let name = files[i];
                let color = if selected {
                    col::WHITE
                } else if name == current {
                    col::PEACH
                } else {
                    col::LIGHT_GREY
                };
                (name, color)
            };
            fb.print(text, x0 + 3, y, color);
        }
    }

    /// Box rectangle (inclusive) for `n` rows, centered, clamped below the bar.
    fn geom(n: usize) -> (i32, i32, i32, i32) {
        let h = n as i32 * LINE_H + 4;
        let x0 = (WIDTH - BOX_W) / 2;
        let y0 = ((HEIGHT - h) / 2).max(9);
        (x0, y0, x0 + BOX_W - 1, y0 + h - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_and_enter_switches() {
        let files = ["lib.rs", "enemy.rs"];
        let mut p = FilePicker::new();
        p.open(&files, "", None);
        assert!(p.key(Key::Down, Mods::default(), &files).is_none());
        let action = p.key(Key::Enter, Mods::default(), &files);
        assert!(matches!(action, Some(PickerAction::Switch(ref s)) if s == "enemy.rs"));
        assert!(!p.is_open());
    }

    #[test]
    fn new_file_flow_emits_create() {
        let files = ["lib.rs"];
        let mut p = FilePicker::new();
        p.open(&files, "", None);
        // Index 1 is the "+ new file" entry.
        p.key(Key::Down, Mods::default(), &files);
        assert!(p.key(Key::Enter, Mods::default(), &files).is_none());
        for c in "enemy".chars() {
            p.key(Key::Char(c), Mods::default(), &files);
        }
        let action = p.key(Key::Enter, Mods::default(), &files);
        assert!(matches!(action, Some(PickerAction::Create(ref s)) if s == "enemy"));
        assert!(!p.is_open());
    }

    #[test]
    fn escape_backs_out_then_closes() {
        let files = ["lib.rs"];
        let mut p = FilePicker::new();
        p.open(&files, "", None);
        p.key(Key::Down, Mods::default(), &files);
        p.key(Key::Enter, Mods::default(), &files); // -> NewFile
        p.key(Key::Escape, Mods::default(), &files); // -> List
        assert!(p.is_open());
        p.key(Key::Escape, Mods::default(), &files); // -> closed
        assert!(!p.is_open());
    }

    #[test]
    fn click_outside_closes_in_new_file_mode() {
        let files = ["lib.rs"];
        let mut p = FilePicker::new();
        p.open(&files, "", None);
        p.key(Key::Down, Mods::default(), &files); // -> "+ new file"
        p.key(Key::Enter, Mods::default(), &files); // -> NewFile mode
                                                    // Default mouse sits off-screen (-16, -16), outside the overlay.
        let outside = Mouse {
            left_pressed: true,
            ..Default::default()
        };
        assert!(p.tick(&outside, &files).is_none());
        assert!(!p.is_open());
    }

    #[test]
    fn open_preselects_previous_else_first_other() {
        let files = ["lib.rs", "enemy.rs", "player.rs"];
        let mut p = FilePicker::new();
        // The previous file is pre-selected when present and not current.
        p.open(&files, "lib.rs", Some("player.rs"));
        let action = p.key(Key::Enter, Mods::default(), &files);
        assert!(matches!(action, Some(PickerAction::Switch(ref s)) if s == "player.rs"));
        // With no previous, the first file that isn't the current one.
        p.open(&files, "lib.rs", None);
        let action = p.key(Key::Enter, Mods::default(), &files);
        assert!(matches!(action, Some(PickerAction::Switch(ref s)) if s == "enemy.rs"));
        // A stale previous (not in the list) falls back to first-other.
        p.open(&files, "enemy.rs", Some("gone.rs"));
        let action = p.key(Key::Enter, Mods::default(), &files);
        assert!(matches!(action, Some(PickerAction::Switch(ref s)) if s == "lib.rs"));
    }
}
