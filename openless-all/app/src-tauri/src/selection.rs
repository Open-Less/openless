#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
//! 跨平台「划词捕获」工具：在用户触发 QA 快捷键时尝试拿到当前前台 app 的选区文本。
//!
//! 三级 fallback：
//! 1. **macOS** AX：`AXUIElementCopyAttributeValue(focused, kAXSelectedTextAttribute)`
//!    走辅助功能 API 直读焦点元素的选区，**不**触碰剪贴板。
//! 2. **macOS / Windows** Cmd+C / Ctrl+C：snapshot 用户原剪贴板 → 模拟复制 → 80ms
//!    后读出新内容 → 还原原剪贴板。
//! 3. **Linux**：返回 `None`（AX 模式不统一，留作 best-effort 后续）。
//!
//! 截断策略：超过 4000 字符的选区只保留首 2000 + 尾 2000 + `[…truncated…]` 标记，
//! 避免给 LLM 灌过长 context。
//!
//! 模块依赖：仅 `arboard`（跨平台剪贴板）+ libc + 平台 native 框架；不依赖其它
//! Rust 模块（与 CLAUDE.md 对齐）。

use std::time::Duration;

const SELECTION_MAX_CHARS: usize = 4000;
const SELECTION_TRUNCATE_HEAD: usize = 2000;
const SELECTION_TRUNCATE_TAIL: usize = 2000;
const SELECTION_TRUNCATED_MARKER: &str = "\n[…truncated…]\n";

/// 从前台 app 读到的选区上下文。
/// `text` 已经过截断处理；`source_app` 是前台 app 的人类可读标签（可空）。
#[derive(Debug, Clone)]
pub struct SelectionContext {
    pub text: String,
    pub source_app: Option<String>,
}

