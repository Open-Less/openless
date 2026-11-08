//! [DEBUG-ISSUE-1338] A2 Windows panel-open experiment; effectiveness unconfirmed.
//! Fixed closed facts only. Producers never serialize, write, initialize a sink,
//! acquire application locks, or wait for queue capacity. try_send is not lock-free.
//! Missing records and the final flush tail remain unknown, including forced exit.
//! Backend-only: no frontend intent/send/response-application coverage. A null
//! parent is unknown; no IDs are inferred from command names or thread proximity.
//! P4 Requested begins the wrapper (possibly before handle acquisition); only
//! DispatchReturned reports dispatch acceptance, never execution completion.
//! P5 covers exported native checks, not every internal platform helper.
#![allow(dead_code)]
use serde::Serialize;
pub(crate) const ATTEMPT: &str = "A2-voice-edit-open-async";
pub(crate) const CHANGE: &str = "C2-async-command-dispatch";
pub(crate) const SESSION: &str = "issue-1338-20261106";
pub(crate) const LOG_NAME: &str = "openless-issue-1338-20261106-session.ndjson";
pub(crate) const fn enabled() -> bool {
    cfg!(all(target_os = "windows", feature = "issue-1338-diagnostics"))
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum Probe { P1, P2, P3, P4, P5, P6 }
impl Probe {
    fn hypotheses(self) -> &'static [&'static str] {
        match self {
            Self::P1 | Self::P2 | Self::P4 => &["R2-H1", "R2-H2", "R2-H3", "R2-H4"],
            Self::P6 => &["R2-H3"],
            Self::P3 | Self::P5 => &["R2-H4"],
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    Entered, Returned, ScopeLeft, Requested, Acquired, Released, DispatchReturned,
    Ready, ExitRequested, Exit, CloseRequested, Destroyed,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Observed, Returned, Ok, Error, Granted, Denied, NotDetermined, Restricted,
    NotApplicable, NoDevice,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    #[default] None,
    GetSettings, ListChannels, ReadCredential, Accessibility, Microphone,
    StartupSnapshot, PrefsRead, WarmConfigure, CredentialWorker,
    CredentialsLock, CacheLock, HostDispatch, CapsuleDispatch, TrayQuit, QuitRequest,
    SettingsSnapshot, UpdateSettingFields, SetSettings, QaHotkeySettings,
    SelectionHotkeySettings, GetHotkeyCapability, ShortcutRecording,
    SettingsHostGate, SnapshotRevision, SnapshotPreferences, SnapshotVerify,
    SettingsPersist, SettingsRefresh,
    SelectionHotkeyRegistration, QaHotkeyRegistration, OtherHotkeyRegistration,
    VoiceEditOpen, VoiceEditLookup, VoiceEditCreate, VoiceEditFocusable, VoiceEditShow,
}
impl Kind {
    // Never pass through arbitrary command names, arguments, labels or payloads.
    pub(crate) fn command(command: &str) -> Option<Self> {
        match command {
            "voice_edit_window_open" => Some(Self::VoiceEditOpen),
            "get_settings" => Some(Self::GetSettings),
            "get_settings_snapshot" => Some(Self::SettingsSnapshot),
            "update_setting_fields" => Some(Self::UpdateSettingFields),
            "set_settings" => Some(Self::SetSettings),
            "set_qa_hotkey" => Some(Self::QaHotkeySettings),
            "set_selection_polish_hotkey" => Some(Self::SelectionHotkeySettings),
            "get_hotkey_capability" => Some(Self::GetHotkeyCapability),
            "set_shortcut_recording_active" => Some(Self::ShortcutRecording),
            "get_startup_snapshot" => Some(Self::StartupSnapshot),
            "list_channels" => Some(Self::ListChannels),
            "read_credential" => Some(Self::ReadCredential),
            "check_accessibility_permission" => Some(Self::Accessibility),
            "check_microphone_permission" => Some(Self::Microphone),
            _ => None,
        }
    }
}
/// Fixed source category, never an arbitrary webview label or page identity.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebviewKind { Main, Other, #[default] Unknown }
impl WebviewKind {
    pub(crate) fn from_label(label: &str) -> Self {
        if label == "main" { Self::Main } else { Self::Other }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct Facts { pub kind: Kind, pub value: u64, pub flag: bool, pub webview: WebviewKind }
#[derive(Clone, Copy, Debug)]
pub(crate) struct Operation { id: u64, parent: u64, probe: Probe }
impl Operation {
    pub(crate) fn new(probe: Probe) -> Self { Self::child(probe, current()) }
    pub(crate) fn child(probe: Probe, parent: Option<Self>) -> Self {
        #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
        let id = sink::NEXT_OPERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
        let id = 0;
        Self { id, parent: parent.map_or(0, |p| p.id), probe }
    }
    pub(crate) fn record(self, event: Event, outcome: Outcome, facts: Facts) {
        #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
        sink::record(self, event, outcome, facts);
        #[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
        let _ = (event, outcome, facts);
    }
    pub(crate) fn mark(self, event: Event) { self.record(event, Outcome::Observed, Facts::default()); }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
thread_local! {
    static CURRENT: std::cell::Cell<Option<Operation>> = const { std::cell::Cell::new(None) };
    static THREAD: u64 = NEXT_THREAD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
static NEXT_THREAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn current() -> Option<Operation> {
    #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
    { CURRENT.with(std::cell::Cell::get) }
    #[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
    { None }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
fn thread_label() -> u64 { THREAD.with(|label| *label) }
/// Only install inside a real synchronous stack; !Send, never held across await.
pub(crate) struct SyncContext {
    previous: Option<Operation>,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl SyncContext {
    pub(crate) fn enter(operation: Operation) -> Self {
        #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
        let previous = CURRENT.with(|slot| slot.replace(Some(operation)));
        #[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
        let previous = None;
        Self { previous, _not_send: std::marker::PhantomData }
    }
}
impl Drop for SyncContext {
    fn drop(&mut self) {
        #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
        CURRENT.with(|slot| slot.set(self.previous));
    }
}
/// Owns no TLS context. Safe to move with an async future; cancellation is scope_left.
pub(crate) struct Boundary { pub operation: Operation, pub kind: Kind }
impl Boundary {
    pub(crate) fn enter(probe: Probe, kind: Kind) -> Self {
        let boundary = Self { operation: Operation::new(probe), kind };
        boundary.record(Event::Entered, Outcome::Observed); boundary
    }
    pub(crate) fn record(&self, event: Event, outcome: Outcome) {
        self.operation.record(event, outcome, Facts { kind: self.kind, ..Default::default() });
    }
    pub(crate) fn returned(&self) { self.record(Event::Returned, Outcome::Returned); }
}
impl Drop for Boundary { fn drop(&mut self) { self.record(Event::ScopeLeft, Outcome::Observed); } }
/// An async command's operation has no invented dispatch parent. Only its
/// individual poll stack is a real synchronous parent for services invoked there.
/// This wrapper pins in place and restores TLS before every Pending/Ready/unwind.
pub(crate) async fn observe_future<T>(kind: Kind, future: impl std::future::Future<Output = T>) -> T {
    let boundary = Boundary::enter(Probe::P2, kind);
    let mut future = std::pin::pin!(future);
    let result = std::future::poll_fn(|cx| {
        let _context = SyncContext::enter(boundary.operation);
        std::future::Future::poll(future.as_mut(), cx)
    }).await;
    boundary.returned();
    result
}

#[derive(Clone, Copy)]
pub(crate) struct MainDispatch { operation: Operation, kind: Kind }
impl MainDispatch {
    pub(crate) fn new(kind: Kind) -> Self {
        let this = Self { operation: Operation::new(Probe::P4), kind };
        this.record(Event::Requested, Outcome::Observed); this
    }
    fn record(self, event: Event, outcome: Outcome) {
        self.operation.record(event, outcome, Facts { kind: self.kind, ..Default::default() });
    }
    pub(crate) fn wrap(self, task: impl FnOnce() + Send + 'static) -> impl FnOnce() + Send + 'static {
        move || {
            let _context = SyncContext::enter(self.operation);
            let boundary = Boundary { operation: self.operation, kind: self.kind };
            boundary.record(Event::Entered, Outcome::Observed);
            task(); boundary.returned();
        }
    }
    // Acceptance/inline return only, NOT closure execution or async completion.
    pub(crate) fn dispatched(self, accepted: bool) {
        self.record(Event::DispatchReturned, if accepted { Outcome::Ok } else { Outcome::Error });
    }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
pub(crate) struct Guard<'a, T: ?Sized> {
    guard: Option<parking_lot::MutexGuard<'a, T>>, boundary: Boundary,
    // Only the synchronous host-gate scope installs context for its real
    // nested main-thread dispatches. MutexGuard and SyncContext are !Send.
    _context: Option<SyncContext>,
}
#[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
pub(crate) type Guard<'a, T> = parking_lot::MutexGuard<'a, T>;
pub(crate) fn lock<T: ?Sized>(mutex: &parking_lot::Mutex<T>, kind: Kind) -> Guard<'_, T> {
    lock_at(mutex, Probe::P3, kind)
}
pub(crate) fn lock_at<T: ?Sized>(mutex: &parking_lot::Mutex<T>, probe: Probe, kind: Kind) -> Guard<'_, T> {
    #[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
    {
        let boundary = Boundary { operation: Operation::new(probe), kind };
        boundary.record(Event::Requested, Outcome::Observed);
        let guard = mutex.lock();
        boundary.record(Event::Acquired, Outcome::Observed);
        let context = matches!(kind, Kind::SettingsHostGate | Kind::SelectionHotkeyRegistration
            | Kind::QaHotkeyRegistration | Kind::OtherHotkeyRegistration)
            .then(|| SyncContext::enter(boundary.operation));
        Guard { guard: Some(guard), boundary, _context: context }
    }
    #[cfg(not(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics"))))]
    { let _ = (probe, kind); mutex.lock() }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
impl<T: ?Sized> std::ops::Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.guard.as_deref().expect("guard alive") }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
impl<T: ?Sized> std::ops::DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.guard.as_deref_mut().expect("guard alive") }
}
#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
impl<T: ?Sized> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take()); // Emit after the REAL unlock, never before it.
        self.boundary.record(Event::Released, Outcome::Observed);
    }
}
pub(crate) fn initialize() {
    #[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics", not(test)))]
    sink::initialize();
}
pub(crate) fn build_id() -> &'static str {
    let value = option_env!("OPENLESS_BUILD_ID").unwrap_or("unbound");
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) { value }
    else { "unbound" }
}

