use super::*;
use crate::host_document::{evaluate_gate, macos::*, GateInputs};
use block2::RcBlock;
use core_foundation::{
    array::CFArray,
    base::{CFRange, CFType, TCFType},
    boolean::CFBoolean,
    number::CFNumber,
    runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource},
    string::{CFString, CFStringGetCharacters, CFStringGetLength, CFStringRef},
};
use objc2::{
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject, Bool},
};
use std::{ffi::c_void, ptr, thread};

const AX_RANGE: i32 = 4;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementGetTypeID() -> usize;
    fn AXValueGetTypeID() -> usize;
    fn AXUIElementPerformAction(element: AxUiElementRef, action: CFStringRef) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: AxUiElementRef,
        name: CFStringRef,
        out: *mut u8,
    ) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
}

#[derive(Clone)]
pub(in crate::ask_window) struct FocusRecovery {
    pid: i32,
    space: u64,
    input: [u32; 6],
}

impl FocusRecovery {
    fn event_counts() -> [u32; 6] {
        [1, 3, 25, 10, 12, 22].map(|kind| unsafe { CGEventSourceCounterForEventType(1, kind) })
    }

    fn before_activation(pid: i32) -> Self {
        Self { pid, space: super::super::active_space_epoch(), input: Self::event_counts() }
    }

    pub(in crate::ask_window) fn is_current(&self) -> bool {
        super::super::foreground_pid() == Some(self.pid as u32)
            && super::super::active_space_epoch() == self.space
            && Self::event_counts() == self.input
    }
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSWorkspaceDidActivateApplicationNotification: *const AnyObject;
    static NSWorkspaceApplicationKey: *const AnyObject;
}

const AX_UNSUPPORTED: i32 = -25205;
const AX_NO_VALUE: i32 = -25212;
const AX_MESSAGE_SECONDS: f32 = 0.065;
const FOCUS_NOTIFICATIONS: [&str; 2] = ["AXFocusedUIElementChanged", "AXFocusedWindowChanged"];

pub(super) struct Platform {
    center: Retained<AnyObject>,
    token: Retained<AnyObject>,
    watch: Option<FocusWatch>,
    epoch: Arc<AtomicU64>,
}

#[derive(Clone)]
pub(super) struct Identity {
    pid: i32,
    application: Retained<AnyObject>,
    ax_app: CFType,
    window: CFType,
    element: CFType,
}

struct FocusWatch {
    pid: i32,
    app: CFType,
    source: CFRunLoopSource,
    _observer: CFType,
    _context: Box<FocusContext>,
}

struct FocusContext {
    pid: i32,
    epoch: Arc<AtomicU64>,
}

unsafe extern "C" fn focus_changed(
    _: AxObserverRef,
    _: AxUiElementRef,
    _: *const c_void,
    context: *mut c_void,
) {
    let context = &*context.cast::<FocusContext>();
    if frontmost().is_some_and(|app| pid(&app) == context.pid) {
        context.epoch.fetch_add(1, Ordering::AcqRel);
    }
}

impl FocusWatch {
    fn new(pid: i32, epoch: Arc<AtomicU64>) -> Option<Self> {
        unsafe {
            let app = ax(AXUIElementCreateApplication(pid), pid)?;
            let mut observer = ptr::null_mut();
            if AXObserverCreate(pid, focus_changed, &mut observer) != 0 || observer.is_null() {
                return None;
            }
            let mut context = Box::new(FocusContext { pid, epoch });
            let observer = CFType::wrap_under_create_rule(observer.cast());
            for name in FOCUS_NOTIFICATIONS {
                let name = CFString::new(name);
                if AXObserverAddNotification(
                    observer.as_CFTypeRef().cast_mut().cast(),
                    raw_ax(&app),
                    name.as_CFTypeRef(),
                    ptr::from_mut(&mut *context).cast(),
                ) != 0
                {
                    return None;
                }
            }
            let source = AXObserverGetRunLoopSource(observer.as_CFTypeRef().cast_mut().cast());
            if source.is_null() {
                return None;
            }
            let source = CFRunLoopSource::wrap_under_get_rule(source);
            CFRunLoop::get_current().add_source(&source, kCFRunLoopDefaultMode);
            Some(Self {
                pid,
                app,
                source,
                _observer: observer,
                _context: context,
            })
        }
    }
}

