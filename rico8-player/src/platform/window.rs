//! Windowed desktop backend: a winit window with a softbuffer CPU surface and keyboard input.
//!
//! The game loop owns the schedule, so this drains pending winit events with `pump_app_events`
//! once per frame rather than handing control to winit's own `run_app`. Audio is silent here; a
//! later task wires up cpal.

use crate::platform::{blit, InputSnapshot, Platform, Rotate};
use anyhow::{anyhow, Result};
use rico8_runtime::fb::Framebuffer;
use softbuffer::{Context, Surface};
use std::{num::NonZeroU32, rc::Rc};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::{Window, WindowId},
};

/// A winit + softbuffer windowed backend. Implements `Platform` with keyboard input.
pub struct WindowPlatform {
    event_loop: EventLoop<()>,
    handler: WinHandler,
}

impl WindowPlatform {
    /// Create the event loop and input handler. The window itself is created lazily on the first
    /// `resumed`, which winit delivers during the first `poll`.
    pub fn new() -> Result<WindowPlatform> {
        let event_loop =
            EventLoop::new().map_err(|e| anyhow!("creating the winit event loop: {e}"))?;
        Ok(WindowPlatform {
            event_loop,
            handler: WinHandler::new(),
        })
    }
}

impl Platform for WindowPlatform {
    fn present(&mut self, fb: &Framebuffer) -> Result<()> {
        self.handler.present(fb)
    }

    fn poll(&mut self) -> InputSnapshot {
        // The fps toggle is an edge, so clear it before draining this frame's events.
        self.handler.fps_edge = false;
        let status = self
            .event_loop
            .pump_app_events(Some(std::time::Duration::ZERO), &mut self.handler);
        if let PumpStatus::Exit(_) = status {
            self.handler.quit = true;
        }
        InputSnapshot {
            buttons: self.handler.buttons,
            select: self.handler.select,
            start: self.handler.start,
            quit_requested: self.handler.quit,
            fps_toggle: self.handler.fps_edge,
        }
    }
}

/// Where a physical key lands: a console button (0..6), a named meta button, or the fps toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Button(usize),
    Select,
    Start,
    Fps,
}

/// Map a physical key to a console action. Arrows + Z/X (with C/V, N/M aliases) like the runtime;
/// Esc backs out, Enter is Start, F1 toggles the fps meter.
pub fn map_key(code: KeyCode) -> Option<Key> {
    use Key::*;
    Some(match code {
        KeyCode::ArrowLeft => Button(0),
        KeyCode::ArrowRight => Button(1),
        KeyCode::ArrowUp => Button(2),
        KeyCode::ArrowDown => Button(3),
        KeyCode::KeyZ | KeyCode::KeyC | KeyCode::KeyN => Button(4),
        KeyCode::KeyX | KeyCode::KeyV | KeyCode::KeyM => Button(5),
        KeyCode::Escape => Select,
        KeyCode::Enter | KeyCode::NumpadEnter => Start,
        KeyCode::F1 => Fps,
        _ => return None,
    })
}

/// The winit application state: the lazily-created window + softbuffer surface and the input
/// accumulated across pumped events. The surface borrows the window by handle, so the window is an
/// `Rc<Window>` shared with both the softbuffer context and the surface.
struct WinHandler {
    /// `None` until `resumed` creates the window on the first `poll`.
    surface: Option<WinSurface>,
    /// Blit rotation, from `RICO8_ROTATE`; unset means `Rotate::None` (the desktop default).
    rotate: Rotate,
    /// Console buttons: 0 left, 1 right, 2 up, 3 down, 4 O, 5 X.
    buttons: [bool; 6],
    select: bool,
    start: bool,
    quit: bool,
    /// Set for a single frame when F1 is pressed (a rising edge, not a hold).
    fps_edge: bool,
}

/// The window and its softbuffer surface. The context is kept alive alongside the surface it
/// created.
struct WinSurface {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    /// Held so it outlives the surface it created.
    _context: Context<Rc<Window>>,
}

