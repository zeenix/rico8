//! The SFX editor, with PICO-8's three views toggled by TAB / the top-left
//! buttons: pitch mode (a graph of pitch bars + a volume strip), tracker mode
//! (a 32-step table edited with piano keys), and the wave designer (drawing a
//! custom waveform into SFX slots 0..8). Space previews the SFX.

use super::history::History;
use crate::{
    shell::{Key, Mods},
    ui::{self, Mouse},
};
use rico8_runtime::{
    assets::{Assets, CustomWave, Note, Sfx, NOTE_CUSTOM_FLAG, SFX_LEN},
    audio::AudioHandle,
    fb::Framebuffer,
    palette::col,
    pico8::{self, Pasted},
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
    /// Pitch-mode palette shown as numbers (toggled by the circle button)
    /// rather than waveform glyphs.
    palette_numeric: bool,
    status: ui::StatusMsg,
    /// Undo/redo of the whole SFX bank (last 10 edits).
    history: History<Vec<Sfx>>,
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
            palette_numeric: false,
            status: ui::StatusMsg::default(),
            history: History::new(),
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
        // In the wave designer, play a sustained note *through* the drawn
        // waveform (the slot's own notes don't use its waveform), so the user
        // can actually hear the waveform they are drawing.
        let (sfx, target) = if self.mode == SfxMode::Wave {
            let scratch = if self.sfx == 63 { 62 } else { 63 };
            let mut sfx = assets.sfx.clone();
            let probe = Note {
                pitch: 33,
                wave: NOTE_CUSTOM_FLAG | self.sfx as u8,
                volume: 5,
                effect: 0,
            };
            sfx[scratch] = Sfx {
                notes: [probe; SFX_LEN],
                ..Sfx::default()
            };
            (sfx, scratch)
        } else {
            (assets.sfx.clone(), self.sfx)
        };
        audio.load(sfx, assets.music.clone());
        let playing = audio.with_synth(|s| s.channel_sfx()[0]);
        if playing == Some(target) {
            audio.play_sfx(-1, 0);
        } else {
            audio.play_sfx(target as i32, 0);
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets, audio: &AudioHandle) {
        if mods.ctrl {
            if let Key::Char(c) = key {
                match c.to_ascii_lowercase() {
                    'z' if mods.shift => {
                        self.history.redo(&mut assets.sfx);
                        return;
                    }
                    'z' => {
                        self.history.undo(&mut assets.sfx);
                        return;
                    }
                    'y' => {
                        self.history.redo(&mut assets.sfx);
                        return;
                    }
                    _ => {}
                }
            }
        }
        // Snapshot before, commit after: a key edit is one undo step, and the
        // compare drops keys that change nothing (Tab, navigation, preview).
        self.history.begin(&assets.sfx);
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
        self.history.commit(&assets.sfx);
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
        self.status.tick();
        let _ = audio;
        let m = *mouse;
        // Bracket each mouse gesture for undo. Edits land on the press/hold
        // frames (spinners, drag-drawing), never on release, so a snapshot taken
        // while the button is down and committed once it comes up captures the
        // whole gesture. The commit compares, so non-editing clicks (mode
        // buttons, slot selection) record nothing.
        if m.left || m.right {
            self.history.begin(&assets.sfx);
        } else {
            self.history.commit(&assets.sfx);
        }
        // Press-edge controls: buttons, spinners, selectors, palette.
        if (m.left_pressed || m.right_pressed) && self.handle_press(&m, assets) {
            return;
        }
        // Click-and-drag drawing on the graph / volume strip / wave canvas.
        if m.left {
            match self.mode {
                SfxMode::Pitch => self.pitch_draw(&m, assets),
                SfxMode::Wave => self.wave_draw(&m, assets),
                SfxMode::Tracker => {}
            }
        }
    }

    /// Handle a press on a button/spinner/selector. Returns `true` when it
    /// consumed the click (so drag-drawing is skipped).
    fn handle_press(&mut self, m: &Mouse, assets: &mut Assets) -> bool {
        let delta: i32 = if m.right_pressed { -1 } else { 1 };
        // Top-left mode buttons (drawn by the shell).
        if m.left_pressed && m.y < 8 {
            if m.over(4, 0, 12, 7) {
                self.mode = SfxMode::Pitch;
                return true;
            } else if m.over(13, 0, 22, 7) {
                self.mode = SfxMode::Tracker;
                return true;
            }
        }
        // SFX selector: left arrow decrements, right arrow increments, and a
        // click on the number itself nudges by the click's direction.
        if m.left_pressed && m.over(4, 9, 8, 16) {
            self.sfx = (self.sfx + 63) % 64;
            return true;
        } else if m.left_pressed && m.over(20, 9, 25, 16) {
            self.sfx = (self.sfx + 1) % 64;
            return true;
        } else if m.over(9, 9, 19, 16) {
            self.sfx = (self.sfx as i32 + delta).rem_euclid(64) as usize;
            return true;
        } else if m.over(49, 9, 59, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.speed = (s.speed as i32 + delta).clamp(1, 255) as u8;
            return true;
        } else if m.over(90, 9, 100, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_start = (s.loop_start as i32 + delta).clamp(0, 31) as u8;
            return true;
        } else if m.over(104, 9, 114, 16) {
            let s = &mut assets.sfx[self.sfx];
            s.loop_end = (s.loop_end as i32 + delta).clamp(0, 32) as u8;
            return true;
        } else if self.sfx < 8 && m.left_pressed && m.over(118, 9, 126, 16) {
            // The wave toggle flips between the wave designer and pitch view.
            self.mode = if self.mode == SfxMode::Wave {
                SfxMode::Pitch
            } else {
                SfxMode::Wave
            };
            return true;
        }
        match self.mode {
            SfxMode::Pitch => {
                // The circle toggles the palette between waveform glyphs and
                // plain numbers.
                if m.left_pressed && m.over(117, 18, 125, 25) {
                    self.palette_numeric = !self.palette_numeric;
                    return true;
                }
                // Palette: click selects the drawing instrument.
                for w in 0..8u8 {
                    let x = 46 + w as i32 * 9;
                    if m.left_pressed && m.over(x, 18, x + 7, 24) {
                        self.wave_sel = w;
                        return true;
                    }
                }
                // Right-click in the graph grabs that note's instrument.
                let step = (m.x - 3) / 4;
                if m.right_pressed
                    && (0..SFX_LEN as i32).contains(&step)
                    && (G_TOP..=G_BOT).contains(&m.y)
                {
                    if let Some(slot) = assets.sfx[self.sfx].notes[step as usize].instrument() {
                        self.wave_sel = slot;
                    }
                    return true;
                }
                false
            }
            SfxMode::Tracker => {
                self.tracker_tick(m, assets, delta);
                true
            }
            SfxMode::Wave => {
                // bass toggle under the canvas.
                if m.left_pressed && m.over(4, WAVE_BOT + 3, 28, WAVE_BOT + 9) {
                    let s = &mut assets.sfx[self.sfx];
                    let mut w = s.custom_wave.unwrap_or(CustomWave {
                        samples: [0; SFX_LEN],
                        bass: false,
                    });
                    w.bass = !w.bass;
                    s.custom_wave = Some(w);
                    return true;
                }
                false
            }
        }
    }

    /// Drag on the pitch graph (sets pitch with the selected instrument) or on
    /// the volume strip (sets volume).
    fn pitch_draw(&mut self, m: &Mouse, assets: &mut Assets) {
        let step = (m.x - 3) / 4;
        if !(0..SFX_LEN as i32).contains(&step) {
            return;
        }
        let note = &mut assets.sfx[self.sfx].notes[step as usize];
        if (G_TOP..=G_BOT).contains(&m.y) {
            note.pitch = ((G_BOT - m.y) * 63 / (G_BOT - G_TOP)).clamp(0, 63) as u8;
            if note.volume == 0 {
                note.volume = 5;
            }
            note.wave = (note.wave & NOTE_CUSTOM_FLAG) | self.wave_sel;
        } else if (V_TOP..=V_BOT).contains(&m.y) {
            note.volume = ((V_BOT - m.y) * 7 / (V_BOT - V_TOP)).clamp(0, 7) as u8;
        }
    }

    /// Drag on the wave-designer canvas to draw the waveform samples.
    fn wave_draw(&mut self, m: &Mouse, assets: &mut Assets) {
        let step = (m.x - 3) / 4;
        if self.sfx >= 8
            || !(0..SFX_LEN as i32).contains(&step)
            || !(WAVE_TOP..=WAVE_BOT).contains(&m.y)
        {
            return;
        }
        let mid = (WAVE_TOP + WAVE_BOT) / 2;
        let value = ((mid - m.y) * 16 / ((WAVE_BOT - WAVE_TOP) / 2)).clamp(-16, 15) as i8;
        let w = assets.sfx[self.sfx].custom_wave.get_or_insert(CustomWave {
            samples: [0; SFX_LEN],
            bass: false,
        });
        w.samples[step as usize] = value;
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

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        fb.rectfill(0, 8, 127, 119, col::DARK_GREY);
        self.draw_header(fb, assets);
        match self.mode {
            SfxMode::Pitch => self.draw_pitch(fb, assets, audio),
            SfxMode::Tracker => self.draw_tracker(fb, assets, audio),
            SfxMode::Wave => self.draw_wave(fb, assets),
        }
    }

    fn draw_header(&self, fb: &mut Framebuffer, assets: &Assets) {
        let s = &assets.sfx[self.sfx];
        ui::arrow_l(fb, 4, 11, col::PINK);
        fb.print(&format!("{:02}", self.sfx), 11, 11, col::WHITE);
        ui::arrow_r(fb, 21, 11, col::PINK);
        fb.print("Spd", 35, 11, col::LIGHT_GREY);
        fb.rectfill(49, 9, 59, 16, col::BLACK);
        fb.print(&format!("{:02}", s.speed), 51, 11, col::WHITE);
        // "loop" with both bounds, or "len" when only the start is set.
        let is_len = s.loop_end == 0 && s.loop_start > 0;
        fb.print(if is_len { "Len" } else { "Loop" }, 70, 11, col::LIGHT_GREY);
        fb.rectfill(90, 9, 100, 16, col::BLACK);
        fb.print(&format!("{:02}", s.loop_start), 92, 11, col::WHITE);
        fb.rectfill(104, 9, 114, 16, col::BLACK);
        fb.print(&format!("{:02}", s.loop_end), 106, 11, col::WHITE);
        // Wave-designer toggle, only meaningful for the 8 instrument slots.
        if self.sfx < 8 {
            ui::blit(fb, 118, 11, &ui::WAVEI);
        }
    }

    fn draw_pitch(&self, fb: &mut Framebuffer, assets: &Assets, audio: &AudioHandle) {
        let s = &assets.sfx[self.sfx];
        let head = audio.with_synth(|sy| {
            if sy.channel_sfx()[0] == Some(self.sfx) {
                sy.channel_step()[0]
            } else {
                None
            }
        });
        fb.print(":Pitch", 3, 20, col::LAVENDER);
        self.draw_palette(fb);
        ui::blit(fb, 117, 19, &ui::CIRCLE);

        // Black graph + volume panels.
        fb.rectfill(0, 27, 127, G_BOT, col::BLACK);
        fb.rectfill(0, V_TOP - 2, 127, 118, col::BLACK);

        for (i, note) in s.notes.iter().enumerate() {
            let x = 3 + i as i32 * 4;
            let playing = head == Some(i);
            if note.volume > 0 {
                // PICO-8 draws dark-blue bars and colours the marker on top by
                // the note's waveform/instrument; the playing step is yellow.
                let bar = if playing { col::YELLOW } else { col::DARK_BLUE };
                let h = (note.pitch as i32 * (G_BOT - G_TOP) / 63).max(1);
                fb.rectfill(x, G_BOT - h, x + 2, G_BOT, bar);
                let tip = if playing {
                    col::WHITE
                } else {
                    wave_color(note)
                };
                fb.rectfill(x, G_BOT - h, x + 2, G_BOT - h + 1, tip);
            }
            // Volume marker.
            let vy = V_BOT - note.volume as i32 * (V_BOT - V_TOP) / 7;
            let vc = if playing { col::YELLOW } else { col::PINK };
            fb.rectfill(x, vy, x + 2, vy + 1, vc);
        }

        fb.print(":Volume", 3, 95, col::LAVENDER);
        for i in 0..3 {
            fb.line(118 + i * 2, 96, 118 + i * 2, 99, col::DARK_BLUE);
        }
        self.status.show(fb, "Tab tracker  drag pitch/vol");
    }

    /// The waveform palette: 8 boxes (graphical glyphs or plain numbers), with
    /// the selected waveform's box in red.
    fn draw_palette(&self, fb: &mut Framebuffer) {
        if self.palette_numeric {
            for w in 0..8u8 {
                let x = 46 + w as i32 * 9;
                let sel = w == self.wave_sel;
                fb.rectfill(
                    x,
                    18,
                    x + 7,
                    24,
                    if sel { col::RED } else { col::LIGHT_GREY },
                );
                let c = if sel { col::WHITE } else { col::DARK_GREY };
                fb.print(&format!("{w}"), x + 2, 19, c);
            }
        } else {
            ui::blit(fb, 46, 19, &ui::PALETTE);
            // Move the red selection box to the chosen waveform.
            if self.wave_sel != 0 {
                recolor_box(fb, 46, col::RED, col::LIGHT_GREY);
                recolor_box(fb, 46 + self.wave_sel as i32 * 9, col::LIGHT_GREY, col::RED);
            }
        }
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
        self.status
            .show(fb, &format!("Tab pitch  oct {} zsxd..", self.octave));
    }

    fn draw_wave(&self, fb: &mut Framebuffer, assets: &Assets) {
        let s = &assets.sfx[self.sfx];
        fb.print(":Wave", 3, 20, col::LAVENDER);
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
            "Bass",
            6,
            WAVE_BOT + 4,
            if on { col::ORANGE } else { col::DARK_GREY },
        );
        self.status.show(fb, "Tab pitch  drag to draw wave");
    }

    /// Set a transient bottom-bar message (used for clipboard errors).
    pub fn set_status(&mut self, msg: String) {
        self.status.set(msg);
    }

    /// Paste a decoded PICO-8 clipboard blob. Only `[sfx]` applies here: its
    /// records overwrite consecutive slots from the selected one.
    pub fn paste(&mut self, pasted: &Pasted, assets: &mut Assets) {
        match pasted {
            Pasted::Sfx(clip) => {
                let report = pico8::paste_sfx(&mut assets.sfx, &clip.records, self.sfx);
                self.status.set(report.summary);
            }
            Pasted::Sprites(_) => self.status.set("sprites - use sprite editor".into()),
        }
    }
}

