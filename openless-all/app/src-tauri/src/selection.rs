//! Cross-platform "selection capture" utilities: on a QA shortcut press, try to obtain the
//! selected text of the current foreground app.
//!
//! 平台路径：
//! 1. **macOS** AX：`AXUIElementCopyAttributeValue(focused, kAXSelectedTextAttribute)`
//!    走辅助功能 API 直读焦点元素的选区，**不**触碰剪贴板。
//! 2. **macOS / Windows** Cmd+C / Ctrl+C：snapshot 用户原剪贴板 → 模拟复制 → 80ms
//!    后读出新内容 → 还原原剪贴板。
//!
//! Explicit selections are captured in full, including boundary whitespace.
//! Prompt budgets for unrelated QA/cursor context remain at their own call sites.
//!
//! 模块依赖：`arboard`（跨平台剪贴板）+ libc + 平台 native 框架。
//! Linux 桌面已改由 egui 前端（`openless-all/app/linux-egui`）承担，Tauri 版不再提供
//! Linux 的选区读取路径。

// 仅 macOS / Windows 的模拟复制路径用 sleep。
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::time::Duration;

#[cfg(any(target_os = "windows", test))]
#[path = "selection/clipboard_capture.rs"]
mod clipboard_capture;
#[path = "selection/target_position.rs"]
mod target_position;
#[cfg(target_os = "windows")]
use clipboard_capture::ClipboardPollOutcome;

/// Selection context read from the foreground app.
/// `text` is the complete selected range, including boundary whitespace.
/// `source_app` is a human-readable label for the foreground app (optional).
#[derive(Debug, Clone)]
pub struct SelectionContext {
    pub text: String,
    pub source_app: Option<String>,
    /// Legacy capture metadata. Full-range capture always sets this to false.
    pub omits_middle: bool,
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
/// macOS retains the AX control identity and native window ID. Replacement
/// validates the same control and re-reads the complete selection using native
/// APIs without borrowing the clipboard during asynchronous validation.
#[derive(Debug, Clone, Default)]
pub(crate) struct SelectionInsertionTarget {
    pub(crate) cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    strict_input_target: bool,
    dictation_selection: Option<String>,
    dictation_position: std::sync::Arc<parking_lot::Mutex<Option<(u64, u64)>>>,
    pub(crate) original_clipboard: std::sync::Arc<parking_lot::Mutex<Option<u64>>>,
    #[cfg(target_os = "windows")]
    dictation_control: Option<Vec<i32>>,
    #[cfg(target_os = "windows")]
    windows: Option<WindowsSelectionTarget>,
    #[cfg(target_os = "windows")]
    windows_app: Option<openless_core::app_style_routing::InputApp>,
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
    name: Option<String>,
    /// Bundle identifier captured with the foreground app, used by the host-document safety gate.
    bundle_id: Option<String>,
    /// Foreground app pid at capture time — used to return focus to the original app after the
    /// preview is confirmed.
    front_app_pid: Option<i32>,
    dictation_window: Option<u32>,
    dictation_element: Option<std::sync::Arc<macos_ax::FocusedElement>>,
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
    #[cfg(any(target_os = "macos", test))]
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
            #[cfg(any(target_os = "macos", test))]
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
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

        let foreground = unsafe { GetForegroundWindow() };
        let windows = capture_windows_selection_target_for_window(foreground);
        let windows_app = windows.map_or_else(
            || windows_app_for_window(foreground),
            |target| crate::activity_app_icons::app_for_pid(target.focused_process_id),
        );
        return SelectionInsertionTarget {
            windows,
            windows_app,
            dictation_control: crate::host_document::focused_input_identity(),
            dictation_position: std::sync::Arc::new(parking_lot::Mutex::new(crate::host_document::focused_input_position())),
            ..Default::default()
        };
    }

    #[cfg(target_os = "macos")]
    {
        let (front_app_name, bundle_id, front_app_pid) = current_front_app_snapshot();
        let front_app = match (front_app_name.as_deref(), bundle_id.as_deref()) {
            (Some(name), Some(bundle)) => Some(format!("{name} ({bundle})")),
            (Some(name), None) => Some(name.to_string()),
            (None, Some(bundle)) => Some(bundle.to_string()),
            (None, None) => None,
        };
        return SelectionInsertionTarget {
            dictation_position: std::sync::Arc::new(parking_lot::Mutex::new(crate::host_document::focused_input_position())),
            macos: Some(MacosSelectionTarget {
                front_app,
                name: front_app_name,
                bundle_id,
                front_app_pid,
                dictation_window: front_app_pid.and_then(macos_front_window_id),
                dictation_element: macos_ax::capture_focused_element(),
            }),
            ..Default::default()
        };
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        SelectionInsertionTarget::default()
    }
}

