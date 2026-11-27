# Linux system interaction audit (package 236)

## Completed in this pass

- Conflict review includes bounded local/cloud style names, not only repeated
  category labels. No credentials, prompts, icon payloads or history are exposed.
- Ordinary errors and warnings expire after eight seconds. In-flight work,
  action-bearing notices and explicitly pinned messages retain their lifetime.
- Permissions displays an Fcitx5 plugin diagnostic instead of claiming desktop
  global shortcut registration. Disk installation, existing daemon, loaded addon,
  insertion, shortcuts and metadata support are separate facts. Refresh resolves
  the unique existing D-Bus owner before introspection and never calls installation,
  restart, activation, input-method selection, document reads or audio capture.
  The legacy get_ime_backend_status endpoint now uses the same passive probe.
- Linux application-style routing is enabled in navigation (the backend alone
  was not enough while the frontend still restricted it to macOS/Windows).
- Tools -> Ask opens the native QA window. The removed `window=qa` route now
  renders a real shared-React view, not the main app. The existing Core QA service
  owns text questions, voice recording, streamed state, cancellation and answers.
  Both shells honor panelOpen even when its phase is idle. Keyboard focus is
  on-demand; opening a view does not start the microphone.
- Correction Add button is label-sized and centered rather than stretching a dark
  background across its grid cell. This fixes the demonstrated layout condition;
  a remaining compositor-specific black artifact requires a screenshot/native test.
- Missing set_skip_auto_style_hotkey is wired through validated preferences,
  settings effects, fcitx5 registration/signal and Core's session-bound toggle.
- Capsule auxiliary replay uses real pending corrections and processing dictations.
  Linux caption tracing is disabled explicitly, instead of throwing at startup.

## Shortcut chain

The following have preference/registration -> addon signal -> shared backend action:

| Shortcut | Action |
| --- | --- |
| Dictation | Core press/release/combined edge dispatch |
| Quick note | Start/stop note or redirect active capture |
| Ask | Toggle QA recording |
| Selection polish | Scoped selection polish / armed voice edit |
| Translation | Arm next capture or update the active one |
| Switch style | Previous style pack |
| Skip auto style | Toggle automatic/skip-auto for the active mutable session |
| Open app | Show main window |
| Individual style pack | Select stable pack ID |

Standalone style/skip-auto modifiers fire on release only when not combined with
another key. Repeat suppression prevents holding a combination from toggling back
and forth. Fcitx5 can observe these only in clients connected to its input context;
this is not an unconditional compositor-wide global binding. The probe says so.
Less Computer/coding-agent shortcut actions remain unavailable: their execution
runtime is not implemented on Linux, and registering no-op shortcuts is not support.

## Remaining boundaries

An intermediate literal-call inventory found 267 frontend command names and 81
absent dispatcher names before the final follow-up fixes. This is an investigation aid, not a semantic assertion that
81 Linux features were broken: it includes mobile/Windows-only commands, guarded
paths and frontend-native operations. The pass fills the relevant missing endpoints
above; it does not replace missing native engines with successful empty responses.

- New standalone Ask host (`ask_*` entry settings, browser/file/screenshot context,
  attachment storage, consent and session history) is still Tauri-coupled. The QA
  view restored here is explicitly the existing shared Core QA service, not a claim
  that all of the new Ask host is implemented. Unsupported context-consent UI is
  hidden on Linux rather than issuing missing permission commands.
- Local ASR/download/Foundry/Sherpa and voice-activation engines remain gated.
- Full-field voice-edit session creation/finalization is not implemented; Linux's
  existing selection-only view gates those controls rather than invoking them.
- Android audio/update/migration, Harmony exports, Windows caption/IME and SIWC
  commands are not Linux capabilities.
- set_capsule_interactive_regions is fulfilled by the injected view-local native
  bridge; it must not be replaced with a backend no-op.
- handle_window_hotkey_event is not a compositor-wide Linux fallback. Full shortcut
  coverage outside Fcitx5 clients would require a separate desktop/portal adapter.
- Linux app-specific assignments use linux:<program> identity. Catalog category
  classification still needs explicit Linux identifiers.

## Regression evidence

- Private D-Bus test with an activatable Fcitx5 service proves that diagnostics do
  not run its activation executable. Neither the test nor settings refresh touches
  the user's input-method selection.
- Core label tests limit disclosure to 100 display characters of style names.
- Notification fake timers cover errors, warnings, saving, actions and pinned items.
- Chromium/WebKit fixtures cover distinct conflict names, automatic warning expiry,
  actual QA view dispatch/render and correction-button geometry. QA fixture answers
  are simulated; they do not verify a remote model service or billable voice request.
