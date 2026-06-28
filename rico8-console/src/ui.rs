//! Editor chrome shared by all modes: the red tab bar, the status bar,
//! mouse state and the cursor. Everything is drawn with the runtime's
//! fantasy-console primitives — no native widgets anywhere.

use crate::shell::Mode;
use rico8_runtime::{
    assets::Note,
    fb::{Framebuffer, HEIGHT, WIDTH},
    font,
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

/// Leftmost and rightmost lit columns of an icon (its ink bounds, 0..8).
fn ink_bounds(icon: &rui::Icon) -> (i32, i32) {
    let mut lo = 8;
    let mut hi = -1;
    for &row in icon.iter() {
        for c in 0..8 {
            if row & (0x80 >> c) != 0 {
                lo = lo.min(c);
                hi = hi.max(c);
            }
        }
    }
    (lo, hi)
}

/// X offset of each tab's 8×8 cell, packed right-aligned so adjacent icons'
/// ink is always one blank pixel apart — each icon's own edge margin is
/// absorbed rather than added, keeping every icon equidistant.
fn tab_positions() -> [i32; TABS.len()] {
    /// Blank pixels between one icon's last ink column and the next's first.
    const GAP: i32 = 1;
    /// Rightmost ink column of the last icon (1px in from the screen edge).
    const RIGHT: i32 = WIDTH - 2;
    let n = TABS.len();
    let mut xs = [0i32; TABS.len()];
    xs[n - 1] = RIGHT - ink_bounds(TABS[n - 1].0).1;
    for i in (0..n - 1).rev() {
        let hi = ink_bounds(TABS[i].0).1;
        let lo_next = ink_bounds(TABS[i + 1].0).0;
        xs[i] = xs[i + 1] + lo_next - hi - (GAP + 1);
    }
    xs
}

fn tab_x(i: usize) -> i32 {
    tab_positions()[i]
}

/// Top bar: the red strip and the five editor tab icons, right-aligned.
/// Per-editor top-left content (the code filename, the SFX mode buttons) is
/// drawn by the shell on top of this.
pub fn draw_tab_bar(fb: &mut Framebuffer, active: Mode) {
    fb.rectfill(0, 0, WIDTH - 1, 7, col::RED);
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

/// Left edge of the filename label in the tab bar.
const FILENAME_X: i32 = 2;

/// The filename label's right-edge limit: it stays one blank column clear of
/// the leftmost tab icon, so a long name never overdraws the tabs or shares a
/// click with them.
fn filename_limit() -> i32 {
    tab_x(0) - 2
}

/// Draw the code editor's current filename in the top-left of the tab bar, in
/// peach to match the highlighted code tab. Truncated to stay clear of the tab
/// icons; empty when no project file is open (a loaded cart), so nothing shows.
pub fn code_filename(fb: &mut Framebuffer, name: &str) {
    let max = ((filename_limit() - FILENAME_X) / font::GLYPH_W).max(0) as usize;
    let shown: String = name.chars().take(max).collect();
    fb.print(&shown, FILENAME_X, 1, col::PEACH);
}

/// Whether a left-press landed on the top-left filename (the click target that
/// opens the file picker). False for an empty name; the region is clamped clear
/// of the tab icons so one click never routes to both.
pub fn filename_clicked(mouse: &Mouse, name: &str) -> bool {
    !name.is_empty()
        && mouse.left_pressed
        && mouse.y < 8
        && mouse.x >= 1
        && mouse.x < filename_limit()
        && mouse.x <= FILENAME_X + font::text_width(name)
}

/// The tab index whose 8×8 cell contains screen-x `x`, if any.
fn tab_at(x: i32) -> Option<usize> {
    (0..TABS.len()).find(|&i| x >= tab_x(i) - 1 && x <= tab_x(i) + 7)
}

/// Which tab (index into `EDITOR_MODES`) was clicked this frame, if any.
pub fn tab_bar_click(mouse: &Mouse) -> Option<usize> {
    if !mouse.left_pressed || mouse.y >= 8 {
        return None;
    }
    tab_at(mouse.x)
}

/// The tab the cursor hovers in the top bar, if any (for the bottom-bar hint).
pub fn tab_bar_hover(mouse: &Mouse) -> Option<usize> {
    if mouse.y >= 8 {
        return None;
    }
    tab_at(mouse.x)
}

/// Display name for tab `i`, in `TABS` order.
pub fn tab_name(i: usize) -> &'static str {
    const NAMES: [&str; TABS.len()] = ["Code", "Sprite", "Map", "SFX", "Music"];
    NAMES[i]
}

/// Bottom status bar with a single line of text.
pub fn status_bar(fb: &mut Framebuffer, text: &str) {
    fb.rectfill(0, HEIGHT - 8, WIDTH - 1, HEIGHT - 1, col::RED);
    fb.print(text, 2, HEIGHT - 7, col::DARK_PURPLE);
}

/// Frames a transient status message stays up (~2.5s at 60fps).
pub const STATUS_TTL: u16 = 150;

/// A transient bottom-bar message with a frame countdown; falls back to a
/// static hint once it expires. Callers pass text already sized for the bar.
#[derive(Default)]
pub struct StatusMsg {
    text: Option<String>,
    ttl: u16,
}

impl StatusMsg {
    /// Show `text` for [`STATUS_TTL`] frames.
    pub fn set(&mut self, text: String) {
        self.text = Some(text);
        self.ttl = STATUS_TTL;
    }

    /// Count down one frame, clearing the message at zero. Call once per tick.
    pub fn tick(&mut self) {
        if self.ttl > 0 {
            self.ttl -= 1;
            if self.ttl == 0 {
                self.text = None;
            }
        }
    }

    /// Draw the message if set, else `fallback`.
    pub fn show(&self, fb: &mut Framebuffer, fallback: &str) {
        status_bar(fb, self.text.as_deref().unwrap_or(fallback));
    }

    /// The current message text, if any (used by tests to assert paste feedback).
    #[cfg(test)]
    pub fn current(&self) -> Option<&str> {
        self.text.as_deref()
    }
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

/// The two top-left view buttons for the sprite/map editors: a panelled layout
/// glyph (normal, a box with a divider strip) and a hollow box (fullscreen). The
/// active one is peach, the other dark-purple. Hit regions match `mode_buttons`:
/// `over(4, 0, 12, 7)` for normal, `over(13, 0, 22, 7)` for fullscreen.
pub fn view_buttons(fb: &mut Framebuffer, fullscreen: bool) {
    let (normal, full) = if fullscreen {
        (col::DARK_PURPLE, col::PEACH)
    } else {
        (col::PEACH, col::DARK_PURPLE)
    };
    // Normal: a small panel with a divider near the bottom (canvas + sheet).
    fb.rect(5, 1, 11, 6, normal);
    fb.line(5, 4, 11, 4, normal);
    // Fullscreen: a single hollow box (the whole screen, no panels).
    fb.rect(15, 1, 21, 6, full);
}

/// The music grid's Pat/Sfx toggle, drawn in the top bar: two labels flanking a
/// slide switch whose raised knob sits on the active side (the active label is
/// white). The whole switch is one click target (handled by the music editor).
pub fn pat_sfx_toggle(fb: &mut Framebuffer, sfx_active: bool) {
    let pat_c = if sfx_active {
        col::DARK_PURPLE
    } else {
        col::WHITE
    };
    let sfx_c = if sfx_active {
        col::WHITE
    } else {
        col::DARK_PURPLE
    };
    fb.print("Pat", 28, 1, pat_c);
    // A thin recessed groove with a knob standing proud of it on the active
    // side — so it reads as a slide switch, not a filled bar.
    fb.rectfill(43, 3, 56, 4, col::DARK_PURPLE);
    let kx = if sfx_active { 51 } else { 43 };
    fb.rectfill(kx, 1, kx + 5, 6, col::WHITE);
    fb.print("Sfx", 59, 1, sfx_c);
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
mod paste_status_tests {
    use super::*;

    #[test]
    fn status_msg_falls_back_then_expires() {
        let mut m = StatusMsg::default();
        assert_eq!(m.current(), None);
        m.set("done".into());
        assert_eq!(m.current(), Some("done"));
        for _ in 0..STATUS_TTL {
            m.tick();
        }
        assert_eq!(m.current(), None);
    }
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

    #[test]
    fn tab_bar_has_no_title_text() {
        // The old "Code" label sat at (2, 1); that area must now be plain red.
        let mut fb = Framebuffer::new();
        draw_tab_bar(&mut fb, Mode::Code);
        for x in 2..18 {
            assert_eq!(fb.pget(x, 1), col::RED, "title row must be blank at x={x}");
        }
    }

    #[test]
    fn hover_maps_x_to_the_tab_and_its_name() {
        // Inside the first tab's cell -> index 0 ("Code"); off the tabs -> None.
        let over_first = Mouse {
            x: tab_x(0) + 3,
            y: 3,
            ..Default::default()
        };
        assert_eq!(tab_bar_hover(&over_first), Some(0));
        assert_eq!(tab_name(tab_bar_hover(&over_first).unwrap()), "Code");
        let last = TABS.len() - 1;
        let over_last = Mouse {
            x: tab_x(last) + 3,
            y: 3,
            ..Default::default()
        };
        assert_eq!(tab_name(tab_bar_hover(&over_last).unwrap()), "Music");
        // Far left (over the filename area) and below the bar are not tabs.
        let off = Mouse {
            x: 1,
            y: 3,
            ..Default::default()
        };
        assert_eq!(tab_bar_hover(&off), None);
        let below = Mouse {
            x: tab_x(0) + 3,
            y: 8,
            ..Default::default()
        };
        assert_eq!(tab_bar_hover(&below), None);
    }

    #[test]
    fn code_filename_draws_and_is_clickable() {
        let mut fb = Framebuffer::new();
        code_filename(&mut fb, "lib.rs");
        // Some pixel in the label band is lit peach.
        let lit = (2..2 + font::text_width("lib.rs"))
            .any(|x| (1..7).any(|y| fb.pget(x, y) == col::PEACH));
        assert!(lit, "filename should render in peach");

        let press = Mouse {
            x: 3,
            y: 2,
            left_pressed: true,
            ..Default::default()
        };
        assert!(filename_clicked(&press, "lib.rs"));
        // Empty name: nothing to click.
        assert!(!filename_clicked(&press, ""));
        // Past the text, below the bar, and a non-press are all rejected.
        let far = Mouse { x: 120, ..press };
        assert!(!filename_clicked(&far, "lib.rs"));
        let below = Mouse { y: 9, ..press };
        assert!(!filename_clicked(&below, "lib.rs"));
        let hover = Mouse {
            left_pressed: false,
            ..press
        };
        assert!(!filename_clicked(&hover, "lib.rs"));
    }

    #[test]
    fn view_buttons_light_the_active_view() {
        // Normal active: the layout glyph is peach, the box glyph dark-purple.
        let mut fb = Framebuffer::new();
        view_buttons(&mut fb, false);
        assert_eq!(fb.pget(5, 1), col::PEACH);
        assert_eq!(fb.pget(15, 1), col::DARK_PURPLE);
        // Fullscreen active: the colours swap.
        let mut fb2 = Framebuffer::new();
        view_buttons(&mut fb2, true);
        assert_eq!(fb2.pget(5, 1), col::DARK_PURPLE);
        assert_eq!(fb2.pget(15, 1), col::PEACH);
    }

    #[test]
    fn tabs_are_equidistant_by_ink() {
        // Every adjacent pair of icons is separated by exactly one blank column
        // between their ink, regardless of each bitmap's own edge margins.
        let xs = tab_positions();
        let gaps: Vec<i32> = (0..TABS.len() - 1)
            .map(|i| {
                let this_right = xs[i] + ink_bounds(TABS[i].0).1;
                let next_left = xs[i + 1] + ink_bounds(TABS[i + 1].0).0;
                next_left - this_right - 1
            })
            .collect();
        assert!(gaps.iter().all(|&g| g == 1), "uneven tab gaps: {gaps:?}");
    }
}
