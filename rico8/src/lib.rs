//! # rico8 — the RICO-8 fantasy console SDK
//!
//! Write tiny games in Rust, run them on a tiny console.
//!
//! ```no_run
//! use rico8::*;
//!
//! struct MyGame {
//!     x: f32,
//!     y: f32,
//! }
//!
//! impl Game for MyGame {
//!     fn update(&mut self, ctx: &mut Context) {
//!         if ctx.is_button_down(Button::Right) {
//!             self.x += 1.0;
//!         }
//!     }
//!
//!     fn draw(&self, gfx: &mut Graphics) {
//!         gfx.clear(Color::BLACK);
//!         gfx.rect_fill(self.x, self.y, 8.0, 8.0, Color::WHITE);
//!     }
//! }
//!
//! rico8::game!(MyGame { x: 64.0, y: 64.0 });
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

pub mod ffi;
mod flags;
mod fmt;
mod glue;
mod motion;

use crate::flags::bitflag_enum;
pub use crate::flags::{BitFlag, BitFlags, UnknownBits};
pub use glue::__internal;
pub use motion::Body;

/// The screen is 128x128 pixels.
pub const SCREEN_W: i32 = 128;
pub const SCREEN_H: i32 = 128;
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

    /// Start music at a pattern; it plays until it loops or stops itself.
    pub fn music(&mut self, m: MusicId) {
        unsafe { ffi::music(m.0 as i32) }
    }

    /// Stop the music.
    pub fn music_stop(&mut self) {
        unsafe { ffi::music(-1) }
    }

    /// Seconds since the cart started, in `1/`[`FRAME_RATE`] steps (1/60 s
    /// by default).
    ///
    /// [`FRAME_RATE`]: Game::FRAME_RATE
    pub fn time(&self) -> f32 {
        unsafe { ffi::time() }
    }

    /// A random float in `[0, max)`.
    pub fn rnd(&mut self, max: f32) -> f32 {
        unsafe { ffi::rnd() * max }
    }

    /// A random integer in `[0, max)` (0 when `max <= 0`).
    pub fn rndi(&mut self, max: i32) -> i32 {
        if max <= 0 {
            0
        } else {
            (self.rnd(max as f32) as i32).min(max - 1)
        }
    }

    /// Log a line to the RICO-8 console (visible after Esc). For
    /// `format!`-style arguments, see [`logf!`](crate::logf).
    pub fn log(&mut self, msg: &str) {
        unsafe { ffi::log(msg.as_ptr(), msg.len() as u32) }
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

    /// Offset all subsequent draws by `(-x, -y)`. Floored to a whole pixel.
    pub fn camera(&mut self, x: f32, y: f32) {
        unsafe { ffi::camera(x, y) }
    }

    /// Restrict drawing to a rectangle in screen space.
    pub fn clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        unsafe { ffi::clip(x, y, w, h) }
    }

    /// Remove the clip rectangle.
    pub fn clip_reset(&mut self) {
        unsafe { ffi::clip(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32) }
    }

    /// Set one pixel. The position is floored to a pixel.
    pub fn set_pixel(&mut self, x: f32, y: f32, color: Color) {
        unsafe { ffi::set_pixel(x, y, color.0 as i32) }
    }

    /// Alias for [`Graphics::set_pixel`].
    pub fn pset(&mut self, x: f32, y: f32, color: Color) {
        self.set_pixel(x, y, color)
    }

    /// Read one pixel (screen space; out of bounds reads 0).
    pub fn pixel(&self, x: f32, y: f32) -> Color {
        Color::from_index(unsafe { ffi::pixel(x, y) } as u8)
    }

    /// Alias for [`Graphics::pixel`].
    pub fn pget(&self, x: f32, y: f32) -> Color {
        self.pixel(x, y)
    }

    /// Line between two points, inclusive.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color) {
        unsafe { ffi::line(x0, y0, x1, y1, color.0 as i32) }
    }

    /// Rectangle outline at `(x, y)` with size `w x h`.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if w > 0.0 && h > 0.0 {
            unsafe { ffi::rect(x, y, x + w - 1.0, y + h - 1.0, color.0 as i32) }
        }
    }

    /// Filled rectangle at `(x, y)` with size `w x h`.
    pub fn rect_fill(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if w > 0.0 && h > 0.0 {
            unsafe { ffi::rect_fill(x, y, x + w - 1.0, y + h - 1.0, color.0 as i32) }
        }
    }

    /// Alias for [`Graphics::rect_fill`].
    pub fn rectfill(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.rect_fill(x, y, w, h, color)
    }

    /// Circle outline.
    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: Color) {
        unsafe { ffi::circle(x, y, r, color.0 as i32) }
    }

    /// Alias for [`Graphics::circle`].
    pub fn circ(&mut self, x: f32, y: f32, r: f32, color: Color) {
        self.circle(x, y, r, color)
    }

    /// Filled circle.
    pub fn circle_fill(&mut self, x: f32, y: f32, r: f32, color: Color) {
        unsafe { ffi::circle_fill(x, y, r, color.0 as i32) }
    }

    /// Alias for [`Graphics::circle_fill`].
    pub fn circfill(&mut self, x: f32, y: f32, r: f32, color: Color) {
        self.circle_fill(x, y, r, color)
    }

    /// Print text with the built-in 4x6 font. Returns the x position after
    /// the last glyph. For `format!`-style arguments, see
    /// [`printf!`](crate::printf).
    pub fn print(&mut self, text: &str, x: f32, y: f32, color: Color) -> f32 {
        unsafe { ffi::print(text.as_ptr(), text.len() as u32, x, y, color.0 as i32) }
    }

    /// Draw a sprite at `(x, y)`. Color 0 is transparent.
    pub fn sprite(&mut self, sprite: SpriteId, x: f32, y: f32) {
        unsafe { ffi::sprite(sprite.0 as u32, x, y, 1.0, 1.0, 0, 0) }
    }

    /// Alias for [`Graphics::sprite`].
    pub fn spr(&mut self, sprite: SpriteId, x: f32, y: f32) {
        self.sprite(sprite, x, y)
    }

    /// Draw a `w x h`-sprite block, optionally flipped. `w`/`h` are in sprite
    /// units and may be fractional: `w = 0.5` draws a 4-pixel-wide slice.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_ext(
        &mut self,
        sprite: SpriteId,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        flip_x: bool,
        flip_y: bool,
    ) {
        unsafe { ffi::sprite(sprite.0 as u32, x, y, w, h, flip_x as i32, flip_y as i32) }
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
/// `format!`-style arguments. Returns the x position after the last glyph.
///
/// The text is formatted into a fixed stack buffer: no allocator, no
/// dependencies. The default buffer holds one screen line (32 characters); a
/// leading integer-literal `N;` sizes it yourself. Overflow is truncated.
///
/// ```ignore
/// use rico8::*;
///
/// fn draw(&self, gfx: &mut Graphics) {
///     rico8::printf!(gfx, 2.0, 2.0, Color::YELLOW, "coins {}", self.coins);
///     // A longer line needs a bigger buffer:
///     rico8::printf!(256; gfx, 0.0, 8.0, Color::WHITE, "pos {} {}", self.x, self.y);
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
        assert_eq!(gfx.pixel(1.0, 1.0), gfx.pget(1.0, 1.0));
        // Drawing aliases forward to primaries (no-op under native stubs).
        gfx.set_pixel(0.0, 0.0, Color::RED);
        gfx.pset(0.0, 0.0, Color::RED);
        gfx.circle(0.0, 0.0, 4.0, Color::RED);
        gfx.circ(0.0, 0.0, 4.0, Color::RED);
        gfx.circle_fill(0.0, 0.0, 4.0, Color::RED);
        gfx.circfill(0.0, 0.0, 4.0, Color::RED);
        gfx.rect_fill(0.0, 0.0, 4.0, 4.0, Color::RED);
        gfx.rectfill(0.0, 0.0, 4.0, 4.0, Color::RED);
        gfx.sprite(SpriteId(0), 0.0, 0.0);
        gfx.spr(SpriteId(0), 0.0, 0.0);
        gfx.sprite_ext(SpriteId(0), 0.0, 0.0, 1.0, 1.0, false, false);
    }

    #[test]
    fn printf_formats_and_returns_cursor() {
        let mut gfx = Graphics { _private: () };
        // The native ffi::print stub returns 0.0; this exercises macro
        // expansion and the f32 return type. String content is covered by the
        // fmt::tests, since the stub does not capture the text.
        let cursor: f32 = printf!(gfx, 0.0, 0.0, Color::WHITE, "n={}", 3);
        assert_eq!(cursor, 0.0);
        // Capacity-override arm, multi-arg, and a no-arg literal all expand.
        let _: f32 = printf!(64; gfx, 0.0, 0.0, Color::WHITE, "{}-{}", 1, 2);
        let _: f32 = printf!(gfx, 0.0, 0.0, Color::WHITE, "literal");
    }

    #[test]
    fn logf_formats_and_runs() {
        let mut ctx = Context { _private: () };
        logf!(ctx, "frame {}", 9);
        logf!(128; ctx, "{}-{}", 1, 2);
        logf!(ctx, "literal");
    }
}
