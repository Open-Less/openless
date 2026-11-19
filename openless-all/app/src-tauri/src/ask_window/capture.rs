use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub(crate) struct AppIdentity {
    pub(crate) name: Option<String>,
    pub(crate) bundle_id: Option<String>,
}

#[derive(Debug)]
pub(crate) enum Selection {
    None,
    Text(String),
    TooLarge,
}

#[derive(Debug)]
pub(crate) struct Invocation {
    pub(crate) invocation_id: uuid::Uuid,
    pub(crate) captured_at: DateTime<Utc>,
    pub(crate) foreground_pid: Option<u32>,
    pub(crate) foreground_app: Option<AppIdentity>,
    pub(crate) selection: Selection,
}

impl Invocation {
    pub(super) fn empty(captured_at: DateTime<Utc>) -> Self {
        Self {
            invocation_id: uuid::Uuid::new_v4(),
            captured_at,
            foreground_pid: None,
            foreground_app: None,
            selection: Selection::None,
        }
    }
}

const MAX_SCALARS: usize = 4_000;
const MAX_BYTES: usize = 16 * 1024;
static INTERACTION_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(crate) fn mark_interaction() {
    INTERACTION_EPOCH.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn bounded_selection(text: String) -> Selection {
    if text.len() > MAX_BYTES || text.chars().take(MAX_SCALARS + 1).count() > MAX_SCALARS {
        Selection::TooLarge
    } else if text.trim().is_empty() {
        Selection::None
    } else {
        Selection::Text(text)
    }
}

pub(super) struct Foreground {
    pub(super) pid: u32,
    #[cfg(target_os = "windows")]
    pub(super) window: usize,
    #[cfg(target_os = "windows")]
    target: crate::selection::SelectionInsertionTarget,
}

pub(super) struct Trigger {
    pub(super) captured_at: DateTime<Utc>,
    pub(super) foreground: Option<Foreground>,
    interaction_epoch: u64,
}

impl Trigger {
    pub(super) fn at_key_event(foreground: Option<Foreground>) -> Self {
        Self {
            captured_at: Utc::now(),
            foreground,
            interaction_epoch: INTERACTION_EPOCH.load(Ordering::Relaxed),
        }
    }

    pub(super) fn read(self) -> Invocation {
        let mut invocation = Invocation::empty(self.captured_at);
        let Some(foreground) = self
            .foreground
            .filter(|foreground| foreground.pid != std::process::id())
        else {
            return invocation;
        };
        invocation.foreground_pid = Some(foreground.pid);
        invocation.foreground_app = app_identity(foreground.pid);
        let still_original = || self.interaction_epoch == INTERACTION_EPOCH.load(Ordering::Relaxed);
        if still_original() {
            let selection = read_selection(&foreground);
            if still_original() {
                invocation.selection = selection;
            }
        }
        invocation
    }
}

#[cfg(target_os = "macos")]
pub(super) fn foreground() -> Option<Foreground> {
    use objc2::{
        msg_send,
        rc::autoreleasepool,
        runtime::{AnyClass, AnyObject},
    };

    autoreleasepool(|_| unsafe {
        let class = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        let application: *mut AnyObject = msg_send![workspace, frontmostApplication];
        let pid: i32 = msg_send![application, processIdentifier];
        (pid > 0).then_some(Foreground { pid: pid as u32 })
    })
}

#[cfg(target_os = "macos")]
fn app_identity(pid: u32) -> Option<AppIdentity> {
    use objc2::{
        msg_send,
        rc::autoreleasepool,
        runtime::{AnyClass, AnyObject},
    };
    use std::ffi::CStr;

    unsafe fn string(object: *mut AnyObject) -> Option<String> {
        let bytes: *const std::ffi::c_char = msg_send![object, UTF8String];
        (!bytes.is_null()).then(|| CStr::from_ptr(bytes).to_string_lossy().into_owned())
    }

    autoreleasepool(|_| unsafe {
        let class = AnyClass::get("NSRunningApplication")?;
        let application: *mut AnyObject =
            msg_send![class, runningApplicationWithProcessIdentifier: pid as i32];
        if application.is_null() {
            return None;
        }
        let name: *mut AnyObject = msg_send![application, localizedName];
        let bundle: *mut AnyObject = msg_send![application, bundleIdentifier];
        Some(AppIdentity {
            name: string(name),
            bundle_id: string(bundle),
        })
    })
}

#[cfg(target_os = "macos")]
fn read_selection(original: &Foreground) -> Selection {
    let still_front = || foreground().is_some_and(|now| now.pid == original.pid);
    if !still_front() || crate::unicode_keystroke::is_secure_input_enabled() {
        return Selection::None;
    }
    let selection = match crate::selection::read_native_ask_selection_for_pid(original.pid) {
        Ok(Some(text)) => bounded_selection(text),
        Ok(None) => Selection::None,
        Err(()) => Selection::TooLarge,
    };
    if still_front() && !crate::unicode_keystroke::is_secure_input_enabled() {
        selection
    } else {
        Selection::None
    }
}

#[cfg(target_os = "windows")]
pub(super) fn foreground() -> Option<Foreground> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    let target = crate::selection::capture_selection_insertion_target();
    if pid == 0 || unsafe { GetForegroundWindow() } != window {
        return None;
    }
    Some(Foreground {
        pid,
        window: window.0 as usize,
        target,
    })
}

#[cfg(target_os = "windows")]
fn app_identity(pid: u32) -> Option<AppIdentity> {
    use windows::core::PWSTR;
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; 1024];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let name = path[..length as usize]
            .rsplit(|unit| *unit == b'\\' as u16)
            .next()?;
        Some(AppIdentity {
            name: Some(String::from_utf16_lossy(name)),
            bundle_id: None,
        })
    }
}

#[cfg(target_os = "windows")]
fn read_selection(original: &Foreground) -> Selection {
    let still_front = || {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        (unsafe { GetForegroundWindow().0 as usize == original.window })
            && crate::selection::selection_insertion_target_is_current(&original.target)
    };
    if !still_front() {
        return Selection::None;
    }
    let selection = crate::host_document::read_native_ask_selection_for_pid(original.pid)
        .map(bounded_selection)
        .unwrap_or(Selection::None);
    if still_front() {
        selection
    } else {
        Selection::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_limits_reject_instead_of_truncating_or_trimming() {
        assert!(matches!(bounded_selection("  ".into()), Selection::None));
        assert!(
            matches!(bounded_selection("🙂".repeat(4000)), Selection::Text(text) if text.chars().count() == 4000)
        );
        assert!(matches!(
            bounded_selection("字".repeat(4001)),
            Selection::TooLarge
        ));
        assert!(
            matches!(bounded_selection(" \ntext ".into()), Selection::Text(text) if text == " \ntext ")
        );
    }
}
