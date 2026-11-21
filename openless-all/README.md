# OpenLess All-Platform

[English](README.md) | [简体中文](README.zh.md)

This directory contains the cross-platform OpenLess application. Start with the repository [documentation index](../docs/index.md), [architecture](../docs/architecture.md), and [source structure](../docs/structure.md).

## Native iOS app and keyboard

[`ios/`](ios/README.md) contains the native iPhone app, Chinese/English keyboard extension, and recording widget. Settings offer a voice-focused layout or a single keyboard with hold-space dictation, translation, and send/cancel gestures. The app and keyboard target iOS 16; the recording widget requires iOS 18. The iOS guide covers the shared version configuration, input engines, source build, signing, and unsigned IPA packaging.

## App Directory

The runnable sources live in `app/`:

- `app/crates/openless-core`: framework-independent shared backend Interface and business rules;
- `app/src`: React frontend for the Tauri hosts;
- `app/src-tauri`: macOS, Windows, and Android Tauri Host and native adapters;
- `app/linux-egui`: Linux Host and egui/eframe UI; it does not depend on Tauri or WebKitGTK;
- `app/harmony`: HarmonyOS host (ArkTS + ArkWeb) with the shared Rust backend;
- `app/mobile-ime`: native input-method backend shared by Android and HarmonyOS;
- `app/scripts/linux-fcitx5-plugin`: the fcitx5 addon Linux dictation goes through.

The Tauri manifest includes path dependencies under `app/src-tauri/vendor/` (`cpal`, `tauri-runtime-wry`). These crates are vendored in the repository, so **there are no Git submodules to initialize** — a fresh clone is ready to build. The root Core/Linux workspace excludes `src-tauri`, so its independent checks do not require them.

```bash
cd app
npm ci
npm run tauri dev
```

## Shared backend and Linux host

This repository supplies the shared typed Rust interface, semantic events, fixtures, and Linux adapters. `linux-egui/src/main.rs` now implements an egui/eframe application, with `LinuxHost` and `LinuxBackendBuilder` connecting it to Core. Remaining Host/UI work and product acceptance are tracked in the [Linux handoff](../docs/linux-egui-handoff/README.md).

```bash
cd app
cargo test -p openless-core --locked
cargo test -p openless-linux-egui --all-targets --locked
pwsh ./scripts/check-core-deps.ps1
pwsh ./scripts/check-core-deps.ps1 openless-linux-egui
```

Linux packaging is manual while the L01–L12 device acceptance is open: the dedicated CI workflows (`check-linux-egui.yml`, `release-linux-egui.yml`) were removed and desktop releases no longer dispatch Linux. The local scripts still build deb/rpm and the fcitx5 plugin without WebKitGTK (AppImage is retired). Restoring automated Linux releases requires the acceptance evidence described in [`RELEASING.md`](../RELEASING.md) and [`docs/ci-trigger-and-cache-policy.md`](../docs/ci-trigger-and-cache-policy.md).

## macOS Build

Use the project build script instead of calling `tauri build` directly:

```bash
cd app
INSTALL=0 ./scripts/build-mac.sh
```

Generated macOS artifacts:

- `app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg`

For local install during development:

```bash
cd app
./scripts/build-mac.sh
```

## Windows Build

The runnable Tauri app is still `app/`. Windows contributors should run a
preflight before building so missing MSVC, Windows SDK, or MinGW tools fail
with actionable messages.

```powershell
cd app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1
```

### MSVC Route

Use this route when Visual Studio Build Tools and the Windows SDK are installed.
Open a Developer PowerShell, or call `vcvars64.bat`, then run:

```powershell
cd app
npm ci
npm run tauri -- build
```

Required Visual Studio Installer components:

- `Microsoft.VisualStudio.Workload.VCTools`
- MSVC v143 x64/x86 build tools
- Windows 10/11 SDK that provides `kernel32.lib`

