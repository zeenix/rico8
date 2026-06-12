//! The raw RICO-8 WASM ABI.
//!
//! This is the entire surface between a cart and the console: a small,
//! explicit, C-like import set in the `"rico8"` module. Carts cannot
//! reach anything else — no filesystem, no network, no host memory.
//! Game code should use the safe wrappers in the crate root; this module
//! is public only so the ABI is inspectable and documented in one place
//! (see docs/ABI.md).

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "rico8")]
extern "C" {
    pub fn cls(color: i32);
    pub fn camera(x: i32, y: i32);
    pub fn clip(x: i32, y: i32, w: i32, h: i32);
    pub fn pset(x: i32, y: i32, color: i32);
    pub fn pget(x: i32, y: i32) -> i32;
    pub fn line(x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
    pub fn rect(x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
    pub fn rectfill(x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
    pub fn circ(x: i32, y: i32, r: i32, color: i32);
    pub fn circfill(x: i32, y: i32, r: i32, color: i32);
    pub fn print(ptr: *const u8, len: u32, x: i32, y: i32, color: i32) -> i32;
    pub fn btn(b: u32) -> i32;
    pub fn btnp(b: u32) -> i32;
    pub fn spr(n: u32, x: i32, y: i32, w: u32, h: u32, flip_x: i32, flip_y: i32);
    pub fn map(cel_x: i32, cel_y: i32, sx: i32, sy: i32, cel_w: i32, cel_h: i32, layers: u32);
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
    pub unsafe fn camera(_x: i32, _y: i32) {}
    pub unsafe fn clip(_x: i32, _y: i32, _w: i32, _h: i32) {}
    pub unsafe fn pset(_x: i32, _y: i32, _color: i32) {}
    pub unsafe fn pget(_x: i32, _y: i32) -> i32 {
        0
    }
    pub unsafe fn line(_x0: i32, _y0: i32, _x1: i32, _y1: i32, _color: i32) {}
    pub unsafe fn rect(_x0: i32, _y0: i32, _x1: i32, _y1: i32, _color: i32) {}
    pub unsafe fn rectfill(_x0: i32, _y0: i32, _x1: i32, _y1: i32, _color: i32) {}
    pub unsafe fn circ(_x: i32, _y: i32, _r: i32, _color: i32) {}
    pub unsafe fn circfill(_x: i32, _y: i32, _r: i32, _color: i32) {}
    pub unsafe fn print(_ptr: *const u8, _len: u32, _x: i32, _y: i32, _color: i32) -> i32 {
        0
    }
    pub unsafe fn btn(_b: u32) -> i32 {
        0
    }
    pub unsafe fn btnp(_b: u32) -> i32 {
        0
    }
    pub unsafe fn spr(_n: u32, _x: i32, _y: i32, _w: u32, _h: u32, _flip_x: i32, _flip_y: i32) {}
    pub unsafe fn map(
        _cel_x: i32,
        _cel_y: i32,
        _sx: i32,
        _sy: i32,
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
