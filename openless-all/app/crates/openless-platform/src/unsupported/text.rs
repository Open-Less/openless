//! 其余平台的文本写入占位。

use crate::common::error::PlatformError;
use crate::text::{PasteShortcut, WindowsSendInputNewlineMode};

pub fn send_paste_shortcut(_shortcut: PasteShortcut) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}

pub fn type_text(
    _text: &str,
    _newline_mode: WindowsSendInputNewlineMode,
) -> Result<usize, PlatformError> {
    Err(PlatformError::Unsupported)
}
