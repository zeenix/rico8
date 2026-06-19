//! Pure blit: indexed 128x128 -> XRGB8888, aspect-fit (nearest-neighbour) scaled, letterboxed
//! and rotated. No I/O.

use crate::platform::Rotate;
use rico8_runtime::{
    fb::{Framebuffer, HEIGHT, WIDTH},
    palette,
};

/// Clear `dst` to black and draw `fb` scaled to fill the smaller destination dimension with the
/// aspect ratio preserved, centered and rotated, using nearest-neighbour sampling.
///
/// The source is square (128x128), so the largest aspect-preserving fit is a square whose side
/// equals the smaller of `dst_w`/`dst_h`; the longer axis is letterboxed/pillarboxed.
pub fn present_into(fb: &Framebuffer, dst: &mut [u32], dst_w: usize, dst_h: usize, rot: Rotate) {
    for p in dst.iter_mut() {
        *p = 0;
    }
    if dst_w == 0 || dst_h == 0 {
        return;
    }
    let (sw, sh) = (WIDTH as usize, HEIGHT as usize); // 128 x 128, square.
    let out = dst_w.min(dst_h);
    let ox = (dst_w - out) / 2;
    let oy = (dst_h - out) / 2;

    for dy in 0..out {
        let sy = dy * sh / out; // Nearest-neighbour source row, in 0..sh.
        let base = (oy + dy) * dst_w + ox;
        for dx in 0..out {
            let sx = dx * sw / out; // Nearest-neighbour source column, in 0..sw.
                                    // Map the screen-space source pixel through the panel rotation to read `fb`.
            let (rx, ry) = match rot {
                Rotate::None => (sx, sy),
                Rotate::Cw90 => (sy, sw - 1 - sx),
                Rotate::Cw180 => (sw - 1 - sx, sh - 1 - sy),
                Rotate::Cw270 => (sh - 1 - sy, sx),
            };
            let idx = (fb.pget(rx as i32, ry as i32) & 0x0f) as usize;
            let [r, g, b] = palette::PALETTE[idx];
            dst[base + dx] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::palette::col;

    fn rgb(idx: u8) -> u32 {
        let [r, g, b] = palette::PALETTE[idx as usize];
        (r as u32) << 16 | (g as u32) << 8 | b as u32
    }

    #[test]
    fn exact_fit_no_letterbox() {
        // 128x128 dst, scale 1, no rotation: dst[y*128+x] == palette(fb.pget(x,y)).
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        fb.pset(127, 127, col::GREEN);
        let mut dst = vec![0u32; WIDTH as usize * HEIGHT as usize];
        present_into(&fb, &mut dst, WIDTH as usize, HEIGHT as usize, Rotate::None);
        assert_eq!(dst[0], rgb(col::RED));
        assert_eq!(dst[127 * 128 + 127], rgb(col::GREEN));
    }

    #[test]
    fn fills_smaller_dimension_and_pillarboxes() {
        // 200x140 dst: the square content fills the smaller dimension (height 140) and is
        // pillarboxed left/right (out = min(200,140) = 140, ox = 30, oy = 0).
        let mut fb = Framebuffer::new();
        fb.cls(col::WHITE);
        let (w, h) = (200usize, 140usize);
        let mut dst = vec![0xdeadbeef_u32; w * h];
        present_into(&fb, &mut dst, w, h, Rotate::None);
        let ox = (w - h) / 2; // 30.
                              // The far corners are black pillarbox.
        assert_eq!(dst[0], 0, "left pillarbox is black");
        assert_eq!(dst[(h / 2) * w], 0, "left pillarbox is black mid-height");
        // The content reaches the very top and bottom rows (it fills the height).
        assert_eq!(dst[ox], rgb(col::WHITE), "content reaches the top edge");
        assert_eq!(
            dst[(h - 1) * w + ox],
            rgb(col::WHITE),
            "content reaches the bottom edge"
        );
    }

    #[test]
    fn fractional_scale_fills_height_on_landscape() {
        // 640x480: content fills the full 480 height (out=480), pillarboxed 80px each side.
        let mut fb = Framebuffer::new();
        fb.cls(col::WHITE);
        let (w, h) = (640usize, 480usize);
        let mut dst = vec![0u32; w * h];
        present_into(&fb, &mut dst, w, h, Rotate::None);
        let out = w.min(h); // 480.
        let ox = (w - out) / 2; // 80.
        assert_eq!(dst[(h / 2) * w], 0, "left edge is black");
        assert_eq!(dst[(h / 2) * w + (w - 1)], 0, "right edge is black");
        assert_eq!(
            dst[(h / 2) * w + (ox - 1)],
            0,
            "pillarbox ends exactly at the content"
        );
        assert_eq!(dst[ox], rgb(col::WHITE), "content top-left corner");
        assert_eq!(
            dst[(h - 1) * w + ox + out - 1],
            rgb(col::WHITE),
            "content bottom-right"
        );
    }

    #[test]
    fn nearest_neighbour_doubles_each_pixel_at_2x() {
        // 256x256: out=256, scale 2. fb pixel (1,0) maps to the 2x2 block at screen (2..4, 0..2).
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(1, 0, col::RED);
        let mut dst = vec![0u32; 256 * 256];
        present_into(&fb, &mut dst, 256, 256, Rotate::None);
        for dy in 0..2 {
            for dx in 2..4 {
                assert_eq!(
                    dst[dy * 256 + dx],
                    rgb(col::RED),
                    "doubled pixel at ({dx},{dy})"
                );
            }
        }
        assert_eq!(dst[0], rgb(col::BLACK), "(0,0) stays background");
    }

    #[test]
    fn rotate_90_maps_top_left_to_top_right() {
        // 128x128, single red pixel at (0,0). After CW90 it lands at (127,0).
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        let mut dst = vec![0u32; 128 * 128];
        present_into(&fb, &mut dst, 128, 128, Rotate::Cw90);
        assert_eq!(dst[127], rgb(col::RED), "(0,0) -> (127,0) under CW90");
    }

    #[test]
    fn rotate_180_maps_top_left_to_bottom_right() {
        // 128x128, single red pixel at (0,0). After CW180 it lands at (127,127).
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        let mut dst = vec![0u32; 128 * 128];
        present_into(&fb, &mut dst, 128, 128, Rotate::Cw180);
        assert_eq!(
            dst[127 * 128 + 127],
            rgb(col::RED),
            "(0,0) -> (127,127) under CW180"
        );
    }
}
