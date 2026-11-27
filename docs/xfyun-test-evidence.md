# Xfyun FIFO sender evidence candidate

This is a test-only characterization candidate based on
`901383dbb61d416baad318d81fbe2c46ce2b9b9a` (tree
`c6797b9dc77bddf36253e50ffdcd0ff7fa0c408e`). It does not apply the separate
`e3e85ba997bb5cf463e8ed4b5ddc77080a44abf4` production pacing candidate.
Compilation, tests and Clippy have **not** been run locally.

## Observation boundary

`xfyun/evidence.rs` is included only under `cfg(test)`. The actual FIFO future
created by `open_session` runs inside a task-local probe. The shared clock only
adds a test-only deadline accessor; its `wait_slot` and `mark_sent` formulas are
unchanged. The shared send helper has test-only hooks inside its existing writer
mutex, timeout and cancellation select. Other providers never install the
task-local probe.

Each synthetic frame has a 1-based FIFO id, End flag, exact bytes, length and a
compact FNV-1a payload tag. The tag is for log alignment, while assertions compare
the actual bytes. Events record both Tokio and standard monotonic instants:

- `Dequeued`, `Wait` (planned deadline), `WaitPending` (the real wait was polled to
  Pending), and `Grant` (actual observation time plus returned scheduled slot).
- `Begin` after the real writer mutex was acquired and checked, immediately
  before `ws.send`; `Complete` immediately after that send/flush returns success.
- `Failed`, explicit `Cancel`, and `Stopped`, including the final End frame.

These are local API boundaries, not physical packet or provider receipt times.
The test observer itself adds allocations and synchronous work; an observed run
cannot reconstruct an earlier uninstrumented run.

## Independent cases

The six controlled cases capture the **actual first spawned FIFO future** using
the existing `TaskSpawner`. They finish a real loopback handshake in ordinary
time, then pause Tokio time and poll this worker manually. Only the transport
await inside the real writer critical section is substituted with synthetic
success, failure or a semaphore gate. The real queue, clock, writer acquisition,
per-frame timeout/cancellation, accounting and End acknowledgement all remain on
the worker path. The synthetic write is not claimed to measure a socket.

| Exact suffix under `asr::xfyun::evidence::tests::` | Required characterization |
| --- | --- |
| `normal_full_frames_and_short_tail_characterization` | Three 1280 B frames and the 400 B tail receive exact 0/40/80/120 ms planned slots; deliberate worker polls at 0/41/82/123 ms observe their begins, then End at 123 ms without another slot. |
| `late_wake_characterizes_current_sender_catch_up` | Frame 2's wait is demonstrably Pending with a 40 ms deadline; worker polling resumes at 90 ms, receives the old 40 ms slot and starts full frames 2/3 together at 90 ms, before draining tail and End. |
| `slow_write_characterizes_current_sender_catch_up` | Full frame 2 obtains its 40 ms slot and writer when polled at 41 ms; the substituted successful transport holds the real writer until 131 ms; full frame 3 starts at that completion, before draining tail and End. |
| `cancel_during_slot_does_not_continue_or_send_end` | Cancel while waiting for frame 2's slot; worker finishes without advancing time for cancellation, tail or End. |
| `cancel_during_write_does_not_continue_or_send_end` | Cancel while frame 2 holds the writer; releasing the gate later cannot continue the stopped worker. |
| `failed_write_does_not_continue_or_send_end` | Injected transport failure on frame 2 stops the worker and rejects queued finish; exact frame 1 and byte accounting survive, tail/End do not send. |

The successful cases compare every byte, FIFO id, frame length and End uniqueness.
The 41 ms explicit virtual-time steps avoid depending on timer readiness exactly
at a deadline boundary; planned slots are still asserted at 40 ms intervals.
The two catch-up cases assert the **current contract**, rather than a repair;
changing the production clock to prohibit catch-up should break their assertions.
The 400 B tail is followed by End, so these cases do not claim to observe a
12.5 ms tail-to-next-audio slot that the public finish path never requests.

`asr::xfyun::evidence::tests::receiver_barrier_distinguishes_reads_from_sends`
uses only real time and real WebSocket sends. It runs one ordinary receiver and
one receiver blocked after handshake until the sender has completed all five
1280 B frames, the 400 B tail and End. The blocked receiver must have no reads
before release; every subsequent read must be after the recorded End completion.
Both runs compare exact payload, FIFO ids, lengths, tail-before-End, one final End
and a synthetic final transcript. No per-frame acknowledgement paces the sender;
no microsecond read-gap or runtime load threshold decides this case.

