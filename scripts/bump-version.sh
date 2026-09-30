#!/usr/bin/env bash
# Sync-update the OpenLess version in four places.
# Usage:
#     ./scripts/bump-version.sh 1.2.21
#
# Locations to edit (CLAUDE.md stresses they must change together, otherwise
# release-tauri.yml fails):
#   - openless-all/app/package.json                "version": "X.Y.Z"
#   - openless-all/app/package-lock.json           root package version + nested refs
#   - openless-all/app/src-tauri/tauri.conf.json   "version": "X.Y.Z"
#   - openless-all/app/src-tauri/Cargo.toml        version = "X.Y.Z" (top level)
#   - openless-all/app/src-tauri/Cargo.lock        synced via cargo update -p openless
#
# CI's cross-platform job verifies the versions match across the files in its last step;
# missing one fails outright.

set -euo pipefail

if [ "${1:-}" = "" ]; then
  echo "用法: $0 <new-version>" >&2
  echo "例:   $0 1.2.21" >&2
  exit 1
fi

NEW="$1"

if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "错误：版本号必须是 X.Y.Z 数字格式 (拿到 '$NEW')" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$REPO_ROOT/openless-all/app"

PKG_JSON="$APP/package.json"
PKG_LOCK="$APP/package-lock.json"
TAURI_CONF="$APP/src-tauri/tauri.conf.json"
CARGO_TOML="$APP/src-tauri/Cargo.toml"
CARGO_LOCK="$APP/src-tauri/Cargo.lock"

for f in "$PKG_JSON" "$PKG_LOCK" "$TAURI_CONF" "$CARGO_TOML" "$CARGO_LOCK"; do
  if [ ! -f "$f" ]; then
    echo "错误：找不到 $f" >&2
    exit 1
  fi
done

# package.json + package-lock.json: npm version updates both in one line, without a git tag.
# --allow-same-version makes the script re-runnable (the real release flow won't, but it's
# dry-run friendly).
echo "▶ 升 package.json + package-lock.json → $NEW"
( cd "$APP" && npm version "$NEW" --no-git-tag-version --allow-same-version > /dev/null )

# tauri.conf.json: both BSD sed and GNU sed support -E + -i.bak suffix; no line-range address.
echo "▶ 升 tauri.conf.json → $NEW"
sed -E -i.bak \
  "s/\"version\":[[:space:]]*\"[0-9]+\.[0-9]+\.[0-9]+\"/\"version\": \"$NEW\"/" \
  "$TAURI_CONF"
rm "$TAURI_CONF.bak"

# Cargo.toml: use awk to replace the version = "X.Y.Z" line inside the [package] section.
# Must anchor on the [package] section: an earlier bare X.Y.Z version line in the file may
# be a dependency version (e.g. keyring's `version = "3.6.3"`); without the anchor a
# dependency would get the new version by mistake.
# No GNU sed `0,/.../` line-range address (macOS BSD sed doesn't support it).
echo "▶ 升 Cargo.toml → $NEW"
awk -v new="$NEW" '
  /^\[package\]$/ { in_package = 1 }
  in_package && !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+"$/ {
    sub(/"[0-9]+\.[0-9]+\.[0-9]+"/, "\"" new "\"")
    done = 1
  }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp"
mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# Cargo.lock: cargo update syncs the openless package explicitly; exit immediately on
# failure, never swallow errors.
echo "▶ 同步 Cargo.lock"
( cd "$APP/src-tauri" && cargo update -p openless 2>&1 | tail -5 )

# Verify consistency across the five (package.json / package-lock.json / tauri.conf.json / Cargo.toml / Cargo.lock)
echo
echo "===== 验证版本一致性 ====="
PKG=$(node -p "require('$PKG_JSON').version")
LOCK_ROOT=$(node -p "require('$PKG_LOCK').version")
LOCK_NESTED=$(node -p "require('$PKG_LOCK').packages[''].version")
TAU=$(node -p "require('$TAURI_CONF').version")
CRG=$(grep -E '^version = ' "$CARGO_TOML" | head -1 | sed -E 's/^version = "(.+)"$/\1/')
CARGO_LOCK_VER=$(awk '/^name = "openless"$/{getline; if (match($0, /version = "([0-9.]+)"/, a)) {print a[1]; exit}}' "$CARGO_LOCK" 2>/dev/null \
  || awk 'BEGIN{found=0} /^name = "openless"$/{found=1; next} found && /^version = /{gsub(/"/,""); print $3; exit}' "$CARGO_LOCK")

printf '%-22s %s\n' 'package.json:'        "$PKG"
printf '%-22s %s\n' 'package-lock root:'   "$LOCK_ROOT"
printf '%-22s %s\n' 'package-lock nested:' "$LOCK_NESTED"
printf '%-22s %s\n' 'tauri.conf.json:'     "$TAU"
printf '%-22s %s\n' 'Cargo.toml:'          "$CRG"
printf '%-22s %s\n' 'Cargo.lock (openless):' "$CARGO_LOCK_VER"

mismatch=0
for v in "$LOCK_ROOT" "$LOCK_NESTED" "$TAU" "$CRG" "$CARGO_LOCK_VER"; do
  if [ "$v" != "$NEW" ]; then mismatch=1; fi
done

if [ "$mismatch" -ne 0 ] || [ "$PKG" != "$NEW" ]; then
  echo
  echo "::error::版本号未对齐 — 请检查脚本输出" >&2
  exit 1
fi

echo
echo "✓ 全部一致：$NEW"
echo
echo "下一步建议："
echo "  git add $PKG_JSON $PKG_LOCK $TAURI_CONF $CARGO_TOML $CARGO_LOCK"
echo "  git commit -m 'chore(release): $NEW'"
echo "  git push"
echo "  git tag v$NEW-tauri && git push origin v$NEW-tauri"
