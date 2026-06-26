//! # rico8 — the RICO-8 fantasy console SDK
//!
//! Write tiny games in Rust, run them on a tiny console.
//!
//! ```no_run
//! use rico8::*;
//!
//! struct MyGame {
//!     x: i32,
//!     y: i32,
//! }
//!
//! impl Game for MyGame {
//!     fn update(&mut self, ctx: &mut Context) {
//!         if ctx.is_button_down(Button::Right) {
//!             self.x += 1;
//!         }
//!     }
//!
//!     fn draw(&self, gfx: &mut Graphics) {
//!         gfx.clear(Color::BLACK);
//!         gfx.rect_fill(self.x, self.y, 8, 8, Color::WHITE).unwrap();
//!     }
//! }
//!
//! rico8::game!(MyGame { x: 64, y: 64 });
//! ```
//!
//! Carts are built for `wasm32-unknown-unknown` as a `cdylib` and run in
//! a strict sandbox: the host functions wrapped by [`Context`] and
//! [`Graphics`] are the only doors out. The screen is 128x128, the
//! palette has 16 fixed colors, `update`/`draw` run at 60 fps (or 30,
//! if the game sets [`Game::FRAME_RATE`]). The constraints
//! are the point.
//!
//! For formatted on-screen text and debug logs, see the [`printf!`](crate::printf)
//! and [`logf!`](crate::logf) macros.
#![cfg_attr(not(feature = "std"), no_std)]

mod dim;
pub mod ffi;
mod flags;
mod fmt;
mod glue;
pub mod memstat;
mod motion;
mod music;

// Install the live-tracking allocator for `std` carts. It lives here, not in
// the `game!` macro, so the `feature = "std"` cfg is evaluated in this crate
// (where the feature is defined) rather than in the cart crate (which has no
// such feature). A library-defined global allocator is picked up by the cart
// cdylib that links it. Wasm-only: on the host it would perturb this crate's
// own allocation tests.
#[cfg(all(feature = "std", target_arch = "wasm32"))]
#[global_allocator]
static RICO8_ALLOC: memstat::TrackingAlloc = memstat::TrackingAlloc;

use crate::flags::bitflag_enum;
pub use crate::flags::{BitFlag, BitFlags, UnknownBits};
use core::ops::{Bound, RangeBounds};
pub use dim::{Dim, ZeroSize};
pub use glue::__internal;
pub use motion::Body;
pub use music::{Music, MusicBusy, MusicChannel, PlayingMusic};

/// The screen is 128x128 pixels.
pub const SCREEN_WIDTH: u32 = 128;
pub const SCREEN_HEIGHT: u32 = 128;
/// Default logical frames per second.
pub const FPS: u32 = 60;

/// How many times per second a cart's `update` and `draw` run.
///
/// The default is [`FrameRate::Fps60`]; set [`Game::FRAME_RATE`] to
/// [`FrameRate::Fps30`] for a 30 fps game, where both `update` and `draw`
/// are called half as often.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRate {
    /// 30 frames per second.
    Fps30,
    /// 60 frames per second (the default).
    Fps60,
}

impl FrameRate {
    /// The rate as a plain frames-per-second number.
    pub const fn fps(self) -> u32 {
        match self {
            FrameRate::Fps30 => 30,
            FrameRate::Fps60 => 60,
        }
    }
}

/// A color in the fixed 16-color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8);

impl Color {
    pub const BLACK: Color = Color(0);
    pub const DARK_BLUE: Color = Color(1);
    pub const DARK_PURPLE: Color = Color(2);
    pub const DARK_GREEN: Color = Color(3);
    pub const BROWN: Color = Color(4);
    pub const DARK_GREY: Color = Color(5);
    pub const LIGHT_GREY: Color = Color(6);
    pub const WHITE: Color = Color(7);
    pub const RED: Color = Color(8);
    pub const ORANGE: Color = Color(9);
    pub const YELLOW: Color = Color(10);
    pub const GREEN: Color = Color(11);
    pub const BLUE: Color = Color(12);
    pub const LAVENDER: Color = Color(13);
    pub const PINK: Color = Color(14);
    pub const PEACH: Color = Color(15);

    /// Color from a palette index (wraps at 16).
    pub const fn from_index(i: u8) -> Color {
        Color(i & 0x0f)
    }
}

impl From<u8> for Color {
    fn from(i: u8) -> Self {
        Color::from_index(i)
    }
}

bitflag_enum! {
    /// The six console buttons.
    pub enum Button {
        Left = 1 << 0,
        Right = 1 << 1,
        Up = 1 << 2,
        Down = 1 << 3,
        /// "O" action button — Z, C or N on the keyboard.
        O = 1 << 4,
        /// "X" action button — X, V or M on the keyboard.
        X = 1 << 5,
    }
}

impl Button {
    /// `Left` and `Up` held together, as a set — `ctx.buttons_down().contains(Button::UP_LEFT)`.
    pub const UP_LEFT: BitFlags<Button> =
        // SAFETY: `Left` and `Up` are real `Button` flags, so the combined bits are valid.
        unsafe { BitFlags::from_bits_unchecked(Button::Left as u8 | Button::Up as u8) };
    /// `Right` and `Up` held together.
    pub const UP_RIGHT: BitFlags<Button> =
        // SAFETY: `Right` and `Up` are real `Button` flags.
        unsafe { BitFlags::from_bits_unchecked(Button::Right as u8 | Button::Up as u8) };
    /// `Left` and `Down` held together.
    pub const DOWN_LEFT: BitFlags<Button> =
        // SAFETY: `Left` and `Down` are real `Button` flags.
        unsafe { BitFlags::from_bits_unchecked(Button::Left as u8 | Button::Down as u8) };
    /// `Right` and `Down` held together.
    pub const DOWN_RIGHT: BitFlags<Button> =
        // SAFETY: `Right` and `Down` are real `Button` flags.
        unsafe { BitFlags::from_bits_unchecked(Button::Right as u8 | Button::Down as u8) };
}

/// The ABI button index (`0..=5`) for a [`Button`] flag.
const fn button_index(b: Button) -> u32 {
    (b as u8).trailing_zeros()
}

