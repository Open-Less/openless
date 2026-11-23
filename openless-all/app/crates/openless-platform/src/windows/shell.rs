//! Windows 外壳集成：在文件管理器中打开路径、打开系统设置页/URL。

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 用系统默认程序打开一个 URI（`ms-settings:`、`https:`、文件路径都可以）。
pub fn open(uri: &str) -> Result<(), String> {
    let operation = wide_null("open");
    let target = wide_null(uri);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW 返回值 > 32 表示成功（历史约定）。
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("open {uri}: ShellExecuteW failed"))
    }
}

/// 在文件管理器中定位一个路径。
pub fn open_path_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    open(&path.display().to_string())
}

/// 打开"隐私/声音/辅助功能"设置页。
pub fn open_settings_pane(pane: &str) -> Result<(), String> {
    let uri = match pane {
        "microphone" => "ms-settings:privacy-microphone",
        "sound" => "ms-settings:sound",
        "accessibility" => "ms-settings:easeofaccess",
        _ => "ms-settings:",
    };
    open(uri)
}
