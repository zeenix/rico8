//! The 128x128 indexed-color framebuffer and software drawing primitives.
//!
//! Everything RICO-8 puts on screen — running carts, the console, every
//! editor — is drawn through this one software rasterizer into a buffer of
//! palette indices. The GPU's only job is to scale the result up with
//! nearest-neighbor filtering.

use crate::{
    assets::{MapData, SpriteSheet, SPRITE_SIZE},
    font, palette,
};

/// Virtual screen width in pixels.
pub const WIDTH: i32 = 128;
/// Virtual screen height in pixels.
pub const HEIGHT: i32 = 128;

/// The virtual screen: one byte per pixel, each a palette index in `0..16`.
pub struct Framebuffer {
    pixels: Vec<u8>,
    camera_x: i32,
    camera_y: i32,
    clip: (i32, i32, i32, i32),
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0; (WIDTH * HEIGHT) as usize],
            camera_x: 0,
            camera_y: 0,
            clip: (0, 0, WIDTH, HEIGHT),
        }
    }

    /// Raw palette-index pixels, row-major, `WIDTH * HEIGHT` long.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Expand the indexed framebuffer into an RGBA8 buffer for GPU upload.
    pub fn write_rgba(&self, out: &mut [u8]) {
        for (i, &c) in self.pixels.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&palette::rgba(c));
        }
    }

    /// Set the camera offset applied to all subsequent draw operations.
    pub fn camera(&mut self, x: i32, y: i32) {
        self.camera_x = x;
        self.camera_y = y;
    }

    /// Restrict drawing to a screen-space rectangle.
    pub fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let x0 = x.clamp(0, WIDTH);
        let y0 = y.clamp(0, HEIGHT);
        let x1 = (x + w).clamp(0, WIDTH);
        let y1 = (y + h).clamp(0, HEIGHT);
        self.clip = (x0, y0, x1, y1);
    }

    /// Remove the clip rectangle.
    pub fn clip_reset(&mut self) {
        self.clip = (0, 0, WIDTH, HEIGHT);
    }

    /// Reset camera and clip to defaults (used between host UI and cart frames).
    pub fn reset_state(&mut self) {
        self.camera_x = 0;
        self.camera_y = 0;
        self.clip_reset();
    }

    /// Fill the whole screen with a color. Does not touch camera/clip.
    pub fn cls(&mut self, color: u8) {
        self.pixels.fill(color & 0x0f);
    }

    #[inline]
    fn raw_pset(&mut self, x: i32, y: i32, color: u8) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
            self.pixels[(y * WIDTH + x) as usize] = color & 0x0f;
        }
    }

    /// Set one pixel (camera-relative, like all draw ops).
    pub fn pset(&mut self, x: i32, y: i32, color: u8) {
        self.raw_pset(x - self.camera_x, y - self.camera_y, color);
    }

    /// Read one pixel in screen space. Out-of-bounds reads return 0.
    pub fn pget(&self, x: i32, y: i32) -> u8 {
        if (0..WIDTH).contains(&x) && (0..HEIGHT).contains(&y) {
            self.pixels[(y * WIDTH + x) as usize]
        } else {
            0
        }
    }

    /// Bresenham line between two points, inclusive.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        let (mut x0, mut y0) = (x0 - self.camera_x, y0 - self.camera_y);
        let (x1, y1) = (x1 - self.camera_x, y1 - self.camera_y);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.raw_pset(x0, y0, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Rectangle outline with inclusive corners, like PICO-8's `rect`.
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        let (xa, xb) = (x0.min(x1), x0.max(x1));
        let (ya, yb) = (y0.min(y1), y0.max(y1));
        self.line(xa, ya, xb, ya, color);
        self.line(xa, yb, xb, yb, color);
        self.line(xa, ya, xa, yb, color);
        self.line(xb, ya, xb, yb, color);
    }

    /// Filled rectangle with inclusive corners.
    pub fn rectfill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        let (xa, xb) = (x0.min(x1), x0.max(x1));
        let (ya, yb) = (y0.min(y1), y0.max(y1));
        for y in ya..=yb {
            for x in xa..=xb {
                self.raw_pset(x - self.camera_x, y - self.camera_y, color);
            }
        }
    }

    /// Circle outline (midpoint algorithm).
    pub fn circ(&mut self, cx: i32, cy: i32, r: i32, color: u8) {
        self.circle_impl(cx, cy, r.max(0), color, false);
    }

    /// Filled circle.
    pub fn circfill(&mut self, cx: i32, cy: i32, r: i32, color: u8) {
        self.circle_impl(cx, cy, r.max(0), color, true);
    }

    fn circle_impl(&mut self, cx: i32, cy: i32, r: i32, color: u8, fill: bool) {
        let (cx, cy) = (cx - self.camera_x, cy - self.camera_y);
        let mut x = r;
        let mut y = 0;
        let mut err = 1 - r;
        while x >= y {
            if fill {
                for px in (cx - x)..=(cx + x) {
                    self.raw_pset(px, cy + y, color);
                    self.raw_pset(px, cy - y, color);
                }
                for px in (cx - y)..=(cx + y) {
                    self.raw_pset(px, cy + x, color);
                    self.raw_pset(px, cy - x, color);
                }
            } else {
                for (px, py) in [
                    (cx + x, cy + y),
                    (cx - x, cy + y),
                    (cx + x, cy - y),
                    (cx - x, cy - y),
                    (cx + y, cy + x),
                    (cx - y, cy + x),
                    (cx + y, cy - x),
                    (cx - y, cy - x),
                ] {
                    self.raw_pset(px, py, color);
                }
            }
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    /// Print text with the built-in font. Returns the x position after the
    /// last character.
    pub fn print(&mut self, text: &str, x: i32, y: i32, color: u8) -> i32 {
        let mut cx = x;
        let mut cy = y;
        for ch in text.chars() {
            if ch == '\n' {
                cx = x;
                cy += font::GLYPH_H;
                continue;
            }
            let rows = font::glyph(ch);
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..3 {
                    if row & (0b100 >> rx) != 0 {
                        self.pset(cx + rx, cy + ry as i32, color);
                    }
                }
            }
            cx += font::GLYPH_W;
        }
        cx
    }

    /// Draw sprite `n` (and `w x h` neighbors) from a sheet. Color 0 is
    /// transparent, matching the classic default. `w`/`h` are in sprite units
    /// and may be fractional: `w = 0.5` draws a 4-pixel-wide slice.
    #[allow(clippy::too_many_arguments)]
    pub fn spr(
        &mut self,
        sheet: &SpriteSheet,
        n: u32,
        x: i32,
        y: i32,
        w: f32,
        h: f32,
        flip_x: bool,
        flip_y: bool,
    ) {
        // Fractional sprite counts draw a partial block: floor to whole
        // pixels, so the last cell can be clipped mid-sprite.
        let pw = (w.max(0.0) * SPRITE_SIZE as f32) as i32;
        let ph = (h.max(0.0) * SPRITE_SIZE as f32) as i32;
        for py in 0..ph {
            for px in 0..pw {
                let sx = if flip_x { pw - 1 - px } else { px };
                let sy = if flip_y { ph - 1 - py } else { py };
                let c = sheet.sprite_pixel(n, sx, sy);
                if c != 0 {
                    self.pset(x + px, y + py, c);
                }
            }
        }
    }

    /// Draw a region of the tile map. `layers` is a flag mask: when nonzero,
    /// only tiles whose flags intersect the mask are drawn. Tile 0 is empty.
    #[allow(clippy::too_many_arguments)]
    pub fn map(
        &mut self,
        map: &MapData,
        sheet: &SpriteSheet,
        cel_x: i32,
        cel_y: i32,
        sx: i32,
        sy: i32,
        cel_w: i32,
        cel_h: i32,
        layers: u8,
    ) {
        for ty in 0..cel_h {
            for tx in 0..cel_w {
                let tile = map.get(cel_x + tx, cel_y + ty);
                if tile == 0 {
                    continue;
                }
                if layers != 0 && sheet.flags(tile as u32) & layers == 0 {
                    continue;
                }
                self.spr(
                    sheet,
                    tile as u32,
                    sx + tx * SPRITE_SIZE as i32,
                    sy + ty * SPRITE_SIZE as i32,
                    1.0,
                    1.0,
                    false,
                    false,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cls_fills_screen() {
        let mut fb = Framebuffer::new();
        fb.cls(7);
        assert!(fb.pixels().iter().all(|&p| p == 7));
    }

    #[test]
    fn pset_pget_roundtrip() {
        let mut fb = Framebuffer::new();
        fb.pset(10, 20, 8);
        assert_eq!(fb.pget(10, 20), 8);
        assert_eq!(fb.pget(11, 20), 0);
    }

    #[test]
    fn out_of_bounds_is_safe() {
        let mut fb = Framebuffer::new();
        fb.pset(-1, 0, 5);
        fb.pset(0, 99999, 5);
        fb.line(-50, -50, 200, 200, 6);
        fb.circfill(0, 0, 300, 3);
        assert_eq!(fb.pget(-1, 0), 0);
    }

    #[test]
    fn camera_offsets_draws() {
        let mut fb = Framebuffer::new();
        fb.camera(10, 0);
        fb.pset(15, 5, 9);
        assert_eq!(fb.pget(5, 5), 9);
        fb.reset_state();
        fb.pset(15, 5, 9);
        assert_eq!(fb.pget(15, 5), 9);
    }

    #[test]
    fn clip_constrains_drawing() {
        let mut fb = Framebuffer::new();
        fb.clip(0, 0, 4, 4);
        fb.rectfill(0, 0, 127, 127, 7);
        assert_eq!(fb.pget(3, 3), 7);
        assert_eq!(fb.pget(4, 4), 0);
    }

    #[test]
    fn rect_outline_is_hollow() {
        let mut fb = Framebuffer::new();
        fb.rect(0, 0, 4, 4, 7);
        assert_eq!(fb.pget(0, 0), 7);
        assert_eq!(fb.pget(4, 4), 7);
        assert_eq!(fb.pget(2, 2), 0);
    }

    #[test]
    fn print_advances_cursor() {
        let mut fb = Framebuffer::new();
        let end = fb.print("abc", 0, 0, 7);
        assert_eq!(end, 3 * font::GLYPH_W);
    }

    #[test]
    fn fractional_sprite_draws_a_partial_slice() {
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        // Fill sprite 0 (the top-left 8x8 cell) solid.
        for y in 0..8 {
            for x in 0..8 {
                sheet.set(x, y, 7);
            }
        }
        // Half width draws only the left four columns.
        fb.spr(&sheet, 0, 0, 0, 0.5, 1.0, false, false);
        assert_eq!(fb.pget(3, 4), 7, "left half is drawn");
        assert_eq!(fb.pget(4, 4), 0, "right half is untouched");
        assert_eq!(fb.pget(7, 7), 0, "bottom-right corner is untouched");
    }
}
