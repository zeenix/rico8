//! Shared asset data models: sprite sheet, map, SFX, music, metadata.
//!
//! These are the in-memory structures every part of RICO-8 agrees on —
//! the editors mutate them, the runtime draws/plays from them, and the
//! cartridge format serializes them. Sizes are fixed on purpose: the
//! constraints are part of the console's identity.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Side length of one sprite in pixels.
pub const SPRITE_SIZE: usize = 8;
/// The sprite sheet is 128x128 pixels: 16x16 sprites = 256 sprites.
pub const SHEET_W: usize = 128;
pub const SHEET_H: usize = 128;
pub const SPRITES_PER_ROW: usize = SHEET_W / SPRITE_SIZE;
pub const SPRITE_COUNT: usize = 256;

/// Map dimensions in tiles.
pub const MAP_W: usize = 128;
pub const MAP_H: usize = 64;

/// Number of SFX slots and music patterns.
pub const SFX_COUNT: usize = 64;
pub const MUSIC_COUNT: usize = 64;
/// Notes per SFX.
pub const SFX_LEN: usize = 32;
/// Audio channels.
pub const CHANNELS: usize = 4;

/// Typed handle for a sprite on the sheet (`0..256`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpriteId(pub u8);

/// Typed handle for an SFX slot (`0..64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SfxId(pub u8);

/// Typed handle for a music pattern (`0..64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MusicId(pub u8);

/// 128x128 indexed-color sprite sheet plus one flag byte per sprite.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteSheet {
    /// One palette index per pixel, row-major, `SHEET_W * SHEET_H` long.
    pub pixels: Vec<u8>,
    /// Eight user flags per sprite, used for map layers and game logic.
    pub flags: Vec<u8>,
}

impl Default for SpriteSheet {
    fn default() -> Self {
        Self {
            pixels: vec![0; SHEET_W * SHEET_H],
            flags: vec![0; SPRITE_COUNT],
        }
    }
}

impl SpriteSheet {
    /// Read a pixel from sheet coordinates. Out of bounds returns 0.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if (0..SHEET_W as i32).contains(&x) && (0..SHEET_H as i32).contains(&y) {
            self.pixels[(y as usize) * SHEET_W + x as usize]
        } else {
            0
        }
    }

    /// Write a pixel at sheet coordinates. Out of bounds is ignored.
    pub fn set(&mut self, x: i32, y: i32, color: u8) {
        if (0..SHEET_W as i32).contains(&x) && (0..SHEET_H as i32).contains(&y) {
            self.pixels[(y as usize) * SHEET_W + x as usize] = color & 0x0f;
        }
    }

    /// Read pixel `(px, py)` of sprite `n`, where `px`/`py` may run past 8
    /// to read neighboring sprites (used by multi-sprite `spr` calls).
    pub fn sprite_pixel(&self, n: u32, px: i32, py: i32) -> u8 {
        let n = (n as usize) % SPRITE_COUNT;
        let sx = (n % SPRITES_PER_ROW * SPRITE_SIZE) as i32 + px;
        let sy = (n / SPRITES_PER_ROW * SPRITE_SIZE) as i32 + py;
        self.get(sx, sy)
    }

    /// All eight flags of sprite `n` as a bitmask.
    pub fn flags(&self, n: u32) -> u8 {
        self.flags[(n as usize) % SPRITE_COUNT]
    }

    /// Set or clear one flag (`0..8`) of sprite `n`.
    pub fn set_flag(&mut self, n: u32, flag: u8, value: bool) {
        let f = &mut self.flags[(n as usize) % SPRITE_COUNT];
        if value {
            *f |= 1 << (flag & 7);
        } else {
            *f &= !(1 << (flag & 7));
        }
    }
}

/// 128x64 tile map; each cell holds a sprite number (0 = empty).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapData {
    pub tiles: Vec<u8>,
}

impl Default for MapData {
    fn default() -> Self {
        Self {
            tiles: vec![0; MAP_W * MAP_H],
        }
    }
}

impl MapData {
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if (0..MAP_W as i32).contains(&x) && (0..MAP_H as i32).contains(&y) {
            self.tiles[(y as usize) * MAP_W + x as usize]
        } else {
            0
        }
    }

    pub fn set(&mut self, x: i32, y: i32, tile: u8) {
        if (0..MAP_W as i32).contains(&x) && (0..MAP_H as i32).contains(&y) {
            self.tiles[(y as usize) * MAP_W + x as usize] = tile;
        }
    }
}

