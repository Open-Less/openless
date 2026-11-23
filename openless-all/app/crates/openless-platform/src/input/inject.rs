//! 按键注入：把合成按键投递出去。
//!
//! 与文本写入共用同一种错误类型（[`PlatformError`]），但实现上互不依赖。

#[cfg(target_os = "windows")]
use crate::windows::input as backend;

#[cfg(target_os = "macos")]
use crate::macos::cgevent as backend;

#[cfg(target_os = "android")]
use crate::android::input as backend;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
use crate::unsupported::input as backend;

pub use crate::common::error::PlatformError;

/// 发出"复制"快捷键（Windows Ctrl+C / macOS Cmd+C）。
pub fn send_copy_shortcut() -> Result<(), PlatformError> {
    backend::send_copy_shortcut()
}

#[cfg(test)]
mod tests {
    #[test]
    fn platform_error_alias_is_shared_with_text_writing() {
        // 两个能力共用同一种错误类型，宿主只需处理一套"部分失败"语义。
        let error = crate::input::PlatformError::SendFailed("fixture".into());
        let as_text_error: crate::text::TextError = error;
        assert_eq!(as_text_error.typed_chars(), 0);
    }
}
