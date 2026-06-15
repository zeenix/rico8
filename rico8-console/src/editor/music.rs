//! The music editor: 64 patterns, four channels each pointing at an SFX,
//! plus flow flags (loop start / loop back / stop). Songs are chains of
//! patterns, walked by the sequencer at runtime.

use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, CHANNELS, MUSIC_COUNT},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
};

const ROW_Y: i32 = 34;
const ROW_H: i32 = 12;

pub struct MusicEditor {
    pattern: usize,
    channel: usize,
    /// Set when the user clicks a channel's pencil; the shell reads it to jump
    /// to that SFX in the SFX editor.
    edit_request: Option<usize>,
}

impl MusicEditor {
    pub fn new() -> Self {
        Self {
            pattern: 0,
            channel: 0,
            edit_request: None,
        }
    }

    /// Take a pending "edit this channel's SFX" request, if any.
    pub fn take_edit_request(&mut self) -> Option<usize> {
        self.edit_request.take()
    }

    fn toggle_play(&self, assets: &Assets, audio: &AudioHandle) {
        audio.load(assets.sfx.clone(), assets.music.clone());
        let playing = audio.with_synth(|s| s.playing_pattern());
        if playing.is_some() {
            audio.play_music(-1);
        } else {
            audio.play_music(self.pattern as i32);
        }
    }

