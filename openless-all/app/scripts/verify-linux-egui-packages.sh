#!/usr/bin/env bash
# Validate exactly what the tag workflow uploads. No GUI session or AppImage required.
set -euo pipefail
APP_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TARGET_DIR=${CARGO_TARGET_DIR:-"$APP_ROOT/target"}
OUTPUT="$TARGET_DIR/linux-egui-packages"
shopt -s nullglob
debs=("$OUTPUT"/*.deb)
rpms=("$OUTPUT"/*.rpm)
appimages=("$OUTPUT"/*.AppImage)
test "${#debs[@]}" -eq 1
test "${#rpms[@]}" -eq 1
test "${#appimages[@]}" -eq 0
test -s "$OUTPUT/SHA256SUMS"
test "$(wc -l < "$OUTPUT/SHA256SUMS")" -eq 2
test "$(find "$OUTPUT" -maxdepth 1 \( -type f -o -type l \) | wc -l)" -eq 3
(
  cd "$OUTPUT"
  sha256sum --check --strict SHA256SUMS
)

check_elf() {
  local binary=$1 deps
  file -L "$binary" | grep -q 'ELF 64-bit.*x86-64'
  deps=$(ldd "$binary")
  if grep -q 'not found' <<< "$deps"; then printf '%s\n' "$deps"; return 1; fi
  if grep -Eqi 'webkit|wry|tauri' <<< "$deps"; then printf '%s\n' "$deps"; return 1; fi
}
check_elf "$TARGET_DIR/release/openless-linux-egui"

WORK=$(mktemp -d)
trap 'rm -rf -- "$WORK"' EXIT
dpkg-deb --extract "${debs[0]}" "$WORK/deb"
test -x "$WORK/deb/usr/bin/openless"
test -s "$WORK/deb/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
test -s "$WORK/deb/usr/share/fcitx5/addon/openless.conf"
test -s "$WORK/deb/usr/share/openless/fcitx5-addon.sha256"
DEB_PLUGIN_SHA=$(sha256sum "$WORK/deb/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so" | cut -d' ' -f1)
grep -Eq "^openless [^ ]+ fcitx5-addon-sha256 $DEB_PLUGIN_SHA$" \
  "$WORK/deb/usr/share/openless/fcitx5-addon.sha256"
for size in 32x32 64x64 128x128 256x256 512x512; do
  test -s "$WORK/deb/usr/share/icons/hicolor/$size/apps/openless.png"
done
desktop-file-validate "$WORK/deb/usr/share/applications/openless.desktop"
appstreamcli validate --no-net "$WORK/deb/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
check_elf "$WORK/deb/usr/bin/openless"
check_elf "$WORK/deb/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
# Catch stale build-script output: dpkg metadata may say -N while the UI
# still displays an older revision embedded in the binary.
DEB_VERSION=$(dpkg-deb --field "${debs[0]}" Version)
test "$("$WORK/deb/usr/bin/openless" --version)" = "OpenLess $DEB_VERSION"
DEB_FILES=$(dpkg-deb --contents "${debs[0]}")
grep -Fq 'usr/bin/openless' <<< "$DEB_FILES"
grep -Fq 'fcitx5/libopenless.so' <<< "$DEB_FILES"
DEB_DEPENDS=$(dpkg-deb --field "${debs[0]}" Depends)
grep -Fq 'fcitx5' <<< "$DEB_DEPENDS"
grep -Fq 'libpipewire-0.3-0' <<< "$DEB_DEPENDS"

mkdir -p "$WORK/rpm"
( cd "$WORK/rpm" && rpm2cpio "${rpms[0]}" | cpio -idm --quiet )
test -x "$WORK/rpm/usr/bin/openless"
test -s "$WORK/rpm/usr/lib64/fcitx5/libopenless.so"
test -s "$WORK/rpm/usr/share/fcitx5/addon/openless.conf"
test -s "$WORK/rpm/usr/share/openless/fcitx5-addon.sha256"
RPM_PLUGIN_SHA=$(sha256sum "$WORK/rpm/usr/lib64/fcitx5/libopenless.so" | cut -d' ' -f1)
grep -Eq "^openless [^ ]+ fcitx5-addon-sha256 $RPM_PLUGIN_SHA$" \
  "$WORK/rpm/usr/share/openless/fcitx5-addon.sha256"
test "$DEB_PLUGIN_SHA" = "$RPM_PLUGIN_SHA"
desktop-file-validate "$WORK/rpm/usr/share/applications/openless.desktop"
appstreamcli validate --no-net "$WORK/rpm/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
check_elf "$WORK/rpm/usr/bin/openless"
check_elf "$WORK/rpm/usr/lib64/fcitx5/libopenless.so"
test "$("$WORK/rpm/usr/bin/openless" --version)" = "OpenLess $DEB_VERSION"
RPM_FILES=$(rpm -qlp "${rpms[0]}")
grep -Fxq '/usr/bin/openless' <<< "$RPM_FILES"
grep -Fxq '/usr/lib64/fcitx5/libopenless.so' <<< "$RPM_FILES"
grep -Fxq '/usr/share/openless/fcitx5-addon.sha256' <<< "$RPM_FILES"
RPM_DEPENDS=$(rpm -qp --requires "${rpms[0]}")
grep -Fq 'fcitx5' <<< "$RPM_DEPENDS"
grep -Fq 'pipewire-libs' <<< "$RPM_DEPENDS"
printf 'PASS: deb/rpm contents, ELF dependencies, desktop metadata and SHA-256\n'