If `link.exe` or `kernel32.lib` is missing, rerun:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain msvc
```

### GNU / MinGW Route

Use this route when MSVC/Windows SDK is unavailable. The app now lives under
the no-space `openless-all` directory to avoid GNU/MinGW path quoting issues
while generating import libraries. Use the helper script to keep the GNU build
environment and target setup consistent.

```powershell
cd app
scoop install rustup mingw
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain gnu
powershell -ExecutionPolicy Bypass -File .\scripts\windows-build-gnu.ps1
```

Generated GNU artifacts:

- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe`
- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\msi\OpenLess_*_x64_en-US.msi`
- `%TEMP%\openless-windows-gnu\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\OpenLess_*_x64-setup.exe`

### Hotkey Injection Gate

Use this gate before/after Windows hotkey changes when a physical keyboard
regression is unavailable. It injects a dev/test-only hotkey click through the
coordinator `handle_pressed` / `handle_released` path, asserts the log contains
`[coord] hotkey pressed`, and cancels the dry-run session automatically.

```powershell
cd app
```

### Windows Runtime Notes

- Windows does not need the macOS Accessibility permission. Use Settings ->
  Permissions -> Global hotkey to inspect listener status.
- Microphone permission is checked by opening a short-lived input stream, so a
  device-format query alone is not treated as permission granted.
- Text insertion through `Ctrl+V` is treated as copy fallback unless the app can
  confirm insertion.

## Android Build

A fresh clone has no generated Android project, and the build script stops until `src-tauri/gen/android` exists. Initialize it, copy the Kotlin/XML scaffolding, and merge the manifests the same way CI does:

```bash
cd app
npm ci
npm run tauri:android:init                  # creates src-tauri/gen/android
npm run copy:android-scaffolding            # Kotlin sources + res/xml
node scripts/merge-android-v1-manifest.mjs  # microphone permission
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
node scripts/merge-android-updater-manifest.mjs   # release builds: in-app update manifest
npm run tauri:android:build:release
```

The same chain, including the local Gradle workflow, is documented in [`app/android/README.md`](app/android/README.md).

APKs are written under `app/src-tauri/gen/android/app/build/outputs/apk/<abi>/release/`. Use `npm run tauri:android:build:debug` for a debug build; the `android-apk.yml` workflow produces debug smoke artifacts for `arm64-v8a`, `armeabi-v7a`, `x86`, and `x86_64`. The native keyboard/IME backend lives in `app/android/` and `app/mobile-ime/`.

## HarmonyOS Build

The ArkTS/ArkWeb host builds through the frontend script; see [`app/harmony/README.md`](app/harmony/README.md) for the toolchain details.

```bash
cd openless-all/app
bash scripts/build-harmony-frontend.sh
```

Requirements: HarmonyOS Command Line Tools, Python 3, and `rustup target add aarch64-unknown-linux-ohos`. The unsigned HAP is written to `app/harmony/openless-project/entry/build/default/outputs/default/`; the `release-harmony.yml` workflow attaches it to releases, while AppGallery packages are submitted manually. Installing on a device additionally needs a Huawei debug certificate for `openless.hm.com` and a debug Profile that contains that device — see [`app/harmony/README.md`](app/harmony/README.md).

## iOS Build

The native project is independent of the desktop version; see [`ios/README.md`](ios/README.md). Requirements: an Apple Silicon Mac, Xcode with its command-line tools selected, Python 3, CMake, Git, and Rustup.

```bash
cd openless-all/ios
./scripts/bootstrap.sh                    # Rust 1.99.0, iOS targets, pinned native deps, XcodeGen
./scripts/bootstrap-local-speech.sh       # bundled SenseVoice runtime
./scripts/build-rime.sh                   # Rime data and compiled table
./scripts/build-core.sh                   # Frameworks/<platform>/libOpenLessCore.a
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py \
  DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-0.1.0-16-unsigned.ipa"
```

`build.sh` takes `<simulator|device> <Debug|Personal|Release> [unsigned|signed]` and defaults to `simulator Debug unsigned`; it refuses to run until `build-core.sh` and `build-rime.sh` have produced `libOpenLessCore.a`, `libOpenLessRime.a`, the Rime table, and the prediction lexicon. `package-ipa.py` needs the built `.app` plus a new output path, never overwrites an existing IPA, and packages only an unsigned device build for personal signing.

## Release Signing

Tagged Tauri releases (`v*-tauri`) must be Developer ID signed and notarized so users can download and open the macOS app without manually removing quarantine attributes. Linux packages use the separate manual egui workflow and an independent minisign secret.

Required GitHub secrets:

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

Optional:

- `APPLE_PROVIDER_SHORT_NAME`
- `KEYCHAIN_PASSWORD`

Manual workflow runs can still produce ad-hoc signed test builds, but tagged macOS releases fail if signing/notarization secrets are missing.

## Third-party components

The overview yearly activity card ports [Rare UI GitHub Activity](https://rareui.com/components/githubactivity) (Copyright (c) 2026 Swami Malode). The license is MIT plus the Commons Clause. Cell size, gap, radius, padding, month labels, tooltip, and the expandable panel stay with the upstream component; local dictation counts replace GitHub contributions, and the apps with the most voice input replace repositories. The same credit is in the app under Settings → About and updates → Component credits. See [docs/ATTRIBUTION.md](../docs/ATTRIBUTION.md).

## Ignored Local Output

The following are intentionally local-only:

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
