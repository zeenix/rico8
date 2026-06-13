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

# Ports live on a FAT/exFAT partition that doesn't keep the Unix
# executable bit, so restore it before launching (ignored if the bit
# is already set, e.g. on ext4).
chmod +x "$DIR/"rico8-player.* 2>/dev/null || true

# These chips are 64-bit, but the firmware's ports runtime may be 32-bit
# armhf -- in which case an aarch64 binary is routed through
# qemu-aarch64-static and fails. uname/loader checks report the kernel,
# not this runtime, so don't guess: probe each binary (a cheap self-test
# that exits before SDL) and use the first that actually executes here.
# Override with RICO8_ARCH=aarch64|armhf.
echo "rico8: $(uname -a)" >"$DIR/log.txt"
PLAYER=""
for arch in ${RICO8_ARCH:-aarch64 armhf}; do
  cand="$DIR/rico8-player.$arch"
  if probe=$("$cand" --probe 2>&1); then
    echo "rico8: probe $arch: ok ($probe)" >>"$DIR/log.txt"
    [ -z "$PLAYER" ] && PLAYER="$cand"
  else
    echo "rico8: probe $arch: failed ($probe)" >>"$DIR/log.txt"
  fi
done
if [ -z "$PLAYER" ]; then
  echo "rico8: no runnable binary for this device" >>"$DIR/log.txt"
  exit 1
fi
echo "rico8: using $PLAYER" >>"$DIR/log.txt"

# Extra controller mappings can be dropped next to the binary.
export RICO8_GCDB="$DIR/gamecontrollerdb.txt"

# These handhelds have no PulseAudio session, so SDL's default pulse
# probe just spews "failed to create secure directory" before falling
# back. Go straight to ALSA, and give anything that still wants an
# XDG runtime dir a writable one.
export SDL_AUDIODRIVER="${SDL_AUDIODRIVER:-alsa}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"

# Full backtraces if the player panics. To rule out audio entirely while
# debugging, set RICO8_NOAUDIO=1 here.
export RUST_BACKTRACE=full
"$PLAYER" "$DIR/carts" >>"$DIR/log.txt" 2>&1
echo "rico8: player exited with code $? (>128 means killed by signal N-128)" >>"$DIR/log.txt"
