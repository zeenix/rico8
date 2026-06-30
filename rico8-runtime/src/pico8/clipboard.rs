//! Decoding PICO-8's editor clipboard formats — the `[gfx]` and `[sfx]`
//! tagged-hex blobs — into RICO-8 assets, for paste-into-editor.
//!
//! PICO-8 has no `[music]` tag: copying a song pattern emits a `[sfx]` blob —
//! the pattern's SFX as records, plus a trailing four-byte channel footer that
//! rebuilds the pattern. The active editor decides how to consume an `[sfx]`
//! blob (the SFX editor pastes the SFX; the music editor rebuilds the pattern).

use super::{hex, hex_bytes, music_from_mem, sfx_from_mem, SFX_MEM_LEN};
use crate::{
    assets::{SHEET_H, SHEET_W},
    clipboard::{tagged, Pasted, PixelRect, SfxClip, Slotted},
};
use anyhow::{bail, Result};

/// Decode a PICO-8 editor clipboard string. Recognises `[gfx]` and `[sfx]`
/// blobs; anything else is an error.
pub fn parse_clipboard(text: &str) -> Result<Pasted> {
    if let Some(inner) = tagged(text, "gfx") {
        return Ok(Pasted::Sprites {
            rect: parse_gfx(inner)?,
            flags: None,
        });
    }
    if let Some(inner) = tagged(text, "sfx") {
        return Ok(Pasted::Sfx(parse_sfx(inner)?));
    }
    bail!("no sprite or sound on the clipboard");
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
        let Pasted::Sprites { rect: r, .. } = p else {
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

    #[test]
    fn footer_only_sfx_blob_decodes_to_an_empty_pattern() {
        let Pasted::Sfx(clip) = parse_clipboard("[sfx]000140404040[/sfx]").unwrap() else {
            panic!("not sfx")
        };
        assert!(clip.records.is_empty());
        assert_eq!(clip.patterns.len(), 1);
        assert_eq!(clip.patterns[0].channels, [None, None, None, None]);
    }
}
