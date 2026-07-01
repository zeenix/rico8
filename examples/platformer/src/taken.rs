use rico8::{Context, SpriteId};

use crate::constants::{COIN_POINTS, COIN_SPRITE, TROPHY_POINTS, TROPHY_SPRITE};

#[derive(Debug)]
pub struct Taken {
    x: i16,
    y: i16,
    sprite: SpriteId,
    points: u8,
}

impl Taken {
    pub fn new_coin(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            sprite: COIN_SPRITE,
            points: COIN_POINTS,
        }
    }

    pub fn new_trophy(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            sprite: TROPHY_SPRITE,
            points: TROPHY_POINTS,
        }
    }

    pub fn put_back(self, ctx: &mut Context) {
        // Put all the rewards back on the map.
        ctx.set_map_tile(self.x, self.y, self.sprite).unwrap();
    }

    pub fn is_trophy(&self) -> bool {
        self.sprite == TROPHY_SPRITE
    }

    pub fn points(&self) -> u8 {
        self.points
    }
}
