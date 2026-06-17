//! Guarantees behind the "don't pre-snap positions" guidance in docs/ABI.md.
//!
//! The console floors every screen-space position to a pixel at draw time
//! (the `px` helper here mirrors what the wasm ABI does). These tests pin
//! the consequences a cart author can rely on — and the one limit that is
//! inherent to an integer screen.

use rico8_runtime::{assets::SpriteSheet, fb::Framebuffer};

/// What the ABI does to every position: floor to a pixel.
fn px(v: f32) -> i32 {
    v.floor() as i32
}

/// A recognizable 8x8 sprite (white body, red border, one blue corner) so a
/// distortion or mirroring would show up as a changed stamp.
fn entity_sheet() -> SpriteSheet {
    let mut s = SpriteSheet::default();
    for y in 0..8 {
        for x in 0..8 {
            let edge = x == 0 || y == 0 || x == 7 || y == 7;
            s.set(x, y, if edge { 8 } else { 7 });
        }
    }
    s.set(1, 1, 12);
    s
}

/// The 8x8 block of screen pixels at (x, y).
fn stamp(fb: &Framebuffer, x: i32, y: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    for dy in 0..8 {
        for dx in 0..8 {
            out.push(fb.pget(x + dx, y + dy));
        }
    }
    out
}

/// The PICO-8 `flr(x) + 0.5` anti-cobblestone idiom is a no-op here: because
/// the console floors, it lands on the exact same pixel as the raw position.
#[test]
fn flr_plus_half_is_a_noop() {
    let sheet = entity_sheet();
    for i in 0..200 {
        let pos = 20.0 + 0.137 * i as f32;
        let mut raw = Framebuffer::new();
        raw.cls(0);
        raw.spr(&sheet, 0, px(pos), px(pos), 1.0, 1.0, false, false);

        let snapped = pos.floor() + 0.5;
        let mut pre = Framebuffer::new();
        pre.cls(0);
        pre.spr(&sheet, 0, px(snapped), px(snapped), 1.0, 1.0, false, false);

        assert_eq!(raw.pixels(), pre.pixels(), "pre-snapping changed frame {i}");
    }
}

/// A single sprite moving diagonally at sub-pixel speed is always the same
/// rigid stamp, merely translated — no shimmer, no distortion.
#[test]
fn single_sprite_is_a_rigid_stamp() {
    let sheet = entity_sheet();
    let reference = {
        let mut fb = Framebuffer::new();
        fb.cls(0);
        fb.spr(&sheet, 0, 40, 40, 1.0, 1.0, false, false);
        stamp(&fb, 40, 40)
    };
    for i in 0..300 {
        let x = 20.0 + 0.31 * i as f32;
        let y = 15.0 + 0.19 * i as f32;
        let mut fb = Framebuffer::new();
        fb.cls(0);
        fb.spr(&sheet, 0, px(x), px(y), 1.0, 1.0, false, false);
        assert_eq!(
            stamp(&fb, px(x), px(y)),
            reference,
            "sprite was not a clean rigid stamp at ({x}, {y})"
        );
    }
}

/// The one inherent limit of an integer screen: two objects at a *fractional*
/// relative spacing that move together see their on-screen gap flicker. This
/// is unrelated to the camera, happens on PICO-8 too, and `flr(x) + 0.5`
/// cannot fix it. Documented so the guidance isn't mistaken for "shimmer is
/// impossible".
#[test]
fn subpixel_spacing_can_flicker() {
    let mut gaps = std::collections::BTreeSet::new();
    for i in 0..40 {
        let t = 0.1 * i as f32;
        let a = px(10.0 + t); // post A at world 10.0
        let b = px(17.5 + t); // post B at world 17.5 — 7.5 apart
        gaps.insert(b - a);
    }
    assert!(
        gaps.len() > 1,
        "expected the sub-pixel-spaced gap to flicker, got {gaps:?}"
    );
}
