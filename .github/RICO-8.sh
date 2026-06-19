#!/bin/bash
# RICO-8 cart player launcher for handhelds (ArkOS / ROCKNIX / etc.),
# in the usual Ports layout:
#
#   /roms/ports/RICO-8.sh        <- this script
#   /roms/ports/rico8/           <- rico8-player binary
#   /roms/ports/rico8/carts/     <- put your .png carts here
#
# Controls: d-pad moves, the two action buttons = O / X. Select returns
# to the cart picker; Start+Select quits. On any pad, holding both action
# buttons for ~1s also returns to the picker.

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
# that exits before opening the display/devices) and use the first that
# actually executes here.
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

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"

# If you know the raw evdev button indices for Select/Start on your pad,
# bind them here so Select = back to picker and Start+Select = quit.
# (Holding both action buttons always returns to the picker, regardless.)
# export RICO8_SELECT=8
# export RICO8_START=9

# Full backtraces if the player panics. To rule out audio entirely while
# debugging, set RICO8_NOAUDIO=1 here.
export RUST_BACKTRACE=full
"$PLAYER" "$DIR/carts" >>"$DIR/log.txt" 2>&1
echo "rico8: player exited with code $? (>128 means killed by signal N-128)" >>"$DIR/log.txt"
