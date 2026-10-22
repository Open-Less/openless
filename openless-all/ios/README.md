# OpenLess iOS

Native iPhone app, keyboard extension, and recording widget. The app uses SwiftUI and UIKit; the keyboard combines UIKit controls with KeyboardKit. The main app owns audio capture, speech recognition, and the shared Rust Core.

## Version and platform support

`Config/Version.xcconfig` is the single version source for all three bundles. This integration is **0.1.0 (16)** and preserves the Build 15 keyboard behavior. Update `MARKETING_VERSION` and `CURRENT_PROJECT_VERSION` there, then regenerate the project with XcodeGen. Product validation rejects mismatched app/extension versions.

| Component | Minimum iOS | Responsibility |
| --- | --- | --- |
| `OpenLess` | 16 | Settings, speech, processing, history, vocabulary, and styles |
| `OpenLessKeyboard` | 16 | Typing, candidates, recording gestures, and insertion |
| `OpenLessRecordingWidget` | 18 | Recording controls and Live Activity integration |

The iOS version is independent of the desktop release version. Device builds currently target arm64 iPhone; simulator builds target Apple silicon.

## Input modes

Choose **Voice focused** or **Single keyboard** in Settings. The selection, keyboard height, Chinese/English language, and selected processing style persist across keyboard sessions.

| Mode | Typing and voice behavior |
| --- | --- |
| Voice focused | Dedicated voice panel plus Chinese/English typing; long-press space can move the cursor or start recording |
| Single keyboard | Chinese/English typing with `123`, language switch, wide space, and the host's Return action; hold space to record |

Both modes retain the voice toolbar, processing styles, translation, and system keyboard switching. Held recording offers cancel, translation, and send actions. Single-keyboard mode uses one configured translation output language; recording freezes that selection for the current request. Basic typing works without Full Access. Shared preferences and cross-process voice control require Full Access and matching App Group entitlements.

Chinese composition uses librime with a bounded rime-ice dictionary. Candidates expand into a grid; users can select Chinese candidates or explicitly confirm the typed Latin letters. English uses an offline frequency/context dictionary, `UITextChecker`, and the supplementary `UILexicon`. These are the input engines used by this extension, rather than the system keyboard's complete prediction model. Supplementary entries stay in memory. Key sounds use `UIDevice.playInputClick()`; haptic and sound preferences remain configurable.

Normal speech recognition uses bundled SenseVoice Small int8. Developer settings can select Apple Speech or configure Core ASR/LLM/Omni providers. Recognition language is separate from typing language. Translation and polishing depend on the configured processing provider. Preparing voice access does not start recording.

## Build from source

Requirements: an Apple silicon Mac, Xcode 27 with its command-line tools selected, Python 3, CMake, Git, and Rustup. Bootstrap installs Rust 1.99.0 and the two iOS targets, and pins native dependencies and XcodeGen. Downloaded speech artifacts and prediction sources are checksum-verified.

Desktop sources under `openless-all/app/` follow `beta` unchanged. `scripts/prepare-core.py` copies the checked-in Core into the ignored `CoreHost/.build/core-source/` directory and applies `CoreHost/Patches/ios-input-contracts.patch` there. The iOS Rust workspace builds and tests that copy. The patch adds the mobile input contracts without changing the Core used by desktop, Android, or Linux. Bootstrap, tests, native builds, and license collection prepare the copy automatically; incompatible upstream changes fail patch validation.

```sh
cd openless-all/ios
./scripts/bootstrap.sh
./scripts/bootstrap-local-speech.sh
./scripts/build-rime.sh
./scripts/build-core.sh
python3 scripts/collect-rust-licenses.py
python3 scripts/validate-project.py
./scripts/build.sh device Release unsigned
python3 scripts/package-ipa.py DerivedData/Build/Products/Release-iphoneos/OpenLess.app \
  "$HOME/Downloads/OpenLess-0.1.0-16-unsigned.ipa"
```

The unsigned IPA is for personal signing. Packaging validates the actual app, keyboard, widget, deployment floors, resource checksums, archive contents, and executable permissions. An existing output is never overwritten.

For the simulator, use `./scripts/build.sh simulator Debug`. `OPENLESS_DERIVED_DATA` selects a build directory and `OPENLESS_XCODEGEN_BIN` selects an installed XcodeGen. `project.yml` generates the checked-in Xcode project; edit the specification before regenerating it.

For Xcode device signing, copy `Config/Local.xcconfig.example` to `Config/Local.xcconfig`, set your team, bundle ID, and App Group, then run `./scripts/build.sh device Personal signed`. All three bundles must use the same App Group. Local signing configuration stays outside Git.

## Enable the keyboard

1. Install the signed app and open OpenLess to enable voice access.
2. In iOS Settings, choose General → Keyboard → Keyboards → Add New Keyboard → OpenLess.
3. Enable Full Access for shared settings and voice control.
4. Choose the keyboard experience in OpenLess Settings and try it in the app's keyboard editor.

The main app activates voice when needed and returns to the requesting host when the runtime supports it. Active PiP can keep the session available between recordings. The isolated PiP/host-return compatibility adapters use runtime-checked private selectors and fall back when unavailable; this source build is not an App Store approval claim. Idle state does not keep the microphone or an audio render loop running.

## Verification

```sh
./scripts/test.sh unit
python3 scripts/test-interface-resources.py
python3 scripts/validate-project.py
IOS_SIMULATOR_ID=<dedicated-simulator-id> ./scripts/test.sh ui
```

Unit checks cover shared contracts, preferences, delivery, the mobile Core and Rust Host, real Rime composition, predictions, and audio conversion. UI tests exercise the installed extension and host app; use a dedicated simulator because they configure keyboard settings. `test.sh all` combines both suites. The GitHub iOS workflow verifies that shared sources remain unchanged, runs mobile Swift/Rust/resource tests, builds the native dependencies and unsigned iPhone app, and validates the IPA. Runtime microphone, routing, and PiP behavior still require device testing.

## Source layout

| Directory | Contents |
| --- | --- |
| `App/` | Native app, settings, speech, audio, and Core integration |
| `Keyboard/` | Typing layout, candidates, voice gestures, and host insertion |
| `RecordingWidget/` | Recording controls and Live Activity views |
| `PlatformShared/` | Shared native controls and container access |
| `Packages/OpenLessShared/` | Versioned mailbox, preferences, gestures, and delivery contracts |
| `Packages/LocalSpeech/` | SenseVoice runtime adapter used by the app |
| `CoreHost/` | iOS Rust workspace, C ABI, and isolated Core source patch |
| `RimeBridge/` | Objective-C++ composition and vocabulary bridge |
| `Resources/` | Brand assets, localization, input schemas, privacy manifest, and licenses |
| `AppTests/`, `UITests/`, `Tests/` | Native, extension, and runtime fixtures |
| `scripts/` | Dependency bootstrap, builds, packaging, and validation |

Credentials stay in the main app's Keychain. The App Group carries bounded commands and results, not raw audio. Typing data and Rime learning stay in the keyboard container; supplementary system words are not persisted. Result insertion validates the current document and records delivery to prevent stale replay.

Dependency sources are pinned in `bootstrap.sh`, `bootstrap-local-speech.sh`, and `build-predictions.py`. Preserve `Resources/Licenses/` when distributing the app. `collect-rust-licenses.py` refreshes attribution from the locked runtime dependency graph.

Generated frameworks, models, compiled dictionaries, logs, screenshots, signing configuration, and IPA archives are excluded from Git. Source assets, fixtures, lockfiles, and dependency notices are part of the project.
