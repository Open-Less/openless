//! Short-lived UIA observation. All registration and COM objects belong to one
//! MTA worker; callbacks only mark text dirty and never read document contents.
use super::EditPair;
use openless_core::host_document::ObservedInsertion;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, OnceLock,
};
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, Result, PWSTR, VARIANT};
use windows::Win32::{
    Foundation::{CloseHandle, HWND},
    System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::{Accessibility::*, WindowsAndMessaging::GetForegroundWindow},
};

type Callback = Box<dyn Fn(EditPair) -> bool + Send + Sync>;

#[derive(Debug, Clone)]
pub(super) struct VoiceEditTarget {
    runtime_id: Vec<i32>,
    process_id: i32,
    text: String,
    selection_utf16: Option<(usize, usize)>,
}

fn voice_edit_target_matches(
    target: &VoiceEditTarget,
    current: &VoiceEditTarget,
) -> std::result::Result<(), String> {
    if current.runtime_id != target.runtime_id || current.process_id != target.process_id {
        return Err("voiceEditTargetChanged".into());
    }
    if current.text != target.text || current.selection_utf16 != target.selection_utf16 {
        return Err("voiceEditFieldChanged".into());
    }
    Ok(())
}

fn with_voice_edit_uia<T>(
    read: impl FnOnce(&IUIAutomation) -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = ComGuard;
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        let timeouts: IUIAutomation2 = uia
            .cast()
            .map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        timeouts
            .SetConnectionTimeout(200)
            .and_then(|_| timeouts.SetTransactionTimeout(200))
            .map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        read(&uia)
    }
}

unsafe fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>> {
    use windows::Win32::System::Ole::{
        SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
    };
    let array = element.GetRuntimeId()?;
    if array.is_null() {
        return Err(windows::core::Error::from_win32());
    }
    let result = (|| {
        let first = SafeArrayGetLBound(array, 1)?;
        let last = SafeArrayGetUBound(array, 1)?;
        if last < first || last.saturating_sub(first) >= 128 {
            return Err(windows::core::Error::from_win32());
        }
        (first..=last)
            .map(|index| {
                let mut value = 0i32;
                SafeArrayGetElement(array, &index, &mut value as *mut _ as *mut _)?;
                Ok(value)
            })
            .collect()
    })();
    let _ = SafeArrayDestroy(array);
    result
}

unsafe fn voice_edit_selection(
    element: &IUIAutomationElement,
    text: &str,
) -> Result<Option<(usize, usize)>> {
    let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    else {
        // ValuePattern-only controls support complete-field SetValue, not a selected-range replacement.
        return Ok(None);
    };
    let selection = pattern.GetSelection()?;
    match selection.Length()? {
        0 => Ok(None),
        1 => {
            let selected = selection.GetElement(0)?;
            let prefix = pattern.DocumentRange()?.Clone()?;
            prefix.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                &selected,
                TextPatternRangeEndpoint_Start,
            )?;
            let prefix = prefix
                .GetText((ObservedInsertion::MAX_DOCUMENT_UTF16 + 1) as i32)?
                .to_string();
            let selected = selected
                .GetText((ObservedInsertion::MAX_DOCUMENT_UTF16 + 1) as i32)?
                .to_string();
            let start = prefix.encode_utf16().count();
            let end = start.saturating_add(selected.encode_utf16().count());
            if end > text.encode_utf16().count()
                || !text.starts_with(&prefix)
                || !text[prefix.len()..].starts_with(&selected)
            {
                return Err(windows::core::Error::from_win32());
            }
            Ok(Some((start, end)))
        }
        _ => Err(windows::core::Error::from_win32()),
    }
}

unsafe fn voice_edit_snapshot(element: &IUIAutomationElement) -> Result<VoiceEditTarget> {
    if element.CurrentIsPassword()?.as_bool() || !allowed_process(element)? {
        return Err(windows::core::Error::from_win32());
    }
    let text = read_text(element)?;
    Ok(VoiceEditTarget {
        runtime_id: runtime_id(element)?,
        process_id: element.CurrentProcessId()?,
        selection_utf16: voice_edit_selection(element, &text)?,
        text,
    })
}

