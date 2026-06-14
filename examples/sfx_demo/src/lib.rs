//! A tiny soundboard: each button plays a different sound effect.

#![no_std]

use rico8::*;

struct SfxDemo {
    last: Option<u8>,
    t: u32,
}

const PADS: [(Button, u8, &str); 4] = [
    (Button::Left, 0, "jump"),
    (Button::Right, 1, "coin"),
    (Button::Up, 2, "laser"),
    (Button::Down, 3, "hurt"),
];

impl Game for SfxDemo {
    fn update(&mut self, ctx: &mut Context) {
        self.t += 1;
        for (btn, sfx, _) in PADS {
            if ctx.btnp(btn) {
                ctx.sfx(SfxId(sfx));
                self.last = Some(sfx);
            }
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_PURPLE);
        gfx.print("sfx soundboard", 36, 8, Color::WHITE);
        for (i, (_, sfx, name)) in PADS.iter().enumerate() {
            let y = 32 + i as i32 * 20;
            let hot = self.last == Some(*sfx);
            let bg = if hot { Color::PINK } else { Color::DARK_BLUE };
            gfx.rect_fill(24, y, 80, 14, bg);
            gfx.print(name, 28, y + 4, Color::WHITE);
        }
        gfx.print("press arrow keys", 32, 116, Color::LIGHT_GREY);
    }
}

rico8::game!(SfxDemo { last: None, t: 0 });
