# Linux egui / Tauri 2 parity tracker

This local stack is based on PR #1019 head `5f668b1d`. Linux ships one
`openless-linux-egui` executable and does not link Tauri, Wry, or WebKitGTK.
Windows, macOS, and Android remain on Tauri.

## Implemented on the native Core 2.0 path

- eframe/egui 0.33.3 shell, system CJK fallback, single instance, minimized
  startup, close-to-tray, safe shutdown, XDG autostart, notifications, external
  URLs, file dialogs, file logging, and diagnostic-log export
- fcitx5 dictation, QA, selection polish, translation, style switching, main
  window, style-pack direct, and applicable Coding Agent hotkeys; settings use
  strict collision checks and survive fcitx5 restart
- CPAL recording with canonical WAV archives, Core retention policy, history
  playback/export and failed-session retranscription
- Core-backed history search/delete/clear/copy/re-polish/retranscribe, vocabulary,
  pending corrections, shared vocabulary presets, correction rules, and complete
  style-pack CRUD/reset/prompt diagnostics/ZIP import-export/direct hotkeys
- provider channel CRUD/order/enable/activation, credential metadata, endpoint,
  model listing, validation, and revision-aware settings conflict merging
- Qwen catalog/download/cancel/delete/activate/prepare/preload/release/test and
  progress; Remote Input TLS service, URLs, PIN, locale, connection count, and
  error events
- Marketplace list/detail/install/download/upload/update/delete, likes, authored
  packs, GitHub device flow, polling, cancel, and logout
- dictation, QA text/voice, selection polish preview/confirm/cancel/revert, and
  Less Computer text/voice/stream/tool approval/cancel through high-level Core APIs
- #997 native QA/selection/capsule popup design: same-executable re-entry,
  versioned serde JSONL, Markdown, drag/Esc, QA microphone and Enter submit,
  session/sequence/kind guards, nonblocking pipes, crash restart, and snapshot replay
- stable/beta AppImage checks, delayed/hourly/manual scheduling, byte progress,
  SHA-256 and pinned minisign verification, same-directory fsync and atomic replace;
  deb/rpm use the release page
- Remote Input assets, Qwen vendor files, shared icons, version parsing, packaging,
  and release workflow are independent of `src-tauri`

## Automated evidence

- `cargo test -p openless-core --locked`
- `cargo test -p openless-linux-egui --locked`
- `cargo clippy --locked -p openless-linux-egui --all-targets -- -D warnings`
- PR #1019 Core/public-surface/dependency contract scripts
- fcitx5 C++ build plus `input_target_contract`
- `cargo tree` and release ELF `ldd` checks for Tauri/Wry/WebKitGTK
- Linux Tauri-free source, packaging, workflow, production-mock, capability, popup,
  settings-conflict, updater-validation, lifecycle, and staged-file gates

## Deliberate limits and device evidence still required

- Linux supports fcitx5 only; there is no IBus or global-hotkey fallback.
- Selection Voice remains hidden because the Linux production target/intent adapter
  is not implemented. It must not be advertised through capabilities. Selection
  polish and QA remain available.
- Generic Qwen is the Linux local runtime. Windows Foundry and Apple MLX are not
  Linux parity requirements.
- Foreground-application context, native post-insertion edit observation, system
  mute/restore, and start/stop sounds are not claimed by this stack. They remain
  explicit follow-up host-adapter work from PR #1019's L02/L03 register, rather
  than simulated UI data or false capabilities.
- X11 and Wayland device runs are still required for focus, Unicode insertion,
  popup positioning, tray, fcitx5 reload/rebind, microphone unplug/recovery,
  Secret Service, real phone Remote Input, real Qwen inference, and signed
  AppImage install/rollback. Ignored hardware tests or a green build are not
  recorded as device proof.

The automated scope is complete only when every command above passes at the
current PR #1019 head. The device-only rows remain “implemented, awaiting device
evidence” or “explicit unsupported” and must not be described as verified.