pub(super) fn capture_voice_edit_target() -> std::result::Result<
    (
        String,
        Option<openless_core::TextSelection>,
        VoiceEditTarget,
    ),
    String,
> {
    with_voice_edit_uia(|uia| unsafe {
        let element = uia
            .GetFocusedElement()
            .map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        let target =
            voice_edit_snapshot(&element).map_err(|_| "voiceEditTargetUnavailable".to_string())?;
        if !uia
            .CompareElements(
                &element,
                &uia.GetFocusedElement()
                    .map_err(|_| "voiceEditTargetChanged".to_string())?,
            )
            .map_err(|_| "voiceEditTargetChanged".to_string())?
            .as_bool()
        {
            return Err("voiceEditTargetChanged".into());
        }
        let selection = target
            .selection_utf16
            .filter(|(start, end)| start != end)
            .map(|(start, end)| openless_core::TextSelection {
                start: super::utf16_offset_to_char_offset(&target.text, start) as u32,
                end: super::utf16_offset_to_char_offset(&target.text, end) as u32,
            });
        Ok((target.text.clone(), selection, target))
    })
}

pub(super) fn apply_voice_edit_target(
    target: &VoiceEditTarget,
    text: &str,
    insert: impl FnOnce(&str) -> std::result::Result<(), String>,
) -> std::result::Result<(), String> {
    with_voice_edit_uia(|uia| unsafe {
        let unavailable = |_| "voiceEditTargetUnavailable".to_string();
        let element = uia.GetFocusedElement().map_err(unavailable)?;
        let current = voice_edit_snapshot(&element).map_err(unavailable)?;
        voice_edit_target_matches(target, &current)?;
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        {
            let range = if target
                .selection_utf16
                .is_some_and(|(start, end)| start != end)
            {
                pattern
                    .GetSelection()
                    .and_then(|ranges| ranges.GetElement(0))
                    .map_err(unavailable)?
            } else {
                pattern.DocumentRange().map_err(unavailable)?
            };
            let original_selection = pattern
                .GetSelection()
                .and_then(|ranges| ranges.GetElement(0))
                .and_then(|range| range.Clone())
                .map_err(unavailable)?;
            let expected = target
                .selection_utf16
                .filter(|(start, end)| start != end)
                .unwrap_or((0, target.text.encode_utf16().count()));
            super::write_with_selection_recovery(
                || {
                    range.Select().map_err(unavailable)?;
                    if !uia
                        .CompareElements(&element, &uia.GetFocusedElement().map_err(unavailable)?)
                        .map_err(unavailable)?
                        .as_bool()
                    {
                        return Err("voiceEditTargetChanged".into());
                    }
                    if read_text(&element).map_err(unavailable)? != target.text {
                        return Err("voiceEditFieldChanged".into());
                    }
                    let selected =
                        voice_edit_selection(&element, &target.text).map_err(unavailable)?;
                    if selected != Some(expected) && !(target.text.is_empty() && selected.is_none())
                    {
                        return Err("voiceEditTargetUnavailable".into());
                    }
                    insert(text)
                },
                || {
                    let current = uia
                        .GetFocusedElement()
                        .and_then(|focused| voice_edit_snapshot(&focused));
                    let mut selected_target = target.clone();
                    selected_target.selection_utf16 = Some(expected);
                    if current.is_ok_and(|current| {
                        voice_edit_target_matches(&selected_target, &current).is_ok()
                    }) {
                        let _ = original_selection.Select();
                    }
                },
            )
        } else {
            let value = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .map_err(unavailable)?;
            if value.CurrentIsReadOnly().map_err(unavailable)?.as_bool()
                || !uia
                    .CompareElements(&element, &uia.GetFocusedElement().map_err(unavailable)?)
                    .map_err(unavailable)?
                    .as_bool()
            {
                return Err("voiceEditTargetUnavailable".into());
            }
            if read_text(&element).map_err(unavailable)? != target.text {
                return Err("voiceEditFieldChanged".into());
            }
            value
                .SetValue(&windows::core::BSTR::from(text))
                .map_err(unavailable)
        }
    })
}

#[cfg(test)]
mod voice_edit_tests {
    use super::*;

