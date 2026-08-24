use super::*;

#[tauri::command]
pub fn list_vocab(coord: CoordinatorState<'_>) -> Result<Vec<DictionaryEntry>, String> {
    coord.vocab().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_vocab(
    coord: CoordinatorState<'_>,
    phrase: String,
    note: Option<String>,
) -> Result<DictionaryEntry, String> {
    coord.vocab().add(phrase, note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_vocab_with_metadata(
    coord: CoordinatorState<'_>,
    phrase: String,
    note: Option<String>,
    scope: DictionaryScope,
    project_key: Option<String>,
    expires_at: Option<String>,
) -> Result<DictionaryEntry, String> {
    let expires_at = if scope == DictionaryScope::Temporary {
        expires_at.or_else(|| Some(coord.temporary_vocab_expiry()))
    } else {
        expires_at
    };
    let project_key = if scope == DictionaryScope::Temporary {
        project_key.or_else(|| coord.active_project_key())
    } else {
        None
    };
    let entry = coord
        .vocab()
        .add_with_metadata(phrase, note, scope, project_key, expires_at)
        .map_err(|e| e.to_string())?;
    if scope == DictionaryScope::Temporary {
        coord
            .vocab()
            .enforce_temporary_capacity(coord.temporary_vocab_capacity())
            .map_err(|e| e.to_string())?;
    }
    Ok(entry)
}

#[tauri::command]
pub fn remove_vocab(coord: CoordinatorState<'_>, id: String) -> Result<(), String> {
    coord.vocab().remove(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_vocab_enabled(
    coord: CoordinatorState<'_>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    coord
        .vocab()
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_vocab_metadata(
    coord: CoordinatorState<'_>,
    id: String,
    pinned: bool,
    scope: DictionaryScope,
    project_key: Option<String>,
    expires_at: Option<String>,
) -> Result<DictionaryEntry, String> {
    let expires_at = if scope == DictionaryScope::Temporary {
        expires_at.or_else(|| Some(coord.temporary_vocab_expiry()))
    } else {
        None
    };
    let project_key = if scope == DictionaryScope::Temporary {
        project_key.or_else(|| coord.active_project_key())
    } else {
        None
    };
    let entry = coord
        .vocab()
        .set_metadata(&id, pinned, scope, project_key, expires_at)
        .map_err(|e| e.to_string())?;
    if scope == DictionaryScope::Temporary {
        coord
            .vocab()
            .enforce_temporary_capacity(coord.temporary_vocab_capacity())
            .map_err(|e| e.to_string())?;
    }
    Ok(entry)
}

#[tauri::command]
pub fn list_vocab_suggestions(
    coord: CoordinatorState<'_>,
) -> Result<Vec<PendingCorrection>, String> {
    if !coord.prefs().get().vocab_suggestion_inbox_enabled {
        return Ok(Vec::new());
    }
    coord
        .vocab()
        .list_suggestions()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dictionary_delivery_preview(
    coord: CoordinatorState<'_>,
) -> DictionaryDeliveryReport {
    coord.dictionary_delivery_preview()
}

#[tauri::command]
pub fn list_correction_rules(coord: CoordinatorState<'_>) -> Result<Vec<CorrectionRule>, String> {
    coord.correction_rules().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_correction_rule(
    coord: CoordinatorState<'_>,
    pattern: String,
    replacement: String,
) -> Result<CorrectionRule, String> {
    coord
        .correction_rules()
        .add(pattern, replacement)
        .map_err(|e| e.to_string())
}

/// 卡片上点了勾：把这个词收进词汇表，打「自动收集」标记，随时能在词汇表页删掉。
#[tauri::command]
pub fn accept_pending_correction(coord: CoordinatorState<'_>, id: String) {
    coord.accept_pending_correction(&id);
}

/// 卡片上点了叉：丢掉这一条，什么都不记（没有拒绝名单）。
#[tauri::command]
pub fn reject_pending_correction(coord: CoordinatorState<'_>, id: String) {
    coord.reject_pending_correction(&id);
}

/// 卡片 10 秒到期，或新一轮听写开始。
#[tauri::command]
pub fn dismiss_vocab_suggestions(coord: CoordinatorState<'_>) {
    coord.dismiss_vocab_suggestions();
}

/// 落字失败兜底卡片上点了「复制」。
///
/// **走后端而不是前端的 `navigator.clipboard`**：卡片浮在别的 app 上面，按钮刻意
/// `preventDefault` 不抢焦点（抢了就把用户正在写的地方的光标弄没了），而未聚焦的
/// 文档调 `navigator.clipboard.writeText` 会直接抛 `Document is not focused`。
#[tauri::command]
pub fn copy_text_to_clipboard(text: String) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    crate::insertion::copy_text_to_clipboard(&text)
}

/// 兜底卡片自己关掉了（用户点关闭 / TTL 到时）。
#[tauri::command]
pub fn dismiss_insert_fallback_card(coord: CoordinatorState<'_>) {
    coord.dismiss_insert_fallback_card();
}

/// 前端按真实折行结果回报卡片高度；presentation_id 用来忽略旧组件迟到的 ResizeObserver。
#[tauri::command]
pub fn report_insert_fallback_card_height(
    coord: CoordinatorState<'_>,
    presentation_id: u64,
    height: f64,
) -> Result<(), String> {
    coord.report_insert_fallback_card_height(presentation_id, height)
}

#[tauri::command]
pub fn remove_correction_rule(coord: CoordinatorState<'_>, id: String) -> Result<(), String> {
    coord
        .correction_rules()
        .remove(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_correction_rule_enabled(
    coord: CoordinatorState<'_>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    coord
        .correction_rules()
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_vocab_presets() -> Result<VocabPresetStore, String> {
    crate::persistence::list_vocab_presets().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_vocab_presets(store: VocabPresetStore) -> Result<(), String> {
    crate::persistence::save_vocab_presets(&store).map_err(|e| e.to_string())
}
