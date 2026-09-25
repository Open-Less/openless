#!/usr/bin/env bash
set -euo pipefail

APP_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=${OPENLESS_LINUX_VERSION:?OPENLESS_LINUX_VERSION is required}
ARCH=${OPENLESS_LINUX_ARCH:-x86_64}
TARGET_DIR=${CARGO_TARGET_DIR:-"$APP_ROOT/target"}
BINARY="$TARGET_DIR/release/openless-linux-egui"
PLUGIN_ROOT="$APP_ROOT/../scripts/linux-fcitx5-plugin/build"
PACKAGING="$APP_ROOT/linux-egui/packaging"
OUTPUT="$TARGET_DIR/linux-egui-packages"
# 图标：与 Tauri 侧共用同一套画（`icon.png` 与 `public/AppIcon.png` 的 md5 相同，
# 都是 512×512）。但本脚本必须保持 Tauri-free —— 两条契约都会检查脚本里不得出现
# Tauri 源码树的路径名 —— 所以这里放字节相同的副本，并由 release 契约做逐字节
# 一致性断言，保证两边共用一套素材且不会静默漂移。
ICON_DIR="$PACKAGING/icons"
# 图标名 → hicolor 尺寸目录 → 源文件（与 Tauri 侧的图标集逐字节一致）。
HICOLOR_ICONS=(
  "32x32:32x32.png"
  "64x64:64x64.png"
  "128x128:128x128.png"
  "256x256:128x128@2x.png"
  "512x512:icon.png"
)

test -x "$BINARY"
test -d "$ICON_DIR"
for spec in "${HICOLOR_ICONS[@]}"; do test -s "$ICON_DIR/${spec#*:}"; done
test -s "$PLUGIN_ROOT/libopenless.so"
test -s "$PLUGIN_ROOT/openless.conf"
test -s "$PACKAGING/openless.desktop"
test -s "$PACKAGING/top.openless.OpenLess.metainfo.xml"
command -v dpkg-deb >/dev/null
command -v rpmbuild >/dev/null

mkdir -p "$OUTPUT"

POST_INSTALL="$TARGET_DIR/openless-fcitx5-postinst"
cat > "$POST_INSTALL" <<'EOF'
#!/usr/bin/env bash
set +e
# Package installation runs as root, while fcitx5 belongs to the logged-in
# desktop user. Reconnect only to existing user DBus sessions; never start a
# daemon or fail the package transaction when no graphical session is active.
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

# 卸载同样要重启 fcitx5：文件被删掉后，运行中的输入法仍持有旧插件的映像。
# 用 D-Bus 的 controller Restart（立即返回），**不要**用 `fcitx5 -r`：它会替换 daemon
# 并一直前台运行，导致 postinst/postrm 每次都要等满 timeout（安装卡顿），还会被 kill 掉新 daemon。
POST_REMOVE="$TARGET_DIR/openless-fcitx5-postrm"
sed 's/Package installation runs as root/Package removal runs as root/' \
  "$POST_INSTALL" > "$POST_REMOVE"
chmod 0755 "$POST_REMOVE"

# 插件指纹清单：装完包后一条命令就能核对「系统里的插件 == 包里的插件」。
PLUGIN_SHA=$(sha256sum "$PLUGIN_ROOT/libopenless.so" | awk '{print $1}')
PLUGIN_MANIFEST="$TARGET_DIR/openless-fcitx5-manifest"
cat > "$PLUGIN_MANIFEST" <<EOF
openless $VERSION fcitx5-addon-sha256 $PLUGIN_SHA
EOF

stage_common() {
  local root=$1
  install -Dm755 "$BINARY" "$root/usr/bin/openless"
  install -Dm644 "$PACKAGING/openless.desktop" \
    "$root/usr/share/applications/openless.desktop"
  install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" \
    "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  # 多档 hicolor 尺寸：桌面环境按需选档（任务栏 32/48、菜单 64/128、大图标 256/512）。
  # 旧的单档安装把 512×512 的图放进了 256x256 目录（那档根本不是 256px）。
  for spec in "${HICOLOR_ICONS[@]}"; do
    install -Dm644 "$ICON_DIR/${spec#*:}" \
      "$root/usr/share/icons/hicolor/${spec%%:*}/apps/openless.png"
  done
}

