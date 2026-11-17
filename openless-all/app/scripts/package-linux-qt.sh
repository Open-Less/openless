#!/usr/bin/env bash
# 打 Linux Qt6 宿主（QtWebEngine = Chromium，原生 Wayland + layer-shell 浮层）。
#
# 组成：
#   /usr/lib/openless/openless-qt           窗口层（Qt，Chromium 渲染）
#   /usr/lib/openless/openless-linux-web    后端 + HTTP 命令桥（Rust，无头运行）
#   /usr/lib/openless/web/dist              官方 React 前端产物
#   /usr/bin/openless                       启动器（exec 窗口层，后端由它拉起）
#
# 依赖说明：WebEngine 必须动态链接系统库（LGPLv3 要求可替换），
# 所以这里不捆绑相关库，只在 Depends 里声明。
set -euo pipefail

APP_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$APP_ROOT"

PKG_NAME=openless
VERSION=${OPENLESS_LINUX_VERSION:?set OPENLESS_LINUX_VERSION (e.g. 2.0.2-Beta.3-172)}
# 包序号与 linux-host/package-revision 交叉校验：CI 用它算版本号，
# 两边不一致会打出同名不同内容的包。
REVISION=$(cat "$APP_ROOT/linux-host/package-revision")
EXPECTED_VERSION="${VERSION%-*}-${REVISION}"
if [ "$VERSION" != "$EXPECTED_VERSION" ]; then
  echo "OPENLESS_LINUX_VERSION ($VERSION) does not match linux-host/package-revision ($REVISION)" >&2
  exit 1
fi

TARGET_DIR=${CARGO_TARGET_DIR:-$APP_ROOT/target}/release
HOST_BIN=${OPENLESS_HOST_BINARY:-$TARGET_DIR/openless-linux-web}
QT_BIN=${OPENLESS_QT_BINARY:-$APP_ROOT/linux-qt/openless-qt}
DIST_DIR=${OPENLESS_WEB_DIST:-$APP_ROOT/linux-web/dist}
PLUGIN_ROOT="$APP_ROOT/scripts/linux-fcitx5-plugin/build"
PACKAGING="$APP_ROOT/linux-host/packaging"
ICON_DIR="$APP_ROOT/linux-host/packaging/icons"
OUTPUT=${OPENLESS_PACKAGE_OUTPUT:-${CARGO_TARGET_DIR:-$APP_ROOT/target}/linux-host-packages}

require_file() {
  [ -s "$1" ] || { echo "missing or empty $1" >&2; exit 1; }
}

