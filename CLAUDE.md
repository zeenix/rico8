# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What RICO-8 is

A PICO-8-like fantasy console where games ("carts") are written in **Rust**, compiled
to `wasm32-unknown-unknown`, and run sandboxed at 60 fps (or 30) inside the console.
The constraints are the product: 128x128 screen, 16 fixed colors, 256 8x8 sprites,
128x64 tile map, 64 SFX / 64 music patterns, 4 audio channels. Carts are shareable
PNG images with the wasm + assets (and optionally source) embedded.

## Build, test, lint

A one-time setup is required because cart-build tests cross-compile to wasm and the
console links ALSA:

```sh
rustup target add wasm32-unknown-unknown
sudo apt install libasound2-dev   # debian/ubuntu (or alsa-lib-devel on fedora)
```

```sh
# Boot the console (alias in .cargo/config.toml for `run --release -p rico8-console`)
cargo console
cargo console -- examples/platformer    # boot with a project loaded, then type `run`

# Tests
cargo test --workspace
cargo test -p rico8-runtime audio::   # single module/test

# Format — .rustfmt.toml uses nightly-only options, so fmt MUST run on nightly
cargo +nightly fmt --all

# Lint — CI treats warnings as errors
cargo clippy --workspace --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs three jobs that must stay green: `fmt` (nightly),
`clippy` (`-D warnings`), and `test` (workspace). Match them locally before pushing.

## Workspace layout

The workspace excludes `examples/` (those are standalone wasm crates). Five members:

- **`rico8/`** — the SDK carts depend on, *deliberately zero-dependency*. `ffi.rs`
  declares the raw ABI imports (stubbed on non-wasm so carts type-check natively);
  `lib.rs` wraps them in zero-sized `Context` (update-time) and `Graphics` (draw-time);
  the `game!` macro exports `rico8_init/update/draw` and installs the panic-forwarding
  hook. Defaults to `std`; disabling the `std` feature makes it `#![no_std]` for
  allocation-free carts with `heapless`.
- **`rico8-runtime/`** — the heart. Modules: `fb` (128x128 indexed framebuffer),
  `font`, `palette`, `vm` (wasmi + ABI linking + fuel metering), `input`, `audio`
  (4-ch synth + cpal layer behind the `audio` feature), `assets`, `project`, `cart`
  (PNG codec), `pico8` (importer), `ui`.
- **`rico8-console/`** — the desktop frontend (winit + wgpu). **The binary it builds is
  named `rico8`, not `rico8-console`.** Contains `shell.rs` (the mode machine),
  `main.rs` (event loop + headless subcommand dispatch), `gpu.rs`, `builder.rs`,
  `webexport.rs`, and `editor/` (the five editors: `code`, `sprite`, `map`, `sfx`,
  `music`).
- **`rico8-web/`** — the browser player: `rico8-runtime` compiled to wasm and wrapped
  in a C-like export surface. `cdylib` + `rlib` (rlib so player logic is host-testable).
- **`rico8-player/`** — pure-Rust cart player with two cargo-feature backends:
  **`window`** (winit + softbuffer, the desktop default — opens a window with keyboard
  input) and **`kms`** (static-musl KMS/evdev/ALSA, built with
  `--no-default-features --features kms`, for handhelds and bare TTYs).

## Architecture essentials

Read `docs/ARCHITECTURE.md` for the full picture. Key invariants worth knowing before
touching code:

- **One screen, one rasterizer.** Everything visible — running carts, the boot console,
  every editor — is software-rendered into a single `Framebuffer` of palette indices.
  The GPU only uploads that as a texture and integer-scales/letterboxes it. There are no
  native widgets anywhere. A consequence: the whole console is **testable headless** —
  tests and the `verify`/`snap` subcommands drive the same framebuffer with no window.
- **The sandbox is allowlist-only.** `GameVm::load` links exactly the `"rico8"` import
  set (~26 C-like functions, documented in `docs/ABI.md`); unknown imports fail
  instantiation. No WASI, no filesystem, no network. Fuel metering caps each
  `update`/`draw` so infinite loops become an error screen, not a freeze. The VM holds a
  *copy* of sprite/map assets so runtime `mset` writes are RAM-only, like a real cart.
- **Assets are one shared data model.** `rico8-runtime/src/assets.rs` defines the serde
  models used by editors (mutate), the VM (draw/play), and the cart codec (embed). Sizes
  are fixed by design. On disk inside a project: one postcard-encoded, version-headered
  `assets.rico8`; inside a cart: the `rcRt` PNG chunk (`docs/CART_FORMAT.md`).
- **The shell is a mode machine** (`Console`, `Run`, five editors). Loaded state is
  either a *project* (a real Cargo crate: full build/run/export) or a *cart* (PNG run
  as-is). `run` spawns `cargo build --release --target wasm32-unknown-unknown` on a
  thread, streams trimmed errors to the console, and hot-reloads when the wasm mtime
  changes (polled once a second).
- **Projects are real crates.** `rico8 new` scaffolds an ordinary cargo crate building a
  `cdylib` for wasm + an `assets.rico8`. `$EDITOR` + `cargo build` works identically to
  the integrated editor.

## Headless / CLI surface

Every pipeline stage also exists as a subcommand of the `rico8` binary (dispatched in
`rico8-console/src/main.rs`), which is how CI keeps examples runnable:
`new`, `build`, `export`, `extract`, `import-pico8`, `export-web`, `verify`, `snap`.

## Conventions

- **Commit messages** are prefixed with a single verbatim [gitmoji](https://gitmoji.dev/)
  emoji (including any U+FE0F variation selector, e.g. `♻️`), then one space, then a
  sentence: `🐛 Stop the synth clipping when two channels peak together`. Use the
  `gimoji` tool (`gimoji --init`) rather than typing emoji from memory. See
  `CONTRIBUTING.md`.
- **Atomic commits.** When addressing review feedback, fix existing commits and
  force-push rather than piling on follow-up commits.
- Audio is feature-gated (`audio`, on by default). Code must still build and run
  (silently) with `--no-default-features` on the console/runtime for machines without
  ALSA.

## Docs index

`docs/ABI.md` (wasm import surface), `docs/ARCHITECTURE.md`, `docs/CART_FORMAT.md`,
`docs/CLIPBOARD_FORMAT.md` (native `[rico8]` clipboard wire format + PICO-8 interop),
`docs/LIMITS.md` + `docs/LIMITS_TESTING.md`, `docs/PICO8_IMPORT.md`,
`docs/WEB_EXPORT.md`, `docs/HANDHELD.md`. Design plans/specs live under
`docs/superpowers/`.
