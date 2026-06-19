# RICO-8 on handhelds

`rico8-player` is the third frontend over the console runtime: a pure-Rust
KMS/evdev/ALSA cart player sized for retro handhelds — PowKiddy RGB10S, the
Anbernic RG351/353 family, and anything else aarch64 running ArkOS, ROCKNIX or
a similar ports-friendly firmware. The same binary also runs on a desktop TTY,
where it opens full-screen via KMS (no window manager required).

```text
rico8-player cart.png      play one cart
rico8-player /path/to/dir  cart picker over a directory
rico8-player               picker over the current directory
```

For windowed cart testing on a desktop, use `cargo console` (its Run mode)
instead.

## Controls

| input                     | action                                           |
| --------------------------| -------------------------------------------------|
| d-pad                     | directions / move in picker                      |
| any face button           | O / X in game, launch in picker                  |
| **hold both O + X (~1s)** | **return to the cart picker** (works on any pad) |
| Select                    | back to picker (named pads)                      |
| Start + Select            | quit (named pads)                                |
| keyboard                  | arrows + Z/X, Esc = back, F1 = fps meter         |

The picker shows the key controls along its bottom edge. Input is read
directly from the kernel via evdev; named buttons like Select/Start only work
when the driver reports them by those names. Because that varies across
devices, the **hold-O+X** combo is the universal, always-available way back to
the picker. For unrecognized pads you can bind raw evdev button indices via
`RICO8_SELECT` / `RICO8_START`.

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
log at `rico8/log.txt` (next to the binary) says why.

You boot into the cart shelf: a console-style list of every cart on
the card. Pick one with A; Select brings you back; runtime errors show
the same friendly RICO-8 error screen as everywhere else.

## Building the bundle

On any Linux x86_64 machine with Rust:

```text
rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf
./.github/build-handheld.sh                   # writes dist/handheld/
```

That is all — no cross-compiler, no extra packages. The script produces fully
static musl binaries for both armhf and aarch64, linked with rust-lld. There
is no dynamic runtime dependency — the binaries are fully self-contained, so
version-mismatch failures at load time cannot occur.

## Notes and limits

- **Play only** — like the web export, this is a player, not the
  console. Make carts on a PC, copy the `.png`s over.
- The screen is letterboxed to square (640x480 -> 480x480 on the
  RGB10S) with nearest-neighbor scaling. Set `RICO8_ROTATE=90`,
  `180`, or `270` to rotate the output, and `RICO8_DRM_CARD` to
  override the default `/dev/dri/card0`.
- Cart-rate logic (30 or 60 fps) with its own timer; vsync is used when
  the device renderer provides it.
- Audio is the same 4-channel synth at 44.1 kHz written straight to
  ALSA via raw ioctls (falls back to 48 kHz if the device requires it).
  Set `RICO8_NOAUDIO=1` to disable audio entirely.
- On a desktop TTY the player renders full-screen via KMS; `/dev/dri`
  and `/dev/input` access is required (root or appropriate group).
- Tested in CI headless and cross-built automatically; on-device testing
  reports are very welcome.
