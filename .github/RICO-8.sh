#!/bin/bash
# RICO-8 cart player launcher for handhelds (ArkOS / ROCKNIX / etc.),
# in the usual Ports layout:
#
#   /roms/ports/RICO-8.sh        <- this script
#   /roms/ports/rico8/           <- rico8-player binary
#   /roms/ports/rico8/carts/     <- put your .png carts here
#
# Controls: d-pad moves, the two action buttons = O / X. Select returns
# to the cart picker and Start+Select quits (on pads SDL recognizes as
# GameControllers); on any pad, holding both action buttons for ~1s
# returns to the picker.

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

# Help SDL recognize the device's gamepad as a GameController (so named
# buttons like Select/Start work) by pointing it at a controller
# database if the firmware ships one. Harmless if none is found -- the
# raw-joystick fallback (d-pad + face buttons + hold-O+X-to-exit) still
# works.
if [ -z "${RICO8_GCDB:-}" ] || [ ! -f "${RICO8_GCDB:-}" ]; then
  for db in \
    "$DIR/gamecontrollerdb.txt" \
    /roms/ports/PortMaster/gamecontrollerdb.txt \
    /opt/system/gamecontrollerdb.txt \
    /usr/local/share/gamecontrollerdb.txt \
    /usr/share/gamecontrollerdb.txt; do
    if [ -f "$db" ]; then export RICO8_GCDB="$db"; break; fi
  done
fi
echo "rico8: gcdb=${RICO8_GCDB:-none}" >>"$DIR/log.txt"

# These handhelds have no PulseAudio session, so SDL's default pulse
# probe just spews "failed to create secure directory" before falling
# back. Go straight to ALSA, and give anything that still wants an
# XDG runtime dir a writable one.
export SDL_AUDIODRIVER="${SDL_AUDIODRIVER:-alsa}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"

# On pads SDL doesn't recognize as GameControllers, Select/Start have no
# names -- only raw button indices. To use them, launch once, press the
# buttons in a game, and read the "joy button index N" lines from the log
# below; then set these to those indices and Select = back to picker,
# Start+Select = quit. (Hold both action buttons always works regardless.)
# export RICO8_SELECT=8
# export RICO8_START=9

# Full backtraces if the player panics. To rule out audio entirely while
# debugging, set RICO8_NOAUDIO=1 here.
export RUST_BACKTRACE=full
"$PLAYER" "$DIR/carts" >>"$DIR/log.txt" 2>&1
echo "rico8: player exited with code $? (>128 means killed by signal N-128)" >>"$DIR/log.txt"