/// Waveforms available to the synthesizer, in classic tracker spirit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Waveform {
    Triangle = 0,
    TiltedSaw = 1,
    Saw = 2,
    Square = 3,
    Pulse = 4,
    Organ = 5,
    Noise = 6,
    Phaser = 7,
}

impl Waveform {
    pub fn from_u8(v: u8) -> Self {
        match v & 7 {
            0 => Self::Triangle,
            1 => Self::TiltedSaw,
            2 => Self::Saw,
            3 => Self::Square,
            4 => Self::Pulse,
            5 => Self::Organ,
            6 => Self::Noise,
            _ => Self::Phaser,
        }
    }
}

/// Per-step note effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum SfxEffect {
    None = 0,
    Slide = 1,
    Vibrato = 2,
    Drop = 3,
    FadeIn = 4,
    FadeOut = 5,
    ArpFast = 6,
    ArpSlow = 7,
}

impl SfxEffect {
    pub fn from_u8(v: u8) -> Self {
        match v & 7 {
            0 => Self::None,
            1 => Self::Slide,
            2 => Self::Vibrato,
            3 => Self::Drop,
            4 => Self::FadeIn,
            5 => Self::FadeOut,
            6 => Self::ArpFast,
            _ => Self::ArpSlow,
        }
    }
}

/// One step of an SFX: pitch (0..64, where 33 = A-4 = 440 Hz), waveform,
/// volume (0..8, 0 = silent) and effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    /// Timbre, packed like PICO-8's SFX waveform nibble: bits 0-2 are the
    /// index and bit 3 is the *custom-instrument* flag. With the flag clear,
    /// the index (0..8) picks a built-in [`Waveform`]; with it set, the index
    /// names another SFX slot (0..8) used as a custom instrument. Use
    /// [`Note::instrument`] / [`Note::wave_index`] rather than reading the
    /// raw bits.
    pub wave: u8,
    pub volume: u8,
    pub effect: u8,
}

/// Bit 3 of [`Note::wave`]: set when the note plays another SFX as a custom
/// instrument instead of a built-in waveform.
pub const NOTE_CUSTOM_FLAG: u8 = 0x08;

impl Note {
    /// The waveform/instrument index (0..8), with the custom-instrument flag
    /// stripped off.
    pub fn wave_index(&self) -> u8 {
        self.wave & 7
    }

    /// `Some(slot)` when this note plays SFX `slot` (0..8) as a custom
    /// instrument; `None` when it uses a built-in waveform.
    pub fn instrument(&self) -> Option<u8> {
        (self.wave & NOTE_CUSTOM_FLAG != 0).then_some(self.wave & 7)
    }
}

/// A drawn custom-waveform instrument occupying SFX slots `0..8`. When present,
/// the slot is used as an instrument timbre (one signed sample per step) by
/// notes that reference it, rather than as a sequence of notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomWave {
    /// One signed sample per step; the editor draws values in `-16..=15`.
    pub samples: [i8; SFX_LEN],
    /// Pitch the waveform an octave down (PICO-8's "bass" toggle).
    pub bass: bool,
}

/// One sound effect: 32 steps played at a configurable speed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sfx {
    pub notes: [Note; SFX_LEN],
    /// Duration of one step in 1/128ths of a second (1..=255).
    pub speed: u8,
    /// Loop start step. Looping is active when `loop_end > loop_start`.
    pub loop_start: u8,
    /// Loop end step (exclusive).
    pub loop_end: u8,
    /// Per-SFX filter switches, matching PICO-8's: replace the noise voice
    /// with pure white noise.
    pub noiz: bool,
    /// Buzzier, harmonically richer timbre.
    pub buzz: bool,
    /// Detune a second voice against the first. `0` off; `1` a slight,
    /// flange-like detune; `2` an octave-ish second voice.
    pub detune: u8,
    /// Echo with a short delay. `0` off; `1`/`2` are the two delay lengths.
    pub reverb: u8,
    /// Low-pass softening. `0` off; `1`/`2` are the two strengths.
    pub dampen: u8,
    /// `Some` only for slots `0..8` that are drawn-waveform instruments.
    #[serde(default)]
    pub custom_wave: Option<CustomWave>,
}

