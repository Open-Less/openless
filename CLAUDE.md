# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

OpenLess is a menu-bar/tray voice-input layer. Hold or toggle a global hotkey, speak, and the dictated text is polished and inserted at the current cursor in any app. Product principles, state machine, and module list live in `docs/openless-development.md` and `docs/openless-overall-logic.md` — read those before changing product behavior.

The repository contains **two parallel implementations** of the same product:

| Path | Stack | Status |
| --- | --- | --- |
| `Sources/`, `Tests/`, `Package.swift`, `scripts/`, `appcast.xml` | SwiftPM macOS-only (macOS 15+, Swift 5.9) | Legacy. Still ships Sparkle updates for `v*` tags so old users keep auto-updating. |
| `openless-all/app/` (no space) | Tauri 2 + Rust backend + React/TS frontend, macOS 12+ and Windows | **Active**. All current development happens here. |

The Tauri port is a faithful module-for-module rewrite of the Swift app. **The Swift original is the behavior authority — when Rust and TS disagree, Swift wins.** When porting, open the Rust file and the matching `Sources/OpenLess<X>/...` Swift file side by side. UI must match `openless-all/design_handoff_openless/*.jsx` pixel-for-pixel; the JSX is reference-only, never imported.

## Build, Run, Test

### Tauri (current — start here)

```bash
cd "openless-all/app"
npm ci

# Dev: vite at :1420 + tauri shell
npm run tauri dev

# Build .app (+ DMG) — use this script, not `tauri build` directly,
# because it threads Apple signing env vars and validates Info.plist.
./scripts/build-mac.sh           # build, sign, install to /Applications, reset TCC
INSTALL=0 ./scripts/build-mac.sh # build only

# Frontend-only TS check
npm run build   # = tsc && vite build

# Rust type-check without full compile
cargo check --manifest-path src-tauri/Cargo.toml
```

### Windows (cross-check only — no macOS runner in CI)

```powershell
# Preflight: verify toolchain
.\scripts\windows-preflight.ps1

# Build (requires Windows host or cross-compile target)
.\scripts\windows-build-gnu.ps1
```

Generated artifacts:
- `openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`
- `openless-all/app/src-tauri/target/release/bundle/dmg/OpenLess_<version>_aarch64.dmg`

Logs: `~/Library/Logs/OpenLess/openless.log` (macOS) / `%LOCALAPPDATA%\OpenLess\Logs\openless.log` (Windows).

There is no test runner wired in for the frontend. `src/lib/providerSetup.test.ts` is a hand-rolled assertion script — run with `npx tsx src/lib/providerSetup.test.ts` if you need it. Rust side has no `cargo test` targets yet; behavior is verified by running the app.

### Swift (legacy — only touch for Sparkle releases)

```bash
swift build
swift test
swift test --filter OpenLessCoreTests.PolishModeTests/<method>

./scripts/build-app.sh              # build .app, ad-hoc sign, embed Sparkle, reset TCC
RESET_TCC=0 ./scripts/build-app.sh  # keep TCC approvals
./scripts/release.sh <version>      # bump build-app.sh, sign zip, append appcast.xml, tag, gh release
```

Logs: `~/Library/Logs/OpenLess/OpenLess.log`.

## Architecture

`DictationCoordinator` (Swift) / `coordinator::Coordinator` (Rust) is the **single owner of session state**. Hotkey edges drive a small phase enum (`Idle → Starting → Listening → Processing`); recorder, ASR, polish, insertion, and history are wired here and nowhere else. Library/module code never calls across modules — they each depend only on shared types.

```
Swift (Sources/OpenLess*)        Rust (openless-all/app/src-tauri/src)        Purpose
─────────────────────────        ──────────────────────────────────────        ────────────────────────────────
OpenLessCore                     types.rs                                      Pure value types: DictationSession, PolishMode, HotkeyBinding, errors
OpenLessHotkey                   hotkey.rs                                     Global hotkey monitor (modifier-key edges)
OpenLessRecorder                 recorder.rs                                   Mic → 16 kHz mono Int16 PCM, RMS callback
OpenLessASR                      asr/{mod,frame,volcengine,whisper}.rs         ASR providers: Volcengine streaming WebSocket + Whisper HTTP
OpenLessPolish                   polish.rs                                     OpenAI-compatible chat completions (Ark / DeepSeek / etc.)
OpenLessInsertion                insertion.rs                                  AX focused-element write → clipboard + Cmd+V → copy-only fallback
OpenLessPersistence              persistence.rs                                History/preferences/vocab JSON + Keychain credentials
OpenLessUI                       src/components/Capsule.tsx                    Capsule view + state enum
OpenLessApp / DictationCoord.    coordinator.rs + commands.rs + lib.rs         State machine, IPC surface, tray icon, window plumbing
                                 permissions.rs                                TCC checks (Accessibility / Microphone)
                                 src/ (React)                                  Main window UI: Overview / History / Vocab / Style / Settings
                                 src/pages/_atoms.tsx                          Recoil atoms — global frontend state
                                 src/state/HotkeySettingsContext.tsx           HotkeySettings React context (capability + binding from backend)
```

### Dictation pipeline

```
hotkey edge (1st)  →  beginSession:  Recorder.start → ASR.openSession → BufferingAudioConsumer.attach
hotkey edge (2nd)  →  endSession:    Recorder.stop → ASR.sendLastFrame → awaitFinal → Polish → Insert → History.save
.cancelled         →  ASR.cancel, Recorder.stop, capsule .cancelled
```

