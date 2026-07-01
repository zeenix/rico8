//! A tiny platformer: run, jump, collect coins. Solid tiles carry sprite
//! flag 0; coins are tile 3 and are collected by rewriting the map.
//!
//! The player is a [`Body`], so a running jump (hold Right + jump) — a
//! sub-pixel diagonal — climbs a clean staircase instead of shimmering. The
//! body owns the position; the cart just hands it the movement it worked out
//! for the frame and draws at `draw_x`/`draw_y`.

#![no_std]

use heapless::Vec;
use rico8::*;

game!(Platformer {
    hero: Hero::new(),
    vx: 0.0,
    vy: 0.0,
    grounded: false,
    badie: Some(Badie::new()),
    taken: Vec::new(),
    badies_killed: 0,
    frame: 0,
    mode: GameMode::Init,
});

struct Platformer {
    hero: Hero,
    vx: f32,
    vy: f32,
    grounded: bool,
    badie: Option<Badie>,
    taken: Vec<Taken, MAX_TAKEN>,
    badies_killed: u8,
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
        let GameMode::InGame {
            start_time,
            time_left,
        } = &mut self.mode
        else {
            unreachable!();
        };

        if *time_left == 0 {
            self.game_over(ctx);

            return;
        }
        let elapsed = (ctx.time() - *start_time).max(0.0) as u8;
        *time_left = GAME_TIMEOUT - elapsed;

        // Horizontal movement (pixels per frame).
        if ctx.is_button_down(Button::Left) {
            self.vx = -HERO_SPEED;
            self.hero.flip = true;
        } else if ctx.is_button_down(Button::Right) {
            self.vx = HERO_SPEED;
            self.hero.flip = false;
        } else {
            self.vx = 0.0;
        }
        // Jump + gravity.
        if self.grounded && (ctx.is_button_pressed(Button::O) || ctx.is_button_pressed(Button::Up))
        {
            self.vy = -3.25;
            ctx.sfx(JUMP_SFX);
        }
        self.vy = (self.vy + 0.25).min(4.0);

        // Resolve collision axis by axis against the body's exact position,
        // then hand the allowed movement over in one call so the diagonal
        // render stays coherent. Tile lookups want whole pixels; positions
        // stay >= 0, so the truncating cast floors.
        let (x, y) = (self.hero.body.x(), self.hero.body.y());
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
        self.hero.body.move_by(dx, dy);

        // Check for collision between our hero and the badie, and decide who dies if there is one.
        if let Some(badie) = &mut self.badie {
            if self.hero.body.draw_x() + HERO_WIDTH >= badie.body.draw_x()
                && self.hero.body.draw_x() < badie.body.draw_x() + BADIE_WIDTH
            {
                if self.hero.body.draw_y() == badie.body.draw_y() {
                    // Hero ramming into badie horizontally is a suicide.
                    self.hero.dead = true;
                    self.game_over(ctx);

                    return;
                } else if self.hero.body.draw_y() + HERO_HEIGHT >= badie.body.draw_y()
                    && self.hero.body.draw_y() < badie.body.draw_y() + BADIE_HEIGHT
                {
                    // Hero hitting the badie from the top, kills the badie and gives hero a boost.
                    self.badie = None;
                    self.badies_killed += 1;
                    self.vy = -3.25;
                    ctx.sfx(BADIE_DEAD_SFX);
                    ctx.sfx(JUMP_SFX);
                }
            }
        }

        if let Some(badie) = &mut self.badie {
            // Our badie moves horizontally back & forth between two points.
            if badie.body.x() < BADIE_END_X {
                badie.flip = true;
            } else if badie.body.x() > BADIE_START_X {
                badie.flip = false;
            }
            if badie.flip {
                badie.body.move_by(BADIE_SPEED, 0.0);
            } else {
                badie.body.move_by(-BADIE_SPEED, 0.0);
            }
        }

