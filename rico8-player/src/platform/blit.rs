//! Pure blit: indexed 128x128 -> XRGB8888, integer-scaled, letterboxed and rotated. No I/O.

use crate::platform::Rotate;
use rico8_runtime::{
    fb::{Framebuffer, HEIGHT, WIDTH},
    palette,
};

/// Clear `dst` to black and draw the largest integer-scaled, rotated, centered copy of `fb`.
pub fn present_into(fb: &Framebuffer, dst: &mut [u32], dst_w: usize, dst_h: usize, rot: Rotate) {
    for p in dst.iter_mut() {
        *p = 0;
    }
    let (sw, sh) = (WIDTH as usize, HEIGHT as usize); // 128 x 128, square.
    let scale = (dst_w / sw).min(dst_h / sh).max(1);
    let (out_w, out_h) = (sw * scale, sh * scale);
    let ox = dst_w.saturating_sub(out_w) / 2;
    let oy = dst_h.saturating_sub(out_h) / 2;

    for sy in 0..sh {
        for sx in 0..sw {
            // Map destination-source coords through the rotation.
            let (rx, ry) = match rot {
                Rotate::None => (sx, sy),
                Rotate::Cw90 => (sy, sw - 1 - sx),
                Rotate::Cw180 => (sw - 1 - sx, sh - 1 - sy),
                Rotate::Cw270 => (sh - 1 - sy, sx),
            };
            let idx = (fb.pget(rx as i32, ry as i32) & 0x0f) as usize;
            let [r, g, b] = palette::PALETTE[idx];
            let px = (r as u32) << 16 | (g as u32) << 8 | b as u32;
            for dy in 0..scale {
                let row = oy + sy * scale + dy;
                if row >= dst_h {
                    continue;
                }
                let base = row * dst_w + ox + sx * scale;
                for dx in 0..scale {
                    let col = ox + sx * scale + dx;
                    if col < dst_w {
                        dst[base + dx] = px;
                    }
                }
            }
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
    fn letterbox_borders_are_black() {
        // 200x140 dst: integer scale = 1 (since 2 would need 256x256), centered with borders.
        let mut fb = Framebuffer::new();
        fb.cls(col::WHITE);
        let (w, h) = (200usize, 140usize);
        let mut dst = vec![0xdeadbeef_u32; w * h];
        present_into(&fb, &mut dst, w, h, Rotate::None);
        assert_eq!(dst[0], 0, "top-left corner is letterbox black");
        let (ox, oy) = ((w - 128) / 2, (h - 128) / 2);
        assert_eq!(dst[oy * w + ox], rgb(col::WHITE), "centered image present");
    }

    #[test]
    fn rotate_90_maps_top_left_to_top_right() {
        // 128x128, single red pixel at (0,0). After CW90 it lands at (w-1, 0).
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
