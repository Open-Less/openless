#!/usr/bin/env bash
#
# Build and optionally install the fcitx5 OpenLess plugin.
#
# Usage:
#   ./build.sh            # build only, .so in build/libopenless.so
#   ./build.sh install    # build + install to system fcitx5 dirs (requires sudo)
#
set -euo pipefail

cd "$(dirname "$0")"

BUILD_DIR="${BUILD_DIR:-build}"

echo "==> Configuring..."
cmake -S . -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE=Release

echo "==> Building..."
cmake --build "$BUILD_DIR" --parallel

echo "==> Plugin built: ${BUILD_DIR}/libopenless.so"

if [ "${1:-}" = "install" ]; then
    echo "==> Installing (requires sudo)..."
    sudo cmake --install "$BUILD_DIR"
    echo "==> Done. Restart fcitx5 to pick up the new plugin."
else
    echo "==> Use '$0 install' to install system-wide."
fi
