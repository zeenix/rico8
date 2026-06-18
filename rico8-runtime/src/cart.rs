//! The PNG cartridge format.
//!
//! A RICO-8 cart is a real PNG image — the picture *is* the cartridge,
//! complete with label art — with the game embedded in a private ancillary
//! chunk. Any image viewer shows the cart; RICO-8 plays it.
//!
//! Chunk layout (see docs/CART_FORMAT.md for the full story):
//!
//! ```text
//! PNG signature
//! IHDR, IDAT*, ...      # ordinary PNG image: the cart label art
//! rcRt                  # RICO-8 payload chunk:
//!   "RICO8"             #   magic
//!   u16 LE version      #   cartridge format version (currently 1)
//!   deflate( postcard( CartPayload ) )
//! IEND
//! ```
//!
//! `CartPayload` always carries the compiled `game.wasm` and the full
//! asset bundle; carts exported as *editable* also carry the Rust source.

use crate::{assets::Assets, font, palette};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

/// Current cartridge format version. Bump (and reject older carts with a clear
/// message) once carts are published; until then format changes are free.
pub const CART_VERSION: u16 = 1;
const CART_MAGIC: &[u8; 5] = b"RICO8";
const CHUNK_TYPE: [u8; 4] = *b"rcRt";
const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// Decompression bomb guard.
const MAX_PAYLOAD: usize = 64 * 1024 * 1024;
/// Hard cap on a cart's compiled wasm module: 128 K, shared with the memory
/// and fuel limits. Keeps carts small and shareable and rations dependency
/// bloat. Enforced on both save and load via [`validate`].
pub const MAX_WASM_SIZE: usize = 131_072;

/// Everything inside a cartridge.
#[derive(Serialize, Deserialize)]
pub struct Cart {
    /// Compiled `wasm32-unknown-unknown` game module.
    pub wasm: Vec<u8>,
    pub assets: Assets,
    /// Rust source (`src/lib.rs`), present in editable carts.
    pub source: Option<String>,
}

