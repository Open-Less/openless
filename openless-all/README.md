# OpenLess All-Platform

This is the current cross-platform OpenLess workspace.

## App Directory

The runnable sources live in `app/` with three explicit layers:

- `app/crates/openless-core`: framework-independent shared backend Interface and business rules;
- `app/src-tauri`: macOS, Windows, and Android Tauri Adapter plus the React frontend;
- `app/linux-egui`: Linux non-UI Adapter consumed by the separately developed egui frontend; it does not depend on Tauri or WebKitGTK.

The macOS Tauri build links a vendored C ASR engine (`Open-Less/qwen-asr`, forked from `antirez/qwen-asr`) tracked as a git submodule under `app/src-tauri/vendor/qwen-asr/`. The root core/Linux workspace excludes `src-tauri`, so Linux checks do not need that submodule.

```bash
# macOS Tauri development only — pull in vendored submodules
git submodule update --init --recursive

cd app
npm ci
npm run tauri dev
```

## Shared backend and Linux host

The egui UI is owned by another team. This repository supplies its typed Rust Interface, semantic events, fixtures, and Linux non-UI Adapters. The checked-in `linux-egui/src/main.rs` remains a stub until that team lands `eframe::App`; do not treat the current binary as a production application.

```bash
cd app
cargo test -p openless-core
cargo test -p openless-linux-egui --all-targets
pwsh ./scripts/check-core-deps.ps1
pwsh ./scripts/check-core-deps.ps1 openless-linux-egui
```

The independent Linux package workflow is `.github/workflows/release-linux-egui.yml`. It builds deb/rpm/AppImage and the fcitx5 plugin without WebKitGTK, but deliberately has no automatic tag trigger until the real egui entry point is present.

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
npm run check:hotkey-injection
```

### Windows Runtime Notes

- Windows does not need the macOS Accessibility permission. Use Settings ->
  Permissions -> Global hotkey to inspect listener status.
- Microphone permission is checked by opening a short-lived input stream, so a
  device-format query alone is not treated as permission granted.
- Text insertion through `Ctrl+V` is treated as copy fallback unless the app can
  confirm insertion.

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

## Ignored Local Output

The following are intentionally local-only:

- `app/node_modules/`
- `app/dist/`
- `app/target/`
- `app/src-tauri/target/`
- `app/src-tauri/gen/`
- `.DS_Store`
