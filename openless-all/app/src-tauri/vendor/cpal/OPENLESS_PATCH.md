# OpenLess Windows WASAPI lifetime patch

This directory vendors the complete official `cpal 0.15.3` crates.io package,
including its Apache-2.0 license and original dependency lockfile. Package version
and non-Windows backends are unchanged. `OPENLESS_UPSTREAM.json` records the source
archive, upstream commit and verified SHA-256:

`873dab07c8f743075e57f524c583985fbaf745602acbe916a01539364369a779`

The archive hash was checked against the crates.io version metadata before
extracting. Only `src/host/wasapi/com.rs`, `device.rs`, `stream.rs`, and `.gitignore`
from the archive are modified. The `.gitignore` change retains the existing
`Cargo.lock` in this repository so the standalone Windows checks can use
`--locked`. The added files are this note, upstream identity metadata and
`tests/windows_lifecycle.rs`.

## Scope and evidence

This fixes independently identified COM ownership/lifetime defects in the pinned
WASAPI implementation. It does **not** establish that those defects caused
OpenLess issue #1227, and does not claim its `0xc0000374` crash is resolved. The
same dependency code existed in OpenLess 1.3.18. Original-machine crash evidence
and controlled before/after runs remain necessary.

The original backend cached one `IMMDeviceEnumerator` process-wide although its
COM initialization was thread-local, and allowed `Device`/`Devices` to cross
threads with `unsafe impl Send/Sync`. It also acquired capture/render/clock
services on a stream builder's thread and released them on an audio thread that
did not itself initialize COM.

Microsoft's interface contracts require a COM-initialized audio thread and
require each service's `Release` on the thread that called `GetService`:

- [IAudioClient](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient)
- [IAudioCaptureClient](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudiocaptureclient)
- [IAudioRenderClient](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudiorenderclient)
- [IAudioClock](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclock)
- [Single-threaded apartments](https://learn.microsoft.com/en-us/windows/win32/com/single-threaded-apartments)

Upstream [cpal#1302](https://github.com/RustAudio/cpal/issues/1302) discusses related
COM affinity problems. Its reported access violation is distinct from the
OpenLess heap-corruption report; the shared dependency does not prove causality.

## Ownership and failure behavior

- Enumeration initializes COM in the calling thread and creates a local
  enumerator. MMDevice/collection objects never enter a process-wide cache.
- WASAPI `Device` and `Devices` no longer implement `Send` or `Sync`. Their dynamic
  platform wrappers inherit this restriction. The application must transfer
  owned device descriptions, not MMDevice pointers.
- Building a stream transfers only an owned UTF-16 endpoint ID, configuration,
  and user callbacks to `cpal_wasapi_in` or `cpal_wasapi_out`.
- The worker initializes COM, resolves its own Device, activates and initializes
  its own AudioClient, obtains capture/render/clock services, and runs callbacks.
  No COM interface crosses the thread boundary. The existing STA initialization
  convention is retained; an already initialized caller may reuse its apartment.
- A startup handshake returns success only after native initialization succeeds.
  Native startup failures return `BuildStreamError` synchronously. A startup
  panic closes the handshake and becomes a builder error after the thread is
  joined. There is no detached native cleanup on those paths.
- Every acquired audio packet has a release guard. Normal release errors do not
  cause a second release. Timestamp errors and callback unwinding return the
  input packet; interrupted output packets are released as silent.
- A zero-frame `AUDCLNT_S_BUFFER_EMPTY` result is handled before reading the data
  pointer or timestamps. It is a successful HRESULT with no acquired packet;
  the callback and `ReleaseBuffer` are skipped for that result.
- Worker shutdown first releases any acquired packet, then stops a playing
  client. `StreamInner` field order releases capture/render and clock interfaces,
  then AudioClient, then its audio event. Partial-construction local ordering
  provides the same client-before-event lifetime on initialization failures.
- The COM guard is thread-local and is destroyed after worker stack values,
  including all interfaces, during both normal exit and panic unwinding.
- Controller destruction sends termination and joins even if the command
  receiver has already disconnected after a callback failure. It does not
  propagate a worker panic from its destructor. The shared kernel control event
  is closed only after the worker can no longer wait on it. Kernel events are
  the only new explicit `Send/Sync` wrapper; they contain no COM interfaces.

## Verification

From the repository root, the Windows CI tests are:

```sh
cargo test --manifest-path openless-all/app/src-tauri/vendor/cpal/Cargo.toml \
  --lib --test windows_lifecycle --locked
```

The integration target launches fresh child processes for the first-thread-exit
and concurrent-enumeration scenarios. Empty endpoint lists still exercise COM
enumerator/collection creation and teardown. It also enforces that the public
Device/Devices wrappers cannot cross threads at compile time. Unit tests cover
startup errors, startup panic, COM availability during stack cleanup, and packet
release on success, release failure and callback unwinding.

Real audio-device tests are explicitly ignored by default:

```sh
cargo test --manifest-path openless-all/app/src-tauri/vendor/cpal/Cargo.toml \
  --test windows_lifecycle --locked -- --ignored --nocapture
```

They require input/output hardware and microphone permission. Missing devices,
missing callbacks or unsupported fixture formats fail an explicitly requested
hardware test; they are not reported as successful coverage. Input tests cover
normal pause/drop, direct cancellation and callback panic. Output tests repeatedly
start and stop silent playback. Each child has a bounded timeout to expose hangs.

On the implementation host (macOS, 2026-10-25), the latest patched library and all
Windows test targets passed cross compilation with Rust 1.98.0:

```sh
cargo check --manifest-path openless-all/app/src-tauri/vendor/cpal/Cargo.toml \
  --target x86_64-pc-windows-gnu --lib --tests --locked --offline
```

This was a compile check, not execution of Windows tests. No Windows runtime,
real microphone or issue #1227 crash reproduction was available on this host.
The application-level macOS tests and Windows CI are tracked by the enclosing
OpenLess change, not claimed here as local WASAPI runtime validation.