/// The target that was active when Selection Polish began.  This deliberately
/// lives outside [`SelectionContext`]: QA can keep using a captured selection
/// after it moves focus to its own window, while Selection Polish must refuse
/// to paste after an asynchronous cloud request if the original target changed.
///
/// On Windows, a top-level HWND alone is not enough: clicking another editor
/// pane in the same app can retain that HWND.  We therefore retain both the
/// foreground window and the focused child control, plus their process/thread
/// identities.
///
/// On macOS we have no HWND equivalent; the closest robust fingerprint is the
/// frontmost application (name + pid) plus the selected-text snapshot itself.
/// Revalidation re-reads the current selection via AX (with the simulated
/// Cmd+C fallback) and compares it to the captured text — if the user moved to
/// another app or changed the selection during the cloud request, we refuse to
/// paste.
#[derive(Debug, Clone, Default)]
pub(crate) struct SelectionInsertionTarget {
    #[cfg(target_os = "windows")]
    windows: Option<WindowsSelectionTarget>,
    #[cfg(target_os = "macos")]
    macos: Option<MacosSelectionTarget>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsSelectionTarget {
    foreground_window: usize,
    focused_window: usize,
    foreground_process_id: u32,
    foreground_thread_id: u32,
    focused_process_id: u32,
    focused_thread_id: u32,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
struct MacosSelectionTarget {
    /// 捕获时的前台应用（NSWorkspace frontmostApplication，`name (bundle)` 形式）。
    front_app: Option<String>,
    /// 捕获时的前台应用 pid —— 预览确认后用它把焦点交还原应用。
    front_app_pid: Option<i32>,
}

/// Result of the final target/selection revalidation immediately before a
/// Selection Polish result could be pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionInsertionTargetValidation {
    Valid,
    TargetUnavailable,
    TargetChanged,
    SelectionChanged,
}

impl SelectionInsertionTargetValidation {
    pub(crate) const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Valid => None,
            Self::TargetUnavailable => Some("selectionPolishTargetUnavailable"),
            Self::TargetChanged => Some("selectionPolishTargetChanged"),
            Self::SelectionChanged => Some("selectionPolishSelectionChanged"),
        }
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
const LINUX_SELECTION_TOOLS_MISSING_WARNING: &str = "linux_selection_tools_missing";

pub struct SelectionCaptureOutcome {
    pub selection: Option<SelectionContext>,
    pub warning_code: Option<&'static str>,
}

/// Snapshot the insertion target before starting an asynchronous Selection
/// Polish request.  Windows is intentionally fail-closed when this cannot
/// identify a concrete foreground target; macOS records the frontmost app so
/// it can prove (by app + selection-text fingerprint) that the target did not
/// change before inserting.
pub(crate) fn capture_selection_insertion_target() -> SelectionInsertionTarget {
    #[cfg(target_os = "windows")]
    {
        return SelectionInsertionTarget {
            windows: capture_windows_selection_target(),
        };
    }

    #[cfg(target_os = "macos")]
    {
        return SelectionInsertionTarget {
            macos: Some(MacosSelectionTarget {
                front_app: current_front_app(),
                front_app_pid: current_front_app_pid(),
            }),
        };
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        SelectionInsertionTarget::default()
    }
}

/// Whether the target snapshot is sufficient to start a Selection Polish
/// request.  On Windows, do not send selected text to the provider if we cannot
/// later prove where it is safe to replace it.  On macOS the frontmost-app
/// snapshot is always available (there is always a frontmost app), so this
/// passes once we have it.
///
/// 非 Windows/macOS（Linux / mobile）尚未实现等效的前台校验：Linux 依赖
/// PRIMARY selection 重读做轻量校验，移动端不提供选区润色。
pub(crate) fn selection_insertion_target_is_captured(
    target: &SelectionInsertionTarget,
) -> bool {
    #[cfg(target_os = "windows")]
    {
        target.windows.is_some()
    }

    #[cfg(target_os = "macos")]
    {
        target.macos.is_some()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux：无前台窗口校验，靠「选区文本一致性」兜底——capture 时能读到
        // PRIMARY selection（run_selection_polish 已挡掉无选区），validate 时
        // 重读 PRIMARY 比较，变了就拒绝粘贴。
        let _ = target;
        true
    }
}

/// Revalidate the target and the selected text immediately before insertion.
///
/// The first and final HWND checks fence the temporary Ctrl+C used to read the
/// current selection.  This keeps the race window down to the direct handoff
/// to the inserter, while failing closed if the user moved to another app or
/// another editor/control during the cloud request.
pub(crate) fn validate_selection_insertion_target(
    target: &SelectionInsertionTarget,
    expected_selection: &str,
) -> SelectionInsertionTargetValidation {
    #[cfg(target_os = "windows")]
    {
        let Some(captured) = target.windows else {
            return SelectionInsertionTargetValidation::TargetUnavailable;
        };
        let Some(current_before_copy) = capture_windows_selection_target() else {
            return SelectionInsertionTargetValidation::TargetUnavailable;
        };
        if !windows_selection_targets_match(captured, current_before_copy) {
            return SelectionInsertionTargetValidation::TargetChanged;
        }

        let current_selection = selected_text_for_validation();
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }

        let Some(current_after_copy) = capture_windows_selection_target() else {
            return SelectionInsertionTargetValidation::TargetUnavailable;
        };
        if !windows_selection_targets_match(captured, current_after_copy) {
            return SelectionInsertionTargetValidation::TargetChanged;
        }
        return SelectionInsertionTargetValidation::Valid;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(captured) = target.macos.as_ref() else {
            return SelectionInsertionTargetValidation::TargetUnavailable;
        };
        // 前台应用一致性：云端等待期间用户切到别的应用 = 目标变更，拒绝粘贴
        //（预览确认模式在 validate 前已 reactivate 回原应用，此处应一致）。
        let front_now = current_front_app();
        if captured
            .front_app
            .as_deref()
            .is_some_and(|name| front_now.as_deref() != Some(name))
        {
            return SelectionInsertionTargetValidation::TargetChanged;
        }
        // 选区文本一致性：AX 直读（与捕获同路径），失败再走模拟 Cmd+C 兜底。
        let current_selection = read_selection_for_validation();
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }
        return SelectionInsertionTargetValidation::Valid;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux：重读 PRIMARY selection 与捕获文本比较——用户改了选区 / 清空
        // PRIMARY 就拒绝粘贴（fcitx CommitText 直接写焦点输入上下文，无需
        // 恢复窗口焦点，所以这里不需要窗口级校验）。
        let current_selection = match linux_selection::read_selected_text() {
            linux_selection::LinuxSelectionRead::Text(text) => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| truncate_selection(trimmed))
            }
            _ => None,
        };
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }
        SelectionInsertionTargetValidation::Valid
    }
}

