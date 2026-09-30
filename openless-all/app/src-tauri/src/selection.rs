//! Cross-platform "selection capture" utilities: on a QA shortcut press, try to obtain the
//! selected text of the current foreground app.
//!
//! 平台路径：
//! 1. **macOS** AX：`AXUIElementCopyAttributeValue(focused, kAXSelectedTextAttribute)`
//!    走辅助功能 API 直读焦点元素的选区，**不**触碰剪贴板。
//! 2. **macOS / Windows** Cmd+C / Ctrl+C：snapshot 用户原剪贴板 → 模拟复制 → 80ms
//!    后读出新内容 → 还原原剪贴板。
//!
//! Truncation policy: selections over 4000 chars keep head 2000 + tail 2000 + the
//! `[…truncated…]` marker, avoiding an overly long LLM context.
//!
//! 模块依赖：`arboard`（跨平台剪贴板）+ libc + 平台 native 框架。
//! Linux 桌面已改由 egui 前端（`openless-all/app/linux-egui`）承担，Tauri 版不再提供
//! Linux 的选区读取路径。

// 仅 macOS / Windows 的模拟复制路径用 sleep。
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::time::Duration;

const SELECTION_MAX_CHARS: usize = 4000;
const SELECTION_TRUNCATE_HEAD: usize = 2000;
const SELECTION_TRUNCATE_TAIL: usize = 2000;
const SELECTION_TRUNCATED_MARKER: &str = "\n[…truncated…]\n";

/// Selection context read from the foreground app.
/// `text` is already truncated; `source_app` is a human-readable label for the foreground app
/// (optional).
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
    /// Foreground app at capture time (NSWorkspace frontmostApplication, `name (bundle)` form).
    front_app: Option<String>,
    /// Foreground app pid at capture time — used to return focus to the original app after the
    /// preview is confirmed.
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

pub struct SelectionCaptureOutcome {
    pub selection: Option<SelectionContext>,
}

#[derive(Debug, Clone)]
struct PrefetchedSelectionWorkspace {
    selection: SelectionContext,
    insertion_target: SelectionInsertionTarget,
}

/// Why a selection capture failed / was skipped. Written to the user-exportable openless.log to
/// distinguish "no selection at all" vs "simulated copy did not overwrite the clipboard" vs
/// "clipboard API failure".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionCaptureMissReason {
    Ok,
    PrefetchHit,
    PrefetchMissLiveOk,
    ClipboardInitFailed,
    ClipboardSentinelWriteFailed,
    CopyShortcutFailed,
    ClipboardUnchangedSentinel,
    ClipboardEmptyAfterCopy,
    ClipboardReadFailed,
    EmptyTrimmed,
    NoCapturePath,
}

impl SelectionCaptureMissReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::PrefetchHit => "prefetch_hit",
            Self::PrefetchMissLiveOk => "prefetch_miss_live_ok",
            Self::ClipboardInitFailed => "clipboard_init_failed",
            Self::ClipboardSentinelWriteFailed => "clipboard_sentinel_write_failed",
            Self::CopyShortcutFailed => "copy_shortcut_failed",
            Self::ClipboardUnchangedSentinel => "clipboard_unchanged_sentinel",
            Self::ClipboardEmptyAfterCopy => "clipboard_empty_after_copy",
            Self::ClipboardReadFailed => "clipboard_read_failed",
            Self::EmptyTrimmed => "empty_trimmed",
            Self::NoCapturePath => "no_capture_path",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SelectionCaptureDiag {
    pub reason: SelectionCaptureMissReason,
    pub front_app: Option<String>,
    pub used_prefetch: bool,
    pub target_captured: bool,
    pub char_count: Option<usize>,
}

