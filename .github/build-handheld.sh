#!/usr/bin/env bash
# Cross-build rico8-player for ARM handhelds (PowKiddy RGB10S, Anbernic
# RG351/353, etc. running ArkOS / ROCKNIX / similar).
#
# These devices vary: the RK3326 chip is 64-bit, but many firmwares
# (ArkOS on the RGB10S) ship a 32-bit armhf userland, while others
# (some ROCKNIX builds) are aarch64. So we build BOTH and let the
# launcher pick by which dynamic loader the device has.
#
# The binaries link the *device's* system libSDL2 at runtime; at build
# time we only need an arm libSDL2.so to link against, pulled from a
# Debian package. We build with cargo-zigbuild against an OLD glibc
# baseline (RICO8_GLIBC, default 2.28) so the binary only requires
# symbols these firmwares (glibc ~2.31) actually have.
#
# Host needs: rustup, the two rust targets (auto-added below), zig +
# cargo-zigbuild (zig auto-fetched if missing), the arm binutils
# (gcc-aarch64-linux-gnu, gcc-arm-linux-gnueabihf) for strip/objdump,
# zstd, curl.
#
#   ./.github/build-handheld.sh [output-dir]
set -euo pipefail

out="${1:-dist/handheld}"
glibc="${RICO8_GLIBC:-2.28}"
root="$(cd "$(dirname "$0")/.." && pwd)"
sdl_ver="2.32.4+dfsg-1"

# Ensure zig is on PATH (cargo-zigbuild needs it).
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

mkdir -p "$out/rico8/carts"

# build_arch <rust-target> <deb-arch> <lib-triple> <suffix>
build_arch() {
  local rust_target="$1" deb_arch="$2" triple="$3" suffix="$4"
  local sdl_dir="$root/target/cross-sdl2-$suffix"
  local deb="libsdl2-2.0-0_${sdl_ver}_${deb_arch}.deb"

  rustup target add "$rust_target" >/dev/null 2>&1 || true
  if [ ! -e "$sdl_dir/libSDL2.so" ]; then
    echo "fetching $deb_arch libSDL2..."
    mkdir -p "$sdl_dir"
    (cd "$sdl_dir" \
      && curl -sfL -O "https://deb.debian.org/debian/pool/main/libs/libsdl2/$deb" \
      && ar x "$deb" \
      && tar xf data.tar.* \
      && ln -sf "usr/lib/$triple/libSDL2-2.0.so.0" libSDL2.so)
  fi

  echo "building rico8-player for $rust_target (glibc $glibc)..."
  RUSTFLAGS="-L $sdl_dir -C link-arg=-Wl,--allow-shlib-undefined" \
    cargo zigbuild --release -p rico8-player --target "$rust_target.$glibc"

  cp "$root/target/$rust_target/release/rico8-player" "$out/rico8/rico8-player.$suffix"
  "$triple-strip" "$out/rico8/rico8-player.$suffix" 2>/dev/null || true
  local maxsym
  maxsym=$("$triple-objdump" -T "$out/rico8/rico8-player.$suffix" 2>/dev/null \
    | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -V | tail -1)
  echo "  -> rico8-player.$suffix (max $maxsym)"
}

build_arch armv7-unknown-linux-gnueabihf armhf arm-linux-gnueabihf armhf
build_arch aarch64-unknown-linux-gnu arm64 aarch64-linux-gnu aarch64

cp "$root/.github/RICO-8.sh" "$out/RICO-8.sh"
chmod +x "$out/RICO-8.sh" "$out/rico8/"rico8-player.*

echo "built $out:"
ls -la "$out" "$out/rico8"