/// macOS 专用：以与捕获时相同的形式（trim + truncate）重读当前选区，供
/// validate 与 expected_selection 比较。AX 未授权或直读失败时退化为模拟
/// Cmd+C + 剪贴板快照（与 `capture_selection_with_status` 的兜底一致）。
#[cfg(target_os = "macos")]
fn read_selection_for_validation() -> Option<String> {
    if let Some(text) = macos_ax::read_selected_text() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(truncate_selection(trimmed));
        }
    }
    let text = simulate_copy_and_read()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| truncate_selection(trimmed))
}

/// 把确认预览后的焦点交还给最初的选区目标。预览窗允许编辑，因此确认时必然不再是
/// 原应用的前台窗口；这里先恢复原目标，再沿用上面的严格选区校验，避免盲目粘贴。
pub(crate) fn reactivate_selection_insertion_target(target: &SelectionInsertionTarget) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, SetForegroundWindow};

        let Some(captured) = target.windows else {
            return false;
        };
        unsafe {
            let foreground = HWND(captured.foreground_window as *mut _);
            let _ = BringWindowToTop(foreground);
            let _ = SetForegroundWindow(foreground);
        }
        std::thread::sleep(Duration::from_millis(80));
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let Some(captured) = target.macos.as_ref() else {
            return false;
        };
        let Some(pid) = captured.front_app_pid else {
            return false;
        };
        // 预览窗是 OpenLess 自己的窗口，确认后需要把焦点交还原应用再粘贴。
        activate_app_by_pid(pid);
        std::thread::sleep(Duration::from_millis(120));
        return true;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = target;
        true
    }
}

/// macOS 专用：把指定 pid 的应用带回前台（NSRunningApplication activate，
/// NSApplicationActivateIgnoringOtherApps = 1）。失败静默——validate 仍会
/// 以选区文本一致性兜底。
#[cfg(target_os = "macos")]
fn activate_app_by_pid(pid: i32) {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    unsafe {
        let Some(cls) = AnyClass::get("NSRunningApplication") else {
            return;
        };
        let app: *mut objc2::runtime::AnyObject =
            msg_send![cls, runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return;
        }
        let _: () = msg_send![app, activateWithOptions: 1u64]; // IgnoringOtherApps
    }
}

/// 捕获选区并返回可向用户展示的非阻断平台提醒。
/// 目前仅 Linux 在 `wl-paste`、`xclip`、`xsel` 均未安装时返回提醒码。
pub fn capture_selection_with_status() -> SelectionCaptureOutcome {
    let source_app = current_front_app();

    // 1. macOS AX 直读
    #[cfg(target_os = "macos")]
    if let Some(text) = macos_ax::read_selected_text() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            log::info!(
                "[selection] AX read OK ({} chars){}",
                trimmed.chars().count(),
                source_app
                    .as_deref()
                    .map(|a| format!(" front_app={a}"))
                    .unwrap_or_default()
            );
            return SelectionCaptureOutcome {
                selection: Some(SelectionContext {
                    text: truncate_selection(trimmed),
                    source_app,
                }),
                warning_code: None,
            };
        }
    }

    // 2. 模拟复制 fallback（macOS / Windows）
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Some(text) = simulate_copy_and_read() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            log::info!(
                "[selection] simulate-copy fallback OK ({} chars){}",
                trimmed.chars().count(),
                source_app
                    .as_deref()
                    .map(|a| format!(" front_app={a}"))
                    .unwrap_or_default()
            );
            return SelectionCaptureOutcome {
                selection: Some(SelectionContext {
                    text: truncate_selection(trimmed),
                    source_app,
                }),
                warning_code: None,
            };
        }
    }

    // 3. Linux：best-effort 读 PRIMARY selection（wl-paste / xclip / xsel）。
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    match linux_selection::read_selected_text() {
        linux_selection::LinuxSelectionRead::Text(text) => {
            let trimmed = text.trim();
            log::info!(
                "[selection] linux primary selection OK ({} chars){}",
                trimmed.chars().count(),
                source_app
                    .as_deref()
                    .map(|a| format!(" front_app={a}"))
                    .unwrap_or_default()
            );
            return SelectionCaptureOutcome {
                selection: Some(SelectionContext {
                    text: truncate_selection(trimmed),
                    source_app,
                }),
                warning_code: None,
            };
        }
        linux_selection::LinuxSelectionRead::ToolsUnavailable => {
            log::warn!(
                "[selection] linux primary selection unavailable: install wl-paste, xclip, or xsel"
            );
            return SelectionCaptureOutcome {
                selection: None,
                warning_code: Some(LINUX_SELECTION_TOOLS_MISSING_WARNING),
            };
        }
        linux_selection::LinuxSelectionRead::NoSelection => {}
    }

    SelectionCaptureOutcome {
        selection: None,
        warning_code: None,
    }
}

