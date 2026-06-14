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
#[derive(Clone, Serialize, Deserialize)]
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
#[derive(Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub wave: u8,
    pub volume: u8,
    pub effect: u8,
}

/// One sound effect: 32 steps played at a configurable speed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sfx {
    pub notes: [Note; SFX_LEN],
    /// Duration of one step in 1/128ths of a second (1..=255).
    pub speed: u8,
    /// Loop start step. Looping is active when `loop_end > loop_start`.
    pub loop_start: u8,
    /// Loop end step (exclusive).
    pub loop_end: u8,
}

impl Default for Sfx {
    fn default() -> Self {
        Self {
            notes: [Note::default(); SFX_LEN],
            speed: 16,
            loop_start: 0,
            loop_end: 0,
        }
    }
}

impl Sfx {
    /// True when no step is audible — used to skip empty slots.
    pub fn is_empty(&self) -> bool {
        self.notes.iter().all(|n| n.volume == 0)
    }
}

/// One music pattern: an SFX slot per channel, plus flow control flags.
/// A song is a chain of patterns; playback walks forward from the started
/// pattern until it hits `stop_at_end` or loops back.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
        bail!("cart assets have invalid dimensions");
    }
    if let Some(label) = &assets.label {
        if label.len() != SHEET_W * SHEET_H {
            bail!("cart label has invalid dimensions");
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
    fn sprite_pixel_addresses_sheet() {
        let mut s = SpriteSheet::default();
        // Sprite 17 sits at sheet position (8, 8).
        s.set(8, 8, 12);
        assert_eq!(s.sprite_pixel(17, 0, 0), 12);
        assert_eq!(s.sprite_pixel(16, 8, 0), 12);
    }
}