pub(crate) fn capture_dictation_insertion_target() -> SelectionInsertionTarget {
    let mut target = capture_selection_insertion_target();
    target.strict_input_target = true;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        *target.dictation_position.lock() = crate::host_document::focused_input_position();
        if target
            .dictation_position
            .lock()
            .is_some_and(|(_, length)| length > 0)
        {
            #[cfg(target_os = "macos")]
            {
                target.dictation_selection = read_selection_for_validation();
            }
            #[cfg(target_os = "windows")]
            {
                target.dictation_selection = crate::host_document::read_qa_selection();
            }
        }
        let mut last = LAST_DICTATION_TARGET.lock();
        if let Some(previous) = last.as_ref() {
            if selection_insertion_target_is_current(previous)
                && same_native_target(previous, &target)
            {
                target.dictation_position = std::sync::Arc::clone(&previous.dictation_position);
            }
        }
        *last = Some(target.clone());
    }
    target
}

fn same_native_target(left: &SelectionInsertionTarget, right: &SelectionInsertionTarget) -> bool {
    #[cfg(target_os = "macos")]
    {
        left.macos
            .as_ref()
            .zip(right.macos.as_ref())
            .is_some_and(|(left, right)| {
                left.front_app_pid == right.front_app_pid
                    && left.dictation_window == right.dictation_window
                    && left
                        .dictation_element
                        .as_ref()
                        .zip(right.dictation_element.as_ref())
                        .is_some_and(|(left, right)| left.same_as(right))
            })
    }
    #[cfg(target_os = "windows")]
    {
        left.windows == right.windows
            && left.dictation_control.is_some()
            && left.dictation_control == right.dictation_control
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (left, right);
        false
    }
}

// Share only a still-current native anchor. Starting a recording is not a focus change.
static LAST_DICTATION_TARGET: parking_lot::Mutex<Option<SelectionInsertionTarget>> =
    parking_lot::Mutex::new(None);

pub(crate) fn dictation_target_requires_position(target: &SelectionInsertionTarget) -> bool {
    target.strict_input_target
}

/// Advance shared pending anchors only after confirmed delivery at the expected new caret.
pub(crate) fn confirm_target_delivery(target: &SelectionInsertionTarget, text: &str) -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if target.strict_input_target {
        if !native_input_target_is_current(target) {
            return false;
        }
        let mut position = target.dictation_position.lock();
        if let Some(advanced) = target_position::advanced(
            *position,
            text.encode_utf16().count() as u64,
            crate::host_document::focused_input_position(),
        ) {
            *position = Some(advanced);
            return true;
        }
        return false;
    }
    true
}

pub(crate) fn protect_selection_original(
    target: &SelectionInsertionTarget,
    text: &str,
) -> Result<(), String> {
    if target.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("selection session cancelled".into());
    }
    let revision = crate::insertion::copy_text_to_clipboard_guarded(text, || {
        !target.cancelled.load(std::sync::atomic::Ordering::Acquire)
    })?;
    *target.original_clipboard.lock() = revision;
    if target.original_clipboard.lock().is_none() {
        return Err("selectionOriginalCopyFailed".into());
    }
    Ok(())
}

pub(crate) fn selection_result_fallback(
    target: &SelectionInsertionTarget,
    text: &str,
) -> Result<openless_core::InsertOutcome, openless_core::BackendError> {
    if target.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(openless_core::BackendError::new(
            openless_core::BackendErrorCode::Cancelled,
            "selection session cancelled",
        ));
    }
    // A user copy after processing began owns the clipboard, even if its text is identical.
    let expected = *target.original_clipboard.lock();
    if crate::insertion::copy_text_to_clipboard_if_current(text, expected, || {
        !target.cancelled.load(std::sync::atomic::Ordering::Acquire)
    })
    .map_err(|error| {
        openless_core::BackendError::new(openless_core::BackendErrorCode::Platform, error)
    })? {
        Ok(openless_core::InsertOutcome::CopiedFallback)
    } else {
        Err(openless_core::BackendError::new(
            openless_core::BackendErrorCode::InvalidState,
            "selectionResultInHistory",
        )
        .target_unavailable())
    }
}

#[path = "selection/recovery_notice.rs"]
mod recovery_notice;
pub(crate) use recovery_notice::original_recovery_message;

