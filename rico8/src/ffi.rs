//! The raw RICO-8 WASM ABI.
//!
//! This is the entire surface between a cart and the console: a small,
//! explicit, C-like import set in the `"rico8"` module. Carts cannot
//! reach anything else — no filesystem, no network, no host memory.
//! Game code should use the safe wrappers in the crate root; this module
//! is public only so the ABI is inspectable and documented in one place
//! (see docs/ABI.md).
//!
//! Screen-space positions and sizes are `f32`: the host floors each to a
//! pixel at draw time, so carts can carry sub-pixel positions (smooth
//! motion, no shimmer) without rounding themselves. Discrete things —
//! sprite/tile indices, cell counts, flags, colors, buttons — stay
//! integers.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "rico8")]
extern "C" {
    pub fn clear(color: i32);
    pub fn camera(x: f32, y: f32);
    pub fn clip(x: f32, y: f32, w: f32, h: f32);
    pub fn set_pixel(x: f32, y: f32, color: i32);
    pub fn pixel(x: f32, y: f32) -> i32;
    pub fn line(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn rect_fill(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn circle(x: f32, y: f32, r: f32, color: i32);
    pub fn circle_fill(x: f32, y: f32, r: f32, color: i32);
    pub fn print(ptr: *const u8, len: u32, x: f32, y: f32, color: i32) -> f32;
    pub fn is_button_down(b: u32) -> i32;
    pub fn is_button_pressed(b: u32) -> i32;
    pub fn buttons_down() -> u32;
    pub fn buttons_pressed() -> u32;
    pub fn sprite(n: u32, x: f32, y: f32, w: f32, h: f32, flip_x: i32, flip_y: i32);
    pub fn map(cel_x: i32, cel_y: i32, sx: f32, sy: f32, cel_w: i32, cel_h: i32, layers: u32);
    pub fn map_tile(x: i32, y: i32) -> i32;
    pub fn set_map_tile(x: i32, y: i32, v: u32);
    pub fn sprite_flags(n: u32) -> i32;
    pub fn set_sprite_flags(n: u32, flags: u32);
    pub fn sfx(n: i32, channel: i32);
    pub fn music(n: i32);
    pub fn time() -> f32;
    pub fn rnd() -> f32;
    pub fn log(ptr: *const u8, len: u32);
    pub fn panic(ptr: *const u8, len: u32);
}

// Host-target stubs so the SDK (and carts) also type-check, document and
// unit-test on native targets. Real behavior only exists on wasm32.
#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #![allow(clippy::missing_safety_doc)]

    pub unsafe fn clear(_color: i32) {}
    pub unsafe fn camera(_x: f32, _y: f32) {}
    pub unsafe fn clip(_x: f32, _y: f32, _w: f32, _h: f32) {}
    pub unsafe fn set_pixel(_x: f32, _y: f32, _color: i32) {}
    pub unsafe fn pixel(_x: f32, _y: f32) -> i32 {
        0
    }
    pub unsafe fn line(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn rect(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn rect_fill(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn circle(_x: f32, _y: f32, _r: f32, _color: i32) {}
    pub unsafe fn circle_fill(_x: f32, _y: f32, _r: f32, _color: i32) {}
    pub unsafe fn print(_ptr: *const u8, _len: u32, _x: f32, _y: f32, _color: i32) -> f32 {
        0.0
    }
    pub unsafe fn is_button_down(_b: u32) -> i32 {
        0
    }
    pub unsafe fn is_button_pressed(_b: u32) -> i32 {
        0
    }
    pub unsafe fn buttons_down() -> u32 {
        0
    }
    pub unsafe fn buttons_pressed() -> u32 {
        0
    }
    pub unsafe fn sprite(_n: u32, _x: f32, _y: f32, _w: f32, _h: f32, _flip_x: i32, _flip_y: i32) {}
    pub unsafe fn map(
        _cel_x: i32,
        _cel_y: i32,
        _sx: f32,
        _sy: f32,
        _cel_w: i32,
        _cel_h: i32,
        _layers: u32,
    ) {
    }
    pub unsafe fn map_tile(_x: i32, _y: i32) -> i32 {
        0
    }
    pub unsafe fn set_map_tile(_x: i32, _y: i32, _v: u32) {}
    pub unsafe fn sprite_flags(_n: u32) -> i32 {
        0
    }
    pub unsafe fn set_sprite_flags(_n: u32, _flags: u32) {}
    pub unsafe fn sfx(_n: i32, _channel: i32) {}
    pub unsafe fn music(_n: i32) {}
    pub unsafe fn time() -> f32 {
        0.0
    }
    pub unsafe fn rnd() -> f32 {
        0.0
    }
    pub unsafe fn log(_ptr: *const u8, _len: u32) {}
    pub unsafe fn panic(_ptr: *const u8, _len: u32) {}
}

#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;