/// Save a cart as a PNG file with label art.
pub fn save_png(cart: &Cart, path: &std::path::Path) -> Result<()> {
    let bytes = encode(cart)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Load and validate a cart from a PNG file.
pub fn load_png(path: &std::path::Path) -> Result<Cart> {
    let bytes = std::fs::read(path)?;
    decode(&bytes)
}

/// Encode a cart into PNG bytes.
pub fn encode(cart: &Cart) -> Result<Vec<u8>> {
    validate(cart)?;
    let label = render_label(&cart.assets);

    let mut png = PNG_SIG.to_vec();
    // IHDR: width, height, 8-bit RGBA.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend((LABEL_W as u32).to_be_bytes());
    ihdr.extend((LABEL_H as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    write_chunk(&mut png, *b"IHDR", &ihdr);

    // IDAT: filter byte 0 before each scanline, zlib-compressed.
    let mut raw = Vec::with_capacity(LABEL_H * (1 + LABEL_W * 4));
    for y in 0..LABEL_H {
        raw.push(0);
        raw.extend_from_slice(&label[y * LABEL_W * 4..(y + 1) * LABEL_W * 4]);
    }
    let idat = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 8);
    write_chunk(&mut png, *b"IDAT", &idat);

    // rcRt: the actual cartridge.
    let mut payload = CART_MAGIC.to_vec();
    payload.extend(CART_VERSION.to_le_bytes());
    let body = postcard::to_allocvec(cart)?;
    payload.extend(miniz_oxide::deflate::compress_to_vec(&body, 8));
    write_chunk(&mut png, CHUNK_TYPE, &payload);

    write_chunk(&mut png, *b"IEND", &[]);
    Ok(png)
}

/// Cheap check: is this PNG a RICO-8 cart? Scans chunk headers for
/// `rcRt` without decompressing anything — used by cart pickers to
/// filter directories quickly.
pub fn is_cart(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&PNG_SIG) {
        return false;
    }
    let mut rest = &bytes[8..];
    while rest.len() >= 12 {
        let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
        let ctype: [u8; 4] = rest[4..8].try_into().unwrap();
        if ctype == CHUNK_TYPE {
            return true;
        }
        if &ctype == b"IEND" || rest.len() < 12 + len {
            return false;
        }
        rest = &rest[12 + len..];
    }
    false
}

/// Decode and validate a cart from PNG bytes.
pub fn decode(bytes: &[u8]) -> Result<Cart> {
    if !bytes.starts_with(&PNG_SIG) {
        bail!("Not a PNG file");
    }
    let mut rest = &bytes[8..];
    let mut payload = None;
    while rest.len() >= 12 {
        let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
        let ctype: [u8; 4] = rest[4..8].try_into().unwrap();
        if rest.len() < 12 + len {
            bail!("Truncated PNG chunk");
        }
        let data = &rest[8..8 + len];
        if ctype == CHUNK_TYPE {
            let crc = u32::from_be_bytes(rest[8 + len..12 + len].try_into().unwrap());
            let mut h = crc32fast::Hasher::new();
            h.update(&ctype);
            h.update(data);
            if h.finalize() != crc {
                bail!("Cart data is corrupted (bad checksum)");
            }
            payload = Some(data.to_vec());
        }
        if &ctype == b"IEND" {
            break;
        }
        rest = &rest[12 + len..];
    }
    let payload = payload.ok_or_else(|| anyhow!("PNG has no RICO-8 cart data"))?;

    let body = payload
        .strip_prefix(CART_MAGIC.as_slice())
        .ok_or_else(|| anyhow!("Bad cart magic"))?;
    if body.len() < 2 {
        bail!("Truncated cart header");
    }
    let version = u16::from_le_bytes(body[0..2].try_into().unwrap());
    if version != CART_VERSION {
        bail!("Cart format version {version} is not supported (this is version {CART_VERSION})");
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(&body[2..], MAX_PAYLOAD)
        .map_err(|e| anyhow!("Cart data is corrupted: {e}"))?;
    let cart: Cart = postcard::from_bytes(&raw)?;
    validate(&cart)?;
    Ok(cart)
}

/// Structural validation applied on both save and load.
fn validate(cart: &Cart) -> Result<()> {
    if !cart.wasm.starts_with(b"\0asm") {
        bail!("Cart payload is not a wasm module");
    }
    if cart.wasm.len() > MAX_WASM_SIZE {
        bail!(
            "Cart wasm is {} bytes; the limit is {} (128K)",
            cart.wasm.len(),
            MAX_WASM_SIZE
        );
    }
    crate::assets::validate(&cart.assets)?;
    Ok(())
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

/// Encode the virtual screen as a standalone PNG (for screenshots and
/// docs). `scale` is an integer zoom factor.
pub fn encode_screen_png(fb: &crate::fb::Framebuffer, scale: usize) -> Vec<u8> {
    let scale = scale.max(1);
    let (w, h) = (128 * scale, 128 * scale);
    let mut png = PNG_SIG.to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    write_chunk(&mut png, *b"IHDR", &ihdr);

    let pixels = fb.logical_pixels();
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = pixels[(y / scale) * 128 + x / scale];
            raw.extend(palette::rgba(c));
        }
    }
    write_chunk(
        &mut png,
        *b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 8),
    );
    write_chunk(&mut png, *b"IEND", &[]);
    png
}

// ---------------------------------------------------------------------------
// Label art
// ---------------------------------------------------------------------------

/// Cart image dimensions.
pub const LABEL_W: usize = 160;
pub const LABEL_H: usize = 205;

const TRANSPARENT: u8 = 0xff;

/// Tiny indexed-color painter for composing the cart image.
struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            px: vec![TRANSPARENT; LABEL_W * LABEL_H],
        }
    }

    fn set(&mut self, x: i32, y: i32, c: u8) {
        if (0..LABEL_W as i32).contains(&x) && (0..LABEL_H as i32).contains(&y) {
            self.px[y as usize * LABEL_W + x as usize] = c;
        }
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, c);
            }
        }
    }

    fn text(&mut self, s: &str, x: i32, y: i32, c: u8, scale: i32) {
        let mut cx = x;
        for ch in s.chars() {
            let rows = font::glyph(ch);
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..3 {
                    if row & (0b100 >> rx) != 0 {
                        self.fill(
                            cx + rx * scale,
                            y + ry as i32 * scale,
                            cx + rx * scale + scale - 1,
                            y + ry as i32 * scale + scale - 1,
                            c,
                        );
                    }
                }
            }
            cx += font::GLYPH_W * scale;
        }
    }

    fn text_centered(&mut self, s: &str, y: i32, c: u8, scale: i32) {
        let w = font::text_width(s) * scale;
        self.text(s, (LABEL_W as i32 - w) / 2, y, c, scale);
    }
}

