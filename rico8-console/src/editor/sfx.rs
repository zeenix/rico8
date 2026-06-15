//! The SFX editor, with PICO-8's three views toggled by TAB / the top-left
//! buttons: pitch mode (a graph of pitch bars + a volume strip), tracker mode
//! (a 32-step table edited with piano keys), and the wave designer (drawing a
//! custom waveform into SFX slots 0..8). Space previews the SFX.

use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, CustomWave, NOTE_CUSTOM_FLAG, SFX_LEN},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SfxMode {
    Pitch,
    Tracker,
    Wave,
}

// Tracker layout.
const STEP_Y: i32 = 19;
const COL_X: [i32; 2] = [6, 70];
const FX_X: i32 = 45;
const FX_Y: i32 = 24;
const FX_DY: i32 = 9;
const FX_LABELS: [&str; 5] = ["nz", "bz", "dt", "rv", "dm"];

// Pitch-graph + volume-strip geometry.
const G_TOP: i32 = 28;
const G_BOT: i32 = 93;
const V_TOP: i32 = 103;
const V_BOT: i32 = 117;

/// Editable fields of a tracker step.
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
    mode: SfxMode,
    /// The waveform applied when drawing pitches in pitch mode (0..8).
    wave_sel: u8,
}

impl SfxEditor {
    pub fn new() -> Self {
        Self {
            sfx: 0,
            step: 0,
            field: 0,
            octave: 2,
            mode: SfxMode::Pitch,
            wave_sel: 0,
        }
    }

    /// Select an SFX slot (used when jumping in from the music editor's pencil).
    pub fn select(&mut self, sfx: usize) {
        self.sfx = sfx % 64;
    }

