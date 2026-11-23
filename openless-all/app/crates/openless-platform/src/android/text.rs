//! Android 文本写入后端（经无障碍服务）。

use openless_core::shared_types::{PasteShortcut, WindowsSendInputNewlineMode};

use crate::common::error::PlatformError;

/// Android 的粘贴快捷键是系统级操作，这里没有可模拟的键。
pub fn send_paste_shortcut(_shortcut: PasteShortcut) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}

/// 逐字输入在 Android 上等同于"经无障碍插入"；不支持部分投递计数，因此要么全成要么失败。
pub fn type_text(
    text: &str,
    _newline_mode: WindowsSendInputNewlineMode,
) -> Result<usize, PlatformError> {
    match super::env() {
        Some(env) => match env.insert_via_accessibility(text) {
            Ok(true) => Ok(text.chars().count()),
            Ok(false) => Err(PlatformError::SendFailed(
                "accessibility insertion was rejected".into(),
            )),
            Err(error) => Err(PlatformError::SendFailed(error)),
        },
        None => Err(PlatformError::Unsupported),
    }
}
