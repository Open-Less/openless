# MicYou DSP adaptation

Source: [MicYou-Dev/MicYou](https://github.com/MicYou-Dev/MicYou), commit
`d821da032e2b72330ced09678dd127acd2714bef` (2026-10-20 import).

`src/lib.rs` comes from `tauri-app/crates/micyou-audio/src/dsp.rs`.
`../micyou-infer/src/{lib,ops}.rs` and the two `src/assets/*.mcy` blobs
come from the corresponding upstream paths. The inference runtime and model
blobs are imported without algorithm changes. Original copyright notices are
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
- Define the small AEC failure enum locally instead of depending on MicYou's
  audio host.

OpenLess supplies bounded session settings, mono downmix, anti-aliasing
resampling, fixed 480-sample hops, model-delay compensation, end-of-stream
flush and exact output-duration accounting in `openless-core/src/audio.rs`
and `audio_dsp.rs`. The main microphone path does not install plugin hooks.

AEC7 and its aligned-reference API are retained and tested here. OpenLess's
microphone hosts do not yet provide synchronized speaker-reference capture,
so the application does not expose or claim active acoustic echo cancellation.
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
