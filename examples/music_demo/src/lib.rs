//! Music playback: O starts the song (fading in), X fades it out. The bars dance.

#![no_std]

use rico8::*;

game!(MusicDemo { music: None, t: 0 });

struct MusicDemo {
    music: Option<PlayingMusic>,
    t: u32,
}

impl Game for MusicDemo {
    fn update(&mut self, ctx: &mut Context) {
        self.t += 1;
        if ctx.is_button_pressed(Button::O) && self.music.is_none() {
            self.music = ctx.music(MusicId(0)).fade_in(500).play().ok();
        }
        if ctx.is_button_pressed(Button::X) {
            if let Some(m) = self.music.take() {
                m.fade_out(500).stop();
            }
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.print("Music demo", 44, 8, Color::WHITE);
        gfx.print("Z: play  X: stop", 32, 116, Color::LIGHT_GREY);
        let playing = self.music.is_some();
        // Dancing bars while playing, flat while stopped.
        for i in 0..16_i32 {
            let phase = (self.t as f32 / 4.0 + i as f32) % 8.0;
            let h = if playing {
                let d = phase - 4.0;
                let d = if d < 0.0 { -d } else { d };
                8 + (d * 8.0) as i32
            } else {
                4
            };
            let c = Color::from_index(8 + (i % 8) as u8);
            gfx.rect_fill(8 + i * 7, 96 - h, 5, h, c).unwrap();
        }
        if playing {
            gfx.print("Now playing: pattern 0", 20, 40, Color::GREEN);
        }
    }
}
