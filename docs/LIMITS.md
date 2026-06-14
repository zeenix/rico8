# Cart limits

RICO-8 is a fantasy console with deliberate constraints — that's the point.
Like PICO-8 before it, every limit is part of what makes a RICO-8 cart a
*RICO-8 cart*. All three runtime limits are built around one easy number:
**128 K**. If you keep that number in mind, you will never be surprised.

| what            | limit   | what happens when you exceed it              |
| --------------- | ------- | -------------------------------------------- |
| cart size       | 128 KiB | build warns; `export` is rejected            |
| RAM             | 128 KiB | "ran out of memory" error screen             |
| per-frame work  | 128 K   | "ran too long (infinite loop?)" error screen |

---

## Cart size — 128 KiB

The compiled game wasm must be no larger than 128 KiB. If you try to export
an over-size cart, the export is rejected. Building an over-size cart locally
also prints a warning, so you know before you share it.

In practice a simple game is tiny — a few kilobytes. The limit is mostly about
dependency discipline: keep your Rust dependencies lean and your cart stays
small. The built-in shrink profile (`opt-level = "s"`, `lto = true`,
`panic = "abort"`) already applies; fighting for the last byte is rarely
needed.

## RAM — 128 KiB

A running cart may use at most 128 KiB of total RAM. Overrunning that budget
stops the cart immediately and shows a "ran out of memory" error screen. The
player can then return to the console and you can inspect the output.

Simple games, especially those without a heap, use a small fraction of this.
Even a game that uses the default allocator freely will typically stay well
inside the budget.

## Per-frame work — 128 K

Each call to `update` and each call to `draw` has a fixed work budget.
A cart that runs an infinite loop, or simply does too much in a single frame,
will be stopped and shown a "ran too long (infinite loop?)" error screen
instead of freezing the console.

Real game logic — move sprites, read input, draw a tilemap — uses a tiny
fraction of the budget. The limit catches runaway loops during development,
not carefully written games.

---

## Staying small — `#![no_std]` + `heapless`

Default carts just work: the `rico8` dependency brings in `std` and a heap
allocator, and you write ordinary Rust. No special effort needed.

If you want the absolute smallest, most predictable cart — no heap, no
allocator overhead, fully static memory — you can go `no_std`:

```toml
# Cargo.toml
[dependencies]
rico8 = { path = "../../rico8", default-features = false }
heapless = "0.8"

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
```

```rust
// src/lib.rs
#![no_std]
use heapless::Vec as HVec;
use rico8::*;

struct MyGame {
    // Fixed-capacity, stack-backed — no heap allocation.
    trail: HVec<(i32, i32), 16>,
    x: i32,
    y: i32,
}

impl Game for MyGame {
    fn update(&mut self, ctx: &mut Context) {
        // ...
    }
    fn draw(&self, gfx: &mut Graphics) {
        // ...
    }
}

rico8::game!(MyGame { trail: HVec::new(), x: 64, y: 64 });
```

`heapless` collections are bounded at compile time (`Vec<T, N>`,
`String<N>`, `FnvIndexMap<K, V, N>`, …), so there is nothing to allocate and
nothing to fail at runtime due to memory pressure.

See [`examples/hello_nostd`](../examples/hello_nostd) for a complete, runnable
cart using this approach.
