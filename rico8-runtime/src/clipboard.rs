//! RICO-8's clipboard data model and paste appliers, plus the native
//! `[rico8]` clipboard codec.
//!
//! The value types here are format-neutral: PICO-8 `[gfx]`/`[sfx]` blobs decode
//! into them (see [`crate::pico8`]) and so does the native `[rico8]` format.

use crate::{
    assets::{
        Assets, MusicPattern, Sfx, SpriteSheet, MAP_H, MAP_W, MUSIC_COUNT, SFX_COUNT, SHEET_H,
        SHEET_W, SPRITES_PER_ROW, SPRITE_SIZE,
    },
    pico8::{
        bytes_to_hex, hex_bytes, next_free_sfx, remap_custom_instruments, remap_music_channels,
    },
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A `width * height` block of palette-index pixels, one per cell, row-major.
pub struct PixelRect {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u8>,
}

/// One clipboard item tagged with the PICO-8 slot it was copied from.
pub struct Slotted<T> {
    pub src: u8,
    pub value: T,
}

/// A decoded `[sfx]` blob: the SFX records plus any trailing pattern footers
/// (present when the blob came from copying a song pattern). The patterns'
/// channel refs hold PICO-8 *source* SFX slots until a paste remaps them.
pub struct SfxClip {
    pub records: Vec<Slotted<Sfx>>,
    pub patterns: Vec<MusicPattern>,
}

/// One decoded clipboard blob, by kind.
pub enum Pasted {
    /// A pixel block; `flags` is `Some` only for native sprite copies (one byte
    /// per covered 8x8 sprite), `None` for PICO-8 `[gfx]`.
    Sprites {
        rect: PixelRect,
        flags: Option<Vec<u8>>,
    },
    Sfx(SfxClip),
    /// A map tile region. Native only — PICO-8 has no map clipboard format.
    Map {
        w: usize,
        h: usize,
        tiles: Vec<u8>,
    },
}

/// The outcome of a paste, ready for the editor's status bar.
pub struct PasteReport {
    /// A compact one-line summary that fits the status bar.
    pub summary: String,
    /// Items that did not fit (capacity) or fell outside the sheet/map.
    pub clipped: usize,
    /// Per-reference warnings; the count is folded into `summary`.
    pub warnings: Vec<String>,
}

/// Blit a pixel rectangle with its top-left at sheet pixel `(x0, y0)`,
/// overwriting in place. Pixels past the sheet edge are clipped. `flags` is
/// one byte per covered 8x8 sprite (row-major); when `Some`, each byte
/// overwrites the destination sprite's flag byte.
pub fn paste_sprites(
    sheet: &mut SpriteSheet,
    rect: &PixelRect,
    x0: i32,
    y0: i32,
    flags: Option<&[u8]>,
) -> PasteReport {
    let mut clipped = 0;
    for ry in 0..rect.h {
        for rx in 0..rect.w {
            let (x, y) = (x0 + rx as i32, y0 + ry as i32);
            if (0..SHEET_W as i32).contains(&x) && (0..SHEET_H as i32).contains(&y) {
                sheet.set(x, y, rect.pixels[ry * rect.w + rx]);
            } else {
                clipped += 1;
            }
        }
    }
    let slot =
        (y0.max(0) as usize / SPRITE_SIZE) * SPRITES_PER_ROW + (x0.max(0) as usize / SPRITE_SIZE);
    // Native sprite copies carry one flag byte per covered 8x8 sprite, laid out
    // row-major from the destination sprite; apply them onto the sheet.
    if let Some(flags) = flags {
        let cols = rect.w.div_ceil(SPRITE_SIZE);
        let sprite_x0 = (x0.max(0) as usize) / SPRITE_SIZE;
        let sprite_y0 = (y0.max(0) as usize) / SPRITE_SIZE;
        for (i, &f) in flags.iter().enumerate() {
            let n = (sprite_y0 + i / cols) * SPRITES_PER_ROW + (sprite_x0 + i % cols);
            if n < sheet.flags.len() {
                sheet.flags[n] = f;
            }
        }
    }
    PasteReport {
        summary: rect_summary(rect.w, rect.h, clipped, slot),
        clipped,
        warnings: Vec::new(),
    }
}

/// SFX-editor paste: overwrite SFX slots from `at` with `records`, remapping
/// each record's custom-instrument note refs to the slots they land in. Records
/// past slot 63 are dropped.
pub fn paste_sfx(sfx: &mut [Sfx], records: &[Slotted<Sfx>], at: usize) -> PasteReport {
    let fit = records.len().min(SFX_COUNT.saturating_sub(at));
    let mut sfx_map: HashMap<u8, usize> = HashMap::new();
    for (i, rec) in records.iter().take(fit).enumerate() {
        sfx[at + i] = rec.value.clone();
        sfx_map.insert(rec.src, at + i);
    }
    let mut warnings = Vec::new();
    for (i, rec) in records.iter().take(fit).enumerate() {
        remap_custom_instruments(
            &mut sfx[at + i].notes,
            &sfx_map,
            &format!("SFX {}", rec.src),
            &mut warnings,
        );
    }
    let clipped = records.len() - fit;
    PasteReport {
        summary: seq_summary("SFX", at, fit, clipped, warnings.len()),
        clipped,
        warnings,
    }
}

/// Music-editor paste: append the clip's SFX after the last used SFX slot
/// (non-destructive), remap each footer pattern's channel refs to where those
/// SFX landed, then overwrite music patterns from `at_pattern`.
pub fn paste_pattern(assets: &mut Assets, clip: &SfxClip, at_pattern: usize) -> PasteReport {
    let mut warnings = Vec::new();

    let sfx_start = next_free_sfx(&assets.sfx);
    let sfx_fit = clip.records.len().min(SFX_COUNT.saturating_sub(sfx_start));
    let mut sfx_map: HashMap<u8, usize> = HashMap::new();
    for (i, rec) in clip.records.iter().take(sfx_fit).enumerate() {
        assets.sfx[sfx_start + i] = rec.value.clone();
        sfx_map.insert(rec.src, sfx_start + i);
    }
    for (i, rec) in clip.records.iter().take(sfx_fit).enumerate() {
        remap_custom_instruments(
            &mut assets.sfx[sfx_start + i].notes,
            &sfx_map,
            &format!("SFX {}", rec.src),
            &mut warnings,
        );
    }

    let pat_fit = clip
        .patterns
        .len()
        .min(MUSIC_COUNT.saturating_sub(at_pattern));
    for (i, pat) in clip.patterns.iter().take(pat_fit).enumerate() {
        let mut p = *pat;
        remap_music_channels(&mut p, &sfx_map, &format!("pattern {i}"), &mut warnings);
        assets.music[at_pattern + i] = p;
    }

    let clipped = (clip.records.len() - sfx_fit) + (clip.patterns.len() - pat_fit);
    let summary = pattern_summary(at_pattern, pat_fit, sfx_fit, clipped, warnings.len());
    PasteReport {
        summary,
        clipped,
        warnings,
    }
}

// The summaries below are deliberately terse so they always fit the 31-char
// status bar (verified by `summaries_fit_the_status_bar`). Counts use compact
// `Ncut`/`Nwarn` tokens rather than spelled-out words.

/// A status-bar-safe rendering of a count: a literal up to 99, then `"99+"`,
/// so summary width stays bounded no matter how many items were clipped or
/// warned about.
fn count(n: usize) -> String {
    if n > 99 {
        "99+".to_string()
    } else {
        n.to_string()
    }
}

/// `"pasted 8x8 spr 1"`, plus ` Ncut` when some cells fell off the sheet.
fn rect_summary(w: usize, h: usize, clipped: usize, slot: usize) -> String {
    let mut s = format!("pasted {w}x{h} spr {slot}");
    if clipped > 0 {
        s.push_str(&format!(" {}cut", count(clipped)));
    }
    s
}

/// `"pasted SFX 3-6"` (or `"pasted SFX 3"` for one), plus ` Ncut`/` Nwarn` tails.
/// The slot range implies the count, so it is not repeated.
fn seq_summary(kind: &str, at: usize, fit: usize, clipped: usize, warns: usize) -> String {
    let mut s = match fit {
        0 => return format!("no free {kind} slot"),
        1 => format!("pasted {kind} {at}"),
        n => format!("pasted {kind} {at}-{}", at + n - 1),
    };
    if clipped > 0 {
        s.push_str(&format!(" {}cut", count(clipped)));
    }
    if warns > 0 {
        s.push_str(&format!(" {}warn", count(warns)));
    }
    s
}

/// `"pasted pat 5 +2sfx"` when clean; on issues it drops the SFX count to make
/// room for ` Ncut`/` Nwarn`, e.g. `"pasted pat 5 1cut 3warn"`.
fn pattern_summary(
    at: usize,
    pat_fit: usize,
    sfx_fit: usize,
    clipped: usize,
    warns: usize,
) -> String {
    if pat_fit == 0 {
        return if sfx_fit > 0 {
            format!("pasted +{sfx_fit}sfx, no pattern")
        } else {
            "no pattern in clipboard".to_string()
        };
    }
    if clipped == 0 && warns == 0 {
        return format!("pasted pat {at} +{sfx_fit}sfx");
    }
    let mut s = format!("pasted pat {at}");
    if clipped > 0 {
        s.push_str(&format!(" {}cut", count(clipped)));
    }
    if warns > 0 {
        s.push_str(&format!(" {}warn", count(warns)));
    }
    s
}

/// The text between `[tag]` and `[/tag]`, if both are present.
pub(crate) fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("[{tag}]");
    let close = format!("[/{tag}]");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(&text[start..end])
}