# 窗口层是构建产物：源码比二进制新就重新编译，绝不把旧二进制打进包
# （踩过：改了 main.cpp 但 package 脚本直接用仓库里的旧 openless-qt）。
QT_HOST_SRC="$APP_ROOT/linux-qt/src/main.cpp"
QT_HOST_BIN="$APP_ROOT/linux-qt/openless-qt"
if [ "${OPENLESS_SKIP_QT_BUILD:-0}" != "1" ]; then
  if [ ! -f "$QT_HOST_BIN" ] || [ "$QT_HOST_SRC" -nt "$QT_HOST_BIN" ]; then
    echo "编译窗口层：$QT_HOST_SRC"
    if pkg-config --exists Qt6Core 2>/dev/null; then
      # 发行版把 Qt 头文件放在 /usr/include/<triplet>/qt6，交给 pkg-config 拼。
      QT_CFLAGS=$(pkg-config --cflags Qt6Quick Qt6WebEngineWidgets Qt6Widgets Qt6Gui Qt6Core Qt6Network)
      QT_LIBS=$(pkg-config --libs Qt6Quick Qt6WebEngineWidgets Qt6WebEngineCore Qt6Widgets Qt6Gui Qt6Core Qt6Network)
      LS_CFLAGS=$(pkg-config --cflags LayerShellQtInterface 2>/dev/null || echo "-I/usr/include")
      LS_LIBS=$(pkg-config --libs LayerShellQtInterface 2>/dev/null || echo "-lLayerShellQtInterface")
    else
      # 没有 dev 包的环境（本机只装了运行时库）：允许用 OPENLESS_QT_PREFIX 指向
      # 一份解包出来的 Qt 头文件树，链接仍走系统运行库。
      prefix=${OPENLESS_QT_PREFIX:?未找到 Qt6 开发包：请安装 qt6-base-dev / qt6-webengine-dev，或设置 OPENLESS_QT_PREFIX 指向含 Qt 头文件的目录}
      QT_INC="$prefix/include/x86_64-linux-gnu/qt6"
      QT_CFLAGS="-I$QT_INC -I$QT_INC/QtCore -I$QT_INC/QtGui -I$QT_INC/QtWidgets -I$QT_INC/QtNetwork \
        -I$QT_INC/QtWebEngineCore -I$QT_INC/QtWebEngineWidgets -I$QT_INC/QtQuick \
        -I$prefix/include"
      QT_LIBS="/usr/lib/x86_64-linux-gnu/libQt6WebEngineWidgets.so.6 \
        /usr/lib/x86_64-linux-gnu/libQt6WebEngineCore.so.6 \
        /usr/lib/x86_64-linux-gnu/libQt6Widgets.so.6 /usr/lib/x86_64-linux-gnu/libQt6Gui.so.6 \
        /usr/lib/x86_64-linux-gnu/libQt6Core.so.6 /usr/lib/x86_64-linux-gnu/libQt6Network.so.6 \
        /usr/lib/x86_64-linux-gnu/libQt6Quick.so.6"
      LS_CFLAGS=""
      LS_LIBS="/usr/lib/x86_64-linux-gnu/libLayerShellQtInterface.so.6"
    fi
    g++ -std=c++17 -O2 -s -fPIC "$QT_HOST_SRC" -o "$QT_HOST_BIN" \
      $QT_CFLAGS $LS_CFLAGS $QT_LIBS $LS_LIBS
  fi
fi
require_file "$QT_BIN"
require_file "$HOST_BIN"
require_file "$QT_HOST_BIN"
require_file "$DIST_DIR/index.html"

# 前端包必须比源码新：之前打过一版包，Rust 改了、dist 还是 11 号的旧构建，
# 装上去 Linux 的能力屏蔽（本地模型 / 剪贴板插入）全都不生效。
if [ "${OPENLESS_SKIP_DIST_CHECK:-0}" != "1" ]; then
  newest_src=$(find "$APP_ROOT/src" -type f -newer "$DIST_DIR/index.html" -print -quit 2>/dev/null || true)
  if [ -n "$newest_src" ]; then
    echo "前端 dist 比源码旧（例如 $newest_src）。先重新构建：" >&2
    echo "  VITE_OPENLESS_PLATFORM=gtk npx vite build --outDir linux-web/dist --emptyOutDir" >&2
    exit 1
  fi
fi
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
WORK="$OUTPUT/.work"
mkdir -p "$WORK"
POST_INSTALL="$WORK/openless-fcitx5-postinst"
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
  # 只有 fcitx5 **已经**是当前会话的输入法时才重启它。
  # 不加这一步，dbus-send 会通过 org.fcitx.Fcitx5.service 触发 D-Bus 自动激活，
  # 把原本用 IBus 的桌面强行换成 fcitx5，用户的中文输入当场失效（issue #1404）。
  owner=$(runuser -u "$user" -- env \
    XDG_RUNTIME_DIR="$runtime_dir" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
    timeout 5s dbus-send --session --print-reply --dest=org.freedesktop.DBus \
      /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner \
      string:org.fcitx.Fcitx5 2>/dev/null | grep -c 'boolean true')
  [ "$owner" = "1" ] || continue
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
lib=$(dirname "$(readlink -f "$0")")/../lib/openless
share=$(dirname "$(readlink -f "$0")")/../share/openless
if [ "$1" = "--version" ]; then cat "$share/version"; exit 0; fi
# Qt 宿主负责拉起同目录下的 openless-linux-web（无头）并在退出时回收。
exec "$lib/openless-qt" "$@"
LAUNCHER
  install -d "$root/usr/share/openless"
  printf 'OpenLess %s\n' "$VERSION" > "$root/usr/share/openless/version"
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