impl Drop for FocusWatch {
    fn drop(&mut self) {
        unsafe { CFRunLoop::get_current().remove_source(&self.source, kCFRunLoopDefaultMode) }
    }
}

fn workspace() -> Option<*mut AnyObject> {
    unsafe {
        let class = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        (!workspace.is_null()).then_some(workspace)
    }
}

fn frontmost() -> Option<Retained<AnyObject>> {
    unsafe { Retained::retain(msg_send![workspace()?, frontmostApplication]) }
}

fn pid(app: &AnyObject) -> i32 {
    unsafe { msg_send![app, processIdentifier] }
}

fn secure_input() -> bool {
    crate::unicode_keystroke::is_secure_input_enabled()
}

impl Platform {
    pub(super) fn new(epoch: Arc<AtomicU64>) -> Option<Self> {
        autoreleasepool(|_| unsafe {
            let center: Retained<AnyObject> =
                Retained::retain(msg_send![workspace()?, notificationCenter])?;
            let external_epoch = epoch.clone();
            let block = RcBlock::new(move |notification: *mut AnyObject| {
                let info: *mut AnyObject = msg_send![notification, userInfo];
                let app: *mut AnyObject = msg_send![info, objectForKey: NSWorkspaceApplicationKey];
                if app
                    .as_ref()
                    .is_none_or(|app| pid(app) as u32 != std::process::id())
                {
                    external_epoch.fetch_add(1, Ordering::AcqRel);
                }
            });
            let token = Retained::retain(msg_send![
                &*center, addObserverForName: NSWorkspaceDidActivateApplicationNotification,
                object: ptr::null::<AnyObject>(), queue: ptr::null::<AnyObject>(), usingBlock: &*block
            ])?;
            Some(Self {
                center,
                token,
                watch: None,
                epoch,
            })
        })
    }

    pub(super) fn poll(&mut self) {
        autoreleasepool(|_| unsafe {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::ZERO, false);
        });
    }

    pub(super) fn foreground(&self) -> Foreground<i32> {
        autoreleasepool(|_| match frontmost().map(|app| pid(&app)) {
            Some(pid) if pid as u32 == std::process::id() => Foreground::Own,
            Some(pid) if pid > 0 => Foreground::External(pid),
            _ => Foreground::Unknown,
        })
    }

    pub(super) fn bind(&mut self, expected_pid: i32) -> Option<Identity> {
        autoreleasepool(|_| unsafe {
            if self
                .watch
                .as_ref()
                .is_some_and(|watch| watch.pid != expected_pid)
            {
                self.watch = None;
            }
            if secure_input() || !AXIsProcessTrusted() {
                return None;
            }
            let application = frontmost().filter(|app| pid(app) == expected_pid)?;
            let bundle_id = {
                let bundle: *const AnyObject = msg_send![&*application, bundleIdentifier];
                (!bundle.is_null())
                    .then(|| CFString::wrap_under_get_rule(bundle.cast()).to_string())
            };
            if evaluate_gate(&GateInputs {
                bundle_id,
                ..GateInputs::default()
            })
            .is_some()
            {
                return None;
            }
            if self.watch.is_none() {
                self.watch = FocusWatch::new(expected_pid, self.epoch.clone());
            }
            let ax_app = match self.watch.as_ref() {
                Some(watch) => watch.app.clone(),
                None => ax(AXUIElementCreateApplication(expected_pid), expected_pid)?,
            };
            let window = element_attr(&ax_app, "AXFocusedWindow", expected_pid)?;
            let element = element_attr(&ax_app, "AXFocusedUIElement", expected_pid)?;
            Some(Identity {
                pid: expected_pid,
                application,
                ax_app,
                window,
                element,
            })
        })
    }

    pub(super) fn capture(
        &self,
        identity: &Identity,
    ) -> Option<(ExternalSelection, Option<Baseline>)> {
        autoreleasepool(|_| {
            identity.verify()?;
            let snapshot = identity.selection()?;
            identity.verify()?;
            Some(snapshot)
        })
    }

    pub(super) fn capture_retained(
        &self, identity: &Identity,
    ) -> Option<(ExternalSelection, Option<Baseline>)> {
        autoreleasepool(|_| {
            identity.verify()?;
            let snapshot = identity.selection()?;
            identity.verify()?;
            Some(snapshot)
        })
    }

    pub(super) fn write(
        &self,
        identity: &Identity,
        baseline: &Baseline,
        operation: WriteOperation,
        text: &str,
        eligible: impl Fn() -> bool,
        recover: impl Fn(FocusRecovery),
        claim: impl FnOnce() -> Result<(), WriteOutcome>,
    ) -> WriteOutcome {
        autoreleasepool(|_| {
            let text_units: Vec<_> = text.encode_utf16().collect();
            let Some(expected) = baseline.expected(&text_units) else {
                return WriteOutcome::NotWritten;
            };
            if identity.verify_control().is_none()
                || document(&identity.element).as_ref() != Some(&baseline.document)
                || (matches!(operation, WriteOperation::Replace)
                    && (identity.verify().is_none() || identity.baseline().as_ref() != Some(baseline)))
                || !matches!(self.foreground(), Foreground::Own | Foreground::External(_))
                || matches!(self.foreground(), Foreground::External(pid) if pid != identity.pid)
                || !eligible()
            {
                return WriteOutcome::NotWritten;
            }
            let proof = (self.foreground() == Foreground::Own).then(|| FocusRecovery::before_activation(identity.pid));
            let fail = |outcome| {
                if let Some(proof) = proof.as_ref().filter(|proof| proof.is_current()) { recover(proof.clone()); }
                outcome
            };
            if !identity.restore_front()
                || !eligible()
                || (matches!(operation, WriteOperation::Insert) && !identity.restore_caret(baseline))
                || identity.verify().is_none()
                || !settable(&identity.element)
                || identity.baseline().as_ref() != Some(baseline)
            {
                return fail(WriteOutcome::NotWritten);
            }
            let name = CFString::new("AXSelectedText");
            let value = CFString::new(text);
            if identity.verify().is_none()
                || self.foreground() != Foreground::External(identity.pid)
                || selected_range(&identity.element).as_ref() != Some(&baseline.selection)
                || secure_input()
                || !eligible()
            {
                return fail(WriteOutcome::NotWritten);
            }
            if let Err(outcome) = claim() {
                return fail(outcome);
            }
            let status = unsafe {
                AXUIElementSetAttributeValue(
                    raw_ax(&identity.element),
                    name.as_CFTypeRef(),
                    value.as_CFTypeRef(),
                )
            };
            if status == 0 && document(&identity.element).as_ref() == Some(&expected) {
                WriteOutcome::Written
            } else {
                fail(WriteOutcome::Unknown)
            }
        })
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.center, removeObserver: &*self.token];
        }
    }
}

