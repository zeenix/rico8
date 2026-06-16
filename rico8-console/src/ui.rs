//! Editor chrome shared by all modes: the red tab bar, the status bar,
//! mouse state and the cursor. Everything is drawn with the runtime's
//! fantasy-console primitives — no native widgets anywhere.

use crate::shell::Mode;
use rico8_runtime::{
    assets::Note,
    fb::{Framebuffer, HEIGHT, WIDTH},
    palette::col,
    ui as rui,
};

/// An 8×8 one-bit icon: each byte is a row, MSB is the left pixel.
pub type Icon8 = [u8; 8];

/// Blit an [`Icon8`] at (x, y) in the given colour; unset bits are left untouched.
pub fn draw_icon8(fb: &mut Framebuffer, icon: &Icon8, x: i32, y: i32, color: u8) {
    for (ry, row) in icon.iter().enumerate() {
        for rx in 0..8 {
            if row & (0x80 >> rx) != 0 {
                fb.pset(x + rx, y + ry as i32, color);
            }
        }
    }
}

/// The pencil glyph, sampled pixel-for-pixel from PICO-8 (shared by the sprite
/// and map editors).
pub const ICON_PENCIL: Icon8 = [0x08, 0x1C, 0x3E, 0x7C, 0xB8, 0x90, 0xE0, 0x00];

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
    // The SFX/music editors draw their own top-left chrome (the pitch/tracker
    // mode buttons), so leave their title blank here.
    let title = match active {
        Mode::Code => "code",
        Mode::Sprite => "sprite",
        Mode::Map => "map",
        _ => "",
    };
    fb.print(title, 2, 1, col::DARK_PURPLE);
    // The active tab is distinguished by icon colour only (peach), never a
    // background box — a box in the inactive-icon colour just melds with them.
    for (i, (icon, mode)) in TABS.iter().enumerate() {
        let x = tab_x(i);
        let color = if *mode == active {
            col::PEACH
        } else {
            col::DARK_PURPLE
        };
        rui::icon(fb, icon, x, 0, color);
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

// ---------------------------------------------------------------------------
// Shared SFX/music editor chrome, matching PICO-8's. Pixel-exact icons (flow
// buttons, waveform palette, the wave/circle toggles) are blitted from grids
// lifted verbatim from PICO-8's framebuffer.
// ---------------------------------------------------------------------------

/// Paint a pixel grid (one hex palette index per cell; `.` = black/0; `5` = the
/// dark-grey editor background, treated as transparent so it shows through).
pub fn blit(fb: &mut Framebuffer, x0: i32, y0: i32, rows: &[&str]) {
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch == '5' {
                continue;
            }
            let c = ch.to_digit(16).unwrap_or(0) as u8;
            fb.pset(x0 + dx as i32, y0 + dy as i32, c);
        }
    }
}

/// Left-pointing arrow (apex at left).
pub fn arrow_l(fb: &mut Framebuffer, x: i32, y: i32, c: u8) {
    fb.pset(x, y + 2, c);
    fb.line(x + 1, y + 1, x + 1, y + 3, c);
    fb.line(x + 2, y, x + 2, y + 4, c);
}

/// Right-pointing arrow (apex at right).
pub fn arrow_r(fb: &mut Framebuffer, x: i32, y: i32, c: u8) {
    fb.line(x, y, x, y + 4, c);
    fb.line(x + 1, y + 1, x + 1, y + 3, c);
    fb.pset(x + 2, y + 2, c);
}

/// The two top-left view-mode buttons: a bar-chart (pitch) and a 3x3 dot grid
/// (tracker). The active one is peach, the other dark-purple.
pub fn mode_buttons(fb: &mut Framebuffer, pitch_active: bool) {
    let (bars, grid) = if pitch_active {
        (col::PEACH, col::DARK_PURPLE)
    } else {
        (col::DARK_PURPLE, col::PEACH)
    };
    for i in 0..4 {
        fb.line(5 + i * 2, 1, 5 + i * 2, 6, bars);
    }
    for r in 0..3 {
        for c in 0..4 {
            fb.pset(15 + c * 2, 2 + r * 2, grid);
        }
    }
}

/// A channel enable toggle: a 5x5 light-grey box with a white centre when on.
pub fn radio(fb: &mut Framebuffer, x: i32, y: i32, on: bool) {
    fb.rect(x, y, x + 4, y + 4, col::LIGHT_GREY);
    if on {
        fb.pset(x + 2, y + 2, col::WHITE);
    }
}

