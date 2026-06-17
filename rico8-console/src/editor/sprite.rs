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
}

impl SpriteEditor {
    pub fn new() -> Self {
        Self {
            sprite: 1,
            color: 7,
            tool: Tool::Pencil,
            page: 0,
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
        // Canvas painting (drag-friendly).
        if (m.left || m.right) && m.over(CANVAS.0, CANVAS.1, CANVAS.0 + 63, CANVAS.1 + 63) {
            let px = (m.x - CANVAS.0) / 8;
            let py = (m.y - CANVAS.1) / 8;
            // Fill and picker should fire once per click, not per frame.
            let one_shot = matches!(self.tool, Tool::Fill | Tool::Picker);
            if !one_shot || m.left_pressed || m.right_pressed {
                self.apply_tool(assets, px, py, m.right && !m.left);
            }
        }
        if !m.left_pressed {
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
                col::DARK_PURPLE
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
        for py in 0..8 {
            for px in 0..8 {
                let c = assets.sprites.get(ox + px, oy + py);
                fb.rectfill(
                    CANVAS.0 + px * 8,
                    CANVAS.1 + py * 8,
                    CANVAS.0 + px * 8 + 7,
                    CANVAS.1 + py * 8 + 7,
                    c,
                );
            }
        }

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
                if on { col::RED } else { col::DARK_PURPLE },
            );
        }

        // Page dots.
        for p in 0..4u32 {
            let x = PAGE_BTNS.0 + p as i32 * 6;
            let c = if p == self.page {
                col::WHITE
            } else {
                col::DARK_PURPLE
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
