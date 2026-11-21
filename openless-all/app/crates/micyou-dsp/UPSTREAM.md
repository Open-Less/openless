# MicYou DSP adaptation

Source: [MicYou-Dev/MicYou](https://github.com/MicYou-Dev/MicYou), commit
`d821da032e2b72330ced09678dd127acd2714bef` (2026-10-20 import).

`src/lib.rs` comes from `tauri-app/crates/micyou-audio/src/dsp.rs`.
`../micyou-infer/src/{lib,ops}.rs` and the two `src/assets/*.mcy` blobs
come from the corresponding upstream paths. The model blobs are imported unchanged. The inference runtime retains the
upstream algorithms, with baseline x86-64 SSE and ARM64 NEON row projections using separate
multiply/add operations and the original accumulation order. Original copyright notices are
retained in all source files. See `LICENSE` for MicYou's complete license and
plugin exception; `LICENSE-PureVox.txt` and `LICENSE-AEC7.txt` are the upstream
MIT model notices. The inference crate also carries MicYou's license.
`LICENSE-RNNoise.txt` is `nnnoiseless` 0.5.2's original BSD notice, including
the RNNoise/Xiph/Mozilla contributors.

OpenLess changes:

- Standalone DSP-only manifests, local inference dependency, no audio device
  or ONNX Runtime dependency.
- Removed playback buffer management: ASR audio must not duplicate or drop
  samples to regulate playback latency.
- Align dry/wet mixing with the neural denoisers' one-hop delay.
- Limit AGC boost on rejected noise, using RNNoise speech probability or
  PureVox wet/dry energy ratio. This prevents AGC from undoing suppression
  when a conservative amount of dry signal is mixed back in.
- Add serde defaults for DSP settings.
- Make the true FFT spectrum analyzer opt-in and throttle snapshots to 10 Hz.
  Closing monitoring releases its FFT resources.
- Use reusable host buffers and cached rational-phase resampling kernels.
- Define the small AEC failure enum locally instead of depending on MicYou's
  audio host.

OpenLess supplies bounded session settings, mono downmix, anti-aliasing
resampling, fixed 480-sample hops, model-delay compensation, end-of-stream
flush and exact output-duration accounting in `openless-core/src/audio.rs`
and `audio_dsp.rs`. The main microphone path does not install plugin hooks.

Android capture requests 48 kHz directly, and all enhancement algorithms keep
that processing rate. The original input filter is preserved even at 48 kHz,
using cached coefficients; ASR output is downsampled to 16 kHz. CPAL retains a device-native fallback if
48 kHz cannot be opened. Capture queues are not enlarged for playback or
monitoring.

AEC7 and its aligned-reference API are retained and tested here. OpenLess's
microphone hosts do not yet provide synchronized speaker-reference capture,
so the application does not expose AEC7 as an active microphone effect.
Android may instead use its session-bound platform AcousticEchoCanceler when
the device supports it; availability and activation are reported separately.
The optional comb filter follows upstream's simple delay subtraction; it is
not a learned room dereverberation model.

Synthetic throughput check (release build):

```sh
cargo run --release --manifest-path openless-all/app/Cargo.toml -p micyou-dsp --example benchmark
```

These measurements do not establish speech quality or recognition accuracy.

## Following upstream fixes

The authoritative current revision and source/model SHA-256 values live in
`../../scripts/micyou-upstream.json`. The version above records the original
import. OpenLess-specific edits live in `../../scripts/micyou-dsp.patch`;
the inference VM retains upstream algorithms with explicit Clippy exceptions
for its numerical kernels and operation signatures. These exceptions are
recorded in the patch; OpenLess application code keeps strict lint checks.

From the app directory:

```sh
python3 scripts/sync-micyou.py --verify          # offline integrity check
python3 scripts/sync-micyou.py --check           # stage latest upstream, no writes
python3 scripts/sync-micyou.py --sync            # apply relevant upstream fixes
python3 scripts/sync-micyou.py --refresh-patch   # record reviewed adapter edits
```

`--check` and `--sync` also accept `--revision <40-character SHA>`.
The importer downloads immutable source/model files, applies the adapter in a
temporary directory, and rejects context conflicts before writing anything.
Local edits require an explicit patch refresh; models and notices cannot be
silently forked by that command. Unrelated upstream commits create no update.

`.github/workflows/sync-micyou.yml` checks daily, tests relevant changes, then
creates/updates a draft PR against `beta`. It becomes active after the workflow
is present on the repository's default branch and Actions is allowed to create
PRs. Candidate compilation has read-only permissions; the write-permission
job does not execute candidate code. No automatic merge is enabled. Run normal
native platform CI on the draft branch before merging; GITHUB_TOKEN-created PRs
do not automatically trigger separate PR workflows.

Most upstream implementation/model fixes can follow this route without manual
copying. Upstream API/dependency/format changes that break the adapter or tests
still require review and a deliberate patch/dependency update.

## Shared performance changes (2026-10-22)

All hosts that use `AudioProcessor` (including desktop Tauri and Linux egui)
reuse mono, frame and resampled output scratch buffers. Rational resampling
phases are precomputed once per stream, capped at 1024 phases; unusual clocks
fall back to the original kernel calculation. The output duration and filter
formula are unchanged, including the original filter at native 48 kHz.

Contiguous matrix and GRU projection rows use separate SIMD multiply and add:
SSE on baseline x86-64, NEON on ARM64 targets with NEON enabled. Other targets
keep the scalar fallback. No additional inference runtime or worker pool is
loaded. This does not change OS-specific capture APIs or require AVX support.

The synthetic full-chain probe defaults to 48 kHz and checks exact output
length. Run on the target device before drawing battery or latency conclusions:

```sh
cargo run --release --manifest-path openless-all/app/Cargo.toml -p openless-core --example audio_performance -- rnnoise 48000
```

## Before/after equivalence validation (2026-10-24)

The frozen baseline is commit `f0744e10`. The same `openless-core` example
`audio_equivalence` was built against that baseline and the optimized sources,
using their locked dependencies, in separate release build directories.
Each build exports 48 cases: 16/44.1/48 kHz, mono/stereo, four denoiser modes,
and default/combined gain-EQ-comb-VAD settings. Inputs contain silence,
harmonics, deterministic noise, high-frequency interference, an impulse,
irregular chunk boundaries, and a partial final frame.

On Linux x86-64, all 48 output PCM files were byte-identical to the baseline:
maximum difference 0 LSB, with matching durations. The full hashes, errors and
diagnostic processing times are committed in
`../../scripts/fixtures/audio-equivalence-2026-10-24.json`. This establishes
equivalence for those inputs and host, not a listening study or ASR accuracy.

Export both builds to separate directories with `audio_equivalence OUTPUT_DIR`,
then run `python3 scripts/compare-audio-outputs.py BEFORE AFTER --report REPORT`.
The comparator fails on any PCM difference or duration/case-set mismatch;
`python3 scripts/test-compare-audio-outputs.py` verifies a one-LSB mutation
and a truncated output cannot pass. The probe sources must be identical in
both builds. Original baseline sources/models must remain unchanged.

Desktop Tauri and Linux egui use the shared processor directly. Harmony uses
the external PCM recorder, which reuses conversion scratch per session.
The iOS native recorder now optionally applies the same processor with
session-frozen settings and a reused input buffer; the stop/drain barrier
flushes its tail before finishing the archive. Enhancement remains off by
default. Its native Swift settings page persists the same software DSP
controls as egui and Tauri, with writes blocked while a capture is active.
Native iOS conversion uses a separate
Swift AVAudioConverter worker; its output scratch is now actor-owned and
reused with exact output capacities and a two-entry bound.
`ios/scripts/test-audio.sh` compares that worker byte-for-byte against
the frozen pre-optimization Swift worker using its complete, fixed 100 ms
input path, over up to 32 format/ownership/packetization combinations and
end-of-stream flush (older macOS runtimes only exercise legacy ownership).
The old variable-packet path itself lost 960 samples in the 44.1 kHz CI case.
The new worker honors the converter's requested input frame count and drains
available output before accepting another packet. Variable-packet output must
match the complete fixed-packet baseline, not its truncated old counterpart;
the duration assertion remains enabled. That Apple SDK test
was added but cannot be executed on this Linux host. No claim of on-device
equivalence, battery savings, or real-time capture latency is made here.
