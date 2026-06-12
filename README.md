# RICO-8

**A PICO-8-like fantasy console where the games are written in Rust.**

RICO-8 is a tiny, self-contained game console that never existed: a
128x128 screen, 16 fixed colors, a 4x6 pixel font, four audio channels,
256 sprites, a 128x64 tile map — and a Rust compiler where the Lua
interpreter would be. You write a little Rust, it compiles to
WebAssembly, and it runs inside the console's sandbox at a steady
30 fps. Carts are shareable PNG images with the game embedded inside.

The programming language is the point: everything else tries to stay as
close to the feel of [PICO-8](https://www.lexaloffle.com/pico-8.php) as
is legal and practical. The palette, the constraints, the editor modes,
the `>` prompt, the charm — those are loving homage. The font, code and
formats are original.

```rust
use rico8::*;

struct Game {
    x: i32,
    y: i32,
}

impl Rico8Game for Game {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.btn(Button::Right) {
            self.x += 1;
        }
    }

    fn draw(&self, gfx: &mut Graphics) {
        gfx.clear(Color::BLACK);
        gfx.rect_fill(self.x, self.y, 8, 8, Color::WHITE);
    }
}

rico8::game!(Game { x: 64, y: 64 });
```

## The console

```text
cargo run --release -p rico8-cli
```

You land at the boot console. Type `help`. The workflow is PICO-8's:

```text
> new mygame          create ./mygame (a real cargo crate!)
> run                 compile to wasm + run     (esc returns)
> save                save code + assets
> export mygame.png   write a shareable png cartridge
> load mygame.png     load a cart back
```

`Esc` flips between the console and the editors; the tab icons (or
`Alt+←/→`) switch between **code**, **sprite**, **map**, **sfx** and
**music** editors. All UI is drawn by the console itself on the same
128x128 screen the games use — there are no native widgets anywhere.

Games are played with the arrow keys plus `Z`/`X` (also `C`/`V`,
`N`/`M`). `Ctrl+R` rebuilds and runs from anywhere. `F6` while a game
runs captures the screen as the cartridge label. Type `keys` in the
console for the full list.

### Constraints (they are the point)

| thing      | size                                  |
| ---------- | ------------------------------------- |
| screen     | 128 x 128, 16 fixed colors            |
| sprites    | 256 of 8x8 pixels, 8 flags each       |
| map        | 128 x 64 tiles                        |
| sfx        | 64 slots, 32 steps, 8 waveforms       |
| music      | 64 patterns, 4 channels               |
| framerate  | 30 fps, fixed                         |
| cart       | one PNG file                          |

## PNG cartridges

`export` produces a real PNG image — cartridge art, label, title — with
the compiled wasm, all assets and (by default) the compressed Rust
source embedded in a private chunk. Anyone can *see* the cart; RICO-8
can *play* it; and if the source is included, `import` turns it back
into an editable project. See [docs/CART_FORMAT.md](docs/CART_FORMAT.md).

## Projects are real crates

A RICO-8 project is an ordinary Cargo crate that builds a `cdylib` for
`wasm32-unknown-unknown`, plus an `assets.rico8` bundle. The integrated
editor is the charming way to work, but `$EDITOR` + `cargo build` works
exactly the same — the console hot-reloads the wasm when it changes on
disk. Headless commands support scripts and CI:

```text
rico8 new <dir>                  create a project
rico8 build <dir>                compile it to wasm
rico8 export <dir> <out.png>     build + write a png cart
rico8 extract <cart.png> <dir>   editable cart -> project
rico8 verify <cart.png>          run 60 frames headless
```

## The sandbox

Carts execute inside [wasmi](https://github.com/wasmi-labs/wasmi) with
no WASI, no filesystem, no network and no host memory access. The only
imports a cart gets are the ~26 small, C-like functions of the RICO-8
ABI (`docs/ABI.md`) — draw, input, audio, map, log. Fuel metering turns
infinite loops into a friendly error screen instead of a hung console.

## Workspace

```text
crates/
  rico8/          the SDK carts depend on (safe wrappers over the ABI)
  rico8-runtime/  framebuffer, font, palette, VM, synth, assets, carts
  rico8-cli/      the console: winit + wgpu shell, editors, prompt
examples/
  hello/          the canonical first cart
  sprite_move/    sprite drawing, flipping, animation
  platformer/     map collision via sprite flags, coins, sfx
  map_demo/       map scrolling and layer masks
  sfx_demo/       a soundboard
  music_demo/     starting/stopping a song
docs/
  ABI.md          the wasm import surface, function by function
  ARCHITECTURE.md how the pieces fit
  CART_FORMAT.md  the PNG cartridge format
```

## Building

Requires Rust (with the `wasm32-unknown-unknown` target for building
carts) and, on Linux, ALSA headers for audio:

```text
rustup target add wasm32-unknown-unknown
sudo apt install libasound2-dev        # debian/ubuntu
sudo dnf install alsa-lib-devel        # fedora
# (or skip audio entirely with --no-default-features)
cargo run --release -p rico8-cli
```

Try a bundled cart:

```text
cargo run --release -p rico8-cli -- examples/platformer
```

then type `run`.

## Status

Stages 0-9 of the original plan are in place: console, VM + ABI,
all five editors, audio runtime, PNG carts, examples, docs and tests.
Web export (stage 10) is future work. Expect rough edges and enjoy
them — it's a fantasy console, not an IDE.