/// 长度截断到首 + 尾 + 标记。
fn truncate_selection(text: &str) -> String {
    let total: usize = text.chars().count();
    if total <= SELECTION_MAX_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(SELECTION_TRUNCATE_HEAD).collect();
    let tail_start = total.saturating_sub(SELECTION_TRUNCATE_TAIL);
    let tail: String = text.chars().skip(tail_start).collect();
    format!("{head}{SELECTION_TRUNCATED_MARKER}{tail}")
}

// ─────────────────────────── 模拟复制 fallback (mac/win) ───────────────────────────

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn simulate_copy_and_read() -> Option<String> {
    // a) snapshot 当前剪贴板（用作还原原状态的备份）
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[selection] clipboard init failed: {e}");
            return None;
        }
    };
    let original = match clipboard.get_text() {
        Ok(t) => Some(t),
        Err(e) => {
            log::info!("[selection] clipboard get_text returned err (likely empty): {e}");
            None
        }
    };

    // b) 写一个 sentinel 进剪贴板 — 之后用来检查模拟复制是否真的有覆盖（如果还是
    //    sentinel 说明 Cmd+C 没生效或目标 app 没选区）。
    let sentinel = format!("__openless_qa_sentinel_{}__", uuid_like_token());
    if let Err(e) = clipboard.set_text(sentinel.clone()) {
        log::warn!("[selection] clipboard set_text(sentinel) failed: {e}");
        // 即使设置 sentinel 失败，也尝试发 Cmd+C 看能不能直接拿到东西
    }

    // c) 模拟 Cmd+C / Ctrl+C
    let post_ok = post_copy_shortcut();
    if !post_ok {
        log::warn!("[selection] post_copy_shortcut failed");
        // 不立刻 return：剪贴板可能已经被某些路径污染，按下方还原流程恢复。
    }

    // d) 等剪贴板更新（macOS / Windows 都需要少量时间让目标 app 把数据 put 进去）
    std::thread::sleep(Duration::from_millis(80));

    // e) 读新值
    let captured = clipboard.get_text().ok();

    // f) 还原原剪贴板
    if let Some(prev) = original {
        if let Err(e) = clipboard.set_text(prev) {
            log::warn!("[selection] clipboard restore failed: {e}");
        }
    } else {
        // 用户原剪贴板就是空 → 把 sentinel / 选区清掉，避免污染。
        if let Err(e) = clipboard.set_text("") {
            log::warn!("[selection] clipboard clear failed: {e}");
        }
    }

    let captured = captured?;
    if captured == sentinel || captured.is_empty() {
        return None;
    }
    Some(captured)
}

/// Read the current selection in the same normalized/truncated form stored by
/// [`SelectionContext`].  This is used only by the Windows final safety check;
/// the clipboard helper snapshots and restores the user's clipboard.
#[cfg(target_os = "windows")]
fn selected_text_for_validation() -> Option<String> {
    let text = simulate_copy_and_read()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| truncate_selection(trimmed))
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    test
))]
fn selection_text_matches(expected: &str, actual: Option<&str>) -> bool {
    actual.is_some_and(|actual| actual == expected)
}

