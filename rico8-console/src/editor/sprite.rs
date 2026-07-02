//! The sprite editor: a zoomed canvas (editing 1x1 up to 8x8 sprite blocks),
//! palette grid, tools, flags, and a sheet strip for picking sprites — the
//! classic layout.

use super::history::History;
use crate::{
    shell::{Key, Mods},
    ui::{self, draw_icon8, Icon8, Mouse, ICON_PENCIL},
};
use rico8_runtime::{
    assets::{Assets, SpriteSheet, SPRITES_PER_ROW},
    clipboard::{self, ClipboardPayload, Pasted},
    fb::Framebuffer,
    palette::col,
};

// Layout.
const CANVAS: (i32, i32) = (3, 20); // always 64x64 on screen, zoom set by size
const PAL: (i32, i32) = (74, 20); // 4x4 grid of 12px swatches
const FLAGS: (i32, i32) = (76, 72); // 8 toggle dots
const SHEET_Y: i32 = 88; // 4 rows of sprites (one page)
const PAGE_BTNS: (i32, i32) = (104, 81); // 4 page dots
const SIZE_BTNS: (i32, i32) = (53, 9); // four "1/2/4/8" block-size buttons

// Fullscreen canvas: anchored at (8, 8), zoom chosen to fit the ~112px area.
const FS_CANVAS: (i32, i32) = (8, 8);
const FS_MAX: i32 = 112;

/// Selectable block sizes, in sprites per side: 1x1, 2x2, 4x4 or 8x8 sprites
/// (8, 16, 32 or 64 pixels). Larger blocks edit several adjacent sheet cells
/// at once, PICO-8 style.
const SIZES: [i32; 4] = [1, 2, 4, 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
    /// Drag to slide the canvas window across the sheet (PICO-8's hand).
    Pan,
}

/// The toolbar, left to right. Icons are stored by value (`Icon8` is `Copy`);
/// each slot sits at `x = 3 + i * 10`, matching the click/hover hit tests.
const TOOLS: [(Tool, Icon8); 5] = [
    (Tool::Pencil, ICON_PENCIL),
    (Tool::Eraser, ICON_ERASER),
    (Tool::Fill, ICON_FILL),
    (Tool::Picker, ICON_PICKER),
    (Tool::Pan, ICON_HAND),
];

/// The x of toolbar slot `i`.
fn tool_x(i: usize) -> i32 {
    3 + i as i32 * 10
}

/// An in-progress pan (hand tool): the screen point grabbed on press and the
/// window's top-left cell then, so each frame re-derives the scroll from the
/// original anchor rather than accumulating.
#[derive(Clone, Copy)]
struct PanDrag {
    /// Screen pixel grabbed on press.
    amx: i32,
    amy: i32,
    /// Window top-left cell (column, row) at press time.
    acol: i32,
    arow: i32,
}

pub struct SpriteEditor {
    sprite: u32,
    color: u8,
    tool: Tool,
    page: u32,
    /// Edited block size in sprites per side (one of [`SIZES`]).
    size: i32,
    fullscreen: bool,
    /// Last-known cursor position in screen pixels (for hover / status bar).
    mx: i32,
    my: i32,
    /// Transient bottom-bar message (e.g. a paste result).
    status: ui::StatusMsg,
    /// In-progress hand-tool pan, if any.
    pan: Option<PanDrag>,
    /// Undo/redo of the sprite sheet (last 10 edits).
    history: History<SpriteSheet>,
}

impl SpriteEditor {
    pub fn new() -> Self {
        Self {
            sprite: 1,
            color: 7,
            tool: Tool::Pencil,
            page: 0,
            size: 1,
            fullscreen: false,
            mx: -16,
            my: -16,
            status: ui::StatusMsg::default(),
            pan: None,
            history: History::new(),
        }
    }

    /// Whether the fullscreen (bare-canvas) view is active.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Edited block size in pixels (8, 16, 32 or 64).
    fn block_px(&self) -> i32 {
        self.size * 8
    }

    /// Top-left cell (column, row) of the edited block, clamped so an N-sprite
    /// block always fits inside the 16x16 sheet even when the cursor sprite sits
    /// near the right or bottom edge.
    fn origin_cell(&self) -> (i32, i32) {
        let per_row = SPRITES_PER_ROW as i32;
        let col = (self.sprite as i32 % per_row).min(per_row - self.size);
        let row = (self.sprite as i32 / per_row).min(per_row - self.size);
        (col, row)
    }

    /// The sprite number of the block's top-left cell (what the header shows).
    fn top_left_sprite(&self) -> u32 {
        let (col, row) = self.origin_cell();
        (row * SPRITES_PER_ROW as i32 + col) as u32
    }

    /// Canvas origin and per-pixel zoom for the active view. The block is
    /// magnified into a fixed 64px canvas (normal) or the ~112px fullscreen area,
    /// so larger blocks simply zoom out.
    fn canvas(&self) -> (i32, i32, i32) {
        let bpx = self.block_px();
        if self.fullscreen {
            (FS_CANVAS.0, FS_CANVAS.1, (FS_MAX / bpx).max(1))
        } else {
            (CANVAS.0, CANVAS.1, 64 / bpx)
        }
    }