    pub fn key(&mut self, key: Key, _mods: Mods, assets: &mut Assets, audio: &AudioHandle) {
        let pat = &mut assets.music[self.pattern];
        match key {
            Key::Up => self.channel = (self.channel + CHANNELS - 1) % CHANNELS,
            Key::Down => self.channel = (self.channel + 1) % CHANNELS,
            Key::PageUp => self.pattern = (self.pattern + MUSIC_COUNT - 1) % MUSIC_COUNT,
            Key::PageDown => self.pattern = (self.pattern + 1) % MUSIC_COUNT,
            Key::Left => {
                let v = pat.channels[self.channel];
                pat.channels[self.channel] = match v {
                    None => Some(63),
                    Some(0) => None,
                    Some(n) => Some(n - 1),
                };
            }
            Key::Right => {
                let v = pat.channels[self.channel];
                pat.channels[self.channel] = match v {
                    None => Some(0),
                    Some(63) => None,
                    Some(n) => Some(n + 1),
                };
            }
            Key::Delete | Key::Backspace | Key::Char('x') => {
                pat.channels[self.channel] = None;
            }
            Key::Char(' ') => self.toggle_play(assets, audio),
            Key::Char('l') => pat.loop_start = !pat.loop_start,
            Key::Char('b') => pat.loop_back = !pat.loop_back,
            Key::Char('s') => pat.stop_at_end = !pat.stop_at_end,
            Key::Char(c) => {
                // Digits type an SFX number (shifted into a 0..64 value).
                if let Some(d) = c.to_digit(10) {
                    let cur = pat.channels[self.channel].unwrap_or(0) as u32;
                    let next = (cur * 10 + d) % 100;
                    pat.channels[self.channel] = Some((next as u8).min(63));
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets, audio: &AudioHandle) {
        let m = *mouse;
        if !m.left_pressed && !m.right_pressed {
            return;
        }
        let delta: i32 = if m.right_pressed { -1 } else { 1 };
        // Pattern spinner.
        if m.over(28, 9, 39, 15) {
            self.pattern = (self.pattern as i32 + delta).rem_euclid(MUSIC_COUNT as i32) as usize;
        }
        // Play button.
        if m.over(110, 9, 125, 15) && m.left_pressed {
            self.toggle_play(assets, audio);
        }
        // Flag toggles.
        let pat = &mut assets.music[self.pattern];
        for (i, y) in [(0, 20), (1, 20), (2, 20)] {
            let x = 2 + i * 42;
            if m.over(x, y, x + 40, y + 6) && m.left_pressed {
                match i {
                    0 => pat.loop_start = !pat.loop_start,
                    1 => pat.loop_back = !pat.loop_back,
                    _ => pat.stop_at_end = !pat.stop_at_end,
                }
            }
        }
        // Channel rows: click selects, click on value nudges.
        for ch in 0..CHANNELS {
            let y = ROW_Y + ch as i32 * ROW_H;
            if m.over(0, y, 127, y + ROW_H - 1) {
                self.channel = ch;
                if m.over(40, y, 70, y + ROW_H - 1) {
                    let v = pat.channels[ch];
                    pat.channels[ch] = match (v, delta) {
                        (None, 1) => Some(0),
                        (None, _) => Some(63),
                        (Some(0), -1) => None,
                        (Some(63), 1) => None,
                        (Some(n), d) => Some((n as i32 + d) as u8),
                    };
                }
            }
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let pat = &assets.music[self.pattern];
        fb.print("music", 2, 10, col::LIGHT_GREY);
        fb.print(&format!("{:02}", self.pattern), 28, 10, col::WHITE);
        let playing = audio.with_synth(|s| s.playing_pattern());
        fb.print(
            if playing.is_some() { "stop" } else { "play" },
            110,
            10,
            if playing.is_some() {
                col::RED
            } else {
                col::GREEN
            },
        );

        // Flow flags.
        for (i, (label, on)) in [
            ("loop>", pat.loop_start),
            ("<loop", pat.loop_back),
            ("stop", pat.stop_at_end),
        ]
        .iter()
        .enumerate()
        {
            let x = 2 + i as i32 * 42;
            let c = if *on { col::YELLOW } else { col::DARK_PURPLE };
            fb.rectfill(
                x,
                19,
                x + 40,
                27,
                if *on { col::DARK_BLUE } else { col::BLACK },
            );
            fb.print(label, x + 2, 21, c);
        }

        // Channel rows.
        for ch in 0..CHANNELS {
            let y = ROW_Y + ch as i32 * ROW_H;
            let selected = ch == self.channel;
            let bg = if selected { col::DARK_BLUE } else { col::BLACK };
            fb.rectfill(2, y, 125, y + ROW_H - 2, bg);
            fb.print(&format!("ch{ch}"), 6, y + 3, col::LIGHT_GREY);
            match pat.channels[ch] {
                Some(n) => {
                    fb.print(&format!("sfx {n:02}"), 44, y + 3, col::WHITE);
                    // Tiny waveform sketch of the first steps.
                    let sfx = &assets.sfx[n as usize];
                    for (i, note) in sfx.notes.iter().enumerate().take(32) {
                        if note.volume > 0 {
                            let h = (note.pitch as i32 * 6 / 64).min(6);
                            fb.rectfill(80 + i as i32, y + 8 - h, 80 + i as i32, y + 8, col::GREEN);
                        }
                    }
                }
                None => {
                    fb.print("--", 44, y + 3, col::DARK_GREY);
                }
            }
            if playing == Some(self.pattern) {
                fb.print(">", 0, y + 3, col::GREEN);
            }
        }

        // Pattern overview strip: which patterns have content.
        let oy = ROW_Y + CHANNELS as i32 * ROW_H + 6;
        fb.rectfill(0, oy - 2, 127, oy + 20, col::BLACK);
        fb.print("patterns", 2, oy, col::LIGHT_GREY);
        for p in 0..MUSIC_COUNT {
            let x = 2 + (p as i32 % 32) * 4;
            let y = oy + 8 + (p as i32 / 32) * 5;
            let filled = !assets.music[p].is_empty();
            let c = if p == self.pattern {
                col::WHITE
            } else if playing == Some(p) {
                col::GREEN
            } else if filled {
                col::BLUE
            } else {
                col::DARK_GREY
            };
            fb.rectfill(x, y, x + 2, y + 3, c);
        }

        ui::status_bar(fb, "l/b/s flags  spc=play  x=clear");
    }
}