#[cfg(target_os = "windows")]
fn capture_windows_selection_target() -> Option<WindowsSelectionTarget> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return None;
        }

        let mut foreground_process_id = 0;
        let foreground_thread_id =
            GetWindowThreadProcessId(foreground, Some(&mut foreground_process_id));
        if foreground_process_id == 0 || foreground_thread_id == 0 {
            return None;
        }

        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let focused = if GetGUIThreadInfo(foreground_thread_id, &mut gui_info).is_ok()
            && !gui_info.hwndFocus.0.is_null()
        {
            gui_info.hwndFocus
        } else {
            foreground
        };
        let mut focused_process_id = 0;
        let focused_thread_id = GetWindowThreadProcessId(focused, Some(&mut focused_process_id));
        if focused_process_id == 0 || focused_thread_id == 0 {
            return None;
        }

        Some(WindowsSelectionTarget {
            foreground_window: foreground.0 as usize,
            focused_window: focused.0 as usize,
            foreground_process_id,
            foreground_thread_id,
            focused_process_id,
            focused_thread_id,
        })
    }
}

#[cfg(target_os = "windows")]
fn windows_selection_targets_match(
    captured: WindowsSelectionTarget,
    current: WindowsSelectionTarget,
) -> bool {
    captured == current
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn uuid_like_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(target_os = "macos")]
fn post_copy_shortcut() -> bool {
    macos_paste::post_cmd_c().is_ok()
}

#[cfg(target_os = "windows")]
fn post_copy_shortcut() -> bool {
    windows_paste::send_ctrl_c().is_ok()
}

#[cfg(any(all(not(target_os = "macos"), not(target_os = "windows")), test))]
mod linux_selection {
    use std::io::ErrorKind;
    use std::process::Command;

    const PRIMARY_SELECTION_COMMANDS: &[(&str, &[&str])] = &[
        ("wl-paste", &["--primary", "--no-newline"]),
        ("xclip", &["-o", "-selection", "primary"]),
        ("xsel", &["--primary", "--output"]),
    ];

    #[derive(Debug, PartialEq, Eq)]
    pub enum LinuxSelectionRead {
        Text(String),
        NoSelection,
        ToolsUnavailable,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ReaderAttempt {
        Text(String),
        AvailableWithoutText,
        Unavailable,
    }

    pub fn read_selected_text() -> LinuxSelectionRead {
        read_selected_text_with(run_capture)
    }

    fn read_selected_text_with<F>(mut run: F) -> LinuxSelectionRead
    where
        F: FnMut(&str, &[&str]) -> ReaderAttempt,
    {
        let mut has_available_reader = false;
        for (bin, args) in PRIMARY_SELECTION_COMMANDS {
            match run(bin, args) {
                ReaderAttempt::Text(text) => return LinuxSelectionRead::Text(text),
                ReaderAttempt::AvailableWithoutText => has_available_reader = true,
                ReaderAttempt::Unavailable => {}
            }
        }
        if has_available_reader {
            LinuxSelectionRead::NoSelection
        } else {
            LinuxSelectionRead::ToolsUnavailable
        }
    }

