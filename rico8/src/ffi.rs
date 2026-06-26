//! The raw RICO-8 WASM ABI.
//!
//! This is the entire surface between a cart and the console: a small,
//! explicit, C-like import set in the `"rico8"` module. Carts cannot
//! reach anything else — no filesystem, no network, no host memory.
//! Game code should use the safe wrappers in the crate root; this module
//! is public only so the ABI is inspectable and documented in one place
//! (see docs/ABI.md).
//!
//! Screen-space positions are `i32` pixels and sizes are `i32` (the SDK
//! validates sizes to non-zero before calling here). Discrete things — sprite
//! and tile indices, colors, flags, buttons — stay integers too. Returns that
//! are genuinely fractional (`time`, `rnd`, the CPU/fps gauges) stay `f32`.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "rico8")]
extern "C" {
    pub fn clear(color: i32);
    pub fn camera(x: i32, y: i32);
    pub fn clip(x: f32, y: f32, w: f32, h: f32);
    pub fn set_pixel(x: i32, y: i32, color: i32);
    pub fn pixel(x: i32, y: i32) -> i32;
    pub fn line(x0: i32, y0: i32, x1: i32, y1: i32, color: i32);
    pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn rect_fill(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn circle(x: i32, y: i32, r: i32, color: i32);
    pub fn circle_fill(x: i32, y: i32, r: i32, color: i32);
    pub fn print(ptr: *const u8, len: u32, x: i32, y: i32, color: i32) -> i32;
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
    pub fn music(n: i32, fade_duration: i32, channel_mask: i32, token: i32) -> i32;
    pub fn time() -> f32;
    pub fn rnd() -> f32;
    pub fn seed_rng(seed: u32);
    pub fn sprite_pixel(x: i32, y: i32) -> i32;
    pub fn set_sprite_pixel(x: i32, y: i32, color: i32);
    pub fn log(ptr: *const u8, len: u32);
    pub fn panic(ptr: *const u8, len: u32);
    pub fn set_transparent_color(color: i32, transparent: i32);
    pub fn reset_transparency();
    pub fn remap_color(from: i32, to: i32, mode: i32);
    pub fn reset_palette();
    pub fn sprite_stretch(
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        flip_x: i32,
        flip_y: i32,
    );
    pub fn ellipse(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn ellipse_fill(x0: f32, y0: f32, x1: f32, y1: f32, color: i32);
    pub fn set_fill_pattern(pattern: i32, secondary: i32, transparent: i32);
    pub fn set_pen_color(color: i32);
    pub fn set_cursor(x: i32, y: i32);
    pub fn print_pen(ptr: *const u8, len: u32) -> i32;
    pub fn cpu_update() -> f32;
    pub fn cpu_draw() -> f32;
    pub fn fps() -> f32;
}

// Host-target stubs so the SDK (and carts) also type-check, document and
// unit-test on native targets. Real behavior only exists on wasm32.
#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #![allow(clippy::missing_safety_doc)]

    pub unsafe fn clear(_color: i32) {}
    pub unsafe fn camera(_x: i32, _y: i32) {}
    pub unsafe fn clip(_x: f32, _y: f32, _w: f32, _h: f32) {}
    pub unsafe fn set_pixel(_x: i32, _y: i32, _color: i32) {}
    pub unsafe fn pixel(_x: i32, _y: i32) -> i32 {
        0
    }
    pub unsafe fn line(_x0: i32, _y0: i32, _x1: i32, _y1: i32, _color: i32) {}
    pub unsafe fn rect(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn rect_fill(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn circle(_x: i32, _y: i32, _r: i32, _color: i32) {}
    pub unsafe fn circle_fill(_x: i32, _y: i32, _r: i32, _color: i32) {}
    pub unsafe fn print(_ptr: *const u8, _len: u32, _x: i32, _y: i32, _color: i32) -> i32 {
        0
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
    // Pretend a start always succeeds (nonzero token) so cart logic type-checks
    // and unit-tests on native targets, with no real audio.
    pub unsafe fn music(_n: i32, _fade_duration: i32, _channel_mask: i32, _token: i32) -> i32 {
        1
    }
    pub unsafe fn time() -> f32 {
        0.0
    }
    pub unsafe fn rnd() -> f32 {
        0.0
    }
    pub unsafe fn seed_rng(_seed: u32) {}
    pub unsafe fn sprite_pixel(_x: i32, _y: i32) -> i32 {
        0
    }
    pub unsafe fn set_sprite_pixel(_x: i32, _y: i32, _color: i32) {}
    pub unsafe fn log(_ptr: *const u8, _len: u32) {}
    pub unsafe fn panic(_ptr: *const u8, _len: u32) {}
    pub unsafe fn set_transparent_color(_color: i32, _transparent: i32) {}
    pub unsafe fn reset_transparency() {}
    pub unsafe fn remap_color(_from: i32, _to: i32, _mode: i32) {}
    pub unsafe fn reset_palette() {}
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn sprite_stretch(
        _sx: i32,
        _sy: i32,
        _sw: i32,
        _sh: i32,
        _dx: f32,
        _dy: f32,
        _dw: f32,
        _dh: f32,
        _flip_x: i32,
        _flip_y: i32,
    ) {
    }
    pub unsafe fn ellipse(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn ellipse_fill(_x0: f32, _y0: f32, _x1: f32, _y1: f32, _color: i32) {}
    pub unsafe fn set_fill_pattern(_pattern: i32, _secondary: i32, _transparent: i32) {}
    pub unsafe fn set_pen_color(_color: i32) {}
    pub unsafe fn set_cursor(_x: i32, _y: i32) {}
    pub unsafe fn print_pen(_ptr: *const u8, _len: u32) -> i32 {
        0
    }
    pub unsafe fn cpu_update() -> f32 {
        0.0
    }
    pub unsafe fn cpu_draw() -> f32 {
        0.0
    }
    pub unsafe fn fps() -> f32 {
        0.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;
