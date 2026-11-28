//! UIA belongs exclusively to this non-UI MTA. Readers never wait for COM.
//! Published proofs expire and are fenced by native focus events and HWND identity.
use openless_core::SessionId;
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{implement, Interface, Result as WinResult},
    Win32::{
        Foundation::HWND,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};

const TICK: Duration = Duration::from_millis(25);
const PROOF_LIFETIME: Duration = Duration::from_millis(150);
const DEMAND_LIFETIME: Duration = Duration::from_millis(200);
const QUERY_MS: u32 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    pub session: SessionId,
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Native {
    window: usize,
    process: u32,
    thread: u32,
    control: usize,
    control_process: u32,
    control_thread: u32,
}
impl Native {
    fn current() -> Option<Self> {
        unsafe {
            let window = GetForegroundWindow();
            if window.0.is_null() {
                return None;
            }
            let mut process = 0;
            let thread = GetWindowThreadProcessId(window, Some(&mut process));
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            if thread == 0
                || process == 0
                || GetGUIThreadInfo(thread, &mut info).is_err()
                || info.hwndFocus.0.is_null()
            {
                return None;
            }
            let mut control_process = 0;
            let control_thread =
                GetWindowThreadProcessId(info.hwndFocus, Some(&mut control_process));
            if control_thread == 0 || control_process != process || GetForegroundWindow() != window
            {
                return None;
            }
            Some(Self {
                window: window.0 as usize,
                process,
                thread,
                control: info.hwndFocus.0 as usize,
                control_process,
                control_thread,
            })
        }
    }
}

#[derive(Clone, Copy)]
struct Proof {
    scope: Scope,
    native: Native,
    event: u64,
    serial: u64,
    queried: Instant,
    verified: bool,
}
struct Shared {
    proof: Mutex<Option<Proof>>,
    demand: Mutex<Option<(Scope, Instant)>>,
    events: Arc<AtomicU64>,
    publication: AtomicU64,
    observer: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    stopped: AtomicBool,
}
impl Shared {
    fn clear(&self) {
        let mut proof = self.proof.lock();
        if proof.take().is_some() {
            self.publication.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn notify(&self) {
        let observer = self.observer.lock().clone();
        if let Some(observer) = observer {
            observer();
        }
    }
}
type Job = Box<dyn FnOnce() + Send>;
struct Request {
    scope: Scope,
    queued: Instant,
    origin: Native,
    event: u64,
    job: Job,
}
struct Handle {
    shared: Arc<Shared>,
    sender: mpsc::SyncSender<Request>,
    thread: thread::Thread,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.clear();
        self.thread.unpark();
    }
}
#[derive(Clone)]
pub(crate) struct Owner(Arc<Handle>);
impl Owner {
    pub(crate) fn start() -> std::io::Result<Self> {
        Self::start_with_uia(true)
    }
    fn start_with_uia(allow_uia: bool) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            proof: Mutex::new(None),
            demand: Mutex::new(None),
            events: Arc::new(AtomicU64::new(0)),
            publication: AtomicU64::new(0),
            observer: Mutex::new(None),
            stopped: AtomicBool::new(false),
        });
        let (sender, receiver) = mpsc::sync_channel::<Request>(8);
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("capsule-focus-mta".into())
            .spawn(move || {
                run(&worker, receiver, allow_uia);
                worker.stopped.store(true, Ordering::Release);
                worker.clear();
                worker.notify();
            })?;
        Ok(Self(Arc::new(Handle {
            shared,
            sender,
            thread: thread.thread().clone(),
        })))
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn without_uia_for_test() -> std::io::Result<Self> {
        Self::start_with_uia(false)
    }
    pub(crate) fn revision(&self) -> (u64, u64) {
        (
            self.0.shared.events.load(Ordering::Acquire),
            self.0.shared.publication.load(Ordering::Acquire),
        )
    }
    pub(crate) fn set_observer(&self, observer: Arc<dyn Fn() + Send + Sync>) {
        *self.0.shared.observer.lock() = Some(observer);
    }
    /// Only value data crosses threads; no UIA interface leaves the owner.
    pub(crate) fn current(&self, scope: Scope) -> Option<(u64, u64, bool)> {
        let shared = &self.0.shared;
        if shared.stopped.load(Ordering::Acquire) {
            return None;
        }
        {
            let mut demand = shared.demand.lock();
            if demand.as_ref().is_some_and(|(newer, _)| {
                newer.epoch > scope.epoch
                    || (newer.epoch == scope.epoch && newer.session != scope.session)
            }) {
                return None;
            }
            *demand = Some((scope, Instant::now()));
        }
        if thread::current().id() != self.0.thread.id() {
            self.0.thread.unpark();
        }
        let proof = (*shared.proof.lock())?;
        let event = shared.events.load(Ordering::Acquire);
        if proof.scope != scope
            || proof.event != event
            || proof.queried.elapsed() > PROOF_LIFETIME
            || Native::current() != Some(proof.native)
            || shared.events.load(Ordering::Acquire) != event
        {
            return None;
        }
        Some((
            proof.serial,
            u64::from(proof.native.process),
            proof.verified,
        ))
    }
    /// Initial hotkey capture must complete on the owner before creating intent.
    /// Neither the Tauri UI thread nor a keyboard hook waits on this job.
    pub(crate) fn submit(&self, scope: Scope, job: impl FnOnce() + Send + 'static) -> bool {
        if self.0.shared.stopped.load(Ordering::Acquire) {
            return false;
        }
        let Some(origin) = Native::current() else {
            return false;
        };
        let event = self.0.shared.events.load(Ordering::Acquire);
        let accepted = self
            .0
            .sender
            .try_send(Request {
                scope,
                queued: Instant::now(),
                origin,
                event,
                job: Box::new(job),
            })
            .is_ok();
        if accepted {
            self.0.thread.unpark();
        }
        accepted
    }
}