bitflag_enum! {
    /// One of a sprite's eight flags. The flags carry no fixed meaning — a cart
    /// assigns its own (e.g. "solid"). Used by [`Context::sprite_flags`] /
    /// [`Context::set_sprite_flags`] and the [`Graphics::map`] layer filter.
    pub enum SpriteFlag {
        Flag0 = 1 << 0,
        Flag1 = 1 << 1,
        Flag2 = 1 << 2,
        Flag3 = 1 << 3,
        Flag4 = 1 << 4,
        Flag5 = 1 << 5,
        Flag6 = 1 << 6,
        Flag7 = 1 << 7,
    }
}

/// A sprite on the 16x16 sprite sheet (`0..=255`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteId(pub u8);

/// A sound effect slot (`0..=63`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxId(pub u8);

/// A music pattern (`0..=63`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicId(pub u8);

/// Game state and input, available during `update`.
///
/// Zero-sized handle over the host ABI; it exists so the borrow checker
/// can tell "update powers" apart from "draw powers".
pub struct Context {
    pub(crate) _private: (),
}

impl Context {
    /// Is a button currently held down?
    pub fn is_button_down(&self, b: Button) -> bool {
        unsafe { ffi::is_button_down(button_index(b)) != 0 }
    }

    /// Alias for [`Context::is_button_down`].
    pub fn btn(&self, b: Button) -> bool {
        self.is_button_down(b)
    }

    /// Was a button just pressed? Repeats after a short delay while held.
    pub fn is_button_pressed(&self, b: Button) -> bool {
        unsafe { ffi::is_button_pressed(button_index(b)) != 0 }
    }

    /// Alias for [`Context::is_button_pressed`].
    pub fn btnp(&self, b: Button) -> bool {
        self.is_button_pressed(b)
    }

    /// Every button currently held down, as a set.
    pub fn buttons_down(&self) -> BitFlags<Button> {
        BitFlags::from_bits(unsafe { ffi::buttons_down() } as u8)
            .expect("buttons_down returned an unknown button bit (rico8 host/SDK ABI mismatch)")
    }

    /// Every button that fired this frame (with repeat), as a set.
    pub fn buttons_pressed(&self) -> BitFlags<Button> {
        BitFlags::from_bits(unsafe { ffi::buttons_pressed() } as u8)
            .expect("buttons_pressed returned an unknown button bit (rico8 host/SDK ABI mismatch)")
    }

    /// The sprite number of a map tile (`SpriteId(0)` = empty).
    pub fn map_tile(&self, x: i32, y: i32) -> SpriteId {
        SpriteId(unsafe { ffi::map_tile(x, y) } as u8)
    }

    /// Alias for [`Context::map_tile`].
    pub fn mget(&self, x: i32, y: i32) -> SpriteId {
        self.map_tile(x, y)
    }

    /// Write a map tile. Changes live in console RAM and are discarded on
    /// reload, like any self-respecting cartridge.
    pub fn set_map_tile(&mut self, x: i32, y: i32, sprite: SpriteId) {
        unsafe { ffi::set_map_tile(x, y, sprite.0 as u32) }
    }

    /// Alias for [`Context::set_map_tile`].
    pub fn mset(&mut self, x: i32, y: i32, sprite: SpriteId) {
        self.set_map_tile(x, y, sprite)
    }

    /// Read a pixel from the sprite sheet (out of bounds reads color 0).
    pub fn sprite_pixel(&self, x: i32, y: i32) -> Color {
        Color::from_index(unsafe { ffi::sprite_pixel(x, y) } as u8)
    }

    /// Alias for [`Context::sprite_pixel`].
    pub fn sget(&self, x: i32, y: i32) -> Color {
        self.sprite_pixel(x, y)
    }

    /// Write a pixel on the sprite sheet. RAM only, discarded on reload.
    pub fn set_sprite_pixel(&mut self, x: i32, y: i32, color: Color) {
        unsafe { ffi::set_sprite_pixel(x, y, color.0 as i32) }
    }

    /// Alias for [`Context::set_sprite_pixel`].
    pub fn sset(&mut self, x: i32, y: i32, color: Color) {
        self.set_sprite_pixel(x, y, color)
    }

    /// Every flag set on a sprite.
    pub fn sprite_flags(&self, sprite: SpriteId) -> BitFlags<SpriteFlag> {
        BitFlags::from_bits(unsafe { ffi::sprite_flags(sprite.0 as u32) } as u8).expect(
            "sprite_flags returned an unknown sprite-flag bit (rico8 host/SDK ABI mismatch)",
        )
    }

    /// Alias for [`Context::sprite_flags`].
    pub fn fget(&self, sprite: SpriteId) -> BitFlags<SpriteFlag> {
        self.sprite_flags(sprite)
    }

    /// Whether a sprite has a particular flag set.
    pub fn has_sprite_flag(&self, sprite: SpriteId, flag: SpriteFlag) -> bool {
        self.sprite_flags(sprite).contains(flag)
    }

    /// Overwrite a sprite's flags.
    pub fn set_sprite_flags(&mut self, sprite: SpriteId, flags: impl Into<BitFlags<SpriteFlag>>) {
        unsafe { ffi::set_sprite_flags(sprite.0 as u32, flags.into().bits() as u32) }
    }

    /// Alias for [`Context::set_sprite_flags`].
    pub fn fset(&mut self, sprite: SpriteId, flags: impl Into<BitFlags<SpriteFlag>>) {
        self.set_sprite_flags(sprite, flags)
    }

    /// Play a sound effect on a free channel.
    pub fn sfx(&mut self, s: SfxId) {
        unsafe { ffi::sfx(s.0 as i32, -1) }
    }

    /// Play a sound effect on a specific channel (`0..4`).
    pub fn sfx_on(&mut self, s: SfxId, channel: u8) {
        unsafe { ffi::sfx(s.0 as i32, channel as i32) }
    }

    /// Stop whatever is playing on a channel.
    pub fn sfx_stop(&mut self, channel: u8) {
        unsafe { ffi::sfx(-1, channel as i32) }
    }

