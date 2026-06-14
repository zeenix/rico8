//! Hello, RICO-8: the canonical first cart. Arrows move the square.

use rico8::*;

struct Hello {
    x: i32,
    y: i32,
}

impl Game for Hello {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.btn(Button::Left) {
            self.x -= 1;
        }
        if ctx.btn(Button::Right) {
            self.x += 1;
        }
        if ctx.btn(Button::Up) {
            self.y -= 1;
        }
        if ctx.btn(Button::Down) {
            self.y += 1;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("hello, rico-8!", 36, 40, Color::WHITE);
        gfx.rect_fill(self.x, self.y, 8, 8, Color::WHITE);
    }
}

rico8::game!(Hello { x: 60, y: 64 });
