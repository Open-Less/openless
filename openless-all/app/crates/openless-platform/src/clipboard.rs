//! 剪贴板能力。
//!
//! 宿主只能通过这里的 [`Clipboard`] 访问剪贴板，禁止再直接依赖 `arboard` 或
//! 调用 Win32 剪贴板 API。各平台实现放在下面的私有后端模块里：
//!
//! - `windows/clipboard`：全格式快照/事务还原 + 历史排除标记；`windows/history`：历史清理
//! - `macos/clipboard`：目前只有纯文本通道，全格式快照待补
//! - `unsupported`：其余平台（Android/iOS/Linux）暂未实现，返回明确错误
//!
//! 两处有意保留的不对称：
//! - **打开失败**：Windows 打不开返回 [`ClipboardError::Busy`]，调用方必须放弃，
//!   以免在没有备份的情况下覆盖用户内容；macOS 的 `arboard` 无法可靠区分
//!   "无文本"和"读失败"，因此统一视为"无文本"（与迁移前行为一致）。
//! - **序列号**：只有 Windows 提供，其余平台返回 0。

#[cfg(target_os = "windows")]
use crate::windows::clipboard as backend;
#[cfg(target_os = "windows")]
pub use crate::windows::history;

#[cfg(target_os = "macos")]
use crate::macos::clipboard as backend;

#[cfg(target_os = "android")]
use crate::android::clipboard as backend;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
use crate::unsupported::clipboard as backend;

use std::fmt;

/// 这次写入剪贴板的意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// 内部传参：听写文本、抓选区前的备份、粘贴后的还原。
    /// 支持的平台会让它不进剪贴板历史、也不同步到云端。
    Internal,
    /// 用户可见：失败卡的"复制"按钮、显式的"同步到剪贴板"。
    UserVisible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardError {
    /// 剪贴板被别的进程占用，重试后仍然打不开。
    Busy,
    /// 分配内存或写剪贴板失败。
    Write(&'static str),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => write!(f, "clipboard is owned by another process"),
            Self::Write(step) => write!(f, "clipboard write failed at {step}"),
        }
    }
}

/// 一次剪贴板快照。
///
/// 内容对调用方不透明：Windows 保存全部可搬移格式，macOS 目前只有纯文本。
#[derive(Debug)]
pub struct Snapshot(backend::Snapshot);

impl Snapshot {
    /// 快照当时的纯文本内容。
    pub fn text(&self) -> Option<String> {
        self.0.text()
    }

    /// 快照里保存的格式数量（仅用于日志诊断）。
    pub fn format_count_for_log(&self) -> usize {
        self.0.format_count_for_log()
    }
}

/// 剪贴板能力的唯一入口。
pub struct Clipboard;

impl Clipboard {
    /// 剪贴板里有没有文本格式。
    pub fn has_text() -> bool {
        backend::has_text()
    }

    /// 读当前剪贴板文本。
    ///
    /// `Ok(None)` 表示没有文本或读不出来；`Err(Busy)` 表示打不开。
    pub fn read_text() -> Result<Option<String>, ClipboardError> {
        backend::read_text()
    }

    /// 写纯文本。`Intent::Internal` 在支持的平台上不会进入剪贴板历史。
    pub fn set_text(text: &str, intent: Intent) -> Result<(), ClipboardError> {
        backend::set_text(text, intent)
    }

    /// 备份全部可搬移格式。打开失败必须当成"不能继续"，不能当作空剪贴板。
    pub fn snapshot() -> Result<Snapshot, ClipboardError> {
        backend::snapshot().map(Snapshot)
    }

    /// 用快照整体还原剪贴板。
    pub fn restore(snapshot: &Snapshot) -> Result<(), ClipboardError> {
        backend::restore(&snapshot.0)
    }

    /// 剪贴板序列号；不支持的后端恒为 0。
    pub fn sequence_number() -> u32 {
        backend::sequence_number()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_is_copy_so_callers_can_pass_it_by_value() {
        let intent = Intent::Internal;
        let copy = intent;
        assert_eq!(copy, Intent::Internal);
        assert_ne!(Intent::Internal, Intent::UserVisible);
    }

    #[test]
    fn errors_are_comparable_and_readable() {
        assert_eq!(ClipboardError::Busy, ClipboardError::Busy);
        assert_eq!(
            ClipboardError::Busy.to_string(),
            "clipboard is owned by another process"
        );
        assert_eq!(
            ClipboardError::Write("SetClipboardData").to_string(),
            "clipboard write failed at SetClipboardData"
        );
    }
}
