# OpenLess Android platform code

[English](README.md) | [简体中文](README.zh.md)

The Android app uses a Tauri WebView for settings and native Kotlin services for the keyboard, overlay, and Accessibility integration. The [Android runtime flow](../../../docs/architecture.md#移动端输入法) describes the runtime behavior.

## Source map

| Path | Responsibility |
| --- | --- |
| [`kotlin/`](kotlin/) | Keyboard, resident runtime, warmup activity, overlay, Accessibility, Shizuku and native tests |
| [`aidl/`](aidl/) and [`manifests/`](manifests/) | Binder interfaces, Android permissions and services, XML resources |
| [`frontend/`](frontend/) | Permission, microphone and overlay controls imported through the Vite `@android` alias |
| [`../src-tauri/src/android/`](../src-tauri/src/android/) | Rust JNI, insertion fallback, permissions and updater. Shared preference types live in Core's [`android_types.rs`](../crates/openless-core/src/android_types.rs) |
| [`../mobile-ime/`](../mobile-ime/README.md) | Shared librime engine, bundled dictionaries, candidate acknowledgements, and learning and privacy rules |

## Build and CI

Use Node.js 22, Rustup with the desired Android target, a JDK, Android SDK and NDK, Python 3 and CMake. See the [APK workflow](../../../.github/workflows/android-apk.yml) for pinned CI toolchain versions. Run from `openless-all/app` with `ANDROID_HOME` and `ANDROID_NDK_HOME` pointing to installed tools. Set `OPENLESS_CACHE_ROOT` to a writable location when `/data-win/openless-cache` is unavailable.

```bash
npm ci
npm run tauri -- android init --ci
npm run copy:android-scaffolding
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
node scripts/merge-android-updater-manifest.mjs
npm run tauri:android:build:debug
```

The copy script stages the Kotlin, AIDL and resources into the generated `src-tauri/gen/android/` project. Make lasting changes in the checked-in Android sources and merge scripts. The debug and release wrappers stage native libraries and offline Rime data for the target ABIs. To check Rime separately, run `python3 scripts/build-mobile-rime.py android --ndk "$ANDROID_NDK_HOME"`. Use `--stage` before building with Gradle directly if the native inputs are missing. With the matching Rust library already built, a Kotlin-only arm64 iteration can run `./gradlew app:assembleArm64Debug -x app:rustBuildArm64Debug` inside `src-tauri/gen/android`.

Use `npm run tauri:android:build:release` for a release-profile build. The [APK workflow](../../../.github/workflows/android-apk.yml) collects the per-ABI packages and handles release signing. For a local test install, select the ABI reported by `adb shell getprop ro.product.cpu.abi` and use `adb install -r <apk>` with compatible signing. Debug and official packages may use different signatures.

`npm test` from the app directory covers contract checks for IPC, manifests, permissions, and insertion. On a matching device, verify keyboard gestures, cold starts, overlays, Accessibility and Shizuku before publishing under the [release policy](../../../RELEASING.md).
