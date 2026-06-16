//! The map editor: a scrollable tile viewport with six PICO-8-style tools
//! (draw, paste, select, pan, fill, circle), a clipboard, and a full-page
//! sprite sheet picker along the bottom.

use crate::{
    shell::{Key, Mods},
    ui::{self, draw_icon8, Icon8, Mouse, ICON_PENCIL},
};
use rico8_runtime::{
    assets::{Assets, MAP_H, MAP_W, SPRITES_PER_ROW},
    fb::Framebuffer,
    palette::col,
};

// --- Layout (see the design spec) -------------------------------------------
const TOOLBAR_Y: i32 = 8; // tool icons drawn at y 9.
const VIEW_Y: i32 = 18;
const VIEW_TILES_X: i32 = 16;
const VIEW_TILES_Y: i32 = 8;
const VIEW_BOTTOM: i32 = VIEW_Y + VIEW_TILES_Y * 8 - 1; // 81
const SEP_Y: i32 = 82;
const SHEET_Y: i32 = 84; // 4 rows of 8 px
const SHEET_BOTTOM: i32 = SHEET_Y + 4 * 8 - 1; // 115
const BRUSH_X: i32 = 88; // 8×8 brush preview
const PAGE_X: i32 = 104; // four 5×5 page dots
const PAGE_Y: i32 = 11;

fn tool_x(i: usize) -> i32 {
    2 + i as i32 * 9
}

// --- Tool icons (Draw reuses the shared pencil; rest sampled from PICO-8) ----
const ICON_STAMP: Icon8 = [0x38, 0x38, 0x38, 0x38, 0xFE, 0x82, 0xFE, 0x00]; // paste
const ICON_SELECT: Icon8 = [0xAA, 0x00, 0x82, 0x00, 0x82, 0x00, 0xAA, 0x00];
const ICON_HAND: Icon8 = [0x28, 0x2A, 0x2A, 0x3E, 0xBE, 0x7E, 0x1C, 0x00]; // pan
const ICON_FILL: Icon8 = [0x08, 0x04, 0x02, 0x7F, 0xBE, 0x9C, 0x88, 0x00]; // bucket
const ICON_CIRCLE: Icon8 = [0x38, 0x44, 0x82, 0x82, 0x82, 0x44, 0x38, 0x00];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Draw,
    Paste,
    Select,
    Pan,
    Fill,
    Circle,
}

// Icons stored by value (`Icon8` is `[u8; 8]`, `Copy`), matching the sprite
// editor's `tools` array, so iterating yields `icon: &Icon8` for `draw_icon8`.
const TOOLS: [(Tool, Icon8); 6] = [
    (Tool::Draw, ICON_PENCIL),
    (Tool::Paste, ICON_STAMP),
    (Tool::Select, ICON_SELECT),
    (Tool::Pan, ICON_HAND),
    (Tool::Fill, ICON_FILL),
    (Tool::Circle, ICON_CIRCLE),
];

/// A rectangular cell selection, in map space (it scrolls with the camera).
#[derive(Debug, Clone, Copy)]
struct Selection {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// A copied block of tiles, row-major.
#[derive(Debug, Clone)]
struct Clipboard {
    w: i32,
    h: i32,
    tiles: Vec<u8>,
}

/// The in-progress mouse drag, if any.
#[derive(Debug, Clone)]
enum Drag {
    None,
    Selecting {
        ax: i32,
        ay: i32,
    },
    Moving {
        tiles: Vec<u8>,
        w: i32,
        h: i32,
        gx: i32,
        gy: i32,
    },
    Panning {
        amx: i32,
        amy: i32,
        acx: i32,
        acy: i32,
    },
    /// Fill-rect and circle previews.
    Shape {
        ax: i32,
        ay: i32,
    },
}

pub struct MapEditor {
    tool: Tool,
    brush: u32,
    cam_x: i32,
    cam_y: i32,
    page: u32,
    frame: u32,
    /// Last-known cursor position in screen pixels (for hover box / previews).
    mx: i32,
    my: i32,
    sel: Option<Selection>,
    clip: Option<Clipboard>,
    drag: Drag,
}

impl MapEditor {
    pub fn new() -> Self {
        Self {
            tool: Tool::Draw,
            brush: 1,
            cam_x: 0,
            cam_y: 0,
            page: 0,
            frame: 0,
            mx: -16,
            my: -16,
            sel: None,
            clip: None,
            drag: Drag::None,
        }
    }

