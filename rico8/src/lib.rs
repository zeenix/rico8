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
//!         if ctx.btn(Button::Right) {
//!             self.x += 1;
//!         }
//!     }
//!
//!     fn draw(&self, gfx: &mut Graphics) {
//!         gfx.clear(Color::BLACK);
//!         gfx.rect_fill(self.x, self.y, 8, 8, Color::WHITE);
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

pub mod ffi;
mod glue;

pub use glue::__internal;

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

/// The six console buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Button {
    Left = 0,
    Right = 1,
    Up = 2,
    Down = 3,
    /// "O" action button — Z, C or N on the keyboard.
    O = 4,
    /// "X" action button — X, V or M on the keyboard.
    X = 5,
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
    /// Is a button held down?
    pub fn btn(&self, b: Button) -> bool {
        unsafe { ffi::btn(b as u32) != 0 }
    }

    /// Was a button just pressed? Repeats after a short delay while held.
    pub fn btnp(&self, b: Button) -> bool {
        unsafe { ffi::btnp(b as u32) != 0 }
    }

    /// Read a map tile (sprite number; 0 = empty).
    pub fn mget(&self, x: i32, y: i32) -> u8 {
        unsafe { ffi::mget(x, y) as u8 }
    }

    /// Write a map tile. Changes live in console RAM and are discarded on
    /// reload, like any self-respecting cartridge.
    pub fn mset(&mut self, x: i32, y: i32, sprite: SpriteId) {
        unsafe { ffi::mset(x, y, sprite.0 as u32) }
    }

    /// All eight flags of a sprite as a bitmask.
    pub fn fget(&self, sprite: SpriteId) -> u8 {
        unsafe { ffi::fget(sprite.0 as u32) as u8 }
    }

    /// True when the sprite has flag `flag` (`0..8`) set.
    pub fn fget_flag(&self, sprite: SpriteId, flag: u8) -> bool {
        self.fget(sprite) & (1 << (flag & 7)) != 0
    }

    /// Overwrite a sprite's flag bitmask.
    pub fn fset(&mut self, sprite: SpriteId, flags: u8) {
        unsafe { ffi::fset(sprite.0 as u32, flags as u32) }
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

    /// Log a line to the RICO-8 console (visible after Esc).
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
        unsafe { ffi::cls(color.0 as i32) }
    }

    /// Alias for [`Graphics::clear`], for fingers that type `cls`.
    pub fn cls(&mut self, color: Color) {
        self.clear(color)
    }

    /// Offset all subsequent draws by `(-x, -y)`.
    pub fn camera(&mut self, x: i32, y: i32) {
        unsafe { ffi::camera(x, y) }
    }

    /// Restrict drawing to a rectangle in screen space.
    pub fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe { ffi::clip(x, y, w, h) }
    }

    /// Remove the clip rectangle.
    pub fn clip_reset(&mut self) {
        unsafe { ffi::clip(0, 0, SCREEN_W, SCREEN_H) }
    }

    /// Set one pixel.
    pub fn pset(&mut self, x: i32, y: i32, color: Color) {
        unsafe { ffi::pset(x, y, color.0 as i32) }
    }

    /// Read one pixel (screen space; out of bounds reads 0).
    pub fn pget(&self, x: i32, y: i32) -> Color {
        Color::from_index(unsafe { ffi::pget(x, y) } as u8)
    }

    /// Line between two points, inclusive.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        unsafe { ffi::line(x0, y0, x1, y1, color.0 as i32) }
    }

    /// Rectangle outline at `(x, y)` with size `w x h`.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w > 0 && h > 0 {
            unsafe { ffi::rect(x, y, x + w - 1, y + h - 1, color.0 as i32) }
        }
    }

    /// Filled rectangle at `(x, y)` with size `w x h`.
    pub fn rect_fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w > 0 && h > 0 {
            unsafe { ffi::rectfill(x, y, x + w - 1, y + h - 1, color.0 as i32) }
        }
    }

    /// Circle outline.
    pub fn circ(&mut self, x: i32, y: i32, r: i32, color: Color) {
        unsafe { ffi::circ(x, y, r, color.0 as i32) }
    }

    /// Filled circle.
    pub fn circ_fill(&mut self, x: i32, y: i32, r: i32, color: Color) {
        unsafe { ffi::circfill(x, y, r, color.0 as i32) }
    }

    /// Print text with the built-in 4x6 font. Returns the x position after
    /// the last glyph.
    pub fn print(&mut self, text: &str, x: i32, y: i32, color: Color) -> i32 {
        unsafe { ffi::print(text.as_ptr(), text.len() as u32, x, y, color.0 as i32) }
    }

    /// Draw a sprite at `(x, y)`. Color 0 is transparent.
    pub fn spr(&mut self, sprite: SpriteId, x: i32, y: i32) {
        unsafe { ffi::spr(sprite.0 as u32, x, y, 1, 1, 0, 0) }
    }

    /// Draw a `w x h`-sprite block, optionally flipped.
    #[allow(clippy::too_many_arguments)]
    pub fn spr_ext(
        &mut self,
        sprite: SpriteId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        flip_x: bool,
        flip_y: bool,
    ) {
        unsafe { ffi::spr(sprite.0 as u32, x, y, w, h, flip_x as i32, flip_y as i32) }
    }

    /// Draw a region of the map: `cel_w x cel_h` tiles starting at tile
    /// `(cel_x, cel_y)`, at screen position `(sx, sy)`. When `layers` is
    /// nonzero only tiles with intersecting flags are drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn map(
        &mut self,
        cel_x: i32,
        cel_y: i32,
        sx: i32,
        sy: i32,
        cel_w: i32,
        cel_h: i32,
        layers: u8,
    ) {
        unsafe { ffi::map(cel_x, cel_y, sx, sy, cel_w, cel_h, layers as u32) }
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