    /// Begin a music-playback request for pattern `m`.
    ///
    /// Nothing plays until [`Music::play`]; set a fade-in or reserved channels
    /// on the returned [`Music`] first.
    pub fn music(&mut self, m: MusicId) -> Music {
        Music::new(m)
    }

    /// Seconds since the cart started, in `1/`[`FRAME_RATE`] steps (1/60 s
    /// by default).
    ///
    /// [`FRAME_RATE`]: Game::FRAME_RATE
    pub fn time(&self) -> f32 {
        unsafe { ffi::time() }
    }

    /// A uniformly random `f32` from `range`.
    ///
    /// Accepts any range syntax — exclusive (`a..b`), inclusive (`a..=b`), or open on
    /// either end. An open lower bound is `f32::MIN`, an open upper bound is `f32::MAX`;
    /// bounds may be negative. A reversed or empty range yields its lower bound.
    pub fn random<R>(&mut self, range: R) -> f32
    where
        R: RangeBounds<f32>,
    {
        let (lo, hi) = f32_bounds(range);
        sample_f32(lo, hi, unsafe { ffi::rnd() })
    }

    /// Scalar shorthand for `random(0.0..max)`.
    pub fn rnd(&mut self, max: f32) -> f32 {
        self.random(0.0..max)
    }

    /// A uniformly random `i32` from `range`.
    ///
    /// Accepts any range syntax — exclusive (`a..b`), inclusive (`a..=b`), or open on
    /// either end. An open lower bound is `i32::MIN`, an open upper bound is `i32::MAX`;
    /// bounds may be negative. A reversed or empty range yields its lower bound.
    pub fn random_integer<R>(&mut self, range: R) -> i32
    where
        R: RangeBounds<i32>,
    {
        let (lo, count) = i32_bounds(range);
        sample_i32(lo, count, unsafe { ffi::rnd() })
    }

    /// Scalar shorthand for `random_integer(0..max)`.
    pub fn rndi(&mut self, max: i32) -> i32 {
        self.random_integer(0..max)
    }

    /// Seed the random sequence for deterministic runs.
    pub fn seed_rng(&mut self, seed: u32) {
        unsafe { ffi::seed_rng(seed) }
    }

    /// Alias for [`Context::seed_rng`].
    pub fn srand(&mut self, seed: u32) {
        self.seed_rng(seed)
    }

    /// Log a line to the RICO-8 console (visible after Esc). For
    /// `format!`-style arguments, see [`logf!`](crate::logf).
    pub fn log(&mut self, msg: &str) {
        unsafe { ffi::log(msg.as_ptr(), msg.len() as u32) }
    }

    /// Fraction (`0.0`–`1.0`) of last frame's `update` CPU budget used.
    ///
    /// Reports the previous completed frame: mid-`update` the current call's
    /// cost isn't known yet. A value near `1.0` means the budget was nearly
    /// exhausted (a call that fully spends it traps before this can report).
    pub fn cpu_update(&self) -> f32 {
        unsafe { ffi::cpu_update() }
    }

    /// Fraction (`0.0`–`1.0`) of last frame's `draw` CPU budget used.
    pub fn cpu_draw(&self) -> f32 {
        unsafe { ffi::cpu_draw() }
    }

    /// The cart's committed-memory high-water — the highest its footprint
    /// (shadow-stack reserve, statics and heap together) has ever reached — as
    /// a fraction (`0.0`–`1.0`) of the 128 K cap. It never decreases (wasm never
    /// returns pages) and counts freed-but-stranded memory, so it tracks real
    /// pressure closely. It is still not an exact OOM line: the allocator keeps
    /// a small reserve above the last allocation, so the cap can be reached
    /// while this reads a little under 100%.
    pub fn mem(&self) -> f32 {
        crate::memstat::used_fraction()
    }

    /// Actual measured frames per second. Equals the target rate (see
    /// [`Game::FRAME_RATE`]) until the host has measured a real one.
    pub fn fps(&self) -> f32 {
        unsafe { ffi::fps() }
    }
}

/// The screen, available during `draw`.
pub struct Graphics {
    pub(crate) _private: (),
}

impl Graphics {
    /// Fill the screen with a color.
    pub fn clear(&mut self, color: Color) {
        unsafe { ffi::clear(color.0 as i32) }
    }

    /// Alias for [`Graphics::clear`], for fingers that type `cls`.
    pub fn cls(&mut self, color: Color) {
        self.clear(color)
    }

    /// Offset all subsequent draws by `(-x, -y)`.
    pub fn camera(&mut self, x: i32, y: i32) {
        unsafe { ffi::camera(x, y) }
    }

