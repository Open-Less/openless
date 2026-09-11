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
command -v fpm >/dev/null

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
    fcitx5 -r >/dev/null 2>&1 || true
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
fpm -s dir -t deb -C "$DEB_ROOT" \
  -n openless -v "$VERSION" -a amd64 \
  --description "OpenLess Linux egui host" \
  --license AGPL-3.0-only \
  --url https://github.com/Open-Less/openless \
  --after-install "$POST_INSTALL" \
  -d fcitx5 -d fcitx5-module-dbus -d libdbus-1-3 -d libasound2 \
  -d libpipewire-0.3-0 -d libpulse0 \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.deb" .

RPM_ROOT="$TARGET_DIR/linux-egui-rpm-root"
rm -rf "$RPM_ROOT"
stage_common "$RPM_ROOT"
install -Dm755 "$PLUGIN_ROOT/libopenless.so" \
  "$RPM_ROOT/usr/lib64/fcitx5/libopenless.so"
install -Dm644 "$PLUGIN_ROOT/openless.conf" \
  "$RPM_ROOT/usr/share/fcitx5/addon/openless.conf"
fpm -s dir -t rpm -C "$RPM_ROOT" \
  -n openless -v "$VERSION" -a x86_64 \
  --description "OpenLess Linux egui host" \
  --license AGPL-3.0-only \
  --url https://github.com/Open-Less/openless \
  --after-install "$POST_INSTALL" \
  -d fcitx5 -d dbus-libs -d alsa-lib \
  -d pipewire-libs -d pulseaudio-libs \
  -p "$OUTPUT/OpenLess-Linux-egui-${VERSION}-${ARCH}.rpm" .

find "$OUTPUT" -maxdepth 1 -type f -printf '%f\n' | sort
