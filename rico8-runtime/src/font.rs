//! RICO-8's built-in pixel font.
//!
//! An original 3-pixel-wide glyph set in a 4x7 cell. Uppercase fills the full
//! cap height; lowercase are shorter x-height glyphs with true ascenders and
//! descenders, so case reads correctly in case-sensitive Rust source. Each
//! glyph is six rows; in every row the leftmost pixel is bit 2 (`0b100`).

/// Advance width of one character cell in pixels.
pub const GLYPH_W: i32 = 4;
/// Line height in pixels.
pub const GLYPH_H: i32 = 7;

/// Glyph used for characters outside the printable ASCII range.
pub const UNKNOWN: [u8; 6] = [0b111, 0b111, 0b111, 0b111, 0b111, 0b000];

/// Look up the glyph rows for a character.
pub fn glyph(c: char) -> [u8; 6] {
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

// Uppercase, digits and symbols occupy the top five rows (the cap band) with a
// blank sixth row. Lowercase letters sit on the baseline with a 4-row x-height;
// `b d f h k l t` reach the ascender line and `g j p q y` drop into the sixth
// row as descenders. The leftmost pixel of each row is bit 2 (`0b100`).
#[rustfmt::skip]
const GLYPHS: [[u8; 6]; 95] = [
    [0b000, 0b000, 0b000, 0b000, 0b000, 0b000], // space
    [0b010, 0b010, 0b010, 0b000, 0b010, 0b000], // !
    [0b101, 0b101, 0b000, 0b000, 0b000, 0b000], // "
    [0b101, 0b111, 0b101, 0b111, 0b101, 0b000], // #
    [0b111, 0b110, 0b111, 0b011, 0b111, 0b000], // $
    [0b101, 0b001, 0b010, 0b100, 0b101, 0b000], // %
    [0b010, 0b101, 0b010, 0b101, 0b011, 0b000], // &
    [0b010, 0b100, 0b000, 0b000, 0b000, 0b000], // '
    [0b001, 0b010, 0b010, 0b010, 0b001, 0b000], // (
    [0b100, 0b010, 0b010, 0b010, 0b100, 0b000], // )
    [0b101, 0b010, 0b111, 0b010, 0b101, 0b000], // *
    [0b000, 0b010, 0b111, 0b010, 0b000, 0b000], // +
    [0b000, 0b000, 0b000, 0b010, 0b100, 0b000], // ,
    [0b000, 0b000, 0b111, 0b000, 0b000, 0b000], // -
    [0b000, 0b000, 0b000, 0b000, 0b010, 0b000], // .
    [0b001, 0b001, 0b010, 0b100, 0b100, 0b000], // /
    [0b111, 0b101, 0b101, 0b101, 0b111, 0b000], // 0
    [0b110, 0b010, 0b010, 0b010, 0b111, 0b000], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111, 0b000], // 2
    [0b111, 0b001, 0b011, 0b001, 0b111, 0b000], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001, 0b000], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111, 0b000], // 5
    [0b011, 0b100, 0b111, 0b101, 0b111, 0b000], // 6
    [0b111, 0b001, 0b001, 0b001, 0b001, 0b000], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111, 0b000], // 8
    [0b111, 0b101, 0b111, 0b001, 0b110, 0b000], // 9
    [0b000, 0b010, 0b000, 0b010, 0b000, 0b000], // :
    [0b000, 0b010, 0b000, 0b010, 0b100, 0b000], // ;
    [0b001, 0b010, 0b100, 0b010, 0b001, 0b000], // <
    [0b000, 0b111, 0b000, 0b111, 0b000, 0b000], // =
    [0b100, 0b010, 0b001, 0b010, 0b100, 0b000], // >
    [0b111, 0b001, 0b011, 0b000, 0b010, 0b000], // ?
    [0b111, 0b101, 0b111, 0b100, 0b011, 0b000], // @
    [0b111, 0b101, 0b111, 0b101, 0b101, 0b000], // A
    [0b111, 0b101, 0b110, 0b101, 0b111, 0b000], // B
    [0b011, 0b100, 0b100, 0b100, 0b011, 0b000], // C
    [0b110, 0b101, 0b101, 0b101, 0b110, 0b000], // D
    [0b111, 0b100, 0b110, 0b100, 0b111, 0b000], // E
    [0b111, 0b100, 0b110, 0b100, 0b100, 0b000], // F
    [0b011, 0b100, 0b101, 0b101, 0b011, 0b000], // G
    [0b101, 0b101, 0b111, 0b101, 0b101, 0b000], // H
    [0b111, 0b010, 0b010, 0b010, 0b111, 0b000], // I
    [0b111, 0b010, 0b010, 0b010, 0b110, 0b000], // J
    [0b101, 0b101, 0b110, 0b101, 0b101, 0b000], // K
    [0b100, 0b100, 0b100, 0b100, 0b111, 0b000], // L
    [0b111, 0b111, 0b101, 0b101, 0b101, 0b000], // M
    [0b110, 0b101, 0b101, 0b101, 0b101, 0b000], // N
    [0b111, 0b101, 0b101, 0b101, 0b111, 0b000], // O
    [0b111, 0b101, 0b111, 0b100, 0b100, 0b000], // P
    [0b111, 0b101, 0b101, 0b111, 0b001, 0b000], // Q
    [0b111, 0b101, 0b110, 0b101, 0b101, 0b000], // R
    [0b011, 0b100, 0b111, 0b001, 0b110, 0b000], // S
    [0b111, 0b010, 0b010, 0b010, 0b010, 0b000], // T
    [0b101, 0b101, 0b101, 0b101, 0b111, 0b000], // U
    [0b101, 0b101, 0b101, 0b101, 0b010, 0b000], // V
    [0b101, 0b101, 0b101, 0b111, 0b111, 0b000], // W
    [0b101, 0b101, 0b010, 0b101, 0b101, 0b000], // X
    [0b101, 0b101, 0b010, 0b010, 0b010, 0b000], // Y
    [0b111, 0b001, 0b010, 0b100, 0b111, 0b000], // Z
    [0b011, 0b010, 0b010, 0b010, 0b011, 0b000], // [
    [0b100, 0b100, 0b010, 0b001, 0b001, 0b000], // backslash
    [0b110, 0b010, 0b010, 0b010, 0b110, 0b000], // ]
    [0b010, 0b101, 0b000, 0b000, 0b000, 0b000], // ^
    [0b000, 0b000, 0b000, 0b000, 0b111, 0b000], // _
    [0b100, 0b010, 0b000, 0b000, 0b000, 0b000], // `
    [0b000, 0b011, 0b101, 0b111, 0b111, 0b000], // a
    [0b100, 0b100, 0b110, 0b101, 0b110, 0b000], // b
    [0b000, 0b011, 0b100, 0b100, 0b011, 0b000], // c
    [0b001, 0b001, 0b011, 0b101, 0b011, 0b000], // d
    [0b000, 0b011, 0b111, 0b100, 0b011, 0b000], // e
    [0b011, 0b010, 0b111, 0b010, 0b010, 0b000], // f
    [0b000, 0b011, 0b101, 0b011, 0b001, 0b110], // g (descender)
    [0b100, 0b100, 0b110, 0b101, 0b101, 0b000], // h
    [0b010, 0b000, 0b010, 0b010, 0b010, 0b000], // i
    [0b001, 0b000, 0b001, 0b001, 0b001, 0b110], // j (descender)
    [0b100, 0b100, 0b101, 0b110, 0b101, 0b000], // k
    [0b110, 0b010, 0b010, 0b010, 0b011, 0b000], // l
    [0b000, 0b111, 0b111, 0b101, 0b101, 0b000], // m
    [0b000, 0b110, 0b101, 0b101, 0b101, 0b000], // n
    [0b000, 0b010, 0b101, 0b101, 0b010, 0b000], // o
    [0b000, 0b110, 0b101, 0b110, 0b100, 0b100], // p (descender)
    [0b000, 0b011, 0b101, 0b011, 0b001, 0b001], // q (descender)
    [0b000, 0b110, 0b100, 0b100, 0b100, 0b000], // r
    [0b000, 0b011, 0b110, 0b011, 0b110, 0b000], // s
    [0b010, 0b111, 0b010, 0b010, 0b011, 0b000], // t
    [0b000, 0b101, 0b101, 0b101, 0b111, 0b000], // u
    [0b000, 0b101, 0b101, 0b101, 0b010, 0b000], // v
    [0b000, 0b101, 0b101, 0b111, 0b111, 0b000], // w
    [0b000, 0b101, 0b010, 0b010, 0b101, 0b000], // x
    [0b000, 0b101, 0b101, 0b011, 0b001, 0b110], // y (descender)
    [0b000, 0b111, 0b001, 0b100, 0b111, 0b000], // z
    [0b011, 0b010, 0b110, 0b010, 0b011, 0b000], // {
    [0b010, 0b010, 0b010, 0b010, 0b010, 0b000], // |
    [0b110, 0b010, 0b011, 0b010, 0b110, 0b000], // }
    [0b000, 0b001, 0b111, 0b100, 0b000, 0b000], // ~
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_is_distinct() {
        // Every letter must render differently in each case; otherwise Rust
        // source is unreadable (the bug this font shape fixes).
        for (lo, up) in ('a'..='z').zip('A'..='Z') {
            assert_ne!(glyph(lo), glyph(up), "{lo} and {up} render identically");
        }
    }

    #[test]
    fn lowercase_is_shorter_than_uppercase() {
        // Plain x-height letters leave the top (ascender/cap) row empty, which
        // is what reads as "lowercase".
        for c in "acemnorsuvwxz".chars() {
            assert_eq!(glyph(c)[0], 0, "{c} should not reach the cap line");
        }
    }

    #[test]
    fn descenders_use_the_bottom_row() {
        for c in "gjpqy".chars() {
            assert_ne!(glyph(c)[5], 0, "{c} should have a descender");
        }
        // Letters without descenders keep the bottom row clear.
        for c in "abcdefhiklmnorstuvwxz".chars() {
            assert_eq!(glyph(c)[5], 0, "{c} should not have a descender");
        }
    }

    #[test]
    fn out_of_range_is_unknown() {
        assert_eq!(glyph('\u{1F600}'), UNKNOWN);
        assert_eq!(glyph('\u{7F}'), UNKNOWN);
    }
}
