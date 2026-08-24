#!/usr/bin/env bash
# 同步更新 OpenLess 五处版本号。
# 用法：
#     ./scripts/bump-version.sh 1.2.21
#     ./scripts/bump-version.sh 1.4.0-Beta.1
#
# 改的位置（CLAUDE.md 强调必须同时改，否则 release-tauri.yml 失败）：
#   - openless-all/app/package.json                "version": "X.Y.Z"
#   - openless-all/app/package-lock.json           根包 version + 嵌套引用
#   - openless-all/app/src-tauri/tauri.conf.json   "version": "X.Y.Z"
#   - openless-all/app/src-tauri/Cargo.toml        version = "X.Y.Z" (顶层)
#   - openless-all/app/src-tauri/Cargo.lock        通过 cargo update -p openless 同步
#
# CI 的 cross-platform 任务最后一步会校验五处版本号一致；漏改一处直接 fail。

set -euo pipefail

if [ "${1:-}" = "" ]; then
  echo "用法: $0 <new-version>" >&2
  echo "例:   $0 1.2.21 或 $0 1.4.0-Beta.1" >&2
  exit 1
fi

NEW="$1"

if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$ ]]; then
  echo "错误：版本号必须是 X.Y.Z 或 X.Y.Z-prerelease 格式 (拿到 '$NEW')" >&2
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

# package.json + package-lock.json：npm version 一行同步两个，且不打 git tag。
# --allow-same-version 让脚本可重复运行（实际 release flow 不会，但 dry-run 友好）。
echo "▶ 升 package.json + package-lock.json → $NEW"
( cd "$APP" && npm version "$NEW" --no-git-tag-version --allow-same-version > /dev/null )

# tauri.conf.json：BSD sed 与 GNU sed 都支持 -E + -i.bak 后缀；不用行号范围地址。
echo "▶ 升 tauri.conf.json → $NEW"
sed -E -i.bak \
  "s/\"version\":[[:space:]]*\"[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?\"/\"version\": \"$NEW\"/" \
  "$TAURI_CONF"
rm "$TAURI_CONF.bak"

# Cargo.toml：用 awk 替换 [package] 段里的 version = "X.Y.Z" 行。
# 必须锚定 [package] 段：文件里先出现的纯 X.Y.Z 版本行可能是依赖版本
# （如 keyring 的 `version = "3.6.3"`），不带锚定会把依赖版本误改成新版本号。
# 不用 GNU sed 的 `0,/.../` 行号范围地址（macOS BSD sed 不支持）。
echo "▶ 升 Cargo.toml → $NEW"
awk -v new="$NEW" '
  /^\[package\]$/ { in_package = 1 }
  in_package && !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?"$/ {
    sub(/"[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?"/, "\"" new "\"")
    done = 1
  }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp"
mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# Cargo.lock：cargo update 显式同步 openless package；失败要立刻退出，不能吞错。
echo "▶ 同步 Cargo.lock"
( cd "$APP/src-tauri" && cargo update -p openless 2>&1 | tail -5 )

# 校验五处一致（package.json / package-lock.json / tauri.conf.json / Cargo.toml / Cargo.lock）
echo
echo "===== 验证版本一致性 ====="
PKG=$(node -p "require('$PKG_JSON').version")
LOCK_ROOT=$(node -p "require('$PKG_LOCK').version")
LOCK_NESTED=$(node -p "require('$PKG_LOCK').packages[''].version")
TAU=$(node -p "require('$TAURI_CONF').version")
CRG=$(grep -E '^version = ' "$CARGO_TOML" | head -1 | sed -E 's/^version = "(.+)"$/\1/')
CARGO_LOCK_VER=$(awk 'BEGIN{found=0} /^name = "openless"$/{found=1; next} found && /^version = /{gsub(/"/,""); print $3; exit}' "$CARGO_LOCK")

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
