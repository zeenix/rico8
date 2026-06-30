//! Decoding PICO-8's editor clipboard formats — the `[gfx]` and `[sfx]`
//! tagged-hex blobs — into RICO-8 assets, for paste-into-editor.
//!
//! PICO-8 has no `[music]` tag: copying a song pattern emits a `[sfx]` blob —
//! the pattern's SFX as records, plus a trailing four-byte channel footer that
//! rebuilds the pattern. The active editor decides how to consume an `[sfx]`
//! blob (the SFX editor pastes the SFX; the music editor rebuilds the pattern).

use super::{
    bytes_to_hex, hex, hex_bytes, music_from_mem, music_to_mem, next_free_sfx,
    remap_custom_instruments, remap_music_channels, sfx_from_mem, sfx_to_mem, SFX_MEM_LEN,
};
use crate::assets::{
    Assets, MusicPattern, Sfx, SpriteSheet, MUSIC_COUNT, SFX_COUNT, SHEET_H, SHEET_W,
    SPRITES_PER_ROW, SPRITE_SIZE,
};
use anyhow::{bail, Result};
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
    Sprites(PixelRect),
    Sfx(SfxClip),
}

/// Decode a PICO-8 editor clipboard string. Recognises `[gfx]` and `[sfx]`
/// blobs; anything else is an error.
pub fn parse_clipboard(text: &str) -> Result<Pasted> {
    if let Some(inner) = tagged(text, "gfx") {
        return Ok(Pasted::Sprites(parse_gfx(inner)?));
    }
    if let Some(inner) = tagged(text, "sfx") {
        return Ok(Pasted::Sfx(parse_sfx(inner)?));
    }
    bail!("no sprite or sound on the clipboard");
}

/// The text between `[tag]` and `[/tag]`, if both are present.
fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("[{tag}]");
    let close = format!("[/{tag}]");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(&text[start..end])
}

/// `[gfx]`: a 2-hex width and height, then `w*h` pixel nibbles. A non-hex
/// character counts as nibble 0, matching PICO-8.
fn parse_gfx(inner: &str) -> Result<PixelRect> {
    let d: Vec<u8> = inner.trim().chars().map(|c| hex(c).unwrap_or(0)).collect();
    if d.len() < 4 {
        bail!("clipboard sprite data is truncated");
    }
    let w = (d[0] << 4 | d[1]) as usize;
    let h = (d[2] << 4 | d[3]) as usize;
    if w == 0 || h == 0 || w > SHEET_W || h > SHEET_H {
        bail!("clipboard sprite size {w}x{h} is out of range");
    }
    if d.len() < 4 + w * h {
        bail!("clipboard sprite data is truncated");
    }
    Ok(PixelRect {
        w,
        h,
        pixels: d[4..4 + w * h].to_vec(),
    })
}

