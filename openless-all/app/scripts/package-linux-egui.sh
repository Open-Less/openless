#!/usr/bin/env bash
set -euo pipefail

APP_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=${OPENLESS_LINUX_VERSION:?OPENLESS_LINUX_VERSION is required}
PACKAGE_VERSION=${VERSION/-/\~}
ARCH=${OPENLESS_LINUX_ARCH:-x86_64}
test "$ARCH" = x86_64 || { echo 'Only x86_64 packages are supported' >&2; exit 1; }
TARGET_DIR=$(realpath -m "${CARGO_TARGET_DIR:-"$APP_ROOT/target"}")
case "$TARGET_DIR/" in "$APP_ROOT/target/"*) ;; *) echo 'Package staging must be under app/target' >&2; exit 1;; esac
BINARY="$TARGET_DIR/release/openless-linux-egui"
PLUGIN_ROOT=${OPENLESS_FCITX_BUILD:-"$APP_ROOT/../scripts/linux-fcitx5-plugin/build"}
DESKTOP_ROOT="$APP_ROOT/../scripts/linux-desktop"
DESKTOP_BUILD=${OPENLESS_DESKTOP_BUILD:-"$DESKTOP_ROOT/kde/build"}
QWEN_RUNTIME="$APP_ROOT/src-tauri/vendor/qwen-asr/qwen_asr"
PACKAGING="$APP_ROOT/linux-egui/packaging"
OUTPUT="$TARGET_DIR/linux-egui-packages"
ICON="$APP_ROOT/src-tauri/icons/128x128@2x.png"
FONT=${OPENLESS_FONT:-/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc}
for file in "$BINARY" "$QWEN_RUNTIME" "$DESKTOP_BUILD/openless-desktop-bridge"; do test -x "$file"; done
for file in "$PLUGIN_ROOT/libopenless.so" "$PLUGIN_ROOT/openless.conf" "$FONT" "$ICON"; do test -s "$file"; done
for tool in fpm appimagetool patchelf desktop-file-validate; do command -v "$tool" >/dev/null; done
desktop-file-validate "$PACKAGING/openless.desktop"
mkdir -p "$OUTPUT"
STAGE=$(mktemp -d "$TARGET_DIR/linux-package.XXXXXX")
cleanup() { case "$STAGE" in "$TARGET_DIR"/linux-package.*) rm -rf -- "$STAGE";; esac; }
trap cleanup EXIT

