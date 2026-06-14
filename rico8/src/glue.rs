//! Lifecycle glue between the `game!` macro exports and the game trait.

use crate::{Context, Game, Graphics};

/// Implementation details of the [`game!`](crate::game) macro. Not part
/// of the public API; do not call directly.
pub mod __internal {
    use super::*;
    use core::cell::UnsafeCell;

    /// Typed storage for the one game instance a cart declares.
    ///
    /// The [`game!`](crate::game) macro creates a `static Slot<G>` for the
    /// cart's concrete game type, so the instance is stored by value with
    /// no heap allocation or trait object.
    pub struct Slot<G>(UnsafeCell<Option<G>>);

    // Carts are single-threaded by construction: the host calls
    // rico8_init/update/draw sequentially on one wasm instance, and the
    // sandbox exposes no way to spawn threads.
    unsafe impl<G> Sync for Slot<G> {}

    impl<G> Slot<G>
    where
        G: Game,
    {
        /// An empty slot, filled later by [`init`](Slot::init).
        ///
        /// Prefer this over [`Default`]: it is `const`, so the slot can
        /// initialize a `static`.
        pub const fn new() -> Self {
            Slot(UnsafeCell::new(None))
        }

        /// Construct and store the game instance.
        pub fn init(&self, make: impl FnOnce() -> G) {
            // Forward panics to the console so carts die with a readable
            // error screen instead of a silent trap.
            std::panic::set_hook(Box::new(|info| {
                let msg = info.to_string();
                unsafe { crate::ffi::panic(msg.as_ptr(), msg.len() as u32) };
            }));
            *self.get() = Some(make());
        }

        /// The cart's selected frame rate, as a frames-per-second number.
        /// The host queries this once after `init` to set its update/draw
        /// cadence. It depends only on the type, not the instance.
        pub fn fps(&self) -> u32 {
            G::FRAME_RATE.fps()
        }

        /// Advance the world one frame.
        pub fn update(&self) {
            if let Some(game) = self.get() {
                game.update(&mut Context { _private: () });
            }
        }

        /// Draw the world.
        pub fn draw(&self) {
            if let Some(game) = self.get() {
                game.draw(&mut Graphics { _private: () });
            }
        }

        #[allow(clippy::mut_from_ref)]
        fn get(&self) -> &mut Option<G> {
            unsafe { &mut *self.0.get() }
        }
    }

    impl<G> Default for Slot<G>
    where
        G: Game,
    {
        fn default() -> Self {
            Self::new()
        }
    }
}
