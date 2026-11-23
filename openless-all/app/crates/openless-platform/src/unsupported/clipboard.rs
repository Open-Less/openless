//! 其余平台（Android / iOS / Linux）的剪贴板后端占位。
//!
//! 这些平台目前由宿主各自处理（Android 走 JNI、Linux 走 fcitx5），
//! 迁到本 crate 之前先返回明确错误，避免出现"看起来成功"的静默失败。

use crate::clipboard::ClipboardError;

#[derive(Debug)]
pub struct Snapshot;

impl Snapshot {
    pub fn text(&self) -> Option<String> {
        None
    }

    pub fn format_count_for_log(&self) -> usize {
        0
    }
}

pub fn has_text() -> bool {
    false
}

pub fn read_text() -> Result<Option<String>, ClipboardError> {
    Ok(None)
}

pub fn set_text(_text: &str, _intent: crate::clipboard::Intent) -> Result<(), ClipboardError> {
    Err(ClipboardError::Write("clipboard backend not implemented on this platform"))
}

pub fn snapshot() -> Result<Snapshot, ClipboardError> {
    Err(ClipboardError::Write("clipboard backend not implemented on this platform"))
}

pub fn restore(_snapshot: &Snapshot) -> Result<(), ClipboardError> {
    Err(ClipboardError::Write("clipboard backend not implemented on this platform"))
}

pub fn sequence_number() -> u32 {
    0
}
