//! The music editor: a song of 64 patterns, each assigning an SFX to up to four
//! channels and showing those SFX's notes inline, PICO-8-style. Note authoring
//! happens in the SFX editor, reached via each channel's pencil. A pattern's
//! length is governed by its left-most non-looping channel (handled by the
//! runtime sequencer); this editor just arranges patterns and flow flags.

use super::history::History;
use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, MusicPattern, Sfx, CHANNELS, MUSIC_COUNT, SFX_LEN},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
    pico8::{self, Pasted},
};

// Layout, matching PICO-8's recovered framebuffer.
const PANEL_X: [i32; 4] = [2, 33, 64, 95];
const PANEL_W: i32 = 29;
const GRID_TOP: i32 = 33;
const GRID_BOT: i32 = 118;
const ROW_PITCH: i32 = 8;
const MAX_ROWS: usize = 11;
/// Per-channel colour for the activity dots and the playhead.
const CHAN_COL: [u8; CHANNELS] = [col::ORANGE, col::YELLOW, col::GREEN, col::BLUE];
// Grid view: a Pat/Sfx toggle above an 8x8 grid of all 64 patterns or SFX.
const G_X: i32 = 4;
const G_Y: i32 = 31;
const G_CELL_W: i32 = 15;
const G_CELL_H: i32 = 11;

pub struct MusicEditor {
    pattern: usize,
    channel: usize,
    /// Remembers each channel's SFX so toggling it off and on keeps the number.
    last_sfx: [u8; CHANNELS],
    /// Set when the user clicks a channel's pencil; the shell reads it to jump
    /// to that SFX in the SFX editor.
    edit_request: Option<usize>,
    /// View toggle: false = note columns, true = the 64-cell grid.
    grid: bool,
    /// In grid view: false = patterns (Pat), true = SFX.
    grid_sfx: bool,
    status: ui::StatusMsg,
    /// Undo/redo of the song's patterns (last 10 edits).
    history: History<Vec<MusicPattern>>,
}

impl MusicEditor {
    pub fn new() -> Self {
        Self {
            pattern: 0,
            channel: 0,
            last_sfx: [0; CHANNELS],
            edit_request: None,
            grid: false,
            grid_sfx: false,
            status: ui::StatusMsg::default(),
            history: History::new(),
        }
    }

    /// Whether the 64-cell grid view is active (vs. the note-columns view).
    pub fn is_grid(&self) -> bool {
        self.grid
    }

    /// Whether the grid is showing SFX (vs. patterns); for the shell's top-bar toggle.
    pub fn grid_sfx(&self) -> bool {
        self.grid_sfx
    }

    /// Take a pending "edit this channel's SFX" request, if any.
    pub fn take_edit_request(&mut self) -> Option<usize> {
        self.edit_request.take()
    }

    fn toggle_play(&self, assets: &Assets, audio: &AudioHandle) {
        audio.load(assets.sfx.clone(), assets.music.clone());
        if audio.with_synth(|s| s.playing_pattern()).is_some() {
            audio.play_music(-1, 0, 0, 0);
        } else {
            audio.play_music(self.pattern as i32, 0, 0, 0);
        }
    }

    /// Toggle channel `ch` on/off, restoring its remembered SFX when re-enabled.
    fn toggle_channel(&mut self, assets: &mut Assets, ch: usize) {
        let pat = &mut assets.music[self.pattern];
        match pat.channels[ch] {
            Some(n) => {
                self.last_sfx[ch] = n;
                pat.channels[ch] = None;
            }
            None => pat.channels[ch] = Some(self.last_sfx[ch]),
        }
    }

