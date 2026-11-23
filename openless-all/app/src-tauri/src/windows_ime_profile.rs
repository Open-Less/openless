//! Windows IME 语言配置（宿主薄封装）。
//!
//! 注册表读写已迁到 `openless-platform::windows::ime_profile`；非 Windows 平台
//! 提供显式的"不适用"结果，命令层在所有平台都能编译。

#[cfg(target_os = "windows")]
pub use openless_platform::windows::ime_profile::{
    apply_windows_openless_keyboard_list, get_windows_ime_status,
    is_openless_language_profile_enabled, set_openless_language_profile_enabled,
};

#[cfg(not(target_os = "windows"))]
mod non_windows {
    use openless_core::shared_types::{WindowsImeInstallState, WindowsImeStatus};

    /// 非 Windows：没有 IME 语言配置可改，直接返回成功（调用方只关心错误）。
    pub fn apply_windows_openless_keyboard_list(_enabled: bool) -> Result<(), String> {
        Ok(())
    }

    /// 非 Windows：状态明确标记"不可用"。
    pub fn get_windows_ime_status() -> WindowsImeStatus {
        WindowsImeStatus {
            state: WindowsImeInstallState::NotWindows,
            using_tsf_backend: false,
            message: String::new(),
            dll_path: None,
        }
    }

    pub fn is_openless_language_profile_enabled() -> bool {
        false
    }

    pub fn set_openless_language_profile_enabled(_enabled: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub use non_windows::*;
