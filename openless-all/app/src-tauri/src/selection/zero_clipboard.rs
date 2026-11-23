//! Windows 选区读取的零剪贴板通道。
//!
//! 顺序与写入路径保持一致，都是"能不用剪贴板就不用"：
//!
//! 1. **TSF**：由我们自己的文本服务直接读宿主应用的活动选区（`ITfContext::GetSelection`），
//!    完全不动剪贴板，也不需要事后清理历史；
//! 2. **UIA**：`TextPattern.GetSelection`，覆盖没有激活 TSF 服务的应用；
//! 3. 两条都失败时由调用方回退到模拟 Ctrl+C（那条路必然写剪贴板，恢复后要删历史）。
//!
//! 这里只提供前两层；回退逻辑留在 `selection.rs`，因为它需要剪贴板事务和目标校验。

use crate::windows_ime_ipc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSource {
    Tsf,
    Uia,
}

impl SelectionSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tsf => "TSF",
            Self::Uia => "UIA",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FocusedSelection {
    pub text: String,
    pub source: SelectionSource,
}

/// 只在完全不碰剪贴板的前提下读取当前焦点选区。
///
/// 返回的文本没有 trim；调用方按自己的截断策略处理。
pub fn read_focused_selection() -> Option<FocusedSelection> {
    if let Some(text) = windows_ime_ipc::read_focused_selection_text() {
        if !text.trim().is_empty() {
            return Some(FocusedSelection {
                text,
                source: SelectionSource::Tsf,
            });
        }
        log::debug!("[selection] TSF selection read returned whitespace-only text");
    }

    if let Some(text) = crate::host_document::read_qa_selection() {
        if !text.trim().is_empty() {
            return Some(FocusedSelection {
                text,
                source: SelectionSource::Uia,
            });
        }
        log::debug!("[selection] UIA selection read returned whitespace-only text");
    }

    None
}