    fn run_capture(bin: &str, args: &[&str]) -> ReaderAttempt {
        let output = match Command::new(bin).args(args).output() {
            Ok(output) => output,
            Err(error) if error.kind() == ErrorKind::NotFound => return ReaderAttempt::Unavailable,
            Err(_) => return ReaderAttempt::AvailableWithoutText,
        };
        if !output.status.success() {
            return ReaderAttempt::AvailableWithoutText;
        }
        let Ok(text) = String::from_utf8(output.stdout) else {
            return ReaderAttempt::AvailableWithoutText;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return ReaderAttempt::AvailableWithoutText;
        }
        ReaderAttempt::Text(trimmed.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reports_tools_unavailable_only_when_all_three_are_missing() {
            let result = read_selected_text_with(|_, _| ReaderAttempt::Unavailable);
            assert_eq!(result, LinuxSelectionRead::ToolsUnavailable);

            let mut attempts = 0;
            let result = read_selected_text_with(|_, _| {
                attempts += 1;
                if attempts == 2 {
                    ReaderAttempt::AvailableWithoutText
                } else {
                    ReaderAttempt::Unavailable
                }
            });
            assert_eq!(result, LinuxSelectionRead::NoSelection);
        }

        #[test]
        fn returns_text_from_first_reader_that_has_a_selection() {
            let result = read_selected_text_with(|bin, _| {
                if bin == "xclip" {
                    ReaderAttempt::Text("selected text".to_string())
                } else {
                    ReaderAttempt::Unavailable
                }
            });
            assert_eq!(
                result,
                LinuxSelectionRead::Text("selected text".to_string())
            );
        }
    }
}

// ─────────────────────────── macOS AX read ───────────────────────────

#[cfg(target_os = "macos")]
mod macos_ax {
    use std::ffi::{c_void, CStr};
    use std::os::raw::c_char;

    #[repr(C)]
    struct OpaqueAxRef(c_void);
    type AxUiElementRef = *mut OpaqueAxRef;
    type CFStringRef = *const c_void;
    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type AxError = i32;

    const AX_ERROR_SUCCESS: AxError = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AxError;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFStringCreateWithCString(
            allocator: CFAllocatorRef,
            cstr: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCStringPtr(s: CFStringRef, encoding: u32) -> *const c_char;
        fn CFStringGetCString(
            s: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// 调 system-wide AX 树拿 focused element，再读它的 selected text。
    /// 失败（权限缺失 / 没焦点 / 该控件不支持选区属性）时返回 None。
    pub fn read_selected_text() -> Option<String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            // 注意：这里不能直接用 CFSTR 宏（Rust 没有），改用 CFStringCreateWithCString
            // 临时构造 attribute key。
            let focused_attr =
                cfstring_from_static(b"AXFocusedUIElement\0").unwrap_or(std::ptr::null());
            let selected_attr =
                cfstring_from_static(b"AXSelectedText\0").unwrap_or(std::ptr::null());
            if focused_attr.is_null() || selected_attr.is_null() {
                if !system.is_null() {
                    CFRelease(system as CFTypeRef);
                }
                if !focused_attr.is_null() {
                    CFRelease(focused_attr);
                }
                if !selected_attr.is_null() {
                    CFRelease(selected_attr);
                }
                return None;
            }

            let mut focused: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(system, focused_attr, &mut focused);
            CFRelease(system as CFTypeRef);
            CFRelease(focused_attr);
            if err != AX_ERROR_SUCCESS || focused.is_null() {
                CFRelease(selected_attr);
                return None;
            }

            let mut selected: CFTypeRef = std::ptr::null();
            let err2 = AXUIElementCopyAttributeValue(
                focused as AxUiElementRef,
                selected_attr,
                &mut selected,
            );
            CFRelease(focused);
            CFRelease(selected_attr);
            if err2 != AX_ERROR_SUCCESS || selected.is_null() {
                return None;
            }

            let result = cfstring_to_rust(selected);
            CFRelease(selected);
            result
        }
    }

    unsafe fn cfstring_from_static(bytes_with_nul: &[u8]) -> Option<CFStringRef> {
        let cstr = CStr::from_bytes_with_nul(bytes_with_nul).ok()?;
        let s =
            CFStringCreateWithCString(std::ptr::null(), cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if s.is_null() {
            None
        } else {
            Some(s)
        }
    }

    unsafe fn cfstring_to_rust(s: CFStringRef) -> Option<String> {
        let direct = CFStringGetCStringPtr(s, K_CF_STRING_ENCODING_UTF8);
        if !direct.is_null() {
            let cstr = CStr::from_ptr(direct);
            return cstr.to_str().ok().map(|s| s.to_string());
        }
        let length = CFStringGetLength(s);
        if length <= 0 {
            return Some(String::new());
        }
        let max_bytes = CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) + 1;
        let mut buf: Vec<u8> = vec![0; max_bytes as usize];
        let ok = CFStringGetCString(
            s,
            buf.as_mut_ptr() as *mut c_char,
            max_bytes,
            K_CF_STRING_ENCODING_UTF8,
        );
        if !ok {
            return None;
        }
        let cstr = CStr::from_ptr(buf.as_ptr() as *const c_char);
        cstr.to_str().ok().map(|s| s.to_string())
    }
}

// ─────────────────────────── macOS Cmd+C post ───────────────────────────

#[cfg(target_os = "macos")]
mod macos_paste {
    use std::ffi::c_void;

