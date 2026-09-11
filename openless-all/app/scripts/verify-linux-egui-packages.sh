#!/usr/bin/env bash
# Package/backend verification only: never starts an egui or desktop UI.
set -euo pipefail
APP_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TARGET_DIR=$(realpath -m "${CARGO_TARGET_DIR:-"$APP_ROOT/target"}")
case "$TARGET_DIR/" in "$APP_ROOT/target/"*) ;; *) echo 'Verification must use app/target' >&2; exit 1;; esac
OUTPUT="$TARGET_DIR/linux-egui-packages"
shopt -s nullglob
debs=("$OUTPUT"/*.deb); rpms=("$OUTPUT"/*.rpm); appimages=("$OUTPUT"/*.AppImage); components=("$OUTPUT"/*.tar.gz)
test "${#debs[@]}" -eq 1
test "${#rpms[@]}" -eq 1
test "${#appimages[@]}" -eq 1
test "${#components[@]}" -eq 1
(cd "$OUTPUT"; sha256sum --check --strict SHA256SUMS)

check_elf() {
  local binary=$1 dependencies
  file "$binary" | grep 'ELF 64-bit.*x86-64' >/dev/null
  dependencies=$(ldd "$binary")
  if grep -q 'not found' <<<"$dependencies"; then printf '%s\n' "$dependencies"; return 1; fi
  if grep -Eqi 'webkit|wry|tauri' <<<"$dependencies"; then printf '%s\n' "$dependencies"; return 1; fi
}
check_elf "$OUTPUT/openless-linux-egui"
WORK=$(mktemp -d "$TARGET_DIR/linux-package-check.XXXXXX")
cleanup() { case "$WORK" in "$TARGET_DIR"/linux-package-check.*) rm -rf -- "$WORK";; esac; }
trap cleanup EXIT
dpkg-deb --contents "${debs[0]}" > "$WORK/deb-contents.txt"
rpm -qlp "${rpms[0]}" > "$WORK/rpm-contents.txt"
for item in usr/bin/openless usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so usr/lib/openless/resources/qwen-asr/qwen_asr usr/lib/openless/resources/linux-desktop/openless-desktop-bridge; do
  grep -F "$item" "$WORK/deb-contents.txt" >/dev/null
done
for item in /usr/bin/openless /usr/lib64/fcitx5/libopenless.so /usr/lib/openless/resources/qwen-asr/qwen_asr /usr/lib/openless/resources/linux-desktop/openless-desktop-bridge; do
  grep -Fx "$item" "$WORK/rpm-contents.txt" >/dev/null
done
dpkg-deb --extract "${debs[0]}" "$WORK/deb"
desktop-file-validate "$WORK/deb/usr/share/applications/openless.desktop"
appstreamcli validate --no-net "$WORK/deb/usr/share/metainfo/top.openless.OpenLess.metainfo.xml"
check_elf "$WORK/deb/usr/bin/openless"
check_elf "$WORK/deb/usr/lib/openless/resources/linux-desktop/openless-desktop-bridge"
check_elf "$WORK/deb/usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so"
(
  cd "$WORK"
  "${appimages[0]}" --appimage-extract > extract.log
)
ROOT="$WORK/squashfs-root"
RESOURCES="$ROOT/usr/lib/openless/resources"
check_elf "$ROOT/usr/bin/openless"
check_elf "$RESOURCES/qwen-asr/qwen_asr"
check_elf "$RESOURCES/linux-desktop/openless-desktop-bridge"
check_elf "$RESOURCES/linux-desktop/plugins/platforms/libqoffscreen.so"
test -s "$RESOURCES/linux-fcitx5-plugin/libopenless.so"
test -s "$RESOURCES/linux-fcitx5-plugin/openless.conf"
test -s "$RESOURCES/fonts/NotoSansCJK-Regular.ttc"
test -s "$RESOURCES/linux-desktop/gnome/modern/extension.js"
test -s "$RESOURCES/linux-desktop/gnome/legacy/extension.js"
test -s "$RESOURCES/linux-desktop/kwin/contents/code/main.js"
test -s "$RESOURCES/linux-desktop/licenses/libqt5core5a-copyright"
"$RESOURCES/qwen-asr/qwen_asr" --help >/dev/null 2>&1
tar -tzf "${components[0]}" > "$WORK/components.txt"
grep -Fx linux-desktop/install.sh "$WORK/components.txt" >/dev/null
grep -Fx linux-desktop/licenses/libqt5core5a-copyright "$WORK/components.txt" >/dev/null
printf 'PASS: ELF, metadata, SHA-256, deb/rpm/AppImage contents and desktop components\n'
