//! A tiny platformer: run, jump, collect coins. Solid tiles carry sprite
//! flag 0; coins are tile 3 and are collected by rewriting the map.

use rico8::*;

struct Game {
    // Fixed-point position/velocity (1/16 pixel).
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    grounded: bool,
    flip: bool,
    coins: u32,
    frame: u32,
}

const SOLID: u8 = 0;

impl Game {
    fn solid_at(&self, ctx: &Context, px: i32, py: i32) -> bool {
        let tile = ctx.mget(px / 8, py / 8);
        ctx.fget_flag(SpriteId(tile), SOLID)
    }

    fn collide(&self, ctx: &Context, x: i32, y: i32) -> bool {
        // Check the four corners of the 8x8 hitbox (in pixels).
        self.solid_at(ctx, x, y)
            || self.solid_at(ctx, x + 7, y)
            || self.solid_at(ctx, x, y + 7)
            || self.solid_at(ctx, x + 7, y + 7)
    }
}

impl Rico8Game for Game {
    fn update(&mut self, ctx: &mut Context) {
        self.frame += 1;
        // Horizontal movement.
        if ctx.btn(Button::Left) {
            self.vx = -16;
            self.flip = true;
        } else if ctx.btn(Button::Right) {
            self.vx = 16;
            self.flip = false;
        } else {
            self.vx = 0;
        }
        // Jump + gravity.
        if self.grounded && (ctx.btnp(Button::O) || ctx.btnp(Button::Up)) {
            self.vy = -52;
            ctx.sfx(SfxId(0));
        }
        self.vy = (self.vy + 4).min(64);

        // Move and collide, axis by axis (pixel space).
        let nx = (self.x + self.vx) / 16;
        if !self.collide(ctx, nx, self.y / 16) {
            self.x += self.vx;
        }
        let ny = (self.y + self.vy) / 16;
        if self.collide(ctx, self.x / 16, ny) {
            self.grounded = self.vy > 0;
            self.vy = 0;
        } else {
            self.y += self.vy;
            self.grounded = false;
        }

        // Coins (tile 3).
        let (cx, cy) = ((self.x / 16 + 4) / 8, (self.y / 16 + 4) / 8);
        if ctx.mget(cx, cy) == 3 {
            ctx.mset(cx, cy, SpriteId(0));
            self.coins += 1;
            ctx.sfx(SfxId(1));
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        // Camera follows the player across the 32-tile-wide level.
        let cam = (self.x / 16 - 60).clamp(0, 32 * 8 - SCREEN_W);
        gfx.camera(cam, 0);
        gfx.map(0, 0, 0, 0, 32, 16, 0);
        let frame = if !self.grounded {
            2
        } else if self.vx != 0 && (self.frame / 4) % 2 == 0 {
            2
        } else {
            1
        };
        gfx.spr_ext(SpriteId(frame), self.x / 16, self.y / 16, 1, 1, self.flip, false);
        gfx.camera(0, 0);
        gfx.print(&format!("coins {}", self.coins), 2, 2, Color::YELLOW);
    }
}

rico8::game!(Game {
    x: 16 * 16,
    y: 16 * 80,
    vx: 0,
    vy: 0,
    grounded: false,
    flip: false,
    coins: 0,
    frame: 0,
});
