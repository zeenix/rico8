//! The sprite editor: zoomed 8x8 canvas, palette grid, tools, flags, and
//! a sheet strip for picking sprites — the classic layout.

use crate::{
    shell::{Key, Mods},
    ui::{self, draw_icon8, Icon8, Mouse, ICON_PENCIL},
};
use rico8_runtime::{
    assets::{Assets, SPRITES_PER_ROW},
    fb::Framebuffer,
    palette::col,
};

// Layout.
const CANVAS: (i32, i32) = (3, 20); // 64x64, 8x zoom
const PAL: (i32, i32) = (74, 20); // 4x4 grid of 12px swatches
const FLAGS: (i32, i32) = (76, 72); // 8 toggle dots
const SHEET_Y: i32 = 88; // 4 rows of sprites (one page)
const PAGE_BTNS: (i32, i32) = (104, 81); // 4 page dots

// Fullscreen canvas: the 8x8 sprite at 14x zoom (112x112), filling rows 8..119.
const FS_CANVAS: (i32, i32) = (8, 8);
const FS_ZOOM: i32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
}

pub struct SpriteEditor {
    sprite: u32,
    color: u8,
    tool: Tool,
    page: u32,
    fullscreen: bool,
}

impl SpriteEditor {
    pub fn new() -> Self {
        Self {
            sprite: 1,
            color: 7,
            tool: Tool::Pencil,
            page: 0,
            fullscreen: false,
        }
    }

    /// Whether the fullscreen (bare-canvas) view is active.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Canvas origin and per-pixel zoom for the active view.
    fn canvas(&self) -> (i32, i32, i32) {
        if self.fullscreen {
            (FS_CANVAS.0, FS_CANVAS.1, FS_ZOOM)
        } else {
            (CANVAS.0, CANVAS.1, 8)
        }
    }

    fn sheet_origin(&self) -> (i32, i32) {
        (
            (self.sprite as i32 % SPRITES_PER_ROW as i32) * 8,
            (self.sprite as i32 / SPRITES_PER_ROW as i32) * 8,
        )
    }

    pub fn key(&mut self, key: Key, _mods: Mods, _assets: &mut Assets) {
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
                // Flood fill within the 8x8 sprite.
                let mut stack = vec![(px, py)];
                while let Some((x, y)) = stack.pop() {
                    if !(0..8).contains(&x) || !(0..8).contains(&y) {
                        continue;
                    }
                    if assets.sprites.get(ox + x, oy + y) != target {
                        continue;
                    }
                    assets.sprites.set(ox + x, oy + y, self.color);
                    stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
                }
            }
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets) {
        let m = *mouse;
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
        let size = z * 8;
        if (m.left || m.right) && m.over(cx, cy, cx + size - 1, cy + size - 1) {
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
        for (i, tool) in [Tool::Pencil, Tool::Eraser, Tool::Fill, Tool::Picker]
            .iter()
            .enumerate()
        {
            let x = 3 + i as i32 * 10;
            if m.over(x, 9, x + 7, 17) {
                self.tool = *tool;
            }
        }
        // Flags.
        for f in 0..8 {
            let x = FLAGS.0 + f * 6;
            if m.over(x, FLAGS.1, x + 4, FLAGS.1 + 4) {
                let cur = assets.sprites.flags(self.sprite);
                assets
                    .sprites
                    .set_flag(self.sprite, f as u8, cur & (1 << f) == 0);
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
        let tools = [
            (Tool::Pencil, ICON_PENCIL),
            (Tool::Eraser, ICON_ERASER),
            (Tool::Fill, ICON_FILL),
            (Tool::Picker, ICON_PICKER),
        ];
        for (i, (tool, icon)) in tools.iter().enumerate() {
            let x = 3 + i as i32 * 10;
            let color = if *tool == self.tool {
                col::WHITE
            } else {
                col::LAVENDER
            };
            if *tool == self.tool {
                fb.rectfill(x - 1, 9, x + 8, 17, col::BLACK);
            }
            draw_icon8(fb, icon, x, 9, color);
        }
        fb.print(&format!("#{:03}", self.sprite), 106, 11, col::WHITE);

        // Canvas.
        fb.rect(
            CANVAS.0 - 1,
            CANVAS.1 - 1,
            CANVAS.0 + 64,
            CANVAS.1 + 64,
            col::BLACK,
        );
        let (ox, oy) = self.sheet_origin();
        // Magnify the 8x8 sprite onto the 64x64 canvas. The artist edits every
        // pixel, so color 0 must show as black here rather than being treated as
        // transparent for this blit.
        fb.set_transparent_color(0, false);
        fb.sspr(
            &assets.sprites,
            ox,
            oy,
            8,
            8,
            CANVAS.0,
            CANVAS.1,
            64,
            64,
            false,
            false,
        );
        fb.reset_transparency();

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
        let flags = assets.sprites.flags(self.sprite);
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
        // Selection box on the strip.
        if self.sprite / 64 == self.page {
            let i = self.sprite % 64;
            let x = (i as i32 % 16) * 8;
            let y = SHEET_Y + (i as i32 / 16) * 8;
            fb.rect(x, y, x + 7, y + 7, col::WHITE);
        }

        ui::status_bar(fb, &format!("Spr {:03} flags {:08b}", self.sprite, flags));
    }

    /// Fullscreen view: the selected 8x8 sprite magnified to fill the editor
    /// area, with no palette/flags/sheet. The shell paints the dark-grey
    /// background; we draw the sprite (colour 0 shown as black) and the status.
    fn draw_fullscreen(&self, fb: &mut Framebuffer, assets: &Assets) {
        let (ox, oy) = self.sheet_origin();
        let (cx, cy, z) = self.canvas();
        fb.set_transparent_color(0, false);
        fb.sspr(
            &assets.sprites,
            ox,
            oy,
            8,
            8,
            cx,
            cy,
            z * 8,
            z * 8,
            false,
            false,
        );
        fb.reset_transparency();
        let flags = assets.sprites.flags(self.sprite);
        ui::status_bar(fb, &format!("Spr {:03} flags {:08b}", self.sprite, flags));
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
}
