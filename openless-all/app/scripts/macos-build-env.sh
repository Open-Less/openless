#!/usr/bin/env bash
# Source locally; execute before rust-cache in GitHub Actions.
set -euo pipefail

MACOS_BUILD_APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Keep opt-level=3 and thin LTO, while allowing LLVM to compile large crates
# in parallel. The shared Cargo profile continues to govern other platforms.
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-16}"
# Avoid malformed proc-macro dylibs with the macOS strip tool.
export CARGO_PROFILE_RELEASE_STRIP=debuginfo
export RUSTC_WRAPPER="$MACOS_BUILD_APP_ROOT/scripts/rustc-macos-proc-macro-wrapper.sh"

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=$CARGO_PROFILE_RELEASE_CODEGEN_UNITS"
    echo "CARGO_PROFILE_RELEASE_STRIP=$CARGO_PROFILE_RELEASE_STRIP"
    echo "RUSTC_WRAPPER=$RUSTC_WRAPPER"
  } >> "$GITHUB_ENV"
fi