/// The "edit this SFX" pencil glyph (lavender).
pub fn pencil(fb: &mut Framebuffer, x: i32, y: i32) {
    fb.line(x + 3, y, x, y + 3, col::LAVENDER);
    fb.line(x + 4, y + 1, x + 1, y + 4, col::LAVENDER);
}

const LETTERS: [&str; 12] = ["c", "c", "d", "d", "e", "f", "f", "g", "g", "a", "a", "b"];
const SHARP: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

/// One tracker note cell at (x, y), in PICO-8's field layout/colours: letter
/// (white) · `#` accidental · octave (light grey) · instrument (pink, or green
/// when a custom instrument) · volume (blue) · effect (grey `.` / orange digit).
/// A silent step (volume 0) renders as a faint dotted line.
pub fn note_cell(fb: &mut Framebuffer, x: i32, y: i32, note: Note) {
    if note.volume == 0 {
        for gx in (x + 2..x + 27).step_by(3) {
            fb.pset(gx, y + 4, col::DARK_BLUE);
        }
        return;
    }
    let k = (note.pitch % 12) as usize;
    fb.print(LETTERS[k], x + 2, y, col::WHITE);
    if SHARP[k] {
        fb.print("#", x + 6, y, col::WHITE);
    }
    fb.print(&format!("{}", note.pitch / 12), x + 10, y, col::LIGHT_GREY);
    let inst_col = if note.instrument().is_some() {
        col::GREEN
    } else {
        col::PINK
    };
    fb.print(&format!("{}", note.wave_index()), x + 15, y, inst_col);
    fb.print(&format!("{}", note.volume), x + 20, y, col::BLUE);
    if note.effect == 0 {
        fb.print(".", x + 24, y, col::DARK_GREY);
    } else {
        fb.print(&format!("{}", note.effect), x + 24, y, col::ORANGE);
    }
}

/// Flow-control flags (loop-start, loop-back, stop), lifted from PICO-8.
pub const FLOW: [&str; 8] = [
    "555555555555555555555555555",
    "555555c55555555555555555555",
    "555555cc5555515515551111555",
    "555cccccc555115515551111555",
    "555c..cc.551111115551111555",
    "555c55c.555.11...5551111555",
    "555.55.55555.1555555....555",
    "5555555555555.5555555555555",
];

/// The 8 SFX waveform-graph palette boxes (box 0 red/selected here), lifted
/// from PICO-8. `8` = red box, `6` = grey box, `7` = white graph line.
pub const PALETTE: [&str; 6] = [
    "888888885666666665666666665666666665666666665666666665666666665",
    "888778885666667665666666775667777765666677765667666665667666765",
    "887887885666776765666677675667666765666676765676766665767676765",
    "878888785677666765667766675667666765666676765766676765777777775",
    "788888875766666675776666675777666775777776775766677675676767675",
    "888888885666666665666666665666666665666666665666666665676666675",
];

/// The palette's display-mode circle toggle (lavender), lifted from PICO-8.
pub const CIRCLE: [&str; 6] = [
    "555555555",
    "55555dd55",
    "5555d55d5",
    "5555d55d5",
    "55555dd55",
    "555555555",
];

/// The header default-waveform wave icon (lavender), lifted from PICO-8.
pub const WAVEI: [&str; 5] = [
    "555555555",
    "555d55555",
    "55d5d5555",
    "5d555d5d5",
    "555555d55",
];

/// Draw the mouse cursor on top of everything.
pub fn draw_cursor(fb: &mut Framebuffer, mouse: &Mouse) {
    rui::cursor(fb, mouse.x, mouse.y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pencil_icon_draws_its_lit_pixels() {
        let mut fb = Framebuffer::new();
        draw_icon8(&mut fb, &ICON_PENCIL, 10, 20, col::WHITE);
        // Row 0 of ICON_PENCIL (0x08) lights exactly column 4.
        assert_eq!(fb.pget(10 + 4, 20), col::WHITE, "row 0 bit 4 lit");
        assert_eq!(fb.pget(10, 20), col::BLACK, "row 0 bit 0 unlit");
        // Row 6 (0xE0) lights columns 0..=2.
        assert_eq!(fb.pget(10, 26), col::WHITE, "row 6 bit 0 lit");
    }

    #[test]
    fn active_tab_has_no_background_box() {
        let mut fb = Framebuffer::new();
        draw_tab_bar(&mut fb, Mode::Music);
        // The active tab is shown by icon colour only: the cell behind it must
        // stay red, not a dark-purple box that melds with the other icons.
        let x = tab_x(4);
        assert_eq!(
            fb.pget(x - 1, 3),
            col::RED,
            "no background box behind the active tab"
        );
    }
}