const WAVE_TOP: i32 = 30;
const WAVE_BOT: i32 = 104;

/// A distinct colour per built-in waveform (0..8), so pitch-graph bars show
/// which instrument each note uses.
const WAVE_COLS: [u8; 8] = [
    col::DARK_BLUE,
    col::BLUE,
    col::LAVENDER,
    col::DARK_GREEN,
    col::GREEN,
    col::ORANGE,
    col::PINK,
    col::LIGHT_GREY,
];

/// The pitch-bar colour for a note: its waveform's colour, or peach when it
/// references a custom instrument.
fn wave_color(note: &Note) -> u8 {
    if note.instrument().is_some() {
        col::PEACH
    } else {
        WAVE_COLS[note.wave_index() as usize]
    }
}

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
    fn wave_preview_plays_through_the_drawn_waveform() {
        use rico8_runtime::assets::SFX_LEN;
        let mut a = Assets::default();
        a.sfx[0].custom_wave = Some(CustomWave {
            samples: [15; SFX_LEN],
            bass: false,
        });
        let mut ed = SfxEditor::new();
        ed.mode = SfxMode::Wave;
        ed.sfx = 0;
        let audio = dummy();
        ed.preview(&a, &audio);
        let mut peak = 0.0f32;
        audio.with_synth(|s| {
            for _ in 0..2000 {
                peak = peak.max(s.next_sample().abs());
            }
        });
        assert!(peak > 0.01, "drawing into the waveform should be audible");
    }

    fn ctrl(shift: bool) -> Mods {
        Mods {
            ctrl: true,
            shift,
            ..Default::default()
        }
    }

    #[test]
    fn undo_and_redo_a_speed_change() {
        let mut ed = SfxEditor::new();
        let mut a = Assets::default();
        let start = a.sfx[0].speed;
        ed.key(Key::Char('='), Mods::default(), &mut a, &dummy());
        assert_eq!(a.sfx[0].speed, start + 1);
        ed.key(Key::Char('z'), ctrl(false), &mut a, &dummy());
        assert_eq!(a.sfx[0].speed, start, "undo restores the speed");
        ed.key(Key::Char('z'), ctrl(true), &mut a, &dummy()); // Ctrl+Shift+Z
        assert_eq!(a.sfx[0].speed, start + 1, "redo re-applies it");
    }

    #[test]
    fn undo_reverts_a_wave_drag() {
        let mut ed = SfxEditor::new();
        ed.mode = SfxMode::Wave;
        let mut a = Assets::default();
        let m = Mouse {
            x: 4,
            y: WAVE_TOP + 2,
            left: true,
            left_pressed: true,
            ..Default::default()
        };
        ed.tick(&m, &mut a, &dummy());
        ed.tick(&Mouse::default(), &mut a, &dummy()); // release closes the stroke.
        assert!(a.sfx[0].custom_wave.is_some());
        ed.key(Key::Char('z'), ctrl(false), &mut a, &dummy());
        assert!(
            a.sfx[0].custom_wave.is_none(),
            "undo removes the drawn waveform"
        );
    }

    #[test]
    fn wave_drag_draws_into_custom_wave() {
        let mut ed = SfxEditor::new();
        ed.mode = SfxMode::Wave;
        let mut a = Assets::default();
        // Drag near the top of the canvas at step 0 -> a high positive sample.
        let m = Mouse {
            x: 4,
            y: WAVE_TOP + 2,
            left: true,
            left_pressed: true,
            ..Default::default()
        };
        ed.tick(&m, &mut a, &dummy());
        let w = a.sfx[0].custom_wave.expect("wave created");
        assert!(
            w.samples[0] > 8,
            "drawn sample should be high, got {}",
            w.samples[0]
        );
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    use rico8_runtime::pico8::{Pasted, SfxClip, Slotted};

    fn one_sfx(pitch: u8) -> Sfx {
        let mut s = Sfx::default();
        s.notes[0].pitch = pitch;
        s.notes[0].volume = 4;
        s
    }

    #[test]
    fn pastes_sfx_from_selected_slot() {
        let mut ed = SfxEditor::new();
        ed.select(7);
        let mut assets = Assets::default();
        let clip = SfxClip {
            records: vec![Slotted {
                src: 0,
                value: one_sfx(33),
            }],
            patterns: vec![],
        };
        ed.paste(&Pasted::Sfx(clip), &mut assets);
        assert_eq!(assets.sfx[7].notes[0].pitch, 33);
        assert!(ed.status.current().unwrap().contains("SFX 7"));
    }
}
