#![cfg(target_os = "windows")]

use std::ffi::{CStr, OsStr};
use std::os::windows::ffi::OsStrExt;

use sha2::{Digest, Sha256};
use tauri::{plugin, AppHandle, Manager, RunEvent, Runtime};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
    GetWindowLongPtrW, GetWindowLongW, GetWindowThreadProcessId, RegisterClassExW, SendMessageW,
    SetWindowLongPtrW, SetWindowLongW, GWLP_USERDATA, GWL_STYLE, WINDOW_LONG_PTR_INDEX,
    WM_COPYDATA, WM_CREATE, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
};

const WMCOPYDATA_SINGLE_INSTANCE_DATA: usize = 1542;

struct MutexHandle(isize);
struct TargetWindowHandle(isize);

type SingleInstanceCallback<R> =
    dyn FnMut(&AppHandle<R>, Vec<String>, String) + Send + Sync + 'static;

struct UserData<R: Runtime> {
    app: AppHandle<R>,
    callback: Box<SingleInstanceCallback<R>>,
}

impl<R: Runtime> UserData<R> {
    unsafe fn from_hwnd_raw(hwnd: HWND) -> *mut Self {
        get_window_long_ptr(hwnd, GWLP_USERDATA) as *mut Self
    }

    unsafe fn from_hwnd(hwnd: HWND) -> &'static mut Self {
        &mut *Self::from_hwnd_raw(hwnd)
    }

    fn run_callback(&mut self, args: Vec<String>, cwd: String) {
        (self.callback)(&self.app, args, cwd);
    }
}

/// Separate installations must not forward their launch intents to one another.
/// The executable path is hashed so the mutex and window names stay bounded and do not expose it.
fn instance_key_for_path(path: &std::path::Path) -> String {
    let normalized = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn executable_instance_key() -> String {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("com.openless.app"));
    instance_key_for_path(&executable)
}

fn encode_wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn init<R: Runtime, F: FnMut(&AppHandle<R>, Vec<String>, String) + Send + Sync + 'static>(
    callback: F,
) -> tauri::plugin::TauriPlugin<R> {
    plugin::Builder::new("single-instance")
        .setup(move |app, _api| {
            let key = executable_instance_key();
            let class_name = encode_wide(format!("OpenLess-{key}-sic"));
            let window_name = encode_wide(format!("OpenLess-{key}-siw"));
            let mutex_name = encode_wide(format!("OpenLess-{key}-sim"));
            let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr()) };

            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe {
                    let hwnd = FindWindowW(class_name.as_ptr(), window_name.as_ptr());
                    if !hwnd.is_null() {
                        let mut pid = 0;
                        GetWindowThreadProcessId(hwnd, &mut pid);
                        if pid != 0 {
                            AllowSetForegroundWindow(pid);
                        }

                        let cwd = std::env::current_dir().unwrap_or_default();
                        let cwd = cwd.to_string_lossy();
                        let args = std::env::args().collect::<Vec<String>>().join("|");
                        let data = format!("{cwd}|{args}\0");
                        let bytes = data.as_bytes();
                        let copy_data = COPYDATASTRUCT {
                            dwData: WMCOPYDATA_SINGLE_INSTANCE_DATA,
                            cbData: bytes.len() as u32,
                            lpData: bytes.as_ptr() as _,
                        };
                        SendMessageW(hwnd, WM_COPYDATA, 0, &copy_data as *const _ as LPARAM);
                        app.cleanup_before_exit();
                        std::process::exit(0);
                    }
                }
            } else {
                app.manage(MutexHandle(mutex as isize));
                let userdata = Box::new(UserData {
                    app: app.clone(),
                    callback: Box::new(callback),
                });
                let userdata = Box::into_raw(userdata);
                let hwnd =
                    unsafe { create_event_target_window(&class_name, &window_name, userdata) };
                app.manage(TargetWindowHandle(hwnd as isize));
            }
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                destroy(app);
            }
        })
        .build()
}

fn destroy<R: Runtime, M: Manager<R>>(manager: &M) {
    if let Some(mutex) = manager.try_state::<MutexHandle>() {
        unsafe {
            ReleaseMutex(mutex.0 as _);
            CloseHandle(mutex.0 as _);
        }
    }
    if let Some(window) = manager.try_state::<TargetWindowHandle>() {
        unsafe {
            DestroyWindow(window.0 as _);
        }
    }
}

#[cfg(target_pointer_width = "32")]
unsafe fn set_window_long_ptr(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    SetWindowLongW(hwnd, index, value as i32) as isize
}

#[cfg(target_pointer_width = "64")]
unsafe fn set_window_long_ptr(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    SetWindowLongPtrW(hwnd, index, value)
}

#[cfg(target_pointer_width = "32")]
unsafe fn get_window_long_ptr(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    GetWindowLongW(hwnd, index) as isize
}

#[cfg(target_pointer_width = "64")]
unsafe fn get_window_long_ptr(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    GetWindowLongPtrW(hwnd, index)
}

unsafe extern "system" fn single_instance_window_proc<R: Runtime>(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let create =
                &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            let userdata = create.lpCreateParams as *const UserData<R>;
            set_window_long_ptr(hwnd, GWLP_USERDATA, userdata as isize);
            0
        }
        WM_COPYDATA => {
            let copy_data = &*(lparam as *const COPYDATASTRUCT);
            if copy_data.dwData == WMCOPYDATA_SINGLE_INSTANCE_DATA {
                let userdata = UserData::<R>::from_hwnd(hwnd);
                let data = CStr::from_ptr(copy_data.lpData as _).to_string_lossy();
                let mut parts = data.split('|');
                let cwd = parts.next().unwrap_or_default().to_string();
                let args = parts.map(str::to_string).collect();
                userdata.run_callback(args, cwd);
            }
            1
        }
        WM_DESTROY => {
            let userdata = UserData::<R>::from_hwnd_raw(hwnd);
            if !userdata.is_null() {
                drop(Box::from_raw(userdata));
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_event_target_window<R: Runtime>(
    class_name: &[u16],
    window_name: &[u16],
    userdata: *const UserData<R>,
) -> HWND {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(single_instance_window_proc::<R>),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(std::ptr::null()),
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    RegisterClassExW(&class);
    let hwnd = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
        class_name.as_ptr(),
        window_name.as_ptr(),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        userdata as _,
    );
    set_window_long_ptr(hwnd, GWL_STYLE, (WS_VISIBLE | WS_POPUP) as isize);
    hwnd
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{executable_instance_key, instance_key_for_path};

    #[test]
    fn instance_key_is_bounded_and_hashed() {
        let key = executable_instance_key();
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn different_install_paths_do_not_share_an_instance_key() {
        assert_ne!(
            instance_key_for_path(Path::new(r"C:\OpenLess\OpenLess.exe")),
            instance_key_for_path(Path::new(r"D:\OpenLess\OpenLess.exe")),
        );
        assert_eq!(
            instance_key_for_path(Path::new(r"C:\OpenLess\OpenLess.exe")),
            instance_key_for_path(Path::new(r"c:/openless/openless.exe")),
        );
    }
}
