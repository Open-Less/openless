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
ICON="$APP_ROOT/public/AppIcon.png"

test -x "$BINARY"
test -s "$PLUGIN_ROOT/libopenless.so"
test -s "$PLUGIN_ROOT/openless.conf"
test -s "$PACKAGING/openless.desktop"
test -s "$PACKAGING/top.openless.OpenLess.metainfo.xml"
test -s "$ICON"
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
    timeout 5s fcitx5 -r >/dev/null 2>&1 || true
done
exit 0
EOF
chmod 0755 "$POST_INSTALL"

stage_common() {
  local root=$1
  install -Dm755 "$BINARY" "$root/usr/bin/openless"
  install -Dm644 "$PACKAGING/openless.desktop" \
    "$root/usr/share/applications/openless.desktop"
  install -Dm644 "$PACKAGING/top.openless.OpenLess.metainfo.xml" \
    "$root/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
  install -Dm644 "$ICON" "$root/usr/share/icons/hicolor/256x256/apps/openless.png"
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
Depends: fcitx5, fcitx5-module-dbus, libdbus-1-3, libasound2, libpipewire-0.3-0, libpulse0
Homepage: https://github.com/Open-Less/openless
EOF
install -m755 "$POST_INSTALL" "$DEB_ROOT/DEBIAN/postinst"
dpkg-deb --build --root-owner-group "$DEB_ROOT" \
  "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.deb"

RPM_ROOT="$TARGET_DIR/linux-egui-rpm-root"
rm -rf "$RPM_ROOT"
stage_common "$RPM_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$RPM_ROOT/usr/share/fcitx5/addon/openless.conf"
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
Requires: fcitx5, dbus-libs, alsa-lib, pipewire-libs, pulseaudio-libs
%description
OpenLess Linux egui host.
%prep
%setup -q -n openless-$RPM_VERSION
%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
%files
/
%post
set +e
for bus in /run/user/[0-9]*/bus; do
  [ -S "\$bus" ] || continue
  runtime_dir=\${bus%/bus}; uid=\${runtime_dir##*/}
  [ "\$uid" != 0 ] || continue
  user=\$(getent passwd "\$uid" | cut -d: -f1)
  [ -n "\$user" ] && timeout 5s runuser -u "\$user" -- env XDG_RUNTIME_DIR="\$runtime_dir" DBUS_SESSION_BUS_ADDRESS="unix:path=\$bus" fcitx5 -r >/dev/null 2>&1 || true
done
exit 0
EOF
rpmbuild --define "_topdir $RPM_TOP" --define "_dbpath $RPM_TOP/rpmdb" \
  --define "_tmppath $RPM_TOP/tmp" -bb "$RPM_TOP/SPECS/openless.spec"
mv "$RPM_TOP/RPMS/x86_64/openless-$RPM_VERSION-1.x86_64.rpm" \
  "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.rpm"

find "$OUTPUT" -maxdepth 1 -type f -printf '%f\n' | sort