/// `RICO8C` — the native clipboard magic, distinct from `RICO8A` (on-disk assets).
const MAGIC: &[u8; 6] = b"RICO8C";
/// Native clipboard format version; bump if `ClipboardPayload` changes shape.
const VERSION: u8 = 1;

/// One copied item, serialized losslessly via serde/postcard. Reuses the asset
/// structs, so `Sfx::custom_wave`, sprite flags, and 8-bit map tiles all survive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardPayload {
    /// A `w * h` pixel block plus one flag byte per covered 8x8 sprite.
    Sprite {
        w: u8,
        h: u8,
        pixels: Vec<u8>,
        flags: Vec<u8>,
    },
    /// One SFX, full fidelity (includes `custom_wave`). `slot` is the source slot.
    Sfx { slot: u8, sfx: crate::assets::Sfx },
    /// A music pattern plus the SFX its channels reference (as `(slot, sfx)`).
    Pattern {
        pattern: MusicPattern,
        sfx: Vec<(u8, crate::assets::Sfx)>,
    },
    /// A `w * h` block of 8-bit map tiles, row-major.
    Map { w: u8, h: u8, tiles: Vec<u8> },
}

/// Encode a payload as a `[rico8]<hex>[/rico8]` clipboard blob.
pub fn encode(payload: &ClipboardPayload) -> String {
    let mut body = MAGIC.to_vec();
    body.push(VERSION);
    // postcard only errors on a serializer fault, which these owned, finite
    // values cannot trigger.
    body.extend(postcard::to_allocvec(payload).expect("clipboard payload serializes"));
    format!("[rico8]{}[/rico8]", bytes_to_hex(&body))
}

