# Linux frontends and system API audit

## Runtime selection

- `openless` or `openless --frontend=qt`: Qt6 WebEngine (default).
- `openless --frontend=gtk`: GTK4 + WebKitGTK (secondary).
- `openless --frontend=gtk --minimized`: explicit GTK background startup.
- `openless --help`: launcher options.

Both shells render the same React build and start the same `openless-linux-web`
backend. There is one `openless.lock` and `openless-host.sock` per runtime directory.
Starting a second frontend raises the existing window rather than registering a
second tray/hotkey listener or creating concurrent recording/data writers. To
switch toolkit, quit the current instance first. Login entries use the default
launcher (Qt), not the last toolkit used for an explicit manual launch.

GTK is built with GTK >= 4.12, WebKitGTK 6.0 and libsoup 3. GTK4 layer-shell is
optional at build time: without it, the main window works but overlays are disabled
and a warning is logged. On desktops without the layer-shell compositor protocol,
use Qt for the full overlay experience. Missing GTK runtime libraries do not affect
the default Qt executable; distro packages recommend the GTK dependencies.

Build from `openless-all/app`:

```sh
cmake -S ../linux/gtk -B target/linux-gtk-build -DCMAKE_BUILD_TYPE=Release
cmake --build target/linux-gtk-build --parallel
```

The shared native document-start bridge is `linux/shared/web_host_bridge.h`.
Clipboard ownership and capsule hit testing are view-local native operations,
not global backend state. Qt uses QClipboard plus Wayland input masks / X11
ShapeInput; GTK uses GdkClipboard and GdkSurface input regions. Only actual capsule
controls and open menus accept input; transparent padding and the waveform do not.
The desktop content frame is square beneath native decorations; internal cards
retain their normal corners.

## Recent system API audit

### Implemented in the shared Linux HTTP dispatcher

- Application style routing: get/refresh catalog, assign style, clear seen history.
  Delegates to Core with validation and persistence; no duplicate routing store.
- Input application icons and activity icons: local desktop files/PNG icons only;
  no desktop `Exec` execution, arbitrary network icon fetch, or whole-system scan.
  Missing icons are an empty optional map, not a fake application identity.
- Native directory and ZIP open/save pickers: `kdialog`, then `zenity` when the
  former is absent. User cancellation is null; missing tools/failures are errors.
- Recording export: validate session UUID and history membership, use quick-note
  export directory only for quick notes, otherwise prompt for a destination; atomic
  save rejects symlink destinations.
- External HTTP(S) URLs: shared validated desktop launcher, not an unhandled popup.
- Microphone prompt: real platform adapter request, not an unconditional success.
- Restart: event to the active shell; shell stops/reaps its backend, releases the
  singleton lock, and starts a replacement. Ordinary close remains close-to-tray;
  explicit tray quit bypasses that veto.
- Copy buttons (including History): native clipboard of the focused Qt/GTK view.
  The fcitx5 clipboard cache is a best-effort side effect, not proof of a successful
  system clipboard write. Older Qt shells can use their injected writeText shim.

### Not complete: do not advertise as supported

1. **New Ask host**: `ask_*` session/attachment/editor/context APIs still live
   in the Tauri host. Linux package 236 restores `window=qa` as a shared React
   view of the existing Core QA service, with text/voice questions and cancellation,
   accessible from Tools. This is not the separate newer Ask entry, whose client
   still imports Tauri directly. Full parity requires extracting Tauri session and
   attachment coordination behind a framework-independent host service and adding
   Linux HTTP/event and native-focus adapters. Do not map missing methods to null
   or claim that the basic QA view implements attachments or context permissions.
2. **Automatic per-app style selection**: Linux now freezes an fcitx5 target before
   Core's asynchronous startup and supplies `linux:<program>` identity from that
   same ticket for style routing and insertion. Program metadata never reads the
   document; ticket-scoped cursor reads respect the opt-in flag and sensitive-app
   gate. Previously seen apps can be explicitly assigned a style in the shared UI.
   Catalog category auto-classification still needs Linux identifiers; macOS bundle
   or Windows executable categories must not be guessed. An old running fcitx5
   addon lacks the new metadata methods until it is reloaded.
3. Ask browser-tab/window enumeration, scoped read consent, screenshots/file
   attachments and voice editor ownership need Linux-specific implementations and
   privacy checks. Persisting an enabled consent switch alone is not support.
4. Local ASR, voice activation, Windows caption/TSF and mobile-only APIs remain
   platform-gated; adding same-name no-op methods would mislead the shared UI.

## Verification

- `node scripts/linux-multi-frontend.test.mjs`: default/explicit frontend selection,
  invalid arguments, shared clipboard/input bridge, shared singleton contract.
- `playwright test e2e/linux-system-interactions.spec.ts
  e2e/linux-surface-transparency.spec.ts`: actual frontend copy entry, local region
  publisher, square desktop content frame, preserved card corners, transparent
  auxiliary roots, capsule controls/empty cleanup (Chromium + WebKit).
- `cargo test -p openless-linux-web`: real Core routing state/validation, picker
  cancellation semantics, capsule payload and voice-edit arming.
- Immutable frontend asset snapshot test: replace/delete installed lazy chunks after
  backend startup and prove that the running process retains its complete old build.
- Production usage initialization/notification, metadata-only frozen target capture,
  credential namespace and Linux multimodal recovery regressions are covered.
- Native isolated virtual KWin test: both shells loaded pages, wrote clipboard
  bytes verified with `wl-paste`, applied native input regions, accepted secondary
  toolkit handoff and exited through the tray event. This is not a full desktop
  dictation/Ask test or proof of every compositor's click-through geometry.

The existing AOCI index has unresolved governance drift and is not aligned with
these changes. No index semantics were synthesized from filenames and no forced
rescan was used to erase that drift.
