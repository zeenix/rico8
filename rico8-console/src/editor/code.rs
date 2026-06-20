//! The code editor: 31 columns of Rust in a 4x7 pixel font, with the
//! classic immediate cursor feel. Not an IDE — a place to type games.

use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{fb::Framebuffer, font, palette::col};

/// Visible text geometry. The row count is derived from the font's line height
/// so the text never overruns the status bar (the bottom 8 rows of the screen).
const AREA_X: i32 = 1;
const AREA_Y: i32 = 9;
const ROWS: usize = ((120 - AREA_Y) / font::GLYPH_H) as usize;
const COLS: usize = 31;

/// Syntax colors, chosen from the fixed palette.
const C_TEXT: u8 = col::WHITE;
const C_KEYWORD: u8 = col::PINK;
const C_STRING: u8 = col::GREEN;
const C_NUMBER: u8 = col::BLUE;
const C_COMMENT: u8 = col::LAVENDER;
const C_TYPE: u8 = col::YELLOW;
const C_MACRO: u8 = col::ORANGE;
const C_PUNCT: u8 = col::LIGHT_GREY;

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

pub struct CodeEditor {
    lines: Vec<String>,
    line: usize,
    col: usize,
    pref_col: usize,
    scroll_y: usize,
    scroll_x: usize,
    anchor: Option<(usize, usize)>,
    clipboard: String,
    undo: Vec<(Vec<String>, usize, usize)>,
    frame: u64,
}

