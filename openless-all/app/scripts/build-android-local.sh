#!/usr/bin/env bash
set -euo pipefail
APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_ROOT="${OPENLESS_CACHE_ROOT:-/data-win/openless-cache}"
export JAVA_HOME="${OPENLESS_ANDROID_JAVA_HOME:-/usr/lib/jvm/java-21-openjdk-amd64}"
export ANDROID_HOME="${ANDROID_HOME:-$CACHE_ROOT/mobile-rime/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/android-ndk-r26b}"
export NDK_HOME="$ANDROID_NDK_HOME"
export ANDROID_USER_HOME="$CACHE_ROOT/android-user"
export GRADLE_USER_HOME="$CACHE_ROOT/gradle"
export CARGO_HOME="$CACHE_ROOT/cargo"
export CARGO_TARGET_DIR="$CACHE_ROOT/android-target"
export npm_config_cache="$CACHE_ROOT/npm"
export OPENLESS_ANDROID_TARGETS="${OPENLESS_ANDROID_TARGETS:-aarch64}"
export PATH="$JAVA_HOME/bin:$PATH"
if [ ! -f "$NDK_HOME/build/cmake/android.toolchain.cmake" ]; then
  echo 'Android NDK missing; set ANDROID_NDK_HOME.' >&2
  exit 1
fi
cd "$APP_ROOT"
if [ ! -f src-tauri/gen/android/app/build.gradle.kts ]; then
  npm run tauri -- android init --ci
fi
node scripts/copy-android-scaffolding.mjs
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
node scripts/merge-android-updater-manifest.mjs
npm run tauri:android:build:debug