    /// Nudge channel `ch`'s SFX number by `delta`, wrapping 0..64.
    fn nudge_sfx(&mut self, assets: &mut Assets, ch: usize, delta: i32) {
        let pat = &mut assets.music[self.pattern];
        if let Some(n) = pat.channels[ch] {
            let next = (n as i32 + delta).rem_euclid(64) as u8;
            pat.channels[ch] = Some(next);
            self.last_sfx[ch] = next;
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets, audio: &AudioHandle) {
        if mods.ctrl {
            if let Key::Char(c) = key {
                match c.to_ascii_lowercase() {
                    'z' if mods.shift => {
                        self.history.redo(&mut assets.music);
                        return;
                    }
                    'z' => {
                        self.history.undo(&mut assets.music);
                        return;
                    }
                    'y' => {
                        self.history.redo(&mut assets.music);
                        return;
                    }
                    _ => {}
                }
            }
        }
        // Snapshot before, commit after: each key edit is one undo step, and the
        // compare drops keys that change no pattern (navigation, Tab, play).
        self.history.begin(&assets.music);
        match key {
            Key::Up => self.channel = (self.channel + CHANNELS - 1) % CHANNELS,
            Key::Down => self.channel = (self.channel + 1) % CHANNELS,
            Key::PageUp => self.pattern = (self.pattern + MUSIC_COUNT - 1) % MUSIC_COUNT,
            Key::PageDown => self.pattern = (self.pattern + 1) % MUSIC_COUNT,
            Key::Left => self.nudge_sfx(assets, self.channel, -1),
            Key::Right => self.nudge_sfx(assets, self.channel, 1),
            Key::Delete | Key::Backspace | Key::Char('x') => {
                self.toggle_channel(assets, self.channel)
            }
            Key::Char(' ') => self.toggle_play(assets, audio),
            Key::Char('l') => {
                let p = &mut assets.music[self.pattern];
                p.loop_start = !p.loop_start;
            }
            Key::Char('b') => {
                let p = &mut assets.music[self.pattern];
                p.loop_back = !p.loop_back;
            }
            Key::Char('s') => {
                let p = &mut assets.music[self.pattern];
                p.stop_at_end = !p.stop_at_end;
            }
            Key::Tab => self.grid = !self.grid,
            _ => {}
        }
        self.history.commit(&assets.music);
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets, audio: &AudioHandle) {
        self.status.tick();
        // While a song plays, follow the playing pattern so the view scrolls
        // through patterns and reveals channels that come in on later patterns.
        if let Some(p) = audio.with_synth(|s| s.playing_pattern()) {
            self.pattern = p;
        }
        let m = *mouse;
        // Bracket each mouse gesture for undo. Pattern edits happen on the press
        // frame (never on release), so a snapshot taken while the button is down
        // and committed once it comes up is one undo step. The commit compares,
        // so view/navigation clicks record nothing.
        if m.left || m.right {
            self.history.begin(&assets.music);
        } else {
            self.history.commit(&assets.music);
        }
        // Top-bar buttons: notes|grid view toggle, and (in grid view) the Pat/Sfx toggle.
        if m.left_pressed && m.y < 8 {
            if m.over(4, 0, 12, 7) {
                self.grid = false;
                return;
            } else if m.over(13, 0, 22, 7) {
                self.grid = true;
                return;
            } else if self.grid && m.over(28, 0, 40, 7) {
                self.grid_sfx = false;
                return;
            } else if self.grid && m.over(58, 0, 71, 7) {
                self.grid_sfx = true;
                return;
            } else if self.grid && m.over(41, 0, 57, 7) {
                self.grid_sfx = !self.grid_sfx;
                return;
            }
        }
        if !m.left_pressed && !m.right_pressed {
            return;
        }
        // Pattern navigator arrows (both views).
        if m.over(35, 12, 38, 17) && m.left_pressed {
            self.pattern = (self.pattern + MUSIC_COUNT - 1) % MUSIC_COUNT;
        } else if m.over(94, 12, 98, 17) && m.left_pressed {
            self.pattern = (self.pattern + 1) % MUSIC_COUNT;
        }
        // Pattern boxes (both views).
        let first = self.first_pattern();
        for i in 0..5 {
            let x = 40 + i as i32 * 11;
            if m.over(x, 12, x + 8, 18) && m.left_pressed {
                self.pattern = first + i;
            }
        }
        // Flow buttons (both views).
        if m.left_pressed {
            let pat = &mut assets.music[self.pattern];
            if m.over(102, 12, 108, 19) {
                pat.loop_start = !pat.loop_start;
            } else if m.over(109, 12, 116, 19) {
                pat.loop_back = !pat.loop_back;
            } else if m.over(117, 12, 123, 19) {
                pat.stop_at_end = !pat.stop_at_end;
            }
        }
        // Per-channel header controls (both views; the SFX-number nudge is notes-only,
        // since grid view picks SFX from the grid).
        let delta: i32 = if m.right_pressed { -1 } else { 1 };
        for (ch, &x) in PANEL_X.iter().enumerate() {
            if m.over(x + 2, 24, x + 6, 28) && m.left_pressed {
                self.channel = ch;
                self.toggle_channel(assets, ch);
            } else if m.over(x + 9, 23, x + 18, 29) {
                self.channel = ch;
                if !self.grid {
                    self.nudge_sfx(assets, ch, delta);
                }
            } else if m.over(x + 21, 23, x + 27, 29) && m.left_pressed {
                if let Some(n) = assets.music[self.pattern].channels[ch] {
                    self.edit_request = Some(n as usize);
                }
            }
        }
        if self.grid {
            self.grid_tick(&m, assets);
        }
        let _ = audio;
    }

    /// Handle a click on a grid cell: pick a pattern (Pat) or assign an SFX to the
    /// current channel (Sfx).
    fn grid_tick(&mut self, m: &Mouse, assets: &mut Assets) {
        if !m.left_pressed {
            return;
        }
        if let Some(n) = grid_cell(m.x, m.y) {
            if self.grid_sfx {
                assets.music[self.pattern].channels[self.channel] = Some(n as u8);
                self.last_sfx[self.channel] = n as u8;
            } else {
                self.pattern = n;
            }
        }
    }

    /// First pattern shown in the 5-box navigator (centred on the current one).
    fn first_pattern(&self) -> usize {
        self.pattern.saturating_sub(2).min(MUSIC_COUNT - 5)
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let pat = &assets.music[self.pattern];
        let playing = audio.with_synth(|s| s.playing_pattern());
        let steps = audio.channel_step();

        // --- Pattern strip (both views) ---
        fb.print("Pattern", 4, 13, col::LIGHT_GREY);
        ui::arrow_l(fb, 35, 13, col::PINK);
        let first = self.first_pattern();
        for i in 0..5 {
            let p = first + i;
            let x = 40 + i as i32 * 11;
            // Activity dots for the channels this pattern uses.
            for (ch, c) in CHAN_COL.iter().enumerate() {
                if assets.music[p].channels[ch].is_some() {
                    fb.pset(x + 1 + ch as i32 * 2, 10, *c);
                }
            }
            let sel = p == self.pattern;
            fb.rectfill(x, 12, x + 8, 18, col::BLACK);
            if sel {
                fb.rect(x - 1, 11, x + 9, 19, col::WHITE);
            }
            let c = if sel { col::WHITE } else { col::LIGHT_GREY };
            fb.print(&format!("{p:02}"), x + 1, 13, c);
        }
        ui::arrow_r(fb, 95, 13, col::PINK);
        // Flow buttons: the colour itself shows on/off.
        ui::blit(fb, 100, 12, &ui::FLOW);
        recolor_flow(fb, 100, 108, pat.loop_start);
        recolor_flow(fb, 109, 116, pat.loop_back);
        recolor_flow(fb, 117, 126, pat.stop_at_end);

        // --- Channel headers (both views): radio + SFX# + pencil ---
        for (ch, &x) in PANEL_X.iter().enumerate() {
            let slot = pat.channels[ch];
            ui::radio(fb, x + 2, 24, slot.is_some());
            if let Some(n) = slot {
                fb.rectfill(x + 9, 23, x + 18, 29, col::BLACK);
                fb.print(&format!("{n:02}"), x + 10, 24, col::WHITE);
                ui::pencil(fb, x + 22, 24);
            }
        }

        if self.grid {
            // Box the current channel — the SFX grid's assign target.
            let hx = PANEL_X[self.channel];
            fb.rect(hx, 22, hx + PANEL_W, 30, col::PINK);
            self.draw_grid(fb, assets, audio);
            self.status.show(fb, "Tab notes  click a cell to pick");
            return;
        }

        // --- Notes view: per-channel note panel + column ---
        for ch in 0..CHANNELS {
            let x = PANEL_X[ch];
            let slot = pat.channels[ch];
            // Note panel: filled black when active, black-bordered box when empty.
            if slot.is_some() {
                fb.rectfill(x, GRID_TOP - 2, x + PANEL_W, GRID_BOT, col::BLACK);
            } else {
                fb.rect(x, GRID_TOP - 2, x + PANEL_W, GRID_BOT, col::BLACK);
            }
            let Some(n) = slot else { continue };
            let sfx = &assets.sfx[n as usize];
            // The channel playhead, only while this pattern is the one playing.
            let head = (playing == Some(self.pattern))
                .then_some(steps[ch])
                .flatten();
            let total = note_rows(sfx.loop_start, sfx.loop_end);
            // Scroll the window so the playhead stays visible during playback.
            let max_first = total.saturating_sub(MAX_ROWS);
            let first = head.map_or(0, |h| h.saturating_sub(MAX_ROWS / 2).min(max_first));
            for row in 0..MAX_ROWS {
                let step = first + row;
                if step >= total {
                    break;
                }
                let y = GRID_TOP + row as i32 * ROW_PITCH;
                if row > 0 && step.is_multiple_of(4) {
                    for gx in (x + 1..x + PANEL_W).step_by(2) {
                        fb.pset(gx, y - 2, col::DARK_GREY);
                    }
                }
                if head == Some(step) {
                    fb.rectfill(x, y - 1, x + PANEL_W, y + 5, col::YELLOW);
                }
                ui::note_cell(fb, x, y, sfx.notes[step]);
            }
        }

        self.status.show(fb, "Spc play  pgup/dn pat  x ch");
    }

    /// The grid view: an 8x8 grid of all 64 patterns or SFX, the current one boxed
    /// white. Pattern cells show the number + per-channel activity dots; SFX cells
    /// show a pitch thumbnail (or the number when empty) and, during playback, a
    /// white playhead sweeping each playing channel's SFX cell.
    fn draw_grid(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let cur_sfx = assets.music[self.pattern].channels[self.channel];
        for n in 0..64usize {
            let x = G_X + (n % 8) as i32 * G_CELL_W;
            let y = G_Y + (n / 8) as i32 * G_CELL_H;
            fb.rectfill(x, y, x + G_CELL_W - 2, y + G_CELL_H - 2, col::DARK_BLUE);
            let sel = if self.grid_sfx {
                cur_sfx == Some(n as u8)
            } else {
                n == self.pattern
            };
            if sel {
                fb.rect(x - 1, y - 1, x + G_CELL_W - 1, y + G_CELL_H - 1, col::WHITE);
            }
            let c = if sel { col::WHITE } else { col::LIGHT_GREY };
            if self.grid_sfx {
                let sfx = &assets.sfx[n];
                if sfx.notes.iter().all(|nt| nt.volume == 0) {
                    fb.print(&format!("{n:02}"), x + 1, y + 1, c);
                } else {
                    draw_sfx_thumb(fb, sfx, x, y);
                }
            } else {
                fb.print(&format!("{n:02}"), x + 1, y + 1, c);
                for (ch, c) in CHAN_COL.iter().enumerate() {
                    if assets.music[n].channels[ch].is_some() {
                        fb.pset(x + 1 + ch as i32 * 3, y + G_CELL_H - 3, *c);
                    }
                }
            }
        }
        // Playheads: in SFX mode, while the shown pattern plays, sweep each playing
        // channel's SFX cell at its current step.
        if self.grid_sfx && audio.with_synth(|s| s.playing_pattern()) == Some(self.pattern) {
            let steps = audio.channel_step();
            let channels = &assets.music[self.pattern].channels;
            for (chan_slot, step_slot) in channels.iter().zip(steps.iter()) {
                let (Some(n), Some(step)) = (chan_slot, step_slot) else {
                    continue;
                };
                let cell = *n as usize;
                let cx = G_X + (cell % 8) as i32 * G_CELL_W;
                let cy = G_Y + (cell / 8) as i32 * G_CELL_H;
                let sfx = &assets.sfx[cell];
                let total = note_rows(sfx.loop_start, sfx.loop_end).max(1) as i32;
                let off = (*step as i32 * (G_CELL_W - 2) / total).clamp(0, G_CELL_W - 2);
                fb.line(cx + off, cy, cx + off, cy + G_CELL_H - 2, col::WHITE);
            }
        }
    }

    /// Set a transient bottom-bar message (used for clipboard errors).
    pub fn set_status(&mut self, msg: String) {
        self.status.set(msg);
    }

    /// Copy the selected pattern as an `[sfx]` clipboard blob (its channel SFX
    /// plus a footer that rebuilds the pattern).
    pub fn copy(&mut self, assets: &Assets) -> String {
        self.status.set(format!("copied pat {}", self.pattern));
        pico8::encode_pattern(assets, self.pattern)
    }

    /// Paste a decoded PICO-8 clipboard blob. An `[sfx]` blob (which is what a
    /// copied pattern is) rebuilds a pattern at the selected slot, appending its
    /// SFX after the last used slot.
    pub fn paste(&mut self, pasted: &Pasted, assets: &mut Assets) {
        match pasted {
            Pasted::Sfx(clip) => {
                let report = pico8::paste_pattern(assets, clip, self.pattern);
                self.status.set(report.summary);
            }
            Pasted::Sprites(_) => self.status.set("sprites - use sprite editor".into()),
        }
    }
}

/// Recolour a flow button's pixels (blue/dark-blue) to show its on/off state:
/// light blue when `on`, dark blue when off.
fn recolor_flow(fb: &mut Framebuffer, x0: i32, x1: i32, on: bool) {
    let to = if on { col::BLUE } else { col::DARK_BLUE };
    for y in 12..21 {
        for x in x0..=x1 {
            let c = fb.pget(x, y);
            if c == col::BLUE || c == col::DARK_BLUE {
                fb.pset(x, y, to);
            }
        }
    }
}

/// The playable step count of an SFX (loop end, LEN marker, or the full 32).
fn note_rows(loop_start: u8, loop_end: u8) -> usize {
    if loop_end > loop_start {
        loop_end as usize
    } else if loop_start > 0 {
        loop_start as usize
    } else {
        SFX_LEN
    }
}

/// The cell index (0..64) under screen point (x, y) in the grid, if any.
fn grid_cell(x: i32, y: i32) -> Option<usize> {
    let (cx, cy) = (x - G_X, y - G_Y);
    if cx < 0 || cy < 0 || cx >= 8 * G_CELL_W || cy >= 8 * G_CELL_H {
        return None;
    }
    Some(((cy / G_CELL_H) * 8 + cx / G_CELL_W) as usize)
}

/// A connected pitch line of an SFX's notes, filling its grid cell. The caller
/// draws this only for non-empty SFX (empty ones show their number instead).
fn draw_sfx_thumb(fb: &mut Framebuffer, sfx: &Sfx, x: i32, y: i32) {
    let top = y + 1;
    let bot = y + G_CELL_H - 2;
    let span = bot - top;
    let w = G_CELL_W - 2;
    let mut prev: Option<i32> = None;
    for col_i in 0..w {
        let step = (col_i as usize * SFX_LEN) / w as usize;
        let note = sfx.notes[step.min(SFX_LEN - 1)];
        if note.volume == 0 {
            prev = None;
            continue;
        }
        let py = bot - note.pitch.min(63) as i32 * span / 63;
        match prev {
            Some(p) => fb.line(x + col_i, p, x + col_i, py, col::PINK),
            None => fb.pset(x + col_i, py, col::PINK),
        }
        prev = Some(py);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> AudioHandle {
        AudioHandle::dummy()
    }

    fn press(x: i32, y: i32) -> Mouse {
        Mouse {
            x,
            y,
            left: true,
            left_pressed: true,
            ..Default::default()
        }
    }

    #[test]
    fn tab_toggles_grid_view() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        assert!(!ed.is_grid());
        ed.key(Key::Tab, Mods::default(), &mut a, &dummy());
        assert!(ed.is_grid());
        ed.key(Key::Tab, Mods::default(), &mut a, &dummy());
        assert!(!ed.is_grid());
    }

    #[test]
    fn view_buttons_toggle_grid() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        ed.tick(&press(15, 2), &mut a, &dummy()); // grid button
        assert!(ed.is_grid());
        ed.tick(&press(6, 2), &mut a, &dummy()); // notes button
        assert!(!ed.is_grid());
    }