/// Final native handoff. Clipboard restore is mandatory for selection workflows.
pub(crate) fn insert_selection_result(
    target: &SelectionInsertionTarget,
    source_text: &str,
    text: &str,
    shortcut: crate::types::PasteShortcut,
) -> Result<openless_core::InsertOutcome, openless_core::BackendError> {
    if !selection_insertion_target_is_current(target) {
        return selection_result_fallback(target, text);
    }
    #[cfg(target_os = "macos")]
    let mut delivery = crate::host_document::KeyboardDelivery::capture();
    if !selection_insertion_target_is_current(target) {
        return selection_result_fallback(target, text);
    }
    let expected_clipboard = *target.original_clipboard.lock();
    if !crate::insertion::clipboard_revision_matches(
        expected_clipboard,
        crate::insertion::clipboard_sequence(),
    ) {
        return Err(openless_core::BackendError::new(
            openless_core::BackendErrorCode::InvalidState,
            "selectionResultInHistory",
        ));
    }
    let status = crate::insertion::TextInserter::new().insert_guarded(
        text,
        true,
        shortcut,
        expected_clipboard,
        || {
            selection_insertion_target_is_current(target)
                && (source_text.is_empty()
                    || validate_selection_insertion_target(target, source_text)
                        .error_code()
                        .is_none())
        },
    );
    match status {
        crate::types::InsertStatus::Inserted | crate::types::InsertStatus::PasteSent => {
            #[cfg(target_os = "macos")]
            if let Some(mut delivery) = delivery.take() {
                delivery.record_posted(text, crate::types::MacosNewlineMode::ShiftReturn);
                if matches!(
                    delivery.finish(),
                    crate::host_document::KeyboardDeliveryOutcome::Delivered
                ) {
                    return Ok(openless_core::InsertOutcome::Inserted);
                }
            }
            Ok(openless_core::InsertOutcome::PasteSent)
        }
        crate::types::InsertStatus::NotRequested => Err(
            openless_core::BackendError::new(
                openless_core::BackendErrorCode::InvalidState,
                "selectionResultInHistory",
            )
            .target_unavailable(),
        ),
        crate::types::InsertStatus::CopiedFallback => {
            Ok(openless_core::InsertOutcome::CopiedFallback)
        }
        _ => Err(openless_core::BackendError::new(
            openless_core::BackendErrorCode::Platform,
            "selectionResultInHistory",
        )),
    }
}

