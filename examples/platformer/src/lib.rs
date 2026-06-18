//! A tiny platformer: run, jump, collect coins. Solid tiles carry sprite
//! flag 0; coins are tile 3 and are collected by rewriting the map.
//!
//! The player stores a float position (`x`, `y`); the console renders at a
//! device scale, flooring in device space, so fractional positions produce
//! smooth sub-pixel motion with no cart-side helper needed.

#![no_std]

use rico8::*;

struct Platformer {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    grounded: bool,
    flip: bool,
    coins: u32,
    frame: u32,
}

const SOLID: SpriteFlag = SpriteFlag::Flag0;
// Sub-pixel run speed — demonstrates smooth sub-pixel motion courtesy of the
// device-scale renderer.
const RUN: f32 = 0.7;

impl Platformer {
    fn solid_at(&self, ctx: &Context, px: i32, py: i32) -> bool {
        let tile = ctx.map_tile(px / 8, py / 8);
        ctx.has_sprite_flag(tile, SOLID)
    }

    fn collide(&self, ctx: &Context, x: i32, y: i32) -> bool {
        // Check the four corners of the 8x8 hitbox (in pixels).
        self.solid_at(ctx, x, y)
            || self.solid_at(ctx, x + 7, y)
            || self.solid_at(ctx, x, y + 7)
            || self.solid_at(ctx, x + 7, y + 7)
    }
}

impl Game for Platformer {
    fn update(&mut self, ctx: &mut Context) {
        self.frame += 1;
        // Horizontal movement (pixels per frame).
        if ctx.is_button_down(Button::Left) {
            self.vx = -RUN;
            self.flip = true;
        } else if ctx.is_button_down(Button::Right) {
            self.vx = RUN;
            self.flip = false;
        } else {
            self.vx = 0.0;
        }
        // Jump + gravity.
        if self.grounded && (ctx.is_button_pressed(Button::O) || ctx.is_button_pressed(Button::Up))
        {
            self.vy = -3.25;
            ctx.sfx(SfxId(0));
        }
        self.vy = (self.vy + 0.25).min(4.0);

        // Resolve collision axis by axis against the exact position, then
        // apply the allowed movement. Tile lookups want whole pixels; positions
        // stay >= 0, so the truncating cast floors.
        let mut dx = self.vx;
        if self.collide(ctx, (self.x + dx) as i32, self.y as i32) {
            dx = 0.0;
        }
        let mut dy = self.vy;
        if self.collide(ctx, (self.x + dx) as i32, (self.y + dy) as i32) {
            self.grounded = self.vy > 0.0;
            self.vy = 0.0;
            dy = 0.0;
        } else {
            self.grounded = false;
        }
        self.x += dx;
        self.y += dy;

        // Coins (tile 3): sample the hitbox center.
        let cx = (self.x as i32 + 4) / 8;
        let cy = (self.y as i32 + 4) / 8;
        if ctx.map_tile(cx, cy) == SpriteId(3) {
            ctx.set_map_tile(cx, cy, SpriteId(0));
            self.coins += 1;
            ctx.sfx(SfxId(1));
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        // Camera follows the player across the 32-tile-wide level.
        let cam = (self.x - 60.0).clamp(0.0, (32 * 8 - SCREEN_W) as f32);
        gfx.camera(cam, 0.0);
        gfx.map(0, 0, 0.0, 0.0, 32, 16, BitFlags::empty());
        let frame = if !self.grounded || (self.vx != 0.0 && (self.frame / 4).is_multiple_of(2)) {
            2
        } else {
            1
        };
        // Draw at the exact float position — the console renders it sub-pixel-smooth.
        gfx.sprite_ext(SpriteId(frame), self.x, self.y, 1.0, 1.0, self.flip, false);
        gfx.camera(0.0, 0.0);
        rico8::printf!(gfx, 2.0, 2.0, Color::YELLOW, "Coins {}", self.coins);
    }
}

rico8::game!(Platformer {
    x: 16.0,
    y: 80.0,
    vx: 0.0,
    vy: 0.0,
    grounded: false,
    flip: false,
    coins: 0,
    frame: 0,
});