The barrier establishes a causal counterexample: the reader's observation window
can move entirely after upload without moving the earlier sender events. Actual
read gaps are logged, not assumed to be zero or below 25 ms; this test does not
require any particular socket buffering or OS read scheduling.

The original `asr::xfyun::tests::buffered_audio_is_paced_and_precedes_end_frame`
keeps its original receiver timestamps, 140 ms total-span check, 25 ms four-gap
checks and all assertions. It only gains compact, aligned sender/receiver output
before those assertions. Log ids start at 1; the original gap message indices
start at 0. The three original lifecycle tests remain unchanged.

## Coordinator review: proposed narrow cloud entry

This section is a proposal for coordinator approval, **not** an instruction to
dispatch or rerun CI. No workflow, default concurrency or gate is changed.

Use a dedicated checkout of the reviewed candidate's exact SHA on the original
`xcode-27` / `xcode-27-arm64` image `20261123.0387.2`, with Rust **1.99.0**
(`b940084d7`, aarch64-apple-darwin) and Clippy. If that exact image is unavailable,
report the difference rather than calling it the same environment. Record
checkout SHA/parent/tree, image/tool versions, action SHAs, manifest/lock hashes,
prepare result, and derived `xfyun.rs`, `xfyun/evidence.rs` and `asr/mod.rs`
hashes. Use synthetic data and `127.0.0.1` only, without provider credentials.

In that dedicated checkout, run the unchanged preparation script:

```sh
python3 -B openless-all/ios/scripts/prepare-core.py
```

Set `CARGO_TARGET_DIR` to a dedicated owned cloud scratch target. Preserve the
original workspace library compilation boundary, then collect strict lint
independently even if tests fail:

```sh
rustup run 1.99.0 cargo test --manifest-path openless-all/ios/CoreHost/Cargo.toml --workspace --lib --no-run --locked
rustup run 1.99.0 cargo clippy --manifest-path openless-all/ios/CoreHost/Cargo.toml -p openless-core --all-targets --locked -- -D warnings
```

List and run each of these eleven **exact** names once, recording nonzero match
counts, exit codes and compact traces. The following template applies to each
`name`; require the list to contain exactly `"${name}: test"` before treating its
execution as evidence:

```sh
rustup run 1.99.0 cargo test --manifest-path openless-all/ios/CoreHost/Cargo.toml -p openless-core --lib --locked "$name" -- --exact --list
rustup run 1.99.0 cargo test --manifest-path openless-all/ios/CoreHost/Cargo.toml -p openless-core --lib --locked "$name" -- --exact --nocapture --test-threads=1
```

```text
asr::xfyun::evidence::tests::normal_full_frames_and_short_tail_characterization
asr::xfyun::evidence::tests::late_wake_characterizes_current_sender_catch_up
asr::xfyun::evidence::tests::slow_write_characterizes_current_sender_catch_up
asr::xfyun::evidence::tests::cancel_during_slot_does_not_continue_or_send_end
asr::xfyun::evidence::tests::cancel_during_write_does_not_continue_or_send_end
asr::xfyun::evidence::tests::failed_write_does_not_continue_or_send_end
asr::xfyun::evidence::tests::receiver_barrier_distinguishes_reads_from_sends
asr::xfyun::tests::buffered_audio_is_paced_and_precedes_end_frame
asr::xfyun::tests::end_frame_waits_for_delayed_send_worker_instead_of_overtaking_audio
asr::xfyun::tests::audio_frame_send_failure_stops_worker_and_fails_send_last_frame_without_end
asr::xfyun::tests::cancel_wakes_paced_send_worker_promptly
```

The serial exact invocations isolate this diagnostic experiment only; they do not
change the upstream full suite or prove the original parallel-suite failure is
resolved. Do not rerun until green. Keep the original failure when it occurs.
The `test-util` feature is added only to the existing Tokio dev dependency;
both lockfiles and the iOS patch are unchanged. `--locked` resolution and target
Rust compatibility still require cloud verification; do not rewrite a lock to
hide a failure.

After all owned processes have exited, clean this checkout's derived Core and
owned scratch/target, preserving small reports and necessary traces. Do not
delete shared dependency caches or another session's paths. This narrow evidence
never substitutes for the exact integrated SHA's existing required CI checks.

## Limits of any future result

A trace can distinguish sender catch-up and delayed reading **for that observed
run**. The late-wake and slow-write cases establish separate reachable sender
behaviors if they pass; the receiver barrier establishes the measurement
counterexample if it passes. Both mechanisms can coexist.

Neither these future tests nor static source review identifies the cause of
historical run `46073813113` / job `140756824107`'s 23.176916 ms receiver gap, or
the older P3 run's 9.125 µs gap. They do not prove service behavior, physical wire
timing, real-user recording correctness, or a production pacing fix.