DEB_ROOT="$WORK/deb-root"
stage_common "$DEB_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$DEB_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" "$DEB_ROOT/usr/share/fcitx5/addon/openless.conf"
# 记录插件校验和：用户本地残留的旧插件（或被发行版覆盖）能一眼看出来。
install -d "$DEB_ROOT/usr/share/openless"
printf 'openless %s fcitx5-addon-sha256 %s\n' "$VERSION" \
  "$(sha256sum "$DEB_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so" | cut -d' ' -f1)" \
  > "$DEB_ROOT/usr/share/openless/fcitx5-addon.sha256"
install -d "$DEB_ROOT/DEBIAN"
cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: $PKG_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenLess Contributors
Description: OpenLess for Linux - voice dictation and writing assistant
Depends: fcitx5, fcitx5-module-dbus, fcitx5-frontend-qt6, libc6, libgcc-s1, libstdc++6, libqt6core6, libqt6gui6, libqt6widgets6, libqt6network6, libqt6webenginecore6, libqt6webenginewidgets6, liblayershellqtinterface6, qt6-wayland, libfcitx5core7, libfcitx5config6, libfcitx5utils2
Recommends: pavucontrol, mesa-vulkan-drivers
Homepage: https://github.com/Open-Less/openless
EOF
install -m755 "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postinst"
cp "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postrm"
dpkg-deb --build --root-owner-group "$DEB_ROOT" \
  "$OUTPUT/OpenLess-Linux-${VERSION}-x86_64.deb"

# ── rpm ─────────────────────────────────────────────────────────────────────
if command -v rpmbuild >/dev/null; then
  RPM_ROOT="$WORK/rpm-root"
  rm -rf "$RPM_ROOT"
  cp -a "$DEB_ROOT" "$RPM_ROOT"
  rm -rf "$RPM_ROOT/DEBIAN"
  # RPM 系发行版把 fcitx5 插件放在 /usr/lib64
  mkdir -p "$RPM_ROOT/usr/lib64/fcitx5"
  mv "$RPM_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so" \
    "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
  cat > "$WORK/openless.spec" << EOF
Name: $PKG_NAME
Version: ${VERSION%%-*}
Release: ${VERSION##*-}
Summary: OpenLess for Linux
License: AGPL-3.0-only
BuildArch: x86_64
# 不要在打包期 strip / 拆 debuginfo：包内字节必须和 deb 里的一致，
# 否则 /usr/share/openless/fcitx5-addon.sha256 记录的校验和对不上。
%global debug_package %{nil}
%global __strip /bin/true
%global __os_install_post %{nil}
Requires: fcitx5, fcitx5-frontend-qt6, qt6-qtwebengine, qt6-qtwayland, LayerShellQt
%description
OpenLess Linux host rendering the official React UI through Qt6 WebEngine.
%prep
%build
%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a $RPM_ROOT/. %{buildroot}/
%files
/usr/bin/openless
/usr/lib/openless
/usr/share/applications/openless.desktop
/usr/share/icons/hicolor/*/apps/openless.png
/usr/share/fcitx5/addon/openless.conf
/usr/lib64/fcitx5/libopenless.so
/usr/share/doc/openless
/usr/share/metainfo/top.openless.OpenLess.metainfo.xml
/usr/share/openless/version
/usr/share/openless/fcitx5-addon.sha256
EOF
  if rpmbuild --define "_topdir $WORK/rpmtop" --define "_rpmdir $WORK/rpmout" -bb \
      "$WORK/openless.spec" >"$WORK/rpmbuild.log" 2>&1; then
    dest="$OUTPUT/OpenLess-Linux-$VERSION-x86_64.rpm"
    while IFS= read -r built; do
      [ "$built" = "$dest" ] || mv -f "$built" "$dest"
    done < <(find "$WORK/rpmout" -name '*.rpm')
  else
    echo "rpmbuild 失败，日志：$WORK/rpmbuild.log（deb 已产出）" >&2
  fi
else
  echo "跳过 rpm：没有 rpmbuild" >&2
fi

(cd "$OUTPUT" && sha256sum ./*.deb ./*.rpm > SHA256SUMS 2>/dev/null || sha256sum ./*.deb > SHA256SUMS)
echo "done: $OUTPUT"
ls -la "$OUTPUT"/*.deb
