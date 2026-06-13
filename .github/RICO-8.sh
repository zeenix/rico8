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
# 32-bit armhf userland, while others are aarch64. Pick the binary that
# matches the device's dynamic loader. Prefer aarch64 when present: a
# 64-bit kernel may not run 32-bit code at all, whereas a 32-bit
# userland simply has no aarch64 loader (and routes aarch64 ELFs through
# qemu, which then fails to find that loader).
if [ -e /lib/ld-linux-aarch64.so.1 ] || [ -e /lib64/ld-linux-aarch64.so.1 ] \
   || [ -e /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1 ]; then
  PLAYER="$DIR/rico8-player.aarch64"
else
  PLAYER="$DIR/rico8-player.armhf"
fi

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
