# RICO-8 architecture

```text
            +------------------------------------------------------+
            |                    rico8-cli                          |
            |  winit window . wgpu blit . shell . prompt . editors  |
            +-----------+------------------------------+-----------+
                        |                              |
            draws into  v                              v  runs carts
            +------------------------------------------------------+
            |                  rico8-runtime                        |
            |  Framebuffer (128x128 indexed) . font . palette       |
            |  GameVm (wasmi + ABI) . InputState . Synth (4ch)      |
            |  Assets . Project . PNG cart codec                    |
            +-----------+------------------------------------------+
                        ^
              ABI calls |  (module "rico8", ~26 C-like imports)
            +-----------+------------------------------------------+
            |  game cart: wasm32-unknown-unknown cdylib             |
            |  built from user Rust code against the rico8 SDK      |
            +------------------------------------------------------+
```

## One screen, one rasterizer

Everything visible — running carts, the boot console, every editor —
is software-rendered into a single 128x128 buffer of palette indices
(`rico8_runtime::fb::Framebuffer`). The GPU's only job (`rico8-cli/src/
gpu.rs`) is to upload that as a texture and blit it at the largest
integer scale that fits the window, letterboxed, nearest-filtered.
That is what makes the UI incapable of looking native: there is no
other way to put pixels on screen.

This also makes the whole console testable headless: tests and the
`verify`/`snap` subcommands drive the same framebuffer with no window.

## The SDK (`crates/rico8`)

Carts depend on one crate. `ffi.rs` declares the raw ABI imports
(stubbed on non-wasm targets so carts also type-check natively);
`lib.rs` wraps them in `Context` (update-time: input, map, audio,
logging) and `Graphics` (draw-time), both zero-sized. The `game!`
macro exports `rico8_init/update/draw` and installs a panic hook that
forwards panic messages to the host before the trap, which is how a
cart panic becomes a readable error screen.

## The VM (`rico8-runtime/src/vm.rs`)

`GameVm::load` compiles the module with wasmi, links exactly the
`"rico8"` import set, and runs `rico8_init`. The host state owns the
cart's framebuffer, input state, and a *copy* of the sprite/map assets
(so runtime `mset` writes are RAM-only, like a real cartridge). Fuel
metering is on: every `update`/`draw` call gets a fixed budget, and
exhaustion is reported as "ran too long (infinite loop?)" instead of a
freeze. Unknown imports fail instantiation — the sandbox is allowlist-
only.

## The shell (`rico8-cli/src/shell.rs`)

A mode machine: `Console`, `Run`, and the five editors. The console
owns the loaded state, which is either a *project* (directory: code +
assets, full build/run/export workflow) or a *cart* (PNG loaded
directly: runs as-is). `run` saves the project, spawns
`cargo build --release --target wasm32-unknown-unknown` on a thread,
streams trimmed errors to the console on failure, and boots the VM on
success. While running, the wasm file's mtime is polled once a second;
external rebuilds hot-reload the cart. The fixed 30 fps tick lives in
`main.rs` (`ControlFlow::WaitUntil` + an accumulator).

## Assets (`rico8-runtime/src/assets.rs`)

One set of serde data models shared by the editors (which mutate
them), the VM (which draws/plays from them) and the cart codec (which
embeds them). Fixed sizes everywhere — 256 sprites, 128x64 map, 64
SFX, 64 patterns — because the constraints are the product. On disk
inside a project they are one postcard-encoded, version-headered
`assets.rico8` file; inside a cart they ride in the `rcRt` chunk
(see CART_FORMAT.md).

## Audio (`rico8-runtime/src/audio.rs`)

A pure 4-channel synthesizer (8 classic waveforms, per-step effects,
an SFX-chaining music sequencer) that renders one sample at a time,
plus a thin cpal output layer behind the `audio` feature. The synth is
fully testable without a device; on headless machines the console runs
silent but otherwise identical. The VM and the editors talk to it
through a shared `AudioHandle`.

## The pipeline, end to end

```text
new      ->  Cargo crate (cdylib) + template + empty assets.rico8
edit     ->  code editor writes src/lib.rs; asset editors write assets.rico8
run      ->  cargo build --target wasm32-unknown-unknown
         ->  GameVm::load(wasm, assets)  ->  30 fps update/draw
export   ->  Cart { wasm, assets, source? }  ->  postcard -> deflate
         ->  rcRt chunk inside the label PNG
load     ->  validate + decode  ->  run, or extract back to a project
```

Every stage of that pipeline also exists as a headless subcommand
(`new`, `build`, `export`, `extract`, `verify`), which is how CI keeps
the examples runnable.