impl Default for Sfx {
    fn default() -> Self {
        Self {
            notes: [Note::default(); SFX_LEN],
            speed: 16,
            loop_start: 0,
            loop_end: 0,
            noiz: false,
            buzz: false,
            detune: 0,
            reverb: 0,
            dampen: 0,
            custom_wave: None,
        }
    }
}

impl Sfx {
    /// True when no step is audible — used to skip empty slots.
    pub fn is_empty(&self) -> bool {
        self.notes.iter().all(|n| n.volume == 0)
    }

    /// Set the filter switches from PICO-8's packed filter byte (the 65th
    /// byte of an on-cart SFX): bit 1 noiz, bit 2 buzz, then base-3 digits
    /// for detune (÷8), reverb (÷24) and dampen (÷72). Bit 0 is PICO-8's
    /// editor mode and carries no sound, so it is ignored.
    pub fn set_filters(&mut self, byte: u8) {
        self.noiz = byte & 2 != 0;
        self.buzz = byte & 4 != 0;
        self.detune = byte / 8 % 3;
        self.reverb = byte / 24 % 3;
        self.dampen = byte / 72 % 3;
    }

    /// PICO-8's packed filter byte — the inverse of [`Sfx::set_filters`]: bit 1
    /// noiz, bit 2 buzz, then base-3 digits for detune (x8), reverb (x24) and
    /// dampen (x72). Bit 0 (PICO-8's editor mode) is always left clear.
    pub fn filters_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.noiz {
            byte |= 2;
        }
        if self.buzz {
            byte |= 4;
        }
        byte += self.detune * 8;
        byte += self.reverb * 24;
        byte += self.dampen * 72;
        byte
    }
}

/// One music pattern: an SFX slot per channel, plus flow control flags.
/// A song is a chain of patterns; playback walks forward from the started
/// pattern until it hits `stop_at_end` or loops back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicPattern {
    /// SFX index per channel; `None` leaves the channel free for game SFX.
    pub channels: [Option<u8>; CHANNELS],
    /// Jump back to the most recent `loop_start` pattern when this ends.
    pub loop_back: bool,
    /// Marks a loop target for `loop_back`.
    pub loop_start: bool,
    /// Stop the song after this pattern.
    pub stop_at_end: bool,
}

impl Default for MusicPattern {
    fn default() -> Self {
        Self {
            channels: [None; CHANNELS],
            loop_back: false,
            loop_start: false,
            stop_at_end: false,
        }
    }
}

impl MusicPattern {
    pub fn is_empty(&self) -> bool {
        self.channels.iter().all(|c| c.is_none())
    }
}

/// Cart metadata shown on the label and in the console.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub version: String,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: "untitled".into(),
            author: String::new(),
            version: "0.1.0".into(),
        }
    }
}

/// Everything a cart owns besides code: the complete asset bundle.
#[derive(Clone, Serialize, Deserialize)]
pub struct Assets {
    pub meta: Metadata,
    pub sprites: SpriteSheet,
    pub map: MapData,
    pub sfx: Vec<Sfx>,
    pub music: Vec<MusicPattern>,
    /// Optional 128x128 indexed-color label image (cart screenshot).
    pub label: Option<Vec<u8>>,
}

impl Default for Assets {
    fn default() -> Self {
        Self {
            meta: Metadata::default(),
            sprites: SpriteSheet::default(),
            map: MapData::default(),
            sfx: vec![Sfx::default(); SFX_COUNT],
            music: vec![MusicPattern::default(); MUSIC_COUNT],
            label: None,
        }
    }
}