    pub fn key(&mut self, key: Key, _mods: Mods, _assets: &mut Assets) {
        match key {
            Key::Left => self.cam_x = (self.cam_x - 1).max(0),
            Key::Right => self.cam_x = (self.cam_x + 1).min(MAP_W as i32 - VIEW_TILES_X),
            Key::Up => self.cam_y = (self.cam_y - 1).max(0),
            Key::Down => self.cam_y = (self.cam_y + 1).min(MAP_H as i32 - VIEW_TILES_Y),
            Key::PageUp => self.page = (self.page + 3) % 4,
            Key::PageDown => self.page = (self.page + 1) % 4,
            Key::Char('d') => self.tool = Tool::Draw,
            Key::Char('t') => self.tool = Tool::Paste,
            Key::Char('s') => self.tool = Tool::Select,
            Key::Char('h') => self.tool = Tool::Pan,
            Key::Char('f') => self.tool = Tool::Fill,
            Key::Char('c') => self.tool = Tool::Circle,
            _ => {}
        }
    }

    // `_assets` is unused until Task 3 adds the per-tool match; the signature
    // must stay `&mut Assets` for the shell's `map_ed.tick(&mouse, a)` call.
    pub fn tick(&mut self, mouse: &Mouse, _assets: &mut Assets) {
        self.frame = self.frame.wrapping_add(1);
        self.mx = mouse.x;
        self.my = mouse.y;

        // Toolbar / sheet / page-dot clicks take priority over map drags.
        if mouse.left_pressed && self.handle_chrome_click(mouse) {
            return;
        }
        // Per-tool map interaction is added in later tasks.
    }

    /// Handle a click on the toolbar, page dots, or sprite sheet. Returns true
    /// if the click was consumed.
    fn handle_chrome_click(&mut self, m: &Mouse) -> bool {
        // Tool icons.
        for (i, (tool, _)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            if m.over(x, TOOLBAR_Y + 1, x + 7, TOOLBAR_Y + 8) {
                self.tool = *tool;
                return true;
            }
        }
        // Page dots.
        for p in 0..4 {
            let x = PAGE_X + p * 6;
            if m.over(x, PAGE_Y, x + 4, PAGE_Y + 4) {
                self.page = p as u32;
                return true;
            }
        }
        // Sprite sheet: pick the brush.
        if m.over(0, SHEET_Y, 127, SHEET_BOTTOM) {
            let cx = m.x / 8;
            let cy = (m.y - SHEET_Y) / 8;
            self.brush = (self.page * 64 + cy as u32 * 16 + cx as u32) % 256;
            return true;
        }
        false
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        self.draw_toolbar(fb, assets);
        self.draw_view(fb, assets);
        fb.rectfill(0, SEP_Y, 127, SEP_Y + 1, col::LIGHT_GREY);
        self.draw_sheet(fb, assets);
        self.draw_status(fb, assets);
    }

    fn draw_toolbar(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, TOOLBAR_Y, 127, VIEW_Y - 1, col::DARK_GREY);
        for (i, (tool, icon)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            let color = if *tool == self.tool {
                fb.rectfill(x - 1, TOOLBAR_Y, x + 8, TOOLBAR_Y + 8, col::BLACK);
                col::WHITE
            } else {
                col::DARK_PURPLE
            };
            draw_icon8(fb, icon, x, TOOLBAR_Y + 1, color);
        }
        // Brush preview (the selected sprite, drawn as-is).
        let (ox, oy) = sprite_origin(self.brush);
        for py in 0..8 {
            for px in 0..8 {
                fb.pset(
                    BRUSH_X + px,
                    TOOLBAR_Y + 1 + py,
                    assets.sprites.get(ox + px, oy + py),
                );
            }
        }
        fb.rect(
            BRUSH_X - 1,
            TOOLBAR_Y,
            BRUSH_X + 8,
            TOOLBAR_Y + 9,
            col::BLACK,
        );
        // Page dots.
        for p in 0..4u32 {
            let x = PAGE_X + p as i32 * 6;
            let c = if p == self.page {
                col::WHITE
            } else {
                col::DARK_PURPLE
            };
            fb.rectfill(x, PAGE_Y, x + 4, PAGE_Y + 4, c);
        }
    }

