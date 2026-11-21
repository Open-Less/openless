use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub(crate) struct AppIdentity {
    pub(crate) name: Option<String>,
    pub(crate) bundle_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Invocation {
    pub(crate) invocation_id: uuid::Uuid,
    pub(crate) captured_at: DateTime<Utc>,
    pub(crate) foreground_pid: Option<u32>,
    pub(crate) foreground_app: Option<AppIdentity>,
}

impl Invocation {
    pub(super) fn empty(captured_at: DateTime<Utc>) -> Self {
        Self {
            invocation_id: uuid::Uuid::new_v4(),
            captured_at,
            foreground_pid: None,
            foreground_app: None,
        }
    }
}

pub(super) struct Foreground {
    pub(super) pid: u32,
    #[cfg(target_os = "windows")]
    pub(super) window: usize,
}

pub(super) struct Trigger {
    pub(super) captured_at: DateTime<Utc>,
    pub(super) foreground: Option<Foreground>,
    #[cfg(target_os = "macos")]
    pub(super) screen: super::macos_window::ScreenPoint,
}

impl Trigger {
    pub(super) fn at_key_event(foreground: Option<Foreground>) -> Self {
        Self {
            captured_at: Utc::now(),
            foreground,
            #[cfg(target_os = "macos")]
            screen: super::macos_window::ScreenPoint::capture(),
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
pub(super) fn app_identity(pid: u32) -> Option<AppIdentity> {
    use objc2::{
        msg_send,
        rc::autoreleasepool,
        runtime::{AnyClass, AnyObject},
    };
    use std::ffi::CStr;

    unsafe fn string(object: *mut AnyObject) -> Option<String> {
        if object.is_null() {
            return None;
        }
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

#[cfg(target_os = "windows")]
pub(super) fn foreground() -> Option<Foreground> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 || unsafe { GetForegroundWindow() } != window {
        return None;
    }
    Some(Foreground {
        pid,
        window: window.0 as usize,
    })
}

#[cfg(target_os = "windows")]
pub(super) fn app_identity(pid: u32) -> Option<AppIdentity> {
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
