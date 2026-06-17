//! Stress cart: probes the three 128 K budgets on real hardware.
//!
//! A trap ends the cart, so the three probes are independently toggleable —
//! enable one, then ramp N until that cap trips. Controls:
//!   - Up/Down : change workload level N
//!   - O       : toggle the memory probe  (hold a heap Vec of N*4 KiB)
//!   - X       : toggle the compute probe (run N*2000 arithmetic iterations)
//!   - drawing N*20 shapes is always on   (the wall-clock baseline)
//!
//! What you read off:
//!   - "ran out of memory" with only MEM on -> N at the 128 K memory budget,
//!   - "ran too long" with only CPU on      -> N at the 128 K fuel budget,
//!   - frame drop with both off             -> draw-only wall-clock limit.
use rico8::*;

struct Stress {
    n: i32,
    mem_on: bool,
    cpu_on: bool,
    blob: Vec<u8>,
    checksum: u32,
}

impl Game for Stress {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.is_button_pressed(Button::Up) {
            self.n += 1;
        }
        if ctx.is_button_pressed(Button::Down) && self.n > 0 {
            self.n -= 1;
        }
        if ctx.is_button_pressed(Button::O) {
            self.mem_on = !self.mem_on;
        }
        if ctx.is_button_pressed(Button::X) {
            self.cpu_on = !self.cpu_on;
        }

        // Memory probe: hold N * 4 KiB on the heap (else release it).
        let want = if self.mem_on {
            (self.n as usize) * 4 * 1024
        } else {
            0
        };
        self.blob.resize(want, 0x5a);

        // Compute probe: N * 2000 arithmetic iterations.
        let mut acc: u32 = 1;
        if self.cpu_on {
            for i in 0..(self.n * 2000) {
                acc = acc.wrapping_mul(31).wrapping_add(i as u32);
            }
        }
        // Touch the blob in O(1) so the optimizer cannot drop the allocation,
        // without the memory probe doing per-frame work proportional to its size.
        let touch = self.blob.first().copied().unwrap_or(0) as u32 ^ self.blob.len() as u32;
        self.checksum = acc ^ touch;
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        // Wall-clock baseline: N * 20 shapes.
        for i in 0..(self.n * 20) {
            let x = ((i * 7) % 128) as f32;
            let y = ((i * 13) % 128) as f32;
            gfx.rect_fill(x, y, 6.0, 6.0, Color::from_index((i % 15 + 1) as u8));
        }
        gfx.print("STRESS", 2.0, 2.0, Color::WHITE);
        let mut label = String::from("N=");
        push_int(&mut label, self.n);
        gfx.print(&label, 2.0, 10.0, Color::YELLOW);
        // The 4x6 font has no lowercase glyphs, so on/off must not rely on
        // letter case. Spell it out, and color it too.
        gfx.print(
            if self.mem_on { "MEM ON" } else { "MEM OFF" },
            2.0,
            18.0,
            if self.mem_on {
                Color::GREEN
            } else {
                Color::DARK_GREY
            },
        );
        gfx.print(
            if self.cpu_on { "CPU ON" } else { "CPU OFF" },
            2.0,
            26.0,
            if self.cpu_on {
                Color::GREEN
            } else {
                Color::DARK_GREY
            },
        );
    }
}

fn push_int(s: &mut String, mut v: i32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut digits = [0u8; 12];
    let mut i = 0;
    while v > 0 {
        digits[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(digits[i] as char);
    }
}

rico8::game!(Stress {
    n: 1,
    mem_on: false,
    cpu_on: false,
    blob: Vec::new(),
    checksum: 0
});
