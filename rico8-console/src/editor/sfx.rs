//! The SFX editor: a 32-step tracker. Two columns of 16 steps, each step
//! holding note, waveform, volume and effect. Piano keys on the keyboard
//! enter notes; space previews.

use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, NOTE_CUSTOM_FLAG, SFX_LEN},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
};

const STEP_Y: i32 = 20;
const COL_X: [i32; 2] = [4, 68];

/// Per-SFX filter switches, drawn as a strip in the right gutter. The first
/// two (`nz`, `bz`) are on/off; the rest cycle 0..3.
const FX_X: i32 = 106;
const FX_Y: i32 = 30;
const FX_DY: i32 = 9;
const FX_LABELS: [&str; 5] = ["nz", "bz", "dt", "rv", "dm"];

/// Editable fields of a step.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Note,
    Wave,
    Vol,
    Fx,
}
const FIELDS: [Field; 4] = [Field::Note, Field::Wave, Field::Vol, Field::Fx];

/// Piano mapping: key -> semitone offset from C of the current octave.
const PIANO_LOW: &str = "zsxdcvgbhnjm";
const PIANO_HIGH: &str = "q2w3er5t6y7u";

pub struct SfxEditor {
    sfx: usize,
    step: usize,
    field: usize,
    octave: u8,
}

impl SfxEditor {
    pub fn new() -> Self {
        Self {
            sfx: 0,
            step: 0,
            field: 0,
            octave: 2,
        }
    }

    /// Select an SFX slot (used when jumping in from the music editor's pencil).
    pub fn select(&mut self, sfx: usize) {
        self.sfx = sfx % 64;
    }