    #[repr(C)]
    struct OpaqueCGEvent(c_void);
    type CGEventRef = *mut OpaqueCGEvent;

    #[repr(C)]
    struct OpaqueCGEventSource(c_void);
    type CGEventSourceRef = *mut OpaqueCGEventSource;

    type CGEventTapLocation = u32;
    type CGEventSourceStateID = i32;
    type CGKeyCode = u16;
    type CGEventFlags = u64;

    const KCG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: CGEventSourceStateID = 1;
    const KCG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;
    /// kVK_ANSI_C
    const KEY_C: CGKeyCode = 8;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: CGEventSourceStateID) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: CGKeyCode,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub fn post_cmd_c() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            let down = CGEventCreateKeyboardEvent(source, KEY_C, true);
            let up = CGEventCreateKeyboardEvent(source, KEY_C, false);
            if down.is_null() || up.is_null() {
                if !source.is_null() {
                    CFRelease(source as *const c_void);
                }
                if !down.is_null() {
                    CFRelease(down as *const c_void);
                }
                if !up.is_null() {
                    CFRelease(up as *const c_void);
                }
                return Err("CGEventCreateKeyboardEvent returned null".into());
            }
            CGEventSetFlags(down, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventSetFlags(up, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, down);
            CGEventPost(KCG_HID_EVENT_TAP, up);
            CFRelease(down as *const c_void);
            CFRelease(up as *const c_void);
            if !source.is_null() {
                CFRelease(source as *const c_void);
            }
        }
        Ok(())
    }
}

// ─────────────────────────── Windows Ctrl+C send ───────────────────────────

#[cfg(target_os = "windows")]
mod windows_paste {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_C, VK_CONTROL,
    };

    pub fn send_ctrl_c() -> Result<(), String> {
        let mut inputs = [
            keyboard_event(VK_CONTROL, false),
            keyboard_event(VK_C, false),
            keyboard_event(VK_C, true),
            keyboard_event(VK_CONTROL, true),
        ];

        let sent = unsafe { SendInput(&mut inputs, std::mem::size_of::<INPUT>() as i32) };
        if (sent as usize) != inputs.len() {
            return Err(format!("SendInput sent {sent}/{}", inputs.len()));
        }
        Ok(())
    }

    fn keyboard_event(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}

// ─────────────────────────── front-app label ───────────────────────────

/// 前台 app 的 **结构化** 标识：`(localizedName, bundleIdentifier)`。
///
/// [`current_front_app`] 那个 `"Safari (com.apple.Safari)"` 显示串是给 LLM prompt 看的，
/// 程序判定（比如 `host_document` 的 bundle 黑名单）没法用 —— 从显示串里再把 bundle
/// 抠出来既脆又蠢。所以真正的取值放在这里，显示串由它拼装。
///
/// 这也是全仓唯一一处「读前台 app」的实现：`coordinator::capsule_focus` 曾有一份近乎
/// 逐字重复的副本，现已改为调用本函数。
#[cfg(target_os = "macos")]
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let Some(cls) = AnyClass::get("NSWorkspace") else {
            return (None, None);
        };
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return (None, None);
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return (None, None);
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        (ns_string_to_rust(name_obj), ns_string_to_rust(bundle_obj))
    }
}

