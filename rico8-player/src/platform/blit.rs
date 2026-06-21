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
    if dst_w == 0 || dst_h == 0 {
        for p in dst.iter_mut() {
            *p = 0;
        }
        return;
    }
    let (sw, sh) = (WIDTH as usize, HEIGHT as usize); // 128 x 128, square.
    let out = dst_w.min(dst_h);
    let ox = (dst_w - out) / 2;
    let oy = (dst_h - out) / 2;

    // Display-palette LUT: fold the screen-time color remap (`display_pal`, PICO-8's
    // `pal(c0,c1,1)`) into a 16-entry table once, then index it per pixel instead of rebuilding the
    // pack. Stored index `i` is shown as color `dpal[i]`, exactly as `write_rgba` does for GPU
    // upload, so display-palette fades/flashes/swaps now render identically on the console, web and
    // player. Each color is packed into native-endian XRGB8888; XRGB native-endian assumes
    // little-endian, which both the KMS and window backends target.
    let dpal = fb.display_palette();
    let mut lut = [0u32; 16];
    for (i, slot) in lut.iter_mut().enumerate() {
        let [r, g, b] = palette::PALETTE[dpal[i] as usize];
        *slot = (r as u32) << 16 | (g as u32) << 8 | b as u32;
    }

    // Precompute the nearest-neighbour source index for every output coordinate `k in 0..out`.
    // The source is square (sw == sh == 128), so the same map serves both axes:
    // `src_map[k] = k * 128 / out`, matching the original per-pixel `dx*sw/out` divide exactly,
    // including at fractional scales (e.g. out=480, where rows are not uniformly duplicated).
    let src_map: Vec<usize> = (0..out).map(|k| k * sw / out).collect();

    // The raw palette-index buffer, row-major 128-wide. Indexing it directly for in-range coords
    // (all `rx`/`ry` here are 0..128, always in range) is equivalent to `pget`'s non-OOB path.
    let pixels = fb.pixels();

    // Row-replication insight: for a fixed output row `dy` the source row `sy = src_map[dy]` is
    // fixed, and for EVERY rotation exactly one source coordinate stays constant across the row
    // while the other varies with `dx`. Hence two output rows with the same `sy` produce identical
    // content, so we only build a row when `sy` changes and otherwise memcpy the previous row. At
    // typical 2x-4x upscales most rows are duplicates, turning per-pixel work into a
    // `copy_from_slice`.
    let mut prev_sy = usize::MAX;
    let mut prev_base = 0usize;
    for dy in 0..out {
        let sy = src_map[dy]; // Nearest-neighbour source row, in 0..sh.
        let base = (oy + dy) * dst_w + ox;
        if sy == prev_sy {
            // Same source row as the previous output row: copy its already-built content span.
            let (head, tail) = dst.split_at_mut(base);
            tail[..out].copy_from_slice(&head[prev_base..prev_base + out]);
            continue;
        }
        let row = &mut dst[base..base + out];
        // Build one output row, holding the constant source coordinate (derived from `sy`) and
        // varying the other with `dx` through `src_map`. The rotation mapping mirrors the original:
        //   None:  (rx,ry) = (src_map[dx], sy)
        //   Cw90:  (rx,ry) = (sy, 127 - src_map[dx])
        //   Cw180: (rx,ry) = (127 - src_map[dx], 127 - sy)
        //   Cw270: (rx,ry) = (127 - sy, src_map[dx])
        match rot {
            Rotate::None => {
                let row_off = sy * sw;
                for (dx, out_px) in row.iter_mut().enumerate() {
                    let idx = (pixels[row_off + src_map[dx]] & 0x0f) as usize;
                    *out_px = lut[idx];
                }
            }
            Rotate::Cw90 => {
                for (dx, out_px) in row.iter_mut().enumerate() {
                    let ry = sh - 1 - src_map[dx];
                    let idx = (pixels[ry * sw + sy] & 0x0f) as usize;
                    *out_px = lut[idx];
                }
            }
            Rotate::Cw180 => {
                let row_off = (sh - 1 - sy) * sw;
                for (dx, out_px) in row.iter_mut().enumerate() {
                    let rx = sw - 1 - src_map[dx];
                    let idx = (pixels[row_off + rx] & 0x0f) as usize;
                    *out_px = lut[idx];
                }
            }
            Rotate::Cw270 => {
                let rx = sw - 1 - sy;
                for (dx, out_px) in row.iter_mut().enumerate() {
                    let ry = src_map[dx];
                    let idx = (pixels[ry * sw + rx] & 0x0f) as usize;
                    *out_px = lut[idx];
                }
            }
        }
        prev_sy = sy;
        prev_base = base;
    }

    // Border-only clear: the content square `[ox, ox+out) x [oy, oy+out)` is fully written above,
    // so only the letterbox/pillarbox border needs zeroing. Its complement is exactly: the top band
    // (rows `0..oy`), the bottom band (rows `oy+out..dst_h`), and within each content row the left
    // pillar (`0..ox`) and right pillar (`ox+out..dst_w`).
    dst[..oy * dst_w].fill(0);
    dst[(oy + out) * dst_w..].fill(0);
    for dy in 0..out {
        let row_start = (oy + dy) * dst_w;
        dst[row_start..row_start + ox].fill(0);
        dst[row_start + ox + out..row_start + dst_w].fill(0);
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
    fn present_applies_display_palette() {
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        // Display-time remap RED -> GREEN; the stored index must stay RED.
        fb.remap_display_color(col::RED, col::GREEN);
        let mut dst = vec![0u32; 128 * 128];
        present_into(&fb, &mut dst, 128, 128, Rotate::None);
        assert_eq!(
            dst[0],
            rgb(col::GREEN),
            "display remap RED->GREEN is honored"
        );
        assert_eq!(fb.pget(0, 0), col::RED, "the stored index is unchanged");
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

    #[test]
    fn rotate_270_maps_top_left_to_bottom_left() {
        // 128x128, single red pixel at (0,0). Under CW270 the mapping is (rx,ry) = (127-dy, dx),
        // so fb (0,0) is read at screen (dx=0, dy=127), i.e. the bottom-left corner.
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        let mut dst = vec![0u32; 128 * 128];
        present_into(&fb, &mut dst, 128, 128, Rotate::Cw270);
        assert_eq!(
            dst[127 * 128],
            rgb(col::RED),
            "(0,0) -> (0,127) under CW270"
        );
    }

    #[test]
    fn rotate_90_doubles_each_pixel_at_2x() {
        // 256x256 (out=256, scale 2) under CW90: src_map[k] = k/2. fb (0,0) maps to (rx,ry)=(0,0)
        // when sy=0 (dy in 0..2) and 127 - dx/2 == 0 (dx in 254..256), i.e. the 2x2 block at the
        // top-right corner. This locks the rotated row-build at a non-1x scale.
        let mut fb = Framebuffer::new();
        fb.cls(col::BLACK);
        fb.pset(0, 0, col::RED);
        let mut dst = vec![0u32; 256 * 256];
        present_into(&fb, &mut dst, 256, 256, Rotate::Cw90);
        for dy in 0..2 {
            for dx in 254..256 {
                assert_eq!(
                    dst[dy * 256 + dx],
                    rgb(col::RED),
                    "doubled rotated pixel at ({dx},{dy})"
                );
            }
        }
        // A pixel just outside that block reads fb's background.
        assert_eq!(dst[2 * 256 + 255], rgb(col::BLACK), "row below the block");
        assert_eq!(dst[253], rgb(col::BLACK), "column left of the block");
    }
}