    /// Restrict drawing to a rectangle in screen space. Errors on a zero/negative
    /// size.
    pub fn clip(&mut self, x: i32, y: i32, w: impl Dim, h: impl Dim) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe { ffi::clip(x, y, w.get() as i32, h.get() as i32) };
        Ok(())
    }

    /// Remove the clip rectangle.
    pub fn clip_reset(&mut self) {
        unsafe { ffi::clip(0, 0, SCREEN_WIDTH as i32, SCREEN_HEIGHT as i32) }
    }

    /// Make a palette color transparent (or opaque) for sprite draws.
    pub fn set_transparent_color(&mut self, color: Color, transparent: bool) {
        unsafe { ffi::set_transparent_color(color.0 as i32, transparent as i32) }
    }

    /// Alias for [`Graphics::set_transparent_color`].
    pub fn palt(&mut self, color: Color, transparent: bool) {
        self.set_transparent_color(color, transparent)
    }

    /// Reset sprite transparency to the default (only color 0 transparent).
    pub fn reset_transparency(&mut self) {
        unsafe { ffi::reset_transparency() }
    }

    /// Remap a draw color: later draws of `from` are written as `to`.
    pub fn remap_color(&mut self, from: Color, to: Color) {
        unsafe { ffi::remap_color(from.0 as i32, to.0 as i32, 0) }
    }

    /// Alias for [`Graphics::remap_color`].
    pub fn pal(&mut self, from: Color, to: Color) {
        self.remap_color(from, to)
    }

    /// Remap a display color: `from` is shown as `to` across the whole screen.
    pub fn remap_display_color(&mut self, from: Color, to: Color) {
        unsafe { ffi::remap_color(from.0 as i32, to.0 as i32, 1) }
    }

    /// Alias for [`Graphics::remap_display_color`].
    pub fn pal_display(&mut self, from: Color, to: Color) {
        self.remap_display_color(from, to)
    }

    /// Reset both the draw and display palettes to identity.
    pub fn reset_palette(&mut self) {
        unsafe { ffi::reset_palette() }
    }

    /// Set one pixel.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        unsafe { ffi::set_pixel(x, y, color.0 as i32) }
    }

    /// Alias for [`Graphics::set_pixel`].
    pub fn pset(&mut self, x: i32, y: i32, color: Color) {
        self.set_pixel(x, y, color)
    }

    /// Read one pixel (screen space; out of bounds reads 0).
    pub fn pixel(&self, x: i32, y: i32) -> Color {
        Color::from_index(unsafe { ffi::pixel(x, y) } as u8)
    }

    /// Alias for [`Graphics::pixel`].
    pub fn pget(&self, x: i32, y: i32) -> Color {
        self.pixel(x, y)
    }

    /// Line between two points, inclusive.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        unsafe { ffi::line(x0, y0, x1, y1, color.0 as i32) }
    }

    /// Rectangle outline at `(x, y)` with size `w x h`. Errors on a zero/negative
    /// size.
    pub fn rect(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe {
            ffi::rect(
                x,
                y,
                x + w.get() as i32 - 1,
                y + h.get() as i32 - 1,
                color.0 as i32,
            )
        };
        Ok(())
    }

    /// Filled rectangle at `(x, y)` with size `w x h`. Errors on a zero/negative
    /// size.
    pub fn rect_fill(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe {
            ffi::rect_fill(
                x,
                y,
                x + w.get() as i32 - 1,
                y + h.get() as i32 - 1,
                color.0 as i32,
            )
        };
        Ok(())
    }

    /// Alias for [`Graphics::rect_fill`].
    pub fn rectfill(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        self.rect_fill(x, y, w, h, color)
    }

    /// Circle outline. `r = 0` draws a single pixel.
    pub fn circle(&mut self, x: i32, y: i32, r: u32, color: Color) {
        unsafe { ffi::circle(x, y, r as i32, color.0 as i32) }
    }

    /// Alias for [`Graphics::circle`].
    pub fn circ(&mut self, x: i32, y: i32, r: u32, color: Color) {
        self.circle(x, y, r, color)
    }

    /// Filled circle. `r = 0` draws a single pixel.
    pub fn circle_fill(&mut self, x: i32, y: i32, r: u32, color: Color) {
        unsafe { ffi::circle_fill(x, y, r as i32, color.0 as i32) }
    }

    /// Alias for [`Graphics::circle_fill`].
    pub fn circfill(&mut self, x: i32, y: i32, r: u32, color: Color) {
        self.circle_fill(x, y, r, color)
    }

    /// Ellipse outline inside the `(x, y, w, h)` box. Errors on a zero/negative
    /// size.
    pub fn ellipse(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe {
            ffi::ellipse(
                x,
                y,
                x + w.get() as i32 - 1,
                y + h.get() as i32 - 1,
                color.0 as i32,
            )
        };
        Ok(())
    }

    /// Alias for [`Graphics::ellipse`].
    pub fn oval(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        self.ellipse(x, y, w, h, color)
    }

    /// Filled ellipse inside the `(x, y, w, h)` box. Errors on a zero/negative
    /// size.
    pub fn ellipse_fill(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe {
            ffi::ellipse_fill(
                x,
                y,
                x + w.get() as i32 - 1,
                y + h.get() as i32 - 1,
                color.0 as i32,
            )
        };
        Ok(())
    }

    /// Alias for [`Graphics::ellipse_fill`].
    pub fn ovalfill(
        &mut self,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        color: Color,
    ) -> Result<(), ZeroSize> {
        self.ellipse_fill(x, y, w, h, color)
    }

    /// Set a two-color fill pattern for the filled shapes. Pattern-1 pixels use
    /// `secondary`. `pattern` is a 4x4 bitmask (bit 15 = top-left); 0 is solid.
    pub fn set_fill_pattern(&mut self, pattern: u16, secondary: Color) {
        unsafe { ffi::set_fill_pattern(pattern as i32, secondary.0 as i32, 0) }
    }

    /// Alias for [`Graphics::set_fill_pattern`] with a black secondary color.
    pub fn fillp(&mut self, pattern: u16) {
        self.set_fill_pattern(pattern, Color::BLACK)
    }

    /// Set a fill pattern whose pattern-1 pixels are left transparent.
    pub fn set_fill_pattern_transparent(&mut self, pattern: u16) {
        unsafe { ffi::set_fill_pattern(pattern as i32, 0, 1) }
    }

    /// Fill solid again (the default).
    pub fn clear_fill_pattern(&mut self) {
        unsafe { ffi::set_fill_pattern(0, 0, 0) }
    }

    /// Print text with the built-in 4x6 font. Returns the x position (as `i32`)
    /// after the last glyph. For `format!`-style arguments, see
    /// [`printf!`](crate::printf).
    pub fn print(&mut self, text: &str, x: i32, y: i32, color: Color) -> i32 {
        unsafe { ffi::print(text.as_ptr(), text.len() as u32, x, y, color.0 as i32) }
    }

    /// Set the persistent pen color used by [`Graphics::print_pen`].
    pub fn set_pen_color(&mut self, color: Color) {
        unsafe { ffi::set_pen_color(color.0 as i32) }
    }

    /// Alias for [`Graphics::set_pen_color`].
    pub fn color(&mut self, color: Color) {
        self.set_pen_color(color)
    }

    /// Set the persistent text cursor used by [`Graphics::print_pen`].
    pub fn set_cursor(&mut self, x: i32, y: i32) {
        unsafe { ffi::set_cursor(x, y) }
    }

    /// Alias for [`Graphics::set_cursor`].
    pub fn cursor(&mut self, x: i32, y: i32) {
        self.set_cursor(x, y)
    }

    /// Print at the cursor in the pen color, advancing the cursor one line.
    /// Returns the x position (as `i32`) after the last glyph. The cursor
    /// advances by a single line regardless of any newlines embedded in `text`.
    pub fn print_pen(&mut self, text: &str) -> i32 {
        unsafe { ffi::print_pen(text.as_ptr(), text.len() as u32) }
    }

    /// Draw a sprite at `(x, y)`. Color 0 is transparent.
    pub fn sprite(&mut self, sprite: SpriteId, x: i32, y: i32) {
        // A whole 8x8 cell, in pixels.
        unsafe { ffi::sprite(sprite.0 as u32, x, y, 8, 8, 0, 0) }
    }

    /// Alias for [`Graphics::sprite`].
    pub fn spr(&mut self, sprite: SpriteId, x: i32, y: i32) {
        self.sprite(sprite, x, y)
    }

    /// Draw a `w x h`-pixel sprite block, optionally flipped. `w`/`h` are in pixels:
    /// `8` is one cell, `4` a half-cell slice. Errors on a zero/negative size.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_ext(
        &mut self,
        sprite: SpriteId,
        x: i32,
        y: i32,
        w: impl Dim,
        h: impl Dim,
        flip_x: bool,
        flip_y: bool,
    ) -> Result<(), ZeroSize> {
        let w = w.to_nonzero().ok_or(ZeroSize)?;
        let h = h.to_nonzero().ok_or(ZeroSize)?;
        unsafe {
            ffi::sprite(
                sprite.0 as u32,
                x,
                y,
                w.get() as i32,
                h.get() as i32,
                flip_x as i32,
                flip_y as i32,
            )
        };
        Ok(())
    }

    /// Draw a sheet rectangle `(sx,sy,sw,sh)` stretched into a screen rectangle
    /// `(dx,dy,dw,dh)`. Honors transparency and the draw palette.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_stretch(
        &mut self,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        flip_x: bool,
        flip_y: bool,
    ) {
        unsafe { ffi::sprite_stretch(sx, sy, sw, sh, dx, dy, dw, dh, flip_x as i32, flip_y as i32) }
    }

    /// Alias for [`Graphics::sprite_stretch`].
    #[allow(clippy::too_many_arguments)]
    pub fn sspr(
        &mut self,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        flip_x: bool,
        flip_y: bool,
    ) {
        self.sprite_stretch(sx, sy, sw, sh, dx, dy, dw, dh, flip_x, flip_y)
    }

    /// Draw a region of the map: `cel_w x cel_h` tiles starting at tile
    /// `(cel_x, cel_y)`, at screen position `(sx, sy)`. With an empty
    /// `layers` set every tile is drawn; otherwise only tiles whose sprite
    /// flags intersect `layers`.
    #[allow(clippy::too_many_arguments)]
    pub fn map(
        &mut self,
        cel_x: i32,
        cel_y: i32,
        sx: f32,
        sy: f32,
        cel_w: i32,
        cel_h: i32,
        layers: impl Into<BitFlags<SpriteFlag>>,
    ) {
        let layers = layers.into().bits() as u32;
        unsafe { ffi::map(cel_x, cel_y, sx, sy, cel_w, cel_h, layers) }
    }
}

