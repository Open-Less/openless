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
    // 保存粘贴前的剪贴板内容，粘贴完成后还原，避免静默覆盖用户剪贴板。
    let previous_clip: Option<String> =
        crate::android::jni::android::with_android_env(|env, context| {
            Ok(crate::android::jni::android::get_primary_clip_text(env, context))
        })
        .ok()
        .flatten();

    if !matches!(inserter.copy_fallback(text), InsertStatus::CopiedFallback) {
        return None;
    }
    let result = if crate::android::accessibility::paste_via_accessibility() {
        Some(InsertStatus::Inserted)
    } else {
        log::warn!("[android-insert] accessibility paste failed; text remains on clipboard");
        Some(InsertStatus::CopiedFallback)
    };

    // 还原用户原有剪贴板内容（仅当粘贴成功时还原；失败时用户需要自己处理）。
    if matches!(result, Some(InsertStatus::Inserted)) {
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

    result
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
