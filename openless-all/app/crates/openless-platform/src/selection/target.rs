//! 选区插入目标的指纹与校验。
//!
//! "用户当初选中的那个控件还在前台吗？" 这个判断必须由平台层给出：
//! - Windows：前台窗口 + 焦点子控件 + 各自 pid/tid（见 [`super::windows_target`]）；
//! - macOS：前台应用（名称 + bundle id + pid）。
//!
//! macOS 的前台应用查询需要 NSWorkspace，平台层不直接链接 AppKit，
//! 由宿主通过 [`install_front_app_probe`] 注入。

use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use crate::selection::windows_target::WindowsSelectionTarget;

/// macOS 捕获时的前台应用信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacosSelectionTarget {
    /// 捕获时的前台应用（`name (bundle)` 形式）。
    pub front_app: Option<String>,
    /// 与前台应用一起捕获的 bundle id（供宿主文档安全检查使用）。
    pub bundle_id: Option<String>,
    /// 捕获时的前台应用 pid（预览确认后用于把焦点还给原应用）。
    pub front_app_pid: Option<i32>,
}

/// 一次选区捕获的插入目标。
#[derive(Debug, Clone, Default)]
pub struct SelectionInsertionTarget {
    #[cfg(target_os = "windows")]
    pub windows: Option<WindowsSelectionTarget>,
    #[cfg(target_os = "macos")]
    pub macos: Option<MacosSelectionTarget>,
}

/// 粘贴前最后一次目标复核的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionInsertionTargetValidation {
    Valid,
    TargetUnavailable,
    TargetChanged,
    SelectionChanged,
}

impl SelectionInsertionTargetValidation {
    /// 对应的用户可见错误码；`Valid` 没有错误码。
    pub const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Valid => None,
            Self::TargetUnavailable => Some("selectionPolishTargetUnavailable"),
            Self::TargetChanged => Some("selectionPolishTargetChanged"),
            Self::SelectionChanged => Some("selectionPolishSelectionChanged"),
        }
    }
}

type FrontAppPidProbe = fn() -> Option<i32>;

static FRONT_APP_PID: OnceLock<FrontAppPidProbe> = OnceLock::new();

/// 注入 macOS 前台应用 pid 查询（宿主启动时调用一次）。
pub fn install_front_app_probe(probe: FrontAppPidProbe) {
    let _ = FRONT_APP_PID.set(probe);
}

/// 目标是否仍是捕获时那个控件。
pub fn is_current(target: &SelectionInsertionTarget) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some(captured) = target.windows else {
            return false;
        };
        return super::windows_target::capture().as_ref() == Some(&captured);
    }

    #[cfg(target_os = "macos")]
    {
        let Some(captured_pid) = target
            .macos
            .as_ref()
            .and_then(|captured| captured.front_app_pid)
        else {
            return false;
        };
        return FRONT_APP_PID.get().copied().and_then(|probe| probe()) == Some(captured_pid);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = target;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_codes_are_stable_contracts() {
        assert_eq!(SelectionInsertionTargetValidation::Valid.error_code(), None);
        assert_eq!(
            SelectionInsertionTargetValidation::TargetUnavailable.error_code(),
            Some("selectionPolishTargetUnavailable")
        );
        assert_eq!(
            SelectionInsertionTargetValidation::TargetChanged.error_code(),
            Some("selectionPolishTargetChanged")
        );
        assert_eq!(
            SelectionInsertionTargetValidation::SelectionChanged.error_code(),
            Some("selectionPolishSelectionChanged")
        );
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn targets_are_current_on_platforms_without_a_fingerprint() {
        assert!(is_current(&SelectionInsertionTarget::default()));
    }
}
