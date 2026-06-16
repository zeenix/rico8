//! Hello, RICO-8: the canonical first cart. Arrows move the square.

#![no_std]

use rico8::*;

struct Hello {
    x: f32,
    y: f32,
}

impl Game for Hello {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.is_button_down(Button::Left) {
            self.x -= 1.0;
        }
        if ctx.is_button_down(Button::Right) {
            self.x += 1.0;
        }
        if ctx.is_button_down(Button::Up) {
            self.y -= 1.0;
        }
        if ctx.is_button_down(Button::Down) {
            self.y += 1.0;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("hello, rico-8!", 36.0, 40.0, Color::WHITE);
        gfx.rect_fill(self.x, self.y, 8.0, 8.0, Color::WHITE);
    }
}

rico8::game!(Hello { x: 60.0, y: 64.0 });
