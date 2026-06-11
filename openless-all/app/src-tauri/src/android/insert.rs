//! Android cross-app text insertion strategies.

#![cfg(target_os = "android")]
use crate::android::types::AndroidInsertStrategy;
use crate::insertion::TextInserter;
use crate::types::InsertStatus;

pub fn android_insert_with_strategy(
    inserter: &TextInserter,
    text: &str,
    strategy: AndroidInsertStrategy,
) -> InsertStatus {
    if text.is_empty() {
        return InsertStatus::CopiedFallback;
    }

    match strategy {
        AndroidInsertStrategy::Clipboard => clipboard_fallback(inserter, text),
        AndroidInsertStrategy::Accessibility
        | AndroidInsertStrategy::Auto
        | AndroidInsertStrategy::Ime => {
            try_accessibility(inserter, text).unwrap_or_else(|| clipboard_fallback(inserter, text))
        }
    }
}

fn try_accessibility(inserter: &TextInserter, text: &str) -> Option<InsertStatus> {
    if !crate::android::accessibility::get_android_accessibility_status().enabled {
        log::info!("[android-insert] accessibility service not enabled");
        return None;
    }
    if !matches!(inserter.copy_fallback(text), InsertStatus::CopiedFallback) {
        return None;
    }
    if crate::android::accessibility::paste_via_accessibility() {
        Some(InsertStatus::Inserted)
    } else {
        log::warn!("[android-insert] accessibility paste failed; text remains on clipboard");
        Some(InsertStatus::CopiedFallback)
    }
}

fn clipboard_fallback(inserter: &TextInserter, text: &str) -> InsertStatus {
    let status = inserter.copy_fallback(text);
    if matches!(status, InsertStatus::CopiedFallback) {
        let _ = crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::show_overlay_toast(env, context, "已复制到剪贴板")
        });
    }
    status
}
