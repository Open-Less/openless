# OpenLess Windows owner-thread target experiment

Base: the published `tauri-runtime-wry 2.12.2` crate, SHA-256
`8d61892587aea2a85245bcaec98c6fe46268874c0a0deae5535fb71ca7b93eae`.
Original source, normalized manifest, build script and MIT/Apache licenses are
retained. `[workspace]` isolates this vendored package from the outer app workspace;
the application root patch and application lockfile select it transitively.
The published crate's own lockfile is provenance, not the application's lockfile.

Attempt: `A1-owner-thread-target` / `C1-windows-target-storage`.

## One changed ownership boundary

Only Windows `DispatcherMainThreadContext::window_target` changes representation.
A `!Send + !Sync` owner token holds a TLS registration. Handles clone only atomic
metadata, never Tao's target or its non-atomic `Rc<EventLoopRunner>`. Target lookup
checks the owner before TLS access, retains an owner-local Rc and releases the
registry borrow before invoking a callback. Retirement closes metadata and removes
the entry before dropping it outside the map borrow. In-flight callbacks retain
their local reference until return, on the same owner. Late task/getter dispatch
returns `EventLoopClosed`; a submission racing retirement can still be in flight.

The token survives iterations and consuming native loops. Windows consuming `run`
mirrors Tao 0.37.1's `run_return` / `process::exit` sequence, but retires the target
before process exit skips destructors. Normal/unwind cleanup precedes destruction
of the native loop. `LoopDestroyed` from an iteration is not owner retirement.
Windows display handles remain pointerless via `DisplayHandle::windows()`.
Non-Windows target storage and display behavior are unchanged.

No new unsafe Send/Sync, target pointer dereference, cross-callback mutex, sleep,
retry, window scheduling change, audio change or IME replay is introduced.
The optional upstream `tracing` feature has separate Rc storage and is not covered;
Actions asserts that it is not resolved in the tested application graph.

## Evidence and limitations

The matched OpenLess 60155b00 dump identifies Context::clone on the coding-hotkey
supervisor. The fault is a shared abort/trap block, not a field-specific check;
missing heap pages prevent identifying the triggering count or first bad write.
This makes an ownership repair actionable, not a confirmed explanation for every
0xc0000374 or the newer unfinished capsule dispatch.

Primary sources:
- https://github.com/tauri-apps/tauri/issues/15408
- https://github.com/tauri-apps/tauri/issues/15793
- https://github.com/tauri-apps/tao/pull/1290 (closed; not copied)
- https://github.com/tauri-apps/tauri/pull/15411 (open; not copied wholesale)

The change avoids both the rejected bare Rc-to-Arc substitution and temporary
Weak-upgrade display-handle borrowing. It does not establish absence of unrelated
framework lifetime defects. Deterministic helper tests and actual runtime lifecycle
execution run only on Actions. Passing them does not establish the field symptom
is gone; retain P1-P6 until the user confirms a comparable fixed reproduction.

Rollback: remove the application patch entry, restore the registry source/checksum
for 2.12.2 in its lockfile, and remove this vendor directory after review. Keep the
diagnostic trail and classify the attempted fix before making a different change.
