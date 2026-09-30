//! Mobile selection capture.

const SELECTION_MAX_CHARS: usize = 4000;
const SELECTION_TRUNCATE_HEAD: usize = 2000;
const SELECTION_TRUNCATE_TAIL: usize = 2000;
const SELECTION_TRUNCATED_MARKER: &str = "\n[…truncated…]\n";

#[derive(Debug, Clone)]
pub struct SelectionContext {
    pub text: String,
    pub source_app: Option<String>,
}

pub struct SelectionCaptureOutcome {
    pub selection: Option<SelectionContext>,
}

/// Mobile has no desktop insertion target.  Keep the type-level seam so the
/// shared QA adapter can compile without carrying platform-specific branches
/// through its session state.
#[derive(Debug, Clone, Default)]
pub(crate) struct SelectionInsertionTarget;

/// Normal dictation creates an insertion session first on every Host. Mobile has no restorable desktop
/// focus, so it only returns a stateless opaque token; actual text insertion remains up to the Android
/// accessibility/Shizuku Adapter, and this token must not be treated as a verifiable Selection Polish target.
pub(crate) fn capture_selection_insertion_target() -> SelectionInsertionTarget {
    SelectionInsertionTarget
}

/// Mobile plain insertion never needs to switch back to another desktop app, so restore is a successful
/// no-op. Selection Polish stays unavailable via `selection_insertion_target_is_captured == false`;
/// the two semantics must not be conflated.
pub(crate) fn reactivate_selection_insertion_target(_target: &SelectionInsertionTarget) -> bool {
    true
}

pub(crate) fn resolve_selection_workspace_capture(
) -> (Option<SelectionContext>, SelectionInsertionTarget) {
    (capture_selection(), SelectionInsertionTarget)
}

pub(crate) fn selection_insertion_target_is_captured(_target: &SelectionInsertionTarget) -> bool {
    false
}

pub fn capture_selection_with_status() -> SelectionCaptureOutcome {
    SelectionCaptureOutcome {
        selection: capture_selection(),
    }
}

#[cfg(target_os = "android")]
pub fn capture_selection() -> Option<SelectionContext> {
    let text = match crate::android::jni::android::with_android_env(|env, context| {
        crate::android::jni::android::accessibility_selected_text(env, context)
    }) {
        Ok(Some(text)) => text,
        Ok(None) => return None,
        Err(error) => {
            log::warn!("[selection] Android accessibility selection read failed: {error}");
            return None;
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    log::info!(
        "[selection] Android accessibility read OK ({} chars)",
        trimmed.chars().count()
    );
    Some(SelectionContext {
        text: truncate_selection(trimmed),
        source_app: Some("Android accessibility".to_string()),
    })
}

#[cfg(not(target_os = "android"))]
pub fn capture_selection() -> Option<SelectionContext> {
    None
}

/// Same shape as the desktop `selection::current_front_app_parts`. Mobile has no "foreground app"
/// concept (we are the foreground), so it always returns empty — this exists only so `capsule_focus`
/// can keep one cross-platform implementation instead of a second platform branch.
pub(crate) fn current_front_app_parts() -> (Option<String>, Option<String>) {
    (None, None)
}

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