#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
#[path = "issue1338_export.rs"]
mod export;
#[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics"))]
pub(crate) fn export_log(source: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    export::export_log(source, target).map_err(str::to_owned)
}

#[derive(Default, Serialize)]
pub(crate) struct Health {
    enabled: bool,
    build_id: &'static str,
    run_id: Option<String>,
    initialized: bool,
    ready: bool,
    dropped: u64,
    queue_closed: u64,
    spawn_failed: bool,
    io_failed: bool,
    written: u64,
    flushed: u64,
}

/// Independent counters, not a record through the possibly failed channel.
/// A snapshot is not transactional; flush is an OS write, not durable fsync.
pub(crate) fn health() -> Health {
    #[allow(unused_mut)]
    let mut health = Health { enabled: enabled(), build_id: build_id(), ..Health::default() };
    #[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics"))]
    if let Some(sink) = sink::SINK.get() {
        sink.health.snapshot(&mut health);
        health.run_id = Some(format!("r{:x}", sink.run));
    }
    health
}

#[cfg(any(test, all(target_os = "windows", feature = "issue-1338-diagnostics")))]
mod sink {
    use super::*;
    use std::io::{self, Write};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
    use std::sync::{Arc, OnceLock};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    pub(super) static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);
    // One source sequence per probe, not a global clock or cross-probe causality.
    static SEQUENCES: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
    pub(super) static SINK: OnceLock<Sink> = OnceLock::new();

    #[derive(Default)]
    pub(super) struct HealthState {
        initialized: AtomicBool,
        ready: AtomicBool,
        dropped: AtomicU64,
        queue_closed: AtomicU64,
        spawn_failed: AtomicBool,
        io_failed: AtomicBool,
        written: AtomicU64,
        flushed: AtomicU64,
    }
    impl HealthState {
        fn snapshot_value(&self) -> Health {
            let mut health = Health { enabled: enabled(), build_id: build_id(), ..Default::default() };
            self.snapshot(&mut health);
            health
        }
        pub(super) fn snapshot(&self, health: &mut Health) {
            health.initialized = self.initialized.load(Relaxed);
            health.ready = self.ready.load(Relaxed);
            health.dropped = self.dropped.load(Relaxed);
            health.queue_closed = self.queue_closed.load(Relaxed);
            health.spawn_failed = self.spawn_failed.load(Relaxed);
            health.io_failed = self.io_failed.load(Relaxed);
            health.written = self.written.load(Relaxed);
            health.flushed = self.flushed.load(Relaxed);
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct Record {
        operation: Operation,
        event: Event,
        outcome: Outcome,
        facts: Facts,
        seq: u64,
        elapsed_us: u64,
        thread_label: u64,
    }

    #[derive(Debug)]
    enum Message {
        Record(Record),
        Snapshot(SyncSender<Result<super::export::Snapshot, &'static str>>),
    }
    pub(super) struct Sink {
        sender: SyncSender<Message>,
        pub(super) health: Arc<HealthState>,
        started: Instant,
        pub(super) run: u128,
    }

    #[cfg(test)]
    thread_local! {
        static INSPECT: std::cell::RefCell<Option<Box<dyn Fn(&Record)>>> = const { std::cell::RefCell::new(None) };
    }
    impl Sink {
        fn submit(&self, record: Record) {
            #[cfg(test)]
            INSPECT.with(|hook| { if let Some(inspect) = hook.borrow().as_ref() { inspect(&record); } });
            match self.sender.try_send(Message::Record(record)) {
                Ok(()) => {},
                Err(TrySendError::Full(_)) => { self.health.dropped.fetch_add(1, Relaxed); },
                Err(TrySendError::Disconnected(_)) => {
                    self.health.dropped.fetch_add(1, Relaxed);
                    self.health.queue_closed.fetch_add(1, Relaxed);
                },
            }
        }
    }

    fn next_sequence(sequences: &[AtomicU64; 6], probe: Probe) -> u64 {
        sequences[probe as usize].fetch_add(1, Relaxed)
    }

    fn submit(sink: &Sink, operation: Operation, event: Event, outcome: Outcome, facts: Facts) {
        sink.submit(Record {
            operation, event, outcome, facts,
            seq: next_sequence(&SEQUENCES, operation.probe),
            elapsed_us: sink.started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            thread_label: thread_label(),
        });
    }

    #[cfg(test)]
    thread_local! {
        static TEST_SINK: std::cell::RefCell<Option<Sink>> = const { std::cell::RefCell::new(None) };
    }

    pub(super) fn record(operation: Operation, event: Event, outcome: Outcome, facts: Facts) {
        // No lazy spawn, channel allocation or file initialization in a probe.
        #[cfg(test)]
        if TEST_SINK.with(|slot| {
            if let Some(sink) = slot.borrow().as_ref() {
                submit(sink, operation, event, outcome, facts);
                true
            } else { false }
        }) { return; }
        if let Some(sink) = SINK.get() { submit(sink, operation, event, outcome, facts); }
    }

    #[cfg(test)]
    pub(super) fn capture(action: impl FnOnce()) -> Vec<serde_json::Value> {
        let (sender, receiver) = sync_channel(512);
        let health = Arc::new(HealthState::default());
        let sink = Sink { sender, health: Arc::clone(&health), started: Instant::now(), run: 1 };
        TEST_SINK.with(|slot| assert!(slot.borrow_mut().replace(sink).is_none()));
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) { TEST_SINK.with(|slot| { slot.borrow_mut().take(); }); }
        }
        let reset = Reset;
        action();
        drop(reset);
        let mut bytes = Vec::new();
        drain(&mut bytes, receiver, 1, 0, &health);
        assert_eq!(health.dropped.load(Relaxed), 0);
        bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap()).collect()
    }

    fn encode(record: Record, run: u128, started_ms: u128, health: &HealthState) -> io::Result<Vec<u8>> {
        let operation = record.operation;
        let mut line = serde_json::to_vec(&serde_json::json!({
            "schema": "diagnosing-bugs.v1", "session_id": SESSION,
            "run_id": format!("r{run:x}"), "build_id": build_id(),
            "attempt_id": ATTEMPT, "change_id": CHANGE,
            "probe_id": operation.probe, "hypothesis_ids": operation.probe.hypotheses(),
            "operation_id": operation.id, "parent_operation_id": (operation.parent != 0).then_some(operation.parent),
            "parent_capture": if operation.parent == 0 { "unknown" } else { "source_local" },
            "thread_label": record.thread_label,
            "event_seq": record.seq, "event": record.event, "outcome": record.outcome,
            "safe": record.facts, "monotonic_us": record.elapsed_us,
            "capture_health": health.snapshot_value(),
            "frontend_coverage": {
                "scope": "backend_only",
                "request_intent": "unknown",
                "request_send": "unknown",
                "response_application": "unknown",
                "transport_loss": "unknown"
            },
            "tail_capture": "unknown", "flush_durability": "os_not_fsync",
            "ts": (started_ms + u128::from(record.elapsed_us / 1000)).to_string(),
        }))?;
        line.push(b'\n');
        Ok(line)
    }

    // Only exports wait, on a blocking worker, with a finite failure boundary.
    // Probe producers retain bounded try_send and never wait for this handshake.
    pub(super) fn snapshot() -> Result<super::export::Snapshot, &'static str> {
        request_snapshot(SINK.get().ok_or("issue1338_no_writer: capture unknown")?)
    }
    fn request_snapshot(sink: &Sink) -> Result<super::export::Snapshot, &'static str> {
        let (tx, rx) = sync_channel(1);
        sink.sender.try_send(Message::Snapshot(tx))
            .map_err(|_| "issue1338_checkpoint_unavailable: capture unknown")?;
        rx.recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "issue1338_checkpoint_unacknowledged: capture unknown")?
    }

    fn checkpoint(writer: &mut impl Write, run: u128, offset: u64,
                  health: &HealthState) -> io::Result<super::export::Snapshot> {
        let mut state = health.snapshot_value();
        state.run_id = Some(format!("r{run:x}"));
        let incomplete = state.dropped != 0 || state.queue_closed != 0 || state.io_failed || state.spawn_failed;
        let mut line = serde_json::to_vec(&serde_json::json!({
            "schema": "issue1338-export-checkpoint.v1", "session_id": SESSION,
            "attempt_id": ATTEMPT, "change_id": CHANGE,
            "checkpoint_start_byte": offset,
            "capture_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis()),
            "run_id": state.run_id, "build_id": build_id(), "feature_enabled": enabled(),
            "capture_health": state,
            "counter_scope": "current_backend_run_before_checkpoint",
            "record_counters": "probe_records_only_excludes_export_checkpoints",
            "backend_preinitialization_capture": "unknown",
            "checkpoint_commit": "ack_only_after_write_and_flush",
            "historical_runs": "included_not_current_coverage_unknown",
            "frontend_coverage": { "scope": "backend_only", "request_intent": "unknown",
                "request_send": "unknown", "response_application": "unknown", "transport_loss": "unknown" },
            "tail_capture": "unknown", "flush_durability": "os_not_fsync",
            "coverage": if incomplete { "incomplete" } else { "backend_checkpoint_only_not_complete_capture" }
        }))?;
        line.push(b'\n');
        writer.write_all(&line)?;
        writer.flush()?;
        let end = offset.checked_add(line.len() as u64).ok_or_else(|| io::Error::other("offset"))?;
        Ok(super::export::Snapshot { end, checkpoint: line, incomplete })
    }

    fn drain(writer: impl Write, rx: Receiver<Message>, run: u128, start: u128,
             health: &HealthState) {
        drain_at(writer, rx, run, start, health, 0);
    }
    fn drain_at(mut writer: impl Write, rx: Receiver<Message>, run: u128, start: u128,
                health: &HealthState, mut offset: u64) {
        health.ready.store(true, Relaxed);
        for message in rx {
            let result = match message {
                Message::Record(record) => (|| -> io::Result<()> {
                    let line = encode(record, run, start, health)?;
                    writer.write_all(&line)?;
                    health.written.fetch_add(1, Relaxed);
                    writer.flush()?;
                    health.flushed.fetch_add(1, Relaxed);
                    offset = offset.checked_add(line.len() as u64).ok_or_else(|| io::Error::other("offset"))?;
                    Ok(())
                })(),
                Message::Snapshot(reply) => match checkpoint(&mut writer, run, offset, health) {
                    Ok(snapshot) => {
                        offset = snapshot.end;
                        let _ = reply.try_send(Ok(snapshot));
                        Ok(())
                    },
                    Err(error) => {
                        let _ = reply.try_send(Err("issue1338_checkpoint_io_failed: capture unknown"));
                        Err(error)
                    }
                },
            };
            if result.is_err() { health.io_failed.store(true, Relaxed); break; }
        }
        health.ready.store(false, Relaxed);
    }

    #[cfg(test)]
    pub(super) fn export_fixture() -> (Vec<u8>, super::export::Snapshot) {
        let (tx, rx) = sync_channel(2);
        let (reply, ack) = sync_channel(1);
        tx.send(Message::Record(Record {
            operation: Operation { id: 1, parent: 0, probe: Probe::P2 },
            event: Event::Entered, outcome: Outcome::Observed, facts: Facts::default(),
            seq: 0, elapsed_us: 0, thread_label: 1,
        })).unwrap();
        tx.send(Message::Snapshot(reply)).unwrap();
        drop(tx);
        let health = HealthState::default();
        health.initialized.store(true, Relaxed);
        let mut bytes = Vec::new();
        drain(&mut bytes, rx, 0x1338, 0, &health);
        (bytes, ack.try_recv().unwrap().unwrap())
    }

    #[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics", not(test)))]
    pub(super) fn initialize() {
        SINK.get_or_init(|| {
            let (sender, receiver) = sync_channel(512);
            let health = Arc::new(HealthState::default());
            health.initialized.store(true, Relaxed);
            let writer_health = Arc::clone(&health);
            let started = Instant::now();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            if std::thread::Builder::new().name("issue-1338-writer".into()).spawn(move || {
                // Only the writer touches the session-owned append-only file.
                let file = (|| -> io::Result<std::fs::File> {
                    let base = std::env::var_os("LOCALAPPDATA")
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| io::Error::other("unavailable"))?;
                    let directory = std::path::PathBuf::from(base).join("OpenLess").join("diagnostics");
                    std::fs::create_dir_all(&directory)?;
                    std::fs::OpenOptions::new().create(true).append(true).open(directory.join(LOG_NAME))
                })();
                match file {
                    Ok(file) => match file.metadata() {
                        Ok(meta) => drain_at(file, receiver, now.as_nanos(), now.as_millis(), &writer_health, meta.len()),
                        Err(_) => { writer_health.io_failed.store(true, Relaxed); },
                    },
                    Err(_) => { writer_health.io_failed.store(true, Relaxed); },
                }
            }).is_err() { health.spawn_failed.store(true, Relaxed); }
            Sink { sender, health, started, run: now.as_nanos() }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn generic_guard_unlocks_before_emitting_release_and_keeps_temporary_lifetime() {
            let mutex = Arc::new(parking_lot::Mutex::new(Some(3_u64)));
            let check = Arc::clone(&mutex);
            INSPECT.with(|hook| *hook.borrow_mut() = Some(Box::new(move |record| {
                if matches!(record.event, Event::Released) { assert!(check.try_lock().is_some()); }
            })));
            let records = capture(|| {
                if let Some(value) = lock(&mutex, Kind::CacheLock).as_mut() {
                    *value = 4;
                    assert!(mutex.try_lock().is_none());
                }
                assert_eq!(*mutex.lock(), Some(4));
            });
            INSPECT.with(|hook| { hook.borrow_mut().take(); });
            let events: Vec<_> = records.iter().map(|r| r["event"].as_str().unwrap()).collect();
            assert_eq!(events, ["requested", "acquired", "released", "scope_left"]);
        }
        #[test]
        fn exact_production_capacity_and_loss_snapshot_are_explicit() {
            let (sender, receiver) = sync_channel(512);
            let sink = Sink { sender, health: Arc::default(), started: Instant::now(), run: 1 };
            for _ in 0..513 { sink.submit(sample()); }
            assert_eq!(sink.health.dropped.load(Relaxed), 1);
            let bytes = encode(sample(), 1, 0, &sink.health).unwrap();
            let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(record["capture_health"]["dropped"], 1);
            assert_eq!(record["tail_capture"], "unknown");
            assert_eq!(record["flush_durability"], "os_not_fsync");
            drop(receiver);
        }
        #[test]
        fn zero_backend_loss_does_not_claim_frontend_evidence_coverage() {
            let bytes = encode(sample(), 1, 0, &HealthState::default()).unwrap();
            let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(record["capture_health"]["dropped"], 0);
            assert_eq!(record["frontend_coverage"], serde_json::json!({
                "scope": "backend_only",
                "request_intent": "unknown",
                "request_send": "unknown",
                "response_application": "unknown",
                "transport_loss": "unknown"
            }));
        }
        fn sample() -> Record {
            Record { operation: Operation { id: 2, parent: 1, probe: Probe::P5 },
                event: Event::ScopeLeft, outcome: Outcome::Observed,
                facts: Facts { kind: Kind::Microphone, value: 0, flag: false, ..Default::default() }, seq: 4, elapsed_us: 1234, thread_label: 7 }
        }
        #[test]
        fn schema_is_typed_single_line_and_scope_left_is_not_success() {
            let line = encode(sample(), 123, 456, &HealthState::default()).unwrap();
            assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
            let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
            for key in ["schema", "session_id", "run_id", "probe_id", "hypothesis_ids", "event", "outcome", "safe", "ts", "build_id", "capture_health", "tail_capture"] {
                assert!(value.get(key).is_some(), "missing closed schema field");
            }
            assert_eq!(value["session_id"], SESSION);
            assert_eq!(value["attempt_id"], ATTEMPT);
            assert_eq!(value["change_id"], CHANGE);
            assert_eq!(value["schema"], "diagnosing-bugs.v1");
            assert_eq!(value["probe_id"], "P5");
            assert_eq!(value["hypothesis_ids"], serde_json::json!(["R2-H4"]));
            assert_eq!(value["event"], "scope_left");
            assert_eq!(value["outcome"], "observed");
            assert_eq!(value["parent_operation_id"], 1);
            assert_eq!(value["event_seq"], 4);
            assert_eq!(value["thread_label"], 7);
            let safe = value["safe"].as_object().unwrap();
            assert_eq!(safe, serde_json::json!({
                "flag": false, "kind": "microphone", "value": 0, "webview": "unknown"
            }).as_object().unwrap());
        }
        #[test]
        fn full_and_disconnected_are_counted_without_waiting() {
            let (sender, receiver) = sync_channel(1);
            let sink = Sink { sender, health: Arc::default(), started: Instant::now(), run: 1 };
            sink.submit(sample());
            sink.submit(sample());
            assert_eq!(sink.health.dropped.load(Relaxed), 1);
            assert_eq!(sink.health.queue_closed.load(Relaxed), 0);
            receiver.try_recv().unwrap();
            drop(receiver);
            sink.submit(sample());
            assert_eq!(sink.health.dropped.load(Relaxed), 2);
            assert_eq!(sink.health.queue_closed.load(Relaxed), 1);
        }
        struct FailWrite;
        impl Write for FailWrite {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::Error::other("injected")) }
            fn flush(&mut self) -> io::Result<()> { panic!("must not flush after a failed write") }
        }
        #[test]
        fn write_failure_does_not_claim_a_written_or_flushed_record() {
            let (tx, rx) = sync_channel(1);
            tx.send(Message::Record(sample())).unwrap(); drop(tx);
            let health = HealthState::default();
            drain(FailWrite, rx, 1, 2, &health);
            assert!(health.io_failed.load(Relaxed));
            assert!(!health.ready.load(Relaxed));
            assert_eq!(health.written.load(Relaxed), 0);
            assert_eq!(health.flushed.load(Relaxed), 0);
        }
        struct FailFlush(Vec<u8>);
        impl Write for FailFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { self.0.write(bytes) }
            fn flush(&mut self) -> io::Result<()> { Err(io::Error::other("injected")) }
        }
        #[test]
        fn io_failure_retains_independent_write_and_flush_watermarks() {
            let (tx, rx) = sync_channel(1);
            tx.send(Message::Record(sample())).unwrap(); drop(tx);
            let health = HealthState::default();
            drain(FailFlush(Vec::new()), rx, 1, 2, &health);
            assert!(health.io_failed.load(Relaxed));
            assert!(!health.ready.load(Relaxed));
            assert_eq!(health.written.load(Relaxed), 1);
            assert_eq!(health.flushed.load(Relaxed), 0);
        }
        #[test]
        fn successful_writer_records_every_complete_line() {
            let (tx, rx) = sync_channel(2);
            tx.send(Message::Record(sample())).unwrap(); tx.send(Message::Record(sample())).unwrap(); drop(tx);
            let health = HealthState::default();
            let mut bytes = Vec::new();
            drain(&mut bytes, rx, 1, 2, &health);
            assert_eq!(health.flushed.load(Relaxed), 2);
            assert_eq!(bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()).count(), 2);
        }
        #[test]
        fn current_checkpoint_is_serialized_by_writer_and_acknowledged_only_after_flush() {
            let (tx, rx) = sync_channel(2);
            let (reply, ack) = sync_channel(1);
            tx.send(Message::Record(sample())).unwrap();
            tx.send(Message::Snapshot(reply)).unwrap();
            drop(tx);
            let health = HealthState::default();
            health.initialized.store(true, Relaxed);
            let mut bytes = b"historical\n".to_vec();
            let initial = bytes.len() as u64;
            drain_at(&mut bytes, rx, 0xab, 0, &health, initial);
            let snapshot = ack.try_recv().unwrap().unwrap();
            assert_eq!(snapshot.end, bytes.len() as u64);
            assert!(bytes.ends_with(&snapshot.checkpoint));
            assert!(!snapshot.incomplete);
            let value: serde_json::Value = serde_json::from_slice(&snapshot.checkpoint).unwrap();
            assert_eq!(value["run_id"], "rab");
            assert_eq!(value["capture_health"]["run_id"], "rab");
            assert_eq!(value["capture_health"]["written"], 1);
            assert_eq!(value["capture_health"]["flushed"], 1);
            assert_eq!(value["capture_health"]["dropped"], 0);
            assert_eq!(value["capture_health"]["ready"], true);
            assert_eq!(value["feature_enabled"], enabled());
            assert_eq!(value["build_id"], build_id());
            assert_eq!(value["tail_capture"], "unknown");
            assert_eq!(value["frontend_coverage"], serde_json::json!({
                "scope": "backend_only", "request_intent": "unknown", "request_send": "unknown",
                "response_application": "unknown", "transport_loss": "unknown"
            }));
            assert_eq!(value["historical_runs"], "included_not_current_coverage_unknown");
            assert_eq!(value["backend_preinitialization_capture"], "unknown");
        }
        #[test]
        fn checkpoint_write_and_flush_failure_never_acknowledge_an_old_snapshot() {
            fn run(writer: impl Write) {
                let (tx, rx) = sync_channel(1);
                let (reply, ack) = sync_channel(1);
                tx.send(Message::Snapshot(reply)).unwrap();
                drop(tx);
                let health = HealthState::default();
                drain(writer, rx, 2, 0, &health);
                assert!(ack.try_recv().unwrap().is_err());
                assert!(health.io_failed.load(Relaxed));
            }
            run(FailWrite);
            run(FailFlush(b"old snapshot\n".to_vec()));
        }
        #[test]
        fn record_failure_before_checkpoint_cannot_authorize_readable_old_file() {
            let (tx, rx) = sync_channel(2);
            let (reply, ack) = sync_channel(1);
            tx.send(Message::Record(sample())).unwrap();
            tx.send(Message::Snapshot(reply)).unwrap();
            drop(tx);
            let health = HealthState::default();
            drain(FailFlush(b"old snapshot\n".to_vec()), rx, 2, 0, &health);
            assert!(ack.try_recv().is_err());
            assert!(health.io_failed.load(Relaxed));
        }
        #[test]
        fn unavailable_writer_and_full_or_closed_queue_are_explicit_failures() {
            assert!(snapshot().is_err()); // initialize() under tests never initializes SINK.
            let (sender, receiver) = sync_channel(1);
            let sink = Sink { sender, health: Arc::default(), started: Instant::now(), run: 9 };
            sink.submit(sample());
            assert!(request_snapshot(&sink).is_err()); // no waiting on full queue
            drop(receiver);
            assert!(request_snapshot(&sink).is_err());
        }
        #[test]
        fn writer_checkpoint_preserves_nonzero_loss_instead_of_claiming_complete() {
            let health = HealthState::default();
            health.dropped.store(1, Relaxed);
            let snapshot = checkpoint(&mut Vec::new(), 7, 0, &health).unwrap();
            assert!(snapshot.incomplete);
            let value: serde_json::Value = serde_json::from_slice(&snapshot.checkpoint).unwrap();
            assert_eq!(value["coverage"], "incomplete");
            assert_eq!(value["capture_health"]["dropped"], 1);
        }
        #[test]
        fn probe_sources_have_independent_sequences() {
            // Use the real producer helper with private counters: other tests
            // deliberately submit records concurrently through global SEQUENCES.
            let sequences = [const { AtomicU64::new(0) }; 6];
            let first = next_sequence(&sequences, Probe::P1);
            assert_eq!(first, 0);
            assert_eq!(next_sequence(&sequences, Probe::P2), 0);
            assert_eq!(next_sequence(&sequences, Probe::P1), first + 1);
            assert_eq!(next_sequence(&sequences, Probe::P2), 1);
            assert!(sequences[2..].iter().all(|sequence| sequence.load(Relaxed) == 0));
        }
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    #[test]
    fn disabled_facade_and_test_initialize_never_open_real_files() {
        initialize();
        assert!(sink::SINK.get().is_none());
        if !enabled() {
            assert!(!health().initialized);
            assert!(!health().ready);
            assert!(health().run_id.is_none());
        }
        assert!(build_id() == "unbound" || (build_id().len() == 40 && build_id().bytes().all(|b| b.is_ascii_hexdigit())));
    }
    #[test]
    fn command_allowlist_redacts_unknown_names_and_startup_is_covered() {
        for name in ["get_settings", "list_channels", "read_credential", "get_startup_snapshot", "check_accessibility_permission", "check_microphone_permission", "get_settings_snapshot", "update_setting_fields", "set_settings", "set_qa_hotkey", "set_selection_polish_hotkey", "get_hotkey_capability", "set_shortcut_recording_active"] {
            assert!(Kind::command(name).is_some());
        }
        for secret in ["", "personal-window", "secret-token", "get_settings extra-args", "plugin:untrusted|value"] {
            assert!(Kind::command(secret).is_none());
        }
        assert_eq!(Probe::P2.hypotheses(), ["R2-H1", "R2-H2", "R2-H3", "R2-H4"]);
        assert!(matches!(Kind::command("voice_edit_window_open"), Some(Kind::VoiceEditOpen)));
    }
    #[test]
    fn webview_origin_is_a_closed_source_category_not_a_window_identity() {
        assert_eq!(WebviewKind::from_label("main"), WebviewKind::Main);
        assert_eq!(WebviewKind::from_label("private-window-name"), WebviewKind::Other);
        let records = sink::capture(|| {
            Operation::new(Probe::P1).record(Event::Entered, Outcome::Observed,
                Facts { kind: Kind::SettingsSnapshot,
                    webview: WebviewKind::from_label("private-window-name"), ..Default::default() });
        });
        assert_eq!(records[0]["safe"]["webview"], "other");
        assert!(!records[0].to_string().contains("private-window-name"));
    }
    #[test]
    fn settings_gate_is_the_real_parent_only_while_its_sync_guard_is_alive() {
        let records = sink::capture(|| {
            let mutex = parking_lot::Mutex::new(());
            let gate = lock_at(&mutex, Probe::P2, Kind::SettingsHostGate);
            let dispatch = MainDispatch::new(Kind::HostDispatch);
            dispatch.wrap(|| {})();
            dispatch.dispatched(true);
            drop(gate);
            assert!(current().is_none());
            assert!(mutex.try_lock().is_some());
        });
        let gate = records.iter().find(|r| r["probe_id"] == "P2" && r["event"] == "requested").unwrap();
        let dispatch = records.iter().find(|r| r["probe_id"] == "P4" && r["event"] == "requested").unwrap();
        assert_eq!(dispatch["parent_operation_id"], gate["operation_id"]);
        assert!(records.iter().any(|r| r["probe_id"] == "P2" && r["event"] == "released"));
    }
    #[test]
    fn cancellation_is_scope_left_and_poll_context_never_leaks_across_await() {
        let records = sink::capture(|| {
            let outer = Operation::new(Probe::P6);
            let _outer = SyncContext::enter(outer);
            let mut future = Box::pin(observe_future(Kind::ListChannels, async {
                assert_ne!(current().unwrap().id, outer.id);
                std::future::pending::<()>().await;
            }));
            fn require_send<T: Send>(_: &T) {}
            require_send(&future);
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));
            assert_eq!(current().unwrap().id, outer.id);
            drop(future);
            assert_eq!(current().unwrap().id, outer.id);
        });
        let events: Vec<_> = records.iter().map(|r| r["event"].as_str().unwrap()).collect();
        assert_eq!(events, ["entered", "scope_left"]);
        assert!(records.iter().all(|r| r["outcome"] == "observed"));
    }
    #[test]
    fn successful_future_returns_identical_result_and_real_sync_children() {
        let records = sink::capture(|| {
            let mut future = Box::pin(observe_future(Kind::ReadCredential, async {
                let parent = current().unwrap();
                let worker = Operation::new(Probe::P2);
                assert_eq!(worker.parent, parent.id);
                Err::<(), u64>(7)
            }));
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Err(7)));
            assert!(current().is_none());
        });
        assert_eq!(records[1]["event"], "returned");
        assert_eq!(records[1]["outcome"], "returned"); // Not business success.
    }
    #[test]
    fn dispatch_acceptance_is_not_execution_and_nested_parent_is_source_local() {
        let records = sink::capture(|| {
            let dispatch = MainDispatch::new(Kind::HostDispatch);
            let queued = dispatch.wrap(|| {
                let child = MainDispatch::new(Kind::CapsuleDispatch);
                child.wrap(|| {})();
                child.dispatched(true);
            });
            dispatch.dispatched(true);
            queued();
        });
        assert_eq!(records[0]["event"], "requested");
        assert_eq!(records[1]["event"], "dispatch_returned");
        assert_eq!(records[2]["event"], "entered");
        assert_eq!(records[3]["parent_operation_id"], records[0]["operation_id"]);
        assert!(current().is_none());
    }
    #[test]
    fn unwinding_restores_context_and_never_reports_returned() {
        let records = sink::capture(|| {
            let result = std::panic::catch_unwind(|| {
                MainDispatch::new(Kind::HostDispatch).wrap(|| panic!("injected"))();
            });
            assert!(result.is_err());
            assert!(current().is_none());
        });
        assert!(!records.iter().any(|r| r["event"] == "returned"));
        assert_eq!(records.last().unwrap()["event"], "scope_left");
    }

    #[test]
    fn inline_and_rejected_dispatch_do_not_invent_queued_execution() {
        let records = sink::capture(|| {
            let inline = MainDispatch::new(Kind::HostDispatch);
            inline.wrap(|| {})();
            inline.dispatched(true);
            let rejected = MainDispatch::new(Kind::CapsuleDispatch);
            let task = rejected.wrap(|| panic!("not executed"));
            drop(task);
            rejected.dispatched(false);
        });
        let events: Vec<_> = records.iter().map(|r| r["event"].as_str().unwrap()).collect();
        assert_eq!(events, ["requested", "entered", "returned", "scope_left", "dispatch_returned", "requested", "dispatch_returned"]);
        assert_eq!(records.last().unwrap()["outcome"], "error");
    }
    #[test]
    fn movable_future_restores_tls_on_each_worker_poll() {
        use std::sync::{Arc, atomic::{AtomicU64, Ordering::Relaxed}};
        let polls = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&polls);
        let mut future = Box::pin(observe_future(Kind::StartupSnapshot, std::future::poll_fn(move |_| {
            assert!(current().is_some());
            if counter.fetch_add(1, Relaxed) == 0 { Poll::Pending } else { Poll::Ready(9_u64) }
        })));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(future.as_mut().poll(&mut cx), Poll::Pending);
        assert!(current().is_none());
        std::thread::spawn(move || {
            assert!(current().is_none());
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(9));
            assert!(current().is_none());
        }).join().unwrap();
        assert_eq!(polls.load(Relaxed), 2);
    }
}