        // Coins & trophy: sample the hitbox center.
        let cx = (self.hero.body.x() as i16 + 4) / 8;
        let cy = (self.hero.body.y() as i16 + 4) / 8;
        match ctx.map_tile(cx, cy) {
            Some(COIN_SPRITE) => {
                ctx.set_map_tile(cx, cy, SpriteId(0)).unwrap();
                self.taken.push(Taken::new_coin(cx, cy)).unwrap();
                ctx.sfx(COIN_SFX);
            }
            Some(TROPHY_SPRITE) => {
                ctx.set_map_tile(cx, cy, SpriteId(0)).unwrap();
                self.taken.push(Taken::new_trophy(cx, cy)).unwrap();
                // Another music can't be playing becase `PlayingMusic` instace has had to have been
                // dropped when the game mode switch away from `Ended`.
                let music = ctx.music(COMPLETION_MUSIC).play().unwrap();
                self.mode = GameMode::Ended {
                    time: ctx.time(),
                    flash: false,
                    _music: music,
                    won: true,
                };
            }
            _ => (),
        }
    }

    fn restart_game(&mut self, ctx: &mut Context) {
        self.hero = Hero::new();
        self.vx = 0.0;
        self.vy = 0.0;
        self.grounded = false;
        self.badie = Some(Badie::new());
        self.frame = 0;
        self.mode.start(ctx);
        // Put all the rewards back on the map.
        for Taken { x, y, sprite, .. } in &self.taken {
            ctx.set_map_tile(*x, *y, *sprite).unwrap();
        }
        self.taken.clear();
    }

    fn game_over(&mut self, ctx: &mut Context) {
        let music = ctx.music(GAME_OVER_MUSIC).play().unwrap();
        self.mode = GameMode::Ended {
            time: ctx.time(),
            flash: false,
            _music: music,
            won: false,
        };
    }

    fn draw_hero(&self, gfx: &mut Graphics) {
        let is_alt_frame = (self.frame / 4).is_multiple_of(2);
        if self.hero.dead && is_alt_frame {
            // If hero dies, we show them flashing in & out of existence.
            return;
        }

        let sprite = if !self.grounded || (self.vx != 0.0 && is_alt_frame) {
            match self.mode {
                GameMode::Ended { won, .. } if won => HERO_HAPPY_SPRITE,
                GameMode::InGame { .. } | GameMode::Ended { .. } => HERO_LEGS_EXTEND_SPRITE,
                GameMode::Init => unreachable!(),
            }
        } else {
            HERO_SPRITE
        };
        // The body's coherent pixel — a running jump climbs cleanly, no zigzag.
        gfx.sprite_ext(
            sprite,
            self.hero.body.draw_x(),
            self.hero.body.draw_y(),
            8,
            8,
            self.hero.flip,
            false,
        )
        .unwrap();
    }

    fn draw_badie(&self, gfx: &mut Graphics) {
        let Some(badie) = &self.badie else {
            return;
        };
        let sprite = match self.mode {
            GameMode::InGame { .. } if (self.frame / 4).is_multiple_of(2) => BADIE_ALT_SPRITE,
            GameMode::Ended { .. } | GameMode::InGame { .. } => BADIE_SPRITE,
            GameMode::Init => unreachable!(),
        };
        gfx.sprite_ext(
            sprite,
            badie.body.draw_x(),
            badie.body.draw_y(),
            8,
            8,
            badie.flip,
            false,
        )
        .unwrap();
    }
}