    #[test]
    fn voice_edit_requires_the_original_control_complete_text_and_caret() {
        let target = VoiceEditTarget {
            runtime_id: vec![1, 2],
            process_id: 1,
            text: "a".repeat(5000),
            selection_utf16: Some((2500, 2500)),
        };
        assert!(voice_edit_target_matches(&target, &target).is_ok());
        let mut changed = target.clone();
        changed.text.replace_range(2500..2501, "b");
        assert_eq!(
            voice_edit_target_matches(&target, &changed),
            Err("voiceEditFieldChanged".into())
        );
        changed = target.clone();
        changed.selection_utf16 = Some((2501, 2501));
        assert!(voice_edit_target_matches(&target, &changed).is_err());
        changed = target.clone();
        changed.runtime_id.push(3);
        assert_eq!(
            voice_edit_target_matches(&target, &changed),
            Err("voiceEditTargetChanged".into())
        );
        assert_eq!(super::super::utf16_offset_to_char_offset("a😀b", 3), 2);
    }
}

/// The paste command itself remains authoritative on failures. UIA may only
/// promote PasteSent after observing a change in the very same editor. Failure
/// to read the host never retries, suppresses, or changes the actual paste.
pub(crate) fn insert_with_delivery_check(
    text: &str,
    consent: impl Fn() -> bool,
    insert: impl FnOnce() -> crate::types::InsertStatus,
) -> crate::types::InsertStatus {
    use crate::types::InsertStatus;
    unsafe {
        if !consent() || CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return insert();
        }
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = ComGuard;
        let snapshot = (|| -> Result<_> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            let timeouts: IUIAutomation2 = uia.cast()?;
            timeouts.SetConnectionTimeout(200)?;
            timeouts.SetTransactionTimeout(200)?;
            let window = GetForegroundWindow();
            let element = uia.GetFocusedElement()?;
            if !consent() || element.CurrentIsPassword()?.as_bool() || !allowed_process(&element)? {
                return Err(windows::core::Error::from_win32());
            }
            let before = read_text(&element)?;
            Ok((uia, element, window, before))
        })()
        .ok();
        let status = insert();
        if status != InsertStatus::PasteSent {
            return status;
        }
        let Some((uia, element, window, before)) = snapshot else {
            return status;
        };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) && consent() {
            let verified = (|| -> Result<bool> {
                if GetForegroundWindow() != window
                    || !uia
                        .CompareElements(&element, &uia.GetFocusedElement()?)?
                        .as_bool()
                {
                    return Err(windows::core::Error::from_win32());
                }
                if !consent() {
                    return Ok(false);
                }
                let after = read_text(&element)?;
                Ok(ObservedInsertion::delivered(&before, &after, text))
            })();
            match verified {
                Ok(true) => return InsertStatus::Inserted,
                Err(_) => break,
                Ok(false) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        status
    }
}
struct Request {
    text: String,
    window: isize,
    lifetime: Duration,
    stop: Arc<AtomicBool>,
    callback: Callback,
}

#[implement(IUIAutomationEventHandler, IUIAutomationPropertyChangedEventHandler)]
struct Changed {
    dirty: Arc<AtomicBool>,
}
impl IUIAutomationEventHandler_Impl for Changed_Impl {
    fn HandleAutomationEvent(
        &self,
        _: Option<&IUIAutomationElement>,
        _: UIA_EVENT_ID,
    ) -> Result<()> {
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}
impl IUIAutomationPropertyChangedEventHandler_Impl for Changed_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        _: Option<&IUIAutomationElement>,
        _: UIA_PROPERTY_ID,
        _: &VARIANT,
    ) -> Result<()> {
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}