    /// Whether the pitch-mode button is the lit one (for the shell's top bar).
    pub fn is_pitch(&self) -> bool {
        self.mode == SfxMode::Pitch
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
            // TAB toggles pitch <-> tracker (wave mode is left via its own button).
            Key::Tab => {
                self.mode = match self.mode {
                    SfxMode::Pitch => SfxMode::Tracker,
                    _ => SfxMode::Pitch,
                }
            }
            Key::PageUp => self.sfx = (self.sfx + 63) % 64,
            Key::PageDown => self.sfx = (self.sfx + 1) % 64,
            Key::Char(' ') => self.preview(assets, audio),
            Key::Char('-') => {
                let s = &mut assets.sfx[self.sfx];
                s.speed = s.speed.saturating_sub(1).max(1);
            }
            Key::Char('=') | Key::Char('+') => {
                let s = &mut assets.sfx[self.sfx];
                s.speed = s.speed.saturating_add(1);
            }
            _ if self.mode == SfxMode::Tracker => self.tracker_key(key, mods, assets),
            _ => {}
        }
    }

    fn tracker_key(&mut self, key: Key, mods: Mods, assets: &mut Assets) {
        match key {
            Key::Up => self.step = (self.step + SFX_LEN - 1) % SFX_LEN,
            Key::Down => self.step = (self.step + 1) % SFX_LEN,
            Key::Left => self.field = (self.field + FIELDS.len() - 1) % FIELDS.len(),
            Key::Right => self.field = (self.field + 1) % FIELDS.len(),
            Key::Delete | Key::Backspace => {
                assets.sfx[self.sfx].notes[self.step] = Default::default();
            }
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
                self.step = (self.step + 1) % SFX_LEN;
            }
            Field::Wave => {
                if c == 'i' {
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

        // Top-left mode buttons (drawn by the shell) toggle the view.
        if m.left_pressed && m.y < 8 {
            if m.over(4, 0, 12, 7) {
                self.mode = SfxMode::Pitch;
                return;
            } else if m.over(13, 0, 22, 7) {
                self.mode = SfxMode::Tracker;
                return;
            }
        }

        // Shared header: sfx selector, speed, loop/len, the wave-designer toggle.
        if m.over(3, 9, 23, 16) {
            self.sfx = (self.sfx as i32 + delta).rem_euclid(64) as usize;
            return;
        } else if m.over(49, 9, 59, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.speed = (s.speed as i32 + delta).clamp(1, 255) as u8;
            return;
        } else if m.over(90, 9, 100, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_start = (s.loop_start as i32 + delta).clamp(0, 31) as u8;
            return;
        } else if m.over(104, 9, 114, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_end = (s.loop_end as i32 + delta).clamp(0, 32) as u8;
            return;
        } else if self.sfx < 8 && m.over(118, 9, 126, 16) && m.left_pressed {
            self.mode = SfxMode::Wave;
            return;
        }

        match self.mode {
            SfxMode::Pitch => self.pitch_tick(&m, assets, audio),
            SfxMode::Tracker => self.tracker_tick(&m, assets, delta),
            SfxMode::Wave => self.wave_tick(&m, assets),
        }
    }

    fn pitch_tick(&mut self, m: &Mouse, assets: &mut Assets, audio: &AudioHandle) {
        // Waveform palette: click selects the drawing instrument.
        for w in 0..8u8 {
            let x = 46 + w as i32 * 9;
            if m.over(x, 18, x + 7, 24) && m.left_pressed {
                self.wave_sel = w;
                return;
            }
        }
        let _ = audio;
        let step = (m.x - 3) / 4;
        if !(0..SFX_LEN as i32).contains(&step) {
            return;
        }
        let note = &mut assets.sfx[self.sfx].notes[step as usize];
        // Drag in the graph sets pitch; right-click grabs the instrument.
        if (G_TOP..=G_BOT).contains(&m.y) {
            if m.right_pressed {
                if let Some(slot) = note.instrument() {
                    self.wave_sel = slot;
                }
                return;
            }
            let pitch = ((G_BOT - m.y) * 63 / (G_BOT - G_TOP)).clamp(0, 63) as u8;
            note.pitch = pitch;
            if note.volume == 0 {
                note.volume = 5;
            }
            note.wave = (note.wave & NOTE_CUSTOM_FLAG) | self.wave_sel;
        } else if (V_TOP..=V_BOT).contains(&m.y) {
            note.volume = ((V_BOT - m.y) * 7 / (V_BOT - V_TOP)).clamp(0, 7) as u8;
        }
    }

    fn tracker_tick(&mut self, m: &Mouse, assets: &mut Assets, delta: i32) {
        // Filter switches.
        for i in 0..FX_LABELS.len() {
            let y = FX_Y + i as i32 * FX_DY;
            if m.over(FX_X, y - 1, FX_X + 9, y + 5) {
                let s = &mut assets.sfx[self.sfx];
                match i {
                    0 => s.noiz = !s.noiz,
                    1 => s.buzz = !s.buzz,
                    2 => s.detune = (s.detune as i32 + delta).rem_euclid(3) as u8,
                    3 => s.reverb = (s.reverb as i32 + delta).rem_euclid(3) as u8,
                    _ => s.dampen = (s.dampen as i32 + delta).rem_euclid(3) as u8,
                }
                return;
            }
        }
        // Step grid: pick the step + field under the cursor (note cell at x+5).
        for (half, x) in COL_X.iter().enumerate() {
            if m.over(x - 5, STEP_Y, x + 37, STEP_Y + 16 * 6 - 1) {
                self.step = half * 16 + (((m.y - STEP_Y - 1).max(0) / 6) as usize).min(15);
                let cx = m.x - (x + 5);
                self.field = match cx {
                    ..=16 => 0,
                    17..=21 => 1,
                    22..=26 => 2,
                    _ => 3,
                };
            }
        }
    }

    fn wave_tick(&mut self, m: &Mouse, assets: &mut Assets) {
        if self.sfx >= 8 {
            return;
        }
        let step = (m.x - 3) / 4;
        if !(0..SFX_LEN as i32).contains(&step) || !(WAVE_TOP..=WAVE_BOT).contains(&m.y) {
            // The bass toggle sits under the canvas.
            if m.over(4, WAVE_BOT + 3, 28, WAVE_BOT + 9) && m.left_pressed {
                let s = &mut assets.sfx[self.sfx];
                let mut w = s.custom_wave.unwrap_or(CustomWave {
                    samples: [0; SFX_LEN],
                    bass: false,
                });
                w.bass = !w.bass;
                s.custom_wave = Some(w);
            }
            return;
        }
        let mid = (WAVE_TOP + WAVE_BOT) / 2;
        let value = ((mid - m.y) * 16 / ((WAVE_BOT - WAVE_TOP) / 2)).clamp(-16, 15) as i8;
        let s = &mut assets.sfx[self.sfx];
        let w = s.custom_wave.get_or_insert(CustomWave {
            samples: [0; SFX_LEN],
            bass: false,
        });
        w.samples[step as usize] = value;
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        fb.rectfill(0, 8, 127, 119, col::DARK_GREY);
        self.draw_header(fb, assets);
        match self.mode {
            SfxMode::Pitch => self.draw_pitch(fb, assets),
            SfxMode::Tracker => self.draw_tracker(fb, assets, audio),
            SfxMode::Wave => self.draw_wave(fb, assets),
        }
    }

    fn draw_header(&self, fb: &mut Framebuffer, assets: &Assets) {
        let s = &assets.sfx[self.sfx];
        ui::arrow_l(fb, 4, 11, col::PINK);
        fb.print(&format!("{:02}", self.sfx), 11, 11, col::WHITE);
        ui::arrow_r(fb, 21, 11, col::PINK);
        fb.print("spd", 35, 11, col::LIGHT_GREY);
        fb.rectfill(49, 9, 59, 16, col::BLACK);
        fb.print(&format!("{:02}", s.speed), 51, 11, col::WHITE);
        // "loop" with both bounds, or "len" when only the start is set.
        let is_len = s.loop_end == 0 && s.loop_start > 0;
        fb.print(if is_len { "len" } else { "loop" }, 70, 11, col::LIGHT_GREY);
        fb.rectfill(90, 9, 100, 16, col::BLACK);
        fb.print(&format!("{:02}", s.loop_start), 92, 11, col::WHITE);
        fb.rectfill(104, 9, 114, 16, col::BLACK);
        fb.print(&format!("{:02}", s.loop_end), 106, 11, col::WHITE);
        // Wave-designer toggle, only meaningful for the 8 instrument slots.
        if self.sfx < 8 {
            ui::blit(fb, 118, 11, &ui::WAVEI);
        }
    }

    fn draw_pitch(&self, fb: &mut Framebuffer, assets: &Assets) {
        let s = &assets.sfx[self.sfx];
        fb.print(":pitch", 3, 20, col::LAVENDER);
        ui::blit(fb, 46, 19, &ui::PALETTE);
        // Move the red selection box to the chosen waveform.
        if self.wave_sel != 0 {
            recolor_box(fb, 46, col::RED, col::LIGHT_GREY);
            recolor_box(fb, 46 + self.wave_sel as i32 * 9, col::LIGHT_GREY, col::RED);
        }
        ui::blit(fb, 117, 19, &ui::CIRCLE);

        // Black graph + volume panels.
        fb.rectfill(0, 27, 127, G_BOT, col::BLACK);
        fb.rectfill(0, V_TOP - 2, 127, 118, col::BLACK);

        for (i, note) in s.notes.iter().enumerate() {
            let x = 3 + i as i32 * 4;
            if note.volume > 0 {
                let h = (note.pitch as i32 * (G_BOT - G_TOP) / 63).max(1);
                fb.rectfill(x, G_BOT - h, x + 2, G_BOT, col::DARK_BLUE);
                fb.rectfill(x, G_BOT - h, x + 2, G_BOT - h + 1, col::RED);
            }
            // Volume marker.
            let vy = V_BOT - note.volume as i32 * (V_BOT - V_TOP) / 7;
            fb.rectfill(x, vy, x + 2, vy + 1, col::PINK);
        }

        fb.print(":volume", 3, 95, col::LAVENDER);
        for i in 0..3 {
            fb.line(118 + i * 2, 96, 118 + i * 2, 99, col::DARK_BLUE);
        }
        ui::status_bar(fb, "tab tracker  drag pitch/vol");
    }

    fn draw_tracker(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let s = &assets.sfx[self.sfx];
        let head = audio.with_synth(|sy| {
            if sy.channel_sfx()[0] == Some(self.sfx) {
                sy.channel_step()[0]
            } else {
                None
            }
        });
        let len = note_rows(s.loop_start, s.loop_end);
        let loops = s.loop_end > s.loop_start || s.loop_start > 0;
        for (half, &x) in COL_X.iter().enumerate() {
            let nx = x + 5; // note-cell origin, clear of the step gutter
            fb.rectfill(x - 5, STEP_Y - 1, x + 37, 118, col::BLACK);
            for row in 0..16usize {
                let i = half * 16 + row;
                let y = STEP_Y + 1 + row as i32 * 6;
                if i < len && loops {
                    fb.rectfill(x - 5, y, x - 4, y + 4, col::DARK_GREEN);
                }
                if head == Some(i) {
                    fb.rectfill(x - 3, y - 1, x + 34, y + 5, col::DARK_BLUE);
                }
                fb.print(&format!("{i:02}"), x - 3, y, col::DARK_GREY);
                ui::note_cell(fb, nx, y, s.notes[i]);
                // Cursor box on the focused field.
                if i == self.step {
                    let (fx0, fw) = match FIELDS[self.field] {
                        Field::Note => (nx + 2, 7),
                        Field::Wave => (nx + 15, 3),
                        Field::Vol => (nx + 20, 3),
                        Field::Fx => (nx + 24, 3),
                    };
                    fb.rect(fx0 - 1, y - 1, fx0 + fw, y + 5, col::WHITE);
                }
            }
        }
        // Filter switches strip between the columns.
        let levels = [s.noiz as u8, s.buzz as u8, s.detune, s.reverb, s.dampen];
        for (i, (label, &level)) in FX_LABELS.iter().zip(levels.iter()).enumerate() {
            let y = FX_Y + i as i32 * FX_DY;
            let on = level > 0;
            fb.print(label, FX_X, y, if on { col::WHITE } else { col::DARK_GREY });
            let val = if i < 2 {
                if on {
                    "*".into()
                } else {
                    "-".into()
                }
            } else {
                format!("{level}")
            };
            let c = if on { col::ORANGE } else { col::DARK_GREY };
            fb.print(&val, FX_X + 12, y, c);
        }
        ui::status_bar(fb, &format!("tab pitch  oct {} zsxd..", self.octave));
    }

    fn draw_wave(&self, fb: &mut Framebuffer, assets: &Assets) {
        let s = &assets.sfx[self.sfx];
        fb.print(":wave", 3, 20, col::LAVENDER);
        fb.rectfill(0, 27, 127, WAVE_BOT + 1, col::BLACK);
        let mid = (WAVE_TOP + WAVE_BOT) / 2;
        fb.line(2, mid, 125, mid, col::DARK_GREY);
        let wave = s.custom_wave.unwrap_or(CustomWave {
            samples: [0; SFX_LEN],
            bass: false,
        });
        for (i, &v) in wave.samples.iter().enumerate() {
            let x = 3 + i as i32 * 4;
            let y = mid - v as i32 * ((WAVE_BOT - WAVE_TOP) / 2) / 16;
            fb.rectfill(x, mid.min(y), x + 2, mid.max(y), col::LAVENDER);
            fb.rectfill(x, y, x + 2, y, col::WHITE);
        }
        // bass toggle.
        let on = wave.bass;
        fb.rectfill(4, WAVE_BOT + 3, 28, WAVE_BOT + 9, col::BLACK);
        fb.print(
            "bass",
            6,
            WAVE_BOT + 4,
            if on { col::ORANGE } else { col::DARK_GREY },
        );
        ui::status_bar(fb, "tab pitch  drag to draw wave");
    }
}

const WAVE_TOP: i32 = 30;
const WAVE_BOT: i32 = 104;

/// Repaint a 8x6 palette box's background colour (cols x..x+7, rows 19..24),
/// leaving the white waveform glyph untouched. Used to move the red selection.
fn recolor_box(fb: &mut Framebuffer, x: i32, from: u8, to: u8) {
    for yy in 19..25 {
        for xx in x..x + 8 {
            if fb.pget(xx, yy) == from {
                fb.pset(xx, yy, to);
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

    fn dummy() -> AudioHandle {
        AudioHandle::dummy()
    }

    #[test]
    fn tab_toggles_pitch_and_tracker() {
        let mut ed = SfxEditor::new();
        let mut a = Assets::default();
        assert_eq!(ed.mode, SfxMode::Pitch);
        ed.key(Key::Tab, Mods::default(), &mut a, &dummy());
        assert_eq!(ed.mode, SfxMode::Tracker);
        ed.key(Key::Tab, Mods::default(), &mut a, &dummy());
        assert_eq!(ed.mode, SfxMode::Pitch);
    }

    #[test]
    fn wave_tick_draws_into_custom_wave() {
        let mut ed = SfxEditor::new();
        ed.mode = SfxMode::Wave;
        let mut a = Assets::default();
        // Click near the top of the canvas at step 0 -> a high positive sample.
        let mut m = Mouse::default();
        m.left = true;
        m.left_pressed = true;
        m.x = 4;
        m.y = WAVE_TOP + 2;
        ed.wave_tick(&m, &mut a);
        let w = a.sfx[0].custom_wave.expect("wave created");
        assert!(
            w.samples[0] > 8,
            "drawn sample should be high, got {}",
            w.samples[0]
        );
    }
}
