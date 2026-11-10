#!/usr/bin/env bash
# 打 Linux Web 宿主（GTK + WebKitGTK 渲染官方 React 前端）的 deb / rpm。
#
# 与 package-linux-egui.sh 的区别：
#   - 可执行文件是 `openless-linux-web`，装成 /usr/bin/openless
#   - **必须**带上前端产物 `linux-web/dist`（装到 /usr/lib/openless/web/dist）
#   - 依赖里要有 libwebkit2gtk-4.1（egui 版没有）
# 相同之处：fcitx5 插件仍要**现编**，不能装仓库里那份旧 build/libopenless.so。
set -euo pipefail

APP_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$APP_ROOT"

PKG_NAME=openless
VERSION=${OPENLESS_LINUX_VERSION:?set OPENLESS_LINUX_VERSION (e.g. 2.0.2-Beta.3-170)}
REVISION_FILE="$APP_ROOT/linux-web/package-revision"
if [ -f "$REVISION_FILE" ]; then
  EXPECTED=$(cat "$REVISION_FILE")
  case "$VERSION" in
    *-"$EXPECTED") ;;
    *) echo "package-revision ($EXPECTED) does not match OPENLESS_LINUX_VERSION ($VERSION)" >&2; exit 1 ;;
  esac
fi

TARGET_DIR=${CARGO_TARGET_DIR:-$APP_ROOT/target}/release
BINARY=${OPENLESS_WEB_BINARY:-$TARGET_DIR/openless-linux-web}
DIST_DIR=${OPENLESS_WEB_DIST:-$APP_ROOT/linux-web/dist}
PLUGIN_ROOT="$APP_ROOT/scripts/linux-fcitx5-plugin/build"
PACKAGING="$APP_ROOT/linux-egui/packaging"
ICON_DIR="$APP_ROOT/src-tauri/icons"
OUTPUT=${OPENLESS_PACKAGE_OUTPUT:-$APP_ROOT/target/linux-web-packages}
ARCH=amd64

require_file() {
  [ -s "$1" ] || { echo "missing or empty $1" >&2; exit 1; }
}

require_file "$BINARY"
# 胶囊窗口由 `capsule` feature 控制（需要 libgtk-layer-shell）。包里必须是带
# 这个 feature 的构建，否则录音时根本不会有胶囊。
if ! strings -a "$BINARY" | grep -q 'openless-capsule'; then
  echo "warning: $BINARY 没有 capsule 支持（用 --features capsule 重新构建）" >&2
fi
require_file "$DIST_DIR/index.html"
require_file "$PLUGIN_ROOT/libopenless.so"
require_file "$PLUGIN_ROOT/openless.conf"
require_file "$PACKAGING/openless.desktop"

# 插件必须现编：仓库里的 build/ 产物可能是旧的，那样插件侧修复不会进包。
if [ "${OPENLESS_SKIP_FCITX5_PLUGIN:-0}" != "1" ]; then
  echo "building the fcitx5 plugin into $PLUGIN_ROOT"
  cmake -S "$APP_ROOT/scripts/linux-fcitx5-plugin" -B "$PLUGIN_ROOT" -DCMAKE_BUILD_TYPE=Release >/dev/null
  cmake --build "$PLUGIN_ROOT" -j"$(nproc)" >/dev/null
fi

mkdir -p "$OUTPUT"

# 安装/卸载后都要重启 fcitx5：文件被替换/删除后，运行中的输入法仍持有旧插件映像。
# 只重连已存在的用户 DBus 会话，不启 daemon、不因为无图形会话让包事务失败。
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

DEB_ROOT="$OUTPUT/deb-root"
RPM_ROOT="$OUTPUT/rpm-root"
rm -rf "$DEB_ROOT" "$RPM_ROOT"

stage_common() {
  local root=$1
  install -Dm755 "$BINARY" "$root/usr/bin/openless"
  # 前端整目录（含 assets/ 与 index.html）
  mkdir -p "$root/usr/lib/openless/web/dist"
  cp -a "$DIST_DIR/." "$root/usr/lib/openless/web/dist/"
  install -Dm644 "$PACKAGING/openless.desktop" "$root/usr/share/applications/openless.desktop"
  [ -f "$PACKAGING/top.openless.OpenLess.metainfo.xml" ] && \
    install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" \
      "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  for size_file in "32x32:32x32.png" "64x64:64x64.png" "128x128:128x128.png" \
                   "256x256:128x128@2x.png" "512x512:icon.png"; do
    install -Dm644 "$ICON_DIR/${size_file#*:}" \
      "$root/usr/share/icons/hicolor/${size_file%%:*}/apps/openless.png"
  done
}