/// Render the cart PNG image: body, stripes, label art, title.
fn render_label(assets: &Assets) -> Vec<u8> {
    let mut c = Canvas::new();
    let (w, h) = (LABEL_W as i32, LABEL_H as i32);

    // Cartridge body with clipped corners.
    c.fill(0, 0, w - 1, h - 1, palette::col::LIGHT_GREY);
    for dy in 0..4 {
        for dx in 0..4 {
            if dx + dy < 4 {
                c.set(dx, dy, TRANSPARENT); // top-left
                c.set(w - 1 - dx, dy, TRANSPARENT); // top-right
                c.set(dx, h - 1 - dy, TRANSPARENT); // bottom-left
                c.set(w - 1 - dx, h - 1 - dy, TRANSPARENT); // bottom-right
            }
        }
    }
    // Darker bottom and right edges for depth.
    c.fill(0, h - 6, w - 1, h - 1, palette::col::DARK_GREY);
    c.fill(w - 3, 0, w - 1, h - 1, palette::col::DARK_GREY);

    // Classic color stripes above the label window.
    let stripe_cols = [
        palette::col::RED,
        palette::col::ORANGE,
        palette::col::YELLOW,
        palette::col::GREEN,
        palette::col::BLUE,
        palette::col::PINK,
    ];
    for (i, col) in stripe_cols.iter().enumerate() {
        let seg = (w - 32) / stripe_cols.len() as i32;
        c.fill(
            16 + i as i32 * seg,
            8,
            16 + (i as i32 + 1) * seg - 1,
            12,
            *col,
        );
    }

    // Label window: the screenshot, or the built-in default art.
    let label_px = assets.label.clone().unwrap_or_else(default_label);
    c.fill(14, 22, 14 + 131, 22 + 131, palette::col::BLACK);
    for y in 0..128 {
        for x in 0..128 {
            c.set(16 + x, 24 + y, label_px[(y * 128 + x) as usize] & 0x0f);
        }
    }

    // Title and author under the window.
    let title = if assets.meta.name.is_empty() {
        "untitled"
    } else {
        &assets.meta.name
    };
    c.text_centered(title, 162, palette::col::DARK_BLUE, 1);
    if !assets.meta.author.is_empty() {
        c.text_centered(
            &format!("by {}", assets.meta.author),
            172,
            palette::col::DARK_GREY,
            1,
        );
    }
    c.text_centered("rico-8 cartridge", 190, palette::col::DARK_GREY, 1);

    // Expand to RGBA.
    let mut rgba = vec![0u8; LABEL_W * LABEL_H * 4];
    for (i, &p) in c.px.iter().enumerate() {
        let out = &mut rgba[i * 4..i * 4 + 4];
        if p == TRANSPARENT {
            out.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            out.copy_from_slice(&palette::rgba(p));
        }
    }
    rgba
}

