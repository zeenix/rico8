//! Lifecycle glue between the `game!` macro exports and the game trait.

use crate::{Context, FrameRate, Graphics, Rico8Game};

/// Implementation details of the [`game!`](crate::game) macro. Not part
/// of the public API; do not call directly.
pub mod __internal {
    use super::*;
    use core::cell::UnsafeCell;

    struct GameSlot(UnsafeCell<Option<Box<dyn Rico8Game>>>);

    // Carts are single-threaded by construction: the host calls
    // rico8_init/update/draw sequentially on one wasm instance, and the
    // sandbox exposes no way to spawn threads.
    unsafe impl Sync for GameSlot {}

    static GAME: GameSlot = GameSlot(UnsafeCell::new(None));

    fn slot() -> &'static mut Option<Box<dyn Rico8Game>> {
        unsafe { &mut *GAME.0.get() }
    }

    pub fn init(make: impl FnOnce() -> Box<dyn Rico8Game>) {
        // Forward panics to the console so carts die with a readable
        // error screen instead of a silent trap.
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            unsafe { crate::ffi::panic(msg.as_ptr(), msg.len() as u32) };
        }));
        *slot() = Some(make());
    }

    /// The cart's selected frame rate, as a frames-per-second number. The
    /// host queries this once after `init` to set its update/draw cadence.
    pub fn fps() -> u32 {
        slot()
            .as_ref()
            .map(|game| game.frame_rate().fps())
            .unwrap_or(FrameRate::Fps30.fps())
    }

    pub fn update() {
        if let Some(game) = slot() {
            game.update(&mut Context { _private: () });
        }
    }

    pub fn draw() {
        if let Some(game) = slot() {
            game.draw(&mut Graphics { _private: () });
        }
    }
}
