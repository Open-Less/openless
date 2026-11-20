use super::*;
use crate::host_document::{is_blocked_process_name, uia_runtime_id};
use std::{cell::RefCell, mem, thread};
use windows::core::{implement, Interface, Result as WinResult, PWSTR};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM},
    System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    },
    UI::{Accessibility::*, Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*},
};

const MESSAGE_MS: u32 = 80;
const EM_GETSEL: u32 = 0x00b0;
const EM_REPLACESEL: u32 = 0x00c2;
const EM_GETPASSWORDCHAR: u32 = 0x00d2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Window {
    handle: HWND,
    pid: u32,
    thread: u32,
}

#[derive(Clone)]
pub(super) struct Identity {
    window: Window,
    process: Arc<Process>,
    root: IUIAutomationElement,
    element: IUIAutomationElement,
    runtime_id: Vec<i32>,
    edit: Option<Window>,
}

pub(super) struct Platform {
    uia: IUIAutomation,
    focus_handler: IUIAutomationFocusChangedEventHandler,
    foreground_hook: HWINEVENTHOOK,
    _apartment: Apartment,
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> Option<Self> {
        unsafe {
            let process = Self(
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
                .ok()?,
            );
            let mut path = [0u16; 1024];
            let mut count = path.len() as u32;
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut count,
            )
            .ok()?;
            let path = String::from_utf16(&path[..count as usize]).ok()?;
            let image = path.rsplit(['\\', '/']).next()?;
            (!image.is_empty() && !is_blocked_process_name(image)).then_some(process)
        }
    }

    fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

