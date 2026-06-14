//! Music playback: O starts the song, X stops it. The bars dance.

use rico8::*;

struct MusicDemo {
    playing: bool,
    t: u32,
}

impl Game for MusicDemo {
    fn update(&mut self, ctx: &mut Context) {
        self.t += 1;
        if ctx.btnp(Button::O) {
            ctx.music(MusicId(0));
            self.playing = true;
        }
        if ctx.btnp(Button::X) {
            ctx.music_stop();
            self.playing = false;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("music demo", 44, 8, Color::WHITE);
        gfx.print("z: play  x: stop", 32, 116, Color::LIGHT_GREY);
        // Dancing bars while playing, flat while stopped.
        for i in 0..16 {
            let phase = (self.t as f32 / 4.0 + i as f32) % 8.0;
            let h = if self.playing {
                8 + ((phase - 4.0).abs() * 8.0) as i32
            } else {
                4
            };
            let c = Color::from_index(8 + (i % 8) as u8);
            gfx.rect_fill(8 + i * 7, 96 - h, 5, h, c);
        }
        if self.playing {
            gfx.print("now playing: pattern 0", 20, 40, Color::GREEN);
        }
    }
}

rico8::game!(MusicDemo {
    playing: false,
    t: 0,
});