DEB_ROOT="$TARGET_DIR/linux-egui-deb-root"
rm -rf "$DEB_ROOT"
stage_common "$DEB_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$DEB_ROOT/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$DEB_ROOT/usr/share/fcitx5/addon/openless.conf"
install -d "$DEB_ROOT/DEBIAN"
cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: openless
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenLess Contributors
Description: OpenLess Linux egui host
Depends: fcitx5, fcitx5-module-dbus, libasound2, libbz2-1.0, libc6, libdbus-1-3, libegl1, libfcitx5config6, libfcitx5core7, libfcitx5utils2, libffi8, libgcc-s1, liblzma5, libpipewire-0.3-0, libpulse0, libstdc++6, libsystemd0, libuuid1, libvulkan1, libwayland-client0, libwayland-egl1, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0
Recommends: mesa-vulkan-drivers
Homepage: https://github.com/Open-Less/openless
EOF
install -m755 "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postinst"
install -m755 "$POST_REMOVE" "$DEB_ROOT/DEBIAN/postrm"
install -Dm644 "$PLUGIN_MANIFEST" \
  "$DEB_ROOT/usr/share/openless/fcitx5-addon.sha256"
dpkg-deb --build --root-owner-group "$DEB_ROOT" \
  "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.deb"

RPM_ROOT="$TARGET_DIR/linux-egui-rpm-root"
rm -rf "$RPM_ROOT"
stage_common "$RPM_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$RPM_ROOT/usr/share/fcitx5/addon/openless.conf"
install -Dm644 "$PLUGIN_MANIFEST" \
  "$RPM_ROOT/usr/share/openless/fcitx5-addon.sha256"
RPM_TOP="$TARGET_DIR/rpmbuild"
RPM_VERSION=${VERSION,,}
RPM_VERSION=${RPM_VERSION//-/.}
rm -rf "$RPM_TOP"
mkdir -p "$RPM_TOP"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS,rpmdb,tmp}
tar -C "$RPM_ROOT" --transform="s,^\./,openless-$RPM_VERSION/," \
  -czf "$RPM_TOP/SOURCES/openless-$RPM_VERSION.tar.gz" .
cat > "$RPM_TOP/SPECS/openless.spec" <<EOF
Name: openless
Version: $RPM_VERSION
Release: 1
Summary: OpenLess Linux egui host
License: AGPL-3.0-only
URL: https://github.com/Open-Less/openless
BuildArch: x86_64
Source0: openless-$RPM_VERSION.tar.gz
Requires: fcitx5, dbus-libs, alsa-lib, pipewire-libs, pulseaudio-libs, libX11, libxcb, libwayland-client, libxkbcommon, libglvnd-egl, vulkan-loader, libXi.so.6()(64bit), libXcursor.so.1()(64bit), libX11-xcb.so.1()(64bit), libxkbcommon-x11.so.0()(64bit), libwayland-egl.so.1()(64bit)
Recommends: mesa-vulkan-drivers
%description
OpenLess Linux egui host.
%prep
%setup -q -n openless-$RPM_VERSION
%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
%files
/
%postun
set +e
for bus in /run/user/[0-9]*/bus; do
  [ -S "\$bus" ] || continue
  runtime_dir=\${bus%/bus}; uid=\${runtime_dir##*/}
  [ "\$uid" != 0 ] || continue
  user=\$(getent passwd "\$uid" | cut -d: -f1)
  [ -n "\$user" ] && timeout 5s runuser -u "\$user" -- env XDG_RUNTIME_DIR="\$runtime_dir" DBUS_SESSION_BUS_ADDRESS="unix:path=\$bus" dbus-send --session --dest=org.fcitx.Fcitx5 --type=method_call /controller org.fcitx.Fcitx.Controller1.Restart >/dev/null 2>&1 || true
done
exit 0
%post
set +e
for bus in /run/user/[0-9]*/bus; do
  [ -S "\$bus" ] || continue
  runtime_dir=\${bus%/bus}; uid=\${runtime_dir##*/}
  [ "\$uid" != 0 ] || continue
  user=\$(getent passwd "\$uid" | cut -d: -f1)
  [ -n "\$user" ] && timeout 5s runuser -u "\$user" -- env XDG_RUNTIME_DIR="\$runtime_dir" DBUS_SESSION_BUS_ADDRESS="unix:path=\$bus" dbus-send --session --dest=org.fcitx.Fcitx5 --type=method_call /controller org.fcitx.Fcitx.Controller1.Restart >/dev/null 2>&1 || true
done
exit 0
EOF
rpmbuild --define "_topdir $RPM_TOP" --define "_dbpath $RPM_TOP/rpmdb" \
  --define "_tmppath $RPM_TOP/tmp" -bb "$RPM_TOP/SPECS/openless.spec"
mv "$RPM_TOP/RPMS/x86_64/openless-$RPM_VERSION-1.x86_64.rpm" \
  "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.rpm"

find "$OUTPUT" -maxdepth 1 -type f -printf '%f\n' | sort