impl SelectionCaptureDiag {
    pub(crate) fn summary(&self) -> String {
        format!(
            "reason={} used_prefetch={} target_captured={} chars={} front_app={}",
            self.reason.as_str(),
            self.used_prefetch,
            self.target_captured,
            self.char_count
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            self.front_app.as_deref().unwrap_or("-")
        )
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
static PREFETCHED_SELECTION_WORKSPACE: std::sync::Mutex<Option<PrefetchedSelectionWorkspace>> =
    std::sync::Mutex::new(None);

/// Snapshot the selection early, on the modifier hotkey edge, before the target app can lose its
/// selection as a side effect (e.g. an Alt menu opening).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn prefetch_selection_workspace_capture() {
    let insertion_target = capture_selection_insertion_target();
    let (capture, reason) = capture_selection_with_status_diag();
    let mut guard = PREFETCHED_SELECTION_WORKSPACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match capture.selection {
        Some(selection) => {
            let chars = selection.text.chars().count();
            log::info!(
                "[selection] prefetched workspace selection ({} chars) reason={} front_app={}",
                chars,
                reason.as_str(),
                selection.source_app.as_deref().unwrap_or("-")
            );
            *guard = Some(PrefetchedSelectionWorkspace {
                selection,
                insertion_target,
            });
        }
        None => {
            log::info!(
                "[selection] prefetch missed (no selection at hotkey edge) reason={} front_app={} target_captured={}",
                reason.as_str(),
                capture_selection_source_app_hint().as_deref().unwrap_or("-"),
                selection_insertion_target_is_captured(&insertion_target)
            );
            guard.take();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn take_prefetched_selection_workspace(
) -> Option<(SelectionContext, SelectionInsertionTarget)> {
    PREFETCHED_SELECTION_WORKSPACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .map(|prefetched| (prefetched.selection, prefetched.insertion_target))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn prefetch_selection_workspace_capture() {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn take_prefetched_selection_workspace(
) -> Option<(SelectionContext, SelectionInsertionTarget)> {
    None
}

/// Consume the selection prefetched at the hotkey edge first; fall back to live capture if none.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn resolve_selection_workspace_capture(
) -> (Option<SelectionContext>, SelectionInsertionTarget) {
    let (selection, target, _) = resolve_selection_workspace_capture_with_diag();
    (selection, target)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn resolve_selection_workspace_capture_with_diag() -> (
    Option<SelectionContext>,
    SelectionInsertionTarget,
    SelectionCaptureDiag,
) {
    if let Some((selection, insertion_target)) = take_prefetched_selection_workspace() {
        let char_count = selection.text.chars().count();
        let front_app = selection.source_app.clone();
        let target_captured = selection_insertion_target_is_captured(&insertion_target);
        let diag = SelectionCaptureDiag {
            reason: SelectionCaptureMissReason::PrefetchHit,
            front_app,
            used_prefetch: true,
            target_captured,
            char_count: Some(char_count),
        };
        log::info!("[selection] resolve used prefetch {}", diag.summary());
        return (Some(selection), insertion_target, diag);
    }
    let insertion_target = capture_selection_insertion_target();
    let (capture, reason) = capture_selection_with_status_diag();
    let target_captured = selection_insertion_target_is_captured(&insertion_target);
    let (front_app, char_count) = match &capture.selection {
        Some(selection) => (
            selection.source_app.clone(),
            Some(selection.text.chars().count()),
        ),
        None => (capture_selection_source_app_hint(), None),
    };
    let reason = if capture.selection.is_some() && reason == SelectionCaptureMissReason::Ok {
        SelectionCaptureMissReason::PrefetchMissLiveOk
    } else {
        reason
    };
    let diag = SelectionCaptureDiag {
        reason,
        front_app,
        used_prefetch: false,
        target_captured,
        char_count,
    };
    log::info!("[selection] resolve live capture {}", diag.summary());
    (capture.selection, insertion_target, diag)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn resolve_selection_workspace_capture(
) -> (Option<SelectionContext>, SelectionInsertionTarget) {
    let (selection, target, _) = resolve_selection_workspace_capture_with_diag();
    (selection, target)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn resolve_selection_workspace_capture_with_diag() -> (
    Option<SelectionContext>,
    SelectionInsertionTarget,
    SelectionCaptureDiag,
) {
    let insertion_target = capture_selection_insertion_target();
    let (capture, reason) = capture_selection_with_status_diag();
    let target_captured = selection_insertion_target_is_captured(&insertion_target);
    let (front_app, char_count) = match &capture.selection {
        Some(selection) => (
            selection.source_app.clone(),
            Some(selection.text.chars().count()),
        ),
        None => (capture_selection_source_app_hint(), None),
    };
    let diag = SelectionCaptureDiag {
        reason,
        front_app,
        used_prefetch: false,
        target_captured,
        char_count,
    };
    log::info!("[selection] resolve live capture {}", diag.summary());
    (capture.selection, insertion_target, diag)
}

fn capture_selection_source_app_hint() -> Option<String> {
    current_front_app()
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
/// 移动端不提供选区润色；只有 macOS / Windows 具有对应的前台校验。
pub(crate) fn selection_insertion_target_is_captured(target: &SelectionInsertionTarget) -> bool {
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
        // 移动端不提供选区润色；此处保留非桌面平台的类型兜底。
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
        // Foreground app consistency: the user switching apps while the cloud request is pending
        // = target change; reject the paste (preview-confirm mode has already reactivated the
        // original app before validate, so this should match).
        let front_now = current_front_app();
        if captured
            .front_app
            .as_deref()
            .is_some_and(|name| front_now.as_deref() != Some(name))
        {
            return SelectionInsertionTargetValidation::TargetChanged;
        }
        // Selection text consistency: AX direct read (same path as capture), falling back to
        // simulated Cmd+C.
        let current_selection = read_selection_for_validation();
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }
        return SelectionInsertionTargetValidation::Valid;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        SelectionInsertionTargetValidation::TargetUnavailable
    }
}

/// macOS only: re-read the current selection in the same form as at capture time (trim +
/// truncate) for validate to compare against expected_selection. Degrades to simulated
/// Cmd+C + clipboard snapshot when AX is unauthorized or the direct read fails (same fallback as
/// `capture_selection_with_status`).
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

/// Return focus to the original selection target after the preview is confirmed. The preview
/// window allows editing, so at confirm time it is necessarily no longer the original app's
/// foreground window; restore the original target first, then run the strict selection validation
/// above, avoiding a blind paste.
pub(crate) fn reactivate_selection_insertion_target(target: &SelectionInsertionTarget) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };

        let Some(captured) = target.windows else {
            return false;
        };
        unsafe {
            let foreground = HWND(captured.foreground_window as *mut _);
            if IsIconic(foreground).as_bool() {
                let _ = ShowWindow(foreground, SW_RESTORE);
            }
            let _ = BringWindowToTop(foreground);
            let _ = SetForegroundWindow(foreground);
        }
        std::thread::sleep(Duration::from_millis(80));
        // Windows' focus-stealing rules may reject SetForegroundWindow. Recovery counts only if
        // the exact same window/control fingerprint is re-captured; do not blindly paste into the
        // current app just because the activation call returned.
        return capture_windows_selection_target().as_ref() == Some(&captured);
    }

    #[cfg(target_os = "macos")]
    {
        let Some(captured) = target.macos.as_ref() else {
            return false;
        };
        let Some(pid) = captured.front_app_pid else {
            return false;
        };
        // Streaming writes usually already own the foreground application.
        // Do not reactivate it and sleep for 80ms on every delta; the caller
        // still validates the captured text control before posting keys.
        if current_front_app_pid() == Some(pid) {
            return true;
        }
        // The preview window is OpenLess's own window; after confirm, focus must go back to the
        // original app before pasting. NSRunningApplication activate is best-effort, and some
        // apps (Electron, custom-drawn windows) need >120ms to restore their key window — a
        // single fixed sleep followed by one pid check can misread "still restoring" as
        // "restore failed". Use short polling instead: return as soon as the pid is stable, at
        // most ~320ms.
        for _attempt in 0..4 {
            // Re-activate each round: NSRunningApplication activate may not take effect when the
            // foreground was stolen by another app; repeated calls are idempotent.
            activate_app_by_pid(pid);
            std::thread::sleep(Duration::from_millis(80));
            if current_front_app_pid() == Some(pid) {
                return true;
            }
        }
        // Still not foreground: fail explicitly; never write blindly into whatever app happens to
        // hold focus.
        false
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = target;
        true
    }
}

/// macOS only: bring the app with the given pid back to the foreground (NSRunningApplication
/// activate, NSApplicationActivateIgnoringOtherApps = 1). Silent on failure — validate still
/// falls back to selection-text consistency.
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
        let _: bool = msg_send![app, activateWithOptions: 1u64]; // IgnoringOtherApps
    }
}

/// macOS only: the last line of defense right before pasting. `validate_selection_insertion_target`'s
/// simulate_copy fallback retries for up to 200ms, during which foreground focus may jump to
/// another window or app (and text comparison alone would let it through if that target happens
/// to expose the same selection text). Immediately before `insert()`, re-read the foreground
/// app's pid+name and compare with capture time; any change rejects the paste — a failed
/// replacement is acceptable, writing into the wrong target is not.
#[cfg(target_os = "macos")]
pub(crate) fn selection_target_still_front(target: &SelectionInsertionTarget) -> bool {
    let Some(captured) = target.macos.as_ref() else {
        return false;
    };
    let Some(pid) = captured.front_app_pid else {
        return false;
    };
    if current_front_app_pid() != Some(pid) {
        return false;
    }
    if let Some(name) = captured.front_app.as_deref() {
        if current_front_app().as_deref() != Some(name) {
            return false;
        }
    }
    true
}

/// 捕获当前前台应用的选区；失败统一视为无选区。
pub fn capture_selection_with_status() -> SelectionCaptureOutcome {
    capture_selection_with_status_diag().0
}

fn capture_selection_with_status_diag() -> (SelectionCaptureOutcome, SelectionCaptureMissReason) {
    let source_app = current_front_app();

    // 1. macOS AX direct read
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
            return (
                SelectionCaptureOutcome {
                    selection: Some(SelectionContext {
                        text: truncate_selection(trimmed),
                        source_app,
                    }),
                },
                SelectionCaptureMissReason::Ok,
            );
        }
        log::info!(
            "[selection] AX read returned empty/whitespace{}",
            source_app
                .as_deref()
                .map(|a| format!(" front_app={a}"))
                .unwrap_or_default()
        );
    }

