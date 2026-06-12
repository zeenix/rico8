//! Game controller state: the classic 6-button pad.
//!
//! Carts see input only through `btn`/`btnp`. The host maps physical keys
//! to these buttons (arrows + Z/X by default) and ticks this state once
//! per logical frame.

/// Button indices, matching the ABI and the classic layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Left = 0,
    Right = 1,
    Up = 2,
    Down = 3,
    /// "O" action button (Z / C / N on the keyboard).
    O = 4,
    /// "X" action button (X / V / M on the keyboard).
    X = 5,
}

pub const BUTTON_COUNT: usize = 6;

/// Frames a button must be held before `btnp` starts repeating.
const REPEAT_DELAY: u32 = 15;
/// Repeat interval in frames once repeating.
const REPEAT_EVERY: u32 = 4;

/// Per-frame button state with press/repeat tracking.
#[derive(Default)]
pub struct InputState {
    held: [bool; BUTTON_COUNT],
    frames_held: [u32; BUTTON_COUNT],
}

impl InputState {
    /// Update the raw held state of a button (called on key events).
    pub fn set_button(&mut self, b: usize, down: bool) {
        if b < BUTTON_COUNT {
            self.held[b] = down;
        }
    }

    /// Advance one logical frame. Must be called exactly once per update.
    pub fn tick(&mut self) {
        for i in 0..BUTTON_COUNT {
            if self.held[i] {
                self.frames_held[i] = self.frames_held[i].saturating_add(1);
            } else {
                self.frames_held[i] = 0;
            }
        }
    }

    /// Is the button currently held?
    pub fn btn(&self, b: u32) -> bool {
        (b as usize) < BUTTON_COUNT && self.held[b as usize]
    }

    /// Was the button just pressed this frame? Repeats after a short delay
    /// while held, matching the classic `btnp` feel.
    pub fn btnp(&self, b: u32) -> bool {
        let i = b as usize;
        if i >= BUTTON_COUNT {
            return false;
        }
        let f = self.frames_held[i];
        f == 1 || (f > REPEAT_DELAY && (f - REPEAT_DELAY) % REPEAT_EVERY == 1)
    }

    /// Clear all held buttons (e.g. when leaving run mode).
    pub fn clear(&mut self) {
        self.held = [false; BUTTON_COUNT];
        self.frames_held = [0; BUTTON_COUNT];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btn_reflects_held_state() {
        let mut s = InputState::default();
        s.set_button(Button::Right as usize, true);
        s.tick();
        assert!(s.btn(1));
        assert!(!s.btn(0));
    }

    #[test]
    fn btnp_fires_once_then_repeats() {
        let mut s = InputState::default();
        s.set_button(0, true);
        s.tick();
        assert!(s.btnp(0), "fires on first frame");
        s.tick();
        assert!(!s.btnp(0), "does not fire on second frame");
        // Hold until just past the repeat delay (frame REPEAT_DELAY + 1).
        for _ in 0..(REPEAT_DELAY - 1) {
            s.tick();
        }
        assert!(s.btnp(0), "repeats after delay");
        s.tick();
        assert!(!s.btnp(0));
    }

    #[test]
    fn release_resets_press() {
        let mut s = InputState::default();
        s.set_button(0, true);
        s.tick();
        s.set_button(0, false);
        s.tick();
        s.set_button(0, true);
        s.tick();
        assert!(s.btnp(0), "fires again after release");
    }
}
