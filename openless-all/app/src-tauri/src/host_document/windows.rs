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