    // 2. Simulated-copy fallback (macOS / Windows)
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        match simulate_copy_and_read_diag() {
            Ok(text) => {
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
                    return (
                        SelectionCaptureOutcome {
                            selection: Some(SelectionContext {
                                text: truncate_selection(trimmed),
                                source_app,
                            }),
                        },
                        SelectionCaptureMissReason::Ok,
                    );
                }
                log::info!(
                    "[selection] simulate-copy returned whitespace-only{}",
                    source_app
                        .as_deref()
                        .map(|a| format!(" front_app={a}"))
                        .unwrap_or_default()
                );
                return (
                    SelectionCaptureOutcome { selection: None },
                    SelectionCaptureMissReason::EmptyTrimmed,
                );
            }
            Err(reason) => {
                log::info!(
                    "[selection] simulate-copy miss reason={}{}",
                    reason.as_str(),
                    source_app
                        .as_deref()
                        .map(|a| format!(" front_app={a}"))
                        .unwrap_or_default()
                );
                return (SelectionCaptureOutcome { selection: None }, reason);
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    (
        SelectionCaptureOutcome { selection: None },
        SelectionCaptureMissReason::NoCapturePath,
    )
}

/// Truncate lengthwise to head + tail + marker.
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

// ─────────────────────────── Simulated-copy fallback (mac/win) ───────────────────────────

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn simulate_copy_and_read() -> Option<String> {
    simulate_copy_and_read_diag().ok()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn simulate_copy_and_read_diag() -> Result<String, SelectionCaptureMissReason> {
    // a) Snapshot the current clipboard (backup used to restore the original state)
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[selection] clipboard init failed: {e}");
            return Err(SelectionCaptureMissReason::ClipboardInitFailed);
        }
    };
    let original = match clipboard.get_text() {
        Ok(t) => Some(t),
        Err(e) => {
            log::info!("[selection] clipboard get_text returned err (likely empty): {e}");
            None
        }
    };

