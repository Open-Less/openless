//! 文本写入能力。
//!
//! 与剪贴板路径并列的"零剪贴板"写入口：逐字 Unicode 输入与模拟粘贴快捷键。
//!
//! 平台差异是**语义级**的，不是实现细节：
//! - Windows：SendInput 逐字 Unicode，绕开 TSF 与输入法，换行由 `newline_mode` 决定；
//! - macOS：只能模拟 Cmd+V（逐字输入与输入法切换仍在宿主，涉及 TIS 与流式节奏）；
//! - 其余平台：返回 [`TextError::Unsupported`]。
//!
//! 宿主负责的是**策略**（什么时候用 TSF、什么时候兜底到逐字、失败后是否复制），
//! 这里只负责"把事件投递出去"。

#[cfg(target_os = "windows")]
use crate::windows::text as backend;

#[cfg(target_os = "macos")]
use crate::macos::text as backend;

#[cfg(target_os = "android")]
use crate::android::text as backend;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
use crate::unsupported::text as backend;

pub use openless_core::shared_types::{PasteShortcut, WindowsSendInputNewlineMode};

/// 文本写入失败。与按键注入共用 [`PlatformError`]，宿主只需一套"部分失败"语义。
pub use crate::common::error::PlatformError as TextError;

/// 模拟一次粘贴快捷键。
///
/// macOS 固定为 Cmd+V，`shortcut` 参数被忽略（与迁移前一致）。
pub fn send_paste_shortcut(shortcut: PasteShortcut) -> Result<(), TextError> {
    backend::send_paste_shortcut(shortcut)
}

/// 逐字投递文本，不碰剪贴板；返回已投递的源字符数。
///
/// 源字符数按 Unicode scalar 计，`\r\n` 里的 `\r` 计入但不产生单独事件。
pub fn type_text(
    text: &str,
    newline_mode: WindowsSendInputNewlineMode,
) -> Result<usize, TextError> {
    backend::type_text(text, newline_mode)
}

/// 宿主用来核对"部分输入"的期望字符数。
pub fn expected_typed_chars(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_errors_report_the_consumed_prefix() {
        let error = TextError::Partial {
            typed_chars: 7,
            source: Box::new(TextError::SendFailed("fixture".into())),
        };
        assert_eq!(error.typed_chars(), 7);
        assert!(error.to_string().contains("after 7 chars"));
    }

    #[test]
    fn non_partial_errors_report_zero_typed_chars() {
        assert_eq!(TextError::Unsupported.typed_chars(), 0);
        assert_eq!(TextError::SendFailed("x".into()).typed_chars(), 0);
    }

    #[test]
    fn expected_typed_chars_counts_unicode_scalars_not_bytes() {
        assert_eq!(expected_typed_chars("a你🙂"), 3);
        assert_eq!(expected_typed_chars(""), 0);
    }
}
