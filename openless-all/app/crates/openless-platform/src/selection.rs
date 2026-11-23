//! 选区读取能力。
//!
//! 宿主侧的读取顺序是"越少副作用越优先"：TSF → UIA/AX → 这里的模拟复制。
//! 本模块只提供最后一层：让目标应用自己把选区复制到剪贴板，再把剪贴板还原。
//!
//! 与其它能力的关系：**只通过门面组合**（`clipboard` + `input`），彼此不共享内部状态。

#[cfg(target_os = "windows")]
use crate::windows::selection as backend;

#[cfg(target_os = "windows")]
pub mod windows_target;

#[cfg(target_os = "macos")]
use crate::macos::selection as backend;

#[cfg(target_os = "android")]
use crate::android::selection as backend;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
use crate::unsupported::selection as backend;

/// 模拟复制通道的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// 读到了选区文本；可能为空串（目标应用复制了空内容）。
    Captured(String),
    /// 剪贴板没变：目标应用没有选区，或复制没有生效。
    Unchanged,
    /// 剪贴板变了但读不出文本。
    Unreadable,
    /// 复制快捷键没能发出去。
    CopyFailed,
    /// 剪贴板被别的进程占用，无法安全备份，因此放弃。
    ClipboardBusy,
    /// 本平台没有这条通道。
    Unsupported,
}

impl CaptureOutcome {
    /// 给人看的原因标签，用于日志与"无选区"诊断。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Captured(_) => "captured",
            Self::Unchanged => "clipboard_unchanged",
            Self::Unreadable => "clipboard_unreadable",
            Self::CopyFailed => "copy_shortcut_failed",
            Self::ClipboardBusy => "clipboard_busy",
            Self::Unsupported => "unsupported",
        }
    }
}

/// 用模拟复制读取当前选区。
///
/// 成功与否都会尽量把用户剪贴板还原；失败原因通过 [`CaptureOutcome`] 返回，
/// 调用方据此决定是否继续尝试其它读取通道。
pub fn capture_via_simulated_copy() -> CaptureOutcome {
    backend::capture_via_simulated_copy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_have_distinct_labels() {
        let labels = [
            CaptureOutcome::Captured(String::new()).label(),
            CaptureOutcome::Unchanged.label(),
            CaptureOutcome::Unreadable.label(),
            CaptureOutcome::CopyFailed.label(),
            CaptureOutcome::ClipboardBusy.label(),
            CaptureOutcome::Unsupported.label(),
        ];
        let unique: std::collections::BTreeSet<&str> = labels.into_iter().collect();
        assert_eq!(unique.len(), labels.len());
    }
}
