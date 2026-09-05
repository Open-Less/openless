# Linux egui / Tauri 2 parity tracker

This branch follows PR #1019 and keeps Linux on the single
`openless-linux-egui` executable. Linux does not link Tauri, Wry, or WebKitGTK.

## Working on the native Core 2.0 path

- eframe/egui 0.33.3 shell with system CJK fallback
- fcitx5 hotkeys, insertion, selection ownership, and single-instance forwarding
- dictation, QA, selection polish, Less Computer, providers, credentials, Remote Input, and local ASR
- history search/delete/clear/copy/re-polish
- vocabulary and deterministic correction CRUD
- style-pack list/activate/enable/delete and Core-owned runtime prompt diagnostics
- Marketplace search/install/like and GitHub device-flow login
- typed, versioned JSONL QA/selection/capsule popup processes with sequence/session guards
- XDG autostart, desktop notification, external URL, and atomic file-save adapters
- verified AppImage replacement primitives that preserve the old image on validation failure

## Still required before declaring full Tauri 2 parity

- native tray lifecycle and close-to-hide behavior
- wire autostart into `SettingsEffectPlan` prepare/commit/rollback
- history recording playback/export and failed-recording retranscription
- vocabulary presets and pending-correction accept/reject card
- style-pack create/edit/reset/import/export and style hotkey editing
- Marketplace detail/download/upload/my-packs/my-likes/delete views
- updater HTTP scheduling/progress and a pinned minisign verifier; until then
  `supports_auto_update` remains false
- X11 and Wayland smoke tests for focus, insertion, popup positioning, tray, and fcitx5

Windows, macOS, and Android remain on Tauri. Linux-only work must not move
platform lifecycle policy into `openless-core`.
