# The RICO-8 WASM ABI

This is the complete surface between a cart and the console. Carts are
`wasm32-unknown-unknown` modules; everything below lives in the import
module **`"rico8"`**. There is intentionally nothing else: no WASI, no
filesystem, no network, no host memory access. Game code should use the
`rico8` SDK crate, which wraps all of this safely — this document is for
people working on the console itself, alternate SDKs, or curiosity.

**Screen-space positions and sizes are `f32`; everything discrete is an
integer.** Coordinates (`x`/`y`), extents (`w`/`h`, radius) and the camera
are `f32`: the host **floors** each to a pixel at draw time, so a cart can
hold sub-pixel state (smooth motion, no shimmer crossing `x = 0`) without
rounding itself. `spr`'s `w`/`h` are also `f32` — they measure sprite cells
and may be fractional (`w = 0.5` draws a 4-pixel slice). Indices and counts
— sprite/tile numbers, map cel coordinates, flags, buttons, channels,
colors — stay `i32`/`u32`. Colors are palette indices `0..16` (masked with
`0x0f`).
Out-of-range coordinates are safe everywhere: draws clip, reads return 0.

(Why not narrower integer types for positions? WebAssembly function
signatures only have `i32`/`i64`/`f32`/`f64` — there is no `i8`/`u8`/`i16`
at the ABI boundary — and positions are deliberately unbounded, drawn
off-screen and scrolled in by the camera. `f32` is the same 32 bits on the
wire and buys sub-pixel precision for free.)

## Guest exports (required)

| export         | signature | called                                  |
| -------------- | --------- | --------------------------------------- |
| `rico8_init`   | `() -> ()`| once, after the module is instantiated  |
| `rico8_update` | `() -> ()`| `rico8_fps` times per second            |
| `rico8_draw`   | `() -> ()`| after each update                       |
| `memory`       | memory    | read by `print`/`log`/`panic`           |

Each call runs under a fuel budget (~100M instructions). Exhausting it
traps with a "ran too long" error screen — infinite loops cannot hang
the console.

## Guest exports (optional)

| export      | signature   | called                                |
| ----------- | ----------- | ------------------------------------- |
| `rico8_fps` | `() -> u32` | once, after `rico8_init`              |

`rico8_fps` reports the cart's logical frame rate. The SDK emits it from
every cart; `30` and `60` are honored, and `60` is the default. A missing
export, or any other value, also means 60, so a hand-written cart that
omits it still runs.

## Host imports

### Drawing

| function | signature | notes |
| --- | --- | --- |
| `cls` | `(color: i32)` | fill the screen |
| `camera` | `(x: f32, y: f32)` | offset subsequent draws by `(-x, -y)`; floored |
| `clip` | `(x, y, w, h: f32)` | restrict drawing; `clip(0,0,128,128)` resets |
| `pset` | `(x, y: f32, color: i32)` | set one pixel |
| `pget` | `(x, y: f32) -> i32` | read one pixel (screen space) |
| `line` | `(x0, y0, x1, y1: f32, color: i32)` | inclusive endpoints |
| `rect` | `(x0, y0, x1, y1: f32, color: i32)` | outline, inclusive corners |
| `rectfill` | `(x0, y0, x1, y1: f32, color: i32)` | filled |
| `circ` | `(x, y, r: f32, color: i32)` | outline |
| `circfill` | `(x, y, r: f32, color: i32)` | filled |
| `print` | `(ptr: u32, len: u32, x, y: f32, color: i32) -> f32` | UTF-8 text from guest memory; returns the x position after the last glyph |

Every `f32` coordinate/extent is floored to a pixel by the host at draw
time (`floor`, not truncation, so motion stays even across `x = 0`).

### Sprites and map

| function | signature | notes |
| --- | --- | --- |
| `spr` | `(n: u32, x, y: f32, w, h: f32, flip_x, flip_y: i32)` | draw a `w x h`-sprite block (fractional `w`/`h` draw a partial slice); color 0 is transparent |
| `map` | `(cel_x, cel_y: i32, sx, sy: f32, cel_w, cel_h: i32, layers: u32)` | draw map tiles; nonzero `layers` draws only tiles whose sprite flags intersect the mask |
| `mget` | `(x, y: i32) -> i32` | read a map tile |
| `mset` | `(x, y: i32, v: u32)` | write a map tile (RAM only; discarded on reload) |
| `fget` | `(n: u32) -> i32` | sprite flag bitmask |
| `fset` | `(n: u32, flags: u32)` | overwrite sprite flags |

### Input

| function | signature | notes |
| --- | --- | --- |
| `btn` | `(b: u32) -> i32` | held? buttons: 0 left, 1 right, 2 up, 3 down, 4 O, 5 X |
| `btnp` | `(b: u32) -> i32` | just pressed? repeats after 15 frames, then every 4 |

### Audio

| function | signature | notes |
| --- | --- | --- |
| `sfx` | `(n: i32, channel: i32)` | play SFX `n`; `channel < 0` picks a free channel; `n < 0` stops `channel` |
| `music` | `(n: i32)` | start music at pattern `n`; `n < 0` stops |

### Misc

| function | signature | notes |
| --- | --- | --- |
| `time` | `() -> f32` | seconds since init, in `1/fps` steps (see `rico8_fps`) |
| `rnd` | `() -> f32` | uniform in `[0, 1)` (host RNG) |
| `log` | `(ptr: u32, len: u32)` | line to the RICO-8 console |
| `panic` | `(ptr: u32, len: u32)` | record a panic message; the SDK's panic hook calls this right before the trap so the error screen shows the real message |

## Versioning

The ABI is part of the cartridge format contract: carts embed the
format version (see CART_FORMAT.md), and ABI additions bump it. Within
version 1, this exact import set is stable.