/// Implement this for your game state, then hand it to [`game!`].
pub trait Game {
    /// The logical frame rate. Set this to [`FrameRate::Fps30`] to run
    /// `update` and `draw` at 30 fps instead of the default 60.
    const FRAME_RATE: FrameRate = FrameRate::Fps60;
    /// Called [`FRAME_RATE`](Game::FRAME_RATE) times per second. Read
    /// input, move the world.
    fn update(&mut self, ctx: &mut Context);
    /// Called after `update`. Draw the world.
    fn draw(&self, gfx: &mut Graphics);
}

/// Declare your game's entry point.
///
/// The common form takes a struct literal that builds the initial state:
///
/// ```ignore
/// rico8::game!(MyGame { x: 64, y: 64 });
/// ```
///
/// Any other constructor works with the `Type = expr` form, and a type
/// implementing [`Default`] needs no initializer:
///
/// ```ignore
/// rico8::game!(MyGame = MyGame::new());
/// rico8::game!(MyGame);
/// ```
#[macro_export]
macro_rules! game {
    ($game:ty = $init:expr) => {
        static GAME: $crate::__internal::Slot<$game> = $crate::__internal::Slot::new();

        #[no_mangle]
        pub extern "C" fn rico8_init() {
            GAME.init(|| $init);
        }

        #[no_mangle]
        pub extern "C" fn rico8_fps() -> u32 {
            GAME.fps()
        }

        #[no_mangle]
        pub extern "C" fn rico8_mem_used() -> u32 {
            $crate::memstat::used_bytes() as u32
        }

        #[no_mangle]
        pub extern "C" fn rico8_update() {
            GAME.update();
        }

        #[no_mangle]
        pub extern "C" fn rico8_draw() {
            GAME.draw();
        }
    };
    ($game:ident { $($field:tt)* }) => {
        $crate::game!($game = $game { $($field)* });
    };
    ($game:ident) => {
        $crate::game!($game = <$game as ::core::default::Default>::default());
    };
}

/// Print formatted text to the screen — like [`Graphics::print`], but with
/// `format!`-style arguments. Returns the cursor x (as `i32`) after the last
/// glyph.
///
/// The text is formatted into a fixed stack buffer: no allocator, no
/// dependencies. The default buffer holds one screen line (32 characters); a
/// leading integer-literal `N;` sizes it yourself. Overflow is truncated.
///
/// ```ignore
/// use rico8::*;
///
/// fn draw(&self, gfx: &mut Graphics) {
///     rico8::printf!(gfx, 2, 2, Color::YELLOW, "coins {}", self.coins);
///     // A longer line needs a bigger buffer:
///     rico8::printf!(256; gfx, 0, 8, Color::WHITE, "pos {} {}", self.x, self.y);
/// }
/// ```
#[macro_export]
macro_rules! printf {
    ($cap:literal; $gfx:expr, $x:expr, $y:expr, $color:expr, $($arg:tt)*) => {{
        let __buf = $crate::__internal::format_args_to_buf::<$cap>(::core::format_args!($($arg)*));
        $gfx.print(__buf.as_str(), $x, $y, $color)
    }};
    ($gfx:expr, $x:expr, $y:expr, $color:expr, $($arg:tt)*) => {{
        let __buf = $crate::__internal::format_args_to_buf::<{ $crate::__internal::LINE_CAP }>(
            ::core::format_args!($($arg)*),
        );
        $gfx.print(__buf.as_str(), $x, $y, $color)
    }};
}