impl Identity {
    pub(super) fn pid(&self) -> u32 {
        self.pid as u32
    }

    /// Restore only the captured A control and collapsed UTF-16 range. A changed
    /// document is not rebased; B replacement never calls this method.
    fn restore_caret(&self, baseline: &Baseline) -> bool {
        if self.verify_control().is_none()
            || !baseline.selection.is_empty()
            || document(&self.element).as_ref() != Some(&baseline.document)
        {
            return false;
        }
        if self.verify().is_none() {
            let raise = CFString::new("AXRaise");
            let focused = CFString::new("AXFocused");
            unsafe {
                if AXUIElementPerformAction(raw_ax(&self.window), raise.as_concrete_TypeRef()) != 0
                    || AXUIElementSetAttributeValue(
                        raw_ax(&self.element), focused.as_CFTypeRef(), CFBoolean::true_value().as_CFTypeRef(),
                    ) != 0
                {
                    return false;
                }
            }
        }
        if self.verify().is_none() || document(&self.element).as_ref() != Some(&baseline.document) {
            return false;
        }
        if selected_range(&self.element).as_ref() != Some(&baseline.selection) {
            let range = CFRange { location: baseline.selection.start as isize, length: 0 };
            let name = CFString::new("AXSelectedTextRange");
            unsafe {
                let value = AXValueCreate(AX_RANGE, ptr::from_ref(&range).cast());
                if value.is_null() {
                    return false;
                }
                let value = CFType::wrap_under_create_rule(value);
                if AXUIElementSetAttributeValue(raw_ax(&self.element), name.as_CFTypeRef(), value.as_CFTypeRef()) != 0 {
                    return false;
                }
            }
        }
        self.verify().is_some() && self.baseline().as_ref() == Some(baseline)
    }

