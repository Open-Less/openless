#!/usr/bin/env bash
set -euo pipefail
MODE=${1:-install}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DATA=${XDG_DATA_HOME:-"$HOME/.local/share"}
CONFIG=${XDG_CONFIG_HOME:-"$HOME/.config"}
BIN="$HOME/.local/bin"
UUID=openless@openless.app
GNOME_DIR="$DATA/gnome-shell/extensions/$UUID"
KWIN_DIR="$DATA/kwin/scripts/openless-desktop"
HELPER="$BIN/openless-desktop-bridge"
RUNTIME="$HOME/.local/lib/openless-desktop-bridge"
case "$MODE" in install|enable|uninstall) ;; *) echo 'Usage: install.sh [install|enable|uninstall]' >&2; exit 2;; esac

is_gnome=false
desktop=${XDG_CURRENT_DESKTOP:-}
case "${desktop,,}" in *gnome*|*ubuntu*) is_gnome=true;; esac

if [ "$MODE" = uninstall ]; then
  if command -v gnome-extensions >/dev/null; then gnome-extensions disable "$UUID" || true; fi
  for tool in kwriteconfig6 kwriteconfig5; do
    if command -v "$tool" >/dev/null; then "$tool" --file kwinrc --group Plugins --key openless-desktopEnabled false; break; fi
  done
  # Only remove these package-owned directories and files under the user's
  # configured roots. Never enumerate arbitrary home files or use shell globs.
  test "$GNOME_DIR" = "$DATA/gnome-shell/extensions/openless@openless.app"
  test "$KWIN_DIR" = "$DATA/kwin/scripts/openless-desktop"
  rm -rf -- "$GNOME_DIR" "$KWIN_DIR" "$RUNTIME"
  rm -f -- "$HELPER" "$DATA/dbus-1/services/org.openless.Desktop1.service" "$CONFIG/autostart/openless-desktop-bridge.desktop"
  echo 'Desktop integration removed. Log out and back in to unload the running component.'
  exit 0
fi

if [ "$MODE" = install ]; then
  if $is_gnome; then
    version=$(gnome-shell --version | sed -n 's/.* \([0-9][0-9]*\).*/\1/p')
    test -n "$version"
    variant=modern; if [ "$version" -lt 45 ]; then variant=legacy; fi
    install -Dm644 "$ROOT/gnome/$variant/metadata.json" "$GNOME_DIR/metadata.json"
    install -Dm644 "$ROOT/gnome/$variant/extension.js" "$GNOME_DIR/extension.js"
  else
    install -Dm644 "$ROOT/kwin/metadata.json" "$KWIN_DIR/metadata.json"
    install -Dm644 "$ROOT/kwin/metadata.desktop" "$KWIN_DIR/metadata.desktop"
    install -Dm644 "$ROOT/kwin/contents/code/main.js" "$KWIN_DIR/contents/code/main.js"
    install -Dm755 "$ROOT/openless-desktop-bridge" "$RUNTIME/openless-desktop-bridge"
    if [ -d "$ROOT/lib" ]; then mkdir -p "$RUNTIME/lib"; cp -a "$ROOT/lib/." "$RUNTIME/lib/"; fi
    if [ -d "$ROOT/plugins" ]; then mkdir -p "$RUNTIME/plugins"; cp -a "$ROOT/plugins/." "$RUNTIME/plugins/"; fi
    if [ -d "$ROOT/licenses" ]; then mkdir -p "$RUNTIME/licenses"; cp -a "$ROOT/licenses/." "$RUNTIME/licenses/"; fi
    mkdir -p "$BIN"
    # Keep the helper and its Qt runtime after the AppImage mount disappears.
    printf '#!/usr/bin/env bash\nexec %q "$@"\n' "$RUNTIME/openless-desktop-bridge" > "$HELPER"
    chmod +x "$HELPER"
    mkdir -p "$DATA/dbus-1/services" "$CONFIG/autostart"
    escaped=${HELPER//\\/\\\\}; escaped=${escaped//\"/\\\"}
    printf '[D-BUS Service]\nName=org.openless.Desktop1\nExec="%s"\n' "$escaped" > "$DATA/dbus-1/services/org.openless.Desktop1.service"
    printf '[Desktop Entry]\nType=Application\nName=OpenLess Desktop Bridge\nExec="%s"\nOnlyShowIn=KDE;\nNoDisplay=true\n' "$escaped" > "$CONFIG/autostart/openless-desktop-bridge.desktop"
  fi
fi

if $is_gnome; then
  if ! gnome-extensions enable "$UUID"; then
    echo 'The extension is installed. Log out and back in, then run this command with enable.' >&2
    exit 1
  fi
else
  enabled=false
  for tool in kwriteconfig6 kwriteconfig5; do
    if command -v "$tool" >/dev/null; then "$tool" --file kwinrc --group Plugins --key openless-desktopEnabled true; enabled=true; break; fi
  done
  $enabled || { echo 'KDE configuration tools are missing' >&2; exit 1; }
  for tool in qdbus6 qdbus qdbus-qt5; do
    if command -v "$tool" >/dev/null; then "$tool" org.kde.KWin /KWin reconfigure; break; fi
  done
  # The service is started by D-Bus on demand and on the next KDE login.
fi
echo 'OpenLess desktop integration enabled. Restart OpenLess to bind shortcuts.'
