//! RICO-8's built-in pixel font.
//!
//! An original 3x5 glyph set in a 4x6 cell, in the same spirit as classic
//! fantasy-console fonts. Lowercase letters render as small caps, which is
//! part of the look. Each glyph is five rows; in every row the leftmost
//! pixel is bit 2 (`0b100`).

/// Advance width of one character cell in pixels.
pub const GLYPH_W: i32 = 4;
/// Line height in pixels.
pub const GLYPH_H: i32 = 6;

/// Glyph used for characters outside the printable ASCII range.
pub const UNKNOWN: [u8; 5] = [0b111, 0b111, 0b111, 0b111, 0b111];

/// Look up the glyph rows for a character.
pub fn glyph(c: char) -> [u8; 5] {
    let c = match c {
        'a'..='z' => ((c as u8) - b'a' + b'A') as char,
        _ => c,
    };
    let i = c as u32;
    if (32..127).contains(&i) {
        GLYPHS[(i - 32) as usize]
    } else {
        UNKNOWN
    }
}

/// Pixel width of a string when printed with the built-in font.
pub fn text_width(s: &str) -> i32 {
    s.chars().count() as i32 * GLYPH_W
}

#[rustfmt::skip]
const GLYPHS: [[u8; 5]; 95] = [
    [0b000, 0b000, 0b000, 0b000, 0b000], // space
    [0b010, 0b010, 0b010, 0b000, 0b010], // !
    [0b101, 0b101, 0b000, 0b000, 0b000], // "
    [0b101, 0b111, 0b101, 0b111, 0b101], // #
    [0b111, 0b110, 0b111, 0b011, 0b111], // $
    [0b101, 0b001, 0b010, 0b100, 0b101], // %
    [0b010, 0b101, 0b010, 0b101, 0b011], // &
    [0b010, 0b100, 0b000, 0b000, 0b000], // '
    [0b001, 0b010, 0b010, 0b010, 0b001], // (
    [0b100, 0b010, 0b010, 0b010, 0b100], // )
    [0b101, 0b010, 0b111, 0b010, 0b101], // *
    [0b000, 0b010, 0b111, 0b010, 0b000], // +
    [0b000, 0b000, 0b000, 0b010, 0b100], // ,
    [0b000, 0b000, 0b111, 0b000, 0b000], // -
    [0b000, 0b000, 0b000, 0b000, 0b010], // .
    [0b001, 0b001, 0b010, 0b100, 0b100], // /
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b110, 0b010, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b011, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b011, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b001, 0b001, 0b001], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b110], // 9
    [0b000, 0b010, 0b000, 0b010, 0b000], // :
    [0b000, 0b010, 0b000, 0b010, 0b100], // ;
    [0b001, 0b010, 0b100, 0b010, 0b001], // <
    [0b000, 0b111, 0b000, 0b111, 0b000], // =
    [0b100, 0b010, 0b001, 0b010, 0b100], // >
    [0b111, 0b001, 0b011, 0b000, 0b010], // ?
    [0b111, 0b101, 0b111, 0b100, 0b011], // @
    [0b111, 0b101, 0b111, 0b101, 0b101], // A
    [0b111, 0b101, 0b110, 0b101, 0b111], // B
    [0b011, 0b100, 0b100, 0b100, 0b011], // C
    [0b110, 0b101, 0b101, 0b101, 0b110], // D
    [0b111, 0b100, 0b110, 0b100, 0b111], // E
    [0b111, 0b100, 0b110, 0b100, 0b100], // F
    [0b011, 0b100, 0b101, 0b101, 0b011], // G
    [0b101, 0b101, 0b111, 0b101, 0b101], // H
    [0b111, 0b010, 0b010, 0b010, 0b111], // I
    [0b111, 0b010, 0b010, 0b010, 0b110], // J
    [0b101, 0b101, 0b110, 0b101, 0b101], // K
    [0b100, 0b100, 0b100, 0b100, 0b111], // L
    [0b111, 0b111, 0b101, 0b101, 0b101], // M
    [0b110, 0b101, 0b101, 0b101, 0b101], // N
    [0b111, 0b101, 0b101, 0b101, 0b111], // O
    [0b111, 0b101, 0b111, 0b100, 0b100], // P
    [0b111, 0b101, 0b101, 0b111, 0b001], // Q
    [0b111, 0b101, 0b110, 0b101, 0b101], // R
    [0b011, 0b100, 0b111, 0b001, 0b110], // S
    [0b111, 0b010, 0b010, 0b010, 0b010], // T
    [0b101, 0b101, 0b101, 0b101, 0b111], // U
    [0b101, 0b101, 0b101, 0b101, 0b010], // V
    [0b101, 0b101, 0b101, 0b111, 0b111], // W
    [0b101, 0b101, 0b010, 0b101, 0b101], // X
    [0b101, 0b101, 0b010, 0b010, 0b010], // Y
    [0b111, 0b001, 0b010, 0b100, 0b111], // Z
    [0b011, 0b010, 0b010, 0b010, 0b011], // [
    [0b100, 0b100, 0b010, 0b001, 0b001], // backslash
    [0b110, 0b010, 0b010, 0b010, 0b110], // ]
    [0b010, 0b101, 0b000, 0b000, 0b000], // ^
    [0b000, 0b000, 0b000, 0b000, 0b111], // _
    [0b100, 0b010, 0b000, 0b000, 0b000], // `
    [0b111, 0b101, 0b111, 0b101, 0b101], // a (small caps A)
    [0b111, 0b101, 0b110, 0b101, 0b111], // b
    [0b011, 0b100, 0b100, 0b100, 0b011], // c
    [0b110, 0b101, 0b101, 0b101, 0b110], // d
    [0b111, 0b100, 0b110, 0b100, 0b111], // e
    [0b111, 0b100, 0b110, 0b100, 0b100], // f
    [0b011, 0b100, 0b101, 0b101, 0b011], // g
    [0b101, 0b101, 0b111, 0b101, 0b101], // h
    [0b111, 0b010, 0b010, 0b010, 0b111], // i
    [0b111, 0b010, 0b010, 0b010, 0b110], // j
    [0b101, 0b101, 0b110, 0b101, 0b101], // k
    [0b100, 0b100, 0b100, 0b100, 0b111], // l
    [0b111, 0b111, 0b101, 0b101, 0b101], // m
    [0b110, 0b101, 0b101, 0b101, 0b101], // n
    [0b111, 0b101, 0b101, 0b101, 0b111], // o
    [0b111, 0b101, 0b111, 0b100, 0b100], // p
    [0b111, 0b101, 0b101, 0b111, 0b001], // q
    [0b111, 0b101, 0b110, 0b101, 0b101], // r
    [0b011, 0b100, 0b111, 0b001, 0b110], // s
    [0b111, 0b010, 0b010, 0b010, 0b010], // t
    [0b101, 0b101, 0b101, 0b101, 0b111], // u
    [0b101, 0b101, 0b101, 0b101, 0b010], // v
    [0b101, 0b101, 0b101, 0b111, 0b111], // w
    [0b101, 0b101, 0b010, 0b101, 0b101], // x
    [0b101, 0b101, 0b010, 0b010, 0b010], // y
    [0b111, 0b001, 0b010, 0b100, 0b111], // z
    [0b011, 0b010, 0b110, 0b010, 0b011], // {
    [0b010, 0b010, 0b010, 0b010, 0b010], // |
    [0b110, 0b010, 0b011, 0b010, 0b110], // }
    [0b000, 0b001, 0b111, 0b100, 0b000], // ~
];

// Note: the a-z rows intentionally duplicate A-Z. Keeping 95 explicit
// entries makes the table a straight `c - 32` index with no special cases
// in hot text-rendering code; `glyph()` also folds case for callers that
// construct chars outside this table.
