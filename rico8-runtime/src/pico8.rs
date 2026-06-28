//! Importing PICO-8 cartridge assets.
//!
//! RICO-8 is a loving homage to [PICO-8](https://www.lexaloffle.com/pico-8.php):
//! it borrows the very same 16-color palette, the same eight chip-tune
//! waveforms and per-step effects, and the same 128x128 sprite sheet and
//! 16-wide sprite layout. That overlap means a PICO-8 cart's *assets* —
//! graphics, sprite flags, map, sound effects and music — transfer into a
//! RICO-8 project almost one-to-one. This module does exactly that.
//!
//! Only the assets are imported. PICO-8 games are written in Lua and RICO-8
//! games in Rust, so the cart's code is ignored entirely: the new project
//! gets a stub `src/lib.rs` to build against, and the game logic is yours to
//! write in Rust against the imported art and audio.
//!
//! Two input formats are understood:
//!
//! - **`.p8`** — the plain-text cartridge: labelled `__gfx__`, `__gff__`, `__label__`, `__map__`,
//!   `__sfx__` and `__music__` sections of hex (the `__lua__` section is skipped).
//! - **`.p8.png`** — the PNG cartridge: the game's 32 KiB ROM hidden two bits at a time in the low
//!   bits of each pixel's A/R/G/B channels. The image is decoded, the ROM reassembled, and the same
//!   fixed memory map PICO-8 uses (`0x0000` gfx, `0x2000` map, `0x3000` flags, `0x3100` music,
//!   `0x3200` sfx) is read out.

use crate::{
    assets::{
        self, Assets, MapData, MusicPattern, Note, Sfx, SpriteSheet, MAP_H, MAP_W, MUSIC_COUNT,
        NOTE_CUSTOM_FLAG, SFX_COUNT, SFX_LEN, SHEET_H, SHEET_W, SPRITES_PER_ROW, SPRITE_COUNT,
        SPRITE_SIZE,
    },
    project::Project,
};
use anyhow::{anyhow, bail, Result};
use std::{collections::HashMap, path::Path};

mod clipboard;
pub use clipboard::{
    parse_clipboard, paste_pattern, paste_sfx, paste_sprites, PasteReport, Pasted, PixelRect,
    SfxClip, Slotted,
};

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// PICO-8 cart image dimensions, fixed by the format.
const PICO8_PNG_W: usize = 160;
const PICO8_PNG_H: usize = 205;

/// Size of the addressable cart ROM (gfx + map + flags + music + sfx).
const ROM_LEN: usize = 0x4300;
/// Rows the explicit PICO-8 map covers: the top 32 rows. The bottom 32 rows
/// alias the shared sprite memory (`0x1000..0x2000`); we bring that region
/// across as map rows 32..64 too — see [`fill_shared_map`].
const PICO8_MAP_ROWS: usize = 32;
/// Start of PICO-8's shared region: the bottom half of the sprite sheet,
/// which doubles as the bottom 32 rows of the map.
const SHARED_BASE: usize = 0x1000;
/// Bytes per SFX in cart memory: 32 notes x 2 + 4 metadata bytes.
const SFX_MEM_LEN: usize = 68;

/// Stub `src/lib.rs` scaffolded for an imported cart. The assets are real;
/// the game logic is the user's to write in Rust.
pub const IMPORT_TEMPLATE: &str = r#"#![no_std]
//! Imported from a PICO-8 cartridge.
//!
//! The graphics, map, sound and music came across intact — open the
//! sprite/map/sfx/music editors to see them. Only the assets were imported;
//! write your game logic in Rust here.
use rico8::*;

#[derive(Default)]
struct Cart;

impl Game for Cart {
    fn update(&mut self, _ctx: &mut Context) {}

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("Imported from PICO-8", 12, 54, Color::WHITE);
        gfx.print("Write your game here", 16, 64, Color::LIGHT_GREY);
    }
}

rico8::game!(Cart);
"#;

/// Which source assets to append into a destination cart. Indices are the
/// source PICO-8 cart's own indices (sprite 0..256, SFX/music 0..64).
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub sprites: Vec<u8>,
    pub sfx: Vec<u8>,
    pub music: Vec<u8>,
}

