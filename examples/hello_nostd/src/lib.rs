//! The minimal path: `#![no_std]`, no allocator, `heapless` for collections.
#![no_std]
use heapless::Vec as HVec;
use rico8::*;

struct Game0 {
    /// Trail of recent positions — stack/static-backed, no heap.
    trail: HVec<(i32, i32), 16>,
    x: i32,
    y: i32,
}

impl Game for Game0 {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.btn(Button::Left) {
            self.x -= 1;
        }
        if ctx.btn(Button::Right) {
            self.x += 1;
        }
        if ctx.btn(Button::Up) {
            self.y -= 1;
        }
        if ctx.btn(Button::Down) {
            self.y += 1;
        }
        if self.trail.is_full() {
            self.trail.remove(0);
        }
        let _ = self.trail.push((self.x, self.y));
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::DARK_BLUE);
        gfx.print("no_std + heapless", 18, 40, Color::WHITE);
        for &(x, y) in &self.trail {
            gfx.pset(x, y, Color::PINK);
        }
        gfx.rect_fill(self.x, self.y, 4, 4, Color::YELLOW);
    }
}

rico8::game!(Game0 {
    trail: HVec::new(),
    x: 64,
    y: 64
});
