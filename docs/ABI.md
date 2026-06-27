# The RICO-8 WASM ABI

This is the complete surface between a cart and the console. Carts are
`wasm32-unknown-unknown` modules; everything below lives in the import
module **`"rico8"`**. There is intentionally nothing else: no WASI, no
filesystem, no network, no host memory access. Game code should use the
`rico8` SDK crate, which wraps all of this safely — this document is for
people working on the console itself, alternate SDKs, or curiosity.

**At this raw ABI, screen-space positions and sizes cross as `i32` pixels; everything discrete
is an integer.** WebAssembly function signatures only have `i32`/`i64`/`f32`/`f64`, so every
coordinate (`x`/`y`), extent (`w`/`h`), the camera, and radius is widened to `i32` here; the
SDK's cart-facing API exposes positions as `i16` and sizes as `u16` and widens them at this
boundary. `sprite`'s `w`/`h` are pixel counts (`8` = one 8×8 cell). Indices and counts —
sprite/tile numbers, map cel coordinates, flags, buttons, channels, colors — stay `i32`/`u32`.
Colors are palette indices `0..16` (masked with `0x0f`). Genuinely fractional returns — `time`,
`rnd`, `cpu_update`, `cpu_draw`, `fps` — stay `f32`.

The SDK validates all size arguments to non-zero before calling the ABI; size-bearing draw
methods return `Result<(), ZeroSize>` at the SDK level. Carts built against the old `f32`
coordinate ABI fail wasm instantiation against this host by design (import-signature mismatch).

Out-of-range coordinates are safe everywhere: draws clip, reads return 0.

A sprite moving diagonally at less than a pixel per frame can shimmer: when
`x` and `y` advance as sub-pixel floats and are independently converted to `i16`
before each call, the two axes tick on different frames — a zigzag rather than
a clean staircase. Holding two buttons for a 45° heading triggers it whenever
`x` and `y` start on different fractions. This is integer-grid geometry
(PICO-8 has it too). Carts that want a clean staircase use the SDK's opt-in
[`Body`] mover, which owns the trajectory and steps both axes together; the
exact sub-pixel position stays available for collision.

[`Body`]: ../rico8/src/motion.rs

(The ABI boundary itself can't be narrower: WebAssembly function signatures only have
`i32`/`i64`/`f32`/`f64` — there is no `i8`/`u8`/`i16`. The SDK still narrows its own API to
`i16` positions and `u16` sizes, which is far wider than the 128×128 screen needs, so sprites
can still be drawn off-screen and scrolled in by the camera.)

## Guest exports (required)

| export         | signature | called                                  |
| -------------- | --------- | --------------------------------------- |
| `rico8_init`   | `() -> ()`| once, after the module is instantiated  |
| `rico8_update` | `() -> ()`| `rico8_fps` times per second            |
| `rico8_draw`   | `() -> ()`| after each update                       |
| `memory`       | memory    | read by `print`/`log`/`panic`           |

Each call runs under a fuel budget (~128K instructions; see LIMITS.md).
Exhausting it traps with a "ran too long" error screen — infinite loops
cannot hang the console.

## Guest exports (optional)

| export             | signature   | called                                       |
| ------------------ | ----------- | -------------------------------------------- |
| `rico8_fps`        | `() -> u32` | once, after `rico8_init`                     |
| `rico8_mem_used`   | `() -> u32` | each frame, for the stats overlay            |

`rico8_fps` reports the cart's logical frame rate. The SDK emits it from
every cart; `30` and `60` are honored, and `60` is the default. A missing
export, or any other value, also means 60, so a hand-written cart that
omits it still runs.

`rico8_mem_used` reports the cart's committed-memory high-water in bytes — the
highest its footprint (shadow-stack reserve + statics + heap) has ever reached.
The host reads it for the F2 stats overlay. It never decreases (wasm never
returns pages) and counts freed-but-stranded memory, so it tracks real pressure
closely; it is still not an exact OOM line, since the allocator keeps a small
reserve above the last allocation. Carts without the export (hand-written WAT
or allocation-free) report 0.