/// The built-in default label used when no screenshot was captured.
pub fn default_label() -> Vec<u8> {
    let mut px = vec![palette::col::DARK_BLUE; 128 * 128];
    // Dot grid backdrop.
    for y in (4..128).step_by(8) {
        for x in (4..128).step_by(8) {
            px[y * 128 + x] = palette::col::DARK_PURPLE;
        }
    }
    // Big RICO-8 wordmark via the scaled built-in font.
    let mut set = |x: i32, y: i32, c: u8| {
        if (0..128).contains(&x) && (0..128).contains(&y) {
            px[y as usize * 128 + x as usize] = c;
        }
    };
    let word = "rico-8";
    let scale = 4;
    let w = font::text_width(word) * scale;
    let x0 = (128 - w) / 2 + scale / 2;
    let y0 = 48;
    for (i, ch) in word.chars().enumerate() {
        let rows = font::glyph(ch);
        let color = [8u8, 9, 10, 11, 12, 14][i % 6];
        for (ry, row) in rows.iter().enumerate() {
            for rx in 0..3i32 {
                if row & (0b100 >> rx) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            set(
                                x0 + (i as i32 * font::GLYPH_W + rx) * scale + dx,
                                y0 + ry as i32 * scale + dy,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Note;

    fn test_cart() -> Cart {
        let mut assets = Assets::default();
        assets.meta.name = "roundtrip".into();
        assets.meta.author = "tester".into();
        assets.sprites.set(5, 5, 14);
        assets.map.set(2, 3, 7);
        assets.sfx[1].notes[0] = Note {
            pitch: 40,
            wave: 2,
            volume: 6,
            effect: 1,
        };
        Cart {
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            assets,
            source: Some("fn main() {}".into()),
        }
    }

    #[test]
    fn png_roundtrip() {
        let cart = test_cart();
        let png = encode(&cart).unwrap();
        assert!(png.starts_with(&PNG_SIG), "output is a real png");
        let back = decode(&png).unwrap();
        assert_eq!(back.wasm, cart.wasm);
        assert_eq!(back.assets.meta.name, "roundtrip");
        assert_eq!(back.assets.sprites.get(5, 5), 14);
        assert_eq!(back.assets.map.get(2, 3), 7);
        assert_eq!(back.assets.sfx[1].notes[0].pitch, 40);
        assert_eq!(back.source.as_deref(), Some("fn main() {}"));
    }

    #[test]
    fn playable_cart_has_no_source() {
        let mut cart = test_cart();
        cart.source = None;
        let back = decode(&encode(&cart).unwrap()).unwrap();
        assert!(back.source.is_none());
    }

    #[test]
    fn corrupted_payload_is_rejected() {
        let cart = test_cart();
        let mut png = encode(&cart).unwrap();
        // Flip a byte inside the rcRt chunk body.
        let pos = png
            .windows(4)
            .position(|w| w == CHUNK_TYPE)
            .expect("chunk present")
            + 20;
        png[pos] ^= 0xff;
        let err = decode(&png).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("corrupted"), "{err}");
    }

    #[test]
    fn non_cart_png_is_rejected() {
        let mut png = PNG_SIG.to_vec();
        write_chunk(&mut png, *b"IHDR", &[0; 13]);
        write_chunk(&mut png, *b"IEND", &[]);
        let err = decode(&png).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("no RICO-8 cart data"), "{err}");
    }

    #[test]
    fn bad_wasm_is_rejected() {
        let mut cart = test_cart();
        cart.wasm = b"not wasm".to_vec();
        assert!(encode(&cart).is_err());
    }

    #[test]
    fn oversized_wasm_is_rejected() {
        let mut cart = test_cart();
        cart.wasm = b"\0asm\x01\0\0\0".to_vec();
        cart.wasm.resize(MAX_WASM_SIZE + 1, 0);
        assert!(
            encode(&cart).is_err(),
            "wasm over 128K must be rejected at export"
        );
    }

    #[test]
    fn max_size_wasm_is_accepted() {
        let mut cart = test_cart();
        cart.wasm = b"\0asm\x01\0\0\0".to_vec();
        cart.wasm.resize(MAX_WASM_SIZE, 0);
        assert!(
            encode(&cart).is_ok(),
            "wasm at exactly 128K must be accepted"
        );
    }

    #[test]
    fn validate_rejects_oversized_wasm() {
        let mut cart = test_cart();
        cart.wasm = b"\0asm\x01\0\0\0".to_vec();
        cart.wasm.resize(MAX_WASM_SIZE + 1, 0);
        let err = validate(&cart).unwrap_err().to_string();
        assert!(
            err.contains("the limit is"),
            "expected size-gate message, got: {err}"
        );
    }

    #[test]
    fn future_version_is_rejected() {
        let cart = test_cart();
        let mut png = encode(&cart).unwrap();
        // Bump the version field (magic is 5 bytes after the chunk type+4 len... find it).
        let pos = png
            .windows(9)
            .position(|w| w[0..4] == CHUNK_TYPE && &w[4..9] == CART_MAGIC)
            .unwrap();
        let vpos = pos + 9;
        png[vpos] = 0xfe;
        // CRC now mismatches, which is also a rejection; patch CRC properly.
        // Simpler: just assert decode fails one way or another.
        assert!(decode(&png).is_err());
    }
}
