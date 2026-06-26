//! A sprite walking around with button-driven animation and flipping.

#![no_std]

use rico8::*;

game!(SpriteMove {
    x: 60,
    y: 64,
    flip: false,
    walking: false,
    frame: 0,
});

struct SpriteMove {
    x: i32,
    y: i32,
    flip: bool,
    walking: bool,
    frame: u32,
}

impl Game for SpriteMove {
    fn update(&mut self, ctx: &mut Context) {
        self.walking = false;
        if ctx.is_button_down(Button::Left) {
            self.x -= 1;
            self.flip = true;
            self.walking = true;
        }
        if ctx.is_button_down(Button::Right) {
            self.x += 1;
            self.flip = false;
            self.walking = true;
        }
        if ctx.is_button_down(Button::Up) {
            self.y -= 1;
            self.walking = true;
        }
        if ctx.is_button_down(Button::Down) {
            self.y += 1;
            self.walking = true;
        }
        self.x = self.x.clamp(0, SCREEN_WIDTH as i32 - 8);
        self.y = self.y.clamp(0, SCREEN_HEIGHT as i32 - 8);
        self.frame += 1;
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        gfx.print("Arrows to walk", 36, 4, Color::LIGHT_GREY);
        // Sprites 1 and 2 are the two walk frames.
        let frame = if self.walking && (self.frame / 4).is_multiple_of(2) {
            2
        } else {
            1
        };
        gfx.sprite_ext(SpriteId(frame), self.x, self.y, 8, 8, self.flip, false)
            .unwrap();
    }
}
