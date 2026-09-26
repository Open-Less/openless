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

source scripts/macos-build-env.sh
echo "▶ Cargo release codegen units: ${CARGO_PROFILE_RELEASE_CODEGEN_UNITS} (macOS only)"
echo "▶ Cargo release strip: ${CARGO_PROFILE_RELEASE_STRIP} (macOS only)"
echo "▶ Rust proc-macro host wrapper: ${RUSTC_WRAPPER}"

# Cargo 为 build-script 可执行文件和 OUT_DIR 创建不同目录。只在多个
# metallib 输出之间清理，保留所有 build-script 缓存，避免每次重新编译。
KEEP_QWEN_DIR=""
for d in src-tauri/target/release/build/qwen3-asr-rs-*; do
  [ -s "$d/out/lib/mlx.metallib" ] || continue
  if [ -z "$KEEP_QWEN_DIR" ] || [ "$d/out/lib/mlx.metallib" -nt "$KEEP_QWEN_DIR/out/lib/mlx.metallib" ]; then
    KEEP_QWEN_DIR="$d"
  fi
done
if [ -n "$KEEP_QWEN_DIR" ]; then
  for d in src-tauri/target/release/build/qwen3-asr-rs-*; do
    [ -s "$d/out/lib/mlx.metallib" ] || continue
    [ "$d" = "$KEEP_QWEN_DIR" ] || rm -rf "$d"
  done
fi

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
APP_VERSION="$(node -p "require('./package.json').version")"
DMG_PATH="$DMG_DIR/OpenLess_${APP_VERSION}_${MAC_BUNDLE_ARCH}.dmg"

# 清掉交付产物，保留 Cargo 缓存。热构建可复用旧时间戳的二进制，不能用
# 编译文件的 mtime 判断 bundle 新鲜度；Tauri 失败时也不能接受上轮安装包。
rm -rf "$APP"
rm -f "$DMG_PATH" "${APP}.tar.gz" "${APP}.tar.gz.sig"
TAURI_BUILD_ARGS+=(-- --locked --timings)
if [ "$MAC_BUNDLE_ARCH" = "aarch64" ]; then
  # Tauri skips Finder AppleScript in CI. Write deterministic Finder metadata
  # into its temporary image before Tauri compresses and signs the final DMG.
  DMG_LAYOUT_ENV_DIR="$(mktemp -d "${TMPDIR:-/tmp}/openless-dmg-python.XXXXXX")"
  cleanup_dmg_environment() { rm -rf "$DMG_LAYOUT_ENV_DIR"; }
  trap cleanup_dmg_environment EXIT
  python3 -m venv "$DMG_LAYOUT_ENV_DIR"
  DMG_LAYOUT_PYTHON="$DMG_LAYOUT_ENV_DIR/bin/python3"
  "$DMG_LAYOUT_PYTHON" -m pip install --quiet --disable-pip-version-check \
    --only-binary=:all: --no-deps --require-hashes -r scripts/macos-dmg-requirements.txt
  "$DMG_LAYOUT_PYTHON" scripts/macos-dmg-layout.test.py
  CI=true TAURI_BUNDLER_DMG_IGNORE_CI=false \
    OPENLESS_DMG_LAYOUT_ROOT="$PWD" OPENLESS_DMG_LAYOUT_PYTHON="$DMG_LAYOUT_PYTHON" \
    OPENLESS_DMG_LAYOUT_STAMP="$DMG_LAYOUT_ENV_DIR/layout-applied" \
    PATH="$PWD/scripts/macos-dmg-bin:$PATH" npm run tauri -- "${TAURI_BUILD_ARGS[@]}"
  if [ ! -s "$DMG_LAYOUT_ENV_DIR/layout-applied" ]; then
    echo "✗ Tauri 未调用 DMG 布局步骤，中止交付"
    exit 1
  fi
  "$DMG_LAYOUT_PYTHON" scripts/macos-dmg-layout.py verify "$DMG_PATH"
  cleanup_dmg_environment
  trap - EXIT
else
  npm run tauri -- "${TAURI_BUILD_ARGS[@]}"
fi

if [ ! -f "$APP/Contents/MacOS/openless" ]; then
  echo "✗ $APP 缺失或不是本次构建的产物（打包未完成），中止"
  exit 1
fi
# DMG 一律由 Tauri 生成（带签名/公证链路）；手搓 hdiutil DMG 会绕过这些步骤，
# bundle contract 测试显式禁止。缺失即失败，不兜底。
if [ ! -f "$DMG_PATH" ]; then
  echo "✗ 未找到本次构建的 DMG：$DMG_PATH（tauri build 未完成打包）"
  exit 1
fi