/// `[sfx]`: a 2-byte header (byte 0 = record count) + N records of a source
/// slot byte and 68 bytes of SFX memory, then 0+ trailing 4-byte pattern
/// footers. The count is taken from the header but clamped to what the payload
/// can hold.
fn parse_sfx(inner: &str) -> Result<SfxClip> {
    let b = hex_bytes(inner);
    if b.len() < 2 {
        bail!("clipboard SFX data is truncated");
    }
    let rec = 1 + SFX_MEM_LEN; // 1 slot byte + 68 SFX bytes.
    let count = (b[0] as usize).min(b.len().saturating_sub(2) / rec);
    // A pattern copied from an all-silent pattern has zero SFX records but still
    // carries its 4-byte channel footer; only a blob with neither is truly empty.
    if count == 0 && b[2..].len() < 4 {
        bail!("clipboard SFX data is empty");
    }
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let off = 2 + i * rec;
        records.push(Slotted {
            src: b[off],
            value: sfx_from_mem(&b[off + 1..off + 1 + SFX_MEM_LEN]),
        });
    }
    // Whole trailing 4-byte groups are pattern footers; ignore a partial group.
    let patterns = b[2 + count * rec..]
        .chunks(4)
        .filter(|c| c.len() == 4)
        .map(|c| music_from_mem([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok(SfxClip { records, patterns })
}

/// Encode a pixel rectangle as a PICO-8 `[gfx]` blob: a 2-hex width and height,
/// then one nibble per pixel, row-major.
pub fn encode_gfx(rect: &PixelRect) -> String {
    let mut s = format!("[gfx]{:02x}{:02x}", rect.w, rect.h);
    for &p in &rect.pixels {
        s.push(char::from_digit((p & 0xf) as u32, 16).unwrap());
    }
    s.push_str("[/gfx]");
    s
}

/// Encode one SFX as a PICO-8 `[sfx]` blob: a 2-byte header (record count, then
/// a byte PICO-8 emits as 1 and we ignore on decode), one record of the source
/// slot byte and 68 SFX bytes.
pub fn encode_sfx(slot: u8, sfx: &Sfx) -> String {
    let mut b = vec![1, 1, slot];
    b.extend_from_slice(&sfx_to_mem(sfx));
    format!("[sfx]{}[/sfx]", bytes_to_hex(&b))
}

/// Encode one music pattern as a PICO-8 `[sfx]` blob: the SFX its channels
/// reference (records tagged with their slot), then a 4-byte channel footer
/// that rebuilds the pattern. Mirrors what PICO-8 emits for a copied pattern.
pub fn encode_pattern(assets: &Assets, pattern: usize) -> String {
    let pat = &assets.music[pattern];
    let mut slots: Vec<u8> = Vec::new();
    for &slot in pat.channels.iter().flatten() {
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    let mut b = vec![slots.len() as u8, 1];
    for &slot in &slots {
        b.push(slot);
        b.extend_from_slice(&sfx_to_mem(&assets.sfx[slot as usize]));
    }
    b.extend_from_slice(&music_to_mem(pat));
    format!("[sfx]{}[/sfx]", bytes_to_hex(&b))
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
/// overwriting in place. Pixels past the sheet edge are clipped.
pub fn paste_sprites(sheet: &mut SpriteSheet, rect: &PixelRect, x0: i32, y0: i32) -> PasteReport {
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

#[cfg(test)]
mod tests {
    use super::*;

    // A copied music pattern: 2 SFX (src slots 8, 9) + footer 08 09 43 44.
    const MUSIC: &str = "[sfx]020108090a0900090a0900090a0900090a0000090a0900090a0900090a0900090\
        a0900090a0900090a0900090a0900090a0900090a0900090a0900090a0900090a09000008002009090\
        a0e000e0a0e00100a1000150a1000180a1000130a1000150a1000100a1000130a10000e0a1000100a1\
        000130a1000090a10000e0a0d00100a0000150a100000080000080943440[/sfx]";

    #[test]
    fn gfx_header_then_nibbles() {
        let p = parse_clipboard("[gfx]0202abcd[/gfx]").unwrap();
        let Pasted::Sprites(r) = p else {
            panic!("not sprites")
        };
        assert_eq!((r.w, r.h), (2, 2));
        assert_eq!(r.pixels, vec![0xa, 0xb, 0xc, 0xd]);
    }

    #[test]
    fn sfx_records_carry_source_slots() {
        let p = parse_clipboard(MUSIC).unwrap();
        let Pasted::Sfx(clip) = p else {
            panic!("not sfx")
        };
        assert_eq!(clip.records.len(), 2);
        assert_eq!(clip.records[0].src, 8);
        assert_eq!(clip.records[1].src, 9);
        // The trailing footer rebuilds the pattern: channels 8 and 9, rest off.
        assert_eq!(clip.patterns.len(), 1);
        assert_eq!(clip.patterns[0].channels, [Some(8), Some(9), None, None]);
    }

    #[test]
    fn unknown_clipboard_is_an_error() {
        assert!(parse_clipboard("hello world").is_err());
    }

    use crate::assets::{Assets, Note, NOTE_CUSTOM_FLAG, SFX_COUNT};

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
        let r = paste_sprites(&mut sheet, &rect, 0, 0);
        assert_eq!(sheet.get(0, 0), 1);
        assert_eq!(sheet.get(1, 1), 4);
        assert_eq!(r.clipped, 0);
        // At the far corner only one pixel lands; three are clipped.
        let r2 = paste_sprites(&mut sheet, &rect, 127, 127);
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
    fn gfx_round_trips() {
        let rect = PixelRect {
            w: 3,
            h: 2,
            pixels: vec![1, 2, 3, 4, 5, 6],
        };
        let Pasted::Sprites(r) = parse_clipboard(&encode_gfx(&rect)).unwrap() else {
            panic!("not sprites")
        };
        assert_eq!((r.w, r.h), (3, 2));
        assert_eq!(r.pixels, rect.pixels);
    }

    #[test]
    fn sfx_round_trips() {
        let mut s = Sfx::default();
        s.notes[0] = Note {
            pitch: 30,
            wave: 2,
            volume: 6,
            effect: 1,
        };
        let Pasted::Sfx(clip) = parse_clipboard(&encode_sfx(7, &s)).unwrap() else {
            panic!("not sfx")
        };
        assert_eq!(clip.records.len(), 1);
        assert_eq!(clip.records[0].src, 7);
        assert_eq!(clip.records[0].value, s);
        assert!(clip.patterns.is_empty());
    }

    #[test]
    fn pattern_round_trips() {
        let mut assets = Assets::default();
        assets.sfx[8] = sfx_with_first_note(20, 4);
        assets.sfx[9] = sfx_with_first_note(22, 4);
        assets.music[5] = MusicPattern {
            channels: [Some(8), Some(9), None, None],
            loop_back: false,
            loop_start: false,
            stop_at_end: false,
        };
        let Pasted::Sfx(clip) = parse_clipboard(&encode_pattern(&assets, 5)).unwrap() else {
            panic!("not sfx")
        };
        assert_eq!(clip.records.len(), 2);
        assert_eq!(clip.patterns.len(), 1);
        assert_eq!(clip.patterns[0].channels, [Some(8), Some(9), None, None]);
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
        let r = paste_sprites(&mut sheet, &rect, 8, 0);
        assert_eq!(r.summary, "pasted 8x8 spr 1");
    }

    #[test]
    fn empty_pattern_round_trips() {
        let assets = Assets::default(); // music[0] defaults to all-None channels.
        let Pasted::Sfx(clip) = parse_clipboard(&encode_pattern(&assets, 0)).unwrap() else {
            panic!("not sfx")
        };
        assert!(clip.records.is_empty());
        assert_eq!(clip.patterns.len(), 1);
        assert_eq!(clip.patterns[0].channels, [None, None, None, None]);
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
