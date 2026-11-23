//! macOS 文本写入后端。
//!
//! 只迁入了"模拟 Cmd+V"；逐字输入与输入法（TIS）切换仍留在宿主，
//! 因为它与流式输入的节奏、`Secure Event Input` 探测、主线程断言绑在一起。

use openless_core::shared_types::{PasteShortcut, WindowsSendInputNewlineMode};

use crate::common::error::PlatformError;

/// macOS 只有 Cmd+V；`shortcut` 参数为跨平台签名保留，实际被忽略。
pub fn send_paste_shortcut(shortcut: PasteShortcut) -> Result<(), PlatformError> {
    super::cgevent::send_paste_shortcut(shortcut)
}

/// 逐字输入尚未从宿主迁出。
pub fn type_text(
    _text: &str,
    _newline_mode: WindowsSendInputNewlineMode,
) -> Result<usize, PlatformError> {
    Err(PlatformError::Unsupported)
}
