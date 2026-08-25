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

echo "▶ 检查 Apple Silicon MLX 构建依赖"
npm run check:macos-metal-toolchain

# Homebrew rustc 在 macOS 上对 `strip=symbols` 生成的 proc-macro dylib
# 可能报 "mis-aligned LINKEDIT string pool"。仅官方 macOS 发布脚本降级
# 为 debuginfo；Cargo.toml 的全局 profile 仍让 Linux/Windows/Android 使用 symbols。
export CARGO_PROFILE_RELEASE_STRIP=debuginfo
export RUSTC_WRAPPER="$PWD/scripts/rustc-macos-proc-macro-wrapper.sh"
echo "▶ Cargo release strip: ${CARGO_PROFILE_RELEASE_STRIP} (macOS only)"
echo "▶ Rust proc-macro host wrapper: ${RUSTC_WRAPPER}"

echo "▶ tauri build"
TAURI_BUILD_ARGS=(build --ci)
case "$(uname -m)" in
  arm64)
    MAC_BUNDLE_ARCH="aarch64"
    TAURI_BUILD_ARGS+=(--config src-tauri/tauri.macos-mlx.conf.json)
    ;;
  x86_64)
    MAC_BUNDLE_ARCH="x64"
    ;;
  *)
    echo "✗ 不支持的 macOS 构建架构：$(uname -m)"
    exit 1
    ;;
esac
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then
  TAURI_BUILD_ARGS+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
fi
npm run tauri -- "${TAURI_BUILD_ARGS[@]}"

APP_VERSION="$(node -p "require('./package.json').version")"
DMG_PATH="$DMG_DIR/OpenLess_${APP_VERSION}_${MAC_BUNDLE_ARCH}.dmg"

echo "▶ 校验 Info.plist / 签名"
/usr/libexec/PlistBuddy -c "Print :NSMicrophoneUsageDescription" "$INFO" >/dev/null
bash scripts/check-macos-speech-usage-description.sh "$INFO"
codesign -d --entitlements :- "$APP" 2>/dev/null | grep -q "com.apple.security.device.audio-input"
codesign --verify --deep --strict --verbose=2 "$APP" 2>&1 | tail -2

if [ "$MAC_BUNDLE_ARCH" = "aarch64" ]; then
  echo "▶ 校验 MLX metallib 已进入 app / DMG / updater"
  APP_METALLIB="$APP/Contents/MacOS/mlx.metallib"
  if [ ! -s "$APP_METALLIB" ]; then
    echo "✗ Apple Silicon app 缺少 Contents/MacOS/mlx.metallib"
    exit 1
  fi
  APP_METALLIB_SHA="$(shasum -a 256 "$APP_METALLIB" | awk '{print $1}')"

  if [ ! -f "$DMG_PATH" ]; then
    echo "✗ 未找到 Tauri 生成的 DMG：$DMG_PATH"
    exit 1
  fi
  DMG_MOUNT="$(mktemp -d "${TMPDIR:-/tmp}/openless-dmg-verify.XXXXXX")"
  cleanup_dmg_mount() {
    hdiutil detach "$DMG_MOUNT" >/dev/null 2>&1 || true
    rmdir "$DMG_MOUNT" >/dev/null 2>&1 || true
  }
  trap cleanup_dmg_mount EXIT
  hdiutil attach "$DMG_PATH" -readonly -nobrowse -mountpoint "$DMG_MOUNT" >/dev/null
  DMG_METALLIB="$DMG_MOUNT/OpenLess.app/Contents/MacOS/mlx.metallib"
  if [ ! -s "$DMG_METALLIB" ]; then
    echo "✗ DMG 中缺少 OpenLess.app/Contents/MacOS/mlx.metallib"
    exit 1
  fi
  DMG_METALLIB_SHA="$(shasum -a 256 "$DMG_METALLIB" | awk '{print $1}')"
  if [ "$DMG_METALLIB_SHA" != "$APP_METALLIB_SHA" ]; then
    echo "✗ app 与 DMG 中的 mlx.metallib SHA-256 不一致"
    exit 1
  fi
  cleanup_dmg_mount
  trap - EXIT

  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then
    UPDATER_ARCHIVE="src-tauri/target/release/bundle/macos/OpenLess.app.tar.gz"
    if [ ! -f "$UPDATER_ARCHIVE" ]; then
      echo "✗ 未找到 Tauri updater archive：$UPDATER_ARCHIVE"
      exit 1
    fi
    UPDATER_METALLIB_SHA="$(tar -xOf "$UPDATER_ARCHIVE" \
      OpenLess.app/Contents/MacOS/mlx.metallib | shasum -a 256 | awk '{print $1}')"
    if [ "$UPDATER_METALLIB_SHA" != "$APP_METALLIB_SHA" ]; then
      echo "✗ app 与 updater 中的 mlx.metallib SHA-256 不一致"
      exit 1
    fi
  fi
  echo "✓ MLX metallib sha256=$APP_METALLIB_SHA"
elif [ -e "$APP/Contents/MacOS/mlx.metallib" ]; then
  echo "✗ Intel app 不应包含 Apple Silicon MLX metallib"
  exit 1
fi

HAS_DEVELOPER_ID=0
if [ -n "${APPLE_CERTIFICATE:-}" ] \
  || { [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ "${APPLE_SIGNING_IDENTITY}" != "-" ]; }; then
  HAS_DEVELOPER_ID=1
fi
HAS_NOTARIZATION_CREDENTIALS=0
if { [ -n "${APPLE_ID:-}" ] \
    && [ -n "${APPLE_PASSWORD:-}" ] \
    && [ -n "${APPLE_TEAM_ID:-}" ]; } \
  || { [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; }; then
  HAS_NOTARIZATION_CREDENTIALS=1
fi
if [ "$HAS_DEVELOPER_ID" = "1" ] && [ "$HAS_NOTARIZATION_CREDENTIALS" = "1" ]; then
  echo "▶ 校验 Gatekeeper 与公证票据"
  spctl --assess --type execute --verbose=2 "$APP"
  xcrun stapler validate "$APP"
  xcrun stapler validate "$DMG_PATH"
fi

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