    #[test]
    fn pat_sfx_toggle_switches_grid_mode() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        ed.grid = true;
        // The toggle now lives in the top bar (y < 8): Pat at x28..40, Sfx at x58..71.
        ed.tick(&press(60, 3), &mut a, &dummy()); // "Sfx"
        assert!(ed.grid_sfx);
        ed.tick(&press(30, 3), &mut a, &dummy()); // "Pat"
        assert!(!ed.grid_sfx);
        // Clicking the switch body (between the labels) flips the mode.
        ed.tick(&press(49, 3), &mut a, &dummy());
        assert!(ed.grid_sfx);
        ed.tick(&press(49, 3), &mut a, &dummy());
        assert!(!ed.grid_sfx);
    }

    #[test]
    fn clicking_a_channel_header_selects_it_in_grid() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        a.music[0].channels[2] = Some(5);
        ed.grid = true;
        ed.grid_sfx = true;
        // Channel 2's SFX-number region: PANEL_X[2]=64, x+9..x+18 = 73..82, y23..29.
        ed.tick(&press(75, 25), &mut a, &dummy());
        assert_eq!(ed.channel, 2, "header click selects the channel");
        assert_eq!(a.music[0].channels[2], Some(5), "no nudge in grid view");
    }

    #[test]
    fn clicking_a_pat_cell_selects_the_pattern() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        // Pat mode (default): cell 10 is col 2, row 1.
        // x = G_X + 2*G_CELL_W + 1 = 35, y = G_Y + G_CELL_H + 1 = 43.
        ed.grid = true;
        ed.tick(&press(35, 43), &mut a, &dummy());
        assert_eq!(ed.pattern, 10);
    }

    #[test]
    fn clicking_an_sfx_cell_assigns_to_the_current_channel() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        ed.grid = true;
        ed.grid_sfx = true;
        ed.channel = 1;
        // Cell 12 -> col 4, row 1: x = G_X + 4*G_CELL_W + 1 = 65, y = G_Y + G_CELL_H + 1 = 43.
        ed.tick(&press(65, 43), &mut a, &dummy());
        assert_eq!(a.music[ed.pattern].channels[1], Some(12));
        assert_eq!(ed.last_sfx[1], 12);
    }

    fn ctrl(shift: bool) -> Mods {
        Mods {
            ctrl: true,
            shift,
            ..Default::default()
        }
    }

    #[test]
    fn undo_and_redo_a_channel_toggle() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        a.music[0].channels[0] = Some(7);
        ed.key(Key::Char('x'), Mods::default(), &mut a, &dummy()); // toggle off
        assert_eq!(a.music[0].channels[0], None);
        ed.key(Key::Char('z'), ctrl(false), &mut a, &dummy());
        assert_eq!(a.music[0].channels[0], Some(7), "undo restores the channel");
        ed.key(Key::Char('z'), ctrl(true), &mut a, &dummy()); // Ctrl+Shift+Z
        assert_eq!(a.music[0].channels[0], None, "redo toggles it back off");
    }

    #[test]
    fn undo_reverts_a_grid_sfx_assignment() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        ed.grid = true;
        ed.grid_sfx = true;
        ed.channel = 1;
        // Assign SFX via a grid click, then release to close the gesture.
        ed.tick(&press(65, 43), &mut a, &dummy()); // cell 12 -> channel 1
        ed.tick(&Mouse::default(), &mut a, &dummy());
        assert_eq!(a.music[ed.pattern].channels[1], Some(12));
        ed.key(Key::Char('z'), ctrl(false), &mut a, &dummy());
        assert_eq!(a.music[ed.pattern].channels[1], None, "undo clears it");
    }

    #[test]
    fn note_rows_handles_loop_len_and_full() {
        assert_eq!(note_rows(0, 0), SFX_LEN); // no loop, no LEN -> full
        assert_eq!(note_rows(8, 0), 8); // LEN marker
        assert_eq!(note_rows(2, 6), 6); // loop range -> end
    }

    #[test]
    fn toggle_channel_remembers_sfx() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        a.music[0].channels[0] = Some(7);
        ed.toggle_channel(&mut a, 0); // off, remembers 7
        assert_eq!(a.music[0].channels[0], None);
        ed.toggle_channel(&mut a, 0); // on, restores 7
        assert_eq!(a.music[0].channels[0], Some(7));
    }

    #[test]
    fn nudge_wraps_and_ignores_disabled() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        a.music[0].channels[0] = Some(63);
        ed.nudge_sfx(&mut a, 0, 1);
        assert_eq!(a.music[0].channels[0], Some(0), "wraps 63 -> 0");
        a.music[0].channels[1] = None;
        ed.nudge_sfx(&mut a, 1, 1);
        assert_eq!(a.music[0].channels[1], None, "disabled channel untouched");
    }

    #[test]
    fn grid_view_draws_without_panic() {
        let mut ed = MusicEditor::new();
        let mut a = Assets::default();
        a.sfx[1].notes[0].pitch = 40;
        a.sfx[1].notes[0].volume = 5;
        ed.grid = true;
        ed.grid_sfx = true;
        let mut fb = Framebuffer::new();
        ed.draw(&mut fb, &a, &dummy()); // exercises thumbnails + playhead path.
        ed.grid_sfx = false;
        ed.draw(&mut fb, &a, &dummy()); // exercises pattern cells + activity dots.
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    use rico8_runtime::{
        assets::{MusicPattern, Sfx},
        pico8::{parse_clipboard, Pasted, SfxClip, Slotted},
    };

    fn one_sfx(pitch: u8) -> Sfx {
        let mut s = Sfx::default();
        s.notes[0].pitch = pitch;
        s.notes[0].volume = 4;
        s
    }

    #[test]
    fn rebuilds_pattern_at_selected_slot() {
        let mut ed = MusicEditor::new(); // pattern 0.
        let mut assets = Assets::default();
        let clip = SfxClip {
            records: vec![
                Slotted {
                    src: 8,
                    value: one_sfx(1),
                },
                Slotted {
                    src: 9,
                    value: one_sfx(2),
                },
            ],
            patterns: vec![MusicPattern {
                channels: [Some(8), Some(9), None, None],
                loop_back: false,
                loop_start: false,
                stop_at_end: false,
            }],
        };
        ed.paste(&Pasted::Sfx(clip), &mut assets);
        assert_eq!(assets.sfx[0].notes[0].pitch, 1); // appended at slot 0.
        assert_eq!(assets.music[0].channels, [Some(0), Some(1), None, None]);
        assert!(ed.status.current().unwrap().contains("pat 0"));
    }

    #[test]
    fn copies_selected_pattern() {
        let mut ed = MusicEditor::new();
        let mut assets = Assets::default();
        assets.music[0].channels = [Some(3), None, None, None];
        assets.sfx[3].notes[0].volume = 5;
        let blob = ed.copy(&assets);
        assert!(ed.status.current().unwrap().contains("copied pat 0"));
        let Pasted::Sfx(clip) = parse_clipboard(&blob).unwrap() else {
            panic!("not sfx")
        };
        assert_eq!(clip.patterns[0].channels, [Some(3), None, None, None]);
    }
}
