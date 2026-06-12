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

| input            | action                          |
| ---------------- | ------------------------------- |
| d-pad            | directions                      |
| A (or Y)         | O button / pick cart            |
| B (or X)         | X button / quit picker          |
| Select           | back to the cart picker         |
| Start + Select   | quit                            |
| keyboard         | arrows + Z/X, Esc = back        |

Input uses SDL's GameController API, which the handheld firmwares
preconfigure for their built-in pads. For exotic pads, drop a
[SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB)
format `gamecontrollerdb.txt` next to the binary (or point `RICO8_GCDB`
at one); unmapped sticks also get a raw hat/button fallback.

## Installing on the device (ArkOS / ROCKNIX style)

1. Build the bundle (see below) or grab the `rico8-handheld` artifact
   from CI. It contains:

   ```text
   RICO-8.sh          launcher
   rico8/
     rico8-player     aarch64 binary
     carts/           put your .png carts here
   ```

2. Copy `RICO-8.sh` and the `rico8/` folder into the ports directory
   on the SD card (`/roms/ports` on ArkOS; ROCKNIX picks ports up from
   `/roms/ports` too).
3. Drop cart `.png`s into `rico8/carts/`.
4. Refresh the games list (or reboot); launch **RICO-8** from Ports.

You boot into the cart shelf: a console-style list of every cart on
the card. Pick one with A; Select brings you back; runtime errors show
the same friendly RICO-8 error screen as everywhere else.

## Building the bundle

On any Linux x86_64 machine with Rust:

```text
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu zstd   # cross toolchain
./.github/build-handheld.sh                   # writes dist/handheld/
```

The script downloads an aarch64 `libSDL2.so` (from Debian) purely to
link against — at runtime the player uses the SDL2 the firmware ships,
which is what knows about the device's KMS/DRM display, gamepad and
audio path. The binary's only dynamic dependencies are `libSDL2-2.0`,
libc and libm.

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