/// Log formatted text to the debug console — like [`Context::log`], but with
/// `format!`-style arguments.
///
/// Same fixed-buffer behavior as [`printf!`]: one screen line by default, an
/// optional leading integer-literal `N;` for more, overflow truncated.
///
/// ```ignore
/// use rico8::*;
///
/// fn update(&mut self, ctx: &mut Context) {
///     rico8::logf!(ctx, "frame {} pos ({},{})", self.frame, self.x, self.y);
/// }
/// ```
#[macro_export]
macro_rules! logf {
    ($cap:literal; $ctx:expr, $($arg:tt)*) => {{
        let __buf = $crate::__internal::format_args_to_buf::<$cap>(::core::format_args!($($arg)*));
        $ctx.log(__buf.as_str());
    }};
    ($ctx:expr, $($arg:tt)*) => {{
        let __buf = $crate::__internal::format_args_to_buf::<{ $crate::__internal::LINE_CAP }>(
            ::core::format_args!($($arg)*),
        );
        $ctx.log(__buf.as_str());
    }};
}

/// The `(lo, hi)` of a float range; an open lower end is `f32::MIN`, an open upper
/// end is `f32::MAX`.
fn f32_bounds<R>(range: R) -> (f32, f32)
where
    R: RangeBounds<f32>,
{
    let lo = match range.start_bound() {
        Bound::Included(&v) | Bound::Excluded(&v) => v,
        Bound::Unbounded => f32::MIN,
    };
    let hi = match range.end_bound() {
        Bound::Included(&v) | Bound::Excluded(&v) => v,
        Bound::Unbounded => f32::MAX,
    };
    (lo, hi)
}

/// The `(lo, count)` of an integer range — the lower bound and the number of distinct
/// values, in `i64` so the full `i32` span doesn't overflow. An open lower end is
/// `i32::MIN`, an open upper end is `i32::MAX` (inclusive). `count <= 0` is a reversed
/// or empty range.
fn i32_bounds<R>(range: R) -> (i64, i64)
where
    R: RangeBounds<i32>,
{
    let lo = match range.start_bound() {
        Bound::Included(&v) => v as i64,
        Bound::Excluded(&v) => v as i64 + 1,
        Bound::Unbounded => i32::MIN as i64,
    };
    let hi = match range.end_bound() {
        Bound::Included(&v) => v as i64,
        Bound::Excluded(&v) => v as i64 - 1,
        Bound::Unbounded => i32::MAX as i64,
    };
    (lo, hi - lo + 1)
}

/// Map a raw `[0, 1)` draw onto `[lo, hi)`. A reversed or empty span yields `lo`.
/// Computed in `f64` so a span wider than `f32::MAX` (an open-ended range) stays finite.
fn sample_f32(lo: f32, hi: f32, raw: f32) -> f32 {
    let width = hi as f64 - lo as f64;
    if width <= 0.0 {
        lo
    } else {
        (lo as f64 + raw as f64 * width) as f32
    }
}

