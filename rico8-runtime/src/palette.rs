//! The fixed 16-color RICO-8 palette.
//!
//! RICO-8 uses the same well-known 16 colors as PICO-8 so that carts have
//! the classic fantasy-console look. Every drawing operation takes a color
//! index in `0..16`; the palette itself is not modifiable by carts.

/// Number of colors in the palette.
pub const PALETTE_SIZE: usize = 16;

/// The palette as `[r, g, b]` triples, indexed by color number.
pub const PALETTE: [[u8; 3]; PALETTE_SIZE] = [
    [0x00, 0x00, 0x00], // 0  black
    [0x1d, 0x2b, 0x53], // 1  dark blue
    [0x7e, 0x25, 0x53], // 2  dark purple
    [0x00, 0x87, 0x51], // 3  dark green
    [0xab, 0x52, 0x36], // 4  brown
    [0x5f, 0x57, 0x4f], // 5  dark grey
    [0xc2, 0xc3, 0xc7], // 6  light grey
    [0xff, 0xf1, 0xe8], // 7  white
    [0xff, 0x00, 0x4d], // 8  red
    [0xff, 0xa3, 0x00], // 9  orange
    [0xff, 0xec, 0x27], // 10 yellow
    [0x00, 0xe4, 0x36], // 11 green
    [0x29, 0xad, 0xff], // 12 blue
    [0x83, 0x76, 0x9c], // 13 lavender
    [0xff, 0x77, 0xa8], // 14 pink
    [0xff, 0xcc, 0xaa], // 15 light peach
];

/// Color index constants, for readable host-side UI code.
pub mod col {
    pub const BLACK: u8 = 0;
    pub const DARK_BLUE: u8 = 1;
    pub const DARK_PURPLE: u8 = 2;
    pub const DARK_GREEN: u8 = 3;
    pub const BROWN: u8 = 4;
    pub const DARK_GREY: u8 = 5;
    pub const LIGHT_GREY: u8 = 6;
    pub const WHITE: u8 = 7;
    pub const RED: u8 = 8;
    pub const ORANGE: u8 = 9;
    pub const YELLOW: u8 = 10;
    pub const GREEN: u8 = 11;
    pub const BLUE: u8 = 12;
    pub const LAVENDER: u8 = 13;
    pub const PINK: u8 = 14;
    pub const PEACH: u8 = 15;
}

/// Convert a color index to RGBA bytes (alpha always 255).
#[inline]
pub fn rgba(color: u8) -> [u8; 4] {
    let [r, g, b] = PALETTE[(color & 0x0f) as usize];
    [r, g, b, 0xff]
}