thread_local! {
    static EVENTS: RefCell<Option<Arc<AtomicU64>>> = const { RefCell::new(None) };
}
unsafe extern "system" fn focus_changed(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    EVENTS.with(|slot| {
        if let Some(epoch) = slot.borrow().as_ref() {
            epoch.fetch_add(1, Ordering::AcqRel);
        }
    });
}
#[implement(IUIAutomationFocusChangedEventHandler)]
struct Changed(Arc<AtomicU64>);
impl IUIAutomationFocusChangedEventHandler_Impl for Changed_Impl {
    fn HandleFocusChangedEvent(&self, _: Option<&IUIAutomationElement>) -> WinResult<()> {
        // Callback threads only invalidate; they never call back into UIA.
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
struct Platform {
    uia: Option<IUIAutomation>,
    handler: Option<IUIAutomationFocusChangedEventHandler>,
    hooks: Vec<HWINEVENTHOOK>,
    _apartment: Apartment,
}
impl Platform {
    fn new(events: Arc<AtomicU64>, allow_uia: bool) -> Option<Self> {
        unsafe {
            // Only this freshly created thread initializes COM. Never change
            // the apartment of a caller (especially Tauri's STA/OLE thread).
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
            let mut platform = Self {
                uia: None,
                handler: None,
                hooks: vec![],
                _apartment: Apartment,
            };
            EVENTS.with(|slot| *slot.borrow_mut() = Some(events.clone()));
            for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS] {
                let hook = SetWinEventHook(
                    event,
                    event,
                    None,
                    Some(focus_changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                if hook.is_invalid() {
                    return None;
                }
                platform.hooks.push(hook);
            }
            // UIA unavailable -> native HWND-bound mouse-only proof. This is
            // permission/provider failure on the owner, never an STA error.
            let setup = || -> WinResult<(IUIAutomation, IUIAutomationFocusChangedEventHandler)> {
                let uia: IUIAutomation =
                    CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
                let timeouts: IUIAutomation2 = uia.cast()?;
                timeouts.SetConnectionTimeout(QUERY_MS)?;
                timeouts.SetTransactionTimeout(QUERY_MS)?;
                let handler: IUIAutomationFocusChangedEventHandler = Changed(events).into();
                uia.AddFocusChangedEventHandler(None, &handler)?;
                Ok((uia, handler))
            };
            if allow_uia {
                if let Ok((uia, handler)) = setup() {
                    platform.uia = Some(uia);
                    platform.handler = Some(handler);
                }
            }
            Some(platform)
        }
    }
    fn poll(&self) {
        unsafe {
            let mut message = MSG::default();
            for _ in 0..64 {
                if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    fn focused(&self, native: Native) -> Option<IUIAutomationElement> {
        unsafe {
            let element = self.uia.as_ref()?.GetFocusedElement().ok()?;
            if element.CurrentProcessId().ok()? != native.process as i32
                || element.CurrentIsPassword().ok()?.as_bool()
            {
                return None;
            }
            Some(element)
        }
    }
}
impl Drop for Platform {
    fn drop(&mut self) {
        unsafe {
            if let (Some(uia), Some(handler)) = (&self.uia, &self.handler) {
                let _ = uia.RemoveFocusChangedEventHandler(handler);
            }
            for hook in &self.hooks {
                let _ = UnhookWinEvent(*hook);
            }
        }
        EVENTS.with(|slot| *slot.borrow_mut() = None);
    }
}
struct Captured {
    native: Native,
    event: u64,
    element: Option<IUIAutomationElement>,
}
fn capture(
    platform: &Platform,
    shared: &Shared,
    scope: Scope,
    serial: &mut u64,
    captured: &mut Option<Captured>,
) {
    let queried = Instant::now();
    let event = shared.events.load(Ordering::Acquire);
    let Some(native) = Native::current() else {
        shared.clear();
        *captured = None;
        return;
    };
    let element = platform.focused(native);
    // Deliver queued WinEvents before comparing the before/after fence.
    platform.poll();
    if shared.stopped.load(Ordering::Acquire)
        || queried.elapsed() > PROOF_LIFETIME
        || shared.events.load(Ordering::Acquire) != event
        || Native::current() != Some(native)
    {
        shared.clear();
        return;
    }
    // A gap in proof validity is a new ownership generation even when the
    // same HWND/control comes back. A prepared/active menu cannot revive just
    // because no reader happened to observe the expired interval.
    let identity_same = captured.as_ref().is_some_and(|old| {
        old.native == native
            && old.event == event
            && match (&old.element, &element, &platform.uia) {
                (Some(old), Some(new), Some(uia)) => unsafe {
                    uia.CompareElements(old, new)
                        .is_ok_and(|same| same.as_bool())
                },
                (None, None, _) => true,
                _ => false,
            }
    });
    let continuous = shared
        .proof
        .lock()
        .as_ref()
        .is_some_and(|old| old.queried.elapsed() <= PROOF_LIFETIME);
    if !identity_same || !continuous {
        *serial = serial
            .checked_add(1)
            .expect("capsule focus serial exhausted");
    }
    let mut proof = Proof {
        scope,
        native,
        event,
        serial: *serial,
        queried,
        verified: element.is_some(),
    };
    *captured = Some(Captured {
        native,
        event,
        element,
    });
    // No old-scope completion can publish over a newer submitted demand.
    if shared
        .demand
        .lock()
        .as_ref()
        .is_some_and(|(wanted, _)| *wanted == scope)
        && shared.events.load(Ordering::Acquire) == event
        && Native::current() == Some(native)
        && queried.elapsed() <= PROOF_LIFETIME
    {
        let mut slot = shared.proof.lock();
        if slot
            .as_ref()
            .is_some_and(|old| old.serial == proof.serial && old.queried.elapsed() > PROOF_LIFETIME)
        {
            *serial = serial
                .checked_add(1)
                .expect("capsule focus serial exhausted");
            proof.serial = *serial;
        }
        let changed = slot.as_ref().is_none_or(|old| {
            old.scope != proof.scope
                || old.serial != proof.serial
                || old.event != proof.event
                || old.verified != proof.verified
                || old.queried.elapsed() > PROOF_LIFETIME
        });
        *slot = Some(proof);
        if changed {
            shared.publication.fetch_add(1, Ordering::AcqRel);
        }
    }
}
fn run(shared: &Shared, receiver: mpsc::Receiver<Request>, allow_uia: bool) {
    let Some(platform) = Platform::new(shared.events.clone(), allow_uia) else {
        log::warn!("[capsule] MTA focus owner initialization failed; input ownership unavailable");
        return;
    };
    let (mut serial, mut captured) = (0, None);
    while !shared.stopped.load(Ordering::Acquire) {
        platform.poll();
        match receiver.try_recv() {
            Ok(request) => {
                if request.queued.elapsed() <= PROOF_LIFETIME
                    && shared.events.load(Ordering::Acquire) == request.event
                    && Native::current() == Some(request.origin)
                {
                    {
                        let mut demand = shared.demand.lock();
                        if demand.as_ref().is_some_and(|(wanted, _)| {
                            wanted.epoch > request.scope.epoch
                                || (wanted.epoch == request.scope.epoch
                                    && wanted.session != request.scope.session)
                        }) {
                            continue;
                        }
                        *demand = Some((request.scope, Instant::now()));
                    }
                    capture(&platform, shared, request.scope, &mut serial, &mut captured);
                    let proof_current = shared.proof.lock().as_ref().is_some_and(|proof| {
                        proof.scope == request.scope
                            && proof.event == request.event
                            && proof.queried.elapsed() <= PROOF_LIFETIME
                    });
                    if !shared.stopped.load(Ordering::Acquire)
                        && shared.events.load(Ordering::Acquire) == request.event
                        && Native::current() == Some(request.origin)
                        && proof_current
                    {
                        (request.job)();
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let demand = *shared.demand.lock();
        if let Some((scope, _)) = demand.filter(|(_, touched)| touched.elapsed() <= DEMAND_LIFETIME)
        {
            capture(&platform, shared, scope, &mut serial, &mut captured);
        } else {
            shared.clear();
            captured = None;
        }
        // Reconcile on the owner too: a transient invalid view must not stop
        // the main-thread watch and leave stale geometry/UI without cleanup.
        shared.notify();
        thread::park_timeout(TICK);
    }
}
