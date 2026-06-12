//! A sprite walking around with button-driven animation and flipping.

use rico8::*;

struct Game {
    x: i32,
    y: i32,
    flip: bool,
    walking: bool,
    frame: u32,
}

impl Rico8Game for Game {
    fn update(&mut self, ctx: &mut Context) {
        self.walking = false;
        if ctx.btn(Button::Left) {
            self.x -= 1;
            self.flip = true;
            self.walking = true;
        }
        if ctx.btn(Button::Right) {
            self.x += 1;
            self.flip = false;
            self.walking = true;
        }
        if ctx.btn(Button::Up) {
            self.y -= 1;
            self.walking = true;
        }
        if ctx.btn(Button::Down) {
            self.y += 1;
            self.walking = true;
        }
        self.x = self.x.clamp(0, SCREEN_W - 8);
        self.y = self.y.clamp(0, SCREEN_H - 8);
        self.frame += 1;
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        gfx.print("arrows to walk", 36, 4, Color::LIGHT_GREY);
        // Sprites 1 and 2 are the two walk frames.
        let frame = if self.walking && (self.frame / 4) % 2 == 0 {
            2
        } else {
            1
        };
        gfx.spr_ext(SpriteId(frame), self.x, self.y, 1, 1, self.flip, false);
    }
}

rico8::game!(Game {
    x: 60,
    y: 64,
    flip: false,
    walking: false,
    frame: 0,
});
