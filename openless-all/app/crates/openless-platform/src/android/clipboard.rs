//! Android 剪贴板后端（经宿主注入的 JNI 环境）。

use crate::clipboard::{ClipboardError, Intent};

/// Android 没有可查询的剪贴板序列号。
#[derive(Debug)]
pub struct Snapshot {
    text: Option<String>,
}

impl Snapshot {
    pub fn text(&self) -> Option<String> {
        self.text.clone()
    }

    pub fn format_count_for_log(&self) -> usize {
        usize::from(self.text.is_some())
    }
}

pub fn has_text() -> bool {
    matches!(read_text(), Ok(Some(_)))
}

pub fn read_text() -> Result<Option<String>, ClipboardError> {
    match super::env() {
        Some(env) => env.clipboard_read_text().map_err(|error| {
            log::warn!("[clipboard] android read failed: {error}");
            ClipboardError::Write("android clipboard read")
        }),
        None => Err(ClipboardError::Write("android env is not installed")),
    }
}

pub fn set_text(text: &str, _intent: Intent) -> Result<(), ClipboardError> {
    // Android 没有 Windows 那种剪贴板历史排除协议，`Intent` 在这里没有可施加的标记。
    match super::env() {
        Some(env) => env.clipboard_write_text(text).map_err(|error| {
            log::warn!("[clipboard] android write failed: {error}");
            ClipboardError::Write("android clipboard write")
        }),
        None => Err(ClipboardError::Write("android env is not installed")),
    }
}

pub fn snapshot() -> Result<Snapshot, ClipboardError> {
    Ok(Snapshot { text: read_text()? })
}

pub fn restore(snapshot: &Snapshot) -> Result<(), ClipboardError> {
    match snapshot.text.as_deref() {
        Some(text) => set_text(text, Intent::Internal),
        None => Ok(()),
    }
}

pub fn sequence_number() -> u32 {
    0
}
