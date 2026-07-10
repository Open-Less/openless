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