/// Map a raw `[0, 1)` draw onto `count` integers starting at `lo`. `count <= 0` (a
/// reversed or empty range) yields `lo`. Arithmetic is `i64` so the full `i32` span
/// (count up to 2^32) doesn't overflow.
fn sample_i32(lo: i64, count: i64, raw: f32) -> i32 {
    if count <= 0 {
        return lo as i32;
    }
    let idx = ((raw as f64 * count as f64) as i64).min(count - 1);
    (lo + idx) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_index_matches_abi_order() {
        assert_eq!(button_index(Button::Left), 0);
        assert_eq!(button_index(Button::Right), 1);
        assert_eq!(button_index(Button::Up), 2);
        assert_eq!(button_index(Button::Down), 3);
        assert_eq!(button_index(Button::O), 4);
        assert_eq!(button_index(Button::X), 5);
    }

    #[test]
    fn button_aliases_match_primaries() {
        let ctx = Context { _private: () };
        for b in [
            Button::Left,
            Button::Right,
            Button::Up,
            Button::Down,
            Button::O,
            Button::X,
        ] {
            assert_eq!(ctx.btn(b), ctx.is_button_down(b));
            assert_eq!(ctx.btnp(b), ctx.is_button_pressed(b));
        }
        // Native stubs report nothing held/pressed.
        assert!(ctx.buttons_down().is_empty());
        assert!(ctx.buttons_pressed().is_empty());
    }

    #[test]
    fn sprite_flag_and_tile_helpers() {
        let ctx = Context { _private: () };
        // Native stubs: fget -> 0 (no flags), mget -> 0.
        assert!(ctx.sprite_flags(SpriteId(1)).is_empty());
        assert_eq!(ctx.sprite_flags(SpriteId(1)), ctx.fget(SpriteId(1)));
        assert!(!ctx.has_sprite_flag(SpriteId(1), SpriteFlag::Flag0));
        assert_eq!(ctx.map_tile(0, 0), SpriteId(0));
        assert_eq!(ctx.map_tile(0, 0), ctx.mget(0, 0));
    }

    #[test]
    fn button_mask_round_trips() {
        // bits 0, 3, 5 == Left, Down, X
        let mask = BitFlags::<Button>::from_bits(0b10_1001).unwrap();
        assert!(mask.contains(Button::Left));
        assert!(mask.contains(Button::Down));
        assert!(mask.contains(Button::X));
        assert!(!mask.contains(Button::Right));
    }

    #[test]
    fn diagonal_button_constants() {
        assert_eq!(Button::UP_LEFT, Button::Left | Button::Up);
        assert!(Button::UP_LEFT.contains(Button::Left));
        assert!(Button::UP_LEFT.contains(Button::Up));
        assert!(!Button::UP_LEFT.contains(Button::Right));
        // The four diagonals are distinct sets.
        for (a, b) in [
            (Button::UP_LEFT, Button::UP_RIGHT),
            (Button::UP_LEFT, Button::DOWN_LEFT),
            (Button::DOWN_RIGHT, Button::UP_LEFT),
        ] {
            assert_ne!(a, b);
        }
    }

    #[test]
    fn map_accepts_flag_set_forms() {
        let mut gfx = Graphics { _private: () };
        gfx.map(0, 0, 0.0, 0.0, 16, 16, BitFlags::empty());
        gfx.map(0, 0, 0.0, 0.0, 16, 16, SpriteFlag::Flag0);
        gfx.map(
            0,
            0,
            0.0,
            0.0,
            16,
            16,
            SpriteFlag::Flag0 | SpriteFlag::Flag3,
        );
    }

    #[test]
    fn graphics_aliases_match_primaries() {
        let mut gfx = Graphics { _private: () };
        assert_eq!(gfx.pixel(1, 1), gfx.pget(1, 1));
        // Drawing aliases forward to primaries (no-op under native stubs).
        gfx.set_pixel(0, 0, Color::RED);
        gfx.pset(0, 0, Color::RED);
        gfx.circle(0, 0, 4, Color::RED);
        gfx.circ(0, 0, 4, Color::RED);
        gfx.circle_fill(0, 0, 4, Color::RED);
        gfx.circfill(0, 0, 4, Color::RED);
        gfx.rect_fill(0, 0, 4, 4, Color::RED).unwrap();
        gfx.rectfill(0, 0, 4, 4, Color::RED).unwrap();
        gfx.sprite(SpriteId(0), 0, 0);
        gfx.spr(SpriteId(0), 0, 0);
    }

    #[test]
    fn printf_formats_and_returns_cursor() {
        let mut gfx = Graphics { _private: () };
        // The native ffi::print stub returns 0; this exercises macro
        // expansion and the i32 return type. String content is covered by the
        // fmt::tests, since the stub does not capture the text.
        let cursor: i32 = printf!(gfx, 0, 0, Color::WHITE, "n={}", 3);
        assert_eq!(cursor, 0);
        // Capacity-override arm, multi-arg, and a no-arg literal all expand.
        let _: i32 = printf!(64; gfx, 0, 0, Color::WHITE, "{}-{}", 1, 2);
        let _: i32 = printf!(gfx, 0, 0, Color::WHITE, "literal");
    }

    #[test]
    fn logf_formats_and_runs() {
        let mut ctx = Context { _private: () };
        logf!(ctx, "frame {}", 9);
        logf!(128; ctx, "{}-{}", 1, 2);
        logf!(ctx, "literal");
    }

    #[test]
    fn context_sheet_and_rng_aliases() {
        let mut ctx = Context { _private: () };
        ctx.seed_rng(1);
        ctx.srand(1);
        ctx.set_sprite_pixel(0, 0, Color::RED);
        ctx.sset(0, 0, Color::RED);
        // Native stubs read 0.
        assert_eq!(ctx.sprite_pixel(0, 0), Color::from_index(0));
        assert_eq!(ctx.sprite_pixel(0, 0), ctx.sget(0, 0));
    }

    #[test]
    fn f32_bounds_fills_open_ends_with_extremes() {
        assert_eq!(f32_bounds(2.0..5.0), (2.0, 5.0));
        assert_eq!(f32_bounds(2.0..=5.0), (2.0, 5.0));
        assert_eq!(f32_bounds(-5.0..-1.0), (-5.0, -1.0));
        assert_eq!(f32_bounds(..44.0), (f32::MIN, 44.0));
        assert_eq!(f32_bounds(0.0..), (0.0, f32::MAX));
        assert_eq!(f32_bounds(..), (f32::MIN, f32::MAX));
    }

    #[test]
    fn i32_bounds_counts_and_fills_open_ends() {
        assert_eq!(i32_bounds(0..10), (0, 10));
        assert_eq!(i32_bounds(1..=6), (1, 6));
        assert_eq!(i32_bounds(-10..0), (-10, 10));
        assert_eq!(i32_bounds(-5..=5), (-5, 11));
        // Open ends use i32::MIN / i32::MAX (upper inclusive).
        assert_eq!(i32_bounds(5..), (5, i32::MAX as i64 - 5 + 1));
        assert_eq!(i32_bounds(..10), (i32::MIN as i64, 10 - i32::MIN as i64));
        assert_eq!(
            i32_bounds(..),
            (i32::MIN as i64, i32::MAX as i64 - i32::MIN as i64 + 1)
        );
        // Reversed -> non-positive count (built at runtime to avoid
        // clippy::reversed_empty_ranges); equal-bound empty is fine as a literal.
        let (a, b): (i32, i32) = (5, 2);
        assert_eq!(i32_bounds(a..b), (5, -3));
        assert_eq!(i32_bounds(5..5), (5, 0));
    }

    #[test]
    fn sample_f32_maps_guards_and_stays_finite() {
        assert_eq!(sample_f32(0.0, 10.0, 0.0), 0.0);
        assert!((sample_f32(0.0, 10.0, 0.5) - 5.0).abs() < 1e-5);
        assert_eq!(sample_f32(-5.0, 5.0, 0.0), -5.0);
        assert!(sample_f32(-5.0, 5.0, 0.5).abs() < 1e-5);
        // Reversed / empty -> lo.
        assert_eq!(sample_f32(5.0, 2.0, 0.5), 5.0);
        assert_eq!(sample_f32(3.0, 3.0, 0.5), 3.0);
        // Full f32 span stays finite (f64 intermediate); midpoint ~ 0.
        let mid = sample_f32(f32::MIN, f32::MAX, 0.5);
        assert!(mid.is_finite());
        assert!(mid.abs() < 1e30);
    }

    #[test]
    fn sample_i32_maps_clamps_and_guards() {
        assert_eq!(sample_i32(0, 10, 0.0), 0);
        assert_eq!(sample_i32(0, 10, 0.999_999), 9);
        assert_eq!(sample_i32(0, 10, 0.55), 5);
        // Inclusive top reachable: lo=1, count=6 -> 6.
        assert_eq!(sample_i32(1, 6, 0.999_999), 6);
        // Negative bounds.
        assert_eq!(sample_i32(-10, 10, 0.999_999), -1);
        // Reversed / empty -> lo.
        assert_eq!(sample_i32(5, -3, 0.5), 5);
        assert_eq!(sample_i32(5, 0, 0.5), 5);
        // Full i32 span (count = 2^32) doesn't overflow.
        let full = i32::MAX as i64 - i32::MIN as i64 + 1;
        assert_eq!(sample_i32(i32::MIN as i64, full, 0.0), i32::MIN);
        assert_eq!(sample_i32(i32::MIN as i64, full, 0.5), 0);
    }

    #[test]
    fn context_random_methods_forward() {
        let mut ctx = Context { _private: () };
        // Native ffi::rnd() stub returns 0.0, so each call yields the lower bound.
        assert_eq!(ctx.random(2.0..5.0), 2.0);
        assert_eq!(ctx.random(2.0..=5.0), 2.0);
        assert_eq!(ctx.random(0.0..), 0.0);
        assert_eq!(ctx.random(..44.0), f32::MIN);
        assert_eq!(ctx.random_integer(3..9), 3);
        assert_eq!(ctx.random_integer(3..=9), 3);
        assert_eq!(ctx.random_integer(5..), 5);
        assert_eq!(ctx.random_integer(..10), i32::MIN);
        assert_eq!(ctx.rnd(5.0), 0.0);
        assert_eq!(ctx.rndi(10), 0);
    }

    #[test]
    fn context_exposes_resource_stats() {
        // On native targets the ffi stubs return 0.0; this asserts the safe
        // wrappers compile and forward to them.
        let ctx = Context { _private: () };
        assert_eq!(ctx.cpu_update(), 0.0);
        assert_eq!(ctx.cpu_draw(), 0.0);
        assert_eq!(ctx.mem(), 0.0);
        assert_eq!(ctx.fps(), 0.0);
    }

    #[test]
    fn graphics_parity_aliases_compile_and_forward() {
        let mut gfx = Graphics { _private: () };
        gfx.set_transparent_color(Color::BLACK, true);
        gfx.palt(Color::BLACK, true);
        gfx.reset_transparency();
        gfx.remap_color(Color::RED, Color::BLUE);
        gfx.pal(Color::RED, Color::BLUE);
        gfx.remap_display_color(Color::RED, Color::BLUE);
        gfx.pal_display(Color::RED, Color::BLUE);
        gfx.reset_palette();
        gfx.sprite_stretch(0, 0, 8, 8, 0.0, 0.0, 16.0, 16.0, false, false);
        gfx.sspr(0, 0, 8, 8, 0.0, 0.0, 16.0, 16.0, true, true);
        gfx.ellipse(0, 0, 8, 6, Color::WHITE).unwrap();
        gfx.oval(0, 0, 8, 6, Color::WHITE).unwrap();
        gfx.ellipse_fill(0, 0, 8, 6, Color::WHITE).unwrap();
        gfx.ovalfill(0, 0, 8, 6, Color::WHITE).unwrap();
        gfx.set_fill_pattern(0b1010, Color::RED);
        gfx.fillp(0b1010);
        gfx.set_fill_pattern_transparent(0b1010);
        gfx.clear_fill_pattern();
        gfx.set_pen_color(Color::YELLOW);
        gfx.color(Color::YELLOW);
        gfx.set_cursor(4, 4);
        gfx.cursor(4, 4);
        let cursor: i32 = gfx.print_pen("hi");
        assert_eq!(cursor, 0, "native print_pen stub returns 0");
    }

    #[test]
    fn fallible_rect_and_ellipse() {
        let mut gfx = Graphics { _private: () };
        // Positive sizes succeed.
        assert_eq!(gfx.rect(0, 0, 4, 4, Color::RED), Ok(()));
        assert_eq!(gfx.rect_fill(0, 0, 4, 4, Color::RED), Ok(()));
        assert_eq!(gfx.ellipse(0, 0, 8, 6, Color::WHITE), Ok(()));
        assert_eq!(gfx.ellipse_fill(0, 0, 8, 6, Color::WHITE), Ok(()));
        // Zero or negative sizes are a ZeroSize error (nothing drawn).
        assert_eq!(gfx.rect_fill(0, 0, 0, 4, Color::RED), Err(ZeroSize));
        assert_eq!(gfx.rect(0, 0, 4, -1, Color::RED), Err(ZeroSize));
        // A computed i32 size (the case core's TryInto can't take) compiles.
        let w = 10 - 4;
        assert_eq!(gfx.rect(0, 0, w, 4, Color::RED), Ok(()));
    }

    #[test]
    fn clip_is_fallible_and_reset_is_not() {
        let mut gfx = Graphics { _private: () };
        assert_eq!(gfx.clip(0, 0, 64, 64), Ok(()));
        assert_eq!(gfx.clip(0, 0, 0, 64), Err(ZeroSize));
        gfx.clip_reset(); // Infallible.
    }

    #[test]
    fn sprite_and_sprite_ext() {
        let mut gfx = Graphics { _private: () };
        gfx.sprite(SpriteId(0), 0, 0);
        gfx.spr(SpriteId(0), 0, 0);
        // Pixel dimensions: a full cell is 8, a half-cell slice is 4.
        assert_eq!(
            gfx.sprite_ext(SpriteId(0), 0, 0, 8, 8, false, false),
            Ok(())
        );
        assert_eq!(gfx.sprite_ext(SpriteId(0), 0, 0, 4, 8, true, false), Ok(()));
        assert_eq!(
            gfx.sprite_ext(SpriteId(0), 0, 0, 0, 8, false, false),
            Err(ZeroSize)
        );
    }
}
