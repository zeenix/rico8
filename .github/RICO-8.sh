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
chmod +x "$DIR/rico8-player" 2>/dev/null || true

# Extra controller mappings can be dropped next to the binary.
export RICO8_GCDB="$DIR/gamecontrollerdb.txt"

./rico8-player "$DIR/carts" >"$DIR/log.txt" 2>&1
