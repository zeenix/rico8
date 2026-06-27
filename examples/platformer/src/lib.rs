//! A tiny platformer: run, jump, collect coins. Solid tiles carry sprite
//! flag 0; coins are tile 3 and are collected by rewriting the map.
//!
//! The player is a [`Body`], so a running jump (hold Right + jump) — a
//! sub-pixel diagonal — climbs a clean staircase instead of shimmering. The
//! body owns the position; the cart just hands it the movement it worked out
//! for the frame and draws at `draw_x`/`draw_y`.

#![no_std]

use rico8::*;

game!(Platformer {
    body: Body::new(16.0, 80.0),
    vx: 0.0,
    vy: 0.0,
    grounded: false,
    flip: false,
    coins: 0,
    frame: 0,
});

struct Platformer {
    body: Body,
    vx: f32,
    vy: f32,
    grounded: bool,
    flip: bool,
    coins: u32,
    frame: u32,
}

const SOLID: SpriteFlag = SpriteFlag::Flag0;
// Sub-pixel run speed, so a running jump is a sub-pixel diagonal — the motion
// Body keeps coherent. At a whole pixel per frame there would be no zigzag.
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

        // Resolve collision axis by axis against the body's exact position,
        // then hand the allowed movement over in one call so the diagonal
        // render stays coherent. Tile lookups want whole pixels; positions
        // stay >= 0, so the truncating cast floors.
        let (x, y) = (self.body.x(), self.body.y());
        let mut dx = self.vx;
        if self.collide(ctx, (x + dx) as i32, y as i32) {
            dx = 0.0;
        }
        let mut dy = self.vy;
        if self.collide(ctx, (x + dx) as i32, (y + dy) as i32) {
            self.grounded = self.vy > 0.0;
            self.vy = 0.0;
            dy = 0.0;
        } else {
            self.grounded = false;
        }
        self.body.move_by(dx, dy);

        // Coins (tile 3): sample the hitbox center.
        let cx = (self.body.x() as i32 + 4) / 8;
        let cy = (self.body.y() as i32 + 4) / 8;
        if ctx.map_tile(cx, cy) == SpriteId(3) {
            ctx.set_map_tile(cx, cy, SpriteId(0));
            self.coins += 1;
            ctx.sfx(SfxId(1));
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        // Camera follows the player across the 32-tile-wide level.
        let cam = (self.body.x() - 60.0).clamp(0.0, (32 * 8 - SCREEN_WIDTH as i32) as f32);
        gfx.camera(cam as i32, 0);
        gfx.map(0, 0, 0, 0, 32, 16, BitFlags::empty()).unwrap();
        let frame = if !self.grounded || (self.vx != 0.0 && (self.frame / 4).is_multiple_of(2)) {
            2
        } else {
            1
        };
        // The body's coherent pixel — a running jump climbs cleanly, no zigzag.
        gfx.sprite_ext(
            SpriteId(frame),
            self.body.draw_x(),
            self.body.draw_y(),
            8,
            8,
            self.flip,
            false,
        )
        .unwrap();
        gfx.camera(0, 0);
        printf!(gfx, 2, 2, Color::YELLOW, "Coins {}", self.coins);
    }
}
