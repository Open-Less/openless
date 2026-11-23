//! Windows IME 语言配置（宿主薄封装）。
//!
//! 注册表读写已迁到 `openless-platform::windows::ime_profile`。

pub use openless_platform::windows::ime_profile::{
    apply_windows_openless_keyboard_list, get_windows_ime_status,
    is_openless_language_profile_enabled, set_openless_language_profile_enabled,
};
