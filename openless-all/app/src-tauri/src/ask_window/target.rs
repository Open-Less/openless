use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
#[path = "target/macos.rs"]
mod native;
#[cfg(target_os = "macos")]
pub(super) use native::FocusRecovery;
#[cfg(target_os = "windows")]
#[path = "target/windows.rs"]
mod native;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "target/unsupported.rs"]
mod native;

const CAPTURE_TIMEOUT: Duration = Duration::from_millis(1200);
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
const TARGET_FRESHNESS: Duration = Duration::from_secs(90);
const OBSERVER_TICK: Duration = Duration::from_millis(25);
const FOCUS_REFRESH: Duration = Duration::from_millis(175);
const MAX_SELECTION_UTF16: usize = 8_000;
const MAX_SELECTION_SCALARS: usize = 4_000;
const MAX_DOCUMENT_UTF16: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExternalSelection {
    None,
    Selected(String),
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct CapturedTarget {
    pub(crate) selection: ExternalSelection,
    pub(crate) write_target: Option<WriteTarget>,
}

impl CapturedTarget {
    pub(super) fn unavailable() -> Self {
        Self {
            selection: ExternalSelection::Unavailable,
            write_target: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct WriteTarget(Arc<()>);

#[derive(Clone, Copy)]
pub(crate) enum WriteOperation {
    Insert,
    Replace,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WriteOutcome {
    Written,
    NotWritten,
    Unknown,
    Declined,
}

#[derive(Default)]
pub(crate) struct TargetTracker {
    running: Mutex<Option<Running>>,
    scope: Arc<AtomicU64>,
}

struct Running {
    sender: mpsc::SyncSender<Request>,
    active: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

type Claim = Box<dyn FnOnce() -> bool + Send>;
type Eligible = Arc<dyn Fn() -> bool + Send + Sync>;

enum Request {
    Capture {
        expected_pid: Option<u32>,
        scope: Option<u64>,
        reply: oneshot::Sender<CapturedTarget>,
    },
    Deliver {
        target: Arc<()>,
        operation: WriteOperation,
        text: String,
        claim: Claim,
        eligible: Eligible,
        #[cfg(target_os = "macos")]
        recover: Arc<dyn Fn(FocusRecovery) + Send + Sync>,
        phase: Arc<DeliveryPhase>,
        reply: oneshot::Sender<WriteOutcome>,
    },
}

impl TargetTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn start(&self) -> io::Result<()> {
        let mut running = self.running.lock().unwrap();
        if running
            .as_ref()
            .is_some_and(|run| run.active.load(Ordering::Acquire))
        {
            return Ok(());
        }
        let (sender, receiver) = mpsc::sync_channel(4);
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = active.clone();
        let scope = self.scope.clone();
        std::thread::Builder::new()
            .name("ask-external-target".into())
            .spawn(move || {
                run(receiver, &worker_active, &scope);
                worker_active.store(false, Ordering::Release);
            })?;
        *running = Some(Running { sender, active });
        Ok(())
    }

    pub(crate) fn stop(&self) {
        self.running.lock().unwrap().take();
    }

    fn send(&self, request: Request) -> bool {
        self.running
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|run| run.sender.try_send(request).is_ok())
    }

    pub(crate) async fn capture(&self) -> CapturedTarget {
        self.capture_for(None, None, CAPTURE_TIMEOUT).await
    }

    pub(crate) fn clear_capture(&self) {
        self.scope.store(0, Ordering::Release);
    }

    pub(crate) async fn capture_before_focus(&self, pid: u32, scope: u64) -> CapturedTarget {
        self.scope.store(scope, Ordering::Release);
        self.capture_for(Some(pid), Some(scope), Duration::from_millis(400)).await
    }

    pub(crate) async fn capture_at_send(&self, scope: u64) -> CapturedTarget {
        self.capture_for(None, Some(scope), CAPTURE_TIMEOUT).await
    }

    async fn capture_for(
        &self, expected_pid: Option<u32>, scope: Option<u64>, timeout: Duration,
    ) -> CapturedTarget {
        let (reply, result) = oneshot::channel();
        if self.send(Request::Capture { expected_pid, scope, reply }) {
            if let Ok(Ok(captured)) = tokio::time::timeout(timeout, result).await {
                return captured;
            }
        }
        CapturedTarget::unavailable()
    }

    pub(crate) async fn deliver(
        &self,
        target: &WriteTarget,
        operation: WriteOperation,
        text: String,
        claim: Claim,
    ) -> WriteOutcome {
        self.deliver_scoped(target, operation, text, Arc::new(|| true),
            #[cfg(target_os = "macos")]
            Arc::new(|_| {}),
            claim).await
    }

    pub(crate) async fn deliver_scoped(
        &self, target: &WriteTarget, operation: WriteOperation, text: String,
        eligible: Eligible,
        #[cfg(target_os = "macos")]
        recover: Arc<dyn Fn(FocusRecovery) + Send + Sync>,
        claim: Claim,
    ) -> WriteOutcome {
        let guard = AbandonOnDrop(Arc::new(DeliveryPhase::default()));
        let (reply, result) = oneshot::channel();
        if !self.send(Request::Deliver {
            target: target.0.clone(),
            operation,
            text,
            claim,
            eligible,
            #[cfg(target_os = "macos")]
            recover,
            phase: guard.0.clone(),
            reply,
        }) {
            return WriteOutcome::NotWritten;
        }
        match tokio::time::timeout(DELIVERY_TIMEOUT, result).await {
            Ok(Ok(outcome)) => outcome,
            _ => guard.0.abandon(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Foreground<Key> {
    Own,
    External(Key),
    Unknown,
}

struct Latest<Key, Identity> {
    front: Foreground<Key>,
    epoch: u64,
    bound: Option<Identity>,
    refreshed: Instant,
}

impl<Key: Copy + Eq, Identity> Latest<Key, Identity> {
    fn new(now: Instant) -> Self {
        Self {
            front: Foreground::Unknown,
            epoch: 0,
            bound: None,
            refreshed: now,
        }
    }

    fn refresh(
        &mut self,
        front: Foreground<Key>,
        epoch: u64,
        now: Instant,
        force: bool,
        bind: impl FnOnce(Key) -> Option<Identity>,
    ) {
        match front {
            Foreground::External(key)
                if force
                    || front != self.front
                    || epoch != self.epoch
                    || now.duration_since(self.refreshed) >= FOCUS_REFRESH =>
            {
                self.bound = bind(key);
                self.refreshed = now;
            }
            Foreground::Unknown => self.bound = None,
            Foreground::Own if self.epoch != epoch => self.bound = None,
            _ => {}
        }
        self.epoch = epoch;
        self.front = front;
    }
}

type TargetLease = (Weak<()>, native::Identity, Baseline, Instant, u64);

#[cfg(target_os = "macos")]
struct CapturedScope {
    id: u64,
    revision: u64,
    at: Instant,
    identity: native::Identity,
}

fn run(receiver: mpsc::Receiver<Request>, active: &AtomicBool, scope: &AtomicU64) {
    let epoch = Arc::new(AtomicU64::new(0));
    let Some(mut platform) = native::Platform::new(epoch.clone()) else {
        return;
    };
    let mut latest = Latest::new(Instant::now());
    let mut targets: Vec<TargetLease> = Vec::new();
    #[cfg(target_os = "macos")]
    let mut captured_scope: Option<CapturedScope> = None;
    while active.load(Ordering::Acquire) {
        let request = match receiver.recv_timeout(OBSERVER_TICK) {
            Ok(request) => Some(request),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if !active.load(Ordering::Acquire) {
            break;
        }
        targets.retain(|(lease, _, _, at, _)| {
            lease.strong_count() != 0 && at.elapsed() <= TARGET_FRESHNESS
        });
        platform.poll();
        #[cfg(target_os = "macos")]
        if captured_scope.as_ref().is_some_and(|item| {
            item.id != scope.load(Ordering::Acquire) || item.at.elapsed() > TARGET_FRESHNESS
        }) {
            captured_scope = None;
        }

        if let Some(Request::Deliver {
            target,
            operation,
            text,
            claim,
            eligible,
            #[cfg(target_os = "macos")]
            recover,
            phase,
            reply,
        }) = request
        {
            let outcome = targets
                .iter()
                .position(|(lease, _, _, _, _)| lease.as_ptr() == Arc::as_ptr(&target))
                .map_or(WriteOutcome::NotWritten, |index| {
                    let (_, identity, baseline, _, revision) = targets.swap_remove(index);
                    if !active.load(Ordering::Acquire)
                        || !phase.pending()
                        || !eligible()
                        || revision != epoch.load(Ordering::Acquire)
                        || text.is_empty()
                        || text.len() > MAX_DOCUMENT_UTF16 * 4
                        || !baseline.permits(operation)
                    {
                        return WriteOutcome::NotWritten;
                    }
                    #[cfg(target_os = "macos")]
                    return platform.write(
                        &identity, &baseline, &text,
                        || active.load(Ordering::Acquire) && phase.pending() && eligible(),
                        |proof| recover(proof),
                        || phase.claim(active, claim),
                    );
                    #[cfg(not(target_os = "macos"))]
                    platform.write(&identity, &baseline, &text, || phase.claim(active, claim))
                });
            let _ = reply.send(outcome);
            continue;
        }

        let front = platform.foreground();
        let revision = epoch.load(Ordering::Acquire);
        let capture = match request {
            Some(Request::Capture { expected_pid, scope: requested, reply }) if !reply.is_closed() => {
                #[cfg(target_os = "macos")]
                let mismatched = expected_pid
                    .is_some_and(|pid| front != Foreground::External(pid as i32));
                #[cfg(not(target_os = "macos"))]
                let mismatched = expected_pid.is_some();
                if mismatched || requested.is_some_and(|id| id != scope.load(Ordering::Acquire)) {
                    let _ = reply.send(CapturedTarget::unavailable());
                    continue;
                }
                Some((expected_pid, requested, reply))
            }
            _ => None,
        };
        latest.refresh(front, revision, Instant::now(), capture.is_some(), |key| {
            platform.bind(key)
        });
        platform.poll();
        let stable = |platform: &native::Platform| {
            platform.foreground() == front && epoch.load(Ordering::Acquire) == revision
        };
        if !stable(&platform) {
            latest.bound = None;
        }
        if let Some((expected_pid, requested, reply)) = capture {
            let mut captured = CapturedTarget::unavailable();
            #[cfg(target_os = "macos")]
            let retained = captured_scope.as_ref().filter(|item| {
                expected_pid.is_none() && requested == Some(item.id)
                    && revision == item.revision && front == Foreground::Own
            });
            let bound = latest.bound.as_ref();
            #[cfg(target_os = "macos")]
            let bound = if front == Foreground::Own && requested.is_some() {
                bound.or_else(|| retained.map(|old| &old.identity))
            } else { bound };
            #[cfg(not(target_os = "macos"))]
            let _ = expected_pid;
            if let Some(identity) = bound {
                #[cfg(target_os = "macos")]
                let read = if front == Foreground::Own && requested.is_some() {
                    platform.capture_retained(identity)
                } else {
                    platform.capture(identity)
                };
                #[cfg(not(target_os = "macos"))]
                let read = platform.capture(identity);
                platform.poll();
                if active.load(Ordering::Acquire) && !reply.is_closed() && stable(&platform)
                    && requested.is_none_or(|id| id == scope.load(Ordering::Acquire))
                {
                    if let Some((selection, baseline)) = read {
                        captured = lease_capture(&mut targets, identity, selection, baseline, revision);
                    }
                    #[cfg(target_os = "macos")]
                    if let (Some(_), Some(id)) = (expected_pid, requested) {
                        captured_scope = Some(CapturedScope {
                            id, revision, at: Instant::now(), identity: identity.clone(),
                        });
                    }
                }
            }
            let _ = reply.send(captured);
        }
    }
}

fn lease_capture(
    targets: &mut Vec<TargetLease>, identity: &native::Identity, selection: ExternalSelection,
    baseline: Option<Baseline>, revision: u64,
) -> CapturedTarget {
    let write_target = baseline.map(|baseline| {
        let lease = Arc::new(());
        targets.push((Arc::downgrade(&lease), identity.clone(), baseline, Instant::now(), revision));
        WriteTarget(lease)
    });
    CapturedTarget { selection, write_target }
}

#[derive(PartialEq, Eq)]
struct Baseline {
    document: Vec<u16>,
    selection: Range<usize>,
}

impl Baseline {
    fn new(document: Vec<u16>, selection: Range<usize>) -> Option<Self> {
        let selected = document.get(selection.clone())?;
        if document.len() > MAX_DOCUMENT_UTF16
            || char::decode_utf16(document.iter().copied()).any(|unit| unit.is_err())
            || !utf16_boundary(&document, selection.start)
            || !utf16_boundary(&document, selection.end)
            || selection_from_units(selected).is_none()
        {
            return None;
        }
        Some(Self {
            document,
            selection,
        })
    }

    fn permits(&self, operation: WriteOperation) -> bool {
        match operation {
            WriteOperation::Insert => self.selection.is_empty(),
            WriteOperation::Replace => !self.selection.is_empty(),
        }
    }

    fn expected(&self, text: &[u16]) -> Option<Vec<u16>> {
        let len = self.document.len() - self.selection.len() + text.len();
        if len > MAX_DOCUMENT_UTF16 {
            return None;
        }
        let mut result = Vec::with_capacity(len);
        result.extend_from_slice(&self.document[..self.selection.start]);
        result.extend_from_slice(text);
        result.extend_from_slice(&self.document[self.selection.end..]);
        Some(result)
    }

    fn selected(&self) -> &[u16] {
        &self.document[self.selection.clone()]
    }
}

fn utf16_boundary(units: &[u16], offset: usize) -> bool {
    units
        .get(offset)
        .is_none_or(|unit| !(0xDC00..=0xDFFF).contains(unit))
}

fn selection_from_units(units: &[u16]) -> Option<ExternalSelection> {
    if units.is_empty() {
        return Some(ExternalSelection::None);
    }
    if units.len() > MAX_SELECTION_UTF16 {
        return None;
    }
    let text = String::from_utf16(units).ok()?;
    (text.chars().count() <= MAX_SELECTION_SCALARS).then_some(ExternalSelection::Selected(text))
}

const PENDING: u8 = 0;
const AUTHORIZING: u8 = 1;
const WRITING: u8 = 2;
const DECLINED: u8 = 3;
const ABANDONED: u8 = 4;

#[derive(Default)]
struct DeliveryPhase(AtomicU8);

impl DeliveryPhase {
    fn pending(&self) -> bool {
        self.0.load(Ordering::Acquire) == PENDING
    }

    fn claim(&self, active: &AtomicBool, claim: Claim) -> Result<(), WriteOutcome> {
        if !active.load(Ordering::Acquire)
            || self
                .0
                .compare_exchange(PENDING, AUTHORIZING, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(WriteOutcome::NotWritten);
        }
        let (next, result) = if !claim() {
            (DECLINED, Err(WriteOutcome::Declined))
        } else if !active.load(Ordering::Acquire) {
            (ABANDONED, Err(WriteOutcome::NotWritten))
        } else {
            (WRITING, Ok(()))
        };
        self.0
            .compare_exchange(AUTHORIZING, next, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| WriteOutcome::NotWritten)?;
        result
    }

    fn abandon(&self) -> WriteOutcome {
        match self.0.swap(ABANDONED, Ordering::AcqRel) {
            WRITING => WriteOutcome::Unknown,
            DECLINED => WriteOutcome::Declined,
            _ => WriteOutcome::NotWritten,
        }
    }
}

struct AbandonOnDrop(Arc<DeliveryPhase>);

impl Drop for AbandonOnDrop {
    fn drop(&mut self) {
        self.0.abandon();
    }
}

#[cfg(test)]
#[path = "target/tests.rs"]
mod tests;