impl WinHandler {
    /// A handler with no window yet and rotation taken from `RICO8_ROTATE` (default upright).
    fn new() -> WinHandler {
        WinHandler {
            surface: None,
            rotate: Rotate::from_env_or(Rotate::None),
            buttons: [false; 6],
            select: false,
            start: false,
            quit: false,
            fps_edge: false,
        }
    }

    /// Present `fb`, scaled and letterboxed, into the softbuffer surface.
    ///
    /// Before the first `resumed` there is no surface yet, so the first frame is a no-op.
    fn present(&mut self, fb: &Framebuffer) -> Result<()> {
        let Some(s) = self.surface.as_mut() else {
            return Ok(());
        };
        let size = s.window.inner_size();
        let (w, h) = (size.width, size.height);
        // A zero dimension (e.g. a minimized window) has no buffer to present into.
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
            return Ok(());
        };
        s.surface
            .resize(nw, nh)
            .map_err(|e| anyhow!("resizing the softbuffer surface: {e}"))?;
        let mut buf = s
            .surface
            .buffer_mut()
            .map_err(|e| anyhow!("acquiring the softbuffer back buffer: {e}"))?;
        blit::present_into(fb, &mut buf, w as usize, h as usize, self.rotate);
        buf.present()
            .map_err(|e| anyhow!("presenting the softbuffer frame: {e}"))
    }
}

impl ApplicationHandler for WinHandler {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("RICO-8")
            .with_inner_size(LogicalSize::new(512.0, 512.0));
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rico8-player: could not open a window: {e}");
                el.exit();
                return;
            }
        };
        let context = match Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rico8-player: softbuffer context failed: {e}");
                el.exit();
                return;
            }
        };
        let surface = match Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rico8-player: softbuffer surface failed: {e}");
                el.exit();
                return;
            }
        };
        self.surface = Some(WinSurface {
            window,
            surface,
            _context: context,
        });
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.quit = true,
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(key) = map_key(code) else {
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                match key {
                    Key::Button(i) => self.buttons[i] = pressed,
                    Key::Select => self.select = pressed,
                    Key::Start => self.start = pressed,
                    // The fps toggle fires once on the rising edge, ignoring auto-repeat.
                    Key::Fps => {
                        if pressed && !event.repeat {
                            self.fps_edge = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_map_to_dpad() {
        assert_eq!(map_key(KeyCode::ArrowLeft), Some(Key::Button(0)));
        assert_eq!(map_key(KeyCode::ArrowRight), Some(Key::Button(1)));
        assert_eq!(map_key(KeyCode::ArrowUp), Some(Key::Button(2)));
        assert_eq!(map_key(KeyCode::ArrowDown), Some(Key::Button(3)));
    }

    #[test]
    fn z_and_x_map_to_face_buttons() {
        assert_eq!(map_key(KeyCode::KeyZ), Some(Key::Button(4)));
        assert_eq!(map_key(KeyCode::KeyC), Some(Key::Button(4)));
        assert_eq!(map_key(KeyCode::KeyN), Some(Key::Button(4)));
        assert_eq!(map_key(KeyCode::KeyX), Some(Key::Button(5)));
        assert_eq!(map_key(KeyCode::KeyV), Some(Key::Button(5)));
        assert_eq!(map_key(KeyCode::KeyM), Some(Key::Button(5)));
    }

    #[test]
    fn meta_keys_map_to_named_buttons() {
        assert_eq!(map_key(KeyCode::Escape), Some(Key::Select));
        assert_eq!(map_key(KeyCode::Enter), Some(Key::Start));
        assert_eq!(map_key(KeyCode::NumpadEnter), Some(Key::Start));
        assert_eq!(map_key(KeyCode::F1), Some(Key::Fps));
    }

    #[test]
    fn unmapped_keys_yield_none() {
        assert_eq!(map_key(KeyCode::KeyA), None);
    }
}
