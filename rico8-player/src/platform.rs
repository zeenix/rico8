//! The platform seam: everything device-specific (display, input) lives behind this trait,
//! so the picker and game loop are written once and tested headless.

pub mod blit;
pub mod null;

use anyhow::Result;
use rico8_runtime::fb::Framebuffer;

/// A display + input backend. Audio is owned internally by the backend (see `KmsPlatform`),
/// pulled from the shared synth, so it is not part of this surface.
pub trait Platform {
    /// Scale, rotate and letterbox a 128x128 indexed frame onto the screen.
    fn present(&mut self, fb: &Framebuffer) -> Result<()>;

    /// A snapshot of input for this frame.
    fn poll(&mut self) -> InputSnapshot;
}

/// One frame's input: the six console buttons plus the meta signals the loops act on.
#[derive(Clone, Copy, Default)]
pub struct InputSnapshot {
    /// Console buttons: 0 left, 1 right, 2 up, 3 down, 4 O, 5 X.
    pub buttons: [bool; 6],
    /// A named Select (when the device exposes one), used for back-to-picker.
    pub select: bool,
    /// A named Start, used with Select for quit.
    pub start: bool,
    /// The OS asked us to quit (SIGTERM / console close).
    pub quit_requested: bool,
    /// The fps meter was toggled this frame (edge, F1).
    pub fps_toggle: bool,
}

/// Screen rotation applied during the blit, for panels mounted rotated.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rotate {
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotate {
    /// `RICO8_ROTATE=0|90|180|270` overrides `default`; an unset/invalid value keeps `default`.
    pub fn from_env_or(default: Rotate) -> Rotate {
        match std::env::var("RICO8_ROTATE").ok().as_deref() {
            Some("0") => Rotate::None,
            Some("90") => Rotate::Cw90,
            Some("180") => Rotate::Cw180,
            Some("270") => Rotate::Cw270,
            _ => default,
        }
    }
}
