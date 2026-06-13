#!/bin/bash
# RICO-8 cart player launcher for handhelds (ArkOS / ROCKNIX / etc.),
# in the usual Ports layout:
#
#   /roms/ports/RICO-8.sh        <- this script
#   /roms/ports/rico8/           <- rico8-player binary
#   /roms/ports/rico8/carts/     <- put your .png carts here
#
# Controls: d-pad moves, A = O, B = X, Select = back to the cart
# picker, Start+Select = quit.

DIR="$(cd "$(dirname "$0")" && pwd)/rico8"
cd "$DIR" || exit 1

mkdir -p "$DIR/carts"

# These chips are 64-bit but many firmwares (ArkOS on the RGB10S) run a
# 32-bit armhf userland on a 64-bit kernel, while others are fully
# aarch64. `uname -m` and the presence of an aarch64 loader both report
# the *kernel*, not the userland, so they're unreliable (a 64-bit
# kernel with a 32-bit rootfs routes aarch64 ELFs through qemu, which
# then fails). Instead match the architecture of an actual userland
# binary: our player shares libc with /bin/sh, so use its ELF class
# (byte 4: 1 = 32-bit, 2 = 64-bit). Override with RICO8_ARCH=armhf|aarch64.
detect_arch() {
  if command -v od >/dev/null 2>&1; then
    od -An -t u1 -j4 -N1 /bin/sh 2>/dev/null | tr -d ' \n'
  elif command -v hexdump >/dev/null 2>&1; then
    hexdump -s4 -n1 -e '1/1 "%d"' /bin/sh 2>/dev/null
  fi
}
case "${RICO8_ARCH:-$([ "$(detect_arch)" = 2 ] && echo aarch64 || echo armhf)}" in
  aarch64) PLAYER="$DIR/rico8-player.aarch64" ;;
  *)       PLAYER="$DIR/rico8-player.armhf" ;;
esac

# Ports live on a FAT/exFAT partition that doesn't keep the Unix
# executable bit, so restore it before launching (ignored if the bit
# is already set, e.g. on ext4).
chmod +x "$DIR/"rico8-player.* 2>/dev/null || true

# Extra controller mappings can be dropped next to the binary.
export RICO8_GCDB="$DIR/gamecontrollerdb.txt"

# These handhelds have no PulseAudio session, so SDL's default pulse
# probe just spews "failed to create secure directory" before falling
# back. Go straight to ALSA, and give anything that still wants an
# XDG runtime dir a writable one.
export SDL_AUDIODRIVER="${SDL_AUDIODRIVER:-alsa}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"

# To rule out audio entirely while debugging, set RICO8_NOAUDIO=1 here.
echo "rico8: launching $PLAYER on $(uname -m)" >"$DIR/log.txt"
"$PLAYER" "$DIR/carts" >>"$DIR/log.txt" 2>&1