    // b) Write a sentinel into the clipboard — used afterwards to check whether the simulated
    //    copy actually overwrote it (if it is still the sentinel, Cmd+C did not take effect or
    //    the target app has no selection).
    let sentinel = format!("__openless_qa_sentinel_{}__", uuid_like_token());
    if let Err(e) = clipboard.set_text(sentinel.clone()) {
        log::warn!("[selection] clipboard set_text(sentinel) failed: {e}");
        return Err(SelectionCaptureMissReason::ClipboardSentinelWriteFailed);
    }

    // Windows + Clipboard History: writing the sentinel wakes an async listener that may
    // briefly own/open the clipboard. Target-app Ctrl+C then fails silently and leaves
    // the sentinel in place (user A/B: history OFF → OK, history ON → fail). Settle first.
    #[cfg(target_os = "windows")]
    windows_wait_clipboard_settle_after_write();

    // c) Simulate Cmd+C / Ctrl+C
    let mut post_ok = post_copy_shortcut();
    log::info!(
        "[selection] post_copy: post_ok={} original_was_some={}",
        post_ok,
        original.is_some()
    );
    if !post_ok {
        log::warn!("[selection] post_copy_shortcut failed");
        // Do not return immediately: the clipboard may already have been clobbered by some path;
        // restore it via the flow below.
    }

    // d) Wait for the clipboard to update; Windows with clipboard history needs polling and,
    //    if necessary, re-sending Ctrl+C.
    #[cfg(not(target_os = "windows"))]
    {
        std::thread::sleep(Duration::from_millis(80));
    }

