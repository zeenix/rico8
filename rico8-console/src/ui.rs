//! Editor chrome shared by all modes: the red tab bar, the status bar,
//! mouse state and the cursor. Everything is drawn with the runtime's
//! fantasy-console primitives — no native widgets anywhere.

use crate::shell::Mode;
use rico8_runtime::fb::{Framebuffer, HEIGHT, WIDTH};
use rico8_runtime::palette::col;
use rico8_runtime::ui as rui;

/// Mouse state in virtual-screen coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Mouse {
    pub x: i32,
    pub y: i32,
    pub left: bool,
    pub right: bool,
    /// Edge-triggered: pressed since the last frame.
    pub left_pressed: bool,
    pub right_pressed: bool,
}

impl Default for Mouse {
    fn default() -> Self {
        // Off-screen until the first real cursor event arrives.
        Self {
            x: -16,
            y: -16,
            left: false,
            right: false,
            left_pressed: false,
            right_pressed: false,
        }
    }
}

impl Mouse {
    /// Clear edge flags; called by the shell at the end of each tick.
    pub fn end_frame(&mut self) {
        self.left_pressed = false;
        self.right_pressed = false;
    }

    pub fn over(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        self.x >= x0 && self.x <= x1 && self.y >= y0 && self.y <= y1
    }
}

const TABS: [(&rui::Icon, Mode); 5] = [
    (&rui::ICON_CODE, Mode::Code),
    (&rui::ICON_SPRITE, Mode::Sprite),
    (&rui::ICON_MAP, Mode::Map),
    (&rui::ICON_SFX, Mode::Sfx),
    (&rui::ICON_MUSIC, Mode::Music),
];

fn tab_x(i: usize) -> i32 {
    WIDTH - 5 * 9 + i as i32 * 9
}

/// Top bar: title on the left, the five editor tab icons on the right.
pub fn draw_tab_bar(fb: &mut Framebuffer, active: Mode) {
    fb.rectfill(0, 0, WIDTH - 1, 7, col::RED);
    let title = match active {
        Mode::Code => "code",
        Mode::Sprite => "sprite",
        Mode::Map => "map",
        Mode::Sfx => "sfx",
        Mode::Music => "music",
        _ => "",
    };
    fb.print(title, 2, 1, col::DARK_PURPLE);
    for (i, (icon, mode)) in TABS.iter().enumerate() {
        let x = tab_x(i);
        if *mode == active {
            fb.rectfill(x - 1, 0, x + 7, 7, col::DARK_PURPLE);
            rui::icon(fb, icon, x, 0, col::WHITE);
        } else {
            rui::icon(fb, icon, x, 0, col::DARK_PURPLE);
        }
    }
}

/// Which tab (index into `EDITOR_MODES`) was clicked this frame, if any.
pub fn tab_bar_click(mouse: &Mouse) -> Option<usize> {
    if !mouse.left_pressed || mouse.y >= 8 {
        return None;
    }
    (0..5).find(|&i| mouse.x >= tab_x(i) - 1 && mouse.x <= tab_x(i) + 7)
}

/// Bottom status bar with a single line of text.
pub fn status_bar(fb: &mut Framebuffer, text: &str) {
    fb.rectfill(0, HEIGHT - 8, WIDTH - 1, HEIGHT - 1, col::RED);
    fb.print(text, 2, HEIGHT - 7, col::DARK_PURPLE);
}

/// Draw the mouse cursor on top of everything.
pub fn draw_cursor(fb: &mut Framebuffer, mouse: &Mouse) {
    rui::cursor(fb, mouse.x, mouse.y);
}