/// Decode a clipboard string into a `Pasted`: a native `[rico8]` blob, else a
/// PICO-8 `[gfx]`/`[sfx]` blob. Any unrecognised or malformed text is an `Err`.
pub fn parse(text: &str) -> Result<Pasted> {
    if let Some(inner) = tagged(text, "rico8") {
        return Ok(decode_native(inner)?.into_pasted());
    }
    crate::pico8::parse_clipboard(text)
}

/// Decode the hex body of a `[rico8]` blob into a payload. Total / panic-free.
fn decode_native(inner: &str) -> Result<ClipboardPayload> {
    let bytes = hex_bytes(inner.trim());
    let body = bytes
        .strip_prefix(MAGIC.as_slice())
        .context("bad clipboard magic")?;
    let (&version, body) = body.split_first().context("missing clipboard version")?;
    if version != VERSION {
        anyhow::bail!("unsupported clipboard version {version}");
    }
    let payload: ClipboardPayload =
        postcard::from_bytes(body).context("malformed clipboard payload")?;
    match &payload {
        ClipboardPayload::Sprite { w, h, pixels, .. } => {
            let (w, h) = (*w as usize, *h as usize);
            if w == 0 || h == 0 || w > SHEET_W || h > SHEET_H || pixels.len() != w * h {
                anyhow::bail!("clipboard sprite dimensions inconsistent with pixel data");
            }
        }
        ClipboardPayload::Map { w, h, tiles } => {
            let (w, h) = (*w as usize, *h as usize);
            if w == 0 || h == 0 || w > MAP_W || h > MAP_H || tiles.len() != w * h {
                anyhow::bail!("clipboard map dimensions inconsistent with tile data");
            }
        }
        _ => {}
    }
    Ok(payload)
}

