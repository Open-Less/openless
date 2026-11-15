# OpenLess Android platform code

[English](README.md) | [简体中文](README.zh.md)

Unified entry point for the Android-related Rust, Kotlin, and frontend code. The desktop build is layered out via `#[cfg(not(mobile))]` and is unaffected.

## Directory structure

```text
android/
├── kotlin/              # Kotlin 模板（CI 复制到 gen/android/）
├── manifests/           # AndroidManifest snippet + res/xml
└── frontend/            # React 模块（Vite 别名 @android）

src-tauri/src/android/   # Rust 运行时模块（crate::android）
```

## Rust (`src-tauri/src/android/`)

| Module | Responsibility |
|------|------|
| `jni.rs` | JNI helpers (clipboard, overlay service, accessibility) |
| `native_bridge.rs` | Kotlin ↔ Coordinator JNI entry point |
| `overlay.rs` | Overlay permission and show/hide |
| `accessibility.rs` | Accessibility service status and paste |
| `shizuku.rs` | Shizuku status diagnostics and controlled accessibility recovery |
| `insert.rs` | Cross-app text insertion strategy |
| `updater.rs` | In-app update (manifest fetch, minisign verification, system installer) |
| `updater_logic.rs` | Pure functions for update URL / version comparison (testable on all platforms) |
| `types.rs` | Android preference and status types |

The main crate pulls it in via `mod android;`, and common APIs are re-exported flat from `crate::android::`.

## Kotlin (`android/kotlin/`)

After `tauri android init`, [`scripts/copy-android-scaffolding.mjs`](../scripts/copy-android-scaffolding.mjs) copies it into `src-tauri/gen/android/app/src/main/java/com/openless/app/`.

Manifest merge scripts:

- [`scripts/merge-android-v1-manifest.mjs`](../scripts/merge-android-v1-manifest.mjs) — microphone permission (`android/manifests/AndroidManifest.v1.snippet.xml`)
- [`scripts/merge-android-overlay-manifest.mjs`](../scripts/merge-android-overlay-manifest.mjs) — overlay / accessibility
- [`scripts/merge-android-shizuku-manifest.mjs`](../scripts/merge-android-shizuku-manifest.mjs) — Shizuku provider / authorization activity
- [`scripts/patch-android-shizuku-deps.mjs`](../scripts/patch-android-shizuku-deps.mjs) — Shizuku Gradle dependencies

### Rime Pinyin input (Pinyin, optional mode)

Long-pressing the spacebar on the English keyboard switches between English and Pinyin. The Pinyin path is now `RimePinyinController` → `OpenLessRime` JNI → `mobile-ime/rime_engine.cpp` / librime 1.17.0 shared with HarmonyOS; Fcitx5 is not introduced. Encoding and candidates are produced by Rime, the spacebar commits the candidate, and after confirmation it is submitted through the InputConnection. Password fields do not enter Pinyin, and learning is prohibited by using a separate scheme whose user dictionary is disabled.

`npm run tauri:android:build` compiles Rime for the target ABIs, generates the JNI libraries, and copies the offline dictionaries into the generated Android project before the Tauri build. It requires `ANDROID_NDK_HOME`; the cache defaults to `/data-win/openless-cache/mobile-rime`. To verify the arm64 engine on its own: `python3 scripts/build-mobile-rime.py android --ndk /path/to/ndk`. Before building directly with Gradle, run that script with `--stage`.

The old `LitePinyin*` lookup, sorting, and learning algorithms and the old built-in Pinyin TSV have been removed; the build scripts delete same-named leftover files in the generated project. `LegacyPinyinBackup` only retains compatibility with the old import/export format and takes no part in candidates or training. Learning and the personal dictionary are now implemented natively by Rime, reusing `pinyin_simp.userdb` in the persistent directory; ordinary input fields enable learning, while private fields use a separate scheme that does not read the personal dictionary.

