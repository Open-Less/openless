use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
#[path = "target/macos.rs"]
mod native;
#[cfg(target_os = "windows")]
#[path = "target/windows.rs"]
mod native;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "target/unsupported.rs"]
mod native;

const CAPTURE_TIMEOUT: Duration = Duration::from_millis(1200);
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
const OBSERVER_TICK: Duration = Duration::from_millis(25);
const FOCUS_REFRESH: Duration = Duration::from_millis(175);
const MAX_SELECTION_UTF16: usize = 8_000;
const MAX_SELECTION_SCALARS: usize = 4_000;
const MAX_DOCUMENT_UTF16: usize = 20_000;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExternalSelection {
    None,
    Selected(String),
    Unavailable,
}

pub(crate) struct CapturedTarget {
    pub(crate) selection: ExternalSelection,
    pub(crate) write_target: Option<WriteTarget>,
}

impl CapturedTarget {
    fn unavailable() -> Self {
        Self {
            selection: ExternalSelection::Unavailable,
            write_target: None,
        }
    }
}

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

enum Request {
    Capture(oneshot::Sender<CapturedTarget>),
    Deliver {
        target: Arc<()>,
        operation: WriteOperation,
        text: String,
        claim: Claim,
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
        std::thread::Builder::new()
            .name("ask-external-target".into())
            .spawn(move || {
                run(receiver, &worker_active);
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
        let (reply, result) = oneshot::channel();
        if self.send(Request::Capture(reply)) {
            if let Ok(Ok(captured)) = tokio::time::timeout(CAPTURE_TIMEOUT, result).await {
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
        let guard = AbandonOnDrop(Arc::new(DeliveryPhase::default()));
        let (reply, result) = oneshot::channel();
        if !self.send(Request::Deliver {
            target: target.0.clone(),
            operation,
            text,
            claim,
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

fn run(receiver: mpsc::Receiver<Request>, active: &AtomicBool) {
    let epoch = Arc::new(AtomicU64::new(0));
    let Some(mut platform) = native::Platform::new(epoch.clone()) else {
        return;
    };
    let mut latest = Latest::new(Instant::now());
    let mut targets: Vec<(Weak<()>, native::Identity, Baseline)> = Vec::new();
    while active.load(Ordering::Acquire) {
        let request = match receiver.recv_timeout(OBSERVER_TICK) {
            Ok(request) => Some(request),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if !active.load(Ordering::Acquire) {
            break;
        }
        targets.retain(|(lease, _, _)| lease.strong_count() != 0);

        if let Some(Request::Deliver {
            target,
            operation,
            text,
            claim,
            phase,
            reply,
        }) = request
        {
            let outcome = targets
                .iter()
                .position(|(lease, _, _)| lease.as_ptr() == Arc::as_ptr(&target))
                .map_or(WriteOutcome::NotWritten, |index| {
                    let (_, identity, baseline) = targets.swap_remove(index);
                    if !active.load(Ordering::Acquire)
                        || !phase.pending()
                        || text.is_empty()
                        || text.len() > MAX_DOCUMENT_UTF16 * 4
                        || !baseline.permits(operation)
                    {
                        return WriteOutcome::NotWritten;
                    }
                    platform.write(&identity, &baseline, &text, || phase.claim(active, claim))
                });
            let _ = reply.send(outcome);
            continue;
        }

        platform.poll();
        let front = platform.foreground();
        let revision = epoch.load(Ordering::Acquire);
        let capture = match request {
            Some(Request::Capture(reply)) if !reply.is_closed() => Some(reply),
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
        if let Some(reply) = capture {
            let mut captured = CapturedTarget::unavailable();
            if let Some((identity, selection, baseline)) = latest.bound.as_ref().and_then(|bound| {
                platform
                    .capture(bound)
                    .map(|(selection, baseline)| (bound, selection, baseline))
            }) {
                platform.poll();
                if active.load(Ordering::Acquire) && !reply.is_closed() && stable(&platform) {
                    captured.selection = selection;
                    if let Some(baseline) = baseline {
                        let lease = Arc::new(());
                        targets.push((Arc::downgrade(&lease), identity.clone(), baseline));
                        captured.write_target = Some(WriteTarget(lease));
                    }
                }
            }
            if captured.selection == ExternalSelection::Unavailable {
                latest.bound = None;
            }
            let _ = reply.send(captured);
        }
    }
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