/// Check that a bundle carries exactly the fixed-size collections RICO-8
/// requires. The editors only ever build correctly-sized bundles, but a
/// corrupted or hand-edited `assets.rico8` (or cart) can deserialize with
/// mismatched lengths; running such a bundle would panic the renderer on
/// an out-of-bounds sprite, map or label read. Every loader validates here
/// so a bad bundle fails with a clear message instead of crashing.
pub fn validate(assets: &Assets) -> Result<()> {
    if assets.sprites.pixels.len() != SHEET_W * SHEET_H
        || assets.sprites.flags.len() != SPRITE_COUNT
        || assets.map.tiles.len() != MAP_W * MAP_H
        || assets.sfx.len() != SFX_COUNT
        || assets.music.len() != MUSIC_COUNT
    {
        bail!("Cart assets have invalid dimensions");
    }
    if let Some(label) = &assets.label {
        if label.len() != SHEET_W * SHEET_H {
            bail!("Cart label has invalid dimensions");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_default_and_rejects_bad_dimensions() {
        assert!(validate(&Assets::default()).is_ok());

        let mut a = Assets::default();
        a.sprites.pixels.truncate(10);
        assert!(validate(&a).is_err(), "short sprite sheet must be rejected");

        let mut a = Assets::default();
        a.sfx.pop();
        assert!(validate(&a).is_err(), "missing sfx slot must be rejected");

        let a = Assets {
            label: Some(vec![0; 8]),
            ..Default::default()
        };
        assert!(validate(&a).is_err(), "wrong-size label must be rejected");

        let a = Assets {
            label: Some(vec![0; SHEET_W * SHEET_H]),
            ..Default::default()
        };
        assert!(validate(&a).is_ok(), "correctly-sized label is allowed");
    }

    #[test]
    fn assets_postcard_roundtrip() {
        let mut a = Assets::default();
        a.meta.name = "test cart".into();
        a.sprites.set(3, 4, 9);
        a.sprites.set_flag(1, 2, true);
        a.map.set(10, 5, 42);
        a.sfx[0].notes[0] = Note {
            pitch: 33,
            wave: 3,
            volume: 5,
            effect: 0,
        };
        a.music[0].channels[0] = Some(0);

        let bytes = postcard::to_allocvec(&a).unwrap();
        let b: Assets = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(b.meta.name, "test cart");
        assert_eq!(b.sprites.get(3, 4), 9);
        assert_eq!(b.sprites.flags(1), 0b100);
        assert_eq!(b.map.get(10, 5), 42);
        assert_eq!(b.sfx[0].notes[0].pitch, 33);
        assert_eq!(b.music[0].channels[0], Some(0));
    }

    #[test]
    fn note_custom_instrument_flag() {
        let builtin = Note {
            wave: 3,
            ..Default::default()
        };
        assert_eq!(builtin.wave_index(), 3);
        assert_eq!(builtin.instrument(), None);

        let custom = Note {
            wave: NOTE_CUSTOM_FLAG | 2,
            ..Default::default()
        };
        assert_eq!(custom.wave_index(), 2);
        assert_eq!(custom.instrument(), Some(2));
    }

    #[test]
    fn sfx_filter_byte_decodes() {
        let mut s = Sfx::default();
        // 0x86 = 2(noiz) + 4(buzz) + 8(detune 1) + 48(reverb 2) + 64(dampen ?)
        // -> detune 1, reverb 2, dampen 1; bit 0 (editor mode) ignored.
        s.set_filters(0x86);
        assert!(s.noiz && s.buzz);
        assert_eq!((s.detune, s.reverb, s.dampen), (1, 2, 1));

        let mut off = Sfx::default();
        off.set_filters(0x01); // only editor-mode bit -> no audible switches
        assert!(!off.noiz && !off.buzz);
        assert_eq!((off.detune, off.reverb, off.dampen), (0, 0, 0));
    }

    #[test]
    fn sprite_pixel_addresses_sheet() {
        let mut s = SpriteSheet::default();
        // Sprite 17 sits at sheet position (8, 8).
        s.set(8, 8, 12);
        assert_eq!(s.sprite_pixel(17, 0, 0), 12);
        assert_eq!(s.sprite_pixel(16, 8, 0), 12);
    }

    #[test]
    fn custom_wave_roundtrips_and_defaults_none() {
        // A fresh SFX has no custom waveform.
        assert!(Sfx::default().custom_wave.is_none());

        let mut a = Assets::default();
        a.sfx[0].custom_wave = Some(CustomWave {
            samples: [3; SFX_LEN],
            bass: true,
        });
        let bytes = postcard::to_allocvec(&a).unwrap();
        let b: Assets = postcard::from_bytes(&bytes).unwrap();
        let w = b.sfx[0].custom_wave.as_ref().expect("wave kept");
        assert_eq!(w.samples[0], 3);
        assert!(w.bass);
        assert!(b.sfx[1].custom_wave.is_none());
    }
}
