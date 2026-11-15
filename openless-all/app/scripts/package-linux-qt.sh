#!/usr/bin/env bash
# 打 Linux Qt6 宿主（QtWebEngine = Chromium，原生 Wayland + layer-shell 浮层）。
#
# 组成：
#   /usr/lib/openless/openless-qt           窗口层（Qt，Chromium 渲染）
#   /usr/lib/openless/openless-linux-web    后端 + HTTP 命令桥（Rust，无头运行）
#   /usr/lib/openless/web/dist              官方 React 前端产物
#   /usr/bin/openless                       启动器（exec openless-qt，后端由它拉起）
#
# 依赖说明：Qt WebEngine 必须**动态链接系统 Qt**（LGPLv3 要求可替换），
# 所以这里不捆绑 qt6 库，只在 Depends 里声明。
set -euo pipefail

APP_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$APP_ROOT"

PKG_NAME=openless
VERSION=${OPENLESS_LINUX_VERSION:?set OPENLESS_LINUX_VERSION (e.g. 2.0.2-Beta.3-172)}

TARGET_DIR=${CARGO_TARGET_DIR:-$APP_ROOT/target}/release
HOST_BIN=${OPENLESS_HOST_BINARY:-$TARGET_DIR/openless-linux-web}
QT_BIN=${OPENLESS_QT_BINARY:-$APP_ROOT/linux-qt/openless-qt}
DIST_DIR=${OPENLESS_WEB_DIST:-$APP_ROOT/linux-web/dist}
PLUGIN_ROOT="$APP_ROOT/scripts/linux-fcitx5-plugin/build"
PACKAGING="$APP_ROOT/linux-host/packaging"
ICON_DIR="$APP_ROOT/src-tauri/icons"
OUTPUT=${OPENLESS_PACKAGE_OUTPUT:-$APP_ROOT/target/linux-qt-packages}

require_file() {
  [ -s "$1" ] || { echo "missing or empty $1" >&2; exit 1; }
}

require_file "$QT_BIN"
require_file "$HOST_BIN"
require_file "$DIST_DIR/index.html"
require_file "$PACKAGING/openless.desktop"

# fcitx5 插件必须现编：仓库里的 build/ 产物可能是旧的。
if [ "${OPENLESS_SKIP_FCITX5_PLUGIN:-0}" != "1" ]; then
  echo "building the fcitx5 plugin into $PLUGIN_ROOT"
  cmake -S "$APP_ROOT/scripts/linux-fcitx5-plugin" -B "$PLUGIN_ROOT" -DCMAKE_BUILD_TYPE=Release >/dev/null
  cmake --build "$PLUGIN_ROOT" -j"$(nproc)" >/dev/null
fi
require_file "$PLUGIN_ROOT/libopenless.so"
require_file "$PLUGIN_ROOT/openless.conf"

rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"

# 安装/卸载后重启 fcitx5：只重连已存在的用户 DBus 会话，不启 daemon。
POST_INSTALL="$OUTPUT/openless-fcitx5-postinst"
cat > "$POST_INSTALL" << 'EOF'
#!/usr/bin/env bash
set +e
for bus in /run/user/[0-9]*/bus; do
  [ -S "$bus" ] || continue
  runtime_dir=${bus%/bus}
  uid=${runtime_dir##*/}
  [ "$uid" != "0" ] || continue
  user=$(getent passwd "$uid" | cut -d: -f1)
  [ -n "$user" ] || continue
  runuser -u "$user" -- env \
    XDG_RUNTIME_DIR="$runtime_dir" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
    timeout 5s dbus-send --session --dest=org.fcitx.Fcitx5 --type=method_call /controller org.fcitx.Fcitx.Controller1.Restart >/dev/null 2>&1 || true
done
exit 0
EOF
chmod 0755 "$POST_INSTALL"

stage_common() {
  local root=$1
  install -Dm755 "$QT_BIN" "$root/usr/lib/openless/openless-qt"
  install -Dm755 "$HOST_BIN" "$root/usr/lib/openless/openless-linux-web"
  mkdir -p "$root/usr/lib/openless/web/dist"
  cp -a "$DIST_DIR/." "$root/usr/lib/openless/web/dist/"
  # 启动器：Qt 宿主自己会拉起无头后端，所以这里只 exec 它。
  install -Dm755 /dev/stdin "$root/usr/bin/openless" << 'LAUNCHER'
#!/usr/bin/env bash
set -e
# 目录相对解析：安装后是 /usr/lib/openless，解包试跑时也能工作。
here=$(dirname "$(readlink -f "$0")")/../lib/openless
# Qt 宿主负责拉起同目录下的 openless-linux-web（无头）并在退出时回收。
exec "$here/openless-qt" "$@"
LAUNCHER
  install -Dm644 "$PACKAGING/openless.desktop" "$root/usr/share/applications/openless.desktop"
  [ -f "$PACKAGING/top.openless.OpenLess.metainfo.xml" ] && \
    install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" \
      "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  for spec in "32x32:32x32.png" "64x64:64x64.png" "128x128:128x128.png" \
              "256x256:128x128@2x.png" "512x512:icon.png"; do
    install -Dm644 "$ICON_DIR/${spec#*:}" \
      "$root/usr/share/icons/hicolor/${spec%%:*}/apps/openless.png"
  done
  # LGPLv3 合规：随包附带 Qt / QtWebEngine / LayerShellQt 的许可证与声明。
  install -Dm644 "$APP_ROOT/linux-qt/THIRD-PARTY.md" \
    "$root/usr/share/doc/openless/THIRD-PARTY.md"
  for pkg in libqt6core6 libqt6webenginecore6 liblayershellqtinterface6; do
    if [ -f "/usr/share/doc/$pkg/copyright" ]; then
      install -Dm644 "/usr/share/doc/$pkg/copyright" \
        "$root/usr/share/doc/openless/licenses/$pkg.txt"
    fi
  done
}

DEB_ROOT="$OUTPUT/deb-root"
stage_common "$DEB_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$DEB_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" "$DEB_ROOT/usr/share/fcitx5/addon/openless.conf"
install -d "$DEB_ROOT/DEBIAN"
cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: $PKG_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenLess Contributors
Description: OpenLess Linux host (Qt6 WebEngine / Chromium, native Wayland)
Depends: fcitx5, fcitx5-module-dbus, fcitx5-frontend-qt6, libc6, libgcc-s1, libstdc++6, libqt6core6, libqt6gui6, libqt6widgets6, libqt6network6, libqt6webenginecore6, libqt6webenginewidgets6, liblayershellqtinterface6, qt6-wayland, libfcitx5core7, libfcitx5config6, libfcitx5utils2
Recommends: pavucontrol
Homepage: https://github.com/Open-Less/openless
EOF
install -m755 "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postinst"
cp "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postrm"
dpkg-deb --build --root-owner-group "$DEB_ROOT" \
  "$OUTPUT/OpenLess-Linux-qt-${VERSION}-x86_64.deb"

(cd "$OUTPUT" && sha256sum ./*.deb > SHA256SUMS)
echo "done: $OUTPUT"
ls -la "$OUTPUT"/*.deb