/// **某个进程**的 bundle id —— 不是「谁在最前面」，是「这个 pid 是谁」。
///
/// `host_document` 的安全闸门要判的是**手里这个 AX 元素属于哪个 app**。用前台 app 顶替
/// 有两个问题，后者是安全问题：
///
/// 1. 焦点元素的归属和「谁在最前面」本来就可能不一致；
/// 2. 更要命的是时间差 —— bundle 在取元素**之前**采样，而每个 AX 调用都可能阻塞到
///    `AX_MESSAGING_TIMEOUT_SECS`。用户在这中间切了 app，闸门就会拿旧 app 的身份，去
///    放行一个属于新 app 的元素。终端、密码管理器正是靠 bundle 黑名单拦的。
///
/// 拿元素自己的 pid 来问，这个窗口就不存在了。
#[cfg(target_os = "macos")]
pub(crate) fn bundle_id_for_pid(pid: i32) -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let cls = AnyClass::get("NSRunningApplication")?;
        let app: *mut AnyObject = msg_send![cls, runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return None;
        }
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        ns_string_to_rust(bundle_obj)
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };
    // Windows 上没有 bundle id 这个概念，窗口标题是我们唯一能免费拿到的标识。
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return (None, None);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return (None, None);
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return (None, None);
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        if title.is_empty() {
            (None, None)
        } else {
            (Some(title), None)
        }
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    (None, None)
}

/// 前台 app 的显示串，形如 `"Safari (com.apple.Safari)"`（Windows 上是窗口标题）。
/// 只作展示 / 进 prompt 用；要做判定请用 [`current_front_app_parts`]。
pub(crate) fn current_front_app() -> Option<String> {
    match current_front_app_parts() {
        (Some(name), Some(bundle)) => Some(format!("{name} ({bundle})")),
        (Some(name), None) => Some(name),
        (None, Some(bundle)) => Some(bundle),
        (None, None) => None,
    }
}

#[cfg(target_os = "macos")]
unsafe fn ns_string_to_rust(ns_string: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::msg_send;
    if ns_string.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = unsafe { msg_send![ns_string, UTF8String] };
    if utf8.is_null() {
        return None;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(utf8) };
    let s = cstr.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "macos")]
fn current_front_app_pid() -> Option<i32> {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;

    unsafe {
        let cls = AnyClass::get("NSWorkspace")?;
        let workspace: *mut objc2::runtime::AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut objc2::runtime::AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        (pid > 0).then_some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_passes_through() {
        let text = "hello world";
        assert_eq!(truncate_selection(text), text);
    }

    #[test]
    fn truncate_long_keeps_head_and_tail() {
        let head: String = "a".repeat(SELECTION_TRUNCATE_HEAD);
        let middle: String = "b".repeat(2_000);
        let tail: String = "c".repeat(SELECTION_TRUNCATE_TAIL);
        let combined = format!("{head}{middle}{tail}");
        let out = truncate_selection(&combined);
        assert!(out.contains("[…truncated…]"));
        assert!(out.starts_with(&"a".repeat(50)));
        assert!(out.ends_with(&"c".repeat(50)));
        // 中段 b 应被裁掉
        assert!(!out.contains(&"b".repeat(20)));
    }

    #[test]
    fn final_selection_check_requires_the_original_text() {
        assert!(selection_text_matches("original", Some("original")));
        assert!(!selection_text_matches("original", Some("different")));
        assert!(!selection_text_matches("original", None));
    }

    #[test]
    fn target_validation_error_codes_are_stable_for_the_capsule_layer() {
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

    #[cfg(target_os = "windows")]
    fn windows_target(seed: usize) -> WindowsSelectionTarget {
        WindowsSelectionTarget {
            foreground_window: seed,
            focused_window: seed + 1,
            foreground_process_id: seed as u32 + 2,
            foreground_thread_id: seed as u32 + 3,
            focused_process_id: seed as u32 + 4,
            focused_thread_id: seed as u32 + 5,
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_target_match_rejects_another_control_in_the_same_app() {
        let captured = windows_target(10);
        assert!(windows_selection_targets_match(captured, captured));

        let mut another_control = captured;
        another_control.focused_window += 100;
        assert!(!windows_selection_targets_match(
            captured,
            another_control
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_requires_a_captured_target_before_contacting_the_provider() {
        assert!(!selection_insertion_target_is_captured(
            &SelectionInsertionTarget { windows: None }
        ));
        assert!(selection_insertion_target_is_captured(
            &SelectionInsertionTarget {
                windows: Some(windows_target(10)),
            }
        ));
    }
}
