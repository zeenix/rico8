//! The 128x128 indexed-color framebuffer and software drawing primitives.
//!
//! Everything RICO-8 puts on screen — running carts, the console, every
//! editor — is drawn through this one software rasterizer into a buffer of
//! palette indices. The GPU's only job is to scale the result up with
//! nearest-neighbor filtering.

use crate::{
    assets::{MapData, SpriteSheet, SPRITES_PER_ROW, SPRITE_COUNT, SPRITE_SIZE},
    font, palette,
};

/// Virtual screen width in pixels.
pub const WIDTH: i32 = 128;
/// Virtual screen height in pixels.
pub const HEIGHT: i32 = 128;

/// Identity color map: index `i` maps to color `i`.
const IDENTITY_PALETTE: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
/// Default transparency mask: only color 0 is transparent.
const DEFAULT_TRANSPARENT: u16 = 0x0001;

/// The virtual screen: one byte per pixel, each a palette index in `0..16`.
pub struct Framebuffer {
    pixels: Vec<u8>,
    camera_x: i32,
    camera_y: i32,
    clip: (i32, i32, i32, i32),
    draw_pal: [u8; 16],
    display_pal: [u8; 16],
    transparent: u16,
    fill_pattern: u16,
    fill_secondary: u8,
    fill_transparent: bool,
    pen_color: u8,
    cursor_x: i32,
    cursor_y: i32,
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
            draw_pal: IDENTITY_PALETTE,
            display_pal: IDENTITY_PALETTE,
            transparent: DEFAULT_TRANSPARENT,
            fill_pattern: 0,
            fill_secondary: 0,
            fill_transparent: false,
            pen_color: 6,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    /// Raw palette-index pixels, row-major, `WIDTH * HEIGHT` long.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The display palette: at present time, stored index `i` is shown as color
    /// `display_palette()[i]`. Presenters apply this when expanding the indexed
    /// framebuffer to RGB, exactly as `write_rgba` does for GPU upload.
    pub fn display_palette(&self) -> &[u8; 16] {
        &self.display_pal
    }