Invariants:
- **Polish/ASR fallbacks are silent.** Missing Ark creds → insert raw transcript. Missing Volcengine creds → mock pipeline copies a placeholder. The contract is *"the user's words don't get lost"* — don't add hard errors here.
- **`BufferingAudioConsumer`** queues PCM until the WebSocket is ready, then drains. Recorder always pushes to it; ASR is attached after `openSession` resolves.
- **Hotkey is toggle-only**, not press-and-hold. The monitor yields one edge per modifier-key keydown; the coordinator interprets odd/even.

### Permissions, credentials, on-disk state

- **Bundle ID `com.openless.app`** is shared between Swift and Tauri builds (hard-coded in `scripts/build-app.sh`, `openless-all/app/src-tauri/tauri.conf.json`, and `CredentialsVault.serviceName`). Changing it breaks Keychain lookups *and* every existing TCC grant.
- **TCC**: Microphone + Accessibility + AppleEvents. Both apps declare `NSMicrophoneUsageDescription` / `NSAccessibilityUsageDescription` / `NSAppleEventsUsageDescription` in their Info.plist. Tauri's lives at `openless-all/app/src-tauri/Info.plist`. After a fresh build that resets TCC, the app must be **fully quit and relaunched** after granting Accessibility before the global hotkey tap installs.
- **Credentials** live in Keychain under accounts in `CredentialAccount` (`volcengine.app_key`, `volcengine.access_key`, `volcengine.resource_id`, `ark.api_key`, `ark.model_id`, `ark.endpoint`). The Rust port additionally reads the legacy plaintext fallback at `~/.openless/credentials.json` so users who configured the Swift app keep their creds without re-entering. Never hard-code keys.
- **Per-user data**:
  - macOS: `~/Library/Application Support/OpenLess/{history.json, preferences.json, dictionary.json}` — same paths as the Swift app, capped at 200 history entries. **Do not rename `dictionary.json` to `vocab.json`** (drops user data).
  - Windows: `%APPDATA%\OpenLess\`
  - Linux: `$XDG_DATA_HOME/OpenLess` (Tauri only)

### Release pipelines

Two separate flows, by design:

- **Swift (Sparkle, old users):** `scripts/release.sh <version>` bumps `build-app.sh`, builds the `.app`, ditto-zips it, signs with Sparkle EdDSA private key (Keychain item, not in repo), appends `<item>` to `appcast.xml`, commits, tags `v<version>`, pushes, and creates the GitHub Release. The public EdDSA key in `build-app.sh` (`SPARKLE_PUBLIC_KEY`) and the appcast URL `https://raw.githubusercontent.com/appergb/openless/main/appcast.xml` are baked into shipped clients — changing either strands existing users.
- **Tauri (cross-platform):** push a `v*-tauri` tag → `.github/workflows/release-tauri.yml` builds macOS arm64 `.dmg` and Windows x64 `.msi`. macOS Developer ID signing + notarization runs only when `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` secrets are set; otherwise it falls back to ad-hoc signing with a CI warning. Tauri tags use `-tauri` suffix specifically to not collide with Swift `vX.Y.Z` tags.

When bumping versions, update **both** `version` fields: `openless-all/app/package.json` and `openless-all/app/src-tauri/tauri.conf.json` (and `Cargo.toml`). For Swift releases, bump `APP_VERSION` *and* `BUILD_NUMBER` in `scripts/build-app.sh`.

## Repo conventions

- **Comments, log messages, user-facing strings, and most docs are in Simplified Chinese.** Match that when editing existing strings; new internal type/API names stay in English.
- **macOS hotkey monitor must use native `CGEventTap`, never `rdev`.** `rdev` synchronously calls `TSMGetInputSourceProperty` from non-main threads, which macOS 14+ aborts via `dispatch_assert_queue_fail` → SIGTRAP. The Swift impl uses CGEventTap; the Rust impl uses CGEventTap on macOS and `rdev` only on Linux/Windows. Don't unify them.
- **Don't `NSApp.activate` on the dictation path** — it steals focus and breaks insertion. The Tauri equivalent: only call `set_activation_policy(Regular)` + `activateIgnoringOtherApps` from `show_main_window` / mic-permission prompts, never from `start_dictation`.
- All public Swift API surface is `Sendable`; UI/coordinator is `@MainActor`; audio/ASR/insertion classes that bridge C APIs are `@unchecked Sendable` with explicit locks. The Rust port mirrors this with `Arc<Mutex<...>>` (parking_lot) wrappers — keep the locking discipline when adding fields.
- Swift libraries depend only on `OpenLessCore`. Rust modules depend only on `types.rs`. New cross-module wiring goes in `DictationCoordinator` / `coordinator.rs`, not in the leaf modules.

### Adding a new module

Tauri (preferred):
1. Add a `<name>.rs` (or directory) under `openless-all/app/src-tauri/src/`, importing only from `types`.
2. Register it in `lib.rs` (`mod <name>;`).
3. Wire it into `coordinator.rs` and expose any frontend-callable surface via `commands.rs` + `invoke_handler!`.
4. Add the matching TS wrapper in `openless-all/app/src/lib/ipc.ts` (with a mock branch for browser dev).

Swift (only if also patching the legacy app):
1. Add target in `Package.swift` under `Sources/OpenLess<Name>`, depending only on `OpenLessCore`.
2. Add it to `OpenLessApp`'s dependency list and wire it in `DictationCoordinator`.
3. Add `Tests/OpenLess<Name>Tests` for pure logic.
