//! macOS 剪贴板后端。
//!
//! 目前只有纯文本通道（`arboard`）。与 Windows 相比缺两件事，后续阶段补齐：
//!
//! 1. 全格式快照：现在只能备份/还原纯文本，图片与富文本在还原时会被降级；
//! 2. 历史控制：macOS 没有 Windows 那种"剪贴板历史"系统服务，只有第三方
//!    管理器，因此 `Intent` 在这里没有可施加的标记，只做语义占位。

use crate::clipboard::ClipboardError;

/// macOS 快照：只有文本。
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
    let mut clipboard = arboard::Clipboard::new().map_err(|_| ClipboardError::Busy)?;
    match clipboard.get_text() {
        Ok(text) => Ok(Some(text)),
        // 与迁移前一致：macOS 侧把"没有文本内容"和"读失败"统一当作无文本，
        // 因为上游 arboard 无法可靠区分，且这条路径只用于还原前的参考读取。
        Err(_) => Ok(None),
    }
}

pub fn set_text(text: &str, _intent: crate::clipboard::Intent) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|_| ClipboardError::Busy)?;
    clipboard
        .set_text(text.to_string())
        .map_err(|_| ClipboardError::Write("arboard::set_text"))
}

pub fn snapshot() -> Result<Snapshot, ClipboardError> {
    Ok(Snapshot { text: read_text()? })
}

pub fn restore(snapshot: &Snapshot) -> Result<(), ClipboardError> {
    // 与迁移前行为一致：原本没有文本时不做任何写入，避免把用户的非文本内容清空。
    match snapshot.text.as_deref() {
        Some(text) => set_text(text, crate::clipboard::Intent::Internal),
        None => Ok(()),
    }
}

/// macOS 没有可查询的剪贴板序列号。
pub fn sequence_number() -> u32 {
    0
}