    fn sheet_origin(&self) -> (i32, i32) {
        let (col, row) = self.origin_cell();
        (col * 8, row * 8)
    }

    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets) {
        if mods.ctrl {
            if let Key::Char(c) = key {
                match c.to_ascii_lowercase() {
                    'z' if mods.shift => {
                        self.history.redo(&mut assets.sprites);
                        return;
                    }
                    'z' => {
                        self.history.undo(&mut assets.sprites);
                        return;
                    }
                    'y' => {
                        self.history.redo(&mut assets.sprites);
                        return;
                    }
                    _ => {}
                }
            }
        }
        let mut moved = false;
        match key {
            Key::Left => {
                self.sprite = (self.sprite + 255) % 256;
                moved = true;
            }
            Key::Right => {
                self.sprite = (self.sprite + 1) % 256;
                moved = true;
            }
            Key::Up => {
                self.sprite = (self.sprite + 240) % 256;
                moved = true;
            }
            Key::Down => {
                self.sprite = (self.sprite + 16) % 256;
                moved = true;
            }
            Key::PageUp => self.page = (self.page + 3) % 4,
            Key::PageDown => self.page = (self.page + 1) % 4,
            Key::Char('p') => self.tool = Tool::Pencil,
            Key::Char('e') => self.tool = Tool::Eraser,
            Key::Char('f') => self.tool = Tool::Fill,
            Key::Char('i') => self.tool = Tool::Picker,
            Key::Char('h') => self.tool = Tool::Pan,
            Key::Tab => self.fullscreen = !self.fullscreen,
            _ => {}
        }
        // Keep the strip showing the selected sprite when moved by keys.
        if moved {
            self.page = self.sprite / 64;
        }
    }

    fn apply_tool(&mut self, assets: &mut Assets, px: i32, py: i32, right: bool) {
        let (ox, oy) = self.sheet_origin();
        let (sx, sy) = (ox + px, oy + py);
        if right {
            // Right-click always picks up the color under the cursor.
            self.color = assets.sprites.get(sx, sy);
            return;
        }
        match self.tool {
            Tool::Pencil => assets.sprites.set(sx, sy, self.color),
            Tool::Eraser => assets.sprites.set(sx, sy, 0),
            Tool::Picker => {
                self.color = assets.sprites.get(sx, sy);
                self.tool = Tool::Pencil;
            }
            Tool::Fill => {
                let target = assets.sprites.get(sx, sy);
                if target == self.color {
                    return;
                }
                // Flood fill within the edited block (which may span several cells).
                let bpx = self.block_px();
                let mut stack = vec![(px, py)];
                while let Some((x, y)) = stack.pop() {
                    if !(0..bpx).contains(&x) || !(0..bpx).contains(&y) {
                        continue;
                    }
                    if assets.sprites.get(ox + x, oy + y) != target {
                        continue;
                    }
                    assets.sprites.set(ox + x, oy + y, self.color);
                    stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
                }
            }
            // Pan is a drag gesture handled in `handle_pan`, not a per-pixel op.
            Tool::Pan => {}
        }
    }

    /// Hand-tool drag: slide the canvas window across the sheet, grabbing a
    /// point and dragging it so adjacent cells scroll into view (PICO-8's pan,
    /// like the map editor's). The window is a block of cells, so panning moves
    /// the top-left cell (`self.sprite`), clamped to keep the block on the sheet.
    /// It never touches pixels, so it records no undo step.
    fn handle_pan(&mut self, m: &Mouse, z: i32, over: bool) {
        if m.left_pressed && over {
            let (col, row) = self.origin_cell();
            self.pan = Some(PanDrag {
                amx: m.x,
                amy: m.y,
                acol: col,
                arow: row,
            });
        }
        let Some(p) = self.pan else { return };
        if !m.left {
            self.pan = None;
            return;
        }
        // One sheet cell spans `z * 8` screen pixels; dividing the drag by that
        // scrolls a cell each time the cursor crosses a cell-worth of distance.
        let cell = z * 8;
        let per_row = SPRITES_PER_ROW as i32;
        let col = (p.acol + (p.amx - m.x).div_euclid(cell)).clamp(0, per_row - self.size);
        let row = (p.arow + (p.amy - m.y).div_euclid(cell)).clamp(0, per_row - self.size);
        self.sprite = (row * per_row + col) as u32;
        self.page = self.sprite / 64;
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets) {
        self.status.tick();
        self.mx = mouse.x;
        self.my = mouse.y;
        let m = *mouse;
        // Bracket each gesture for undo. Painting happens on the press/hold
        // frames (never on release), so a snapshot taken while the button is
        // down and committed once it comes up captures the whole stroke. The
        // commit compares against the snapshot, so non-drawing clicks (palette,
        // tools, picker) record nothing.
        if m.left || m.right {
            self.history.begin(&assets.sprites);
        } else {
            self.history.commit(&assets.sprites);
        }
        // View-toggle buttons in the top bar (normal | fullscreen).
        if m.left_pressed && m.y < 8 {
            if m.over(4, 0, 12, 7) {
                self.fullscreen = false;
                return;
            } else if m.over(13, 0, 22, 7) {
                self.fullscreen = true;
                return;
            }
        }
        // Canvas painting (drag-friendly), through the active view's origin/zoom.
        let (cx, cy, z) = self.canvas();
        let size = z * self.block_px();
        let over_canvas = m.over(cx, cy, cx + size - 1, cy + size - 1);
        if matches!(self.tool, Tool::Pan) {
            self.handle_pan(&m, z, over_canvas);
        } else if (m.left || m.right) && over_canvas {
            let px = (m.x - cx) / z;
            let py = (m.y - cy) / z;
            // Fill and picker should fire once per click, not per frame.
            let one_shot = matches!(self.tool, Tool::Fill | Tool::Picker);
            if !one_shot || m.left_pressed || m.right_pressed {
                self.apply_tool(assets, px, py, m.right && !m.left);
            }
        }
        // The palette, tools, flags, page dots and sheet exist only in the
        // normal view; fullscreen is a bare canvas.
        if !m.left_pressed || self.fullscreen {
            return;
        }
        // Palette.
        if m.over(PAL.0, PAL.1, PAL.0 + 47, PAL.1 + 47) {
            let c = ((m.y - PAL.1) / 12) * 4 + (m.x - PAL.0) / 12;
            self.color = c as u8;
        }
        // Tool icons.
        for (i, (tool, _)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            if m.over(x, 9, x + 7, 17) {
                self.tool = *tool;
            }
        }
        // Block-size buttons.
        for (i, &n) in SIZES.iter().enumerate() {
            let x = SIZE_BTNS.0 + i as i32 * 8;
            if m.over(x, SIZE_BTNS.1, x + 6, SIZE_BTNS.1 + 7) {
                self.size = n;
            }
        }
        // Flags (of the block's top-left sprite).
        let flag_sprite = self.top_left_sprite();
        for f in 0..8 {
            let x = FLAGS.0 + f * 6;
            if m.over(x, FLAGS.1, x + 4, FLAGS.1 + 4) {
                let cur = assets.sprites.flags(flag_sprite);
                assets
                    .sprites
                    .set_flag(flag_sprite, f as u8, cur & (1 << f) == 0);
            }
        }
        // Page dots.
        for p in 0..4 {
            let x = PAGE_BTNS.0 + p * 6;
            if m.over(x, PAGE_BTNS.1, x + 4, PAGE_BTNS.1 + 5) {
                self.page = p as u32;
            }
        }
        // Sheet strip: select sprite.
        if m.over(0, SHEET_Y, 127, SHEET_Y + 31) {
            let cx = m.x / 8;
            let cy = (m.y - SHEET_Y) / 8;
            self.sprite = (self.page * 64 + cy as u32 * 16 + cx as u32) % 256;
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        if self.fullscreen {
            self.draw_fullscreen(fb, assets);
            return;
        }
        // Tool icons.
        for (i, (tool, icon)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            let color = if *tool == self.tool {
                fb.rectfill(x - 1, 9, x + 8, 17, col::BLACK);
                col::WHITE
            } else {
                col::LAVENDER
            };
            draw_icon8(fb, icon, x, 9, color);
        }
        fb.print(
            &format!("#{:03}", self.top_left_sprite()),
            106,
            11,
            col::WHITE,
        );

        // Block-size buttons ("1/2/4/8", one per entry in SIZES).
        for (i, &n) in SIZES.iter().enumerate() {
            let x = SIZE_BTNS.0 + i as i32 * 8;
            let sel = n == self.size;
            let color = if sel {
                fb.rectfill(x - 1, SIZE_BTNS.1, x + 7, SIZE_BTNS.1 + 8, col::BLACK);
                col::WHITE
            } else {
                col::LAVENDER
            };
            fb.print(&n.to_string(), x + 1, SIZE_BTNS.1 + 1, color);
        }

        // Canvas.
        fb.rect(
            CANVAS.0 - 1,
            CANVAS.1 - 1,
            CANVAS.0 + 64,
            CANVAS.1 + 64,
            col::BLACK,
        );
        let (ox, oy) = self.sheet_origin();
        let bpx = self.block_px();
        // Magnify the edited block onto the 64x64 canvas. The artist edits every
        // pixel, so color 0 must show as black here rather than being treated as
        // transparent for this blit.
        fb.set_transparent_color(0, false);
        fb.sspr(
            &assets.sprites,
            ox,
            oy,
            bpx,
            bpx,
            CANVAS.0,
            CANVAS.1,
            64,
            64,
            false,
            false,
        );
        fb.reset_transparency();
        self.draw_canvas_hover(fb);

        // Palette grid.
        for c in 0u8..16 {
            let x = PAL.0 + (c as i32 % 4) * 12;
            let y = PAL.1 + (c as i32 / 4) * 12;
            fb.rectfill(x, y, x + 11, y + 11, c);
        }
        let sx = PAL.0 + (self.color as i32 % 4) * 12;
        let sy = PAL.1 + (self.color as i32 / 4) * 12;
        fb.rect(sx, sy, sx + 11, sy + 11, col::WHITE);
        fb.rect(sx - 1, sy - 1, sx + 12, sy + 12, col::BLACK);

        // Flags.
        let flags = assets.sprites.flags(self.top_left_sprite());
        for f in 0..8 {
            let x = FLAGS.0 + f * 6;
            let on = flags & (1 << f) != 0;
            fb.circfill(
                x + 2,
                FLAGS.1 + 2,
                2,
                if on { col::RED } else { col::LAVENDER },
            );
        }

        // Page dots.
        for p in 0..4u32 {
            let x = PAGE_BTNS.0 + p as i32 * 6;
            let c = if p == self.page {
                col::WHITE
            } else {
                col::LAVENDER
            };
            fb.rectfill(x, PAGE_BTNS.1, x + 4, PAGE_BTNS.1 + 4, c);
        }

        // Sheet strip: 64 sprites of the current page.
        for cy in 0..4i32 {
            for cx in 0..16i32 {
                let n = self.page * 64 + (cy * 16 + cx) as u32;
                let (sx, sy) = (
                    (n as i32 % SPRITES_PER_ROW as i32) * 8,
                    (n as i32 / SPRITES_PER_ROW as i32) * 8,
                );
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
        // Selection box on the strip: the N-sprite block, clipped to the four
        // rows this page shows (an 8-tall block spans two pages).
        let (col, row) = self.origin_cell();
        let page_row0 = self.page as i32 * 4;
        let y0 = (row - page_row0).max(0);
        let y1 = (row + self.size - page_row0).min(4);
        if y1 > y0 {
            let x = col * 8;
            let y = SHEET_Y + y0 * 8;
            fb.rect(
                x,
                y,
                x + self.size * 8 - 1,
                y + (y1 - y0) * 8 - 1,
                col::WHITE,
            );
        }

        self.draw_status(fb, assets);
    }

    /// Fullscreen view: the selected 8x8 sprite magnified to fill the editor
    /// area, with no palette/flags/sheet. The shell paints the dark-grey
    /// background; we draw the sprite (colour 0 shown as black) and the status.
    fn draw_fullscreen(&self, fb: &mut Framebuffer, assets: &Assets) {
        let (ox, oy) = self.sheet_origin();
        let (cx, cy, z) = self.canvas();
        let bpx = self.block_px();
        fb.set_transparent_color(0, false);
        fb.sspr(
            &assets.sprites,
            ox,
            oy,
            bpx,
            bpx,
            cx,
            cy,
            z * bpx,
            z * bpx,
            false,
            false,
        );
        fb.reset_transparency();
        self.draw_canvas_hover(fb);
        self.draw_status(fb, assets);
    }

    /// Outline the magnified pixel block under the cursor, snapped to the block
    /// grid and clamped to the canvas — the pixel-editing selection marker. Not
    /// shown over the sheet strip, which is a sprite picker, not an edit grid.
    fn draw_canvas_hover(&self, fb: &mut Framebuffer) {
        let Some((px, py)) = self.canvas_pixel_under_cursor() else {
            return;
        };
        let (cx, cy, z) = self.canvas();
        let (bx, by) = (cx + px * z, cy + py * z);
        fb.rect(bx, by, bx + z - 1, by + z - 1, col::WHITE);
    }

    /// The tool whose toolbar icon is under the cursor, if any.
    fn tool_under_cursor(&self) -> Option<Tool> {
        // The toolbar is hidden in fullscreen, so no tool can be under the cursor.
        if self.fullscreen {
            return None;
        }
        TOOLS.iter().enumerate().find_map(|(i, &(tool, _))| {
            let x = tool_x(i);
            (self.mx >= x && self.mx <= x + 7 && self.my >= 9 && self.my <= 17).then_some(tool)
        })
    }

    /// The block-local pixel (0..block, 0..block) under the cursor, if any.
    fn canvas_pixel_under_cursor(&self) -> Option<(i32, i32)> {
        let (cx, cy, z) = self.canvas();
        let size = z * self.block_px();
        if self.mx >= cx && self.mx < cx + size && self.my >= cy && self.my < cy + size {
            Some(((self.mx - cx) / z, (self.my - cy) / z))
        } else {
            None
        }
    }

    /// Render the status bar: tool label when hovering a tool icon, pixel info
    /// when hovering the canvas, sprite/flags summary otherwise.
    fn draw_status(&self, fb: &mut Framebuffer, assets: &Assets) {
        let text = if let Some(tool) = self.tool_under_cursor() {
            tool_label(tool).to_string()
        } else if let Some((px, py)) = self.canvas_pixel_under_cursor() {
            let (ox, oy) = self.sheet_origin();
            let c = assets.sprites.get(ox + px, oy + py);
            format!("#{:03} x{} y{} c{:02}", self.top_left_sprite(), px, py, c)
        } else {
            let flags = assets.sprites.flags(self.top_left_sprite());
            format!("Spr {:03} flags {:08b}", self.top_left_sprite(), flags)
        };
        self.status.show(fb, &text);
    }

    /// Set a transient bottom-bar message (used for clipboard errors).
    pub fn set_status(&mut self, msg: String) {
        self.status.set(msg);
    }

    /// Paste a decoded PICO-8 clipboard blob. Only sprite pixels apply here;
    /// other kinds set a hint pointing at the right editor.
    pub fn paste(&mut self, pasted: &Pasted, assets: &mut Assets) {
        self.history.begin(&assets.sprites);
        match pasted {
            Pasted::Sprites { rect, flags } => {
                let (x0, y0) = self.sheet_origin();
                let report =
                    clipboard::paste_sprites(&mut assets.sprites, rect, x0, y0, flags.as_deref());
                self.status.set(report.summary);
            }
            Pasted::Sfx(_) => self.status.set("sfx - use sfx editor".into()),
            Pasted::Map { .. } => self.status.set("map - use map editor".into()),
        }
        self.history.commit(&assets.sprites);
    }

    /// Copy the edited block (pixels + one flag byte per covered sprite,
    /// row-major) as a native blob.
    pub fn copy(&mut self, assets: &Assets) -> String {
        let (x0, y0) = self.sheet_origin();
        let (col, row) = self.origin_cell();
        let bpx = self.block_px();
        let mut pixels = Vec::with_capacity((bpx * bpx) as usize);
        for dy in 0..bpx {
            for dx in 0..bpx {
                pixels.push(assets.sprites.get(x0 + dx, y0 + dy));
            }
        }
        let mut flags = Vec::with_capacity((self.size * self.size) as usize);
        for ry in 0..self.size {
            for rx in 0..self.size {
                let n = (row + ry) * SPRITES_PER_ROW as i32 + (col + rx);
                flags.push(assets.sprites.flags(n as u32));
            }
        }
        let top = self.top_left_sprite();
        self.status.set(if self.size == 1 {
            format!("copied sprite {top}")
        } else {
            format!("copied {bpx}x{bpx} spr {top}")
        });
        clipboard::encode(&ClipboardPayload::Sprite {
            w: bpx as u8,
            h: bpx as u8,
            pixels,
            flags,
        })
    }
}

/// The tool's display name and keyboard shortcut, shown in the status bar.
fn tool_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Pencil => "Pencil (p)",
        Tool::Eraser => "Eraser (e)",
        Tool::Fill => "Fill (f)",
        Tool::Picker => "Picker (i)",
        Tool::Pan => "Pan (h)",
    }
}

