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
        self, Assets, MapData, MusicPattern, Note, Sfx, MAP_H, MAP_W, MUSIC_COUNT, SFX_COUNT,
        SFX_LEN, SHEET_H, SHEET_W, SPRITE_COUNT,
    },
    project::Project,
};
use anyhow::{anyhow, bail, Result};
use std::path::Path;

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
        gfx.print("imported from pico-8", 12, 54, Color::WHITE);
        gfx.print("write your game here", 16, 64, Color::LIGHT_GREY);
    }
}

rico8::game!(Cart);
"#;

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
            .map_err(|_| anyhow!("not a pico-8 cartridge (neither a png nor utf-8 .p8 text)"))?;
        if !(text.contains("__gfx__") || text.contains("__lua__") || text.starts_with("pico-8")) {
            bail!("not a pico-8 cartridge (missing the pico-8 header and sections)");
        }
        parse_text(text)
    }
}

/// Create a new RICO-8 project from a PICO-8 cart's assets.
///
/// The project's crate name comes from the target directory (like `new`);
/// the cart title comes from the source file. Imported assets are written to
/// `assets.rico8` and a stub `src/lib.rs` is scaffolded.
pub fn import_project(src: &Path, dir: &Path, sdk: &Path) -> Result<Project> {
    let assets = parse_file(src)?;

    let crate_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "imported".into());
    let title = cart_title(src).unwrap_or_else(|| crate_name.clone());

    let mut project = Project::create(dir, &crate_name, sdk)?;
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
        assets.sfx[s] = Sfx {
            notes,
            speed,
            loop_start,
            loop_end,
        };
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
        bail!("not a pico-8 cart png (expected {PICO8_PNG_W}x{PICO8_PNG_H}, got {w}x{h})");
    }
    let mut rom: Vec<u8> = rgba
        .chunks_exact(4)
        .map(|p| ((p[3] & 3) << 6) | ((p[0] & 3) << 4) | ((p[1] & 3) << 2) | (p[2] & 3))
        .collect();
    if rom.len() < ROM_LEN {
        bail!("pico-8 cart png is too small to hold a rom");
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
/// custom-instrument flag 15), then editor-mode, speed, loop-start and
/// loop-end metadata.
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
    Sfx {
        notes,
        speed: b[65].max(1),
        loop_start: b[66],
        loop_end: b[67],
    }
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
        let mut sfx = String::from("00100204"); // mode, speed, loop start, loop end
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

        // Note 1 is a custom instrument: index 2 with the custom flag set.
        let n1 = a.sfx[0].notes[1];
        assert_eq!(n1.instrument(), Some(2));
        assert_eq!(n1.wave_index(), 2);

        let m = &a.music[0];
        assert!(m.loop_start && !m.loop_back && !m.stop_at_end);
        assert_eq!(m.channels, [Some(1), None, None, None]);
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
        let project = import_project(&src, &dir, Path::new("/tmp/sdk")).unwrap();

        assert_eq!(project.name, "ported");
        assert_eq!(project.assets.meta.name, "celeste");
        assert_eq!(project.assets.sprites.get(2, 0), 0xa);
        assert!(project.code.contains("imported from pico-8"));
        // Only assets are imported; no Lua is preserved.
        assert!(!dir.join("pico8.lua").exists());

        std::fs::remove_dir_all(&base).unwrap();
    }
}
