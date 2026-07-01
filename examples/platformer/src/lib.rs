//! A tiny platformer: run and jump across a scrolling level, collect coins,
//! grab the trophy to win, and dodge or stomp the patrolling badie — all
//! before the 30-second clock runs out. Solid tiles carry sprite flag 0;
//! coins (tile 3) and the trophy (tile 4) are collected by rewriting the map,
//! and put back when the game restarts.
//!
//! The hero owns a [`Body`], so a running jump (hold Right + jump) — a
//! sub-pixel diagonal — climbs a clean staircase instead of shimmering. The
//! body owns the position; the cart just hands it the movement it worked out
//! for the frame and draws at `draw_x`/`draw_y`.
//!
//! The code is split into small modules: `hero` and `badie` (the two moving
//! actors), `taken` (a collected coin or trophy, so it can be scored and put
//! back), `game_mode` (the `Init` → `InGame` → `Ended` state machine), and
//! `constants`.

#![no_std]

mod badie;
mod constants;
mod game_mode;
mod hero;
mod taken;

use heapless::Vec;
use rico8::*;

use crate::{
    badie::Badie,
    constants::{
        BADIE_DEAD_SFX, BADIE_HEIGHT, BADIE_KILL_POINTS, BADIE_WIDTH, COMPLETION_MUSIC,
        GAME_OVER_MUSIC, GAME_OVER_TIMEOUT, GAME_TIMEOUT, HERO_HEIGHT, HERO_WIDTH, MAX_TAKEN,
    },
    game_mode::GameMode,
    hero::Hero,
    taken::Taken,
};

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
            let took_trophy = taken.is_trophy();
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
        self.badies_killed = 0;
        for taken in self.taken.drain(..) {
            taken.put_back(ctx);
        }
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

        let score = self.taken.iter().fold(0, |acc, r| acc + r.points())
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