#[cfg(target_os = "macos")]
fn macos_front_window_id(pid: i32) -> Option<u32> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::*;

    // The WindowServer returns its front-to-back order without Screen Recording
    // permission. Use only numeric metadata; no titles or images are read.
    let windows = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let owner = CFString::from_static_string("kCGWindowOwnerPID");
    let layer = CFString::from_static_string("kCGWindowLayer");
    let number = CFString::from_static_string("kCGWindowNumber");
    for window in windows.iter() {
        // SAFETY: CGWindowListCopyWindowInfo returns an array of CFDictionaries.
        let window = unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule((*window).cast()) };
        let value = |key: &CFString| window.find(key)?.downcast::<CFNumber>()?.to_i32();
        if value(&owner) == Some(pid) && value(&layer) == Some(0) {
            return u32::try_from(value(&number)?).ok().filter(|id| *id != 0);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_target_is_current(target: &MacosSelectionTarget, pid: i32) -> bool {
    current_front_app_pid() == Some(pid)
        && target.dictation_window.is_some_and(|window| {
            macos_front_window_id(pid) == Some(window)
                && current_front_app_pid() == Some(pid)
                && target
                    .dictation_element
                    .as_ref()
                    .is_some_and(|element| element.is_current())
        })
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

/// Check that the foreground/focused target fingerprint has not changed since capture.
/// Windows callers use this as a document-read fence, before and after synchronous UIA calls.
pub(crate) fn selection_insertion_target_is_current(target: &SelectionInsertionTarget) -> bool {
    if target.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return false;
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if !target_position::unchanged(
            *target.dictation_position.lock(),
            crate::host_document::focused_input_position(),
        )
    {
        return false;
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if target.strict_input_target
        && target
            .dictation_position
            .lock()
            .is_some_and(|(_, length)| length > 0)
    {
        #[cfg(target_os = "macos")]
        let current_selection = read_selection_for_validation();
        #[cfg(target_os = "windows")]
        let current_selection = crate::host_document::read_qa_selection();
        if target.dictation_selection.is_none() || target.dictation_selection != current_selection {
            return false;
        }
    }
    native_input_target_is_current(target)
}

fn native_input_target_is_current(target: &SelectionInsertionTarget) -> bool {
    if target.cancelled.load(std::sync::atomic::Ordering::Acquire) { return false; }
    #[cfg(target_os = "windows")]
    {
        let Some(captured) = target.windows else {
            return false;
        };
        return capture_windows_selection_target().as_ref() == Some(&captured)
            && (target.dictation_control.as_ref().is_some_and(|identity| {
                crate::host_document::focused_input_identity().as_ref() == Some(identity)
            }));
    }

    #[cfg(target_os = "macos")]
    {
        return target.macos.as_ref().is_some_and(|captured| {
            (!target.strict_input_target || captured.dictation_window.is_some() && captured.dictation_element.is_some())
                && captured.front_app_pid.is_some_and(|pid| macos_target_is_current(captured, pid))
        });
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = target;
        true
    }
}

/// Revalidate the target and the selected text immediately before insertion.
///
/// Native text and position reads never borrow the clipboard. This can run inside the
/// delivery gate immediately before paste, after an earlier caller-side validation.
pub(crate) fn validate_selection_insertion_target(
    target: &SelectionInsertionTarget,
    expected_selection: &str,
) -> SelectionInsertionTargetValidation {
    if !selection_insertion_target_is_current(target) {
        return SelectionInsertionTargetValidation::TargetChanged;
    }
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

        let current_selection = crate::host_document::read_qa_selection();
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }

        let Some(current_after_copy) = capture_windows_selection_target() else {
            return SelectionInsertionTargetValidation::TargetUnavailable;
        };
        if !windows_selection_targets_match(captured, current_after_copy) {
            return SelectionInsertionTargetValidation::TargetChanged;
        }
        if !selection_insertion_target_is_current(target) {
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
        // Selection text consistency: read AX directly, without simulated Cmd+C.
        let current_selection = read_selection_for_validation();
        if !selection_text_matches(expected_selection, current_selection.as_deref()) {
            return SelectionInsertionTargetValidation::SelectionChanged;
        }
        if !selection_insertion_target_is_current(target) {
            return SelectionInsertionTargetValidation::TargetChanged;
        }
        return SelectionInsertionTargetValidation::Valid;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        SelectionInsertionTargetValidation::TargetUnavailable
    }
}

/// Read the complete live range for exact replacement validation. Missing native
/// range/text capability refuses automatic replacement; it never uses Copy here.
#[cfg(target_os = "macos")]
fn read_selection_for_validation() -> Option<String> {
    macos_ax::read_selected_text()
        .filter(|text| !text.trim().is_empty())
        .or_else(macos_ax::read_explicit_qa_text)
}

/// Return focus to the original selection target after the preview is confirmed. The preview
/// window allows editing, so at confirm time it is necessarily no longer the original app's
/// foreground window; restore the original target first, then run the strict selection validation
/// above, avoiding a blind paste.
pub(crate) fn reactivate_selection_insertion_target(target: &SelectionInsertionTarget) -> bool {
    reactivate_insertion_target(target, true)
}

/// Ordinary dictation retains stable app-level macOS activation; selection replacement
/// continues to require its captured control before the full source/range validation.
pub(crate) fn reactivate_dictation_insertion_target(target: &SelectionInsertionTarget) -> bool {
    reactivate_insertion_target(target, false)
}

fn reactivate_insertion_target(target: &SelectionInsertionTarget, strict_control: bool) -> bool {
    #[cfg(not(target_os = "macos"))]
    let _ = strict_control;
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
            return !strict_control || macos_target_is_current(captured, pid);
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
            if current_front_app_pid() == Some(pid)
                && (!strict_control || macos_target_is_current(captured, pid)) {
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

/// Q&A accepts only an explicit native selection. Copy shortcuts can copy an
/// entire editor line with no selection, so they are not evidence for this path.
pub(crate) fn capture_explicit_qa_selection() -> (Option<SelectionContext>, SelectionInsertionTarget)
{
    let target = capture_selection_insertion_target();
    #[cfg(target_os = "macos")]
    let text = macos_ax::read_explicit_qa_text();
    #[cfg(target_os = "windows")]
    let text = crate::host_document::read_qa_selection();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let text: Option<String> = None;
    let selection = text.filter(|text| !text.trim().is_empty()).map(|text| {
        let (text, omits_middle) = stored_selection_text(&text);
        SelectionContext {
            text,
            omits_middle,
            source_app: current_front_app(),
        }
    });
    (selection, target)
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
            let (text, omits_middle) = stored_selection_text(&text);
            return (
                SelectionCaptureOutcome {
                    selection: Some(SelectionContext {
                        text,
                        source_app,
                        omits_middle,
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
                    let (text, omits_middle) = stored_selection_text(&text);
                    return (
                        SelectionCaptureOutcome {
                            selection: Some(SelectionContext {
                                text,
                                source_app,
                                omits_middle,
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

/// Preserve the explicit selection exactly; validation compares the same bytes.
fn stored_selection_text(text: &str) -> (String, bool) {
    (text.to_string(), false)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn simulate_copy_and_read_diag() -> Result<String, SelectionCaptureMissReason> {
    #[cfg(target_os = "windows")]
    {
        windows_simulate_copy_and_read_diag()
    }
    #[cfg(target_os = "macos")]
    {
        macos_simulate_copy_and_read_diag()
    }
}

/// Windows: detect the simulated Ctrl+C by watching the clipboard sequence number instead of
/// writing a sentinel first. The sentinel used to land in Windows Clipboard History (Win+V) as
/// `__openless_qa_sentinel_*__` entries on every capture (issues #1331 / #1335), and writing it
/// woke the history listener mid-capture. Watching the sequence number removes that extra write
/// and keeps these internal markers out of the user's clipboard history.
#[cfg(target_os = "windows")]
fn windows_simulate_copy_and_read_diag() -> Result<String, SelectionCaptureMissReason> {
    let Some(_delivery) = crate::insertion::clipboard_delivery::try_lock() else {
        return Err(SelectionCaptureMissReason::ClipboardReadFailed);
    };
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[selection] clipboard init failed: {e}");
            return Err(SelectionCaptureMissReason::ClipboardInitFailed);
        }
    };
    let (post_ok, outcome) =
        clipboard_capture::capture_with_restore(&mut clipboard).map_err(|e| {
            log::warn!("[selection] clipboard backup failed; skipping simulated copy: {e}");
            SelectionCaptureMissReason::ClipboardReadFailed
        })?;

    if !post_ok {
        return Err(SelectionCaptureMissReason::CopyShortcutFailed);
    }
    match outcome {
        // The sequence never moved → Ctrl+C did not take effect or the target has no selection.
        ClipboardPollOutcome::Unchanged => {
            Err(SelectionCaptureMissReason::ClipboardUnchangedSentinel)
        }
        ClipboardPollOutcome::Unreadable => Err(SelectionCaptureMissReason::ClipboardReadFailed),
        ClipboardPollOutcome::Captured(text) if text.is_empty() => {
            Err(SelectionCaptureMissReason::ClipboardEmptyAfterCopy)
        }
        ClipboardPollOutcome::Captured(text) => Ok(text),
    }
}

#[cfg(target_os = "windows")]
impl clipboard_capture::ClipboardCapture for arboard::Clipboard {
    type Error = arboard::Error;

    fn backup_text(&mut self) -> Result<Option<String>, Self::Error> {
        match self.get_text() {
            Ok(text) => Ok(Some(text)),
            // Only a missing text format may be treated as an absent text backup. Contention
            // and other access failures must stop capture before Ctrl+C overwrites the user data.
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn copy_selection(&mut self) -> (bool, ClipboardPollOutcome, u64) {
        use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

        let baseline_sequence = unsafe { GetClipboardSequenceNumber() };
        let mut post_ok = post_copy_shortcut();
        log::info!("[selection] post_copy: post_ok={post_ok}");
        if !post_ok {
            log::warn!("[selection] post_copy_shortcut failed");
        }
        let (outcome, sequence) =
            windows_poll_clipboard_after_copy(self, baseline_sequence, &mut post_ok);
        (post_ok, outcome, sequence)
    }

    fn sequence(&self) -> u64 {
        unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() as u64 }
    }

    fn restore_text(&mut self, text: &str) {
        if let Err(error) = self.set_text(text) {
            log::warn!("[selection] clipboard restore failed: {error}");
        }
    }
}

/// macOS: write a sentinel so a failed Cmd+C can be told apart from a target app that has no
/// selection, then restore the user's clipboard.
#[cfg(target_os = "macos")]
fn macos_simulate_copy_and_read_diag() -> Result<String, SelectionCaptureMissReason> {
    let Some(_delivery) = crate::insertion::clipboard_delivery::try_lock() else {
        return Err(SelectionCaptureMissReason::ClipboardReadFailed);
    };
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
        Err(arboard::Error::ContentNotAvailable) => None,
        Err(e) => {
            log::warn!("[selection] clipboard backup failed; skipping simulated copy: {e}");
            return Err(SelectionCaptureMissReason::ClipboardReadFailed);
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

    // c) Simulate Cmd+C
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

    std::thread::sleep(Duration::from_millis(80));
    let copied_sequence = crate::insertion::clipboard_sequence();
    let captured = clipboard.get_text().ok();

    // A newer external copy takes priority over this temporary borrow.
    if !crate::insertion::clipboard_revision_matches(
        copied_sequence,
        crate::insertion::clipboard_sequence(),
    ) {
        return Err(SelectionCaptureMissReason::ClipboardReadFailed);
    }
    // d) Restore the original clipboard
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

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn selection_text_matches(expected: &str, actual: Option<&str>) -> bool {
    actual.is_some_and(|actual| actual == expected)
}

#[cfg(target_os = "windows")]
fn capture_windows_selection_target() -> Option<WindowsSelectionTarget> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    capture_windows_selection_target_for_window(unsafe { GetForegroundWindow() })
}

#[cfg(target_os = "windows")]
fn capture_windows_selection_target_for_window(
    foreground: windows::Win32::Foundation::HWND,
) -> Option<WindowsSelectionTarget> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    unsafe {
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

#[cfg(target_os = "macos")]
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

    #[repr(C)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, seconds: f32) -> AxError;
        fn AXUIElementGetPid(element: AxUiElementRef, pid: *mut i32) -> AxError;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AxError;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: AxUiElementRef,
            attribute: CFStringRef,
            parameter: CFTypeRef,
            value: *mut CFTypeRef,
        ) -> AxError;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFEqual(left: CFTypeRef, right: CFTypeRef) -> u8;
        fn CFRetain(value: CFTypeRef) -> CFTypeRef;
        fn CFGetTypeID(value: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
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
        fn CFStringGetCharacters(s: CFStringRef, range: CfRange, buffer: *mut u16);
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// A retained native identity, never an AX text snapshot. AX objects may be
    /// compared from the insertion worker; ownership ends with the segment.
    #[derive(Debug)]
    pub(super) struct FocusedElement(usize);
    impl Drop for FocusedElement {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0 as CFTypeRef);
            }
        }
    }
    impl FocusedElement {
        pub(super) fn same_as(&self, other: &Self) -> bool {
            unsafe { CFEqual(self.0 as CFTypeRef, other.0 as CFTypeRef) != 0 }
        }
        pub(super) fn is_current(&self) -> bool {
            capture_focused_element().is_some_and(|current| unsafe {
                CFEqual(self.0 as CFTypeRef, current.0 as CFTypeRef) != 0
            })
        }
    }
    pub(super) fn capture_focused_element() -> Option<std::sync::Arc<FocusedElement>> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() { return None; }
            AXUIElementSetMessagingTimeout(system, 0.2);
            let element = attribute(system, b"AXFocusedUIElement\0");
            CFRelease(system as CFTypeRef);
            element.map(|element| std::sync::Arc::new(FocusedElement(element as usize)))
        }
    }

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

    unsafe fn attribute(element: AxUiElementRef, name: &'static [u8]) -> Option<CFTypeRef> {
        let key = cfstring_from_static(name)?;
        let mut value = std::ptr::null();
        let result = AXUIElementCopyAttributeValue(element, key, &mut value);
        CFRelease(key);
        if result == AX_ERROR_SUCCESS && !value.is_null() {
            Some(value)
        } else {
            if !value.is_null() {
                CFRelease(value);
            }
            None
        }
    }
    unsafe fn string_attribute(element: AxUiElementRef, name: &'static [u8]) -> Option<String> {
        let value = attribute(element, name)?;
        let text = if CFGetTypeID(value) == CFStringGetTypeID() {
            cfstring_to_rust(value)
        } else {
            None
        };
        CFRelease(value);
        text
    }
    /// Web content may expose its selection through text markers on an ancestor.
    /// This path reads selected ranges only and never falls back to document text or Copy.
    pub fn read_explicit_qa_text() -> Option<String> {
        read_explicit_text(None).ok().flatten()
    }

    unsafe fn selected_string(value: CFTypeRef, bounded: bool) -> Result<Option<String>, ()> {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            return Ok(None);
        }
        if !bounded {
            return Ok(cfstring_to_rust(value));
        }
        let length = CFStringGetLength(value);
        if length > 8_000 {
            return Err(());
        }
        if length <= 0 {
            return Ok(None);
        }
        let mut units = vec![0u16; length as usize];
        CFStringGetCharacters(value, CfRange { location: 0, length }, units.as_mut_ptr());
        Ok(String::from_utf16(&units).ok())
    }

    pub fn read_explicit_text(expected_pid: Option<i32>) -> Result<Option<String>, ()> {
        unsafe {
            let system = match expected_pid {
                Some(pid) => AXUIElementCreateApplication(pid),
                None => AXUIElementCreateSystemWide(),
            };
            if system.is_null() {
                return Ok(None);
            }
            let _timeout = expected_pid.map(|_| AXUIElementSetMessagingTimeout(system, 0.12));
            let focused = attribute(system, b"AXFocusedUIElement\0");
            let Some(focused) = focused else {
                CFRelease(system as CFTypeRef);
                return Ok(None);
            };
            let mut pid = 0;
            if expected_pid.is_some_and(|expected| {
                AXUIElementGetPid(focused as AxUiElementRef, &mut pid) != AX_ERROR_SUCCESS
                    || pid != expected
            }) || string_attribute(focused as AxUiElementRef, b"AXSubrole\0").as_deref()
                == Some("AXSecureTextField")
                || (expected_pid.is_some()
                    && string_attribute(focused as AxUiElementRef, b"AXRole\0").as_deref()
                        == Some("AXSecureTextField"))
            {
                CFRelease(focused);
                CFRelease(system as CFTypeRef);
                return Ok(None);
            }
            if expected_pid.is_some() {
                CFRetain(focused);
            }
            let mut current = focused;
            let mut result = Ok(None);
            for _ in 0..8 {
                if expected_pid.is_some_and(|expected| {
                    AXUIElementGetPid(current as AxUiElementRef, &mut pid) != AX_ERROR_SUCCESS
                        || pid != expected
                }) {
                    break;
                }
                if expected_pid.is_some() {
                    let _ = AXUIElementSetMessagingTimeout(current as AxUiElementRef, 0.12);
                }
                if let Some(value) = attribute(current as AxUiElementRef, b"AXSelectedText\0") {
                    result = selected_string(value, expected_pid.is_some())
                        .map(|text| text.filter(|text| !text.trim().is_empty()));
                    CFRelease(value);
                    if !matches!(result, Ok(None)) {
                        break;
                    }
                }
                if let Some(range) =
                    attribute(current as AxUiElementRef, b"AXSelectedTextMarkerRange\0")
                {
                    if let Some(key) = cfstring_from_static(b"AXStringForTextMarkerRange\0") {
                        let mut value = std::ptr::null();
                        if AXUIElementCopyParameterizedAttributeValue(
                            current as AxUiElementRef,
                            key,
                            range,
                            &mut value,
                        ) == AX_ERROR_SUCCESS
                            && !value.is_null()
                        {
                            result = selected_string(value, expected_pid.is_some())
                                .map(|text| text.filter(|text| !text.trim().is_empty()));
                            CFRelease(value);
                        }
                        CFRelease(key);
                    }
                    CFRelease(range);
                    if !matches!(result, Ok(None)) {
                        break;
                    }
                }
                let Some(parent) = attribute(current as AxUiElementRef, b"AXParent\0") else {
                    break;
                };
                if parent == current {
                    CFRelease(parent);
                    break;
                }
                CFRelease(current);
                current = parent;
            }
            CFRelease(current);
            if expected_pid.is_some() {
                let same = attribute(system, b"AXFocusedUIElement\0").is_some_and(|now| {
                    let same = CFEqual(focused, now) != 0;
                    CFRelease(now);
                    same
                });
                CFRelease(focused);
                if !same {
                    result = Ok(None);
                }
            }
            CFRelease(system as CFTypeRef);
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

/// Poll the clipboard sequence number (not its text) until the target app's Ctrl+C lands.
/// Watching the sequence avoids writing a sentinel — the old sentinel showed up in Windows
/// Clipboard History as `__openless_qa_sentinel_*__` entries (#1331 / #1335). One Ctrl+C resend
/// mid-window covers the case where the first chord hit a still-busy clipboard.
#[cfg(target_os = "windows")]
fn windows_poll_clipboard_after_copy(
    clipboard: &mut arboard::Clipboard,
    baseline_sequence: u32,
    post_ok: &mut bool,
) -> (ClipboardPollOutcome, u64) {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

    const MAX_MS: u64 = 450;
    const STEP_MS: u64 = 25;
    let start = std::time::Instant::now();
    let mut attempts = 0u32;
    let mut resent_ctrl_c = false;
    let mut changed = false;
    let mut last_sequence;

    loop {
        attempts += 1;
        last_sequence = unsafe { GetClipboardSequenceNumber() };
        if last_sequence != baseline_sequence {
            changed = true;
            // The clipboard may be transiently held by another process (Clipboard History,
            // cloud clipboard). Keep polling until the deadline instead of failing immediately.
            if let Ok(text) = clipboard.get_text() {
                log::info!(
                    "[selection] clipboard_poll: waited_ms={} attempts={} resent={} changed=true",
                    start.elapsed().as_millis() as u64,
                    attempts,
                    resent_ctrl_c
                );
                if unsafe { GetClipboardSequenceNumber() } == last_sequence {
                    return (ClipboardPollOutcome::Captured(text), last_sequence as u64);
                }
            }
        }

        if start.elapsed().as_millis() as u64 >= MAX_MS {
            break;
        }
        // Halfway through: if nothing landed yet, resend Ctrl+C once.
        if !resent_ctrl_c && start.elapsed().as_millis() as u64 >= 150 {
            if post_copy_shortcut() {
                *post_ok = true;
                resent_ctrl_c = true;
                log::info!("[selection] clipboard_poll: resent Ctrl+C after sequence miss");
            }
        }
        std::thread::sleep(Duration::from_millis(STEP_MS));
    }

    log::info!(
        "[selection] clipboard_poll: waited_ms={} attempts={} resent={} changed={}",
        start.elapsed().as_millis() as u64,
        attempts,
        resent_ctrl_c,
        changed
    );
    if changed {
        (ClipboardPollOutcome::Unreadable, last_sequence as u64)
    } else {
        (ClipboardPollOutcome::Unchanged, last_sequence as u64)
    }
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

/// Read the frontmost application identity from one `NSRunningApplication` snapshot.
///
/// The name, bundle identifier, and pid must come from the same application object: querying
/// `NSWorkspace.frontmostApplication` separately for each field can combine metadata from two apps
/// if focus changes between calls.
#[cfg(target_os = "macos")]
fn current_front_app_snapshot() -> (Option<String>, Option<String>, Option<i32>) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let Some(cls) = AnyClass::get("NSWorkspace") else {
            return (None, None, None);
        };
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return (None, None, None);
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return (None, None, None);
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        let pid: i32 = msg_send![app, processIdentifier];
        (
            ns_string_to_rust(name_obj),
            ns_string_to_rust(bundle_obj),
            (pid > 0).then_some(pid),
        )
    }
}

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
    let (name, bundle, _) = current_front_app_snapshot();
    (name, bundle)
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
pub(crate) fn front_app_for_target(target: &SelectionInsertionTarget) -> Option<String> {
    target.windows_app.as_ref().map(|app| app.name.clone())
}

#[cfg(target_os = "macos")]
pub(crate) fn front_app_for_target(target: &SelectionInsertionTarget) -> Option<String> {
    target
        .macos
        .as_ref()
        .and_then(|captured| captured.front_app.clone())
}

#[cfg(target_os = "macos")]
pub(crate) fn bundle_id_for_target(target: &SelectionInsertionTarget) -> Option<String> {
    target
        .macos
        .as_ref()
        .and_then(|captured| captured.bundle_id.clone())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn front_app_for_target(_target: &SelectionInsertionTarget) -> Option<String> {
    None
}

/// Identity comes from the same native target as the insertion adapter.
pub(crate) fn input_app_for_target(target: &SelectionInsertionTarget) -> Option<openless_core::app_style_routing::InputApp> {
    #[cfg(target_os = "macos")]
    {
        let captured = target.macos.as_ref()?;
        let pid = captured.front_app_pid?;
        if pid as u32 == std::process::id() {
            return None;
        }
        openless_core::app_style_routing::InputApp::macos(
            captured.bundle_id.as_deref()?,
            captured.name.as_deref(),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let captured = target.windows?;
        if captured.focused_process_id == std::process::id() {
            return None;
        }
        target.windows_app.clone()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
        None
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn ime_target_for_selection(
    target: &SelectionInsertionTarget,
) -> Option<crate::windows_ime_ipc::ImeSubmitTarget> {
    let captured = target.windows?;
    Some(crate::windows_ime_ipc::ImeSubmitTarget {
        process_id: captured.focused_process_id,
        thread_id: captured.focused_thread_id,
        window_handle: captured.focused_window,
    })
}

#[cfg(target_os = "windows")]
fn windows_app_for_window(hwnd: windows::Win32::Foundation::HWND) -> Option<openless_core::app_style_routing::InputApp> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    crate::activity_app_icons::app_for_pid(pid)
}

#[cfg(target_os = "windows")]
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    (windows_app_for_window(unsafe { GetForegroundWindow() }).map(|app| app.name), None)
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    (None, None)
}

/// The foreground app's display string, like `"Safari (com.apple.Safari)"` (the executable name on
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
pub(crate) fn current_front_app_pid() -> Option<i32> {
    current_front_app_snapshot().2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_preserve_whitespace_and_the_middle_at_any_length() {
        for length in [32, 4001, 16001, 48000] {
            let live = format!(
                " \n{} UNIQUE_MIDDLE_选区 {}\n\t",
                "字".repeat(length / 2),
                "尾".repeat(length / 2)
            );
            let (stored, omits_middle) = stored_selection_text(&live);
            assert!(!omits_middle);
            assert_eq!(stored, live);
            assert!(selection_text_matches(&stored, Some(&live)));
            assert!(!selection_text_matches(&stored, Some(live.trim())));
        }
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
            &SelectionInsertionTarget::default()
        ));
        assert!(selection_insertion_target_is_captured(
            &SelectionInsertionTarget {
                windows: Some(windows_target(10)),
                windows_app: None,
                ..Default::default()
            }
        ));
    }
}
