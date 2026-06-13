# RICO-8 on handhelds

`rico8-player` is the third frontend over the console runtime: an SDL2
cart player sized for retro handhelds — PowKiddy RGB10S, the Anbernic
RG351/353 family, and anything else aarch64 running ArkOS, ROCKNIX or
a similar ports-friendly firmware. The same binary also runs on a
desktop with SDL2, where it doubles as a minimal cart player.

```text
rico8-player cart.png      play one cart
rico8-player /path/to/dir  cart picker over a directory
rico8-player               picker over the current directory
```

## Controls

| input                  | action                           |
| ---------------------- | -------------------------------- |
| d-pad                  | directions / move in picker      |
| any face button        | O / X in game, launch in picker  |
| **hold both O + X (~1s)** | **return to the cart picker** (works on any pad) |
| Select                 | back to picker (recognized pads) |
| Start + Select         | quit (recognized pads)           |
| keyboard               | arrows + Z/X, Esc = back         |

The picker shows the key controls along its bottom edge. Input prefers
SDL's GameController API, which most firmwares preconfigure for their
built-in pads; named buttons like Select/Start only work when the pad
is recognized that way. Because that varies wildly across devices, the
**hold-O+X** combo is the universal, always-available way back to the
picker. For unrecognized pads you can bind raw button indices for
Select/Start via `RICO8_SELECT` / `RICO8_START`, and dropping a
[SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB)
`gamecontrollerdb.txt` next to the binary (or `RICO8_GCDB`) can promote
a pad to a recognized GameController.

## Installing on the device (ArkOS / ROCKNIX style)

1. Build the bundle (see below) or grab the `rico8-handheld` artifact
   from CI. It contains:

   ```text
   RICO-8.sh                 launcher (picks the right binary)
   rico8/
     rico8-player.armhf      32-bit ARM build
     rico8-player.aarch64    64-bit ARM build
     carts/                  put your .png carts here
   ```

   These chips are 64-bit, but many firmwares (ArkOS on the RGB10S)
   run a **32-bit armhf userland**, while others (some ROCKNIX builds)
   are aarch64. The bundle ships both; `RICO-8.sh` picks by the ELF class of
   `/bin/sh` (the userland's own arch) rather than `uname -m` or the
   kernel's loaders, which mislead on a 64-bit kernel running a 32-bit
   rootfs. Force it with `RICO8_ARCH=armhf` or `aarch64`. (Give a 32-bit
   device an aarch64 binary and its `binfmt_misc` routes it through
   `qemu-aarch64-static`, which then fails to find an aarch64 loader —
   that's the tell.)

2. Copy `RICO-8.sh` and the `rico8/` folder into the ports directory
   on the SD card. Reading the card on a PC, this is the `ports` folder
   at the root of the games partition — **`EASYROMS`** on ArkOS (it
   mounts as `/roms`, so its `ports` folder is `/roms/ports` in-game),
   or `/roms/ports` on ROCKNIX. (The empty `roms` folder on the Linux
   system/root partition is just a mount point — not the place.)
3. Drop cart `.png`s into `rico8/carts/`.
4. Refresh the games list (or reboot); launch **RICO-8** from Ports.

The launcher restores the binary's executable bit on each run, since
FAT/exFAT game partitions don't preserve it. If a launch fails, the
log at `rico8/log.txt` (next to the binary) says why — a glibc version
mismatch there means the device's runtime is older than the build
host's, so rebuild on / against an older system.

You boot into the cart shelf: a console-style list of every cart on
the card. Pick one with A; Select brings you back; runtime errors show
the same friendly RICO-8 error screen as everywhere else.

## Building the bundle

On any Linux x86_64 machine with Rust:

```text
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
sudo apt install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf zstd
cargo install cargo-zigbuild                  # (script auto-fetches zig)
./.github/build-handheld.sh                   # writes dist/handheld/
```

It builds both the armhf and aarch64 binaries.

The script downloads an aarch64 `libSDL2.so` (from Debian) purely to
link against — at runtime the player uses the SDL2 the firmware ships,
which is what knows about the device's KMS/DRM display, gamepad and
audio path. The binary's only dynamic dependencies are `libSDL2-2.0`,
libc, libm, libpthread and libdl.

It builds with [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild)
against an **old glibc baseline** (`RICO8_GLIBC`, default 2.28) because
these firmwares ship glibc ~2.31, while a stock cross toolchain emits
`GLIBC_2.34+` references that fail to load with
`version 'GLIBC_2.34' not found`. musl is not an option here: the
player dynamically loads the device's glibc-linked `libSDL2.so`, which
a musl-static process can't host. If a device turns out older still,
lower the baseline, e.g. `RICO8_GLIBC=2.27 ./.github/build-handheld.sh`.

A desktop build is just `cargo build --release -p rico8-player`
(needs `libsdl2-dev` / `SDL2-devel`).

## Notes and limits

- **Play only** — like the web export, this is a player, not the
  console. Make carts on a PC, copy the `.png`s over.
- The screen is letterboxed to square (640x480 -> 480x480 on the
  RGB10S) with nearest-neighbor scaling.
- 30 fps logic with its own timer; vsync is used when the device
  renderer provides it.
- Audio is the same 4-channel synth at 44.1 kHz through SDL/ALSA.
- Tested in CI headless (SDL dummy drivers) and cross-built
  automatically; on-device testing reports are very welcome.
