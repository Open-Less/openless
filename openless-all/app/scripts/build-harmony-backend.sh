#!/usr/bin/env bash
set -euo pipefail
APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_ROOT="${OPENLESS_CACHE_ROOT:-/data-win/openless-cache}"
TOOLCHAIN="${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}"
NATIVE="$TOOLCHAIN/sdk/default/openharmony/native"
export CARGO_HOME="$CACHE_ROOT/cargo"
export CARGO_TARGET_DIR="$CACHE_ROOT/harmony/rust-target"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$NATIVE/llvm/bin/aarch64-unknown-linux-ohos-clang"
export CC_aarch64_unknown_linux_ohos="$CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER"
export AR_aarch64_unknown_linux_ohos="$NATIVE/llvm/bin/llvm-ar"

# aws-lc-sys configures its CMake tree once and then caches the detected
# architecture: switching to a different toolchain path reuses rules that were
# generated for the previous one, and the build dies assembling x86_64 sources
# for aarch64. Rebuild only that crate when the toolchain moves.
marker="$CARGO_TARGET_DIR/ohos-toolchain-path"
if [ "$(cat "$marker" 2>/dev/null)" != "$TOOLCHAIN" ]; then
  rm -rf "$CARGO_TARGET_DIR"/aarch64-unknown-linux-ohos/release/build/aws-lc-sys-*
  mkdir -p "$CARGO_TARGET_DIR"
  printf '%s\n' "$TOOLCHAIN" > "$marker"
fi
cargo build --manifest-path "$APP_ROOT/Cargo.toml" -p openless-harmony-host --target aarch64-unknown-linux-ohos --release