## Host imports

### Drawing

| function        | signature                                            | notes                                                                     |
| --------------- | ---------------------------------------------------- | ------------------------------------------------------------------------- |
| `clear`         | `(color: i32)`                                       | fill the screen                                                           |
| `camera`        | `(x: i32, y: i32)`                                   | offset subsequent draws by `(-x, -y)`                                     |
| `clip`          | `(x, y, w, h: i32)`                                  | restrict drawing; `clip(0,0,128,128)` resets                              |
| `set_pixel`     | `(x, y: i32, color: i32)`                            | set one pixel                                                             |
| `pixel`         | `(x, y: i32) -> i32`                                 | read one pixel (screen space)                                             |
| `line`          | `(x0, y0, x1, y1: i32, color: i32)`                  | inclusive endpoints                                                       |
| `rect`          | `(x0, y0, x1, y1: i32, color: i32)`                  | outline, inclusive corners                                                |
| `rect_fill`     | `(x0, y0, x1, y1: i32, color: i32)`                  | filled                                                                    |
| `circle`        | `(x, y: i32, r: i32, color: i32)`                    | outline; `r = 0` draws a single pixel                                     |
| `circle_fill`   | `(x, y: i32, r: i32, color: i32)`                    | filled; `r = 0` draws a single pixel                                      |
| `print`         | `(ptr: u32, len: u32, x, y: i32, color: i32) -> i32` | UTF-8 text from guest memory; returns the x position after the last glyph |
| `ellipse`       | `(x0, y0, x1, y1: i32, color: i32)`                  | ellipse outline, inclusive corners                                        |
| `ellipse_fill`  | `(x0, y0, x1, y1: i32, color: i32)`                  | filled ellipse                                                            |
| `set_pen_color` | `(color: i32)`                                       | persistent default pen color for `print_pen`                              |
| `set_cursor`    | `(x, y: i32)`                                        | persistent text cursor for `print_pen`                                    |
| `print_pen`     | `(ptr: u32, len: u32) -> i32`                        | print at cursor in pen color; returns x after text                        |

### Palette, transparency, and fill patterns

These set persistent draw state that lives for the cart's lifetime (like the
camera), so set them in `rico8_init` or each frame as needed.

| function                | signature                                          | notes                                                                                                                          |
| ----------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `set_transparent_color` | `(color: i32, transparent: i32)`                   | mark a color transparent (`1`) or opaque (`0`) for sprite draws; default: only color 0 transparent                             |
| `reset_transparency`    | `()`                                               | back to the default (only color 0 transparent)                                                                                 |
| `remap_color`           | `(from: i32, to: i32, mode: i32)`                  | remap a color; `mode` 0 = draw palette (affects later draws), 1 = display palette (applied to the whole screen at present)     |
| `reset_palette`         | `()`                                               | reset both the draw and display palettes to identity                                                                           |
| `set_fill_pattern`      | `(pattern: i32, secondary: i32, transparent: i32)` | 4x4 stipple (bit 15 = top-left); pattern-1 pixels take `secondary`, or are skipped when `transparent`; `pattern` 0 fills solid |

### Sprites and map