## Frontend (`android/frontend/`, alias `@android`)

| Path | Responsibility |
|------|------|
| `lib/androidTypes.ts` | Android preference and status TS types |
| `lib/androidIpc.ts` | overlay / accessibility / Shizuku Tauri invoke |
| `lib/androidMicrophonePermission.ts` | WebView microphone permission helper |
| `components/AndroidPermissionsPanel.tsx` | Settings page Android permissions and overlay configuration |

`src/lib/types.ts` and `src/lib/ipc.ts` keep their re-exports, so existing import paths still work.

## Recent stroke input method IME updates (`OpenLessImeService.kt`)

The panel height is fixed at 300dp (enforced by `SwipeModeContainer.onMeasure()`). Inside the stroke panel, the encoding area at 24dp + the candidate area at 36dp (60dp combined) share the remaining height with the key area below, and no relayout happens in any input state.

| Feature | Description |
|------|------|
| Gestures | The horizontal swipe threshold for switching panels goes from 72dp→100dp; a new downward swipe of ≥120dp hides the keyboard (`hideKeyboardPanel()`); panel switches have a slide-in animation (`refreshInputView(slideDirection)`) |
| Encoding area / candidate area | Fixed height, flat background + separator line (`buildEncodeAreaBackground()`); the selected/first candidate is now the same light blue as the encoding text + bold (`strokeEncodeAccentColor`) instead of red; candidates that overflow the area expand in a floating layer via `showCandidateOverlay()` without pushing the keys |
| Center 3×4 stroke keys | Spacing tightened to ~2dp (key margins `setMargins(dp(1)...)`), and row margins now align with the red function-key column on the right and the punctuation column on the left |
| Keyboard settings | Long-press the logo to open the full-screen native settings page `OpenLessKeyboardSettingsActivity` (vibration intensity/duration first, more items can be added later) |
| Language sync fix | `OpenLessApplication` used to check `MainActivity` by exact type, but the settings page actually runs on the subclass `OpenLessBackendWarmupActivity`, so it never triggered; changed to an `is` check |
| Clipboard | Added history persistence `OpenLessClipboardHistory.kt`, with button colors unified with the stroke panel |
| Voice correction integration | `native_bridge.rs` adds `nativeAddCorrectionRule`, so manually corrected dictation results are automatically written to the correction dictionary |

Development workflow: after each change, sync with `npm run copy:android-scaffolding` → `gradlew app:assembleArm64Debug -x app:rustBuildArm64Debug` (Kotlin-only changes skip the Rust rebuild) → install on device with `adb install -r` → verify the real-device result via `adb exec-out screencap` or user-provided screenshots; when sizes are disputed, use `adb shell wm density` + measured px to derive dp instead of guessing at layout problems.

## Build and CI

**CI (overlay / accessibility ADB test APK)** — merges the v1 microphone permission + overlay / accessibility manifests, for testing the full overlay and accessibility capabilities (not just in-app dictation) with ADB on a real device:

```bash
cd openless-all/app
npm ci && npm run build
CI=true npm run tauri -- android init --ci
node scripts/copy-android-scaffolding.mjs
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
CI=true npm run tauri:android:build
```

Workflow: [`.github/workflows/android-apk.yml`](../../../.github/workflows/android-apk.yml)

**Local overlay / accessibility development (v3)** — the same manifest merge chain as CI, using the local init / copy scripts:

```bash
cd openless-all/app
npm run tauri:android:init
npm run copy:android-scaffolding
node scripts/merge-android-v1-manifest.mjs
node scripts/merge-android-overlay-manifest.mjs
node scripts/merge-android-shizuku-manifest.mjs
node scripts/patch-android-shizuku-deps.mjs
npm run tauri:android:build
```

## Related documentation

- [docs/android-mobile-apk-overlay-plan.md](../../../docs/android-mobile-apk-overlay-plan.md) — phased product plan
