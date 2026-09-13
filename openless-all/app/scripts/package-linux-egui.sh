#!/usr/bin/env bash
set -euo pipefail

APP_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=${OPENLESS_LINUX_VERSION:?OPENLESS_LINUX_VERSION is required}
ARCH=${OPENLESS_LINUX_ARCH:-x86_64}
case "$ARCH" in
  x86_64)
    DEB_ARCH=amd64
    RPM_ARCH=x86_64
    DEB_LIB_ARCH=x86_64-linux-gnu
    NODE_ARCH=x64
    ;;
  aarch64 | arm64)
    ARCH=aarch64
    DEB_ARCH=arm64
    RPM_ARCH=aarch64
    DEB_LIB_ARCH=aarch64-linux-gnu
    NODE_ARCH=arm64
    ;;
  *) echo "Unsupported Linux package architecture: $ARCH" >&2; exit 1 ;;
esac
TARGET_DIR=${CARGO_TARGET_DIR:-"$APP_ROOT/target"}
BINARY="$TARGET_DIR/release/openless-linux-egui"
PLUGIN_ROOT="$APP_ROOT/../scripts/linux-fcitx5-plugin/build"
QWEN_RUNTIME="$APP_ROOT/src-tauri/vendor/qwen-asr/qwen_asr"
PI_BACKEND="$APP_ROOT/src-tauri/resources/pi-backend"
PACKAGING="$APP_ROOT/linux-egui/packaging"
OUTPUT="$TARGET_DIR/linux-egui-packages"
ICON="$APP_ROOT/src-tauri/icons/128x128@2x.png"

test -x "$BINARY"
test -s "$PLUGIN_ROOT/libopenless.so"
test -s "$PLUGIN_ROOT/openless.conf"
test -x "$QWEN_RUNTIME"
test -x "$PI_BACKEND/node"
test -x "$PI_BACKEND/openless-computer"
test -s "$PI_BACKEND/runtime/index.mjs"
test -s "$PI_BACKEND/manifest.json"
test "$("$PI_BACKEND/node" -p 'process.arch')" = "$NODE_ARCH"
test -s "$PACKAGING/openless.desktop"
test -s "$PACKAGING/top.openless.OpenLess.metainfo.xml"
test -s "$ICON"
command -v fpm > /dev/null
command -v appimagetool > /dev/null

mkdir -p "$OUTPUT"

stage_common() {
  local root=$1
  install -Dm755 "$BINARY" "$root/usr/bin/openless"
  install -Dm644 "$PACKAGING/openless.desktop" \
    "$root/usr/share/applications/openless.desktop"
  install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" \
    "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  install -Dm644 "$ICON" "$root/usr/share/icons/hicolor/256x256/apps/openless.png"
  install -Dm755 "$QWEN_RUNTIME" \
    "$root/usr/lib/openless/resources/qwen-asr/qwen_asr"
  mkdir -p "$root/usr/lib/openless/resources/pi-backend"
  cp -a "$PI_BACKEND/." "$root/usr/lib/openless/resources/pi-backend/"
}

DEB_ROOT="$TARGET_DIR/linux-egui-deb-root"
rm -rf "$DEB_ROOT"
stage_common "$DEB_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$DEB_ROOT/usr/lib/$DEB_LIB_ARCH/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$DEB_ROOT/usr/share/fcitx5/addon/openless.conf"
fpm -s dir -t deb -C "$DEB_ROOT" \
  -n openless -v "$VERSION" -a "$DEB_ARCH" \
  --description "OpenLess Linux egui host" \
  --license AGPL-3.0-only \
  --url https://github.com/Open-Less/openless \
  -d fcitx5 -d fcitx5-module-dbus -d libdbus-1-3 -d libasound2 -d libopenblas0-pthread \
  -d libxcb1 -d libgbm1 -d libegl1 -d libpipewire-0.3-0 -d libxkbcommon0 \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.deb" .

RPM_ROOT="$TARGET_DIR/linux-egui-rpm-root"
rm -rf "$RPM_ROOT"
stage_common "$RPM_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$RPM_ROOT/usr/share/fcitx5/addon/openless.conf"
fpm -s dir -t rpm -C "$RPM_ROOT" \
  -n openless -v "$VERSION" -a "$RPM_ARCH" \
  --description "OpenLess Linux egui host" \
  --license AGPL-3.0-only \
  --url https://github.com/Open-Less/openless \
  -d fcitx5 -d dbus-libs -d alsa-lib -d openblas \
  -d libxcb -d mesa-libgbm -d libglvnd-egl -d pipewire-libs -d libxkbcommon \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.rpm" .

APPDIR="$TARGET_DIR/OpenLess.AppDir"
rm -rf "$APPDIR"
stage_common "$APPDIR"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$APPDIR/usr/lib/openless/resources/linux-fcitx5-plugin/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$APPDIR/usr/lib/openless/resources/linux-fcitx5-plugin/openless.conf"
QWEN_APPDIR="$APPDIR/usr/lib/openless/resources/qwen-asr"
while read -r library; do
  case "$(basename "$library")" in
    libc.so.* | libm.so.* | libpthread.so.* | libdl.so.* | librt.so.* | ld-linux-*.so.*) continue ;;
  esac
  install -Dm755 "$library" "$QWEN_APPDIR/$(basename "$library")"
done < <(ldd "$QWEN_RUNTIME" | awk '$2 == "=>" && $3 ~ /^\// { print $3 }')
for binary in "$QWEN_APPDIR"/*; do
  patchelf --set-rpath '$ORIGIN' "$binary"
done
PI_APPDIR="$APPDIR/usr/lib/openless/resources/pi-backend"
mkdir -p "$PI_APPDIR/lib"
while read -r library; do
  case "$(basename "$library")" in
    libc.so.* | libm.so.* | libpthread.so.* | libdl.so.* | librt.so.* | ld-linux-*.so.*) continue ;;
  esac
  install -Dm755 "$library" "$PI_APPDIR/lib/$(basename "$library")"
done < <(ldd "$PI_BACKEND/node" "$PI_BACKEND/openless-computer" | awk '$2 == "=>" && $3 ~ /^\// { print $3 }' | sort -u)
for binary in "$PI_APPDIR/lib/"*; do
  [ -f "$binary" ] || continue
  patchelf --set-rpath '$ORIGIN' "$binary"
done
for binary in "$PI_APPDIR/node" "$PI_APPDIR/openless-computer"; do
  patchelf --set-rpath '$ORIGIN/lib' "$binary"
done
ln -s usr/bin/openless "$APPDIR/AppRun"
cp "$PACKAGING/openless.desktop" "$APPDIR/openless.desktop"
cp "$ICON" "$APPDIR/openless.png"
ln -s openless.png "$APPDIR/.DirIcon"
ARCH="$ARCH" appimagetool "$APPDIR" \
  "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.AppImage"

find "$OUTPUT" -maxdepth 1 -type f -printf '%f\n' | sort