| function           | signature                                                          | notes                                                                                                               |
| ------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| `sprite`           | `(n: u32, x, y: i32, w, h: i32, flip_x, flip_y: i32)`              | draw a `w×h`-pixel sprite block; `w`/`h` are pixel counts (`8` = one 8×8 cell); color 0 is transparent              |
| `sprite_stretch`   | `(sx, sy, sw, sh: i32, dx, dy, dw, dh: i32, flip_x, flip_y: i32)`  | draw a sheet rectangle stretched to a screen rectangle (nearest-neighbor); honors transparency and the draw palette |
| `sprite_pixel`     | `(x, y: i32) -> i32`                                               | read a sprite-sheet pixel                                                                                           |
| `set_sprite_pixel` | `(x, y: i32, color: i32)`                                          | write a sprite-sheet pixel (RAM only)                                                                               |
| `map`              | `(cel_x, cel_y: i32, sx, sy: i32, cel_w, cel_h: i32, layers: u32)` | draw map tiles; nonzero `layers` draws only tiles whose sprite flags intersect the mask                             |
| `map_tile`         | `(x, y: i32) -> i32`                                               | read a map tile                                                                                                     |
| `set_map_tile`     | `(x, y: i32, v: u32)`                                              | write a map tile (RAM only; discarded on reload)                                                                    |
| `sprite_flags`     | `(n: u32) -> i32`                                                  | sprite flag bitmask                                                                                                 |
| `set_sprite_flags` | `(n: u32, flags: u32)`                                             | overwrite sprite flags                                                                                              |

### Input

| function            | signature         | notes                                                                  |
| ------------------- | ----------------- | ---------------------------------------------------------------------- |
| `is_button_down`    | `(b: u32) -> i32` | held? buttons: 0 left, 1 right, 2 up, 3 down, 4 O, 5 X                 |
| `is_button_pressed` | `(b: u32) -> i32` | just pressed? repeats after 15 frames, then every 4                    |
| `buttons_down`      | `() -> u32`       | held buttons as a bitmask, bit i = button i                            |
| `buttons_pressed`   | `() -> u32`       | just-pressed buttons as a bitmask (same repeat as `is_button_pressed`) |

### Audio

| function | signature                | notes                                                                     |
| -------- | ------------------------ | ------------------------------------------------------------------------- |
| `sfx`    | `(n: i32, channel: i32)` | play SFX `n`; `channel < 0` picks a free channel; `n < 0` stops `channel` |
| `music`  | `(n: i32, fade_duration: i32, channel_mask: i32, token: i32) -> i32` | start pattern `n` (`n<0` = stop); `fade_duration` ms fade in (start) / out (stop), 0 = instant; `channel_mask` reserves channels (bits 0..3) for music on start; start returns a nonzero play-token or 0 if a song is already playing; stop's `token` selects the song (≤0 = unconditional) |

### Misc

| function   | signature              | notes                                                                                                                    |
| ---------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `time`     | `() -> f32`            | seconds since init, in `1/fps` steps (see `rico8_fps`)                                                                   |
| `rnd`      | `() -> f32`            | uniform in `[0, 1)` (host RNG)                                                                                           |
| `seed_rng` | `(seed: u32)`          | reseed the `rnd` sequence for deterministic runs                                                                         |
| `log`      | `(ptr: u32, len: u32)` | line to the RICO-8 console                                                                                               |
| `panic`    | `(ptr: u32, len: u32)` | record a panic message; the SDK's panic hook calls this right before the trap so the error screen shows the real message |

### Resources

Read-only meters a cart can watch. CPU is reported for the *last completed frame* (the
current call isn't finished yet); fps is live. The CPU budget is 128K instructions per
call (see LIMITS.md). Memory usage is cart-reported via the `rico8_mem_used` guest
export rather than a host import; see the Guest exports (optional) section above.

| function     | signature   | notes                                                              |
| ------------ | ----------- | ------------------------------------------------------------------ |
| `cpu_update` | `() -> f32` | fraction `0..1` of `update`'s fuel budget used last frame          |
| `cpu_draw`   | `() -> f32` | fraction `0..1` of `draw`'s fuel budget used last frame            |
| `fps`        | `() -> f32` | measured frames per second (target rate until a frontend measures) |

## Versioning

The ABI is part of the cartridge format contract: carts embed the format
version (see CART_FORMAT.md). Version 1's import set grows by addition — a
newer import like `buttons_down` is part of the version-1 surface — and a cart
only needs the imports it actually uses.