    fn preview(&self, assets: &Assets, audio: &AudioHandle) {
        audio.load(assets.sfx.clone(), assets.music.clone());
        let playing = audio.with_synth(|s| s.channel_sfx()[0]);
        if playing == Some(self.sfx) {
            audio.play_sfx(-1, 0);
        } else {
            audio.play_sfx(self.sfx as i32, 0);
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets, audio: &AudioHandle) {
        match key {
            Key::Up => self.step = (self.step + SFX_LEN - 1) % SFX_LEN,
            Key::Down => self.step = (self.step + 1) % SFX_LEN,
            Key::Left => self.field = (self.field + FIELDS.len() - 1) % FIELDS.len(),
            Key::Right => self.field = (self.field + 1) % FIELDS.len(),
            Key::PageUp => self.sfx = (self.sfx + 63) % 64,
            Key::PageDown => self.sfx = (self.sfx + 1) % 64,
            Key::Delete | Key::Backspace => {
                assets.sfx[self.sfx].notes[self.step] = Default::default();
            }
            Key::Char(' ') => self.preview(assets, audio),
            Key::Char('[') => {
                if mods.shift {
                    let s = &mut assets.sfx[self.sfx];
                    s.loop_end = s.loop_end.saturating_sub(1);
                } else {
                    self.octave = self.octave.saturating_sub(1);
                }
            }
            Key::Char(']') => {
                if mods.shift {
                    let s = &mut assets.sfx[self.sfx];
                    s.loop_end = (s.loop_end + 1).min(SFX_LEN as u8);
                } else {
                    self.octave = (self.octave + 1).min(4);
                }
            }
            Key::Char('-') => {
                let s = &mut assets.sfx[self.sfx];
                s.speed = s.speed.saturating_sub(1).max(1);
            }
            Key::Char('=') | Key::Char('+') => {
                let s = &mut assets.sfx[self.sfx];
                s.speed = s.speed.saturating_add(1);
            }
            Key::Char(c) => self.char_input(c, assets),
            _ => {}
        }
    }

    fn char_input(&mut self, c: char, assets: &mut Assets) {
        let note = &mut assets.sfx[self.sfx].notes[self.step];
        match FIELDS[self.field] {
            Field::Note => {
                let (offset, oct_up) = if let Some(i) = PIANO_LOW.find(c) {
                    (i as u8, 0)
                } else if let Some(i) = PIANO_HIGH.find(c) {
                    (i as u8, 1)
                } else {
                    return;
                };
                note.pitch = ((self.octave + oct_up) * 12 + offset).min(63);
                if note.volume == 0 {
                    note.volume = 5;
                }
                // Stepping down makes entering melodies fast.
                self.step = (self.step + 1) % SFX_LEN;
            }
            Field::Wave => {
                if c == 'i' {
                    // Toggle "custom instrument": the index then names another
                    // SFX (0-7) used as the instrument rather than a built-in
                    // waveform, matching PICO-8.
                    note.wave ^= NOTE_CUSTOM_FLAG;
                } else if let Some(d) = c.to_digit(8) {
                    note.wave = (note.wave & NOTE_CUSTOM_FLAG) | d as u8;
                }
            }
            Field::Vol => {
                if let Some(d) = c.to_digit(8) {
                    note.volume = d as u8;
                }
            }
            Field::Fx => {
                if let Some(d) = c.to_digit(8) {
                    note.effect = d as u8;
                }
            }
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets, audio: &AudioHandle) {
        let m = *mouse;
        if !m.left_pressed && !m.right_pressed {
            return;
        }
        let delta: i32 = if m.right_pressed { -1 } else { 1 };
        // Header spinners: sfx number, speed, loop start/end.
        if m.over(16, 9, 27, 15) {
            self.sfx = (self.sfx as i32 + delta).rem_euclid(64) as usize;
        } else if m.over(48, 9, 59, 15) {
            let s = &mut assets.sfx[self.sfx];
            s.speed = (s.speed as i32 + delta).clamp(1, 255) as u8;
        } else if m.over(84, 9, 91, 15) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_start = (s.loop_start as i32 + delta).clamp(0, 31) as u8;
        } else if m.over(96, 9, 103, 15) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_end = (s.loop_end as i32 + delta).clamp(0, 32) as u8;
        } else if m.over(110, 9, 125, 15) && m.left_pressed {
            self.preview(assets, audio);
        }
        // Filter switches: nz/bz toggle, dt/rv/dm cycle (left +1, right -1).
        for i in 0..FX_LABELS.len() {
            let y = FX_Y + i as i32 * FX_DY;
            if m.over(FX_X, y - 1, FX_X + 17, y + 5) {
                let s = &mut assets.sfx[self.sfx];
                match i {
                    0 => s.noiz = !s.noiz,
                    1 => s.buzz = !s.buzz,
                    2 => s.detune = (s.detune as i32 + delta).rem_euclid(3) as u8,
                    3 => s.reverb = (s.reverb as i32 + delta).rem_euclid(3) as u8,
                    _ => s.dampen = (s.dampen as i32 + delta).rem_euclid(3) as u8,
                }
            }
        }
        // Step grid.
        for (half, x) in COL_X.iter().enumerate() {
            if m.over(*x, STEP_Y, x + 35, STEP_Y + 16 * 6 - 1) {
                let row = ((m.y - STEP_Y) / 6) as usize;
                self.step = half * 16 + row;
                let cx = m.x - x;
                self.field = match cx {
                    0..=13 => 0,
                    14..=21 => 1,
                    22..=29 => 2,
                    _ => 3,
                };
            }
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let s = &assets.sfx[self.sfx];
        fb.rectfill(0, STEP_Y - 2, 127, 119, col::BLACK);
        // Header.
        fb.print("sfx", 2, 10, col::LIGHT_GREY);
        fb.print(&format!("{:02}", self.sfx), 16, 10, col::WHITE);
        fb.print("spd", 34, 10, col::LIGHT_GREY);
        fb.print(&format!("{:02}", s.speed), 48, 10, col::WHITE);
        fb.print("loop", 64, 10, col::LIGHT_GREY);
        fb.print(&format!("{:02}", s.loop_start), 84, 10, col::WHITE);
        fb.print(&format!("{:02}", s.loop_end), 96, 10, col::WHITE);
        let playing = audio.with_synth(|sy| sy.channel_sfx()[0]) == Some(self.sfx);
        fb.print(
            if playing { "stop" } else { "play" },
            110,
            10,
            if playing { col::RED } else { col::GREEN },
        );

        // Steps.
        for (half, &x) in COL_X.iter().enumerate() {
            for row in 0..16usize {
                let i = half * 16 + row;
                let y = STEP_Y + row as i32 * 6;
                let n = s.notes[i];
                let active = n.volume > 0;

                // Loop range marker.
                if s.loop_end > s.loop_start && (i as u8) >= s.loop_start && (i as u8) < s.loop_end
                {
                    fb.rectfill(x - 3, y, x - 2, y + 4, col::DARK_GREEN);
                }
                // Cursor.
                if i == self.step {
                    fb.rectfill(x - 1, y - 1, x + 35, y + 5, col::DARK_BLUE);
                    let fx0 = x + [0, 16, 24, 32][self.field];
                    let fw = if self.field == 0 { 11 } else { 3 };
                    fb.rect(fx0 - 1, y - 1, fx0 + fw + 1, y + 5, col::WHITE);
                }

                let note_col = if active { col::WHITE } else { col::DARK_GREY };
                fb.print(&note_name(n.pitch, active), x, y, note_col);
                // The waveform/instrument index, tinted yellow when it refers
                // to a custom instrument (another SFX) rather than a built-in.
                let wave_col = if !active {
                    col::DARK_GREY
                } else if n.instrument().is_some() {
                    col::YELLOW
                } else {
                    col::PINK
                };
                fb.print(&format!("{}", n.wave_index()), x + 16, y, wave_col);
                fb.print(
                    &format!("{}", n.volume),
                    x + 24,
                    y,
                    if active { col::GREEN } else { col::DARK_GREY },
                );
                fb.print(
                    &format!("{}", n.effect),
                    x + 32,
                    y,
                    if n.effect != 0 && active {
                        col::ORANGE
                    } else {
                        col::DARK_GREY
                    },
                );
            }
        }

        // Filter switches strip (right gutter).
        fb.print("fx", FX_X + 4, STEP_Y, col::LIGHT_GREY);
        let levels = [s.noiz as u8, s.buzz as u8, s.detune, s.reverb, s.dampen];
        for (i, (label, &level)) in FX_LABELS.iter().zip(levels.iter()).enumerate() {
            let y = FX_Y + i as i32 * FX_DY;
            let on = level > 0;
            fb.print(label, FX_X, y, if on { col::WHITE } else { col::DARK_GREY });
            // nz/bz are on/off; the rest show their level.
            let val = if i < 2 {
                if on {
                    "*".into()
                } else {
                    "-".into()
                }
            } else {
                format!("{level}")
            };
            fb.print(
                &val,
                FX_X + 12,
                y,
                if on { col::ORANGE } else { col::DARK_GREY },
            );
        }

        ui::status_bar(fb, &format!("oct {} [zsxd..] i=inst spc=play", self.octave));
    }
}

/// Tracker-style note name: "c-2", "f#3", or "..." for silent steps.
fn note_name(pitch: u8, active: bool) -> String {
    if !active {
        return "...".into();
    }
    const NAMES: [&str; 12] = [
        "c-", "c#", "d-", "d#", "e-", "f-", "f#", "g-", "g#", "a-", "a#", "b-",
    ];
    format!("{}{}", NAMES[(pitch % 12) as usize], pitch / 12)
}