    fn verify_control(&self) -> Option<()> {
        let terminated: Bool = unsafe { msg_send![&*self.application, isTerminated] };
        if terminated.as_bool() || secure_input() || !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        let role = attr(&self.element, "AXRole")
            .ok()?
            .downcast_into::<CFString>()?;
        if !matches!(role.to_string().as_str(), "AXTextField" | "AXTextArea") {
            return None;
        }
        let subrole = match attr(&self.element, "AXSubrole") {
            Ok(value) => Some(value.downcast_into::<CFString>()?.to_string()),
            Err(AX_UNSUPPORTED | AX_NO_VALUE) => None,
            Err(_) => return None,
        };
        if evaluate_gate(&GateInputs {
            subrole,
            ..GateInputs::default()
        })
        .is_some()
            || element_attr(&self.element, "AXWindow", self.pid)
                .or_else(|| element_attr(&self.element, "AXTopLevelUIElement", self.pid))? != self.window
        {
            return None;
        }
        Some(())
    }

    fn verify(&self) -> Option<()> {
        self.verify_control()?;
        (element_attr(&self.ax_app, "AXFocusedWindow", self.pid)? == self.window
            && element_attr(&self.ax_app, "AXFocusedUIElement", self.pid)? == self.element)
            .then_some(())
    }

    fn selection(&self) -> Option<(ExternalSelection, Option<Baseline>)> {
        let range = selected_range(&self.element);
        let exposed = string_units(attr(&self.element, "AXSelectedText").ok(), MAX_SELECTION_UTF16);
        let exact = range.clone().and_then(|range| {
            let units = text_for_range(&self.element, range.clone(), MAX_SELECTION_UTF16)
                .or_else(|| exposed.clone().filter(|units| units.len() == range.len()))?;
            if exposed.as_ref().is_some_and(|text| text != &units)
                || selected_range(&self.element)? != range
            {
                return None;
            }
            Some((range, units))
        });
        if let Some((range, units)) = exact {
            let selection = selection_from_units(&units)?;
            let baseline = settable(&self.element)
                .then(|| self.baseline()).flatten()
                .filter(|baseline| baseline.selection == range && baseline.selected() == units);
            if selected_range(&self.element)? != range {
                return None;
            }
            return Some((selection, baseline));
        }
        if range.is_some() {
            return None;
        }
        let exposed = exposed.filter(|units| !units.is_empty())?;
        (string_units(attr(&self.element, "AXSelectedText").ok(), MAX_SELECTION_UTF16)? == exposed)
            .then(|| selection_from_units(&exposed).map(|selection| (selection, None)))
            .flatten()
    }

    fn restore_front(&self) -> bool {
        let is_front = || frontmost().is_some_and(|app| pid(&app) == self.pid);
        if !is_front() {
            let activated: Bool =
                unsafe { msg_send![&*self.application, activateWithOptions: 2usize] };
            if !activated.as_bool() {
                return false;
            }
            for _ in 0..12 {
                if is_front() {
                    return true;
                }
                thread::sleep(Duration::from_millis(15));
            }
        }
        is_front()
    }

    fn baseline(&self) -> Option<Baseline> {
        let range = selected_range(&self.element)?;
        let document = document(&self.element)?;
        if selected_range(&self.element)? != range {
            return None;
        }
        Baseline::new(document, range)
    }
}

fn attr(element: &CFType, name: &str) -> Result<CFType, i32> {
    let name = CFString::new(name);
    let mut value = ptr::null();
    let status =
        unsafe { AXUIElementCopyAttributeValue(raw_ax(element), name.as_CFTypeRef(), &mut value) };
    if status != 0 {
        Err(status)
    } else if value.is_null() {
        Err(AX_NO_VALUE)
    } else {
        Ok(unsafe { CFType::wrap_under_create_rule(value) })
    }
}

unsafe fn ax(raw: AxUiElementRef, pid: i32) -> Option<CFType> {
    if raw.is_null() {
        return None;
    }
    validate_ax(CFType::wrap_under_create_rule(raw.cast()), pid)
}

fn raw_ax(value: &CFType) -> AxUiElementRef {
    value.as_CFTypeRef().cast_mut().cast()
}

fn validate_ax(value: CFType, expected_pid: i32) -> Option<CFType> {
    let mut pid = 0;
    let raw = raw_ax(&value);
    unsafe {
        (value.type_of() == AXUIElementGetTypeID()
            && AXUIElementGetPid(raw, &mut pid) == 0
            && pid == expected_pid
            && AXUIElementSetMessagingTimeout(raw, AX_MESSAGE_SECONDS) == 0)
            .then_some(value)
    }
}

