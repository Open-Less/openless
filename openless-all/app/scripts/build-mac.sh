#!/usr/bin/env bash
# 一键构建 macOS 正式版 .app / .dmg。
#
# macOS 的 NSXxxUsageDescription 放在 src-tauri/Info.plist，
# 由 Tauri 在生成 .app 和 .dmg 前合入，避免上传的 DMG 仍是旧 Info.plist。
#
# 用法：在 app/ 目录下执行
#     ./scripts/build-mac.sh           # 构建 + 签名 + 装到 /Applications
#     INSTALL=0 ./scripts/build-mac.sh # 只构建，不装

set -euo pipefail

cd "$(dirname "$0")/.."

APP="src-tauri/target/release/bundle/macos/OpenLess.app"
INFO="$APP/Contents/Info.plist"
DMG_DIR="src-tauri/target/release/bundle/dmg"
INSTALL="${INSTALL:-1}"

if [ -z "${APPLE_CERTIFICATE:-}" ] && [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  export APPLE_SIGNING_IDENTITY="-"
  echo "▶ 未检测到 Apple 签名证书，使用 ad-hoc 签名（下载分发仍会触发 Gatekeeper）"
else
  echo "▶ 检测到 Apple 签名环境，交给 Tauri 做 Developer ID 签名 / 公证"
fi

echo "▶ tauri build"
npm run tauri build

echo "▶ 校验 Info.plist / 签名"
/usr/libexec/PlistBuddy -c "Print :NSMicrophoneUsageDescription" "$INFO" >/dev/null
codesign -d --entitlements :- "$APP" 2>/dev/null | grep -q "com.apple.security.device.audio-input"
codesign --verify --deep --strict --verbose=2 "$APP" 2>&1 | tail -2

echo "▶ 清理发布产物扩展属性"
# 这只能保证 CI/本机构建产物本身干净；浏览器下载仍可能重新加 quarantine。
# 用户免手工 xattr 的根本方案是 Developer ID 签名 + Apple notarization。
xattr -cr "$APP" 2>/dev/null || true
find "$DMG_DIR" -maxdepth 1 -name '*.dmg' -exec xattr -c {} \; 2>/dev/null || true

echo "▶ 校验 quarantine 属性"
if xattr -pr com.apple.quarantine "$APP" >/dev/null 2>&1; then
  echo "✗ $APP 仍包含 com.apple.quarantine"
  exit 1
fi
while IFS= read -r dmg; do
  if xattr -p com.apple.quarantine "$dmg" >/dev/null 2>&1; then
    echo "✗ $dmg 仍包含 com.apple.quarantine"
    exit 1
  fi
done < <(find "$DMG_DIR" -maxdepth 1 -name '*.dmg' -print)

if [ "$INSTALL" = "1" ]; then
  echo "▶ 装到 /Applications"
  pkill -f "OpenLess.app/Contents/MacOS/openless" 2>/dev/null || true
  sleep 1
  # 每次重装前重置 TCC：ad-hoc 签名 hash 每次构建都会变，旧授权立即失效，
  # 不重置就会出现"系统设置里看着已勾选实际不生效"。
  tccutil reset Accessibility com.openless.app 2>/dev/null || true
  tccutil reset Microphone com.openless.app 2>/dev/null || true
  rm -rf /Applications/OpenLess.app
  cp -R "$APP" /Applications/
  xattr -dr com.apple.quarantine /Applications/OpenLess.app 2>/dev/null || true
  echo "✓ 装好了：/Applications/OpenLess.app"
  echo "  打开方式：open /Applications/OpenLess.app"
fi