thread_local! {
    static FOREGROUND_EPOCH: RefCell<Option<Arc<AtomicU64>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn foreground_changed(
    _: HWINEVENTHOOK,
    _: u32,
    window: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if Window::new(GetAncestor(window, GA_ROOT)).is_some_and(|root| root.pid == std::process::id())
    {
        return;
    }
    FOREGROUND_EPOCH.with(|slot| {
        if let Some(epoch) = slot.borrow().as_ref() {
            epoch.fetch_add(1, Ordering::AcqRel);
        }
    });
}

#[implement(IUIAutomationFocusChangedEventHandler)]
struct FocusChanged(Arc<AtomicU64>);

impl IUIAutomationFocusChangedEventHandler_Impl for FocusChanged_Impl {
    fn HandleFocusChangedEvent(&self, element: Option<&IUIAutomationElement>) -> WinResult<()> {
        if Window::new(unsafe { GetForegroundWindow() })
            .is_some_and(|window| window.pid == std::process::id())
        {
            return Ok(());
        }
        let pid = element.and_then(|element| unsafe { element.CachedProcessId().ok() });
        if pid != Some(std::process::id() as i32) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
}

impl Platform {
    pub(super) fn new(epoch: Arc<AtomicU64>) -> Option<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
            let apartment = Apartment;
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()?;
            let timeouts: IUIAutomation2 = uia.cast().ok()?;
            timeouts.SetConnectionTimeout(MESSAGE_MS).ok()?;
            timeouts.SetTransactionTimeout(MESSAGE_MS).ok()?;
            let cache = uia.CreateCacheRequest().ok()?;
            cache.AddProperty(UIA_ProcessIdPropertyId).ok()?;
            let focus_handler: IUIAutomationFocusChangedEventHandler =
                FocusChanged(epoch.clone()).into();
            uia.AddFocusChangedEventHandler(&cache, &focus_handler)
                .ok()?;
            let foreground_hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if foreground_hook.is_invalid() {
                let _ = uia.RemoveFocusChangedEventHandler(&focus_handler);
                return None;
            }
            FOREGROUND_EPOCH.with(|slot| *slot.borrow_mut() = Some(epoch));
            Some(Self {
                uia,
                focus_handler,
                foreground_hook,
                _apartment: apartment,
            })
        }
    }

    pub(super) fn poll(&mut self) {
        let mut message = MSG::default();
        unsafe {
            for _ in 0..32 {
                if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    pub(super) fn foreground(&self) -> Foreground<Window> {
        match Window::new(unsafe { GetForegroundWindow() }) {
            Some(window) if window.pid == std::process::id() => Foreground::Own,
            Some(window) => Foreground::External(window),
            None => Foreground::Unknown,
        }
    }

    pub(super) fn bind(&mut self, window: Window) -> Option<Identity> {
        unsafe {
            let process = Arc::new(Process::open(window.pid)?);
            let root = self.uia.ElementFromHandle(window.handle).ok()?;
            let element = self.uia.GetFocusedElement().ok()?;
            let handle = element.CurrentNativeWindowHandle().ok()?;
            let runtime_id = uia_runtime_id(&element).ok()?;
            let edit = Window::new(handle).filter(|candidate| {
                candidate.pid == window.pid
                    && GetAncestor(handle, GA_ROOT) == window.handle
                    && window.focused_child() == Some(handle)
                    && standard_edit(handle)
            });
            let identity = Identity {
                window,
                process,
                root,
                element,
                runtime_id,
                edit,
            };
            self.verify(&identity)?;
            self.focus_matches(&identity).then_some(identity)
        }
    }

    pub(super) fn capture(
        &self,
        identity: &Identity,
    ) -> Option<(ExternalSelection, Option<Baseline>)> {
        unsafe {
            self.verify(identity)?;
            let pattern = identity
                .element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok()?;
            let first = one_range(&pattern)?;
            let units = range_text(&first)?;
            let selection = selection_from_units(&units)?;
            let baseline = identity
                .edit
                .and_then(|edit| identity.write_baseline(edit, &pattern, &first, &units));
            let last = one_range(&pattern)?;
            if !same_range(&first, &last)? || range_text(&last)? != units {
                return None;
            }
            self.verify_capture(identity)?;
            Some((selection, baseline))
        }
    }

    pub(super) fn write(
        &self,
        identity: &Identity,
        baseline: &Baseline,
        text: &str,
        claim: impl FnOnce() -> Result<(), WriteOutcome>,
    ) -> WriteOutcome {
        let Some(edit) = identity.edit else {
            return WriteOutcome::NotWritten;
        };
        let Some(state) = edit
            .safe_edit_style()
            .filter(|style| style & ES_READONLY == 0)
        else {
            return WriteOutcome::NotWritten;
        };
        if text.contains('\0') || (state & ES_MULTILINE == 0 && text.contains(['\r', '\n'])) {
            return WriteOutcome::NotWritten;
        }
        let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let mut units: Vec<_> = text.encode_utf16().collect();
        let Some(expected) = baseline.expected(&units) else {
            return WriteOutcome::NotWritten;
        };
        units.push(0);

        if self.verify(identity).is_none()
            || edit.baseline().as_ref() != Some(baseline)
            || !identity.window.restore_front()
            || !self.focus_matches(identity)
            || edit.safe_edit_style() != Some(state)
            || edit.baseline().as_ref() != Some(baseline)
            || !self.focus_matches(identity)
            || edit.selection().as_ref() != Some(&baseline.selection)
        {
            return WriteOutcome::NotWritten;
        }
        if let Err(outcome) = claim() {
            return outcome;
        }
        if edit
            .send(EM_REPLACESEL, 1, units.as_ptr() as isize)
            .is_some()
            && edit.document().as_ref() == Some(&expected)
        {
            WriteOutcome::Written
        } else {
            WriteOutcome::Unknown
        }
    }

    fn verify(&self, target: &Identity) -> Option<()> {
        unsafe {
            let kind = target.element.CurrentControlType().ok()?;
            if !target.process.alive()
                || Window::new(target.window.handle)? != target.window
                || target.element.CurrentProcessId().ok()? as u32 != target.window.pid
                || !self.safe_from_password(target)?
                || uia_runtime_id(&target.element).ok()? != target.runtime_id
                || (kind != UIA_EditControlTypeId && kind != UIA_DocumentControlTypeId)
            {
                return None;
            }
            let current_root = self.uia.ElementFromHandle(target.window.handle).ok()?;
            if !self
                .uia
                .CompareElements(&current_root, &target.root)
                .ok()?
                .as_bool()
            {
                return None;
            }
            let walker = self.uia.RawViewWalker().ok()?;
            let mut node = target.element.clone();
            for _ in 0..24 {
                let handle = node.CurrentNativeWindowHandle().ok()?;
                if handle == target.window.handle {
                    return self
                        .uia
                        .CompareElements(&node, &target.root)
                        .ok()?
                        .as_bool()
                        .then_some(());
                }
                if !handle.0.is_null() && GetAncestor(handle, GA_ROOT) != target.window.handle {
                    return None;
                }
                node = walker.GetParentElement(&node).ok()?;
            }
            None
        }
    }

    unsafe fn safe_from_password(&self, target: &Identity) -> Option<bool> {
        if target.element.CurrentIsPassword().ok()?.as_bool() {
            return Some(false);
        }
        if let Some(edit) = target.edit {
            return Some(edit.safe_edit_style().is_some());
        }
        let property = target
            .element
            .GetCurrentPropertyValueEx(UIA_IsPasswordPropertyId, true)
            .ok()?;
        if self.uia.CheckNotSupported(&property).ok()?.as_bool() {
            return None;
        }
        Some(!bool::try_from(&property).ok()?)
    }

    fn verify_capture(&self, target: &Identity) -> Option<()> {
        self.verify(target)?;
        match self.foreground() {
            Foreground::Own => Some(()),
            Foreground::External(window)
                if window == target.window && self.focus_matches(target) =>
            {
                Some(())
            }
            _ => None,
        }
    }

    fn focus_matches(&self, target: &Identity) -> bool {
        let matches = || unsafe {
            let current = self.uia.GetFocusedElement().ok()?;
            let same = self
                .uia
                .CompareElements(&current, &target.element)
                .ok()?
                .as_bool()
                && uia_runtime_id(&current).ok()? == target.runtime_id;
            Some(same && self.safe_from_password(target)?)
        };
        self.foreground() == Foreground::External(target.window)
            && matches() == Some(true)
            && self.foreground() == Foreground::External(target.window)
            && target
                .edit
                .is_none_or(|edit| target.window.focused_child() == Some(edit.handle))
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWinEvent(self.foreground_hook);
            let _ = self.uia.RemoveFocusChangedEventHandler(&self.focus_handler);
        }
        FOREGROUND_EPOCH.with(|slot| *slot.borrow_mut() = None);
    }
}

impl Identity {
    unsafe fn write_baseline(
        &self,
        edit: Window,
        pattern: &IUIAutomationTextPattern,
        range: &IUIAutomationTextRange,
        selected: &[u16],
    ) -> Option<Baseline> {
        if self.window.focused_child()? != edit.handle || edit.safe_edit_style()? & ES_READONLY != 0
        {
            return None;
        }
        let baseline = edit.baseline()?;
        if baseline.selected() != selected
            || uia_prefix(pattern, range, baseline.selection.start)?.as_wide()
                != &baseline.document[..baseline.selection.start]
        {
            return None;
        }
        Some(baseline)
    }
}

impl Window {
    fn new(handle: HWND) -> Option<Self> {
        if handle.0.is_null() {
            return None;
        }
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(handle, Some(&mut pid)) };
        (thread != 0 && pid != 0).then_some(Self {
            handle,
            pid,
            thread,
        })
    }

    fn focused_child(self) -> Option<HWND> {
        let mut state = GUITHREADINFO {
            cbSize: mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(self.thread, &mut state).ok()? };
        (state.hwndActive == self.handle && !state.hwndFocus.0.is_null()).then_some(state.hwndFocus)
    }

    fn restore_front(self) -> bool {
        let is_front = || Self::new(unsafe { GetForegroundWindow() }) == Some(self);
        if !is_front() {
            if Self::new(self.handle) != Some(self)
                || !unsafe { SetForegroundWindow(self.handle).as_bool() }
            {
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

    fn safe_edit_style(self) -> Option<i32> {
        if Self::new(self.handle)? != self || !standard_edit(self.handle) {
            return None;
        }
        let style = unsafe { GetWindowLongW(self.handle, GWL_STYLE) };
        if style & ES_PASSWORD != 0 || self.send(EM_GETPASSWORDCHAR, 0, 0)? != 0 {
            return None;
        }
        Some(if unsafe { IsWindowEnabled(self.handle).as_bool() } {
            style
        } else {
            style | ES_READONLY
        })
    }

    fn baseline(self) -> Option<Baseline> {
        let range = self.selection()?;
        let document = self.document()?;
        if self.selection()? != range {
            return None;
        }
        Baseline::new(document, range)
    }

    fn document(self) -> Option<Vec<u16>> {
        let count = self.send(WM_GETTEXTLENGTH, 0, 0)?;
        if count > MAX_DOCUMENT_UTF16 {
            return None;
        }
        let mut units = vec![0u16; count + 2];
        let copied = self.send(WM_GETTEXT, units.len(), units.as_mut_ptr() as isize)?;
        if copied != count || self.send(WM_GETTEXTLENGTH, 0, 0)? != count {
            return None;
        }
        units.truncate(count);
        Some(units)
    }

    fn selection(self) -> Option<Range<usize>> {
        let packed = self.send(EM_GETSEL, 0, 0)?;
        if packed >= u32::MAX as usize {
            return None;
        }
        let start = packed & 0xffff;
        let end = packed >> 16;
        (start <= end).then_some(start..end)
    }

    fn send(self, message: u32, wparam: usize, lparam: isize) -> Option<usize> {
        let mut result = 0;
        unsafe {
            let sent = SendMessageTimeoutW(
                self.handle,
                message,
                WPARAM(wparam),
                LPARAM(lparam),
                SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
                MESSAGE_MS,
                Some(&mut result),
            );
            (sent.0 != 0).then_some(result)
        }
    }
}

fn standard_edit(handle: HWND) -> bool {
    let mut class = [0u16; 16];
    let mut real_class = [0u16; 16];
    unsafe {
        let len = GetClassNameW(handle, &mut class);
        let real_len = RealGetWindowClassW(handle, &mut real_class);
        IsWindowUnicode(handle).as_bool()
            && len == 4
            && real_len == 4
            && class[..4].eq(&[b'E' as u16, b'd' as u16, b'i' as u16, b't' as u16])
            && class[..4] == real_class[..4]
    }
}

unsafe fn one_range(pattern: &IUIAutomationTextPattern) -> Option<IUIAutomationTextRange> {
    let ranges = pattern.GetSelection().ok()?;
    (ranges.Length().ok()? == 1).then(|| ranges.GetElement(0).ok())?
}

unsafe fn uia_prefix(
    pattern: &IUIAutomationTextPattern,
    range: &IUIAutomationTextRange,
    length: usize,
) -> Option<windows::core::BSTR> {
    let prefix = pattern.DocumentRange().ok()?;
    prefix
        .MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            range,
            TextPatternRangeEndpoint_Start,
        )
        .ok()?;
    prefix.GetText((length + 1) as i32).ok()
}

unsafe fn same_range(
    first: &IUIAutomationTextRange,
    last: &IUIAutomationTextRange,
) -> Option<bool> {
    Some(
        first
            .CompareEndpoints(
                TextPatternRangeEndpoint_Start,
                last,
                TextPatternRangeEndpoint_Start,
            )
            .ok()?
            == 0
            && first
                .CompareEndpoints(
                    TextPatternRangeEndpoint_End,
                    last,
                    TextPatternRangeEndpoint_End,
                )
                .ok()?
                == 0,
    )
}

unsafe fn range_text(range: &IUIAutomationTextRange) -> Option<Vec<u16>> {
    match range
        .CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            range,
            TextPatternRangeEndpoint_End,
        )
        .ok()?
    {
        0 => Some(Vec::new()),
        order if order < 0 => {
            let text = range.GetText((MAX_SELECTION_UTF16 + 1) as i32).ok()?;
            (!text.is_empty() && text.len() <= MAX_SELECTION_UTF16).then(|| text.as_wide().to_vec())
        }
        _ => None,
    }
}
