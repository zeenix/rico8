#!/usr/bin/env bash
# Build rico8-player for ARM handhelds (PowKiddy / Anbernic running ArkOS / ROCKNIX / etc.).
#
# The binaries are fully-static musl builds (KMS/evdev/ALSA, no SDL, no glibc), so there is no
# toolchain to install beyond rustup + the two targets: no zig, no Docker, no cross-gcc. The
# launcher picks armhf vs aarch64 by the device's userland arch.
#
#   ./.github/build-handheld.sh [output-dir]
set -euo pipefail

out="${1:-dist/handheld}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out/rico8/carts"

build_arch() {
  local rust_target="$1" suffix="$2"
  rustup target add "$rust_target" >/dev/null 2>&1 || true
  echo "building rico8-player for $rust_target..."
  cargo build --release -p rico8-player --no-default-features --features kms --target "$rust_target"
  cp "$root/target/$rust_target/release/rico8-player" "$out/rico8/rico8-player.$suffix"
  echo "  -> rico8-player.$suffix ($(file -b "$out/rico8/rico8-player.$suffix" | cut -d, -f1,2))"
}

build_arch armv7-unknown-linux-musleabihf armhf
build_arch aarch64-unknown-linux-musl aarch64

cp "$root/.github/RICO-8.sh" "$out/RICO-8.sh"
chmod +x "$out/RICO-8.sh" "$out/rico8/"rico8-player.*

echo "built $out:"
ls -la "$out" "$out/rico8"
