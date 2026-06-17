//! Guards that `use rico8::*;` keeps `printf!`/`logf!` unambiguous with every
//! prelude macro — the reason `print!` itself was unusable. Stand-in sinks
//! avoid the crate-private `Graphics`/`Context` while still exercising macro
//! resolution and expansion.

use rico8::*;

struct FakeGfx;

impl FakeGfx {
    fn print(&mut self, _s: &str, _x: f32, _y: f32, _color: Color) -> f32 {
        0.0
    }
}

struct FakeCtx;

impl FakeCtx {
    fn log(&mut self, _s: &str) {}
}

#[test]
fn macros_resolve_through_glob_and_path() {
    let mut gfx = FakeGfx;
    let mut ctx = FakeCtx;

    // Unqualified, through the glob import.
    let _: f32 = printf!(gfx, 0.0, 0.0, Color::WHITE, "x {}", 1);
    logf!(ctx, "y {}", 2);

    // Path-qualified.
    let _: f32 = rico8::printf!(gfx, 0.0, 0.0, Color::WHITE, "x {}", 1);
    rico8::logf!(ctx, "y {}", 2);
}