impl CodeEditor {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            line: 0,
            col: 0,
            pref_col: 0,
            scroll_y: 0,
            scroll_x: 0,
            anchor: None,
            clipboard: String::new(),
            undo: Vec::new(),
            frame: 0,
        }
    }

    /// Load text, keeping the cursor when the text is unchanged (so
    /// switching tabs back and forth doesn't lose your place).
    pub fn set_text(&mut self, text: &str) {
        let new: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(str::to_string).collect()
        };
        if new != self.lines {
            self.lines = new;
            self.line = 0;
            self.col = 0;
            self.scroll_y = 0;
            self.scroll_x = 0;
            self.anchor = None;
            self.undo.clear();
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn clamp_cursor(&mut self) {
        self.line = self.line.min(self.lines.len() - 1);
        self.col = self.col.min(self.lines[self.line].chars().count());
    }

    fn push_undo(&mut self) {
        self.undo.push((self.lines.clone(), self.line, self.col));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    fn byte_idx(s: &str, char_idx: usize) -> usize {
        s.char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(s.len())
    }

    fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.anchor?;
        let b = (self.line, self.col);
        if a == b {
            return None;
        }
        Some(if a < b { (a, b) } else { (b, a) })
    }

    fn delete_selection(&mut self) -> bool {
        let Some(((l0, c0), (l1, c1))) = self.selection() else {
            return false;
        };
        let head = self.lines[l0][..Self::byte_idx(&self.lines[l0], c0)].to_string();
        let tail = self.lines[l1][Self::byte_idx(&self.lines[l1], c1)..].to_string();
        self.lines.splice(l0..=l1, [head + &tail]);
        self.line = l0;
        self.col = c0;
        self.anchor = None;
        true
    }

    fn selected_text(&self) -> String {
        let Some(((l0, c0), (l1, c1))) = self.selection() else {
            return String::new();
        };
        if l0 == l1 {
            let s = &self.lines[l0];
            return s[Self::byte_idx(s, c0)..Self::byte_idx(s, c1)].to_string();
        }
        let mut out = self.lines[l0][Self::byte_idx(&self.lines[l0], c0)..].to_string();
        for l in &self.lines[l0 + 1..l1] {
            out.push('\n');
            out.push_str(l);
        }
        out.push('\n');
        out.push_str(&self.lines[l1][..Self::byte_idx(&self.lines[l1], c1)]);
        out
    }

    fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                let at = Self::byte_idx(&self.lines[self.line], self.col);
                let rest = self.lines[self.line].split_off(at);
                self.lines.insert(self.line + 1, rest);
                self.line += 1;
                self.col = 0;
            } else {
                let at = Self::byte_idx(&self.lines[self.line], self.col);
                self.lines[self.line].insert(at, c);
                self.col += 1;
            }
        }
    }

    fn move_cursor(&mut self, key: Key, mods: Mods) {
        if mods.shift {
            if self.anchor.is_none() {
                self.anchor = Some((self.line, self.col));
            }
        } else {
            self.anchor = None;
        }
        match key {
            Key::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.line > 0 {
                    self.line -= 1;
                    self.col = self.lines[self.line].chars().count();
                }
                self.pref_col = self.col;
            }
            Key::Right => {
                if self.col < self.lines[self.line].chars().count() {
                    self.col += 1;
                } else if self.line + 1 < self.lines.len() {
                    self.line += 1;
                    self.col = 0;
                }
                self.pref_col = self.col;
            }
            Key::Up => {
                self.line = self.line.saturating_sub(1);
                self.col = self.pref_col;
                self.clamp_cursor();
            }
            Key::Down => {
                self.line = (self.line + 1).min(self.lines.len() - 1);
                self.col = self.pref_col;
                self.clamp_cursor();
            }
            Key::Home => {
                self.col = 0;
                self.pref_col = 0;
            }
            Key::End => {
                self.col = self.lines[self.line].chars().count();
                self.pref_col = self.col;
            }
            Key::PageUp => {
                self.line = self.line.saturating_sub(ROWS);
                self.col = self.pref_col;
                self.clamp_cursor();
            }
            Key::PageDown => {
                self.line = (self.line + ROWS).min(self.lines.len() - 1);
                self.col = self.pref_col;
                self.clamp_cursor();
            }
            _ => {}
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods, code: &mut String) {
        self.set_text(code);
        match key {
            Key::Left
            | Key::Right
            | Key::Up
            | Key::Down
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown => self.move_cursor(key, mods),
            Key::Char(c) if mods.ctrl => match c {
                'a' => {
                    self.anchor = Some((0, 0));
                    self.line = self.lines.len() - 1;
                    self.col = self.lines[self.line].chars().count();
                }
                'c' => self.clipboard = self.selected_text(),
                'x' => {
                    self.clipboard = self.selected_text();
                    if !self.clipboard.is_empty() {
                        self.push_undo();
                        self.delete_selection();
                    }
                }
                'v' => {
                    if !self.clipboard.is_empty() {
                        self.push_undo();
                        self.delete_selection();
                        let text = self.clipboard.clone();
                        self.insert_str(&text);
                    }
                }
                'z' => {
                    if let Some((lines, line, col)) = self.undo.pop() {
                        self.lines = lines;
                        self.line = line;
                        self.col = col;
                        self.anchor = None;
                    }
                }
                _ => {}
            },
            Key::Char(c) => {
                self.push_undo();
                self.delete_selection();
                let mut buf = [0u8; 4];
                self.insert_str(c.encode_utf8(&mut buf));
                self.pref_col = self.col;
            }
            Key::Tab => {
                self.push_undo();
                self.delete_selection();
                self.insert_str("  ");
            }
            Key::Enter => {
                self.push_undo();
                self.delete_selection();
                // Auto-indent: carry the current line's leading spaces.
                let indent: String = self.lines[self.line]
                    .chars()
                    .take_while(|c| *c == ' ')
                    .take(self.col)
                    .collect();
                self.insert_str(&format!("\n{indent}"));
                self.pref_col = self.col;
            }
            Key::Backspace => {
                self.push_undo();
                if !self.delete_selection() {
                    if self.col > 0 {
                        let at = Self::byte_idx(&self.lines[self.line], self.col - 1);
                        self.lines[self.line].remove(at);
                        self.col -= 1;
                    } else if self.line > 0 {
                        let cur = self.lines.remove(self.line);
                        self.line -= 1;
                        self.col = self.lines[self.line].chars().count();
                        self.lines[self.line].push_str(&cur);
                    }
                }
                self.pref_col = self.col;
            }
            Key::Delete => {
                self.push_undo();
                if !self.delete_selection() {
                    let len = self.lines[self.line].chars().count();
                    if self.col < len {
                        let at = Self::byte_idx(&self.lines[self.line], self.col);
                        self.lines[self.line].remove(at);
                    } else if self.line + 1 < self.lines.len() {
                        let next = self.lines.remove(self.line + 1);
                        self.lines[self.line].push_str(&next);
                    }
                }
            }
            Key::Escape | Key::CaptureLabel | Key::ToggleStats => {}
        }
        self.scroll_to_cursor();
        *code = self.text();
    }

    fn scroll_to_cursor(&mut self) {
        if self.line < self.scroll_y {
            self.scroll_y = self.line;
        }
        if self.line >= self.scroll_y + ROWS {
            self.scroll_y = self.line - ROWS + 1;
        }
        if self.col < self.scroll_x {
            self.scroll_x = self.col;
        }
        if self.col >= self.scroll_x + COLS {
            self.scroll_x = self.col - COLS + 1;
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, code: &str) {
        self.set_text(code);
        self.frame += 1;
        let in_area = mouse.y >= AREA_Y && mouse.y < AREA_Y + (ROWS as i32) * font::GLYPH_H;
        if (mouse.left_pressed || mouse.left) && in_area {
            let l = (self.scroll_y as i32 + (mouse.y - AREA_Y) / font::GLYPH_H).max(0) as usize;
            let c = (self.scroll_x as i32 + (mouse.x - AREA_X) / 4).max(0) as usize;
            if mouse.left_pressed {
                self.anchor = None;
                self.line = l.min(self.lines.len() - 1);
                self.col = c;
                self.clamp_cursor();
                self.anchor = Some((self.line, self.col));
            } else {
                // Drag-select.
                self.line = l.min(self.lines.len() - 1);
                self.col = c;
                self.clamp_cursor();
            }
            self.pref_col = self.col;
        }
        if !mouse.left {
            if let Some(a) = self.anchor {
                if a == (self.line, self.col) {
                    self.anchor = None;
                }
            }
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, code: &str) {
        let lines: Vec<&str> = if code.is_empty() {
            vec![""]
        } else {
            code.split('\n').collect()
        };
        fb.rectfill(0, 8, 127, 119, col::BLACK);

        // Block-comment state up to the first visible line.
        let mut in_block = false;
        for l in lines.iter().take(self.scroll_y) {
            in_block = scan_block_state(l, in_block);
        }

        let sel = self.selection();
        for row in 0..ROWS {
            let li = self.scroll_y + row;
            let Some(line) = lines.get(li) else { break };
            let y = AREA_Y + row as i32 * font::GLYPH_H;

            // Selection background.
            if let Some(((l0, c0), (l1, c1))) = sel {
                if li >= l0 && li <= l1 {
                    let len = line.chars().count();
                    let s = if li == l0 { c0 } else { 0 };
                    let e = if li == l1 { c1 } else { len + 1 };
                    let (s, e) = (
                        s.saturating_sub(self.scroll_x),
                        e.saturating_sub(self.scroll_x),
                    );
                    if e > s {
                        fb.rectfill(
                            AREA_X + s as i32 * 4,
                            y - 1,
                            (AREA_X + e as i32 * 4 - 1).min(127),
                            y + font::GLYPH_H - 2,
                            col::DARK_BLUE,
                        );
                    }
                }
            }

            // Highlighted text.
            let spans = highlight(line, &mut in_block);
            for (start, text, color) in spans {
                let vis_start = start as i32 - self.scroll_x as i32;
                for (i, ch) in text.chars().enumerate() {
                    let cx = vis_start + i as i32;
                    if (0..COLS as i32).contains(&cx) {
                        fb.print(ch.encode_utf8(&mut [0u8; 4]), AREA_X + cx * 4, y, color);
                    }
                }
            }
        }

        // Cursor (blinking).
        if (self.frame / 8).is_multiple_of(2) {
            let cy = self.line as i32 - self.scroll_y as i32;
            let cx = self.col as i32 - self.scroll_x as i32;
            if (0..ROWS as i32).contains(&cy) && (0..=COLS as i32).contains(&cx) {
                fb.rectfill(
                    AREA_X + cx * 4,
                    AREA_Y + cy * font::GLYPH_H - 1,
                    AREA_X + cx * 4 + 3,
                    AREA_Y + cy * font::GLYPH_H + font::GLYPH_H - 2,
                    col::RED,
                );
            }
        }

        ui::status_bar(
            fb,
            &format!(
                "Line {}/{} col {}",
                self.line + 1,
                lines.len(),
                self.col + 1
            ),
        );
    }
}

/// Track whether a line ends inside a `/* */` block comment.
fn scan_block_state(line: &str, mut in_block: bool) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if in_block {
            if &b[i..i + 2] == b"*/" {
                in_block = false;
                i += 2;
                continue;
            }
        } else {
            if &b[i..i + 2] == b"//" {
                return in_block;
            }
            if &b[i..i + 2] == b"/*" {
                in_block = true;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    in_block
}

/// Split a line into colored spans: `(start_col, text, color)`.
fn highlight<'a>(line: &'a str, in_block: &mut bool) -> Vec<(usize, &'a str, u8)> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    let slice = |a: usize, b: usize| -> &'a str {
        let start = line
            .char_indices()
            .nth(a)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let end = line
            .char_indices()
            .nth(b)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        &line[start..end]
    };

    while i < n {
        // Inside a block comment: eat until */.
        if *in_block {
            let start = i;
            while i < n {
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    i += 2;
                    *in_block = false;
                    break;
                }
                i += 1;
            }
            out.push((start, slice(start, i), C_COMMENT));
            continue;
        }
        let c = chars[i];
        // Line comment.
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            out.push((i, slice(i, n), C_COMMENT));
            break;
        }
        // Block comment start.
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            *in_block = true;
            i += 2;
            continue;
        }
        // String literal.
        if c == '"' {
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let i2 = i.min(n);
            out.push((start, slice(start, i2), C_STRING));
            i = i2;
            continue;
        }
        // Number.
        if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            out.push((start, slice(start, i), C_NUMBER));
            continue;
        }
        // Identifier / keyword / type / macro.
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word = slice(start, i);
            let color = if i < n && chars[i] == '!' {
                C_MACRO
            } else if KEYWORDS.contains(&word) {
                C_KEYWORD
            } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                C_TYPE
            } else {
                C_TEXT
            };
            out.push((start, word, color));
            continue;
        }
        // Attribute marker.
        if c == '#' {
            out.push((i, slice(i, i + 1), C_MACRO));
            i += 1;
            continue;
        }
        // Whitespace: skip.
        if c == ' ' {
            i += 1;
            continue;
        }
        // Everything else is punctuation.
        let start = i;
        i += 1;
        out.push((start, slice(start, i), C_PUNCT));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed_with(text: &str) -> (CodeEditor, String) {
        let mut e = CodeEditor::new();
        e.set_text(text);
        (e, text.to_string())
    }

    #[test]
    fn typing_inserts() {
        let (mut e, mut code) = ed_with("");
        for c in "fn main".chars() {
            e.key(Key::Char(c), Mods::default(), &mut code);
        }
        assert_eq!(code, "fn main");
    }

    #[test]
    fn enter_auto_indents() {
        let (mut e, mut code) = ed_with("  abc");
        e.key(Key::End, Mods::default(), &mut code);
        e.key(Key::Enter, Mods::default(), &mut code);
        assert_eq!(code, "  abc\n  ");
    }

    #[test]
    fn backspace_joins_lines() {
        let (mut e, mut code) = ed_with("ab\ncd");
        e.key(Key::Down, Mods::default(), &mut code);
        e.key(Key::Home, Mods::default(), &mut code);
        e.key(Key::Backspace, Mods::default(), &mut code);
        assert_eq!(code, "abcd");
    }

    #[test]
    fn select_all_cut_paste() {
        let (mut e, mut code) = ed_with("hello\nworld");
        let ctrl = Mods {
            ctrl: true,
            ..Default::default()
        };
        e.key(Key::Char('a'), ctrl, &mut code);
        e.key(Key::Char('x'), ctrl, &mut code);
        assert_eq!(code, "");
        e.key(Key::Char('v'), ctrl, &mut code);
        assert_eq!(code, "hello\nworld");
    }

    #[test]
    fn undo_restores() {
        let (mut e, mut code) = ed_with("abc");
        e.key(Key::End, Mods::default(), &mut code);
        e.key(Key::Char('!'), Mods::default(), &mut code);
        assert_eq!(code, "abc!");
        e.key(
            Key::Char('z'),
            Mods {
                ctrl: true,
                ..Default::default()
            },
            &mut code,
        );
        assert_eq!(code, "abc");
    }

    #[test]
    fn highlight_classifies() {
        let mut in_block = false;
        let spans = highlight("let x = \"hi\"; // c", &mut in_block);
        let find = |text: &str| spans.iter().find(|(_, t, _)| *t == text).unwrap().2;
        assert_eq!(find("let"), C_KEYWORD);
        assert_eq!(find("x"), C_TEXT);
        assert_eq!(find("\"hi\""), C_STRING);
        assert_eq!(find("// c"), C_COMMENT);
    }

    #[test]
    fn block_comment_state_tracks() {
        assert!(scan_block_state("a /* b", false));
        assert!(!scan_block_state("b */ c", true));
        assert!(!scan_block_state("// /* not", false));
    }
}
