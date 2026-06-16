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
    pub fn cls(color: i32);
    pub fn camera(x: f32, y: f32);
    pub fn clip(x: f32, y: f32, w: f32, h: f32);
    pub fn pset(x: f32, y: f32, color: i32);
    pub fn pget(x: f32, y: f32) -> i32;
    pub fn line(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn rectfill(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn circ(x: f32, y: f32, r: f32, color: i32);
    pub fn circfill(x: f32, y: f32, r: f32, color: i32);
    pub fn print(ptr: *const u8, len: u32, x: f32, y: f32, color: i32) -> f32;
    pub fn btn(b: u32) -> i32;
    pub fn btnp(b: u32) -> i32;
    pub fn btn_mask() -> u32;
    pub fn btnp_mask() -> u32;
    pub fn spr(n: u32, x: f32, y: f32, w: f32, h: f32, flip_x: i32, flip_y: i32);
    pub fn map(cel_x: i32, cel_y: i32, sx: f32, sy: f32, cel_w: i32, cel_h: i32, layers: u32);
    pub fn mget(x: i32, y: i32) -> i32;
    pub fn mset(x: i32, y: i32, v: u32);
    pub fn fget(n: u32) -> i32;
    pub fn fset(n: u32, flags: u32);
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

    pub unsafe fn cls(_color: i32) {}
    pub unsafe fn camera(_x: f32, _y: f32) {}
    pub unsafe fn clip(_x: f32, _y: f32, _w: f32, _h: f32) {}
    pub unsafe fn pset(_x: f32, _y: f32, _color: i32) {}
    pub unsafe fn pget(_x: f32, _y: f32) -> i32 {
        0
    }
    pub unsafe fn line(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn rect(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn rectfill(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn circ(_x: f32, _y: f32, _r: f32, _color: i32) {}
    pub unsafe fn circfill(_x: f32, _y: f32, _r: f32, _color: i32) {}
    pub unsafe fn print(_ptr: *const u8, _len: u32, _x: f32, _y: f32, _color: i32) -> f32 {
        0.0
    }
    pub unsafe fn btn(_b: u32) -> i32 {
        0
    }
    pub unsafe fn btnp(_b: u32) -> i32 {
        0
    }
    pub unsafe fn btn_mask() -> u32 {
        0
    }
    pub unsafe fn btnp_mask() -> u32 {
        0
    }
    pub unsafe fn spr(_n: u32, _x: f32, _y: f32, _w: f32, _h: f32, _flip_x: i32, _flip_y: i32) {}
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
    pub unsafe fn mget(_x: i32, _y: i32) -> i32 {
        0
    }
    pub unsafe fn mset(_x: i32, _y: i32, _v: u32) {}
    pub unsafe fn fget(_n: u32) -> i32 {
        0
    }
    pub unsafe fn fset(_n: u32, _flags: u32) {}
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
