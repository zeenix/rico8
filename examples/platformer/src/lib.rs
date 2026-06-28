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
    mode: GameMode::InGame,
});

struct Platformer {
    body: Body,
    vx: f32,
    vy: f32,
    grounded: bool,
    flip: bool,
    coins: u32,
    frame: u32,
    mode: GameMode,
}

impl Platformer {
    fn solid_at(&self, ctx: &Context, px: i16, py: i16) -> bool {
        ctx.map_tile(px / 8, py / 8)
            .is_some_and(|tile| ctx.has_sprite_flag(tile, SOLID))
    }

    fn collide(&self, ctx: &Context, x: i16, y: i16) -> bool {
        // Check the four corners of the 8x8 hitbox (in pixels).
        self.solid_at(ctx, x, y)
            || self.solid_at(ctx, x + 7, y)
            || self.solid_at(ctx, x, y + 7)
            || self.solid_at(ctx, x + 7, y + 7)
    }

    fn in_game_update(&mut self, ctx: &mut Context) {
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
            ctx.sfx(SfxId::new(0).unwrap());
        }
        self.vy = (self.vy + 0.25).min(4.0);

        // Resolve collision axis by axis against the body's exact position,
        // then hand the allowed movement over in one call so the diagonal
        // render stays coherent. Tile lookups want whole pixels; positions
        // stay >= 0, so the truncating cast floors.
        let (x, y) = (self.body.x(), self.body.y());
        let mut dx = self.vx;
        if self.collide(ctx, (x + dx) as i16, y as i16) {
            dx = 0.0;
        }
        let mut dy = self.vy;
        if self.collide(ctx, (x + dx) as i16, (y + dy) as i16) {
            self.grounded = self.vy > 0.0;
            self.vy = 0.0;
            dy = 0.0;
        } else {
            self.grounded = false;
        }
        self.body.move_by(dx, dy);

        // Coins (tile 3) & trophy: sample the hitbox center.
        let cx = (self.body.x() as i16 + 4) / 8;
        let cy = (self.body.y() as i16 + 4) / 8;
        match ctx.map_tile(cx, cy) {
            Some(COIN_SPRITE) => {
                let _ = ctx.set_map_tile(cx, cy, SpriteId(0));
                self.coins += 1;
                ctx.sfx(SfxId::new(1).unwrap());
            }
            Some(TROPHY_SPRITE) => {
                let _ = ctx.set_map_tile(cx, cy, SpriteId(0));
                ctx.sfx(SfxId::new(8).unwrap());
                self.mode = GameMode::Ended {
                    time: ctx.time(),
                    flash: false,
                };
            }
            _ => (),
        }
    }

    fn restart_game(&mut self) {
        self.body = Body::new(16.0, 80.0);
        self.vx = 0.0;
        self.vy = 0.0;
        self.grounded = false;
        self.flip = false;
        self.coins = 0;
        self.frame = 0;
        self.mode = GameMode::InGame;
    }
}

impl Game for Platformer {
    fn update(&mut self, ctx: &mut Context) {
        self.frame += 1;

        match &mut self.mode {
            GameMode::InGame => self.in_game_update(ctx),
            GameMode::Ended { time, .. } if ctx.time() - *time > GAME_OVER_TIMEOUT => {
                self.restart_game()
            }
            // Flash on every 16th frame.
            GameMode::Ended { flash, .. } => *flash = self.frame.is_multiple_of(16),
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        if matches!(self.mode, GameMode::Ended { flash: true, .. }) {
            gfx.clear(Color::WHITE);
        } else {
            gfx.clear(Color::DARK_BLUE);
        }

        // Camera follows the player across the 32-tile-wide level.
        let cam = (self.body.x() - 60.0).clamp(0.0, (32 * 8 - SCREEN_WIDTH as i16) as f32);
        gfx.camera(cam as i16, 0);
        gfx.map(0, 0, 0, 0, 32, 16, BitFlags::empty()).unwrap();
        let sprite = if !self.grounded || (self.vx != 0.0 && (self.frame / 4).is_multiple_of(2)) {
            HERO_LEGS_EXTEND_SPRITE
        } else {
            HERO_SPRITE
        };
        // The body's coherent pixel — a running jump climbs cleanly, no zigzag.
        gfx.sprite_ext(
            sprite,
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

#[derive(Debug)]
enum GameMode {
    InGame,
    Ended { time: f32, flash: bool },
}

const SOLID: SpriteFlag = SpriteFlag::Flag0;
// Sub-pixel run speed, so a running jump is a sub-pixel diagonal — the motion
// Body keeps coherent. At a whole pixel per frame there would be no zigzag.
const RUN: f32 = 0.7;
const HERO_SPRITE: SpriteId = SpriteId(1);
const HERO_LEGS_EXTEND_SPRITE: SpriteId = SpriteId(2);
const COIN_SPRITE: SpriteId = SpriteId(3);
const TROPHY_SPRITE: SpriteId = SpriteId(4);
const GAME_OVER_TIMEOUT: f32 = 5.0;
