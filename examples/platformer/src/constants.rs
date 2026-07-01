use rico8::{MusicId, SfxId, SpriteFlag, SpriteId, SCREEN_HEIGHT, SCREEN_WIDTH};

pub const MAX_TAKEN: usize = 8;

pub const SOLID: SpriteFlag = SpriteFlag::Flag0;
// Sub-pixel run speed, so a running jump is a sub-pixel diagonal — the motion
// Body keeps coherent. At a whole pixel per frame there would be no zigzag.
pub const HERO_SPEED: f32 = 0.7;
// Our badie moves slower.
pub const BADIE_SPEED: f32 = 0.5;

pub const HERO_SPRITE: SpriteId = SpriteId(1);
pub const HERO_LEGS_EXTEND_SPRITE: SpriteId = SpriteId(2);
pub const HERO_HAPPY_SPRITE: SpriteId = SpriteId(5);
pub const HERO_WIDTH: i16 = 8;
pub const HERO_HEIGHT: i16 = 7;

pub const BADIE_SPRITE: SpriteId = SpriteId(6);
pub const BADIE_ALT_SPRITE: SpriteId = SpriteId(7);
pub const BADIE_START_X: f32 = (SCREEN_WIDTH * 2 - 16) as f32;
pub const BADIE_END_X: f32 = (SCREEN_WIDTH * 2 - 8 * 8) as f32;
pub const BADIE_Y: f32 = (SCREEN_HEIGHT - 3 * 8) as f32;
pub const BADIE_WIDTH: i16 = 8;
pub const BADIE_HEIGHT: i16 = 7;

pub const COIN_SPRITE: SpriteId = SpriteId(3);
pub const TROPHY_SPRITE: SpriteId = SpriteId(4);
pub const COIN_POINTS: u8 = 1;
pub const TROPHY_POINTS: u8 = 4;
pub const BADIE_KILL_POINTS: u8 = 4;
pub const GAME_TIMEOUT: u8 = 30;
pub const GAME_OVER_TIMEOUT: f32 = 5.0;

pub const JUMP_SFX: SfxId = SfxId::new(0).unwrap();
pub const COIN_SFX: SfxId = SfxId::new(1).unwrap();
pub const BADIE_DEAD_SFX: SfxId = SfxId::new(4).unwrap();
pub const COMPLETION_MUSIC: MusicId = MusicId::new(0).unwrap();
pub const GAME_OVER_MUSIC: MusicId = MusicId::new(1).unwrap();
