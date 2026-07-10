#!/usr/bin/env bash
# Inject fcitx5 plugin files into Linux packages at system paths.
# Usage: ./inject-fcitx5-plugin.sh <package-path>
#
# Supports: .deb, .rpm
# AppImage is NOT supported — fcitx5 runs on the host and cannot load
# addons from inside the AppImage mount.
set -euo pipefail

PKG="$1"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLUGIN_DIR="$SCRIPT_DIR/linux-fcitx5-plugin/build_release"
SO_SRC="$PLUGIN_DIR/libopenless.so"
CONF_SRC="$PLUGIN_DIR/openless.conf"

if [ ! -f "$SO_SRC" ] || [ ! -f "$CONF_SRC" ]; then
    echo "[inject-fcitx5] Plugin not built — run build.sh first. Skipping."
    exit 0
fi

TARGET_CONF="/usr/share/fcitx5/addon/openless.conf"

case "$PKG" in
    *.deb)
        # Detect multiarch triplet for the target architecture
        MULTIARCH=$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo "")
        if [ -n "$MULTIARCH" ]; then
            TARGET_LIB="/usr/lib/$MULTIARCH/fcitx5/libopenless.so"
        else
            TARGET_LIB="/usr/lib/fcitx5/libopenless.so"
        fi
        echo "[inject-fcitx5] Injecting into deb ($MULTIARCH): $PKG"
        TMPDIR=$(mktemp -d)
        trap 'rm -rf "$TMPDIR"' EXIT
        dpkg-deb -R "$PKG" "$TMPDIR"
        mkdir -p "$TMPDIR/$(dirname "$TARGET_LIB")"
        mkdir -p "$TMPDIR/$(dirname "$TARGET_CONF")"
        cp "$SO_SRC" "$TMPDIR/$TARGET_LIB"
        cp "$CONF_SRC" "$TMPDIR/$TARGET_CONF"
        dpkg-deb -b "$TMPDIR" "$PKG"
        echo "[inject-fcitx5] Done — deb updated"
        ;;
    *.rpm)
        TARGET_LIB="/usr/lib64/fcitx5/libopenless.so"
        echo "[inject-fcitx5] Injecting into rpm: $PKG"
        TMPDIR=$(mktemp -d)
        trap 'rm -rf "$TMPDIR"' EXIT
        cd "$TMPDIR"
        rpm2cpio "$PKG" | cpio -idm 2>/dev/null || true
        mkdir -p "$(dirname ".$TARGET_LIB")"
        mkdir -p "$(dirname ".$TARGET_CONF")"
        cp "$SO_SRC" ".$TARGET_LIB"
        cp "$CONF_SRC" ".$TARGET_CONF"
        if command -v rpmrebuild &>/dev/null; then
            rpmrebuild -np -d "$TMPDIR" "$PKG" 2>/dev/null || {
                echo "[inject-fcitx5] ERROR: rpmrebuild failed" >&2
                exit 1
            }
        else
            echo "[inject-fcitx5] ERROR: rpmrebuild not found — required for RPM injection. Install it with: sudo dnf install rpmrebuild" >&2
            exit 1
        fi
        echo "[inject-fcitx5] Done — rpm updated"
        ;;
    *)
        echo "[inject-fcitx5] Unknown package format: $PKG (supported: .deb, .rpm)"
        exit 1
        ;;
esac