const ICON_ERASER: Icon8 = [
    0b00111100, 0b01111110, 0b11111111, 0b11111111, 0b11111111, 0b01111110, 0b00111100, 0b00000000,
];
const ICON_FILL: Icon8 = [
    0b00011000, 0b00111100, 0b01111110, 0b11111111, 0b11111111, 0b01111110, 0b00011000, 0b00010000,
];
const ICON_PICKER: Icon8 = [
    0b00000111, 0b00000111, 0b00001110, 0b00011100, 0b00111000, 0b01110000, 0b01100000, 0b00000000,
];
// A grabbing hand, matching the map editor's pan tool.
const ICON_HAND: Icon8 = [0x28, 0x2A, 0x2A, 0x3E, 0xBE, 0x7E, 0x1C, 0x00];

#[cfg(test)]
mod paste_tests {
    use super::*;
    use rico8_runtime::clipboard::{parse, Pasted, PixelRect};

    #[test]
    fn pastes_pixels_at_selected_sprite() {
        let mut ed = SpriteEditor::new(); // sprite 1 -> sheet (8, 0).
        let mut assets = Assets::default();
        let rect = PixelRect {
            w: 2,
            h: 1,
            pixels: vec![9, 10],
        };
        ed.paste(&Pasted::Sprites { rect, flags: None }, &mut assets);
        assert_eq!(assets.sprites.get(8, 0), 9);
        assert_eq!(assets.sprites.get(9, 0), 10);
        assert!(ed.status.current().unwrap().contains("pasted"));
    }

