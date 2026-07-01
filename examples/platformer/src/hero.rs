use rico8::{Body, Button, Context, Graphics, SpriteId, SCREEN_WIDTH};

use crate::{
    constants::{
        COIN_SFX, COIN_SPRITE, HERO_HAPPY_SPRITE, HERO_LEGS_EXTEND_SPRITE, HERO_SPEED, HERO_SPRITE,
        JUMP_SFX, SOLID, TROPHY_SPRITE,
    },
    GameMode, Taken,
};

#[derive(Debug)]
pub struct Hero {
    body: Body,
    vx: f32,
    vy: f32,
    flip: bool,
    dead: bool,
    grounded: bool,
}

impl Hero {
    pub fn new() -> Self {
        Self {
            body: Body::new(16.0, 80.0),
            vx: 0.0,
            vy: 0.0,
            flip: false,
            dead: false,
            grounded: false,
        }
    }

    pub fn update(&mut self, ctx: &mut Context) -> Option<Taken> {
        // Horizontal movement (pixels per frame).
        if ctx.is_button_down(Button::Left) {
            self.vx = -HERO_SPEED;
            self.flip = true;
        } else if ctx.is_button_down(Button::Right) {
            self.vx = HERO_SPEED;
            self.flip = false;
        } else {
            self.vx = 0.0;
        }
        // Jump + gravity.
        if self.grounded && (ctx.is_button_pressed(Button::O) || ctx.is_button_pressed(Button::Up))
        {
            self.jump(ctx);
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

        // Coins & trophy: sample the hitbox center.
        let cx = (self.body.x() as i16 + 4) / 8;
        let cy = (self.body.y() as i16 + 4) / 8;
        match ctx.map_tile(cx, cy) {
            Some(COIN_SPRITE) => {
                ctx.set_map_tile(cx, cy, SpriteId(0)).unwrap();
                ctx.sfx(COIN_SFX);
                Some(Taken::new_coin(cx, cy))
            }
            Some(TROPHY_SPRITE) => {
                ctx.set_map_tile(cx, cy, SpriteId(0)).unwrap();
                Some(Taken::new_trophy(cx, cy))
            }
            _ => None,
        }
    }

    pub fn jump(&mut self, ctx: &mut Context) {
        self.vy = -3.25;
        ctx.sfx(JUMP_SFX);
    }

    // Camera follows the player across the 32-tile-wide level.
    pub fn center(&self, gfx: &mut Graphics) {
        let cam = (self.body.x() - 60.0).clamp(8.0, (32 * 8 - SCREEN_WIDTH as i16) as f32);
        gfx.camera(cam as i16, 0);
    }

    pub fn draw(&self, gfx: &mut Graphics, frame: u32, mode: &GameMode) {
        let is_alt_frame = (frame / 4).is_multiple_of(2);
        if self.dead && is_alt_frame {
            // If hero dies, we show them flashing in & out of existence.
            return;
        }

        let sprite = if !self.grounded || (self.vx != 0.0 && is_alt_frame) {
            match mode {
                GameMode::Ended { won, .. } if *won => HERO_HAPPY_SPRITE,
                GameMode::InGame { .. } | GameMode::Ended { .. } => HERO_LEGS_EXTEND_SPRITE,
                GameMode::Init => unreachable!(),
            }
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
    }

    pub fn draw_x(&self) -> i16 {
        self.body.draw_x()
    }

    pub fn draw_y(&self) -> i16 {
        self.body.draw_y()
    }

    pub fn die(&mut self) {
        self.dead = true;
    }

    fn collide(&self, ctx: &Context, x: i16, y: i16) -> bool {
        // Check the four corners of the 8x8 hitbox (in pixels).
        self.solid_at(ctx, x, y)
            || self.solid_at(ctx, x + 7, y)
            || self.solid_at(ctx, x, y + 7)
            || self.solid_at(ctx, x + 7, y + 7)
    }

    fn solid_at(&self, ctx: &Context, px: i16, py: i16) -> bool {
        ctx.map_tile(px / 8, py / 8)
            .is_some_and(|tile| ctx.has_sprite_flag(tile, SOLID))
    }
}
