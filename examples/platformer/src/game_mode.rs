use rico8::{Context, PlayingMusic};

use crate::constants::GAME_TIMEOUT;

#[derive(Debug)]
pub enum GameMode {
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
    pub fn start(&mut self, ctx: &mut Context) {
        assert!(matches!(self, Self::Init | Self::Ended { .. }));

        *self = Self::InGame {
            start_time: ctx.time(),
            time_left: GAME_TIMEOUT,
        };
    }
}