    #[test]
    fn rejects_sfx_with_a_hint() {
        use rico8_runtime::clipboard::SfxClip;
        let mut ed = SpriteEditor::new();
        let mut assets = Assets::default();
        ed.paste(
            &Pasted::Sfx(SfxClip {
                records: vec![],
                patterns: vec![],
            }),
            &mut assets,
        );
        assert!(ed.status.current().unwrap().contains("sfx"));
        assert_eq!(assets.sprites.get(8, 0), 0); // nothing drawn.
    }

    #[test]
    fn copies_selected_sprite() {
        let mut ed = SpriteEditor::new(); // sprite 1 -> sheet (8, 0).
        let mut assets = Assets::default();
        assets.sprites.set(8, 0, 5);
        assets.sprites.set(15, 7, 9);
        let blob = ed.copy(&assets);
        assert!(ed.status.current().unwrap().contains("copied sprite 1"));
        let Pasted::Sprites { rect: r, .. } = parse(&blob).unwrap() else {
            panic!("not sprites")
        };
        assert_eq!((r.w, r.h), (8, 8));
        assert_eq!(r.pixels[0], 5);
        assert_eq!(r.pixels[63], 9);
    }

    #[test]
    fn undo_and_redo_a_paste() {
        let mut ed = SpriteEditor::new(); // sprite 1 -> sheet (8, 0).
        let mut assets = Assets::default();
        let rect = PixelRect {
            w: 2,
            h: 1,
            pixels: vec![9, 10],
        };
        ed.paste(&Pasted::Sprites { rect, flags: None }, &mut assets);
        assert_eq!(assets.sprites.get(8, 0), 9);

        let ctrl = Mods {
            ctrl: true,
            shift: false,
            ..Default::default()
        };
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        ed.key(Key::Char('z'), ctrl, &mut assets);
        assert_eq!(assets.sprites.get(8, 0), 0, "undo clears the paste");
        assert_eq!(assets.sprites.get(9, 0), 0);
        ed.key(Key::Char('z'), ctrl_shift, &mut assets);
        assert_eq!(assets.sprites.get(8, 0), 9, "redo re-applies the paste");
        assert_eq!(assets.sprites.get(9, 0), 10);
    }