impl ClipboardPayload {
    /// Lower a decoded payload to the editor-facing `Pasted` outcome.
    fn into_pasted(self) -> Pasted {
        match self {
            ClipboardPayload::Sprite {
                w,
                h,
                pixels,
                flags,
            } => Pasted::Sprites {
                rect: PixelRect {
                    w: w as usize,
                    h: h as usize,
                    pixels,
                },
                flags: Some(flags),
            },
            ClipboardPayload::Sfx { slot, sfx } => Pasted::Sfx(SfxClip {
                records: vec![Slotted {
                    src: slot,
                    value: sfx,
                }],
                patterns: Vec::new(),
            }),
            ClipboardPayload::Pattern { pattern, sfx } => Pasted::Sfx(SfxClip {
                records: sfx
                    .into_iter()
                    .map(|(src, value)| Slotted { src, value })
                    .collect(),
                patterns: vec![pattern],
            }),
            ClipboardPayload::Map { w, h, tiles } => Pasted::Map {
                w: w as usize,
                h: h as usize,
                tiles,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{
        Assets, CustomWave, MusicPattern, Note, Sfx, SpriteSheet, NOTE_CUSTOM_FLAG, SFX_COUNT,
    };

    #[test]
    fn native_sprite_round_trips_with_flags() {
        let payload = ClipboardPayload::Sprite {
            w: 8,
            h: 8,
            pixels: (0..64).map(|i| (i % 16) as u8).collect(),
            flags: vec![0b1100_0011],
        };
        let Pasted::Sprites { rect, flags } = parse(&encode(&payload)).unwrap() else {
            panic!("not sprites")
        };
        assert_eq!((rect.w, rect.h), (8, 8));
        assert_eq!(rect.pixels[1], 1);
        assert_eq!(flags, Some(vec![0b1100_0011]));
    }

    #[test]
    fn native_sfx_round_trips_custom_wave() {
        let sfx = Sfx {
            custom_wave: Some(CustomWave {
                samples: [7; 32],
                bass: true,
            }),
            ..Sfx::default()
        };
        let payload = ClipboardPayload::Sfx {
            slot: 3,
            sfx: sfx.clone(),
        };
        let Pasted::Sfx(clip) = parse(&encode(&payload)).unwrap() else {
            panic!("not sfx")
        };
        assert_eq!(clip.records.len(), 1);
        assert_eq!(clip.records[0].src, 3);
        assert_eq!(clip.records[0].value, sfx); // custom_wave preserved.
        assert!(clip.patterns.is_empty());
    }

    #[test]
    fn native_pattern_round_trips_with_referenced_sfx() {
        let pattern = MusicPattern {
            channels: [Some(8), Some(9), None, None],
            loop_back: false,
            loop_start: false,
            stop_at_end: false,
        };
        let payload = ClipboardPayload::Pattern {
            pattern,
            sfx: vec![(8, Sfx::default()), (9, Sfx::default())],
        };
        let Pasted::Sfx(clip) = parse(&encode(&payload)).unwrap() else {
            panic!("not sfx")
        };
        assert_eq!(clip.records.len(), 2);
        assert_eq!(clip.patterns, vec![pattern]);
    }

    #[test]
    fn native_map_round_trips() {
        let payload = ClipboardPayload::Map {
            w: 3,
            h: 2,
            tiles: vec![1, 2, 3, 4, 5, 6],
        };
        let Pasted::Map { w, h, tiles } = parse(&encode(&payload)).unwrap() else {
            panic!("not map")
        };
        assert_eq!((w, h), (3, 2));
        assert_eq!(tiles, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn native_decode_rejects_bad_blobs_without_panicking() {
        assert!(parse("[rico8]zzzz[/rico8]").is_err()); // odd/non-hex body.
        assert!(parse("[rico8]00[/rico8]").is_err()); // too short for magic.
                                                      // Wrong magic (`RICO8X` + v1 + empty payload), hex-encoded.
        assert!(parse("[rico8]5249434f3858017d[/rico8]").is_err());
    }

    #[test]
    fn native_decode_rejects_short_sprite_pixels_without_panicking() {
        // 8x8 sprite with an empty pixel vec — should error, not panic during paste.
        let blob = encode(&ClipboardPayload::Sprite {
            w: 8,
            h: 8,
            pixels: vec![],
            flags: vec![],
        });
        assert!(parse(&blob).is_err());
    }

    #[test]
    fn native_decode_rejects_short_map_tiles_without_panicking() {
        // 4x4 map with an empty tile vec — should error, not panic during paste.
        let blob = encode(&ClipboardPayload::Map {
            w: 4,
            h: 4,
            tiles: vec![],
        });
        assert!(parse(&blob).is_err());
    }

    #[test]
    fn native_decode_rejects_wrong_version() {
        // Correct magic, version 0x02 (unknown), then a dummy byte.
        let blob = bytes_to_hex(b"RICO8C\x02\x00");
        let text = format!("[rico8]{blob}[/rico8]");
        assert!(parse(&text).is_err());
    }

    #[test]
    fn parse_still_accepts_pico8_gfx() {
        let Pasted::Sprites { rect, flags } = parse("[gfx]0202abcd[/gfx]").unwrap() else {
            panic!("not sprites")
        };
        assert_eq!((rect.w, rect.h), (2, 2));
        assert_eq!(flags, None); // PICO-8 carries no flags.
    }

    fn sfx_with_first_note(pitch: u8, vol: u8) -> Sfx {
        let mut s = Sfx::default();
        s.notes[0].pitch = pitch;
        s.notes[0].volume = vol;
        s
    }

    #[test]
    fn paste_sprites_blits_and_clips() {
        let mut sheet = SpriteSheet::default();
        let rect = PixelRect {
            w: 2,
            h: 2,
            pixels: vec![1, 2, 3, 4],
        };
        let r = paste_sprites(&mut sheet, &rect, 0, 0, None);
        assert_eq!(sheet.get(0, 0), 1);
        assert_eq!(sheet.get(1, 1), 4);
        assert_eq!(r.clipped, 0);
        // At the far corner only one pixel lands; three are clipped.
        let r2 = paste_sprites(&mut sheet, &rect, 127, 127, None);
        assert_eq!(sheet.get(127, 127), 1);
        assert_eq!(r2.clipped, 3);
    }

    #[test]
    fn paste_sfx_overwrites_from_selection_only() {
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        sfx[0] = sfx_with_first_note(40, 5); // pre-existing, must survive.
        let records = vec![Slotted {
            src: 8,
            value: sfx_with_first_note(12, 3),
        }];
        let r = paste_sfx(&mut sfx, &records, 3);
        assert_eq!(sfx[3].notes[0].pitch, 12);
        assert_eq!(sfx[0].notes[0].volume, 5); // untouched.
        assert!(r.summary.contains("SFX 3"));
    }

    #[test]
    fn paste_sfx_remaps_custom_instrument_refs() {
        // Record for src 5 has a note that plays src 4 as a custom instrument.
        let mut inst_note = Sfx::default();
        inst_note.notes[1] = Note {
            pitch: 20,
            wave: NOTE_CUSTOM_FLAG | 4,
            volume: 5,
            effect: 0,
        };
        let records = vec![
            Slotted {
                src: 4,
                value: sfx_with_first_note(1, 1),
            },
            Slotted {
                src: 5,
                value: inst_note,
            },
        ];
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        paste_sfx(&mut sfx, &records, 2); // src 4 -> 2, src 5 -> 3.
        assert_eq!(sfx[3].notes[1].wave, NOTE_CUSTOM_FLAG | 2);
    }

    #[test]
    fn paste_sfx_clips_at_capacity() {
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        let records = vec![
            Slotted {
                src: 0,
                value: sfx_with_first_note(1, 1),
            },
            Slotted {
                src: 1,
                value: sfx_with_first_note(2, 1),
            },
            Slotted {
                src: 2,
                value: sfx_with_first_note(3, 1),
            },
        ];
        let r = paste_sfx(&mut sfx, &records, 62); // only 62, 63 fit.
        assert_eq!(sfx[62].notes[0].pitch, 1);
        assert_eq!(sfx[63].notes[0].pitch, 2);
        assert_eq!(r.clipped, 1);
        assert!(r.summary.contains("cut"));
    }

    #[test]
    fn summaries_fit_the_status_bar() {
        use crate::{fb::WIDTH, font::text_width};
        // Worst cases: a wide slot range with everything clipped and many warns.
        let budget = WIDTH - 2; // the bar's usable pixel width.
        assert!(text_width(&seq_summary("SFX", 10, 54, 255, 2048)) <= budget);
        assert!(text_width(&rect_summary(128, 128, 16384, 255)) <= budget);
        assert!(text_width(&pattern_summary(63, 1, 64, 255, 2048)) <= budget);
    }

    #[test]
    fn sprite_paste_summary_names_destination() {
        let mut sheet = SpriteSheet::default();
        let rect = PixelRect {
            w: 8,
            h: 8,
            pixels: vec![1; 64],
        };
        // Sprite 1 lives at sheet pixel (8, 0).
        let r = paste_sprites(&mut sheet, &rect, 8, 0, None);
        assert_eq!(r.summary, "pasted 8x8 spr 1");
    }

    #[test]
    fn paste_sprites_applies_flags_when_present() {
        let mut sheet = SpriteSheet::default();
        let rect = PixelRect {
            w: 8,
            h: 8,
            pixels: vec![5; 64],
        };
        // Sprite 1 sits at sheet pixel (8, 0).
        paste_sprites(&mut sheet, &rect, 8, 0, Some(&[0b1010_0101]));
        assert_eq!(sheet.flags(1), 0b1010_0101);
        // Without flags, the destination flag byte is left as-is.
        sheet.set_flag(2, 0, true);
        paste_sprites(&mut sheet, &rect, 16, 0, None);
        assert_eq!(sheet.flags(2), 0b0000_0001);
    }

    #[test]
    fn paste_pattern_appends_sfx_and_rewires_channels() {
        let mut assets = Assets::default();
        assets.sfx[10] = sfx_with_first_note(60, 5); // marks slot 10 used.
        let clip = SfxClip {
            records: vec![
                Slotted {
                    src: 8,
                    value: sfx_with_first_note(1, 4),
                },
                Slotted {
                    src: 9,
                    value: sfx_with_first_note(2, 4),
                },
            ],
            patterns: vec![MusicPattern {
                channels: [Some(8), Some(9), None, None],
                loop_back: false,
                loop_start: false,
                stop_at_end: false,
            }],
        };
        let r = paste_pattern(&mut assets, &clip, 5);
        // SFX appended after the last used slot (10) -> 11, 12; slot 10 intact.
        assert_eq!(assets.sfx[11].notes[0].pitch, 1);
        assert_eq!(assets.sfx[12].notes[0].pitch, 2);
        assert_eq!(assets.sfx[10].notes[0].pitch, 60);
        // The pattern lands at slot 5 with channels remapped to 11, 12.
        assert_eq!(assets.music[5].channels, [Some(11), Some(12), None, None]);
        assert!(r.warnings.is_empty());
    }
}