pub(super) fn spawn_edit_watcher(
    text: String,
    lifetime: Duration,
    callback: Callback,
) -> Option<Arc<AtomicBool>> {
    if text.trim().is_empty() {
        return None;
    }
    static WORKER: OnceLock<Option<mpsc::Sender<Request>>> = OnceLock::new();
    let worker = WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Request>();
            std::thread::Builder::new()
                .name("vocab-uia".into())
                .spawn(move || unsafe {
                    if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                        return;
                    }
                    while let Ok(request) = rx.recv() {
                        if !request.stop.load(Ordering::Acquire) {
                            let _ = observe(&request);
                        }
                    }
                    CoUninitialize();
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()?;
    let stop = Arc::new(AtomicBool::new(false));
    worker
        .send(Request {
            text,
            lifetime,
            window: unsafe { GetForegroundWindow().0 as isize },
            stop: stop.clone(),
            callback,
        })
        .ok()?;
    Some(stop)
}

unsafe fn allowed_process(element: &IUIAutomationElement) -> Result<bool> {
    let pid = element.CurrentProcessId()? as u32;
    if pid == std::process::id() {
        return Ok(false);
    }
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        PWSTR(buffer.as_mut_ptr()),
        &mut len,
    );
    let _ = CloseHandle(process);
    result?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase();
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    Ok(![
        "keepass",
        "1password",
        "bitwarden",
        "lastpass",
        "dashlane",
        "windowsterminal",
        "powershell",
        "pwsh",
        "cmd.exe",
        "conhost",
        "mintty",
        "wezterm",
        "alacritty",
        "putty",
    ]
    .iter()
    .any(|blocked| name.contains(blocked)))
}

unsafe fn read_text(element: &IUIAutomationElement) -> Result<String> {
    if element.CurrentIsPassword()?.as_bool() {
        return Err(windows::core::Error::from_win32());
    }
    let text = if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    {
        pattern
            .DocumentRange()?
            .GetText((ObservedInsertion::MAX_DOCUMENT_UTF16 + 1) as i32)?
            .to_string()
    } else {
        element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)?
            .CurrentValue()?
            .to_string()
    };
    if text.encode_utf16().count() > ObservedInsertion::MAX_DOCUMENT_UTF16 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(text)
}

unsafe fn observe(request: &Request) -> Result<()> {
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
    let timeouts: IUIAutomation2 = uia.cast()?;
    timeouts.SetConnectionTimeout(200)?;
    timeouts.SetTransactionTimeout(200)?;
    let element = uia.GetFocusedElement()?;
    if element.CurrentIsPassword()?.as_bool() || !allowed_process(&element)? {
        return Ok(());
    }
    let active = || -> Result<bool> {
        Ok(!request.stop.load(Ordering::Acquire)
            && GetForegroundWindow() == HWND(request.window as *mut _)
            && !element.CurrentIsPassword()?.as_bool()
            && uia
                .CompareElements(&element, &uia.GetFocusedElement()?)?
                .as_bool())
    };
    let started = Instant::now();
    let mut anchor = loop {
        if !active()? || started.elapsed() >= Duration::from_secs(1) {
            return Ok(());
        }
        if let Some(anchor) = ObservedInsertion::new(read_text(&element)?, &request.text) {
            break anchor;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let dirty = Arc::new(AtomicBool::new(true));
    let handler: IUIAutomationEventHandler = Changed {
        dirty: dirty.clone(),
    }
    .into();
    let property: IUIAutomationPropertyChangedEventHandler = handler.cast()?;
    let text_registered = uia
        .AddAutomationEventHandler(
            UIA_Text_TextChangedEventId,
            &element,
            TreeScope_Element,
            None,
            &handler,
        )
        .is_ok();
    let value_registered = uia
        .AddPropertyChangedEventHandlerNativeArray(
            &element,
            TreeScope_Element,
            None,
            &property,
            &[UIA_ValueValuePropertyId],
        )
        .is_ok();
    if !text_registered && !value_registered {
        return Ok(());
    }
    // Always unregister, including provider errors and opt-out. Late callbacks
    // only retain their own dirty flag; no host text or Core sink is accessible.
    let result = (|| -> Result<()> {
        let mut changed_at = Some(Instant::now());
        while started.elapsed() < request.lifetime && active()? {
            if dirty.swap(false, Ordering::AcqRel) {
                changed_at = Some(Instant::now());
            }
            if changed_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(700)) {
                changed_at = None;
                let current = read_text(&element)?;
                if !active()?
                    || !anchor.observe(&current, |edit| {
                        !request.stop.load(Ordering::Acquire) && (request.callback)(edit)
                    })
                {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    if text_registered {
        let _ = uia.RemoveAutomationEventHandler(UIA_Text_TextChangedEventId, &element, &handler);
    }
    if value_registered {
        let _ = uia.RemovePropertyChangedEventHandler(&element, &property);
    }
    result
}
