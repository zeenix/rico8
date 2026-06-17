//! Fantasy-console UI primitives.
//!
//! Everything the shell and editors draw — panels, tab icons, the mouse
//! cursor, selections — goes through these helpers into the same 128x128
//! framebuffer carts use. There are deliberately no native-looking
//! widgets here: this is console chrome, not a GUI toolkit.

use crate::{fb::Framebuffer, palette::col};

/// Filled panel with a 1px border, inclusive corners.
pub fn panel(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32, bg: u8, border: u8) {
    fb.rectfill(x0, y0, x1, y1, bg);
    fb.rect(x0, y0, x1, y1, border);
}

/// Text with a 1px drop shadow, for headers on busy backgrounds.
pub fn shadow_text(fb: &mut Framebuffer, s: &str, x: i32, y: i32, color: u8, shadow: u8) {
    fb.print(s, x + 1, y + 1, shadow);
    fb.print(s, x, y, color);
}

/// Invert-style selection: repaint a rectangle's pixels with swapped
/// foreground/background, used for text selections and highlights.
pub fn selection(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32, fg: u8, bg: u8) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = fb.pget(x, y);
            let n = if c == bg {
                fg
            } else if c == fg {
                bg
            } else {
                c
            };
            fb.pset(x, y, n);
        }
    }
}

/// 8x8 single-color icons for the editor tab bar, one bit per pixel.
/// Order matches `TAB_ICONS`' documentation: code, sprite, map, sfx, music.
pub type Icon = [u8; 8];

/// `(*)` brackets-ish glyph: the code editor.
pub const ICON_CODE: Icon = [
    0b00000000, 0b01100110, 0b11000011, 0b10000001, 0b10000001, 0b11000011, 0b01100110, 0b00000000,
];
/// Checkered square: the sprite editor.
pub const ICON_SPRITE: Icon = [
    0b11111111, 0b10101011, 0b11010101, 0b10101011, 0b11010101, 0b10101011, 0b11010101, 0b11111111,
];
/// Tile grid: the map editor.
pub const ICON_MAP: Icon = [
    0b11111111, 0b10010011, 0b10010011, 0b11111111, 0b10010011, 0b10010011, 0b11111111, 0b00000000,
];
/// Speaker: the SFX editor.
pub const ICON_SFX: Icon = [
    0b00000110, 0b00001110, 0b01111110, 0b01111110, 0b01111110, 0b00001110, 0b00000110, 0b00000000,
];
/// Note: the music editor.
pub const ICON_MUSIC: Icon = [
    0b00111110, 0b00100010, 0b00100010, 0b00100010, 0b01100110, 0b11101110, 0b01000100, 0b00000000,
];

/// Draw an icon in one color (bits set = pixels drawn).
pub fn icon(fb: &mut Framebuffer, icon: &Icon, x: i32, y: i32, color: u8) {
    for (ry, row) in icon.iter().enumerate() {
        for rx in 0..8 {
            if row & (0x80 >> rx) != 0 {
                fb.pset(x + rx, y + ry as i32, color);
            }
        }
    }
}

/// Mouse cursor: white arrow with a black outline, hotspot at (0, 0).
/// First layer is the white fill, second the black outline.
const CURSOR_FILL: Icon = [
    0b00000000, 0b01000000, 0b01100000, 0b01110000, 0b01111000, 0b01100000, 0b00100000, 0b00000000,
];
const CURSOR_OUTLINE: Icon = [
    0b11000000, 0b10100000, 0b10010000, 0b10001000, 0b10000100, 0b10011100, 0b11010000, 0b00110000,
];

/// The console's friendly runtime-error screen, shared by every
/// frontend (desktop, web, handheld) so a crashed cart looks the same
/// everywhere.
pub fn error_screen(message: &str) -> Framebuffer {
    use crate::fb::{HEIGHT, WIDTH};
    let mut fb = Framebuffer::new();
    fb.cls(col::BLACK);
    fb.rectfill(0, 0, WIDTH - 1, 7, col::RED);
    fb.print("RICO-8", 2, 1, col::WHITE);
    fb.print("** Runtime error **", 2, 14, col::RED);
    let mut y = 24;
    for line in message.lines().take(12) {
        let mut rest = line;
        while !rest.is_empty() && y < HEIGHT - 8 {
            let take = rest
                .char_indices()
                .nth(31)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            fb.print(&rest[..take], 2, y, col::ORANGE);
            rest = &rest[take..];
            y += 6;
        }
    }
    fb
}

/// Draw the mouse cursor at a framebuffer position.
pub fn cursor(fb: &mut Framebuffer, x: i32, y: i32) {
    for (icon, color) in [(&CURSOR_OUTLINE, col::BLACK), (&CURSOR_FILL, col::WHITE)] {
        for (ry, row) in icon.iter().enumerate() {
            for rx in 0..8 {
                if row & (0x80 >> rx) != 0 {
                    fb.pset(x + rx, y + ry as i32, color);
                }
            }
        }
    }
}