    /// Expand the indexed framebuffer into an RGBA8 buffer for GPU upload.
    pub fn write_rgba(&self, out: &mut [u8]) {
        // Fold the display palette into a 16-entry RGBA lookup table once, so
        // the per-pixel loop is a plain table read plus a fixed-size copy. This
        // drops the per-pixel `display_pal` + `rgba` work and the range-index
        // bounds check, and autovectorizes cleanly.
        let mut lut = [[0u8; 4]; 16];
        for (i, entry) in lut.iter_mut().enumerate() {
            *entry = palette::rgba(self.display_pal[i]);
        }
        for (chunk, &c) in out.chunks_exact_mut(4).zip(self.pixels.iter()) {
            chunk.copy_from_slice(&lut[(c & 0x0f) as usize]);
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
        self.draw_pal = IDENTITY_PALETTE;
        self.display_pal = IDENTITY_PALETTE;
        self.transparent = DEFAULT_TRANSPARENT;
        self.fill_pattern = 0;
        self.fill_secondary = 0;
        self.fill_transparent = false;
        self.pen_color = 6;
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// Make a palette color transparent (or opaque) for sprite draws.
    pub fn set_transparent_color(&mut self, color: u8, transparent: bool) {
        let bit = 1u16 << (color & 0x0f);
        if transparent {
            self.transparent |= bit;
        } else {
            self.transparent &= !bit;
        }
    }

    /// Reset transparency to the default (only color 0 transparent).
    pub fn reset_transparency(&mut self) {
        self.transparent = DEFAULT_TRANSPARENT;
    }

    /// Remap a draw-palette color: later draws of `from` are written as `to`.
    pub fn remap_color(&mut self, from: u8, to: u8) {
        self.draw_pal[(from & 0x0f) as usize] = to & 0x0f;
    }

    /// Remap a display-palette color: `from` is shown as `to` at upload time.
    pub fn remap_display_color(&mut self, from: u8, to: u8) {
        self.display_pal[(from & 0x0f) as usize] = to & 0x0f;
    }

    /// Reset both the draw and display palettes to identity.
    pub fn reset_palette(&mut self) {
        self.draw_pal = IDENTITY_PALETTE;
        self.display_pal = IDENTITY_PALETTE;
    }

    /// Configure the fill pattern for the filled shape primitives. `pattern` is
    /// a 4x4 bitmask (bit 15 = top-left). Pattern-0 pixels take the shape's
    /// color; pattern-1 pixels take `secondary`, or are skipped when
    /// `transparent`. A `pattern` of 0 fills solid.
    pub fn set_fill_pattern(&mut self, pattern: u16, secondary: u8, transparent: bool) {
        self.fill_pattern = pattern;
        self.fill_secondary = secondary & 0x0f;
        self.fill_transparent = transparent;
    }

    /// The color a fill should write at framebuffer pixel `(x, y)`, or `None`
    /// when the transparent pattern skips it. `x`/`y` are post-camera.
    fn fill_color_at(&self, x: i32, y: i32, primary: u8) -> Option<u8> {
        if self.fill_pattern == 0 {
            return Some(primary);
        }
        let idx = ((y & 3) * 4 + (x & 3)) as u16;
        if (self.fill_pattern >> (15 - idx)) & 1 == 0 {
            Some(primary)
        } else if self.fill_transparent {
            None
        } else {
            Some(self.fill_secondary)
        }
    }

    /// Like `raw_pset` but honoring the fill pattern. `x`/`y` are post-camera.
    fn raw_pset_fill(&mut self, x: i32, y: i32, primary: u8) {
        if let Some(c) = self.fill_color_at(x, y, primary) {
            self.raw_pset(x, y, c);
        }
    }

    /// Fill a solid horizontal run on row `y` from `x0..=x1` (inclusive,
    /// POST-camera), clipped to the clip rect. Applies the draw palette. Used by
    /// the solid (non-patterned) fill path: clipping the span once and writing
    /// it as one memset is far cheaper than clipping every pixel.
    fn fill_span(&mut self, x0: i32, x1: i32, y: i32, color: u8) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        if y < cy0 || y >= cy1 {
            return;
        }
        let xa = x0.max(cx0);
        let xb = x1.min(cx1 - 1);
        if xa > xb {
            return;
        }
        let c = self.draw_pal[(color & 0x0f) as usize] & 0x0f;
        let start = (y * WIDTH + xa) as usize;
        let end = (y * WIDTH + xb + 1) as usize;
        self.pixels[start..end].fill(c);
    }

    /// Fill the whole screen with a color. Does not touch camera/clip.
    pub fn cls(&mut self, color: u8) {
        self.pixels.fill(color & 0x0f);
    }

    #[inline]
    fn raw_pset(&mut self, x: i32, y: i32, color: u8) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
            let c = self.draw_pal[(color & 0x0f) as usize] & 0x0f;
            self.pixels[(y * WIDTH + x) as usize] = c;
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
        if self.fill_pattern == 0 {
            // Solid fill: each row is one clipped memset (xa/xb are pre-camera).
            for y in ya..=yb {
                self.fill_span(
                    xa - self.camera_x,
                    xb - self.camera_x,
                    y - self.camera_y,
                    color,
                );
            }
        } else {
            for y in ya..=yb {
                for x in xa..=xb {
                    self.raw_pset_fill(x - self.camera_x, y - self.camera_y, color);
                }
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
            if fill && self.fill_pattern == 0 {
                // Solid fill: each scanline of the disc is one clipped memset
                // (cx/cy are already post-camera here).
                self.fill_span(cx - x, cx + x, cy + y, color);
                self.fill_span(cx - x, cx + x, cy - y, color);
                self.fill_span(cx - y, cx + y, cy + x, color);
                self.fill_span(cx - y, cx + y, cy - x, color);
            } else if fill {
                for px in (cx - x)..=(cx + x) {
                    self.raw_pset_fill(px, cy + y, color);
                    self.raw_pset_fill(px, cy - y, color);
                }
                for px in (cx - y)..=(cx + y) {
                    self.raw_pset_fill(px, cy + x, color);
                    self.raw_pset_fill(px, cy - x, color);
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

    /// Ellipse outline within the inclusive bounding box `(x0,y0)-(x1,y1)`.
    pub fn oval(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        self.oval_impl(x0, y0, x1, y1, color, false);
    }

    /// Filled ellipse within the inclusive bounding box `(x0,y0)-(x1,y1)`.
    pub fn ovalfill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        self.oval_impl(x0, y0, x1, y1, color, true);
    }

    fn oval_impl(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8, fill: bool) {
        let (xa, xb) = (x0.min(x1), x0.max(x1));
        let (ya, yb) = (y0.min(y1), y0.max(y1));
        let cx = (xa + xb) as f32 / 2.0;
        let cy = (ya + yb) as f32 / 2.0;
        let a = (xb - xa) as f32 / 2.0;
        let b = (yb - ya) as f32 / 2.0;
        if fill {
            for y in ya..=yb {
                let dy = if b > 0.0 { (y as f32 - cy) / b } else { 0.0 };
                let s = 1.0 - dy * dy;
                if s < 0.0 {
                    continue;
                }
                let dx = a * s.sqrt();
                let left = (cx - dx).round() as i32;
                let right = (cx + dx).round() as i32;
                if self.fill_pattern == 0 {
                    // Solid fill: one clipped memset per scanline of the oval.
                    self.fill_span(
                        left - self.camera_x,
                        right - self.camera_x,
                        y - self.camera_y,
                        color,
                    );
                } else {
                    for x in left..=right {
                        self.raw_pset_fill(x - self.camera_x, y - self.camera_y, color);
                    }
                }
            }
        } else {
            // Plot the extremes along each axis so the outline has no gaps.
            for y in ya..=yb {
                let dy = if b > 0.0 { (y as f32 - cy) / b } else { 0.0 };
                let s = 1.0 - dy * dy;
                if s < 0.0 {
                    continue;
                }
                let dx = a * s.sqrt();
                self.pset((cx - dx).round() as i32, y, color);
                self.pset((cx + dx).round() as i32, y, color);
            }
            for x in xa..=xb {
                let dx = if a > 0.0 { (x as f32 - cx) / a } else { 0.0 };
                let s = 1.0 - dx * dx;
                if s < 0.0 {
                    continue;
                }
                let dy = b * s.sqrt();
                self.pset(x, (cy - dy).round() as i32, color);
                self.pset(x, (cy + dy).round() as i32, color);
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

    /// Set the persistent pen color used by `print_pen`.
    pub fn set_pen_color(&mut self, color: u8) {
        self.pen_color = color & 0x0f;
    }

    /// Set the persistent text cursor used by `print_pen`.
    pub fn set_cursor(&mut self, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// Print at the cursor in the pen color, then advance the cursor one line
    /// down. Returns the x position after the last glyph.
    pub fn print_pen(&mut self, text: &str) -> i32 {
        let (x, y) = (self.cursor_x, self.cursor_y);
        let end = self.print(text, x, y, self.pen_color);
        self.cursor_y = y + font::GLYPH_H;
        end
    }

    /// Draw sprite `n` (and the `w x h`-pixel block to its right and below)
    /// from a sheet. Color 0 is transparent, matching the classic default.
    /// `w`/`h` are pixel extents: `w = 4` draws a 4-pixel-wide slice.
    #[allow(clippy::too_many_arguments)]
    pub fn spr(
        &mut self,
        sheet: &SpriteSheet,
        n: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flip_x: bool,
        flip_y: bool,
    ) {
        // `w`/`h` are pixel extents; a partial last cell is clipped mid-sprite.
        let pw = w.max(0);
        let ph = h.max(0);

        // Clip the destination rectangle to the clip rect once, up front, then
        // walk only the visible sub-rectangle. This skips the per-pixel clip
        // test entirely and fast-rejects fully off-screen sprites (a big win
        // for `map`, which calls `spr` once per tile). The destination spans
        // `[dx0, dx0 + pw)` x `[dy0, dy0 + ph)` in post-camera space; the `px`
        // range is where that lands inside `[cx0, cx1)`.
        let (dx0, dy0) = (x - self.camera_x, y - self.camera_y);
        let (cx0, cy0, cx1, cy1) = self.clip;
        let px_lo = (cx0 - dx0).max(0);
        let px_hi = (cx1 - dx0).min(pw);
        let py_lo = (cy0 - dy0).max(0);
        let py_hi = (cy1 - dy0).min(ph);
        if px_lo >= px_hi || py_lo >= py_hi {
            return;
        }

        // Decode the sprite's sheet origin once instead of per pixel. The flip
        // still mirrors about the FULL sprite extent (`pw`/`ph`), and source
        // reads may run past 8 into neighboring sprites for multi-sprite draws,
        // exactly as `sprite_pixel` does.
        let n = (n as usize) % SPRITE_COUNT;
        let base_sx = (n % SPRITES_PER_ROW * SPRITE_SIZE) as i32;
        let base_sy = (n / SPRITES_PER_ROW * SPRITE_SIZE) as i32;
        for py in py_lo..py_hi {
            let sy = if flip_y { ph - 1 - py } else { py };
            for px in px_lo..px_hi {
                let sx = if flip_x { pw - 1 - px } else { px };
                let c = sheet.get(base_sx + sx, base_sy + sy);
                if ((self.transparent >> c) & 1) == 0 {
                    // In bounds by construction: `px`/`py` lie within the clip.
                    let px_dst = dx0 + px;
                    let py_dst = dy0 + py;
                    self.pixels[(py_dst * WIDTH + px_dst) as usize] =
                        self.draw_pal[(c & 0x0f) as usize] & 0x0f;
                }
            }
        }
    }

    /// Draw a sheet rectangle `(sx,sy,sw,sh)` stretched into a screen rectangle
    /// `(dx,dy,dw,dh)` with nearest-neighbor sampling. Honors per-color
    /// transparency and the draw palette.
    #[allow(clippy::too_many_arguments)]
    pub fn sspr(
        &mut self,
        sheet: &SpriteSheet,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        flip_x: bool,
        flip_y: bool,
    ) {
        if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
            return;
        }
        for py in 0..dh {
            for px in 0..dw {
                let fx = if flip_x { dw - 1 - px } else { px };
                let fy = if flip_y { dh - 1 - py } else { py };
                let c = sheet.get(sx + fx * sw / dw, sy + fy * sh / dh);
                if ((self.transparent >> c) & 1) == 0 {
                    self.pset(dx + px, dy + py, c);
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
                    SPRITE_SIZE as i32,
                    SPRITE_SIZE as i32,
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
    fn partial_pixel_sprite_draws_a_partial_slice() {
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        // Fill sprite 0 (the top-left 8x8 cell) solid.
        for y in 0..8 {
            for x in 0..8 {
                sheet.set(x, y, 7);
            }
        }
        // A 4px width draws only the left four columns.
        fb.spr(&sheet, 0, 0, 0, 4, 8, false, false);
        assert_eq!(fb.pget(3, 4), 7, "left half is drawn");
        assert_eq!(fb.pget(4, 4), 0, "right half is untouched");
        assert_eq!(fb.pget(7, 7), 0, "bottom-right corner is untouched");
    }

    #[test]
    fn spr_clipped_flip_mirrors_about_full_sprite() {
        // Flip must mirror about the FULL 8x8 sprite, then the clip rect cuts
        // the result — not the other way round. Mark the four source corners
        // with distinct colors; double-flip swaps each corner to the opposite
        // one, and a 4x4 top-left clip should keep exactly one of them.
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        sheet.set(0, 0, 8); // top-left  -> dest (7,7)
        sheet.set(7, 0, 9); // top-right -> dest (0,7)
        sheet.set(0, 7, 11); // bottom-left  -> dest (7,0)
        sheet.set(7, 7, 12); // bottom-right -> dest (0,0)
        fb.clip(0, 0, 4, 4);
        fb.spr(&sheet, 0, 0, 0, 8, 8, true, true);
        assert_eq!(
            fb.pget(0, 0),
            12,
            "source bottom-right mirrors to dest (0,0) and survives the clip"
        );
        assert_eq!(fb.pget(7, 7), 0, "dest (7,7) is outside the 4x4 clip");
        assert_eq!(fb.pget(0, 7), 0, "dest (0,7) is outside the 4x4 clip");
        assert_eq!(fb.pget(7, 0), 0, "dest (7,0) is outside the 4x4 clip");
    }

    #[test]
    fn spr_partly_offscreen_under_camera_aligns_source() {
        // With the camera pushing the sprite up-and-left, only its bottom-right
        // part is on screen; the visible pixels must come from the matching
        // source columns/rows (no wrap), starting at dest (0,0).
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        for y in 0..8 {
            for x in 0..8 {
                sheet.set(x, y, 7);
            }
        }
        sheet.set(2, 3, 9); // the source pixel that lands on dest (0,0)
        fb.camera(2, 3);
        fb.spr(&sheet, 0, 0, 0, 8, 8, false, false);
        assert_eq!(
            fb.pget(0, 0),
            9,
            "source (2,3) lands at the top-left corner"
        );
        assert_eq!(fb.pget(1, 0), 7, "source (3,3) is the next column");
        assert_eq!(fb.pget(5, 4), 7, "the rest of the on-screen part is drawn");
        // Nothing wrapped to the far edges of the screen.
        assert_eq!(fb.pget(127, 127), 0, "no wrap to the opposite corner");
    }

    #[test]
    fn spr_partial_pixel_width_meets_clip_edge() {
        // A 4px-wide sprite slice further trimmed by a 2px-wide clip: only the
        // first two destination columns survive.
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        for y in 0..8 {
            for x in 0..8 {
                sheet.set(x, y, 7);
            }
        }
        fb.clip(0, 0, 2, 128);
        fb.spr(&sheet, 0, 0, 0, 4, 8, false, false);
        assert_eq!(fb.pget(1, 3), 7, "inside the clip and the partial width");
        assert_eq!(fb.pget(2, 3), 0, "clipped away at x=2");
        assert_eq!(fb.pget(4, 3), 0, "beyond the 4px width anyway");
    }

    #[test]
    fn transparency_mask_controls_sprite_pixels() {
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        for y in 0..8 {
            for x in 0..8 {
                sheet.set(x, y, 8); // a solid red sprite
            }
        }
        // Default: nonzero colors draw.
        fb.spr(&sheet, 0, 0, 0, 8, 8, false, false);
        assert_eq!(fb.pget(1, 1), 8);
        // Make red transparent: redrawing over green leaves green showing.
        fb.cls(3);
        fb.set_transparent_color(8, true);
        fb.spr(&sheet, 0, 0, 0, 8, 8, false, false);
        assert_eq!(fb.pget(1, 1), 3, "red made transparent");
        // reset_transparency restores the default; red draws again.
        fb.reset_transparency();
        fb.spr(&sheet, 0, 0, 0, 8, 8, false, false);
        assert_eq!(fb.pget(1, 1), 8);
    }

    #[test]
    fn color_zero_can_be_made_opaque() {
        let mut fb = Framebuffer::new();
        let sheet = SpriteSheet::default(); // all color 0
        fb.cls(7);
        fb.set_transparent_color(0, false);
        fb.spr(&sheet, 0, 0, 0, 8, 8, false, false);
        assert_eq!(fb.pget(3, 3), 0, "color 0 now drawn over white");
    }

    #[test]
    fn draw_palette_remaps_writes() {
        let mut fb = Framebuffer::new();
        fb.remap_color(8, 12); // draw red as blue
        fb.pset(5, 5, 8);
        assert_eq!(fb.pget(5, 5), 12);
        fb.reset_palette();
        fb.pset(6, 6, 8);
        assert_eq!(fb.pget(6, 6), 8);
    }

    #[test]
    fn cls_ignores_draw_palette() {
        let mut fb = Framebuffer::new();
        fb.remap_color(0, 8);
        fb.cls(0);
        assert_eq!(fb.pget(10, 10), 0, "cls clears to the literal color");
    }

    #[test]
    fn display_palette_remaps_at_upload() {
        let mut fb = Framebuffer::new();
        fb.pset(0, 0, 8); // red stored
        fb.remap_display_color(8, 12); // show red as blue
        let mut out = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        fb.write_rgba(&mut out);
        assert_eq!(&out[0..4], &palette::rgba(12), "pixel uploaded as blue");
        assert_eq!(fb.pget(0, 0), 8, "stored index is unchanged");
    }

    #[test]
    fn ovalfill_fills_center_not_corner() {
        let mut fb = Framebuffer::new();
        fb.ovalfill(0, 0, 10, 6, 7);
        assert_eq!(fb.pget(5, 3), 7, "center filled");
        assert_eq!(fb.pget(0, 0), 0, "bounding-box corner stays empty");
    }

    #[test]
    fn oval_outline_is_hollow() {
        let mut fb = Framebuffer::new();
        fb.oval(0, 0, 10, 10, 7);
        assert_eq!(fb.pget(5, 0), 7, "top of the outline is set");
        assert_eq!(fb.pget(5, 5), 0, "center is hollow");
    }

    #[test]
    fn two_color_fill_pattern_alternates() {
        let mut fb = Framebuffer::new();
        // bit 15 (top-left) = 1, bit 14 = 0, ...
        fb.set_fill_pattern(0b1010_0101_1010_0101, 12, false);
        fb.rectfill(0, 0, 3, 3, 7);
        assert_eq!(
            fb.pget(0, 0),
            12,
            "pattern-1 pixel uses the secondary color"
        );
        assert_eq!(fb.pget(1, 0), 7, "pattern-0 pixel uses the primary color");
    }

    #[test]
    fn transparent_fill_pattern_skips_pixels() {
        let mut fb = Framebuffer::new();
        fb.cls(3);
        fb.set_fill_pattern(0xffff, 0, true); // every pixel is pattern-1, transparent
        fb.rectfill(0, 0, 3, 3, 7);
        assert_eq!(
            fb.pget(1, 1),
            3,
            "all pattern-1 pixels skipped; background shows"
        );
    }

    #[test]
    fn zero_pattern_fills_solid() {
        let mut fb = Framebuffer::new();
        fb.set_fill_pattern(0, 0, false);
        fb.rectfill(0, 0, 3, 3, 7);
        assert_eq!(fb.pget(2, 2), 7);
    }

    #[test]
    fn sspr_upscales_with_nearest_neighbor() {
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        sheet.set(0, 0, 8); // single red source pixel
        fb.sspr(&sheet, 0, 0, 1, 1, 10, 10, 4, 4, false, false);
        assert_eq!(fb.pget(10, 10), 8);
        assert_eq!(
            fb.pget(13, 13),
            8,
            "the whole 4x4 block is the source pixel"
        );
    }

    #[test]
    fn sspr_respects_transparency() {
        let mut fb = Framebuffer::new();
        let sheet = SpriteSheet::default(); // all color 0
        fb.cls(3);
        fb.sspr(&sheet, 0, 0, 2, 2, 0, 0, 4, 4, false, false);
        assert_eq!(fb.pget(1, 1), 3, "color 0 is transparent by default");
    }

    #[test]
    fn sspr_flips_horizontally() {
        let mut fb = Framebuffer::new();
        let mut sheet = SpriteSheet::default();
        sheet.set(0, 0, 8);
        sheet.set(1, 0, 9);
        fb.sspr(&sheet, 0, 0, 2, 1, 0, 0, 2, 1, true, false);
        assert_eq!(
            fb.pget(1, 0),
            8,
            "flip puts the source-left pixel on the right"
        );
        assert_eq!(fb.pget(0, 0), 9);
    }

    #[test]
    fn print_pen_matches_print_at_cursor() {
        let mut a = Framebuffer::new();
        let mut b = Framebuffer::new();
        a.set_pen_color(9);
        a.set_cursor(10, 20);
        let end_a = a.print_pen("hi");
        let end_b = b.print("hi", 10, 20, 9);
        assert_eq!(end_a, end_b);
        assert_eq!(
            a.pixels(),
            b.pixels(),
            "print_pen draws identically to print"
        );
    }

    #[test]
    fn print_pen_advances_cursor_one_line() {
        let mut fb = Framebuffer::new();
        fb.set_cursor(5, 5);
        fb.print_pen("x");
        fb.print_pen("y");
        let mut expect = Framebuffer::new();
        expect.print("x", 5, 5, 6); // default pen color is 6
        expect.print("y", 5, 5 + font::GLYPH_H, 6);
        assert_eq!(fb.pixels(), expect.pixels());
    }
}
