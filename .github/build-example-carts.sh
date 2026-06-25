#!/usr/bin/env bash
# Export every example cart to a shareable PNG, ready to drop onto a handheld's SD card.
#
# Each cart PNG is what the rico8 player loads directly, so the output directory mirrors the
# handheld bundle's rico8/carts folder and can be copied straight onto the device:
#
#   ./.github/build-example-carts.sh [output-dir]   # defaults to dist/handheld/rico8/carts
set -euo pipefail

out="${1:-dist/handheld/rico8/carts}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out"

# rico8 export needs no audio, so build the smaller no-default-features exporter.
echo "building the rico8 exporter..."
cargo build --release -p rico8-console --no-default-features
rico8="$root/target/release/rico8"

for manifest in "$root"/examples/*/Cargo.toml; do
  ex=$(basename "$(dirname "$manifest")")
  echo "exporting $ex..."
  "$rico8" export "$root/examples/$ex" "$out/$ex.png"
done

echo "exported example carts to $out:"
ls -la "$out"
