# OpenLess All-Platform Build Guide

[English](README.md) | [简体中文](README.zh.md)

This directory contains the cross-platform OpenLess source tree. Start with the [documentation index](../docs/index.md) and [architecture & data-flow guide](../docs/architecture.md) to orient yourself before building. Official user and setup documentation lives at <https://openless.top/en/docs/>.

## Repository Layout

| Path | Role |
| --- | --- |
| `app/crates/openless-core` | Framework-independent shared Rust core (dictation engine, ASR/LLM/Multimodal/DeepSeek-suite pipelines, style packs, dictionary, cloud sync, backend contract `2.0.0`). |
| `app/crates/micyou-dsp`, `app/crates/micyou-infer` | Audio DSP preprocessing and inference crates. |
| `app/crates/openless-capsule`, `app/native-capsule` | Native recording capsule state and platform UI support. |
| `app/src` | Shared React + TypeScript frontend for Tauri desktop and Android. |
| `app/src-tauri` | macOS, Windows, and Android Tauri host, native adapters, and vendored crates (`src-tauri/vendor/`). |
| `app/ask-window` | Standalone Ask Anything desktop window assets and bridge. |
| `linux/egui` | Native Linux host and `egui`/`eframe` UI (no Tauri or WebKitGTK dependency). |
| `android` | Android host packaging, manifest merges, and native glue. |
| `harmony` | Native HarmonyOS ArkUI host and `harmony/rust` Node-API bridge. |
| `app/mobile-ime` | Shared native input-method core for Android and HarmonyOS. |
| `windows/ime` | Windows TSF/IME integration scripts and helpers. |
| `windows/nsis`, `windows/wix`, `windows/scripts` | Windows installer assets and build/acceptance tools. |
| `macos/dmg`, `macos/scripts`, `macos/*.plist` | macOS installer layout, build/acceptance tools, and application permissions. |
| `linux/fcitx5-plugin` | Fcitx5 addon used by the Linux host for text insertion. |
| `ios/` | Native Swift iPhone app, Chinese/English keyboard extension, recording widget, and `CoreHost` Rust bridge. |

> **No Git submodules required:** Patched crates under `app/src-tauri/vendor/` (`cpal`, `tauri-runtime-wry`) are vendored directly in the repository, so a fresh clone is ready to build.

## Prerequisites & Quick Start (Desktop Tauri)

- **Node.js**: 22+
- **Rust**: 1.99+ (`rust-version = "1.99"` across workspace crates)
- **Platform Tauri prerequisites**: Xcode Command Line Tools on macOS; Visual Studio Build Tools + Windows SDK (or MinGW) on Windows.

Start the desktop Tauri development build from `openless-all/app`:

```bash
cd openless-all/app
npm ci
npm run tauri dev
```

After launch, dictate a short sentence into an empty input field to confirm both transcription and cursor insertion work before making changes.

Common verification checks:

```bash
cd openless-all/app
npm test
npm run build
cargo test --locked -p openless-core
```

## Shared backend and Linux host

The root Rust workspace (`openless-all/app/Cargo.toml`) covers `openless-core`, `micyou-dsp`, `micyou-infer`, `openless-capsule`, `native-capsule`, `linux-egui`, and `harmony/rust`, excluding `src-tauri` so core and Linux checks run independently without Tauri or WebKitGTK.

`linux-egui` talks to `openless-core` in-process via the typed Rust `contract` module and communicates with its UI subprocess over Unix sockets (JSONL v2). Text insertion uses Fcitx5 (`linux/fcitx5-plugin`).

Run tests and local packaging on Linux:

```bash
cd openless-all/app
cargo test --locked -p openless-linux-egui
OPENLESS_LINUX_VERSION="$(node -p "require('./package.json').version.split('+')[0]")-$(cat ../linux/egui/package-revision)" bash ../linux/scripts/package-linux-egui.sh
bash ../linux/scripts/verify-linux-egui-packages.sh
```

> Linux CI and automated release workflows remain paused while device/packaging acceptance is pending. Local scripts still produce verified `.deb` / `.rpm` packages and the Fcitx5 addon (AppImage is retired). See [`RELEASING.md`](../RELEASING.md) before distributing Linux packages.

## macOS Build

Use the repository build script rather than invoking `tauri build` directly:

```bash
cd openless-all/app
INSTALL=0 ./../macos/scripts/build-mac.sh
```

Generated macOS outputs:

- `openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `openless-all/app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg` (Apple Silicon) or `OpenLess_<version>_x64.dmg` (Intel)

To build and install locally during development, run `./../macos/scripts/build-mac.sh` without `INSTALL=0`. Local macOS builds fall back to ad-hoc signing when Apple signing credentials are absent.

## Windows Build

Run the preflight script first so missing MSVC, Windows SDK, or MinGW components fail with actionable guidance:

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-preflight.ps1
```

### MSVC Route (Recommended)

Install Visual Studio Build Tools with `Microsoft.VisualStudio.Workload.VCTools`, MSVC v143 x64/x86 build tools, and a Windows 10/11 SDK providing `kernel32.lib`. From Developer PowerShell (or after calling `vcvars64.bat`):

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-package-msvc.ps1
```

### GNU / MinGW Route

When MSVC/Windows SDK is unavailable, use the GNU helper script (the space-free `openless-all` path avoids MinGW quoting issues):

```powershell
cd openless-all/app
scoop install rustup mingw
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-preflight.ps1 -Toolchain gnu
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-build-gnu.ps1
```

Generated GNU artifacts:

- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe`
- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\msi\OpenLess_*_x64_en-US.msi`
- `%TEMP%\openless-windows-gnu\app\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis\OpenLess_*_x64-setup.exe`

### Hotkey Injection Gate & Runtime Notes

Before/after Windows hotkey changes without a physical keyboard regression, run the dry-run injection gate:

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\..\windows\scripts\windows-hotkey-injection-smoke.ps1 -ExePath .\.artifacts\windows-gnu\dev\openless.exe
```

- Windows does not require macOS Accessibility permission; inspect listener health under **Settings → Permissions → Global hotkey**.
- Microphone permission is verified by opening a short-lived input stream rather than querying device formats alone.
- `Ctrl+V` insertion is reported as a clipboard fallback unless insertion is confirmed.

## Android Build

See [`android/README.md`](android/README.md#build-and-ci) for Android SDK/NDK versions, generated Tauri Android scaffolding, APK builds across ABIs (`arm64-v8a`, `armeabi-v7a`, `x86_64`, `x86`), and device verification.

## iOS Build

iOS (`ios/`) is a native Swift + Rust project targeting iOS 16+ (Recording Widget requires iOS 18+) with independent versioning. See the [iOS build & signing guide](ios/README.md#build-from-source) for bootstrapping dependencies, building `CoreHost` and Rime, personal Xcode signing, simulator runs, and unsigned IPA packaging.

## Release Signing & Distribution

Follow [`RELEASING.md`](../RELEASING.md) for version synchronization across all five release manifests, signing, notarization, CDN updater manifests, and channel promotion.

## Third-Party Components & Fonts

OpenLess adapts components from **Rare UI** (GitHub Activity, Password Input, Toast, AI Prompt Box / Streaming Response; Copyright (c) 2026 Swami Malode, MIT + Commons Clause + Attribution) and bundles **Open Runde** fonts (SIL Open Font License 1.1). Full attribution and license files are documented in [`docs/ATTRIBUTION.md`](../docs/ATTRIBUTION.md) and visible in-app under **Settings → About and updates → Component credits**.

## Ignored Local Output

These build outputs are intentionally untracked:

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
