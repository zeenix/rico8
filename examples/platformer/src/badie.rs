use rico8::{Body, Context, Graphics};

use crate::{
    constants::{BADIE_ALT_SPRITE, BADIE_END_X, BADIE_SPEED, BADIE_SPRITE, BADIE_START_X, BADIE_Y},
    GameMode,
};

#[derive(Debug)]
pub struct Badie {
    body: Body,
    flip: bool,
}

impl Badie {
    pub fn new() -> Self {
        Self {
            body: Body::new(BADIE_START_X, BADIE_Y),
            flip: false,
        }
    }

    pub fn update(&mut self, _ctx: &mut Context) {
        // Our badie moves horizontally back & forth between two points.
        if self.body.x() < BADIE_END_X {
            self.flip = true;
        } else if self.body.x() > BADIE_START_X {
            self.flip = false;
        }
        if self.flip {
            self.body.move_by(BADIE_SPEED, 0.0);
        } else {
            self.body.move_by(-BADIE_SPEED, 0.0);
        }
    }

    pub fn draw(&self, gfx: &mut Graphics, frame: u32, mode: &GameMode) {
        let sprite = match mode {
            GameMode::InGame { .. } if (frame / 4).is_multiple_of(2) => BADIE_ALT_SPRITE,
            GameMode::Ended { .. } | GameMode::InGame { .. } => BADIE_SPRITE,
            GameMode::Init => unreachable!(),
        };
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
}
