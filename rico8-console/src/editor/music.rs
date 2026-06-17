//! The music editor: a song of 64 patterns, each assigning an SFX to up to four
//! channels and showing those SFX's notes inline, PICO-8-style. Note authoring
//! happens in the SFX editor, reached via each channel's pencil. A pattern's
//! length is governed by its left-most non-looping channel (handled by the
//! runtime sequencer); this editor just arranges patterns and flow flags.

use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, CHANNELS, MUSIC_COUNT, SFX_LEN},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
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

pub struct MusicEditor {
    pattern: usize,
    channel: usize,
    /// Remembers each channel's SFX so toggling it off and on keeps the number.
    last_sfx: [u8; CHANNELS],
    /// Set when the user clicks a channel's pencil; the shell reads it to jump
    /// to that SFX in the SFX editor.
    edit_request: Option<usize>,
}

impl MusicEditor {
    pub fn new() -> Self {
        Self {
            pattern: 0,
            channel: 0,
            last_sfx: [0; CHANNELS],
            edit_request: None,
        }
    }

    /// Take a pending "edit this channel's SFX" request, if any.
    pub fn take_edit_request(&mut self) -> Option<usize> {
        self.edit_request.take()
    }

    fn toggle_play(&self, assets: &Assets, audio: &AudioHandle) {
        audio.load(assets.sfx.clone(), assets.music.clone());
        if audio.with_synth(|s| s.playing_pattern()).is_some() {
            audio.play_music(-1);
        } else {
            audio.play_music(self.pattern as i32);
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

    pub fn key(&mut self, key: Key, _mods: Mods, assets: &mut Assets, audio: &AudioHandle) {
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
            _ => {}
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets, audio: &AudioHandle) {
        // While a song plays, follow the playing pattern so the view scrolls
        // through patterns and reveals channels that come in on later patterns.
        if let Some(p) = audio.with_synth(|s| s.playing_pattern()) {
            self.pattern = p;
        }
        let m = *mouse;
        if !m.left_pressed && !m.right_pressed {
            return;
        }
        let delta: i32 = if m.right_pressed { -1 } else { 1 };
        // Pattern navigator arrows.
        if m.over(35, 12, 38, 17) && m.left_pressed {
            self.pattern = (self.pattern + MUSIC_COUNT - 1) % MUSIC_COUNT;
        } else if m.over(94, 12, 98, 17) && m.left_pressed {
            self.pattern = (self.pattern + 1) % MUSIC_COUNT;
        }
        // Pattern boxes.
        let first = self.first_pattern();
        for i in 0..5 {
            let x = 40 + i as i32 * 11;
            if m.over(x, 12, x + 8, 18) && m.left_pressed {
                self.pattern = first + i;
            }
        }
        // Flow buttons (loop-start / loop-back / stop).
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
        // Per-channel header controls.
        for (ch, &x) in PANEL_X.iter().enumerate() {
            if m.over(x + 2, 24, x + 6, 28) && m.left_pressed {
                self.channel = ch;
                self.toggle_channel(assets, ch);
            } else if m.over(x + 9, 23, x + 18, 29) {
                self.channel = ch;
                self.nudge_sfx(assets, ch, delta);
            } else if m.over(x + 21, 23, x + 27, 29) && m.left_pressed {
                if let Some(n) = assets.music[self.pattern].channels[ch] {
                    self.edit_request = Some(n as usize);
                }
            }
        }
        // Click in a note panel selects that channel; double-use as play toggle
        // is via the keyboard.
        let _ = audio;
    }

    /// First pattern shown in the 5-box navigator (centred on the current one).
    fn first_pattern(&self) -> usize {
        self.pattern.saturating_sub(2).min(MUSIC_COUNT - 5)
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let pat = &assets.music[self.pattern];
        let playing = audio.with_synth(|s| s.playing_pattern());
        let steps = audio.channel_step();

        // --- Pattern strip ---
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
        // Flow buttons: the colour itself shows on/off — light blue when the
        // flag is set, dark blue when not (matching PICO-8).
        ui::blit(fb, 100, 12, &ui::FLOW);
        recolor_flow(fb, 100, 108, pat.loop_start);
        recolor_flow(fb, 109, 116, pat.loop_back);
        recolor_flow(fb, 117, 126, pat.stop_at_end);

        // --- Channels: header + note column ---
        for ch in 0..CHANNELS {
            let x = PANEL_X[ch];
            let slot = pat.channels[ch];
            ui::radio(fb, x + 2, 24, slot.is_some());
            if let Some(n) = slot {
                fb.rectfill(x + 9, 23, x + 18, 29, col::BLACK);
                fb.print(&format!("{n:02}"), x + 10, 24, col::WHITE);
                ui::pencil(fb, x + 22, 24);
            }
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

        ui::status_bar(fb, "Spc play  pgup/dn pat  x ch");
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
