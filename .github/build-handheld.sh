#!/usr/bin/env bash
# Cross-build rico8-player for aarch64 handhelds (PowKiddy RGB10S,
# Anbernic RG351/353, etc. running ArkOS / ROCKNIX / similar).
#
# The binary links the *device's* system libSDL2 at runtime; at build
# time we only need an aarch64 libSDL2.so to link against, which this
# script pulls from a Debian package.
#
# We build with cargo-zigbuild against an OLD glibc baseline
# (RICO8_GLIBC, default 2.28) so the binary only requires symbols these
# devices actually have — their firmwares ship glibc ~2.31, while a
# modern cross toolchain would emit GLIBC_2.34+ references that fail to
# load. Requirements on the build host: rustup target
# aarch64-unknown-linux-gnu, zig + cargo-zigbuild (this script installs
# a local zig if none is found), zstd.
#
#   ./.github/build-handheld.sh [output-dir]
set -euo pipefail

out="${1:-dist/handheld}"
glibc="${RICO8_GLIBC:-2.28}"
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

# Ensure zig is on PATH (cargo-zigbuild needs it); fetch a local copy
# if missing.
if ! command -v zig >/dev/null 2>&1; then
  zig_dir="$root/target/zig"
  if [ ! -x "$zig_dir/zig" ]; then
    echo "fetching zig..."
    ver=0.13.0
    mkdir -p "$zig_dir"
    curl -sfL "https://ziglang.org/download/$ver/zig-linux-x86_64-$ver.tar.xz" \
      | tar xJ -C "$zig_dir" --strip-components=1
  fi
  export PATH="$zig_dir:$PATH"
fi
command -v cargo-zigbuild >/dev/null 2>&1 || cargo install cargo-zigbuild

echo "building rico8-player for aarch64 (glibc $glibc baseline)..."
RUSTFLAGS="-L $sdl_dir -C link-arg=-Wl,--allow-shlib-undefined" \
  cargo zigbuild --release -p rico8-player \
    --target "aarch64-unknown-linux-gnu.$glibc"

mkdir -p "$out/rico8/carts"
bin="$root/target/aarch64-unknown-linux-gnu/release/rico8-player"
cp "$bin" "$out/rico8/rico8-player"
"${STRIP:-aarch64-linux-gnu-strip}" "$out/rico8/rico8-player" 2>/dev/null || true
cp "$root/.github/RICO-8.sh" "$out/RICO-8.sh"
chmod +x "$out/RICO-8.sh" "$out/rico8/rico8-player"

echo "built $out (max glibc symbol below):"
aarch64-linux-gnu-objdump -T "$out/rico8/rico8-player" 2>/dev/null \
  | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -V | tail -1 || true
ls -la "$out" "$out/rico8"
