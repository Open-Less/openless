//! Android cross-app text insertion strategies.

#![cfg(target_os = "android")]
use crate::android::accessibility::{is_accessibility_enabled, paste_via_accessibility_with_result};
use crate::android::insert_tiers::{
    resolve_tiered_insert_status, TieredInsertOutcome, PASTE_RESULT_SUCCESS,
    PASTE_RESULT_SHIZUKU_UNAVAILABLE,
};
use crate::android::shizuku::paste_via_shizuku_with_result;
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
        | AndroidInsertStrategy::Ime => insert_with_tiered_fallback(inserter, text),
    }
}

fn insert_with_tiered_fallback(inserter: &TextInserter, text: &str) -> InsertStatus {
    let previous_clip: Option<String> =
        crate::android::jni::android::with_android_env(|env, context| {
            Ok(crate::android::jni::android::get_primary_clip_text(env, context))
        })
        .ok()
        .flatten();

    if !matches!(inserter.copy_fallback(text), InsertStatus::CopiedFallback) {
        return InsertStatus::Failed;
    }

    let accessibility_result = if is_accessibility_enabled() {
        Some(paste_via_accessibility_with_result(text))
    } else {
        log::info!("[android-insert] tier1 skipped: accessibility service not enabled");
        None
    };
    if let Some(ref paste_result) = accessibility_result {
        if paste_result != PASTE_RESULT_SUCCESS {
            log::warn!("[android-insert] tier1 accessibility paste failed reason={paste_result}");
        }
    }

    let shizuku_result = match accessibility_result.as_deref() {
        Some(PASTE_RESULT_SUCCESS) => {
            log::info!("[android-insert] tier2 skipped: tier1 succeeded");
            None
        }
        _ => Some(paste_via_shizuku_with_result()),
    };
    if let Some(ref result) = shizuku_result {
        if result != PASTE_RESULT_SUCCESS && result != PASTE_RESULT_SHIZUKU_UNAVAILABLE {
            log::warn!("[android-insert] tier2 shizuku paste failed reason={result}");
        } else if result == PASTE_RESULT_SHIZUKU_UNAVAILABLE {
            log::info!("[android-insert] tier2 skipped: shizuku unavailable");
        }
    }

    match resolve_tiered_insert_status(
        accessibility_result.as_deref(),
        shizuku_result.as_deref(),
    ) {
        TieredInsertOutcome::Inserted => {
            restore_clipboard_after_success(previous_clip);
            InsertStatus::Inserted
        }
        TieredInsertOutcome::ClipboardFallback => clipboard_fallback(inserter, text),
    }
}

fn restore_clipboard_after_success(previous_clip: Option<String>) {
    if let Some(prev) = previous_clip {
        if let Err(e) =
            crate::android::jni::android::with_android_env(|env, context| {
                crate::android::jni::android::set_primary_clip_text(env, context, &prev)
            })
        {
            log::warn!("[android-insert] failed to restore clipboard: {e}");
        }
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
