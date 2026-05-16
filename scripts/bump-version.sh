#!/usr/bin/env bash
# 同步更新 OpenLess 四處版本號。
# 用法：
#     ./scripts/bump-version.sh 1.2.21
#
# 改的位置（CLAUDE.md 強調必須同時改，否則 release-tauri.yml 失敗）：
#   - openless-all/app/package.json                "version": "X.Y.Z"
#   - openless-all/app/package-lock.json           根包 version + 巢狀引用
#   - openless-all/app/src-tauri/tauri.conf.json   "version": "X.Y.Z"
#   - openless-all/app/src-tauri/Cargo.toml        version = "X.Y.Z" (頂層)
#   - openless-all/app/src-tauri/Cargo.lock        透過 cargo update -p openless 同步
#
# CI 的 cross-platform 任務最後一步會校驗四個檔案版本號一致；漏改一處直接 fail。

set -euo pipefail

if [ "${1:-}" = "" ]; then
  echo "用法: $0 <new-version>" >&2
  echo "例:   $0 1.2.21" >&2
  exit 1
fi

NEW="$1"

if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "錯誤：版本號必須是 X.Y.Z 數字格式 (拿到 '$NEW')" >&2
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
    echo "錯誤：找不到 $f" >&2
    exit 1
  fi
done

# package.json + package-lock.json：npm version 一行同步兩個，且不打 git tag。
# --allow-same-version 讓指令碼可重複執行（實際 release flow 不會，但 dry-run 友好）。
echo "▶ 升 package.json + package-lock.json → $NEW"
( cd "$APP" && npm version "$NEW" --no-git-tag-version --allow-same-version > /dev/null )

# tauri.conf.json：BSD sed 與 GNU sed 都支援 -E + -i.bak 字尾；不用行號範圍地址。
echo "▶ 升 tauri.conf.json → $NEW"
sed -E -i.bak \
  "s/\"version\":[[:space:]]*\"[0-9]+\.[0-9]+\.[0-9]+\"/\"version\": \"$NEW\"/" \
  "$TAURI_CONF"
rm "$TAURI_CONF.bak"

# Cargo.toml：用 awk 替換檔案裡第一個 version = "X.Y.Z" 行（頂層 [package].version）。
# 不用 GNU sed 的 `0,/.../` 行號範圍地址（macOS BSD sed 不支援）。
echo "▶ 升 Cargo.toml → $NEW"
awk -v new="$NEW" '
  !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+"$/ {
    sub(/"[0-9]+\.[0-9]+\.[0-9]+"/, "\"" new "\"")
    done = 1
  }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp"
mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# Cargo.lock：cargo update 顯式同步 openless package；失敗要立刻退出，不能吞錯。
echo "▶ 同步 Cargo.lock"
( cd "$APP/src-tauri" && cargo update -p openless 2>&1 | tail -5 )

# 校驗五處一致（package.json / package-lock.json / tauri.conf.json / Cargo.toml / Cargo.lock）
echo
echo "===== 驗證版本一致性 ====="
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
  echo "::error::版本號未對齊 — 請檢查指令碼輸出" >&2
  exit 1
fi

echo
echo "✓ 全部一致：$NEW"
echo
echo "下一步建議："
echo "  git add $PKG_JSON $PKG_LOCK $TAURI_CONF $CARGO_TOML $CARGO_LOCK"
echo "  git commit -m 'chore(release): $NEW'"
echo "  git push"
echo "  git tag v$NEW-tauri && git push origin v$NEW-tauri"