stage_plugin_deb() {
  install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
    "$1/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
  install -Dm644 "$PLUGIN_ROOT/openless.conf" \
    "$1/usr/share/fcitx5/addon/openless.conf"
}

stage_plugin_rpm() {
  install -Dm755 "$PLUGIN_ROOT/libopenless.so" "$1/usr/lib64/fcitx5/libopenless.so"
  install -Dm644 "$PLUGIN_ROOT/openless.conf" "$1/usr/share/fcitx5/addon/openless.conf"
}

# ── deb ────────────────────────────────────────────────────────────────────
stage_common "$DEB_ROOT"
stage_plugin_deb "$DEB_ROOT"
install -d "$DEB_ROOT/DEBIAN"
# 依赖以 WebKitGTK 4.1 为核心；libwebkit2gtk-4.1-0 与 javascriptcoregtk-4.1 是
# 这条渲染链的最小集合，缺任何一个宿主都起不来。
cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: $PKG_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenLess Contributors
Description: OpenLess Linux host (GTK + WebKitGTK, official React UI)
Depends: fcitx5, fcitx5-module-dbus, libgtk-3-0, libwebkit2gtk-4.1-0, libjavascriptcoregtk-4.1-0, libasound2, libc6, libdbus-1-3, libfcitx5config6, libfcitx5core7, libfcitx5utils2, libgcc-s1, libglib2.0-0, libpulse0, libstdc++6, libuuid1, libwayland-client0, libx11-6, libxkbcommon0
Recommends: pavucontrol
Homepage: https://github.com/Open-Less/openless
EOF
install -m755 "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postinst"
cp "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postrm"
chmod 0755 "$DEB_ROOT/DEBIAN/postrm"
dpkg-deb --build --root-owner-group "$DEB_ROOT" \
  "$OUTPUT/OpenLess-Linux-web-${VERSION}-x86_64.deb"

# ── rpm ────────────────────────────────────────────────────────────────────
if command -v rpmbuild >/dev/null; then
  stage_common "$RPM_ROOT"
  stage_plugin_rpm "$RPM_ROOT"
  cat > "$OUTPUT/openless-web.spec" << EOF
Name: $PKG_NAME
Version: ${VERSION%%-*}
Release: ${VERSION##*-}
Summary: OpenLess Linux host (GTK + WebKitGTK, official React UI)
License: MIT
BuildArch: x86_64
Requires: fcitx5, gtk3, webkit2gtk4.1, javascriptcoregtk4.1, alsa-lib
%description
OpenLess Linux host rendering the official React UI through GTK + WebKitGTK.
%prep
%build
%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a $RPM_ROOT/. %{buildroot}/
%files
/usr/share/metainfo/top.openless.OpenLess.metainfo.xml
/usr/bin/openless
/usr/lib/openless
/usr/share/applications/openless.desktop
/usr/share/icons/hicolor
/usr/share/fcitx5/addon/openless.conf
/usr/lib64/fcitx5/libopenless.so
EOF
  if ! rpmbuild -bb --define "_topdir $OUTPUT/rpmtop" --define "_rpmdir $OUTPUT/rpmout" \
      "$OUTPUT/openless-web.spec" >"$OUTPUT/rpmbuild.log" 2>&1; then
    echo "rpmbuild 失败，日志：$OUTPUT/rpmbuild.log（deb 已产出）" >&2
    tail -5 "$OUTPUT/rpmbuild.log" >&2
  fi
  find "$OUTPUT/rpmout" -name '*.rpm' -exec mv {} "$OUTPUT/OpenLess-Linux-web-${VERSION}-x86_64.rpm" \; 2>/dev/null || true
else
  echo "跳过 rpm：没有 rpmbuild" >&2
fi

(cd "$OUTPUT" && sha256sum ./*.deb ./*.rpm 2>/dev/null > SHA256SUMS || sha256sum ./*.deb > SHA256SUMS)
echo "packages written to $OUTPUT"
ls -la "$OUTPUT"/*.deb "$OUTPUT"/*.rpm 2>/dev/null || ls -la "$OUTPUT"/*.deb
