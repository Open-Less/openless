//! Windows UI Automation edit observer. All COM work stays on one MTA thread.
//! No keyboard logging, clipboard access, OCR, or document upload.
use super::{
    edit_session::{EditSession, MAX_FIELD_CHARS},
    EditPair,
};
use ::windows::{
    core::{Interface, PWSTR},
    Win32::{
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
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
                IUIAutomationTextPattern, UIA_TextPatternId,
            },
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    },
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(250);
const SETTLE: Duration = Duration::from_millis(1200);
const LIFETIME: Duration = Duration::from_secs(60);

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

fn blocked_process(path: &str) -> bool {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    [
        "openless.exe",
        "1password.exe",
        "bitwarden.exe",
        "keepass.exe",
        "keepassxc.exe",
        "dashlane.exe",
        "lastpass.exe",
        "keeperpasswordmanager.exe",
        "enpass.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "windowsterminal.exe",
        "wt.exe",
        "conhost.exe",
        "openconsole.exe",
        "mintty.exe",
        "wezterm-gui.exe",
        "alacritty.exe",
        "code.exe",
        "cursor.exe",
        "windsurf.exe",
    ]
    .contains(&name.as_str())
}

unsafe fn safe_foreground(hwnd: HWND) -> Option<u32> {
    if hwnd.0.is_null() || GetForegroundWindow() != hwnd {
        return None;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut path = [0u16; 32768];
    let mut size = path.len() as u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(path.as_mut_ptr()),
        &mut size,
    );
    let _ = CloseHandle(handle);
    result.ok()?;
    if blocked_process(&String::from_utf16_lossy(&path[..size as usize])) {
        return None;
    }
    Some(pid)
}

unsafe fn read_field(element: &IUIAutomationElement, pid: u32) -> Option<String> {
    // Privacy checks precede every content read, including initial anchoring.
    if element.CurrentProcessId().ok()? as u32 != pid
        || element.CurrentIsPassword().ok()?.as_bool()
        || !element.CurrentHasKeyboardFocus().ok()?.as_bool()
        || !element.CurrentIsEnabled().ok()?.as_bool()
    {
        return None;
    }
    let class_name = element
        .CurrentClassName()
        .ok()?
        .to_string()
        .to_ascii_lowercase();
    if ["terminal", "termcontrol", "console", "xterm"]
        .iter()
        .any(|s| class_name.contains(s))
    {
        return None;
    }
    let pattern: IUIAutomationTextPattern = element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
    let text = pattern
        .DocumentRange()
        .ok()?
        .GetText((MAX_FIELD_CHARS + 1) as i32)
        .ok()?
        .to_string();
    // UIA's maxLength is UTF-16, not Rust char count. Reject truncated emoji
    // documents too, rather than treating a truncated prefix as the full field.
    if text.encode_utf16().count() > MAX_FIELD_CHARS {
        return None;
    }
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

pub(super) fn spawn_edit_watcher(
    typed: String,
    on_edit: Box<dyn Fn(EditPair) + Send + Sync>,
) -> Option<Arc<AtomicBool>> {
    // Capture the target now, not after a delayed background task starts.
    let hwnd_value = unsafe { GetForegroundWindow().0 as usize };
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("openless-edit-watch".into())
        .spawn(move || {
            let result =
                unsafe { observe(HWND(hwnd_value as *mut _), &typed, &worker_stop, on_edit) };
            if result.is_none() {
                log::debug!("[edit-learning] input unavailable or safety gate closed");
            }
        })
        .ok()?;
    Some(stop)
}

unsafe fn observe(
    hwnd: HWND,
    typed: &str,
    stop: &AtomicBool,
    on_edit: Box<dyn Fn(EditPair) + Send + Sync>,
) -> Option<()> {
    let started = Instant::now();
    let pid = safe_foreground(hwnd)?;
    CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
    let _apartment = ComApartment;
    let uia2: IUIAutomation2 =
        CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()?;
    uia2.SetConnectionTimeout(500).ok()?;
    uia2.SetTransactionTimeout(500).ok()?;
    uia2.SetAutoSetFocus(false).ok()?;
    let uia: IUIAutomation = uia2.cast().ok()?;
    let element = uia.GetFocusedElement().ok()?;
    let typed = typed.replace("\r\n", "\n").replace('\r', "\n");
    let mut snapshot = String::new();
    let mut session = None;
    // TSF/paste delivery may finish just after insertion returns.
    for _ in 0..8 {
        if stop.load(Ordering::Relaxed) || safe_foreground(hwnd)? != pid {
            return None;
        }
        if !uia
            .CompareElements(&element, &uia.GetFocusedElement().ok()?)
            .ok()?
            .as_bool()
        {
            return None;
        }
        snapshot = read_field(&element, pid)?;
        session = EditSession::anchor(&snapshot, &typed);
        if session.is_some() {
            break;
        }
        thread::sleep(POLL);
    }
    let mut session = session?;
    log::info!("[edit-learning] Windows observer armed (60s maximum)");
    let mut changed_at = Instant::now();
    let mut unsettled = false;
    while !stop.load(Ordering::Relaxed) && started.elapsed() < LIFETIME {
        thread::sleep(POLL);
        if stop.load(Ordering::Relaxed) || safe_foreground(hwnd)? != pid {
            return None;
        }
        if !uia
            .CompareElements(&element, &uia.GetFocusedElement().ok()?)
            .ok()?
            .as_bool()
        {
            return None;
        }
        let next = read_field(&element, pid)?;
        if next.is_empty() || session.region(&next).is_none() {
            return None;
        }
        if next != snapshot {
            snapshot = next;
            changed_at = Instant::now();
            unsettled = true;
        } else if unsettled && changed_at.elapsed() >= SETTLE {
            unsettled = false;
            if let Some(edit) = session.settled_edit(&snapshot) {
                if !stop.load(Ordering::Relaxed) {
                    on_edit(edit);
                }
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_sensitive_processes_case_insensitively() {
        assert!(blocked_process(r"C:\Apps\KeePassXC.EXE"));
        assert!(blocked_process(r"C:\Windows\System32\cmd.exe"));
        assert!(blocked_process("Code.exe")); // embedded terminals
        assert!(!blocked_process("notepad.exe"));
        assert!(!blocked_process("chrome.exe"));
    }
}
