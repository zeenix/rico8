//! Stress cart: probes the three 128 K budgets, each independently.
//!
//! Toggle the live resource overlay with F1, then ramp one probe until its cap
//! trips. Controls:
//!   - Up/Down    : heap +/- one 4 KiB block
//!   - O/X        : shadow stack +/- one ~1 KiB frame
//!   - Right/Left : CPU +/- 4000 arithmetic iterations per frame
//!
//! What you read off:
//!   - "ran out of memory" while ramping heap -> the 128 K memory budget,
//!   - a trap while ramping stack            -> the shadow-stack reserve,
//!   - "ran too long" while ramping CPU      -> the per-call fuel budget.
//!
//! Every step is deterministic: heap blocks are exact 4 KiB boxes (no amortized
//! over-allocation), each stack frame is a fixed 1 KiB, and CPU work is a fixed
//! count — so the cap trips at a predictable level.
use rico8::*;

game!(Stress {
    heap: Vec::new(),
    stack_n: 0,
    cpu_n: 0,
    checksum: 0
});

/// Bytes per heap block.
const HEAP_BLOCK: usize = 4096;
/// Bytes of shadow stack burned per recursion level.
const STACK_FRAME: usize = 1024;
/// Arithmetic iterations per CPU level, per frame.
const CPU_ITERS: u32 = 4000;

struct Stress {
    heap: Vec<Box<[u8; HEAP_BLOCK]>>,
    stack_n: u32,
    cpu_n: u32,
    checksum: u32,
}

impl Game for Stress {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.is_button_pressed(Button::Up) {
            self.heap.push(Box::new([0x5a; HEAP_BLOCK]));
        }
        if ctx.is_button_pressed(Button::Down) {
            self.heap.pop();
        }
        if ctx.is_button_pressed(Button::O) {
            self.stack_n += 1;
        }
        if ctx.is_button_pressed(Button::X) && self.stack_n > 0 {
            self.stack_n -= 1;
        }
        if ctx.is_button_pressed(Button::Right) {
            self.cpu_n += 1;
        }
        if ctx.is_button_pressed(Button::Left) && self.cpu_n > 0 {
            self.cpu_n -= 1;
        }

        // Burn the configured stack depth and CPU each frame, and touch the
        // heap in O(1), so nothing is optimized away.
        let mut acc = burn_stack(self.stack_n);
        for i in 0..(self.cpu_n * CPU_ITERS) {
            acc = acc.wrapping_mul(31).wrapping_add(i);
        }
        let touch = self.heap.first().map_or(0, |b| b[0] as u32) ^ self.heap.len() as u32;
        self.checksum = acc ^ touch;
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        // Draw in the vertical middle so the F1 stats overlay (top corners)
        // never covers the readouts.
        gfx.print("Stress  F1 = stats", 2.0, 50.0, Color::WHITE);
        printf!(
            gfx,
            2.0,
            62.0,
            Color::GREEN,
            "Heap blk  (U/D): {}",
            self.heap.len()
        );
        printf!(
            gfx,
            2.0,
            70.0,
            Color::from_index(12),
            "Stack frm (O/X): {}",
            self.stack_n
        );
        printf!(
            gfx,
            2.0,
            78.0,
            Color::from_index(9),
            "CPU lvl   (R/L): {}",
            self.cpu_n
        );
    }
}

/// Recurse `depth` levels, each burning `STACK_FRAME` bytes of shadow stack.
/// `black_box(&mut frame)` forces the whole array onto the stack (otherwise the
/// optimizer keeps only the bytes it sees used), so each level costs a real
/// `STACK_FRAME` bytes.
#[inline(never)]
fn burn_stack(depth: u32) -> u32 {
    let mut frame = [0xa5u8; STACK_FRAME];
    core::hint::black_box(&mut frame);
    let here = frame[depth as usize % STACK_FRAME] as u32 ^ depth;
    if depth == 0 {
        here
    } else {
        here.wrapping_add(burn_stack(depth - 1))
    }
}
