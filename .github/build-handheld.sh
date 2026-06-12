#!/usr/bin/env bash
# Cross-build rico8-player for aarch64 handhelds (PowKiddy RGB10S,
# Anbernic RG351/353, etc. running ArkOS / ROCKNIX / similar).
#
# The binary links the *device's* system libSDL2 at runtime; at build
# time we only need an aarch64 libSDL2.so to link against, which this
# script pulls from a Debian package. Requirements on the build host:
# rustup target aarch64-unknown-linux-gnu, aarch64-linux-gnu-gcc, zstd.
#
#   ./.github/build-handheld.sh [output-dir]
set -euo pipefail

out="${1:-dist/handheld}"
root="$(cd "$(dirname "$0")/.." && pwd)"
sdl_dir="$root/target/cross-sdl2-arm64"

SDL_DEB="libsdl2-2.0-0_2.32.4+dfsg-1_arm64.deb"
SDL_URL="https://deb.debian.org/debian/pool/main/libs/libsdl2/$SDL_DEB"

if [ ! -e "$sdl_dir/libSDL2.so" ]; then
  echo "fetching aarch64 libSDL2 for linking..."
  mkdir -p "$sdl_dir"
  (cd "$sdl_dir" \
    && curl -sfL -O "$SDL_URL" \
    && ar x "$SDL_DEB" \
    && tar xf data.tar.* \
    && ln -sf usr/lib/aarch64-linux-gnu/libSDL2-2.0.so.0 libSDL2.so)
fi

echo "building rico8-player for aarch64..."
RUSTFLAGS="-L $sdl_dir -C link-arg=-Wl,--allow-shlib-undefined" \
  cargo build --release -p rico8-player --target aarch64-unknown-linux-gnu

mkdir -p "$out/rico8/carts"
bin="$root/target/aarch64-unknown-linux-gnu/release/rico8-player"
cp "$bin" "$out/rico8/rico8-player"
aarch64-linux-gnu-strip "$out/rico8/rico8-player" 2>/dev/null || true
cp "$root/.github/RICO-8.sh" "$out/RICO-8.sh"
chmod +x "$out/RICO-8.sh" "$out/rico8/rico8-player"

echo "built $out:"
ls -la "$out" "$out/rico8"
