//! Scroll around a tile map with the arrow keys; X toggles layer
//! filtering to show how sprite flags select what `map` draws.

#![no_std]

use rico8::*;

struct MapDemo {
    cam_x: f32,
    cam_y: f32,
    solid_only: bool,
}

impl Game for MapDemo {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.is_button_down(Button::Left) {
            self.cam_x -= 2.0;
        }
        if ctx.is_button_down(Button::Right) {
            self.cam_x += 2.0;
        }
        if ctx.is_button_down(Button::Up) {
            self.cam_y -= 2.0;
        }
        if ctx.is_button_down(Button::Down) {
            self.cam_y += 2.0;
        }
        self.cam_x = self.cam_x.clamp(0.0, (32 * 8 - SCREEN_W) as f32);
        self.cam_y = self.cam_y.clamp(0.0, (16 * 8 - SCREEN_H + 64) as f32);
        if ctx.is_button_pressed(Button::X) {
            self.solid_only = !self.solid_only;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.camera(self.cam_x, self.cam_y);
        // Empty set draws everything; flag 0 selects only solid tiles.
        let layers: BitFlags<SpriteFlag> = if self.solid_only {
            SpriteFlag::Flag0.into()
        } else {
            BitFlags::empty()
        };
        gfx.map(0, 0, 0.0, 0.0, 32, 16, layers);
        gfx.camera(0.0, 0.0);
        gfx.print("Arrows scroll, X: layers", 4.0, 2.0, Color::WHITE);
        if self.solid_only {
            gfx.print("Solid tiles only", 4.0, 120.0, Color::ORANGE);
        }
    }
}

rico8::game!(MapDemo {
    cam_x: 0.0,
    cam_y: 0.0,
    solid_only: false,
});