# ldd reports transitive dependencies. Keep glibc and graphics drivers on the
# target desktop; all other runtime libraries go beside the private binary.
bundle_libraries() {
  local binary=$1 destination=$2 notices=$3 library owner package copyright
  mkdir -p "$destination"
  mkdir -p "$notices"
  if ldd "$binary" | grep -q 'not found'; then ldd "$binary"; return 1; fi
  while read -r library; do
    case "$(basename "$library")" in
      libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|ld-linux-*.so.*|libEGL.so.*|libGL.so.*|libGLX.so.*|libGLdispatch.so.*|libdrm.so.*) continue;;
    esac
    install -m755 "$library" "$destination/$(basename "$library")"
    # Keep distribution notices beside every private runtime, including the
    # standalone desktop-component archive. The build baseline is Ubuntu.
    owner=$(dpkg-query -S "$library" 2>/dev/null | head -1 || true)
    if [ -z "$owner" ]; then owner=$(dpkg-query -S "$(realpath "$library")" 2>/dev/null | head -1 || true); fi
    package=${owner%%: /*}; package=${package%%:*}
    copyright="/usr/share/doc/$package/copyright"
    if [ -n "$package" ] && [ -f "$copyright" ]; then
      install -m644 "$copyright" "$notices/$package-copyright"
    fi
  done < <(ldd "$binary" | awk '$2 == "=>" && $3 ~ /^\// {print $3}')
  for library in "$destination"/*; do patchelf --set-rpath '$ORIGIN' "$library"; done
}

stage_common() {
  local root=$1 resources="$1/usr/lib/openless/resources"
  install -Dm755 "$BINARY" "$root/usr/bin/openless"
  install -Dm644 "$PACKAGING/openless.desktop" "$root/usr/share/applications/openless.desktop"
  install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  install -Dm644 "$ICON" "$root/usr/share/icons/hicolor/256x256/apps/openless.png"
  install -Dm755 "$QWEN_RUNTIME" "$resources/qwen-asr/qwen_asr"
  if [ -f "$(dirname "$QWEN_RUNTIME")/LICENSE" ]; then
    install -Dm644 "$(dirname "$QWEN_RUNTIME")/LICENSE" "$resources/qwen-asr/LICENSE"
  fi
  for relative in install.sh README.md gnome/legacy/metadata.json gnome/legacy/extension.js gnome/modern/metadata.json gnome/modern/extension.js kwin/metadata.json kwin/metadata.desktop kwin/contents/code/main.js; do
    install -Dm644 "$DESKTOP_ROOT/$relative" "$resources/linux-desktop/$relative"
  done
  chmod +x "$resources/linux-desktop/install.sh"
  install -Dm755 "$DESKTOP_BUILD/openless-desktop-bridge" "$resources/linux-desktop/openless-desktop-bridge"
  bundle_libraries "$DESKTOP_BUILD/openless-desktop-bridge" "$resources/linux-desktop/lib" "$resources/linux-desktop/licenses"
  local plugin_root
  plugin_root=$(qmake -query QT_INSTALL_PLUGINS)
  install -Dm755 "$plugin_root/platforms/libqoffscreen.so" "$resources/linux-desktop/plugins/platforms/libqoffscreen.so"
  bundle_libraries "$plugin_root/platforms/libqoffscreen.so" "$resources/linux-desktop/lib" "$resources/linux-desktop/licenses"
  patchelf --set-rpath '$ORIGIN/../../lib' "$resources/linux-desktop/plugins/platforms/libqoffscreen.so"
  patchelf --set-rpath '$ORIGIN/lib' "$resources/linux-desktop/openless-desktop-bridge"
  install -Dm755 "$PACKAGING/openless-desktop-integration" "$root/usr/bin/openless-desktop-integration"
  install -Dm644 "$APP_ROOT/../../LICENSE" "$root/usr/share/doc/openless/copyright"
}

DEB_ROOT="$STAGE/deb"
stage_common "$DEB_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" "$DEB_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" "$DEB_ROOT/usr/share/fcitx5/addon/openless.conf"
fpm --force -s dir -t deb -C "$DEB_ROOT" -n openless -v "$PACKAGE_VERSION" -a amd64 \
  --description 'OpenLess 2.0 Linux egui desktop' --license AGPL-3.0-only --url https://github.com/Open-Less/openless \
  -d fcitx5 -d fcitx5-module-dbus -d libdbus-1-3 -d libasound2 -d libopenblas0-pthread \
  -d pulseaudio-utils -d fonts-noto-cjk -d libxkbcommon0 -d libxcb1 -d libssl3 \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.deb" .

RPM_ROOT="$STAGE/rpm"
stage_common "$RPM_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" "$RPM_ROOT/usr/share/fcitx5/addon/openless.conf"
fpm --force -s dir -t rpm -C "$RPM_ROOT" -n openless -v "$PACKAGE_VERSION" -a x86_64 \
  --description 'OpenLess 2.0 Linux egui desktop' --license AGPL-3.0-only --url https://github.com/Open-Less/openless \
  -d fcitx5 -d dbus-libs -d alsa-lib -d openblas -d pulseaudio-utils -d google-noto-sans-cjk-fonts \
  -d libxkbcommon -d libxcb -d openssl-libs \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.rpm" .

APPDIR="$STAGE/OpenLess.AppDir"
stage_common "$APPDIR"
RESOURCES="$APPDIR/usr/lib/openless/resources"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" "$RESOURCES/linux-fcitx5-plugin/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" "$RESOURCES/linux-fcitx5-plugin/openless.conf"
install -Dm644 "$FONT" "$RESOURCES/fonts/NotoSansCJK-Regular.ttc"
for license in /usr/share/doc/fonts-noto-cjk/copyright /usr/share/doc/libqt5core5a/copyright /usr/share/doc/libkf5globalaccel5/copyright; do
  test ! -f "$license" || install -Dm644 "$license" "$APPDIR/usr/share/doc/openless/$(basename "$(dirname "$license")")-copyright"
done
bundle_libraries "$BINARY" "$APPDIR/usr/lib/openless/bundle" "$APPDIR/usr/share/doc/openless/bundled-libraries"
patchelf --set-rpath '$ORIGIN/../lib/openless/bundle' "$APPDIR/usr/bin/openless"
bundle_libraries "$QWEN_RUNTIME" "$RESOURCES/qwen-asr/lib" "$RESOURCES/qwen-asr/licenses"
patchelf --set-rpath '$ORIGIN/lib' "$RESOURCES/qwen-asr/qwen_asr"
install -m755 "$PACKAGING/AppRun" "$APPDIR/AppRun"
cp "$PACKAGING/openless.desktop" "$APPDIR/openless.desktop"
cp "$ICON" "$APPDIR/openless.png"
ln -s openless.png "$APPDIR/.DirIcon"
ARCH="$ARCH" appimagetool "$APPDIR" "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.AppImage"

install -m755 "$BINARY" "$OUTPUT/openless-linux-egui"
tar -C "$RESOURCES" -czf "$OUTPUT/OpenLess-desktop-integration-${VERSION}-${ARCH}.tar.gz" linux-desktop
(cd "$OUTPUT"; sha256sum openless-linux-egui *.deb *.rpm *.AppImage *.tar.gz > SHA256SUMS)
find "$OUTPUT" -maxdepth 1 -type f -printf '%f\n' | sort