    fn draw_view(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, VIEW_Y, 127, VIEW_BOTTOM, col::BLACK);
        fb.map(
            &assets.map,
            &assets.sprites,
            self.cam_x,
            self.cam_y,
            0,
            VIEW_Y,
            VIEW_TILES_X,
            VIEW_TILES_Y,
            0,
        );
    }

    fn draw_sheet(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, SHEET_Y, 127, SHEET_BOTTOM, col::DARK_GREY);
        for cy in 0..4i32 {
            for cx in 0..16i32 {
                let n = self.page * 64 + (cy * 16 + cx) as u32;
                let (sx, sy) = sprite_origin(n);
                for py in 0..8 {
                    for px in 0..8 {
                        fb.pset(
                            cx * 8 + px,
                            SHEET_Y + cy * 8 + py,
                            assets.sprites.get(sx + px, sy + py),
                        );
                    }
                }
            }
        }
        if self.brush / 64 == self.page {
            let i = self.brush % 64;
            let x = (i as i32 % 16) * 8;
            let y = SHEET_Y + (i as i32 / 16) * 8;
            fb.rect(x, y, x + 7, y + 7, col::WHITE);
        }
    }

    fn draw_status(&self, fb: &mut Framebuffer, assets: &Assets) {
        let text = match self.hovered_cell() {
            Some((cx, cy)) => format!(
                "x{:03} y{:03} t{:03} b{:03} pg{}",
                cx,
                cy,
                assets.map.get(cx, cy),
                self.brush,
                self.page
            ),
            None => format!("b{:03} pg{}", self.brush, self.page),
        };
        ui::status_bar(fb, &text);
    }

    /// The map cell under the cursor, if the cursor is over the view.
    fn hovered_cell(&self) -> Option<(i32, i32)> {
        if self.mx < 0 || self.mx > 127 || self.my < VIEW_Y || self.my > VIEW_BOTTOM {
            return None;
        }
        Some((
            self.cam_x + self.mx / 8,
            self.cam_y + (self.my - VIEW_Y) / 8,
        ))
    }
}

fn sprite_origin(n: u32) -> (i32, i32) {
    (
        (n as i32 % SPRITES_PER_ROW as i32) * 8,
        (n as i32 / SPRITES_PER_ROW as i32) * 8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::assets::Assets;

    fn press(x: i32, y: i32) -> Mouse {
        Mouse {
            x,
            y,
            left: true,
            left_pressed: true,
            ..Default::default()
        }
    }

    #[test]
    fn clicking_a_tool_slot_selects_it() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        // Slot 2 is Select (0=Draw,1=Paste,2=Select).
        ed.tick(&press(tool_x(2) + 1, 10), &mut a);
        assert_eq!(ed.tool, Tool::Select);
    }

    #[test]
    fn clicking_a_page_dot_sets_the_page() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.tick(&press(PAGE_X + 2 * 6 + 1, PAGE_Y + 1), &mut a);
        assert_eq!(ed.page, 2);
    }

    #[test]
    fn clicking_the_sheet_sets_the_brush() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.page = 1; // page 1 starts at sprite 64.
                     // Second row, third column of the strip -> 64 + 1*16 + 2 = 82.
        ed.tick(&press(2 * 8 + 1, SHEET_Y + 8 + 1), &mut a);
        assert_eq!(ed.brush, 82);
    }
}