fn element_attr(element: &CFType, name: &str, pid: i32) -> Option<CFType> {
    validate_ax(attr(element, name).ok()?, pid)
}

fn get_range(value: &CFType) -> Option<Range<usize>> {
    let mut raw = CFRange {
        location: 0,
        length: 0,
    };
    unsafe {
        if value.type_of() != AXValueGetTypeID()
            || AXValueGetValue(
                value.as_CFTypeRef(),
                AX_RANGE,
                ptr::from_mut(&mut raw).cast(),
            ) == 0
        {
            return None;
        }
    }
    let start = usize::try_from(raw.location).ok()?;
    let len = usize::try_from(raw.length).ok()?;
    Some(start..start.checked_add(len)?)
}

fn selected_range(element: &CFType) -> Option<Range<usize>> {
    let range = get_range(&attr(element, "AXSelectedTextRange").ok()?)?;
    match attr(element, "AXSelectedTextRanges") {
        Ok(value) => {
            let ranges = value.downcast_into::<CFArray>()?;
            let first = *ranges.get(0)?;
            if ranges.len() != 1 || first.is_null() {
                return None;
            }
            let first = unsafe { CFType::wrap_under_get_rule(first) };
            if get_range(&first)? != range {
                return None;
            }
        }
        Err(AX_UNSUPPORTED | AX_NO_VALUE) => {}
        Err(_) => return None,
    }
    Some(range)
}

fn settable(element: &CFType) -> bool {
    let name = CFString::new("AXSelectedText");
    let mut settable = 0;
    unsafe {
        AXUIElementIsAttributeSettable(raw_ax(element), name.as_concrete_TypeRef(), &mut settable)
            == 0
            && settable != 0
    }
}

fn document(element: &CFType) -> Option<Vec<u16>> {
    let count = attr(element, "AXNumberOfCharacters")
        .ok().and_then(|value| value.downcast_into::<CFNumber>())
        .and_then(|value| value.to_i64())
        .and_then(|count| usize::try_from(count).ok());
    count.and_then(|count| text_for_range(element, 0..count, MAX_DOCUMENT_UTF16))
        .or_else(|| {
            string_units(attr(element, "AXValue").ok(), MAX_DOCUMENT_UTF16)
                .filter(|units| count.is_none_or(|count| count == units.len()))
        })
}

fn string_units(value: Option<CFType>, cap: usize) -> Option<Vec<u16>> {
    let value = value?.downcast_into::<CFString>()?;
    let string = value.as_concrete_TypeRef();
    let len = unsafe { CFStringGetLength(string) };
    let length = usize::try_from(len).ok()?;
    if length > cap {
        return None;
    }
    let mut units = vec![0; length];
    unsafe {
        if length > 0 {
            CFStringGetCharacters(string, CFRange { location: 0, length: len }, units.as_mut_ptr());
        }
    }
    Some(units)
}

fn text_for_range(element: &CFType, range: Range<usize>, cap: usize) -> Option<Vec<u16>> {
    if range.len() > cap {
        return None;
    }
    if range.is_empty() {
        return Some(Vec::new());
    }
    let raw = CFRange {
        location: isize::try_from(range.start).ok()?,
        length: isize::try_from(range.len()).ok()?,
    };
    let name = CFString::new("AXStringForRange");
    unsafe {
        let parameter = AXValueCreate(AX_RANGE, ptr::from_ref(&raw).cast());
        if parameter.is_null() {
            return None;
        }
        let parameter = CFType::wrap_under_create_rule(parameter);
        let mut value = ptr::null();
        let status = AXUIElementCopyParameterizedAttributeValue(
            raw_ax(element),
            name.as_CFTypeRef(),
            parameter.as_CFTypeRef(),
            &mut value,
        );
        if status != 0 || value.is_null() {
            return None;
        }
        let string = CFType::wrap_under_create_rule(value).downcast_into::<CFString>()?;
        let string = string.as_concrete_TypeRef();
        if CFStringGetLength(string) != raw.length {
            return None;
        }
        let mut units = vec![0; range.len()];
        CFStringGetCharacters(
            string,
            CFRange {
                location: 0,
                length: raw.length,
            },
            units.as_mut_ptr(),
        );
        Some(units)
    }
}
