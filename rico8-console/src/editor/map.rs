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

    /// Switch the active tool, abandoning any in-progress drag.
    fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.drag = Drag::None;
    }

    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets) {
        match key {
            Key::Left => self.cam_x = (self.cam_x - 1).max(0),
            Key::Right => self.cam_x = (self.cam_x + 1).min(MAP_W as i32 - VIEW_TILES_X),
            Key::Up => self.cam_y = (self.cam_y - 1).max(0),
            Key::Down => self.cam_y = (self.cam_y + 1).min(MAP_H as i32 - VIEW_TILES_Y),
            Key::PageUp => self.page = (self.page + 3) % 4,
            Key::PageDown => self.page = (self.page + 1) % 4,
            Key::Char('c') if mods.ctrl => self.copy_selection(assets, false),
            Key::Char('x') if mods.ctrl => self.copy_selection(assets, true),
            Key::Delete | Key::Backspace => self.delete_selection(assets),
            Key::Char('d') => self.set_tool(Tool::Draw),
            Key::Char('t') => self.set_tool(Tool::Paste),
            Key::Char('s') => self.set_tool(Tool::Select),
            Key::Char('h') => self.set_tool(Tool::Pan),
            Key::Char('f') => self.set_tool(Tool::Fill),
            Key::Char('c') => self.set_tool(Tool::Circle),
            _ => {}
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets) {
        self.frame = self.frame.wrapping_add(1);
        self.mx = mouse.x;
        self.my = mouse.y;

        // Toolbar / sheet / page-dot clicks take priority over map drags.
        if mouse.left_pressed && self.handle_chrome_click(mouse) {
            return;
        }

        // Per-tool map interaction.
        match self.tool {
            Tool::Draw => {
                if mouse.left {
                    if let Some((cx, cy)) = self.hovered_cell() {
                        assets.map.set(cx, cy, self.brush as u8);
                    }
                }
            }
            Tool::Paste => {
                if mouse.left_pressed {
                    if let Some((cx, cy)) = self.hovered_cell() {
                        self.paste_at(assets, cx, cy);
                    }
                }
            }
            Tool::Select => {
                if mouse.left_pressed && mouse.y >= VIEW_Y && mouse.y <= VIEW_BOTTOM {
                    let (cx, cy) = self.clamped_cell();
                    if self.point_in_selection(cx, cy) {
                        let s = self.sel.unwrap();
                        let mut tiles = Vec::with_capacity((s.w * s.h) as usize);
                        for y in 0..s.h {
                            for x in 0..s.w {
                                tiles.push(assets.map.get(s.x + x, s.y + y));
                            }
                        }
                        self.drag = Drag::Moving {
                            tiles,
                            w: s.w,
                            h: s.h,
                            gx: cx,
                            gy: cy,
                        };
                    } else {
                        self.drag = Drag::Selecting { ax: cx, ay: cy };
                    }
                }
                if !mouse.left {
                    match std::mem::replace(&mut self.drag, Drag::None) {
                        Drag::Selecting { ax, ay } => {
                            let (cx, cy) = self.clamped_cell();
                            let (x, y, w, h) = normalize_rect(ax, ay, cx, cy);
                            self.sel = Some(Selection { x, y, w, h });
                        }
                        Drag::Moving {
                            tiles,
                            w,
                            h,
                            gx,
                            gy,
                        } => {
                            let s = self.sel.unwrap();
                            let (cx, cy) = self.clamped_cell();
                            let (nx, ny) = (s.x + (cx - gx), s.y + (cy - gy));
                            for y in 0..h {
                                for x in 0..w {
                                    assets.map.set(s.x + x, s.y + y, 0);
                                }
                            }
                            for y in 0..h {
                                for x in 0..w {
                                    let (dx, dy) = (nx + x, ny + y);
                                    if (0..MAP_W as i32).contains(&dx)
                                        && (0..MAP_H as i32).contains(&dy)
                                    {
                                        assets.map.set(dx, dy, tiles[(y * w + x) as usize]);
                                    }
                                }
                            }
                            self.sel = Some(Selection { x: nx, y: ny, w, h });
                        }
                        // A non-Select drag (e.g. a stale Shape/Pan) is put back untouched.
                        other => self.drag = other,
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle a click on the toolbar, page dots, or sprite sheet. Returns true
    /// if the click was consumed.
    fn handle_chrome_click(&mut self, m: &Mouse) -> bool {
        // Tool icons.
        for (i, (tool, _)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            if m.over(x, TOOLBAR_Y + 1, x + 7, TOOLBAR_Y + 8) {
                self.set_tool(*tool);
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
        self.draw_selection(fb);
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
        if let Some((cx, cy)) = self.hovered_cell() {
            let sx = (cx - self.cam_x) * 8;
            let sy = VIEW_Y + (cy - self.cam_y) * 8;
            fb.rect(sx, sy, sx + 7, sy + 7, col::WHITE);
        }
    }

    fn draw_selection(&self, fb: &mut Framebuffer) {
        let rect = self
            .selection_in_progress()
            .or_else(|| self.sel.map(|s| (s.x, s.y, s.w, s.h)));
        let Some((x, y, w, h)) = rect else { return };
        let sx = (x - self.cam_x) * 8;
        let sy = VIEW_Y + (y - self.cam_y) * 8;
        let (x0, y0, x1, y1) = (sx, sy, sx + w * 8 - 1, sy + h * 8 - 1);
        let mut i = 0i32;
        let ant = |fb: &mut Framebuffer, px: i32, py: i32, i: &mut i32| {
            if (0..=127).contains(&px) && (VIEW_Y..=VIEW_BOTTOM).contains(&py) {
                let on = ((*i + (self.frame / 4) as i32) / 2) % 2 == 0;
                fb.pset(px, py, if on { col::WHITE } else { col::BLACK });
            }
            *i += 1;
        };
        for px in x0..=x1 {
            ant(fb, px, y0, &mut i);
        }
        for py in (y0 + 1)..=y1 {
            ant(fb, x1, py, &mut i);
        }
        for px in (x0..x1).rev() {
            ant(fb, px, y1, &mut i);
        }
        for py in ((y0 + 1)..y1).rev() {
            ant(fb, x0, py, &mut i);
        }
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
        let text = if let Some((_, _, w, h)) = self.selection_in_progress() {
            format!("sel {}x{}        b{:03} pg{}", w, h, self.brush, self.page)
        } else {
            match self.hovered_cell() {
                Some((cx, cy)) => format!(
                    "x{:03} y{:03} t{:03} b{:03} pg{}",
                    cx,
                    cy,
                    assets.map.get(cx, cy),
                    self.brush,
                    self.page
                ),
                None => format!("b{:03} pg{}", self.brush, self.page),
            }
        };
        ui::status_bar(fb, &text);
    }

    /// The cursor's map cell, clamped to the visible view (for drags).
    fn clamped_cell(&self) -> (i32, i32) {
        let mx = self.mx.clamp(0, 127);
        let my = self.my.clamp(VIEW_Y, VIEW_BOTTOM);
        (self.cam_x + mx / 8, self.cam_y + (my - VIEW_Y) / 8)
    }

    /// The rectangle (x, y, w, h) of an in-progress selection drag, if any.
    fn selection_in_progress(&self) -> Option<(i32, i32, i32, i32)> {
        if let Drag::Selecting { ax, ay } = self.drag {
            let (cx, cy) = self.clamped_cell();
            Some(normalize_rect(ax, ay, cx, cy))
        } else {
            None
        }
    }

    /// Copy (or cut) the current selection into the clipboard.
    fn copy_selection(&mut self, assets: &mut Assets, cut: bool) {
        let Some(s) = self.sel else { return };
        let mut tiles = Vec::with_capacity((s.w * s.h) as usize);
        for y in 0..s.h {
            for x in 0..s.w {
                tiles.push(assets.map.get(s.x + x, s.y + y));
            }
        }
        self.clip = Some(Clipboard {
            w: s.w,
            h: s.h,
            tiles,
        });
        if cut {
            // Cut clears the source cells but keeps the selection rectangle.
            for y in 0..s.h {
                for x in 0..s.w {
                    assets.map.set(s.x + x, s.y + y, 0);
                }
            }
        }
    }

    /// Clear the current selection to tile 0 without touching the clipboard. The
    /// selection itself is kept, so it can be re-filled, moved, or pasted over.
    fn delete_selection(&self, assets: &mut Assets) {
        let Some(s) = self.sel else { return };
        for y in 0..s.h {
            for x in 0..s.w {
                assets.map.set(s.x + x, s.y + y, 0);
            }
        }
    }

    /// Stamp the clipboard at map cell (cx, cy).
    fn paste_at(&self, assets: &mut Assets, cx: i32, cy: i32) {
        let Some(clip) = &self.clip else { return };
        for y in 0..clip.h {
            for x in 0..clip.w {
                let (dx, dy) = (cx + x, cy + y);
                if (0..MAP_W as i32).contains(&dx) && (0..MAP_H as i32).contains(&dy) {
                    assets
                        .map
                        .set(dx, dy, clip.tiles[(y * clip.w + x) as usize]);
                }
            }
        }
    }

    /// Whether map cell (cx, cy) is inside the current selection.
    fn point_in_selection(&self, cx: i32, cy: i32) -> bool {
        match self.sel {
            Some(s) => cx >= s.x && cx < s.x + s.w && cy >= s.y && cy < s.y + s.h,
            None => false,
        }
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

/// Two corner cells into a top-left origin and inclusive (w, h).
fn normalize_rect(ax: i32, ay: i32, bx: i32, by: i32) -> (i32, i32, i32, i32) {
    let x = ax.min(bx);
    let y = ay.min(by);
    (x, y, (ax - bx).abs() + 1, (ay - by).abs() + 1)
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

    #[test]
    fn draw_tool_paints_the_hovered_cell() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.brush = 7;
        // Hover cell (1, 0): x in [8,15], y in [VIEW_Y, VIEW_Y+7].
        ed.tick(&press(9, VIEW_Y + 1), &mut a);
        assert_eq!(a.map.get(1, 0), 7);
    }

    #[test]
    fn draw_respects_the_camera_offset() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.brush = 5;
        ed.cam_x = 10;
        ed.cam_y = 4;
        let held = Mouse {
            x: 0,
            y: VIEW_Y,
            left: true,
            ..Default::default()
        };
        ed.tick(&held, &mut a);
        assert_eq!(a.map.get(10, 4), 5);
    }

    #[test]
    fn hover_box_outlines_the_cell_under_the_cursor() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        let hover = Mouse {
            x: 9,
            y: VIEW_Y + 1,
            ..Default::default()
        };
        ed.tick(&hover, &mut a);
        let mut fb = Framebuffer::new();
        ed.draw(&mut fb, &a);
        // Cell (1,0) screen rect top-left corner at (8, VIEW_Y).
        assert_eq!(fb.pget(8, VIEW_Y), col::WHITE);
    }

    // A held drag frame (button down, no fresh press edge).
    fn held(x: i32, y: i32) -> Mouse {
        Mouse {
            x,
            y,
            left: true,
            ..Default::default()
        }
    }
    // A release frame: the real input keeps the cursor position and only drops
    // `left` (the shell updates x/y on CursorMoved, not MouseInput), so release at
    // the drag-end coordinates — NOT off-screen.
    fn rel(x: i32, y: i32) -> Mouse {
        Mouse {
            x,
            y,
            ..Default::default()
        }
    }

    #[test]
    fn select_drag_builds_a_normalized_selection() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.tool = Tool::Select;
        // Press at cell (3,1), drag up-left to cell (1,0), release there.
        ed.tick(&press(3 * 8 + 1, VIEW_Y + 1 * 8 + 1), &mut a);
        ed.tick(&held(1 * 8 + 1, VIEW_Y + 1), &mut a);
        ed.tick(&rel(1 * 8 + 1, VIEW_Y + 1), &mut a);
        let s = ed.sel.expect("selection set");
        assert_eq!((s.x, s.y, s.w, s.h), (1, 0, 3, 2));
    }

    #[test]
    fn selecting_shows_size_in_status_bar() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.tool = Tool::Select;
        ed.tick(&press(8 + 1, VIEW_Y + 1), &mut a); // cell (1,0).
        ed.tick(&held(3 * 8 + 1, VIEW_Y + 8 + 1), &mut a); // drag to (3,1).
        assert!(matches!(ed.drag, Drag::Selecting { .. }));
        assert_eq!(ed.selection_in_progress(), Some((1, 0, 3, 2)));
    }

    #[test]
    fn switching_tool_abandons_an_in_progress_select_drag() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.tool = Tool::Select;
        ed.tick(&press(8 + 1, VIEW_Y + 1), &mut a); // start a marquee at cell (1,0).
        assert!(matches!(ed.drag, Drag::Selecting { .. }));
        ed.key(Key::Char('d'), Mods::default(), &mut a); // switch tools mid-drag.
        assert!(matches!(ed.drag, Drag::None), "tool switch clears the drag");
        // Back in Select with the button up: must not commit a phantom selection.
        ed.key(Key::Char('s'), Mods::default(), &mut a);
        ed.tick(&rel(40, VIEW_Y + 40), &mut a);
        assert!(ed.sel.is_none(), "no phantom selection committed");
    }

    fn make_selection(ed: &mut MapEditor, a: &mut Assets, x: i32, y: i32, w: i32, h: i32) {
        ed.tool = Tool::Select;
        ed.tick(&press(x * 8 + 1, VIEW_Y + y * 8 + 1), a);
        ed.tick(&held((x + w - 1) * 8 + 1, VIEW_Y + (y + h - 1) * 8 + 1), a);
        ed.tick(&rel((x + w - 1) * 8 + 1, VIEW_Y + (y + h - 1) * 8 + 1), a);
    }

    #[test]
    fn copy_then_paste_reproduces_the_block() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        a.map.set(2, 1, 11);
        a.map.set(3, 1, 12);
        make_selection(&mut ed, &mut a, 2, 1, 2, 1);
        ed.key(
            Key::Char('c'),
            Mods {
                ctrl: true,
                ..Default::default()
            },
            &mut a,
        );
        let clip = ed.clip.clone().expect("clipboard filled");
        assert_eq!((clip.w, clip.h, clip.tiles), (2, 1, vec![11, 12]));
        // Paste at cell (5, 4).
        ed.tool = Tool::Paste;
        ed.tick(&press(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
        assert_eq!(a.map.get(5, 4), 11);
        assert_eq!(a.map.get(6, 4), 12);
    }

    #[test]
    fn cut_clears_the_source() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        a.map.set(2, 1, 11);
        make_selection(&mut ed, &mut a, 2, 1, 1, 1);
        ed.key(
            Key::Char('x'),
            Mods {
                ctrl: true,
                ..Default::default()
            },
            &mut a,
        );
        assert_eq!(a.map.get(2, 1), 0);
        assert_eq!(ed.clip.as_ref().unwrap().tiles, vec![11]);
    }

    #[test]
    fn delete_clears_without_touching_the_clipboard() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        a.map.set(2, 1, 11);
        make_selection(&mut ed, &mut a, 2, 1, 1, 1);
        ed.key(Key::Delete, Mods::default(), &mut a);
        assert_eq!(a.map.get(2, 1), 0);
        assert!(ed.clip.is_none());
    }

    #[test]
    fn dragging_inside_a_selection_moves_the_block() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        a.map.set(2, 1, 11);
        make_selection(&mut ed, &mut a, 2, 1, 1, 1);
        // Press inside the selection (cell 2,1), drag to cell (5,4), release there.
        ed.tick(&press(2 * 8 + 1, VIEW_Y + 1 * 8 + 1), &mut a);
        ed.tick(&held(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
        ed.tick(&rel(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
        assert_eq!(a.map.get(2, 1), 0, "source cleared");
        assert_eq!(a.map.get(5, 4), 11, "block moved");
        let s = ed.sel.expect("selection follows the move");
        assert_eq!((s.x, s.y, s.w, s.h), (5, 4, 1, 1));
    }
}