    #[cfg(target_os = "windows")]
    let captured = windows_poll_clipboard_after_copy(&mut clipboard, &sentinel, &mut post_ok);
    #[cfg(not(target_os = "windows"))]
    let captured = clipboard.get_text().ok();

    // f) Restore the original clipboard
    if let Some(ref prev) = original {
        if let Err(e) = clipboard.set_text(prev) {
            log::warn!("[selection] clipboard restore failed: {e}");
        }
    } else {
        // The user's original clipboard was empty → clear the sentinel / selection to avoid
        // polluting it.
        if let Err(e) = clipboard.set_text("") {
            log::warn!("[selection] clipboard clear failed: {e}");
        }
    }

    classify_simulated_copy_result(post_ok, captured, &sentinel)
}

fn classify_simulated_copy_result(
    post_ok: bool,
    captured: Option<String>,
    sentinel: &str,
) -> Result<String, SelectionCaptureMissReason> {
    if !post_ok {
        return Err(SelectionCaptureMissReason::CopyShortcutFailed);
    }
    let captured = captured.ok_or(SelectionCaptureMissReason::ClipboardReadFailed)?;
    if captured == sentinel {
        return Err(SelectionCaptureMissReason::ClipboardUnchangedSentinel);
    }
    if captured.is_empty() {
        return Err(SelectionCaptureMissReason::ClipboardEmptyAfterCopy);
    }
    Ok(captured)
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

#[cfg(any(target_os = "windows", target_os = "macos", test))]
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
        ) -> u8;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// Query the system-wide AX tree for the focused element, then read its selected text.
    /// Returns None on failure (missing permission / no focus / control lacks the selection
    /// attribute).
    pub fn read_selected_text() -> Option<String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            // Note: the CFSTR macro is not available in Rust, so build the attribute key
            // temporarily with CFStringCreateWithCString.
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
        // CoreFoundation's Boolean is an unsigned byte in the SDK ABI.
        if ok == 0 {
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

/// After we write a sentinel, Clipboard History (and other listeners) may briefly open
/// the clipboard. Wait until nobody holds it and the sequence number stops moving.
#[cfg(target_os = "windows")]
fn windows_wait_clipboard_settle_after_write() {
    use windows::Win32::System::DataExchange::{
        GetClipboardSequenceNumber, GetOpenClipboardWindow,
    };

    const MAX_MS: u64 = 250;
    const STEP_MS: u64 = 10;
    let start = std::time::Instant::now();
    let mut last_seq = unsafe { GetClipboardSequenceNumber() };
    let mut stable_rounds = 0u32;
    let mut open_cleared = false;
    let mut seq_stable = false;

    while (start.elapsed().as_millis() as u64) < MAX_MS {
        let open_is_free = match unsafe { GetOpenClipboardWindow() } {
            Ok(hwnd) => hwnd.0.is_null(),
            Err(_) => true,
        };
        let seq = unsafe { GetClipboardSequenceNumber() };
        if open_is_free {
            open_cleared = true;
            if seq == last_seq {
                stable_rounds += 1;
                // ~30ms of unchanged seq with clipboard free → history likely caught up.
                if stable_rounds >= 3 {
                    seq_stable = true;
                    break;
                }
            } else {
                stable_rounds = 0;
                last_seq = seq;
            }
        } else {
            open_cleared = false;
            stable_rounds = 0;
            last_seq = seq;
        }
        std::thread::sleep(Duration::from_millis(STEP_MS));
    }

    let waited_ms = start.elapsed().as_millis() as u64;
    log::info!(
        "[selection] clipboard_settle: waited_ms={} open_cleared={} seq_stable={}",
        waited_ms,
        open_cleared,
        seq_stable
    );
}

/// Poll until clipboard leaves the sentinel (selection copy landed). One Ctrl+C resend
/// mid-window covers the case where the first chord hit a still-busy clipboard.
#[cfg(target_os = "windows")]
fn windows_poll_clipboard_after_copy(
    clipboard: &mut arboard::Clipboard,
    sentinel: &str,
    post_ok: &mut bool,
) -> Option<String> {
    const MAX_MS: u64 = 450;
    const STEP_MS: u64 = 25;
    let start = std::time::Instant::now();
    let mut attempts = 0u32;
    let mut resent_ctrl_c = false;
    let mut captured = None;

    while (start.elapsed().as_millis() as u64) < MAX_MS {
        attempts += 1;
        match clipboard.get_text() {
            Ok(text) if text != sentinel => {
                captured = Some(text);
                break;
            }
            Ok(text) => {
                captured = Some(text);
            }
            Err(_) => {
                captured = None;
            }
        }

        // Halfway through: if still on sentinel, resend Ctrl+C once.
        if !resent_ctrl_c && (start.elapsed().as_millis() as u64) >= 150 {
            if post_copy_shortcut() {
                *post_ok = true;
                resent_ctrl_c = true;
                log::info!("[selection] clipboard_poll: resent Ctrl+C after settle miss");
            }
        }
        std::thread::sleep(Duration::from_millis(STEP_MS));
    }

    let waited_ms = start.elapsed().as_millis() as u64;
    log::info!(
        "[selection] clipboard_poll: waited_ms={} attempts={} resent={} still_sentinel={}",
        waited_ms,
        attempts,
        resent_ctrl_c,
        captured.as_deref() == Some(sentinel)
    );
    captured
}

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

/// The foreground app's **structured** identity: `(localizedName, bundleIdentifier)`.
///
/// [`current_front_app`]'s `"Safari (com.apple.Safari)"` display string is for the LLM prompt;
/// programmatic decisions (e.g. `host_document`'s bundle denylist) cannot use it — re-extracting
/// the bundle from the display string is fragile and ugly. The real value lives here, and the
/// display string is assembled from it.
///
/// This is also the only place in the repo that "reads the front app": `coordinator::capsule_focus`
/// used to carry a near-verbatim duplicate and now calls this function.
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

/// The bundle id of **one process** — not "who is frontmost" but "who is this pid".
///
/// `host_document`'s security gate must decide **which app owns this AX element in hand**.
/// Substituting the front app has two problems, the second being a security issue:
///
/// 1. The focused element's owner and "who is frontmost" may legitimately differ;
/// 2. More importantly, the timing gap — the bundle is sampled **before** fetching the element,
///    and every AX call can block up to `AX_MESSAGING_TIMEOUT_SECS`. If the user switches apps in
///    between, the gate would use the old app's identity to admit an element belonging to the new
///    app. Terminals and password managers are exactly what the bundle denylist blocks.
///
/// Asking via the element's own pid removes this window entirely.
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
    // Windows has no bundle id concept; the window title is the only identity we get for free.
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

/// The foreground app's display string, like `"Safari (com.apple.Safari)"` (the window title on
/// Windows). For display / prompt use only; for decisions use [`current_front_app_parts`].
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
        // The middle "b" run must be cut
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

    #[test]
    fn simulated_copy_result_classifies_failures_and_multibyte_text() {
        let sentinel = "__openless_test_sentinel__";
        for (post_ok, captured, expected) in [
            (
                false,
                Some("stale clipboard".to_string()),
                Err(SelectionCaptureMissReason::CopyShortcutFailed),
            ),
            (
                true,
                None,
                Err(SelectionCaptureMissReason::ClipboardReadFailed),
            ),
            (
                true,
                Some(sentinel.to_string()),
                Err(SelectionCaptureMissReason::ClipboardUnchangedSentinel),
            ),
            (
                true,
                Some(String::new()),
                Err(SelectionCaptureMissReason::ClipboardEmptyAfterCopy),
            ),
            (
                true,
                Some("selected".to_string()),
                Ok("selected".to_string()),
            ),
            (
                true,
                Some("中文选区".to_string()),
                Ok("中文选区".to_string()),
            ),
        ] {
            assert_eq!(
                classify_simulated_copy_result(post_ok, captured, sentinel),
                expected
            );
        }
        assert_eq!(
            SelectionCaptureMissReason::ClipboardSentinelWriteFailed.as_str(),
            "clipboard_sentinel_write_failed"
        );
    }

    #[test]
    fn capture_diag_summary_contains_metadata_only() {
        let selected_text = "不得写入日志的中文选区";
        let summary = SelectionCaptureDiag {
            reason: SelectionCaptureMissReason::PrefetchHit,
            front_app: Some("Editor".to_string()),
            used_prefetch: true,
            target_captured: true,
            char_count: Some(selected_text.chars().count()),
        }
        .summary();

        assert_eq!(
            summary,
            "reason=prefetch_hit used_prefetch=true target_captured=true chars=11 front_app=Editor"
        );
        assert!(!summary.contains(selected_text));
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
        assert!(!windows_selection_targets_match(captured, another_control));
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
