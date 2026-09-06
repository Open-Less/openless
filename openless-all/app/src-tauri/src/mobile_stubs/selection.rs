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

/// 普通听写在所有 Host 都先创建一个 insertion session。移动端没有可恢复的桌面焦点，
/// 因此只返回不携带状态的 opaque token；真正落字仍由 Android accessibility/Shizuku
/// Adapter 决定，不能把这个 token 当作 Selection Polish 的可校验目标。
pub(crate) fn capture_selection_insertion_target() -> SelectionInsertionTarget {
    SelectionInsertionTarget
}

/// 移动端普通落字无需切回另一个桌面应用，恢复动作是成功的 no-op。Selection Polish
/// 仍通过 `selection_insertion_target_is_captured == false` 保持不可用，二者语义不可混用。
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

/// 与桌面端 `selection::current_front_app_parts` 同形。移动端没有「前台 app」这个
/// 概念（我们自己就是前台），恒返回空 —— 存在的意义只是让 `capsule_focus` 那边能有
/// 一份跨平台统一的实现，不必再写第二份平台分流。
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
