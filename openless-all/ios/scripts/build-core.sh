#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-all}"
case "$PLATFORM" in
  all|device|simulator) ;;
  *) echo 'Usage: scripts/build-core.sh [all|device|simulator]' >&2; exit 2;;
esac
python3 "$ROOT/scripts/prepare-core.py"
# Homebrew rustc can precede rustup shims even when cargo is launched by rustup.
export RUSTC="$(rustup which --toolchain 1.99.0 rustc)"
export RUSTDOC="$(rustup which --toolchain 1.99.0 rustdoc)"
# Isolate iOS 16 artifacts from caches built with another deployment floor.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.build/core-ios16}"
for spec in "device:aarch64-apple-ios:iphoneos" "simulator:aarch64-apple-ios-sim:iphonesimulator"; do
  IFS=: read -r platform target sdk <<< "$spec"
  [ "$PLATFORM" = all ] || [ "$PLATFORM" = "$platform" ] || continue
  mkdir -p "$ROOT/Frameworks/$platform"
  IPHONEOS_DEPLOYMENT_TARGET=16.0 SDKROOT="$(xcrun --sdk "$sdk" --show-sdk-path)" \
    rustup run 1.99.0 cargo build --manifest-path "$ROOT/CoreHost/Cargo.toml" --locked --release --target "$target"
  cp "$CARGO_TARGET_DIR/$target/release/libopenless_ios_host.a" "$ROOT/Frameworks/$platform/libOpenLessCore.a"
  python3 "$ROOT/scripts/validate-deployment.py" "$ROOT/Frameworks/$platform/libOpenLessCore.a"
done