    #[test]
    fn an_incompatible_paste_records_no_undo() {
        use rico8_runtime::clipboard::SfxClip;
        let mut ed = SpriteEditor::new();
        let mut assets = Assets::default();
        assets.sprites.set(8, 0, 5); // a pre-existing pixel the undo must not touch.
        ed.paste(
            &Pasted::Sfx(SfxClip {
                records: vec![],
                patterns: vec![],
            }),
            &mut assets,
        );
        let ctrl = Mods {
            ctrl: true,
            shift: false,
            ..Default::default()
        };
        ed.key(Key::Char('z'), ctrl, &mut assets);
        assert_eq!(
            assets.sprites.get(8, 0),
            5,
            "the hint paste recorded no undo step"
        );
    }
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
    fn tab_toggles_fullscreen() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        assert!(!ed.is_fullscreen());
        ed.key(Key::Tab, Mods::default(), &mut a);
        assert!(ed.is_fullscreen());
        ed.key(Key::Tab, Mods::default(), &mut a);
        assert!(!ed.is_fullscreen());
    }

    #[test]
    fn view_buttons_toggle_fullscreen() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.tick(&press(15, 2), &mut a); // fullscreen button
        assert!(ed.is_fullscreen());
        ed.tick(&press(6, 2), &mut a); // normal button
        assert!(!ed.is_fullscreen());
    }

    #[test]
    fn inactive_page_dot_is_lavender() {
        // Page 0 is active (white) by default; page 1 is inactive and must be
        // lavender on the dark-grey panel.
        let ed = SpriteEditor::new();
        let a = Assets::default();
        let mut fb = Framebuffer::new();
        ed.draw(&mut fb, &a);
        assert_eq!(fb.pget(PAGE_BTNS.0 + 6, PAGE_BTNS.1), col::LAVENDER);
    }

    #[test]
    fn fullscreen_drag_draws_through_the_zoomed_canvas() {
        // Default sprite is 1 (sheet origin (8, 0)), pencil, colour 7. In
        // fullscreen the canvas origin is (8, 8) at 14x zoom, so screen (9, 9)
        // maps to sprite pixel (0, 0) -> sheet pixel (8, 0).
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.key(Key::Tab, Mods::default(), &mut a);
        ed.tick(&press(9, 9), &mut a);
        assert_eq!(a.sprites.get(8, 0), 7);
    }

    #[test]
    fn hovering_a_tool_reports_its_label() {
        // Eraser is slot 1: x = 3 + 1*10 = 13, y in 9..=17.
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        let hover = Mouse {
            x: 13,
            y: 13,
            ..Default::default()
        };
        ed.tick(&hover, &mut a);
        // Hovering does not select the tool.
        assert_eq!(ed.tool, Tool::Pencil);
        assert_eq!(ed.tool_under_cursor(), Some(Tool::Eraser));
        assert_eq!(tool_label(Tool::Eraser), "Eraser (e)");
    }

    #[test]
    fn tool_under_cursor_is_none_in_fullscreen() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        // Toggle fullscreen.
        ed.key(Key::Tab, Mods::default(), &mut a);
        // Hover where the Eraser icon would be in normal view.
        let hover = Mouse {
            x: 13,
            y: 13,
            ..Default::default()
        };
        ed.tick(&hover, &mut a);
        assert_eq!(ed.tool_under_cursor(), None);
    }

    #[test]
    fn canvas_pixel_maps_screen_to_sprite_pixel() {
        // Normal view: canvas origin (3, 20), zoom 8.
        // Screen (3 + 8*2 + 1, 20 + 8*3 + 1) = (20, 45) -> sprite pixel (2, 3).
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        let hover = Mouse {
            x: 3 + 8 * 2 + 1,
            y: 20 + 8 * 3 + 1,
            ..Default::default()
        };
        ed.tick(&hover, &mut a);
        assert_eq!(ed.canvas_pixel_under_cursor(), Some((2, 3)));
    }

    #[test]
    fn canvas_hover_outlines_the_snapped_pixel_block() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        // Hover sprite pixel (2, 3): canvas origin (3, 20), zoom 8 -> block
        // top-left (3 + 2*8, 20 + 3*8) = (19, 44).
        let hover = Mouse {
            x: 3 + 8 * 2 + 1,
            y: 20 + 8 * 3 + 1,
            ..Default::default()
        };
        ed.tick(&hover, &mut a);
        let mut fb = Framebuffer::new();
        ed.draw(&mut fb, &a);
        // The snapped block's top-left corner is on the white outline; the pixel
        // just left of it (the neighbouring block) is not — the marker sticks to
        // one block and never bleeds into the adjacent one.
        assert_eq!(fb.pget(19, 44), col::WHITE);
        assert_ne!(fb.pget(18, 44), col::WHITE);
    }

    fn ctrl(shift: bool) -> Mods {
        Mods {
            ctrl: true,
            shift,
            ..Default::default()
        }
    }

    fn release() -> Mouse {
        Mouse::default()
    }

    #[test]
    fn undo_and_redo_a_pencil_stroke() {
        let mut ed = SpriteEditor::new(); // sprite 1 -> sheet (8,0), pencil, colour 7.
        let mut a = Assets::default();
        // Paint pixel (0,0) of the sprite, then release to close the stroke.
        ed.tick(&press(CANVAS.0, CANVAS.1), &mut a);
        ed.tick(&release(), &mut a);
        assert_eq!(a.sprites.get(8, 0), 7);
        // Undo restores the blank pixel; redo paints it again.
        ed.key(Key::Char('z'), ctrl(false), &mut a);
        assert_eq!(a.sprites.get(8, 0), 0);
        ed.key(Key::Char('z'), ctrl(true), &mut a); // Ctrl+Shift+Z
        assert_eq!(a.sprites.get(8, 0), 7);
    }

    #[test]
    fn undo_only_keeps_the_last_ten_strokes() {
        use crate::editor::history::MAX_HISTORY;
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        // MAX_HISTORY + 2 separate strokes, each on a distinct pixel of the 8x8
        // sprite. Stroke i paints sprite pixel (i % 8, i / 8), i.e. sheet pixel
        // (8 + i % 8, i / 8).
        let strokes = MAX_HISTORY as i32 + 2;
        for i in 0..strokes {
            let sx = CANVAS.0 + (i % 8) * 8;
            let sy = CANVAS.1 + (i / 8) * 8;
            ed.tick(&press(sx, sy), &mut a);
            ed.tick(&release(), &mut a);
        }
        // Undo the full depth; only MAX_HISTORY strokes can be reverted.
        for _ in 0..MAX_HISTORY {
            ed.key(Key::Char('z'), ctrl(false), &mut a);
        }
        // The two oldest pixels are past the cap and stay painted; the rest were
        // undone.
        assert_eq!(a.sprites.get(8, 0), 7, "oldest stroke is past the cap");
        assert_eq!(a.sprites.get(9, 0), 7, "second-oldest stroke too");
        assert_eq!(a.sprites.get(10, 0), 0, "third stroke was undone");
        // Nothing left to undo beyond the cap.
        ed.key(Key::Char('z'), ctrl(false), &mut a);
        assert_eq!(a.sprites.get(8, 0), 7);
    }

    #[test]
    fn no_canvas_hover_when_over_the_palette() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        // Over the palette, not the canvas: no pixel-block marker is produced.
        let on_palette = Mouse {
            x: PAL.0 + 1,
            y: PAL.1 + 1,
            ..Default::default()
        };
        ed.tick(&on_palette, &mut a);
        assert_eq!(ed.canvas_pixel_under_cursor(), None);
    }

    #[test]
    fn clicking_a_size_button_sets_the_block_size() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        assert_eq!(ed.size, 1);
        // Button index 1 -> size 2, at x = SIZE_BTNS.0 + 1*8.
        ed.tick(&press(SIZE_BTNS.0 + 8 + 1, SIZE_BTNS.1 + 1), &mut a);
        assert_eq!(ed.size, 2);
        // Button index 3 -> size 8.
        ed.tick(&press(SIZE_BTNS.0 + 3 * 8 + 1, SIZE_BTNS.1 + 1), &mut a);
        assert_eq!(ed.size, 8);
    }

    #[test]
    fn painting_a_2x2_block_reaches_the_adjacent_sprite() {
        // With a 2x2 block the canvas spans sprites 1,2,17,18. Painting the
        // right half must land in the neighbour (sprite 2 at sheet (16, 0))
        // rather than being clipped away.
        let mut ed = SpriteEditor::new(); // sprite 1 -> block origin (8, 0).
        let mut a = Assets::default();
        ed.size = 2; // zoom becomes 64 / 16 = 4.
                     // Block pixel (8, 0) -> screen (CANVAS.0 + 8*4, CANVAS.1) = (35, 20).
        ed.tick(&press(CANVAS.0 + 8 * 4, CANVAS.1), &mut a);
        assert_eq!(a.sprites.get(16, 0), 7, "painted into the adjacent sprite");
    }

    #[test]
    fn a_block_near_the_edge_clamps_onto_the_sheet() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.size = 2;
        ed.sprite = 15; // last column, row 0.
        assert_eq!(
            ed.origin_cell(),
            (14, 0),
            "clamped so the 2-wide block fits"
        );
        assert_eq!(ed.top_left_sprite(), 14);
        // Painting the top-left of the canvas writes to sprite 14 at sheet (112, 0).
        ed.tick(&press(CANVAS.0, CANVAS.1), &mut a);
        assert_eq!(a.sprites.get(112, 0), 7);
    }

    #[test]
    fn copy_covers_the_whole_block() {
        use rico8_runtime::clipboard::{parse, Pasted};
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.size = 2; // 16x16 block of sprites 1,2,17,18.
        a.sprites.set(8, 0, 5); // top-left pixel of the block.
        a.sprites.set(23, 15, 9); // bottom-right pixel (sheet (8+15, 15)).
        let blob = ed.copy(&a);
        assert_eq!(ed.status.current(), Some("copied 16x16 spr 1"));
        let Pasted::Sprites { rect, flags } = parse(&blob).unwrap() else {
            panic!("not sprites")
        };
        assert_eq!((rect.w, rect.h), (16, 16));
        assert_eq!(rect.pixels[0], 5);
        assert_eq!(rect.pixels[16 * 16 - 1], 9);
        // One flag byte per covered sprite (2x2 = 4).
        assert_eq!(flags.map(|f| f.len()), Some(4));
    }

    // A held drag frame: button down, no fresh press edge.
    fn held(x: i32, y: i32) -> Mouse {
        Mouse {
            x,
            y,
            left: true,
            ..Default::default()
        }
    }

    #[test]
    fn hand_tool_is_selectable_by_key_and_icon() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.key(Key::Char('h'), Mods::default(), &mut a);
        assert_eq!(ed.tool, Tool::Pan);
        ed.tool = Tool::Pencil;
        // The hand is toolbar slot 4.
        ed.tick(&press(tool_x(4) + 1, 12), &mut a);
        assert_eq!(ed.tool, Tool::Pan);
    }

    #[test]
    fn pan_slides_the_window_across_the_sheet() {
        // Size 2 -> zoom 4, so one sheet cell spans z*8 = 32 screen px.
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.size = 2;
        ed.sprite = 34; // col 2, row 2.
        ed.tool = Tool::Pan;
        ed.tick(&press(CANVAS.0, CANVAS.1), &mut a); // grab the window.
                                                     // Drag the grabbed point right one cell (32 px): the window follows the
                                                     // hand, so the top-left cell scrolls left by one column.
        ed.tick(&held(CANVAS.0 + 32, CANVAS.1), &mut a);
        assert_eq!(ed.origin_cell(), (1, 2));
        // Drag down one cell too.
        ed.tick(&held(CANVAS.0 + 32, CANVAS.1 + 32), &mut a);
        assert_eq!(ed.origin_cell(), (1, 1));
    }

    #[test]
    fn pan_clamps_at_the_sheet_edge_and_touches_no_pixels() {
        let mut ed = SpriteEditor::new();
        let mut a = Assets::default();
        ed.size = 2; // cell = z*8 = 32 px.
        ed.sprite = 0; // window already at the top-left corner.
        a.sprites.set(8, 8, 9); // a pixel that must survive the pan.
        ed.tool = Tool::Pan;
        ed.tick(&press(CANVAS.0, CANVAS.1), &mut a);
        // Drag far right/down: the window can't scroll past the corner.
        ed.tick(&held(CANVAS.0 + 96, CANVAS.1 + 96), &mut a);
        assert_eq!(ed.origin_cell(), (0, 0), "clamped at the corner");
        ed.tick(&release(), &mut a);
        assert_eq!(a.sprites.get(8, 8), 9, "pan left pixels untouched");
        // A pan records no undo step (nothing to revert).
        ed.key(Key::Char('z'), ctrl(false), &mut a);
        assert_eq!(a.sprites.get(8, 8), 9);
    }

    #[test]
    fn copy_emits_native_blob_with_flags() {
        use rico8_runtime::clipboard::{parse, Pasted};
        let mut a = Assets::default();
        a.sprites.set_flag(1, 3, true); // flag bit 3 on sprite 1.
        let mut ed = SpriteEditor::new();
        ed.sprite = 1;
        let blob = ed.copy(&a);
        assert!(blob.starts_with("[rico8]"));
        let Pasted::Sprites { flags, .. } = parse(&blob).unwrap() else {
            panic!("not sprites")
        };
        assert_eq!(flags, Some(vec![0b0000_1000]));
        assert_eq!(ed.status.current(), Some("copied sprite 1"));
    }
}