impl Game for Platformer {
    fn update(&mut self, ctx: &mut Context) {
        self.frame += 1;

        match &mut self.mode {
            mode @ GameMode::Init => mode.start(ctx),
            GameMode::InGame { .. } => self.in_game_update(ctx),
            GameMode::Ended { time, .. } if ctx.time() - *time > GAME_OVER_TIMEOUT => {
                self.restart_game(ctx)
            }
            // Flash on every 16th frame if game ended with winning.
            GameMode::Ended {
                flash, won: true, ..
            } => *flash = self.frame.is_multiple_of(16),
            GameMode::Ended { .. } => (),
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        if matches!(self.mode, GameMode::Ended { flash: true, .. }) {
            gfx.clear(Color::WHITE);
        } else {
            gfx.clear(Color::DARK_BLUE);
        }

        // Camera follows the player across the 32-tile-wide level.
        let cam = (self.hero.body.x() - 60.0).clamp(8.0, (32 * 8 - SCREEN_WIDTH as i16) as f32);
        gfx.camera(cam as i16, 0);
        gfx.map(0, 0, 0, 0, 32, 16, BitFlags::empty()).unwrap();

        self.draw_hero(gfx);
        self.draw_badie(gfx);

        gfx.camera(0, 0);

        let score = self.taken.iter().fold(0, |acc, r| acc + r.points)
            + self.badies_killed * BADIE_KILL_POINTS;
        printf!(gfx, 2, 2, Color::YELLOW, "Score {}", score);

        if let GameMode::InGame { time_left, .. } = self.mode {
            let color = if time_left < 5 {
                Color::RED
            } else {
                Color::YELLOW
            };
            printf!(
                gfx,
                (SCREEN_WIDTH - 3 * 4) as i16,
                2,
                color,
                "{:>2}s",
                time_left
            );
        }
    }
}

#[derive(Debug)]
struct Hero {
    body: Body,
    flip: bool,
    dead: bool,
}

impl Hero {
    fn new() -> Self {
        Self {
            body: Body::new(16.0, 80.0),
            flip: false,
            dead: false,
        }
    }
}

#[derive(Debug)]
struct Badie {
    body: Body,
    flip: bool,
}

impl Badie {
    fn new() -> Self {
        Self {
            body: Body::new(BADIE_START_X, BADIE_Y),
            flip: false,
        }
    }
}

#[derive(Debug)]
enum GameMode {
    Init,
    InGame {
        start_time: f32,
        time_left: u8,
    },
    Ended {
        time: f32,
        flash: bool,
        _music: PlayingMusic,
        won: bool,
    },
}

impl GameMode {
    fn start(&mut self, ctx: &mut Context) {
        assert!(matches!(self, Self::Init | Self::Ended { .. }));

        *self = Self::InGame {
            start_time: ctx.time(),
            time_left: GAME_TIMEOUT,
        };
    }
}

#[derive(Debug)]
struct Taken {
    x: i16,
    y: i16,
    sprite: SpriteId,
    points: u8,
}

impl Taken {
    fn new_coin(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            sprite: COIN_SPRITE,
            points: COIN_POINTS,
        }
    }
    fn new_trophy(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            sprite: TROPHY_SPRITE,
            points: TROPHY_POINTS,
        }
    }
}

const MAX_TAKEN: usize = 8;

const SOLID: SpriteFlag = SpriteFlag::Flag0;
// Sub-pixel run speed, so a running jump is a sub-pixel diagonal — the motion
// Body keeps coherent. At a whole pixel per frame there would be no zigzag.
const HERO_SPEED: f32 = 0.7;
// Our badie moves slower.
const BADIE_SPEED: f32 = 0.5;

const HERO_SPRITE: SpriteId = SpriteId(1);
const HERO_LEGS_EXTEND_SPRITE: SpriteId = SpriteId(2);
const HERO_HAPPY_SPRITE: SpriteId = SpriteId(5);
const HERO_WIDTH: i16 = 8;
const HERO_HEIGHT: i16 = 7;

const BADIE_SPRITE: SpriteId = SpriteId(6);
const BADIE_ALT_SPRITE: SpriteId = SpriteId(7);
const BADIE_START_X: f32 = (SCREEN_WIDTH * 2 - 16) as f32;
const BADIE_END_X: f32 = (SCREEN_WIDTH * 2 - 8 * 8) as f32;
const BADIE_Y: f32 = (SCREEN_HEIGHT - 3 * 8) as f32;
const BADIE_WIDTH: i16 = 8;
const BADIE_HEIGHT: i16 = 7;

const COIN_SPRITE: SpriteId = SpriteId(3);
const TROPHY_SPRITE: SpriteId = SpriteId(4);
const COIN_POINTS: u8 = 1;
const TROPHY_POINTS: u8 = 4;
const BADIE_KILL_POINTS: u8 = 4;
const GAME_TIMEOUT: u8 = 30;
const GAME_OVER_TIMEOUT: f32 = 5.0;

const JUMP_SFX: SfxId = SfxId::new(0).unwrap();
const COIN_SFX: SfxId = SfxId::new(1).unwrap();
const BADIE_DEAD_SFX: SfxId = SfxId::new(4).unwrap();
const COMPLETION_MUSIC: MusicId = MusicId::new(0).unwrap();
const GAME_OVER_MUSIC: MusicId = MusicId::new(1).unwrap();
