//! Music playback: O starts the song, X stops it. The bars dance.

#![no_std]

use rico8::*;

struct MusicDemo {
    playing: bool,
    t: u32,
}

impl Game for MusicDemo {
    fn update(&mut self, ctx: &mut Context) {
        self.t += 1;
        if ctx.is_button_pressed(Button::O) {
            ctx.music(MusicId(0));
            self.playing = true;
        }
        if ctx.is_button_pressed(Button::X) {
            ctx.music_stop();
            self.playing = false;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("Music demo", 44.0, 8.0, Color::WHITE);
        gfx.print("Z: play  X: stop", 32.0, 116.0, Color::LIGHT_GREY);
        // Dancing bars while playing, flat while stopped.
        for i in 0..16 {
            let phase = (self.t as f32 / 4.0 + i as f32) % 8.0;
            let h = if self.playing {
                let d = phase - 4.0;
                let d = if d < 0.0 { -d } else { d };
                8 + (d * 8.0) as i32
            } else {
                4
            };
            let c = Color::from_index(8 + (i % 8) as u8);
            gfx.rect_fill((8 + i * 7) as f32, (96 - h) as f32, 5.0, h as f32, c);
        }
        if self.playing {
            gfx.print("Now playing: pattern 0", 20.0, 40.0, Color::GREEN);
        }
    }
}

rico8::game!(MusicDemo {
    playing: false,
    t: 0,
});
