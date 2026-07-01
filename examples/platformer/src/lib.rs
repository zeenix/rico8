//! A tiny platformer: run, jump, collect coins. Solid tiles carry sprite
//! flag 0; coins are tile 3 and are collected by rewriting the map.
//!
//! The player is a [`Body`], so a running jump (hold Right + jump) — a
//! sub-pixel diagonal — climbs a clean staircase instead of shimmering. The
//! body owns the position; the cart just hands it the movement it worked out
//! for the frame and draws at `draw_x`/`draw_y`.

#![no_std]

mod badie;
mod constants;
mod game_mode;
mod hero;

use heapless::Vec;
use rico8::*;

use badie::*;
use constants::*;
use game_mode::*;
use hero::*;

game!(Platformer {
    hero: Hero::new(),
    badie: Some(Badie::new()),
    taken: Vec::new(),
    badies_killed: 0,
    frame: 0,
    mode: GameMode::Init,
});

struct Platformer {
    hero: Hero,
    badie: Option<Badie>,
    taken: Vec<Taken, MAX_TAKEN>,
    badies_killed: u8,
    frame: u32,
    mode: GameMode,
}

impl Platformer {
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

        if let Some(taken) = self.hero.update(ctx) {
            let took_trophy = taken.sprite == TROPHY_SPRITE;
            self.taken.push(taken).unwrap();
            if took_trophy {
                // Another music can't be playing becase `PlayingMusic` instace has had to have been
                // dropped when the game mode switch away from `Ended`.
                let music = ctx.music(COMPLETION_MUSIC).play().unwrap();
                self.mode = GameMode::Ended {
                    time: ctx.time(),
                    flash: false,
                    _music: music,
                    won: true,
                };

                return;
            }
        }

        // Check for collision between our hero and the badie, and decide who dies if there is one.
        if let Some(badie) = &mut self.badie {
            if self.hero.draw_x() + HERO_WIDTH >= badie.draw_x()
                && self.hero.draw_x() < badie.draw_x() + BADIE_WIDTH
            {
                if self.hero.draw_y() == badie.draw_y() {
                    // Hero ramming into badie horizontally is a suicide.
                    self.hero.die();
                    self.game_over(ctx);

                    return;
                } else if self.hero.draw_y() + HERO_HEIGHT >= badie.draw_y()
                    && self.hero.draw_y() < badie.draw_y() + BADIE_HEIGHT
                {
                    // Hero hitting the badie from the top, kills the badie and gives hero a boost.
                    self.badie = None;
                    self.badies_killed += 1;
                    self.hero.jump(ctx);
                    ctx.sfx(BADIE_DEAD_SFX);
                }
            }
        }

        if let Some(badie) = &mut self.badie {
            badie.update(ctx);
        }
    }

    fn restart_game(&mut self, ctx: &mut Context) {
        self.hero = Hero::new();
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

        self.hero.center(gfx);
        gfx.map(0, 0, 0, 0, 32, 16, BitFlags::empty()).unwrap();

        self.hero.draw(gfx, self.frame, &self.mode);
        if let Some(badie) = &self.badie {
            badie.draw(gfx, self.frame, &self.mode);
        }

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
