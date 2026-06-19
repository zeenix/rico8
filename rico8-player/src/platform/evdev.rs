//! evdev input: read every /dev/input/event*, fold key/button/hat events into the six console
//! buttons (0 L,1 R,2 U,3 D,4 O,5 X) plus named Select/Start.

use crate::platform::InputSnapshot;
use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};

/// Where a mapped input lands: a console button, or a named meta button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mapped {
    Button(usize),
    Select,
    Start,
}

/// Map a key/button code to a console button or named meta button. Covers keyboard, named
/// gamepad buttons, and the raw BTN_0..3 fallback for pads with no proper mapping.
pub fn map_key(k: KeyCode) -> Option<Mapped> {
    use Mapped::*;
    Some(match k {
        KeyCode::KEY_LEFT => Button(0),
        KeyCode::KEY_RIGHT => Button(1),
        KeyCode::KEY_UP => Button(2),
        KeyCode::KEY_DOWN => Button(3),
        KeyCode::KEY_Z | KeyCode::KEY_C | KeyCode::KEY_N => Button(4),
        KeyCode::KEY_X | KeyCode::KEY_V | KeyCode::KEY_M => Button(5),
        KeyCode::KEY_ESC => Select,
        KeyCode::BTN_DPAD_LEFT => Button(0),
        KeyCode::BTN_DPAD_RIGHT => Button(1),
        KeyCode::BTN_DPAD_UP => Button(2),
        KeyCode::BTN_DPAD_DOWN => Button(3),
        KeyCode::BTN_SOUTH | KeyCode::BTN_NORTH | KeyCode::BTN_0 | KeyCode::BTN_3 => Button(4),
        KeyCode::BTN_EAST | KeyCode::BTN_WEST | KeyCode::BTN_1 | KeyCode::BTN_2 => Button(5),
        KeyCode::BTN_SELECT => Select,
        KeyCode::BTN_START => Start,
        _ => return None,
    })
}

/// Optional raw-index overrides (RICO8_SELECT / RICO8_START) for pads with no named buttons.
fn env_btn(var: &str) -> Option<u16> {
    std::env::var(var).ok()?.trim().parse().ok()
}

/// Polls all `/dev/input/event*` devices non-blocking, accumulating button state per frame.
pub struct Input {
    devices: Vec<Device>,
    buttons: [bool; 6],
    select: bool,
    start: bool,
    fps_prev: bool,
    fps_edge: bool,
    sel_raw: Option<u16>,
    start_raw: Option<u16>,
}

impl Input {
    /// Open and enumerate all available input devices.
    pub fn new() -> Input {
        let mut devices: Vec<Device> = evdev::enumerate().map(|(_, d)| d).collect();
        for d in &mut devices {
            let _ = d.set_nonblocking(true);
        }
        eprintln!("rico8-player: {} input device(s)", devices.len());
        Input {
            devices,
            buttons: [false; 6],
            select: false,
            start: false,
            fps_prev: false,
            fps_edge: false,
            sel_raw: env_btn("RICO8_SELECT"),
            start_raw: env_btn("RICO8_START"),
        }
    }

    /// Drain all pending events from every device and return a snapshot of current button state.
    pub fn poll(&mut self) -> InputSnapshot {
        self.fps_edge = false;
        // Collect events first to avoid holding the &mut borrow across the match.
        let mut events = Vec::new();
        for dev in &mut self.devices {
            match dev.fetch_events() {
                Ok(iter) => events.extend(iter),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
        for ev in events {
            match ev.destructure() {
                EventSummary::Key(_, KeyCode::KEY_F1, 1) => self.fps_edge = true,
                EventSummary::Key(_, code, val) if val != 2 => {
                    let down = val == 1;
                    if Some(code.code()) == self.sel_raw {
                        self.select = down;
                    } else if Some(code.code()) == self.start_raw {
                        self.start = down;
                    } else if let Some(m) = map_key(code) {
                        match m {
                            Mapped::Button(b) => self.buttons[b] = down,
                            Mapped::Select => self.select = down,
                            Mapped::Start => self.start = down,
                        }
                    }
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0X, v) => {
                    self.buttons[0] = v < 0;
                    self.buttons[1] = v > 0;
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0Y, v) => {
                    self.buttons[2] = v < 0;
                    self.buttons[3] = v > 0;
                }
                _ => {}
            }
        }
        InputSnapshot {
            buttons: self.buttons,
            select: self.select,
            start: self.start,
            quit_requested: false,
            fps_toggle: self.fps_edge,
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_arrows_and_face() {
        assert_eq!(map_key(KeyCode::KEY_LEFT), Some(Mapped::Button(0)));
        assert_eq!(map_key(KeyCode::KEY_DOWN), Some(Mapped::Button(3)));
        assert_eq!(map_key(KeyCode::KEY_Z), Some(Mapped::Button(4)));
        assert_eq!(map_key(KeyCode::KEY_X), Some(Mapped::Button(5)));
        assert_eq!(map_key(KeyCode::KEY_ESC), Some(Mapped::Select));
        assert_eq!(map_key(KeyCode::KEY_A), None);
    }

    #[test]
    fn gamepad_dpad_and_buttons() {
        assert_eq!(map_key(KeyCode::BTN_DPAD_UP), Some(Mapped::Button(2)));
        assert_eq!(map_key(KeyCode::BTN_SOUTH), Some(Mapped::Button(4)));
        assert_eq!(map_key(KeyCode::BTN_EAST), Some(Mapped::Button(5)));
        assert_eq!(map_key(KeyCode::BTN_SELECT), Some(Mapped::Select));
        assert_eq!(map_key(KeyCode::BTN_START), Some(Mapped::Start));
    }

    #[test]
    fn raw_face_fallback_indices() {
        // Nintendo-style cross fallback for unmapped pads: BTN_0/3 = O, BTN_1/2 = X.
        assert_eq!(map_key(KeyCode::BTN_0), Some(Mapped::Button(4)));
        assert_eq!(map_key(KeyCode::BTN_1), Some(Mapped::Button(5)));
    }
}
