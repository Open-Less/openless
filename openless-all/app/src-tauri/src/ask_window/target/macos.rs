use super::*;
use crate::host_document::{evaluate_gate, macos::*, GateInputs};
use block2::RcBlock;
use core_foundation::{
    array::CFArray,
    base::{CFRange, CFType, TCFType},
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
    fn AXUIElementIsAttributeSettable(
        element: AxUiElementRef,
        name: CFStringRef,
        out: *mut u8,
    ) -> i32;
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
            let ax_app = self.watch.as_ref()?.app.clone();
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
            let range = selected_range(&identity.element)?;
            let units = text_for_range(&identity.element, range.clone(), MAX_SELECTION_UTF16)?;
            let selection = selection_from_units(&units)?;
            let baseline = settable(&identity.element)
                .then(|| identity.baseline())
                .flatten()
                .filter(|baseline| baseline.selection == range && baseline.selected() == units);
            if selected_range(&identity.element)? != range {
                return None;
            }
            identity.verify()?;
            Some((selection, baseline))
        })
    }

    pub(super) fn write(
        &self,
        identity: &Identity,
        baseline: &Baseline,
        text: &str,
        claim: impl FnOnce() -> Result<(), WriteOutcome>,
    ) -> WriteOutcome {
        autoreleasepool(|_| {
            let text_units: Vec<_> = text.encode_utf16().collect();
            let Some(expected) = baseline.expected(&text_units) else {
                return WriteOutcome::NotWritten;
            };
            if identity.verify().is_none()
                || !settable(&identity.element)
                || identity.baseline().as_ref() != Some(baseline)
                || !identity.restore_front()
                || identity.verify().is_none()
                || identity.baseline().as_ref() != Some(baseline)
            {
                return WriteOutcome::NotWritten;
            }
            let name = CFString::new("AXSelectedText");
            let value = CFString::new(text);
            if identity.verify().is_none()
                || self.foreground() != Foreground::External(identity.pid)
                || selected_range(&identity.element).as_ref() != Some(&baseline.selection)
                || secure_input()
            {
                return WriteOutcome::NotWritten;
            }
            if let Err(outcome) = claim() {
                return outcome;
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
                WriteOutcome::Unknown
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
    fn verify(&self) -> Option<()> {
        let terminated: Bool = unsafe { msg_send![&*self.application, isTerminated] };
        if terminated.as_bool() || secure_input() {
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
            || element_attr(&self.ax_app, "AXFocusedWindow", self.pid)? != self.window
            || element_attr(&self.ax_app, "AXFocusedUIElement", self.pid)? != self.element
            || element_attr(&self.element, "AXWindow", self.pid)? != self.window
        {
            return None;
        }
        Some(())
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
        .ok()?
        .downcast_into::<CFNumber>()?
        .to_i64()?;
    text_for_range(element, 0..usize::try_from(count).ok()?, MAX_DOCUMENT_UTF16)
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
