//! The map editor: a scrollable tile viewport painted from the sprite
//! sheet, with a picker strip along the bottom.

use crate::shell::{Key, Mods};
use crate::ui::{self, Mouse};
use rico8_runtime::assets::{Assets, MAP_H, MAP_W, SPRITES_PER_ROW};
use rico8_runtime::fb::Framebuffer;
use rico8_runtime::palette::col;

/// Viewport: 16x12 tiles starting under the tab bar.
const VIEW_Y: i32 = 8;
const VIEW_TILES_X: i32 = 16;
const VIEW_TILES_Y: i32 = 12;
/// Picker strip: 2 rows of 16 sprites.
const PICK_Y: i32 = VIEW_Y + VIEW_TILES_Y * 8;

pub struct MapEditor {
    tile: u32,
    cam_x: i32,
    cam_y: i32,
    /// Picker page: 8 pages of 32 sprites.
    page: u32,
}

impl MapEditor {
    pub fn new() -> Self {
        Self {
            tile: 1,
            cam_x: 0,
            cam_y: 0,
            page: 0,
        }
    }

    pub fn key(&mut self, key: Key, _mods: Mods, _assets: &mut Assets) {
        match key {
            Key::Left => self.cam_x = (self.cam_x - 1).max(0),
            Key::Right => self.cam_x = (self.cam_x + 1).min(MAP_W as i32 - VIEW_TILES_X),
            Key::Up => self.cam_y = (self.cam_y - 1).max(0),
            Key::Down => self.cam_y = (self.cam_y + 1).min(MAP_H as i32 - VIEW_TILES_Y),
            Key::PageUp => self.page = (self.page + 7) % 8,
            Key::PageDown => self.page = (self.page + 1) % 8,
            _ => {}
        }
    }

    pub fn tick(&mut self, mouse: &Mouse, assets: &mut Assets) {
        let m = *mouse;
        // Paint / erase in the viewport (drag-friendly).
        if (m.left || m.right) && m.over(0, VIEW_Y, 127, PICK_Y - 1) {
            let cx = self.cam_x + m.x / 8;
            let cy = self.cam_y + (m.y - VIEW_Y) / 8;
            let t = if m.right { 0 } else { self.tile as u8 };
            assets.map.set(cx, cy, t);
        }
        // Picker strip.
        if m.left_pressed && m.over(0, PICK_Y, 127, PICK_Y + 15) {
            let cx = m.x / 8;
            let cy = (m.y - PICK_Y) / 8;
            self.tile = (self.page * 32 + cy as u32 * 16 + cx as u32) % 256;
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        // Viewport.
        fb.rectfill(0, VIEW_Y, 127, PICK_Y - 1, col::BLACK);
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

        // Picker strip.
        fb.rectfill(0, PICK_Y, 127, PICK_Y + 15, col::DARK_GREY);
        for cy in 0..2i32 {
            for cx in 0..16i32 {
                let n = self.page * 32 + (cy * 16 + cx) as u32;
                let (sx, sy) = (
                    (n as i32 % SPRITES_PER_ROW as i32) * 8,
                    (n as i32 / SPRITES_PER_ROW as i32) * 8,
                );
                for py in 0..8 {
                    for px in 0..8 {
                        fb.pset(
                            cx * 8 + px,
                            PICK_Y + cy * 8 + py,
                            assets.sprites.get(sx + px, sy + py),
                        );
                    }
                }
            }
        }
        if self.tile / 32 == self.page {
            let i = self.tile % 32;
            let x = (i as i32 % 16) * 8;
            let y = PICK_Y + (i as i32 / 16) * 8;
            fb.rect(x, y, x + 7, y + 7, col::WHITE);
        }

        ui::status_bar(
            fb,
            &format!(
                "cel {:03},{:02} tile {:03} pg{}",
                self.cam_x, self.cam_y, self.tile, self.page
            ),
        );
    }
}
