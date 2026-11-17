# Issue #1223: playback-preserving acoustic echo cancellation

**Status:** implementation and feasibility note; the baseline research below is supplemented by the feature-branch implementation.
**Baseline revision examined:** `de072a88373a222280a8c22948092a260880196c` (`upstream/beta`, 2.0.2 Beta.1).
**Issue:** [Open-Less/openless#1223](https://github.com/Open-Less/openless/issues/1223)

## Executive conclusion

Issue #1223 is technically feasible, but not as a platform-neutral switch around the existing microphone normalizer. Acoustic echo cancellation (AEC) needs two time-related signals: the microphone/capture signal and a render/playback reference, plus a usable estimate of the delay between them. The official WebRTC API exposes this explicitly through `ProcessReverseStream()` for render audio, `ProcessStream()` for capture audio, and `set_stream_delay_ms()` for their timing relationship ([WebRTC Audio Processing API](https://webrtc.googlesource.com/src/+/refs/heads/main/api/audio/audio_processing.h)). A microphone-only RMS gate or gain stage can reduce quiet noise, but it cannot reliably identify and subtract speech from a video.

The practical recommendation is an **optional, capability-reported, best-effort feature** that leaves playback and volume untouched. Keep “Mute while recording” as the explicit fallback. Implement and validate one platform at a time rather than promising universal AEC:

1. Add a capability probe, diagnostics, fallback behavior, and an offline/fixture test harness.
2. Prototype Windows with WASAPI loopback plus a proven AEC implementation such as WebRTC APM/AEC3.
3. Add a PipeWire-specific Linux path where the echo-cancel virtual sink/source can own the playback reference.
4. Investigate macOS VoiceProcessingIO or a supported Core Audio render tap; do not claim that VoiceProcessingIO cancels arbitrary applications whose playback is outside that unit.
5. Add Android’s native `AudioRecord`/`AcousticEchoCanceler` path only where the device reports the effect as available.

## 1. Issue and GitHub history

### User request

The issue asks for an optional playback-suppression/AEC setting so a video can keep playing while OpenLess recognizes the user’s speech. It explicitly accepts device- and environment-dependent results and asks that supported platforms and limitations be documented ([issue body](https://github.com/Open-Less/openless/issues/1223)).

The two issue comments are:

- On 2026-10-17, `HKLHaoBin` explained that AEC was omitted from the earlier audio-enhancement work because of CPU cost, additional dependencies, and implementation/maintenance complexity, and offered to try an implementation ([comment 6237559767](https://github.com/Open-Less/openless/issues/1223#issuecomment-6237559767)).
- On 2026-10-17, the reporter said a closed-source competitor appears to preserve video playback while preventing the video voice from entering recognition ([comment 6237637338](https://github.com/Open-Less/openless/issues/1223#issuecomment-6237637338)). This is a user observation, not an independently verified technical claim.

`gh api --paginate repos/Open-Less/openless/issues/1223/events` returned an empty list. The issue timeline contained only the two `commented` entries above; it contained no cross-reference, linked-commit, or pull-request event. `gh search prs 1223 --repo Open-Less/openless` and `gh search commits 1223 --repo Open-Less/openless` returned no results at investigation time. The issue body does explicitly reference [#269](https://github.com/Open-Less/openless/issues/269), so that issue and its implementation PR are relevant context, but they are not direct links from the #1223 timeline.

### Related repository history

- [#269](https://github.com/Open-Less/openless/issues/269) requested pausing and restoring background media. It is closed.
- [PR #274](https://github.com/Open-Less/openless/pull/274) implemented the current “Mute while recording” behavior. Its description says the implementation mutes the system output instead of pausing each media application. Its automated review recorded the important limitation: media continues advancing while muted, and macOS/Linux needed real-device verification. The resulting commits were `9511eb6`, `bb0fefc`, and `78a3846`.
- [PR #1194](https://github.com/Open-Less/openless/pull/1194), merged for #1026, added opt-in RMS/downmix microphone enhancement and explicitly documented that it does **not** include echo cancellation. Its merge commit is `44972015349cac3a52c4a28190675554a8986fcb`.

## 2. Current audio architecture

The following bullets describe the committed code at the baseline revision above; they explain why the feature branch adds both a shared DSP seam and host-specific reference capture:

- `openless-all/app/crates/openless-core/src/audio.rs` owns the canonical 16 kHz, mono, signed-16-bit PCM normalization contract. The baseline `AudioProcessingConfig`/`AudioProcessor` path implemented optional microphone enhancement/noise gating but had no render-reference input or AEC state ([file at baseline revision](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/crates/openless-core/src/audio.rs)).
- `openless-all/app/src-tauri/src/recorder.rs` opens a `cpal` input stream, converts the selected microphone format to `f32`, then sends microphone callback data through the processor/normalizer. The baseline had no corresponding render/loopback stream in this adapter ([file](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/src-tauri/src/recorder.rs)).
- `openless-all/app/linux-egui/src/audio.rs` likewise owned a `cpal` microphone input stream and forwarded normalized chunks. The baseline had no playback-monitor/reference capture path ([file](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/linux-egui/src/audio.rs)).
- `openless-all/app/src-tauri/src/audio_mute.rs` is an output-mute guard: Windows uses `IAudioEndpointVolume`, macOS invokes `osascript` to set output mute, and Linux tries `wpctl` then `pactl`. It records and restores mute state, but it does not observe or process the render signal ([file](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/src-tauri/src/audio_mute.rs)).
- `openless-all/app/crates/openless-core/src/ports.rs` states that each host owns device selection, permissions, resampling, and native stream implementation. This is the seam used by the feature branch for host-specific render-reference capture while keeping cancellation inside the existing PCM pipeline ([file](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/crates/openless-core/src/ports.rs)).

### Feature-branch implementation boundary

The feature branch adds a session-frozen, opt-in `echo_cancellation_enabled` preference, excluded from cloud sync because it is host/device dependent. `ReferenceAudioBuffer` keeps at most two seconds of downmixed render samples and bypasses cancellation on startup starvation or underrun. `AudioProcessor::process_with_reference` resamples microphone and reference data to the canonical rate, applies a bounded adaptive canceller before the existing microphone enhancement, and preserves the old microphone-only path when the switch is off or a reference is unavailable.

The Tauri adapter uses cpal 0.18’s render-endpoint input path on desktop targets where the backend supplies loopback (WASAPI and cpal’s macOS CoreAudio loopback support). The Linux egui adapter attempts PipeWire sink capture and PulseAudio monitor sources; ALSA remains microphone-only. All reference samples remain transient and are not archived or transmitted. Any unavailable, invalidated, or runtime-failed reference leaves ordinary recording running without changing system volume or media playback state. The independent `mute_during_recording` option remains unchanged and is still the explicit output-mute fallback.

## 3. What AEC requires

### Render reference and timing are mandatory inputs

The official WebRTC Audio Processing API describes approximately 10 ms linear PCM frames, sends the reverse/render stream to `ProcessReverseStream()`, sends microphone/capture audio to `ProcessStream()`, and exposes a stream-delay setting for the timing relationship. Its AEC configuration is therefore fundamentally different from a capture-only gain or noise gate ([WebRTC API header](https://webrtc.googlesource.com/src/+/refs/heads/main/api/audio/audio_processing.h)). WebRTC’s AEC3 interface similarly describes 10 ms band-split frames, a render-analysis path that stores the render signal, and a capture-processing path that removes estimated echo ([AEC3 header](https://webrtc.googlesource.com/src/+/refs/heads/main/modules/audio_processing/aec3/echo_canceller3.h)).

A FIFO of samples is not automatically a valid reference. The render and capture clocks can differ, the acoustic path has an unknown delay and impulse response, and output routing can change. The implementation must either use a platform voice-processing unit or maintain timestamps/delay and bounded drift handling in a software AEC path.

### Why the existing enhancement is insufficient

PR #1194’s enhancement is a stateful RMS/noise-floor gate and gain/soft-clip stage. It has no render input, delay estimate, adaptive echo filter, or double-talk reference. It should remain separately configurable; enabling it must not be presented as enabling AEC ([PR #1194](https://github.com/Open-Less/openless/pull/1194)).

## 4. Platform feasibility and constraints

| Platform | Official capability | Feasibility for OpenLess | Main limitation / required adapter |
| --- | --- | --- | --- |
| Windows | WASAPI loopback captures the stream played by a render endpoint; Microsoft says loopback exists primarily to support AEC. It is shared-mode only, and endpoint/hardware/DRM behavior matters ([Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)). Windows also exposes Communications/Speech processing modes and effect discovery, but modes are driver-defined and optional ([Audio Signal Processing Modes](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-signal-processing-modes); [APO architecture](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-processing-object-architecture)). | **Best first desktop prototype.** Passive WASAPI loopback can preserve audible playback while supplying a reference to WebRTC APM/AEC3. | The current `cpal` input path does not expose this render-reference lifecycle. Use a Windows-specific WASAPI adapter, select/track the actual render endpoint, handle shared-mode and protected-content gaps, and avoid assuming a driver APO is present. |
| macOS | Apple’s VoiceProcessingIO unit provides voice processing such as echo cancellation and AGC ([VoiceProcessingIO](https://developer.apple.com/documentation/audiotoolbox/kaudiounitsubtype_voiceprocessingio)). | **Feasible for a controlled voice-I/O route; uncertain for arbitrary system playback.** | VoiceProcessingIO expects application playback through its output path. OpenLess currently mutes the system endpoint and does not own every application’s playback graph. A separate supported Core Audio render tap/monitor path and permissions would need investigation; otherwise report unsupported and retain fallback. Voice processing can change speech-oriented audio characteristics, so compare near-end fidelity. |
| Linux (PipeWire) | PipeWire’s official echo-cancel module creates virtual echo-cancel capture/playback nodes and streams. Playback sent to the echo-cancel sink supplies the reference; `monitor.mode` can capture a default sink monitor. The documented backend is `aec/libspa-aec-webrtc` ([PipeWire Echo Cancel](https://docs.pipewire.org/page_module_echo_cancel.html)). | **Feasible when PipeWire can own or monitor the playback route.** | The current `linux-egui` recorder is a generic `cpal` input path. Add a PipeWire-specific node/monitor adapter or use the virtual source/sink; do not assume an ALSA/Pulse/default-device capture has a synchronized monitor. Device/route changes and non-PipeWire systems require capability fallback. |
| Android | `AcousticEchoCanceler` was added in API 16, is device-dependent, must be checked with `isAvailable()`, and attaches to the `AudioRecord` audio session; `create()` may return `null` ([Android API reference](https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler)). | **Feasible only through a native Android capture path on supported devices.** | The current shared recorder uses `cpal`; the repository’s Kotlin code handles microphone permission but does not expose an `AudioRecord` session/effect adapter. Add native session ownership, route-specific tests, and a clear unavailable state. |
| Portable software fallback | WebRTC APM/AEC3 is a cross-platform algorithmic option, but its official API still requires render and capture frames plus timing ([APM header](https://webrtc.googlesource.com/src/+/refs/heads/main/api/audio/audio_processing.h)). | **Feasible only where each host can provide a reliable render/monitor reference.** | A portable library does not create a system loopback source. Adding it without a platform reference would produce an apparent setting with weak or misleading results. It also adds binary size, CPU, latency, and real-time integration work; these are consistent with the complexity concerns recorded in #1223’s first comment. |

The W3C `echoCancellation` constraint is a useful model for product wording, not a guarantee that every source supports AEC. The specification says `true` must attempt at least remote-only cancellation, while `all` must attempt all system-played sound; it also requires capabilities to report only `false` when cancellation is unavailable ([Media Capture and Streams, `echoCancellation`](https://w3c.github.io/mediacapture-main/#def-constraint-echoCancellation)). OpenLess should expose an equivalent capability/unsupported result instead of claiming universal support.

## 5. Risks and mitigations

1. **Wrong or missing reference.** A default render monitor may not correspond to the selected output, a Bluetooth route may add variable delay, and applications may use another sink. Mitigate with endpoint identity, timestamps, route-change invalidation, reference starvation bypass, and a visible capability/diagnostic state.
2. **Double-talk damage.** The user and video speaker can talk simultaneously. AEC must protect near-end speech; use a proven double-talk strategy and measure near-end word recall, not only echo reduction.
3. **Clock drift and buffering.** Separate render/capture clocks eventually move out of alignment. Bound queues, monitor underflow/overflow, and resample or adapt delay rather than draining an un-timestamped FIFO indefinitely.
4. **Real-time callback safety.** Audio callbacks must not block on a long DSP operation, allocate without bounds, or wait on a worker. Keep callback work bounded and use a lock-free or carefully bounded handoff where the platform requires it.
5. **Native processing conflicts.** Windows driver APOs and Android/macOS voice processing may already apply AEC/AGC/NS. Discover/record active effects where the platform permits and avoid unintentionally stacking processors ([Windows effect discovery guidance](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-signal-processing-modes)).
6. **Protected or inaccessible playback.** Microsoft documents that loopback behavior depends on endpoint capabilities and that protected content may not be capturable ([WASAPI loopback](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)). Treat reference loss as unsupported/bypass, never as silence that could train the canceller incorrectly.
7. **Privacy.** A render reference can contain all system audio, not only the video selected by the user. Keep it in-process, do not archive or transmit it, and document that it is used only transiently to process microphone PCM.
8. **Failure cleanup.** Existing mute behavior is best-effort and uses a guard restored on drop ([`audio_mute.rs`](https://github.com/Open-Less/openless/blob/de072a88373a222280a8c22948092a260880196c/openless-all/app/src-tauri/src/audio_mute.rs)). AEC startup, route changes, cancellation, recorder errors, and shutdown must preserve the same cleanup guarantees without muting playback when AEC is selected.

## 6. Phased implementation recommendation

### Phase 0: contract and observability

- Add a session-frozen AEC mode separate from `mute_during_recording` and microphone enhancement: `off`, `native`, or `software` is preferable to a boolean if the UI needs to explain fallback.
- Define a host capability response containing platform, reference source, endpoint identity, availability, and reason for unavailability.
- Preserve current mute as an explicit fallback, never silently mute when the user selected playback-preserving AEC.
- Add deterministic tests for PCM framing, reference underflow/overflow, reset, cancellation, and “no reference means bypass.”
- Add an offline fixture containing far-end audio, a known delayed/filtered echo, near-end speech, and double-talk. Test both echo reduction and preservation of near-end samples.

### Phase 1: Windows proof of value

- Implement a Windows WASAPI shared-mode loopback capture on the render endpoint used for playback.
- Feed 10 ms render and microphone frames into WebRTC APM/AEC3, preserving stream delay and route identity.
- Measure speaker, wired, Bluetooth, default-device change, device unplug, Remote Desktop, and protected-content cases before exposing the setting broadly.
- Keep AEC off by default until real-device results meet the acceptance gates below.

### Phase 2: Linux PipeWire

- Detect PipeWire and create/use the documented echo-cancel source/sink or monitor path. Keep the generic `cpal` path as fallback.
- Test default sink monitor, per-application routing, route changes, PulseAudio compatibility mode, and systems without PipeWire.
- Do not advertise “Linux support” as a single guarantee; report the backend and route capability.

### Phase 3: macOS and Android

- macOS: prototype VoiceProcessingIO only for a controlled voice-processing route; separately investigate a supported system render tap and permission requirements. If arbitrary app playback cannot be referenced reliably, mark the feature unavailable rather than muting or claiming cancellation.
- Android: add a Kotlin/native `AudioRecord` adapter, attach `AcousticEchoCanceler` to the correct session, check both availability and `create()` result, and test speaker/headset/Bluetooth/device-vendor combinations.

### Phase 4: product hardening

- Add per-platform support text, diagnostic logging without raw audio, route-change handling, power/CPU telemetry, and a one-click AEC sample test.
- Keep the existing mute option and clarify that it is the deterministic fallback; keep microphone enhancement separately described as noise suppression, not AEC.

## 7. Proposed acceptance and test matrix

The following are proposed product gates, not guarantees stated by platform vendors. Record the exact device, OS, audio route, sample rates, output endpoint, and AEC mode for every run.

| Test area | Required cases | Evidence / gate |
| --- | --- | --- |
| Baseline and toggle semantics | AEC off; enhancement off/on; mute off/on; AEC setting changed between sessions | Existing behavior is unchanged when AEC is off. AEC does not call endpoint mute or change system volume. Mute restores the user’s original state on normal stop, cancel, startup failure, runtime fault, and shutdown. |
| Far-end-only | Video speech, music, tones, silence at microphone; several levels and delays | Compare processed and unprocessed microphone recordings. False transcription rate decreases materially; report ERLE/SNR or another reproducible echo metric. |
| Near-end-only | User speech with no playback; quiet/normal/loud speech; short words and sustained speech | Near-end word recall/WER remains within an agreed regression budget; no audible clipping or gate-induced loss in the archived processed PCM. |
| Double talk | Video speech overlaps user speech at beginning, middle, and end of utterances | Near-end speech remains intelligible and transcribes; cancellation does not treat the user as echo. |
| Route/device matrix | Built-in speaker + mic, USB mic, wired headset, Bluetooth headset/speaker, output and input changes during capture | Each combination is either validated or reports unsupported/bypassed. No stale reference after endpoint change. |
| Platform matrix | Windows WASAPI loopback; PipeWire source/sink and monitor mode; macOS VoiceProcessingIO/tap candidate; Android supported and unsupported devices | Capability result identifies backend and reason. Unsupported platforms keep working without a crash and without unexpected mute. |
| Faults and permissions | Missing microphone permission, missing render permission/device, protected content, route unplug, stream underflow/overflow | Recording remains usable through documented bypass/fallback; no raw render audio is persisted or transmitted; cleanup is verified. |
| Timing and performance | Start/stop latency, reference starvation, clock drift, 10-minute capture, repeated sessions | Bounded queue memory, no callback watchdog failures, agreed p95 CPU/latency/power budget, and no unbounded growth. |
| Regression | History playback/retranscription, streaming ASR, QA/selection voice, archive retention, concurrent lifecycle races | Existing recorder/core contract tests pass; processed archive format remains canonical 16 kHz mono PCM WAV where enabled. |

## Current implementation decision

The feature branch follows the best-effort design above: AEC is opt-in, session-frozen, capability-by-fallback, and independent from output muting. The shared software canceller is covered by deterministic signal tests, while host-specific loopback and monitor paths remain subject to platform/device validation. The setting must not be presented as a guarantee of perfect cancellation. Windows WASAPI and macOS CoreAudio loopback are the primary desktop paths; PipeWire/Pulse monitor capture is attempted on Linux; unsupported routes, ALSA-only capture, mobile builds, protected content, and unavailable monitors continue with ordinary microphone processing. A route changed during an active session is not dynamically rebound by this phase; users must stop and start a new dictation session for the new output route to be selected, and the route-change acceptance cases remain hardware-validation work.

The remaining acceptance work is real-device validation of endpoint identity, route changes, Bluetooth latency, double-talk speech preservation, and long-running clock drift. Those hardware-only results should be recorded per platform rather than inferred from the offline tests.

## Primary sources

- [Issue #1223 and comments](https://github.com/Open-Less/openless/issues/1223)
- [PR #274: mute system output](https://github.com/Open-Less/openless/pull/274)
- [PR #1194: lightweight microphone enhancement](https://github.com/Open-Less/openless/pull/1194)
- [WebRTC Audio Processing API](https://webrtc.googlesource.com/src/+/refs/heads/main/api/audio/audio_processing.h)
- [WebRTC EchoCanceller3](https://webrtc.googlesource.com/src/+/refs/heads/main/modules/audio_processing/aec3/echo_canceller3.h)
- [Microsoft WASAPI Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [Microsoft Audio Signal Processing Modes](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-signal-processing-modes)
- [Microsoft Audio Processing Object Architecture](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-processing-object-architecture)
- [Apple VoiceProcessingIO](https://developer.apple.com/documentation/audiotoolbox/kaudiounitsubtype_voiceprocessingio)
- [PipeWire Echo Cancel module](https://docs.pipewire.org/page_module_echo_cancel.html)
- [Android AcousticEchoCanceler](https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler)
- [W3C `echoCancellation` constraint](https://w3c.github.io/mediacapture-main/#def-constraint-echoCancellation)

*The baseline history and primary-source citations were collected during the read-only investigation; the feature-branch implementation notes above describe the changes now under test.*
