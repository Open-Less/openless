# OpenLess iOS Build Guide

[English](README.md) | [简体中文](README.zh.md)

This directory contains the native Swift iPhone app, Chinese/English keyboard extension, recording widget, and `CoreHost` Rust bridge. For general end-user installation and IPA download notes, see <https://openless.top/en/docs/installation/>.

## Platform & Version Baseline

- **Deployment targets**: iOS **16.0+** for the main app and keyboard extension; iOS **18.0+** for `RecordingWidget` (Live Activity / recording controls).
- **Architecture & language**: `arm64`, Swift 6.0 (strict concurrency), C++17 (for `RimeBridge`).
- **Versioning**: iOS maintains independent versioning in `Config/Version.xcconfig` (included by `Config/Base.xcconfig`).
- **Default identifiers**: Bundle ID `app.openless.ios`, App Group `group.app.openless.ios` (all three targets must share the same App Group when customized).

## Input Modes & Runtime Architecture

- **Keyboard layouts**: Choose in OpenLess Settings between a voice-focused layout or a unified typing keyboard with hold-space dictation, inline translation, and send/cancel gestures.
- **Engines**: Chinese/English typing uses `RimeBridge` (Objective-C++), on-device local speech uses `Packages/LocalSpeech` (SenseVoice), and cloud ASR / LLM / Multimodal / DeepSeek-suite pipelines run through `CoreHost` (static Rust library `libOpenLessCore`).
- **Security & privacy boundaries**:
  - Service credentials stay in the main app's iOS Keychain.
  - The shared App Group mailbox (`Packages/OpenLessShared`) exchanges bounded commands and results only—never raw audio.
  - Typing data and Rime user learning stay inside the keyboard container; result insertion checks document identity to prevent stale replay.
  - Idle state keeps neither the microphone nor an audio render loop active. Runtime-checked PiP/host-return helpers gracefully fall back when unavailable; this source build does not imply App Store review approval.

## Build from source

### Prerequisites

- macOS with Xcode (configured for iOS 16+ SDKs) and Command Line Tools
- Rust toolchain with iOS targets (`aarch64-apple-ios`, `aarch64-apple-ios-sim`)
- Python 3, CMake / C++ toolchain for building Rime dependencies, and XcodeGen (auto-resolved via `scripts/resolve-xcodegen.sh` or `OPENLESS_XCODEGEN_BIN`)

### Full Device Build & Unsigned IPA Packaging

`project.yml` is the source of truth for `OpenLess.xcodeproj`. Edit `project.yml` before regenerating the project. Core build scripts copy and patch `openless-core` in isolation so shared desktop/mobile sources are never modified in place.

```bash
cd openless-all/ios
./scripts/bootstrap.sh
./scripts/bootstrap-local-speech.sh
./scripts/build-rime.sh
./scripts/build-core.sh
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-unsigned.ipa"
```

- `package-ipa.py` validates the app, keyboard extension, widget, deployment floors, resource checksums, archive layout, and executable permissions, and never overwrites an existing IPA path.
- Unsigned IPAs are intended for personal signing (e.g., via Xcode or your own developer signing flow).

### Simulator Build

```bash
cd openless-all/ios
./scripts/build.sh simulator Debug
```

Override derived data output with `OPENLESS_DERIVED_DATA` and XcodeGen binary with `OPENLESS_XCODEGEN_BIN`.

### Personal Device Signing in Xcode

1. Copy `Config/Local.xcconfig.example` to `Config/Local.xcconfig` (ignored by Git).
2. Set your Apple Development Team, unique `OL_BUNDLE_ID`, and matching `OL_APP_GROUP` shared across the app, keyboard extension, and widget.
3. Build a signed device package:

```bash
cd openless-all/ios
./scripts/build.sh device Personal signed
```

## Enable the Keyboard on iPhone

1. Install the signed app and launch **OpenLess** once to grant microphone access and configure your speech/LLM pipeline (<https://openless.top/en/docs/setup/>).
2. Open iOS **Settings → General → Keyboard → Keyboards → Add New Keyboard → OpenLess**.
3. Tap **OpenLess** in the keyboard list and turn on **Allow Full Access** (required for shared settings and voice control via the App Group).
4. Pick your preferred keyboard mode in OpenLess Settings and verify dictation in the built-in test field.

## Verification

```bash
cd openless-all/ios
./scripts/test.sh unit
python3 scripts/test-interface-resources.py
python3 scripts/validate-project.py
IOS_SIMULATOR_ID=<dedicated-simulator-id> ./scripts/test.sh ui
```

- `unit` verifies shared contracts, preferences, delivery guarantees, mobile Core/Rust host, Rime composition, predictions, and audio conversion.
- `ui` exercises the installed keyboard extension and host app on a dedicated simulator (use a dedicated simulator because it modifies keyboard settings). `./scripts/test.sh all` runs both.
- Hardware microphone routing, live audio capture, and PiP behavior require physical device testing.

## Source Layout

| Directory | Contents |
| --- | --- |
| `App/` | Native SwiftUI host app, settings, speech pipeline, audio capture, and Core bridge |
| `Keyboard/` | Typing layout, candidate bar, hold-space voice gestures, and text insertion |
| `RecordingWidget/` | iOS 18+ recording controls and Live Activity views |
| `PlatformShared/` | Shared native controls and App Group container helpers |
| `Packages/OpenLessShared/` | Versioned mailbox, preferences, gesture, and delivery contracts |
| `Packages/LocalSpeech/` | On-device SenseVoice speech runtime adapter |
| `CoreHost/` | iOS Rust workspace, C ABI bridge, and isolated Core patch |
| `RimeBridge/` | Objective-C++ Rime composition and vocabulary bridge |
| `Resources/` | Brand assets, `en.lproj` / `zh-Hans.lproj` localization, input schemas, privacy manifest, and `Licenses/` |
| `AppTests/`, `UITests/`, `Tests/` | Native unit, extension UI, and runtime test fixtures |
| `scripts/` | Dependency bootstrap, Rime/Core builds, IPA packaging, and validation scripts |

Dependency revisions are pinned in `scripts/bootstrap.sh`, `scripts/bootstrap-local-speech.sh`, and `scripts/build-predictions.py`. Preserve `Resources/Licenses/` when redistributing; refresh Rust dependency attributions with `python3 scripts/collect-rust-licenses.py`.