echo "▶ 校验 Info.plist / 签名"
/usr/libexec/PlistBuddy -c "Print :NSMicrophoneUsageDescription" "$INFO" > /dev/null
bash scripts/check-macos-speech-usage-description.sh "$INFO"
codesign -d --entitlements :- "$APP" 2> /dev/null | grep -q "com.apple.security.device.audio-input"
codesign --verify --deep --strict --verbose=2 "$APP" 2>&1 | tail -2

if [ "$MAC_BUNDLE_ARCH" = "aarch64" ]; then
  echo "▶ 校验 MLX metallib 已进入 app / DMG / updater"
  APP_METALLIB="$APP/Contents/Resources/mlx.metallib"
  if [ ! -s "$APP_METALLIB" ]; then
    echo "✗ Apple Silicon app 缺少 Contents/Resources/mlx.metallib"
    exit 1
  fi
  # Contents/MacOS 里的非 Mach-O 会被 codesign 当成 nested code。
  if [ -e "$APP/Contents/MacOS/mlx.metallib" ]; then
    echo "✗ mlx.metallib 不能放在 Contents/MacOS（ad-hoc codesign 会失败）"
    exit 1
  fi
  APP_METALLIB_SHA="$(shasum -a 256 "$APP_METALLIB" | awk '{print $1}')"

  if [ ! -f "$DMG_PATH" ]; then
    echo "✗ 未找到 Tauri 生成的 DMG：$DMG_PATH"
    exit 1
  fi
  DMG_MOUNT="$(mktemp -d "${TMPDIR:-/tmp}/openless-dmg-verify.XXXXXX")"
  cleanup_dmg_mount() {
    hdiutil detach "$DMG_MOUNT" > /dev/null 2>&1 || true
    rmdir "$DMG_MOUNT" > /dev/null 2>&1 || true
  }
  trap cleanup_dmg_mount EXIT
  hdiutil attach "$DMG_PATH" -readonly -nobrowse -mountpoint "$DMG_MOUNT" > /dev/null
  DMG_METALLIB="$DMG_MOUNT/OpenLess.app/Contents/Resources/mlx.metallib"
  if [ ! -s "$DMG_METALLIB" ]; then
    echo "✗ DMG 中缺少 OpenLess.app/Contents/Resources/mlx.metallib"
    exit 1
  fi
  if [ -e "$DMG_MOUNT/OpenLess.app/Contents/MacOS/mlx.metallib" ]; then
    echo "✗ DMG 中的 mlx.metallib 不能放在 Contents/MacOS"
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
      OpenLess.app/Contents/Resources/mlx.metallib | shasum -a 256 | awk '{print $1}')"
    if [ "$UPDATER_METALLIB_SHA" != "$APP_METALLIB_SHA" ]; then
      echo "✗ app 与 updater 中的 mlx.metallib SHA-256 不一致"
      exit 1
    fi
  fi
  echo "✓ MLX metallib sha256=$APP_METALLIB_SHA"
elif [ -e "$APP/Contents/MacOS/mlx.metallib" ] || [ -e "$APP/Contents/Resources/mlx.metallib" ]; then
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
xattr -cr "$APP" 2> /dev/null || true
find "$DMG_DIR" -maxdepth 1 -name '*.dmg' -exec xattr -c {} \; 2> /dev/null || true

echo "▶ 校验 quarantine 属性"
if xattr -pr com.apple.quarantine "$APP" > /dev/null 2>&1; then
  echo "✗ $APP 仍包含 com.apple.quarantine"
  exit 1
fi
while IFS= read -r dmg; do
  if xattr -p com.apple.quarantine "$dmg" > /dev/null 2>&1; then
    echo "✗ $dmg 仍包含 com.apple.quarantine"
    exit 1
  fi
done < <(find "$DMG_DIR" -maxdepth 1 -name '*.dmg' -print)

if [ "$INSTALL" = "1" ]; then
  echo "▶ 装到 /Applications"
  pkill -f "OpenLess.app/Contents/MacOS/openless" 2> /dev/null || true
  sleep 1
  # 每次重装前重置 TCC：ad-hoc 签名 hash 每次构建都会变，旧授权立即失效，
  # 不重置就会出现"系统设置里看着已勾选实际不生效"。
  tccutil reset Accessibility com.openless.app 2> /dev/null || true
  tccutil reset Microphone com.openless.app 2> /dev/null || true
  rm -rf /Applications/OpenLess.app
  cp -R "$APP" /Applications/
  xattr -dr com.apple.quarantine /Applications/OpenLess.app 2> /dev/null || true
  echo "✓ 装好了：/Applications/OpenLess.app"
  echo "  打开方式：open /Applications/OpenLess.app"
fi