impl Selection {
    /// Build a selection from the range strings as typed on the command line,
    /// e.g. `Selection::parse(Some("0-15,32"), Some("0-3"), None)`. At least
    /// one kind must be given, or this is an error. Each string is parsed with
    /// inclusive ranges and validated against its kind's maximum.
    pub fn parse(sprites: Option<&str>, sfx: Option<&str>, music: Option<&str>) -> Result<Self> {
        if sprites.is_none() && sfx.is_none() && music.is_none() {
            bail!("select at least one of sprites, SFX or music to import");
        }
        Ok(Self {
            sprites: sprites
                .map(|s| parse_index_ranges(s, SPRITE_COUNT))
                .transpose()?
                .unwrap_or_default(),
            sfx: sfx
                .map(|s| parse_index_ranges(s, SFX_COUNT))
                .transpose()?
                .unwrap_or_default(),
            music: music
                .map(|s| parse_index_ranges(s, MUSIC_COUNT))
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

/// Where one kind's appended items landed in the destination.
#[derive(Debug, Clone, Copy, Default)]
pub struct Placement {
    /// First destination slot written (meaningful only when `count > 0`).
    pub start: usize,
    /// Number of items appended.
    pub count: usize,
}

impl Placement {
    /// A human line like `"12 sprites at slots 16 to 27"`, or `None` if nothing
    /// of this kind was appended.
    fn describe(&self, kind: &str) -> Option<String> {
        (self.count > 0).then(|| {
            format!(
                "{} {kind} at slots {} to {}",
                self.count,
                self.start,
                self.start + self.count - 1
            )
        })
    }
}

/// The outcome of an append: where each kind landed, plus any warnings (e.g.
/// references left pointing at slots that were not part of the selection).
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub sprites: Placement,
    pub sfx: Placement,
    pub music: Placement,
    pub warnings: Vec<String>,
}

impl Report {
    /// One line per kind that had anything appended.
    pub fn summary_lines(&self) -> Vec<String> {
        [
            self.sprites.describe("sprites"),
            self.sfx.describe("SFX"),
            self.music.describe("music"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Append the selected `src` assets into `dest`, after `dest`'s last used slot
/// of each kind. Imported audio cross-references (music channels and SFX
/// custom-instrument notes) are remapped to the slots their targets landed in;
/// references to slots that were not selected are left as-is and reported as
/// warnings. `dest` is only mutated on success (a capacity or validation error
/// leaves it untouched).
pub fn append_pico8_assets(dest: &mut Assets, src: &Assets, sel: &Selection) -> Result<Report> {
    // Guard against out-of-range indices so a hand-built selection can't panic
    // on an indexing operation below (sprite indices are u8, always in range).
    if let Some(&s) = sel.sfx.iter().find(|&&s| s as usize >= SFX_COUNT) {
        bail!("SFX index {s} is out of range (below {SFX_COUNT})");
    }
    if let Some(&m) = sel.music.iter().find(|&&m| m as usize >= MUSIC_COUNT) {
        bail!("music index {m} is out of range (below {MUSIC_COUNT})");
    }

    // Build into a clone; commit only after everything succeeds.
    let mut out = dest.clone();
    let mut warnings = Vec::new();

    // --- Sprites: copy the 8x8 block and the flag byte. No references. ---
    let spr_start = next_free_sprite(&out.sprites);
    if spr_start + sel.sprites.len() > SPRITE_COUNT {
        bail!(
            "not enough room for {} sprites: {} of {} slots free",
            sel.sprites.len(),
            SPRITE_COUNT - spr_start,
            SPRITE_COUNT
        );
    }
    for (i, &s) in sel.sprites.iter().enumerate() {
        copy_sprite(&mut out.sprites, spr_start + i, &src.sprites, s as usize);
    }
    let sprites = Placement {
        start: spr_start,
        count: sel.sprites.len(),
    };

    // --- SFX: copy, then remap each copy's custom-instrument note refs. ---
    let sfx_start = next_free_sfx(&out.sfx);
    if sfx_start + sel.sfx.len() > SFX_COUNT {
        bail!(
            "not enough room for {} SFX: {} of {} slots free",
            sel.sfx.len(),
            SFX_COUNT - sfx_start,
            SFX_COUNT
        );
    }
    // Source SFX index -> destination slot, for remapping references.
    let mut sfx_map: HashMap<u8, usize> = HashMap::new();
    for (i, &s) in sel.sfx.iter().enumerate() {
        out.sfx[sfx_start + i] = src.sfx[s as usize].clone();
        sfx_map.insert(s, sfx_start + i);
    }
    for (i, &s) in sel.sfx.iter().enumerate() {
        remap_custom_instruments(
            &mut out.sfx[sfx_start + i].notes,
            &sfx_map,
            &format!("SFX {s}"),
            &mut warnings,
        );
    }
    let sfx = Placement {
        start: sfx_start,
        count: sel.sfx.len(),
    };

    // --- Music: copy, then remap each channel's SFX reference. ---
    let mus_start = next_free_music(&out.music);
    if mus_start + sel.music.len() > MUSIC_COUNT {
        bail!(
            "not enough room for {} music patterns: {} of {} slots free",
            sel.music.len(),
            MUSIC_COUNT - mus_start,
            MUSIC_COUNT
        );
    }
    for (i, &m) in sel.music.iter().enumerate() {
        let mut pat = src.music[m as usize];
        remap_music_channels(&mut pat, &sfx_map, &format!("music {m}"), &mut warnings);
        out.music[mus_start + i] = pat;
    }
    let music = Placement {
        start: mus_start,
        count: sel.music.len(),
    };

    assets::validate(&out)?;
    *dest = out;
    Ok(Report {
        sprites,
        sfx,
        music,
        warnings,
    })
}

/// The first free sprite slot: one past the highest used sprite, or 0 if none
/// is used. A sprite is "used" when its 8x8 block has any non-zero pixel or its
/// flag byte is non-zero.
fn next_free_sprite(sheet: &SpriteSheet) -> usize {
    (0..SPRITE_COUNT)
        .rev()
        .find(|&n| sprite_used(sheet, n))
        .map(|n| n + 1)
        .unwrap_or(0)
}

/// True when sprite `n` has any non-zero pixel or a non-zero flag byte.
fn sprite_used(sheet: &SpriteSheet, n: usize) -> bool {
    if sheet.flags[n] != 0 {
        return true;
    }
    let sx = (n % SPRITES_PER_ROW) * SPRITE_SIZE;
    let sy = (n / SPRITES_PER_ROW) * SPRITE_SIZE;
    (0..SPRITE_SIZE)
        .any(|dy| (0..SPRITE_SIZE).any(|dx| sheet.pixels[(sy + dy) * SHEET_W + (sx + dx)] != 0))
}

/// The first free SFX slot: one past the highest non-empty or custom-wave SFX, or 0.
fn next_free_sfx(sfx: &[Sfx]) -> usize {
    (0..SFX_COUNT)
        .rev()
        .find(|&i| !sfx[i].is_empty() || sfx[i].custom_wave.is_some())
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// The first free music slot: one past the highest non-empty or flow-control pattern, or 0.
fn next_free_music(music: &[MusicPattern]) -> usize {
    (0..MUSIC_COUNT)
        .rev()
        .find(|&i| {
            let p = &music[i];
            !p.is_empty() || p.loop_back || p.loop_start || p.stop_at_end
        })
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// Copy sprite `src_n`'s 8x8 block and flag byte into `dst_n`.
fn copy_sprite(dst: &mut SpriteSheet, dst_n: usize, src: &SpriteSheet, src_n: usize) {
    let dx0 = (dst_n % SPRITES_PER_ROW) * SPRITE_SIZE;
    let dy0 = (dst_n / SPRITES_PER_ROW) * SPRITE_SIZE;
    let sx0 = (src_n % SPRITES_PER_ROW) * SPRITE_SIZE;
    let sy0 = (src_n / SPRITES_PER_ROW) * SPRITE_SIZE;
    for dy in 0..SPRITE_SIZE {
        for dx in 0..SPRITE_SIZE {
            dst.pixels[(dy0 + dy) * SHEET_W + (dx0 + dx)] =
                src.pixels[(sy0 + dy) * SHEET_W + (sx0 + dx)];
        }
    }
    dst.flags[dst_n] = src.flags[src_n];
}

/// Remap custom-instrument note refs in `notes` through `sfx_map` (source SFX
/// slot to destination slot). A note whose instrument landed outside slots 0-7,
/// or was not among the mapped SFX, is left as-is and a warning is pushed.
/// `label` names the SFX in warnings (e.g. `"SFX 3"`).
fn remap_custom_instruments(
    notes: &mut [Note],
    sfx_map: &HashMap<u8, usize>,
    label: &str,
    warnings: &mut Vec<String>,
) {
    for note in notes.iter_mut() {
        let Some(inst) = note.instrument() else {
            continue;
        };
        match sfx_map.get(&inst) {
            // A custom instrument can only be addressed in slots 0..8.
            Some(&dst) if dst <= 7 => note.wave = NOTE_CUSTOM_FLAG | dst as u8,
            Some(&dst) => warnings.push(format!(
                "{label}: custom instrument landed in slot {dst}, which a note \
                 can't reference (only slots 0-7); left pointing at slot {inst}"
            )),
            None => warnings.push(format!(
                "{label}: custom instrument {inst} was not imported; left \
                 pointing at slot {inst}"
            )),
        }
    }
}

/// Remap a music pattern's channel SFX refs through `sfx_map`. A channel whose
/// SFX was not among the mapped SFX is left as-is and a warning is pushed.
fn remap_music_channels(
    pat: &mut MusicPattern,
    sfx_map: &HashMap<u8, usize>,
    label: &str,
    warnings: &mut Vec<String>,
) {
    for ch in pat.channels.iter_mut() {
        let Some(old) = *ch else { continue };
        match sfx_map.get(&old) {
            Some(&dst) => *ch = Some(dst as u8),
            None => warnings.push(format!(
                "{label}: channel SFX {old} was not imported; left pointing \
                 at slot {old}"
            )),
        }
    }
}

/// Parse a comma-separated list of indices and inclusive ranges (e.g.
/// `"0-15,32,40-43"`) into a sorted, deduped list, validating every value is
/// below `max`.
fn parse_index_ranges(s: &str, max: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for tok in s.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            bail!("empty index in selection \"{s}\"");
        }
        let (lo, hi) = match tok.split_once('-') {
            Some((a, b)) => (parse_one(a, max)?, parse_one(b, max)?),
            None => {
                let v = parse_one(tok, max)?;
                (v, v)
            }
        };
        if hi < lo {
            bail!("reversed range \"{tok}\": start {lo} is past end {hi}");
        }
        out.extend(lo..=hi);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Parse one index, requiring it to be a number below `max`.
fn parse_one(s: &str, max: usize) -> Result<u8> {
    let v: usize = s
        .trim()
        .parse()
        .map_err(|_| anyhow!("\"{s}\" is not a number"))?;
    if v >= max {
        bail!("index {v} is out of range (must be below {max})");
    }
    Ok(v as u8)
}

/// Read a PICO-8 cart's assets from a file, auto-detecting `.p8` text vs
/// `.p8.png`.
pub fn parse_file(path: &Path) -> Result<Assets> {
    let bytes = std::fs::read(path)?;
    parse_bytes(&bytes)
}

/// Read a PICO-8 cart's assets from raw bytes, auto-detecting the format.
pub fn parse_bytes(bytes: &[u8]) -> Result<Assets> {
    if bytes.starts_with(&PNG_SIG) {
        parse_png(bytes)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| anyhow!("Not a PICO-8 cartridge (neither a PNG nor UTF-8 .p8 text)"))?;
        if !(text.contains("__gfx__") || text.contains("__lua__") || text.starts_with("pico-8")) {
            bail!("Not a PICO-8 cartridge (missing the PICO-8 header and sections)");
        }
        parse_text(text)
    }
}

/// Create a new RICO-8 project from a PICO-8 cart's assets.
///
/// The project's crate name comes from the target directory (like `new`);
/// the cart title comes from the source file. Imported assets are written to
/// `assets.rico8` and a stub `src/lib.rs` is scaffolded.
pub fn import_project(src: &Path, dir: &Path) -> Result<Project> {
    let assets = parse_file(src)?;

    let crate_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "imported".into());
    let title = cart_title(src).unwrap_or_else(|| crate_name.clone());

    let mut project = Project::create(dir, &crate_name)?;
    project.assets = assets;
    project.assets.meta.name = title;
    project.code = IMPORT_TEMPLATE.to_string();
    project.save()?;
    Ok(project)
}

/// Human-readable cart title from a source path: the file name with the
/// `.png` and `.p8` suffixes peeled off (`celeste.p8.png` -> `celeste`).
fn cart_title(src: &Path) -> Option<String> {
    let name = src.file_name()?.to_string_lossy();
    let name = name.strip_suffix(".png").unwrap_or(&name);
    let name = name.strip_suffix(".p8").unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

/// Default project directory name to import a cart into when none is given:
/// the cart's name with its suffixes peeled off (`airwolf.p8` -> `airwolf`),
/// falling back to `imported`.
pub fn default_dir_name(src: &Path) -> String {
    cart_title(src).unwrap_or_else(|| "imported".into())
}

// ---------------------------------------------------------------------------
// Text .p8 parsing
// ---------------------------------------------------------------------------

fn parse_text(text: &str) -> Result<Assets> {
    let mut assets = Assets::default();
    let mut section = "";
    let (mut gfx, mut gff, mut label, mut map, mut sfx, mut music) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    for line in text.lines() {
        if let Some(name) = section_header(line) {
            section = name;
            continue;
        }
        match section {
            "gfx" => gfx.push(line),
            "gff" => gff.push(line),
            "label" => label.push(line),
            "map" => map.push(line),
            "sfx" => sfx.push(line),
            "music" => music.push(line),
            // Everything else (including the ignored "lua" section) is skipped.
            _ => {}
        }
    }

    // gfx: one hex digit per pixel, row-major.
    for (y, row) in gfx.iter().take(SHEET_H).enumerate() {
        for (x, c) in row.trim().chars().take(SHEET_W).enumerate() {
            if let Some(v) = hex(c) {
                assets.sprites.pixels[y * SHEET_W + x] = v;
            }
        }
    }

    // gff: one byte (two hex digits) per sprite.
    for (i, b) in hex_bytes(&gff.concat())
        .into_iter()
        .take(SPRITE_COUNT)
        .enumerate()
    {
        assets.sprites.flags[i] = b;
    }

    // map: one byte per tile; the text section is the top 32 rows.
    for (y, row) in map.iter().take(PICO8_MAP_ROWS).enumerate() {
        for (x, b) in hex_bytes(row.trim()).into_iter().take(MAP_W).enumerate() {
            assets.map.tiles[y * MAP_W + x] = b;
        }
    }

    // Bring the shared region across as the bottom map rows as well. In the
    // text format it lives in the bottom 64 gfx rows we just parsed; read it
    // back as packed bytes (two pixels each, low nibble first).
    let pixels = assets.sprites.pixels.clone();
    fill_shared_map(&mut assets.map, |off| {
        let byte = SHARED_BASE + off;
        let (row, col) = (byte / 64, (byte % 64) * 2);
        pixels[row * SHEET_W + col] | (pixels[row * SHEET_W + col + 1] << 4)
    });

    // label: 128x128 hex screenshot, one digit per pixel.
    if !label.is_empty() {
        let mut px = vec![0u8; SHEET_W * SHEET_H];
        for (y, row) in label.iter().take(SHEET_H).enumerate() {
            for (x, c) in row.trim().chars().take(SHEET_W).enumerate() {
                if let Some(v) = hex(c) {
                    px[y * SHEET_W + x] = v;
                }
            }
        }
        assets.label = Some(px);
    }

    // sfx: 168 hex per line — 4 metadata bytes then 32 notes of 5 hex each.
    for (s, row) in sfx.iter().take(SFX_COUNT).enumerate() {
        let h = hex_digits(row.trim());
        if h.len() < 8 {
            continue;
        }
        // The first metadata byte packs the editor mode and filter switches.
        let filters = h[0] << 4 | h[1];
        let speed = (h[2] << 4 | h[3]).max(1);
        let loop_start = h[4] << 4 | h[5];
        let loop_end = h[6] << 4 | h[7];
        let mut notes = [Note::default(); SFX_LEN];
        for (i, note) in notes.iter_mut().enumerate() {
            let base = 8 + i * 5;
            if base + 5 > h.len() {
                break;
            }
            *note = Note {
                pitch: (h[base] << 4 | h[base + 1]) & 0x3f,
                // The waveform hex digit is the full nibble: bit 3 flags a
                // custom instrument, bits 0-2 the index. Keep it intact.
                wave: h[base + 2] & 0x0f,
                volume: h[base + 3] & 7,
                effect: h[base + 4] & 7,
            };
        }
        let mut out = Sfx {
            notes,
            speed,
            loop_start,
            loop_end,
            ..Default::default()
        };
        out.set_filters(filters);
        assets.sfx[s] = out;
    }

    // music: a flag byte then four channel bytes, e.g. "00 41424344".
    for (p, row) in music.iter().take(MUSIC_COUNT).enumerate() {
        let mut toks = row.split_whitespace();
        let (Some(flag_tok), Some(chan_tok)) = (toks.next(), toks.next()) else {
            continue;
        };
        let flags = u8::from_str_radix(flag_tok, 16).unwrap_or(0);
        let ch = hex_bytes(chan_tok);
        if ch.len() < 4 {
            continue;
        }
        assets.music[p] = MusicPattern {
            channels: [
                channel(ch[0]),
                channel(ch[1]),
                channel(ch[2]),
                channel(ch[3]),
            ],
            loop_start: flags & 1 != 0,
            loop_back: flags & 2 != 0,
            stop_at_end: flags & 4 != 0,
        };
    }

    assets::validate(&assets)?;
    Ok(assets)
}

/// Recognize an exact `__name__` section header line.
fn section_header(line: &str) -> Option<&str> {
    let name = line.trim().strip_prefix("__")?.strip_suffix("__")?;
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(name)
}

// ---------------------------------------------------------------------------
// PNG .p8.png parsing
// ---------------------------------------------------------------------------

fn parse_png(bytes: &[u8]) -> Result<Assets> {
    let rom = rom_from_png(bytes)?;
    let mut assets = Assets::default();

    // gfx 0x0000..0x2000: two pixels per byte, low nibble is the left pixel.
    for y in 0..SHEET_H {
        for x in 0..SHEET_W {
            let byte = rom[y * 64 + x / 2];
            assets.sprites.pixels[y * SHEET_W + x] =
                if x & 1 == 0 { byte & 0x0f } else { byte >> 4 };
        }
    }
    // map 0x2000..0x3000: top 32 rows, one byte per tile.
    for y in 0..PICO8_MAP_ROWS {
        for x in 0..MAP_W {
            assets.map.tiles[y * MAP_W + x] = rom[0x2000 + y * MAP_W + x];
        }
    }
    // The shared region 0x1000..0x2000 doubles as map rows 32..64.
    fill_shared_map(&mut assets.map, |off| rom[SHARED_BASE + off]);
    // gff 0x3000..0x3100: one flag byte per sprite.
    assets
        .sprites
        .flags
        .copy_from_slice(&rom[0x3000..0x3000 + SPRITE_COUNT]);
    // music 0x3100..0x3200: four bytes per pattern.
    for p in 0..MUSIC_COUNT {
        let base = 0x3100 + p * 4;
        assets.music[p] = music_from_mem([rom[base], rom[base + 1], rom[base + 2], rom[base + 3]]);
    }
    // sfx 0x3200..0x4300: 68 bytes per slot.
    for s in 0..SFX_COUNT {
        let base = 0x3200 + s * SFX_MEM_LEN;
        assets.sfx[s] = sfx_from_mem(&rom[base..base + SFX_MEM_LEN]);
    }

    assets::validate(&assets)?;
    Ok(assets)
}

/// Reassemble the cart ROM from a PICO-8 PNG: two bits per channel, A/R/G/B.
fn rom_from_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let (w, h, rgba) = decode_png_rgba(bytes)?;
    if w != PICO8_PNG_W || h != PICO8_PNG_H {
        bail!("Not a PICO-8 cart PNG (expected {PICO8_PNG_W}x{PICO8_PNG_H}, got {w}x{h})");
    }
    let mut rom: Vec<u8> = rgba
        .chunks_exact(4)
        .map(|p| ((p[3] & 3) << 6) | ((p[0] & 3) << 4) | ((p[1] & 3) << 2) | (p[2] & 3))
        .collect();
    if rom.len() < ROM_LEN {
        bail!("PICO-8 cart PNG is too small to hold a ROM");
    }
    rom.truncate(ROM_LEN);
    Ok(rom)
}

/// Fill map rows 32..64 from PICO-8's shared region. PICO-8 aliases
/// `0x1000..0x2000` between the bottom half of the sprite sheet and the
/// bottom 32 rows of the map; a cart uses it for one or the other, with no
/// flag saying which. RICO-8 de-aliases the two (it has a full 256-sprite
/// sheet *and* a full 128x64 map), so we bring the region across both ways:
/// the bytes already populate sprites 128..256, and here they populate the
/// lower map too. The user keeps whichever their cart actually used and
/// clears the other. `byte_at(off)` returns the byte at `0x1000 + off`.
fn fill_shared_map(map: &mut MapData, byte_at: impl Fn(usize) -> u8) {
    for r in 0..(MAP_H - PICO8_MAP_ROWS) {
        for x in 0..MAP_W {
            map.tiles[(PICO8_MAP_ROWS + r) * MAP_W + x] = byte_at(r * MAP_W + x);
        }
    }
}

/// One PICO-8 music pattern from its four cart-memory bytes. The loop/stop
/// flags ride in the high bit of the first three channel bytes.
fn music_from_mem(ch: [u8; 4]) -> MusicPattern {
    MusicPattern {
        channels: [
            channel(ch[0]),
            channel(ch[1]),
            channel(ch[2]),
            channel(ch[3]),
        ],
        loop_start: ch[0] & 0x80 != 0,
        loop_back: ch[1] & 0x80 != 0,
        stop_at_end: ch[2] & 0x80 != 0,
    }
}

/// One PICO-8 SFX from its 68 cart-memory bytes: 32 notes of two bytes
/// (little-endian: pitch 0-5, waveform 6-8, volume 9-11, effect 12-14,
/// custom-instrument flag 15), then the filter/editor-mode byte, speed,
/// loop-start and loop-end metadata.
fn sfx_from_mem(b: &[u8]) -> Sfx {
    let mut notes = [Note::default(); SFX_LEN];
    for (i, note) in notes.iter_mut().enumerate() {
        let v = b[i * 2] as u16 | (b[i * 2 + 1] as u16) << 8;
        // Bit 15 is PICO-8's custom-instrument flag; fold it into our wave
        // nibble (bit 3) alongside the 3-bit waveform/instrument index.
        let custom = (v >> 15 & 1) as u8;
        *note = Note {
            pitch: (v & 0x3f) as u8,
            wave: (v >> 6 & 7) as u8 | custom << 3,
            volume: (v >> 9 & 7) as u8,
            effect: (v >> 12 & 7) as u8,
        };
    }
    let mut sfx = Sfx {
        notes,
        speed: b[65].max(1),
        loop_start: b[66],
        loop_end: b[67],
        ..Default::default()
    };
    sfx.set_filters(b[64]);
    sfx
}

/// Decode one music channel byte: the low 6 bits are the SFX index; bit 6
/// marks the channel silent. (Bit 7 carries pattern flags, handled apart.)
fn channel(b: u8) -> Option<u8> {
    (b & 0x40 == 0).then_some(b & 0x3f)
}

// ---------------------------------------------------------------------------
// A small PNG decoder (8-bit RGBA, non-interlaced) for cart images
// ---------------------------------------------------------------------------

/// Decode an 8-bit RGBA, non-interlaced PNG to `(width, height, rgba)`.
/// Just enough of the spec to read a PICO-8 cart image; richer PNGs are
/// rejected with a clear message rather than mis-decoded.
fn decode_png_rgba(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
    if !bytes.starts_with(&PNG_SIG) {
        bail!("not a png file");
    }
    let mut rest = &bytes[8..];
    let (mut width, mut height) = (0usize, 0usize);
    let mut idat = Vec::new();
    let mut have_ihdr = false;
    while rest.len() >= 12 {
        let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
        let ctype = &rest[4..8];
        if rest.len() < 12 + len {
            bail!("truncated png chunk");
        }
        let data = &rest[8..8 + len];
        match ctype {
            b"IHDR" => {
                if len < 13 {
                    bail!("malformed png header");
                }
                width = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
                height = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
                let (bit_depth, color_type, interlace) = (data[8], data[9], data[12]);
                if bit_depth != 8 || color_type != 6 {
                    bail!("unsupported png: need 8-bit rgba (a pico-8 cart png is)");
                }
                if interlace != 0 {
                    bail!("interlaced png is not supported");
                }
                have_ihdr = true;
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        rest = &rest[12 + len..];
    }
    if !have_ihdr {
        bail!("png has no header chunk");
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&idat, 64 * 1024 * 1024)
        .map_err(|e| anyhow!("png image data is corrupted: {e:?}"))?;

    const BPP: usize = 4;
    let stride = width * BPP;
    if raw.len() < height * (stride + 1) {
        bail!("png image data is truncated");
    }
    let mut out = vec![0u8; height * stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        for i in 0..stride {
            let a = if i >= BPP {
                out[y * stride + i - BPP]
            } else {
                0
            };
            let b = if y > 0 { out[(y - 1) * stride + i] } else { 0 };
            let c = if y > 0 && i >= BPP {
                out[(y - 1) * stride + i - BPP]
            } else {
                0
            };
            out[y * stride + i] = match filter {
                0 => line[i],
                1 => line[i].wrapping_add(a),
                2 => line[i].wrapping_add(b),
                3 => line[i].wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => line[i].wrapping_add(paeth(a, b, c)),
                f => bail!("unknown png filter type {f}"),
            };
        }
    }
    Ok((width, height, out))
}

/// The PNG Paeth predictor.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let (pa, pb, pc) = (
        (p - a as i32).abs(),
        (p - b as i32).abs(),
        (p - c as i32).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Hex helpers
// ---------------------------------------------------------------------------

fn hex(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// Every hex digit in `s`, as nibble values, skipping anything else.
fn hex_digits(s: &str) -> Vec<u8> {
    s.chars().filter_map(hex).collect()
}

/// Hex digits of `s` paired into bytes (most-significant nibble first).
fn hex_bytes(s: &str) -> Vec<u8> {
    hex_digits(s)
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| c[0] << 4 | c[1])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but complete text `.p8` exercising every asset section.
    fn sample_p8() -> String {
        let mut s = String::from("pico-8 cartridge // http://www.pico-8.com\nversion 41\n");
        s.push_str("__lua__\n");
        s.push_str("function _draw()\n cls(1)\nend\n");
        // gfx: set pixel (2,0)=a and (3,1)=5; rest zero.
        s.push_str("__gfx__\n");
        let mut row0 = vec!['0'; 128];
        row0[2] = 'a';
        s.push_str(&row0.iter().collect::<String>());
        s.push('\n');
        let mut row1 = vec!['0'; 128];
        row1[3] = '5';
        s.push_str(&row1.iter().collect::<String>());
        s.push('\n');
        // gff: sprite 0 flags = 0x03, sprite 1 = 0x80.
        s.push_str("__gff__\n");
        let mut gff = String::from("0380");
        gff.push_str(&"00".repeat(254));
        s.push_str(&gff);
        s.push('\n');
        // map: tile (1,0)=2a.
        s.push_str("__map__\n");
        let mut map = String::from("002a");
        map.push_str(&"00".repeat(126));
        s.push_str(&map);
        s.push('\n');
        // sfx 0: speed 0x10, loop 02..04, note0 pitch 21 wave 3 vol 6 eff 1.
        s.push_str("__sfx__\n");
        // filter byte 0x86 = noiz + buzz + detune 1 + reverb 2 + dampen 1.
        let mut sfx = String::from("86100204"); // filters, speed, loop start, loop end
        sfx.push_str("21361"); // note 0: pitch 21, wave 3, vol 6, eff 1
        sfx.push_str("10a50"); // note 1: custom instrument 2 (nibble 0xa), vol 5
        sfx.push_str(&"00000".repeat(30)); // notes 2..32 silent
        s.push_str(&sfx);
        s.push('\n');
        // music 0: loop start flag, ch0=sfx1, others silent.
        s.push_str("__music__\n");
        s.push_str("01 01404040\n");
        s
    }

    #[test]
    fn parses_text_sections() {
        let a = parse_text(&sample_p8()).unwrap();

        assert_eq!(a.sprites.get(2, 0), 0xa);
        assert_eq!(a.sprites.get(3, 1), 0x5);
        assert_eq!(a.sprites.flags(0), 0x03);
        assert_eq!(a.sprites.flags(1), 0x80);
        assert_eq!(a.map.get(1, 0), 0x2a);

        let n = a.sfx[0].notes[0];
        assert_eq!((n.pitch, n.wave, n.volume, n.effect), (0x21, 3, 6, 1));
        assert_eq!(n.instrument(), None, "a plain note is not a custom instr");
        assert_eq!(a.sfx[0].speed, 0x10);
        assert_eq!((a.sfx[0].loop_start, a.sfx[0].loop_end), (0x02, 0x04));
        // Filter byte 0x86 decodes to every switch engaged.
        let f = &a.sfx[0];
        assert!(f.noiz && f.buzz);
        assert_eq!((f.detune, f.reverb, f.dampen), (1, 2, 1));

        // Note 1 is a custom instrument: index 2 with the custom flag set.
        let n1 = a.sfx[0].notes[1];
        assert_eq!(n1.instrument(), Some(2));
        assert_eq!(n1.wave_index(), 2);

        let m = &a.music[0];
        assert!(m.loop_start && !m.loop_back && !m.stop_at_end);
        assert_eq!(m.channels, [Some(1), None, None, None]);
    }

    #[test]
    fn default_dir_name_strips_suffixes() {
        assert_eq!(default_dir_name(Path::new("airwolf.p8")), "airwolf");
        assert_eq!(default_dir_name(Path::new("celeste.p8.png")), "celeste");
        assert_eq!(default_dir_name(Path::new("/a/b/jelpi.p8")), "jelpi");
        assert_eq!(default_dir_name(Path::new("noext")), "noext");
    }

    #[test]
    fn rejects_non_pico8_bytes() {
        assert!(parse_bytes(b"just some text").is_err());
        assert!(parse_bytes(&[0u8, 1, 2, 3]).is_err());
    }

    /// Build a PICO-8-style PNG from a ROM and round-trip it through the
    /// PNG decoder + stegano extraction.
    #[test]
    fn parses_png_cart() {
        // A ROM with a couple of distinctive asset bytes set.
        let mut rom = vec![0u8; ROM_LEN];
        rom[0] = 0xb0; // gfx byte 0: pixel(0,0)=0, pixel(1,0)=0xb
        rom[0x3000] = 0x42; // sprite 0 flags
        rom[0x2000 + 5] = 0x09; // map tile (5,0)
        rom[0x1000 + 3] = 0x57; // shared region -> map row 32, col 3
                                // sfx 0, note 0: pitch=0x12, custom instr 2, vol=5, eff=3.
        let v: u16 = 0x12 | (2 << 6) | (5 << 9) | (3 << 12) | (1 << 15);
        rom[0x3200] = (v & 0xff) as u8;
        rom[0x3201] = (v >> 8) as u8;
        rom[0x3200 + 64] = 0x1a; // filters: noiz + reverb 1
        rom[0x3200 + 65] = 0x18; // speed
                                 // music 0: ch0 = sfx 7, stop flag on ch2.
        rom[0x3100] = 0x07;
        rom[0x3100 + 2] = 0x80;

        let png = build_pico8_png(&rom);
        let a = parse_bytes(&png).unwrap();

        assert_eq!(a.sprites.get(0, 0), 0x0);
        assert_eq!(a.sprites.get(1, 0), 0xb);
        assert_eq!(a.sprites.flags(0), 0x42);
        assert_eq!(a.map.get(5, 0), 0x09);
        // The shared region lands both in sprites 128.. and in the lower map.
        assert_eq!(a.map.get(3, 32), 0x57);
        let n = a.sfx[0].notes[0];
        assert_eq!((n.pitch, n.volume, n.effect), (0x12, 5, 3));
        assert_eq!(n.instrument(), Some(2), "bit 15 marks a custom instrument");
        assert_eq!(a.sfx[0].speed, 0x18);
        assert!(a.sfx[0].noiz && !a.sfx[0].buzz);
        assert_eq!(
            (a.sfx[0].detune, a.sfx[0].reverb, a.sfx[0].dampen),
            (0, 1, 0)
        );
        assert_eq!(a.music[0].channels[0], Some(7));
        assert!(a.music[0].stop_at_end);
    }

    /// Encode a ROM into a 160x205 RGBA PNG the way PICO-8 does: two bits
    /// of each byte per A/R/G/B channel, filter-0 scanlines, zlib IDAT.
    fn build_pico8_png(rom: &[u8]) -> Vec<u8> {
        let (w, h) = (PICO8_PNG_W, PICO8_PNG_H);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            let byte = rom.get(i).copied().unwrap_or(0);
            px[0] = byte >> 4 & 3; // r
            px[1] = byte >> 2 & 3; // g
            px[2] = byte & 3; // b
            px[3] = byte >> 6 & 3; // a
        }
        let mut raw = Vec::with_capacity(h * (1 + w * 4));
        for y in 0..h {
            raw.push(0);
            raw.extend_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
        }

        let mut png = PNG_SIG.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend((w as u32).to_be_bytes());
        ihdr.extend((h as u32).to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        write_chunk(&mut png, *b"IHDR", &ihdr);
        let idat = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        write_chunk(&mut png, *b"IDAT", &idat);
        write_chunk(&mut png, *b"IEND", &[]);
        png
    }

    fn write_chunk(out: &mut Vec<u8>, ctype: [u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(ctype);
        out.extend_from_slice(data);
        let mut h = crc32fast::Hasher::new();
        h.update(&ctype);
        h.update(data);
        out.extend(h.finalize().to_be_bytes());
    }

    #[test]
    fn import_project_writes_assets() {
        let base = std::env::temp_dir().join(format!("rico8_p8_import_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("celeste.p8");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&src, sample_p8()).unwrap();

        let dir = base.join("ported");
        let project = import_project(&src, &dir).unwrap();

        assert_eq!(project.name, "ported");
        assert_eq!(project.assets.meta.name, "celeste");
        assert_eq!(project.assets.sprites.get(2, 0), 0xa);
        assert!(project.code.contains("Imported from PICO-8"));
        // Only assets are imported; no Lua is preserved.
        assert!(!dir.join("pico8.lua").exists());

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn parse_ranges_singles_and_ranges() {
        assert_eq!(
            parse_index_ranges("0-3,5,8-9", 64).unwrap(),
            vec![0, 1, 2, 3, 5, 8, 9]
        );
        assert_eq!(parse_index_ranges("7", 64).unwrap(), vec![7]);
        // Whitespace is tolerated; output is sorted and deduped.
        assert_eq!(
            parse_index_ranges(" 3, 1 ,1, 2 ", 64).unwrap(),
            vec![1, 2, 3]
        );
        // The max is exclusive: index 255 is the last valid sprite.
        assert_eq!(parse_index_ranges("255", 256).unwrap(), vec![255]);
    }

    #[test]
    fn parse_ranges_rejects_bad_input() {
        assert!(parse_index_ranges("", 64).is_err(), "empty string");
        assert!(parse_index_ranges("1,,2", 64).is_err(), "empty token");
        assert!(parse_index_ranges("64", 64).is_err(), "out of range");
        assert!(parse_index_ranges("5-3", 64).is_err(), "reversed range");
        assert!(parse_index_ranges("x", 64).is_err(), "non-numeric");
        assert!(
            parse_index_ranges("0-99", 64).is_err(),
            "range end out of bounds"
        );
    }

    #[test]
    fn selection_parse_requires_one_kind() {
        assert!(Selection::parse(None, None, None).is_err());
        let s = Selection::parse(Some("0-2"), None, Some("3")).unwrap();
        assert_eq!(s.sprites, vec![0, 1, 2]);
        assert!(s.sfx.is_empty());
        assert_eq!(s.music, vec![3]);
    }

    #[test]
    fn append_sprites_into_empty_lands_at_zero() {
        let mut src = Assets::default();
        src.sprites.set(0, 0, 7); // sprite 0, pixel (0,0).
        src.sprites.set(8, 0, 9); // sprite 1, pixel (0,0).
        src.sprites.flags[1] = 0x05;
        let mut dest = Assets::default();
        let sel = Selection {
            sprites: vec![0, 1],
            sfx: vec![],
            music: vec![],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        assert_eq!((r.sprites.start, r.sprites.count), (0, 2));
        assert_eq!(dest.sprites.get(0, 0), 7);
        assert_eq!(dest.sprites.get(8, 0), 9);
        assert_eq!(dest.sprites.flags(1), 0x05);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn append_sprites_after_last_used_slot() {
        let mut dest = Assets::default();
        dest.sprites.set(0, 0, 1); // sprite 0 used by a pixel.
        dest.sprites.flags[3] = 0x01; // sprite 3 used by a flag only.
        let mut src = Assets::default();
        src.sprites.set(0, 0, 0xc);
        let sel = Selection {
            sprites: vec![0],
            sfx: vec![],
            music: vec![],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        // Highest used was sprite 3, so the import lands at sprite 4.
        assert_eq!(r.sprites.start, 4);
        // Sprite 4 sits at sheet (32, 0).
        assert_eq!(dest.sprites.get(32, 0), 0xc);
        // Earlier slots are untouched.
        assert_eq!(dest.sprites.get(0, 0), 1);
    }

    #[test]
    fn append_music_remaps_imported_sfx_refs() {
        let mut src = Assets::default();
        src.sfx[5].notes[0].volume = 5;
        src.sfx[6].notes[0].volume = 5;
        src.music[0].channels = [Some(5), Some(6), None, None];
        let mut dest = Assets::default();
        let sel = Selection {
            sprites: vec![],
            sfx: vec![5, 6],
            music: vec![0],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        assert_eq!((r.sfx.start, r.sfx.count), (0, 2));
        assert_eq!((r.music.start, r.music.count), (0, 1));
        // SFX 5 landed in slot 0 and 6 in slot 1; the channels follow.
        assert_eq!(dest.music[0].channels, [Some(0), Some(1), None, None]);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn append_music_keeps_and_warns_on_dangling_ref() {
        let mut src = Assets::default();
        src.sfx[5].notes[0].volume = 5;
        // Channel 1 references SFX 9, which is not in the selection.
        src.music[0].channels = [Some(5), Some(9), None, None];
        let mut dest = Assets::default();
        let sel = Selection {
            sprites: vec![],
            sfx: vec![5],
            music: vec![0],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        assert_eq!(dest.music[0].channels, [Some(0), Some(9), None, None]);
        assert_eq!(r.warnings.len(), 1);
        assert!(
            r.warnings[0].contains('9'),
            "warning names the dangling slot"
        );
    }

    #[test]
    fn append_custom_instrument_past_slot7_keeps_and_warns() {
        let mut src = Assets::default();
        src.sfx[2].notes[0].volume = 5; // a referenced instrument timbre.
        src.sfx[10].notes[0] = Note {
            pitch: 20,
            wave: NOTE_CUSTOM_FLAG | 2, // plays SFX 2 as a custom instrument.
            volume: 5,
            effect: 0,
        };
        let mut dest = Assets::default();
        // Fill SFX 0..8 so the import is forced to land at slot 8+.
        for i in 0..8 {
            dest.sfx[i].notes[0].volume = 1;
        }
        let sel = Selection {
            sprites: vec![],
            sfx: vec![2, 10],
            music: vec![],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        assert_eq!(r.sfx.start, 8); // SFX 2 -> slot 8, SFX 10 -> slot 9.
                                    // The custom-instrument ref can't point past slot 7,
                                    // so it is kept.
        assert_eq!(dest.sfx[9].notes[0].instrument(), Some(2));
        assert!(r.warnings.iter().any(|w| w.contains("custom instrument")));
    }

    #[test]
    fn append_overflow_errors_and_leaves_dest_untouched() {
        let mut dest = Assets::default();
        dest.sfx[63].notes[0].volume = 1; // highest slot used -> no room.
        let src = Assets::default();
        let sel = Selection {
            sprites: vec![],
            sfx: vec![0],
            music: vec![],
        };

        let err = append_pico8_assets(&mut dest, &src, &sel).unwrap_err();

        assert!(err.to_string().contains("room"), "got: {err}");
        // Transactional: the destination is unchanged on error.
        assert_eq!(dest.sfx[63].notes[0].volume, 1);
    }

    #[test]
    fn append_does_not_overwrite_custom_wave_instrument() {
        let mut dest = Assets::default();
        // A silent custom-wave instrument at slot 3 (no audible notes).
        dest.sfx[3].custom_wave = Some(assets::CustomWave {
            samples: [0; SFX_LEN],
            bass: false,
        });
        let mut src = Assets::default();
        src.sfx[0].notes[0].volume = 5;
        let sel = Selection {
            sprites: vec![],
            sfx: vec![0],
            music: vec![],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        // The instrument at slot 3 counts as used, so the import lands at slot 4.
        assert_eq!(r.sfx.start, 4);
        assert!(dest.sfx[3].custom_wave.is_some(), "instrument preserved");
    }

    #[test]
    fn append_does_not_overwrite_flow_only_music_pattern() {
        let mut dest = Assets::default();
        // A pattern with no channels but a stop flag (a real song terminator).
        dest.music[2].stop_at_end = true;
        let mut src = Assets::default();
        src.music[0].channels[0] = Some(0);
        let sel = Selection {
            sprites: vec![],
            sfx: vec![],
            music: vec![0],
        };

        let r = append_pico8_assets(&mut dest, &src, &sel).unwrap();

        assert_eq!(r.music.start, 3, "import lands after the flow-only pattern");
        assert!(dest.music[2].stop_at_end, "flow pattern preserved");
    }

    #[test]
    fn append_sprite_overflow_errors_and_keeps_dest() {
        let mut dest = Assets::default();
        dest.sprites.set(120, 120, 1); // sprite 255's block -> sheet is full.
        let src = Assets::default();
        let sel = Selection {
            sprites: vec![0],
            sfx: vec![],
            music: vec![],
        };

        let err = append_pico8_assets(&mut dest, &src, &sel).unwrap_err();

        assert!(err.to_string().contains("room"), "got: {err}");
        assert_eq!(dest.sprites.get(120, 120), 1); // unchanged.
    }

    #[test]
    fn append_music_overflow_errors() {
        let mut dest = Assets::default();
        dest.music[63].channels[0] = Some(0); // pattern 63 used -> full.
        let src = Assets::default();
        let sel = Selection {
            sprites: vec![],
            sfx: vec![],
            music: vec![0],
        };

        let err = append_pico8_assets(&mut dest, &src, &sel).unwrap_err();
        assert!(err.to_string().contains("room"), "got: {err}");
    }

    #[test]
    fn append_round_trips_through_a_project() {
        let base = std::env::temp_dir().join(format!("rico8_append_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("celeste.p8");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&src, sample_p8()).unwrap();

        // Destination project with sprite 0 already used.
        let dir = base.join("dest");
        let mut project = Project::create(&dir, "dest").unwrap();
        project.assets.sprites.set(0, 0, 4);
        project.save().unwrap();

        // Append source sprite 0 (pixel (2,0) = 0xa per sample_p8).
        let assets = parse_file(&src).unwrap();
        let sel = Selection {
            sprites: vec![0],
            sfx: vec![],
            music: vec![],
        };
        append_pico8_assets(&mut project.assets, &assets, &sel).unwrap();
        project.save().unwrap();

        // Reload from disk and confirm the append persisted at sprite 1.
        let reloaded = Project::load(&dir).unwrap();
        // Sprite 1 sits at sheet (8, 0); source pixel (2,0) lands at (10, 0).
        assert_eq!(reloaded.assets.sprites.get(10, 0), 0xa);
        assert_eq!(reloaded.assets.sprites.get(0, 0), 4); // original kept.

        std::fs::remove_dir_all(&base).unwrap();
    }
}
