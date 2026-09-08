use super::*;

#[tauri::command]
pub fn list_vocab(core: CoreState<'_>) -> Result<Vec<DictionaryEntry>, String> {
    core.list_vocabulary().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_vocab(
    core: CoreState<'_>,
    phrase: String,
    note: Option<String>,
) -> Result<DictionaryEntry, String> {
    core.add_vocabulary(phrase, note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_vocab(core: CoreState<'_>, id: String) -> Result<(), String> {
    core.remove_vocabulary(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_vocab_enabled(core: CoreState<'_>, id: String, enabled: bool) -> Result<(), String> {
    core.set_vocabulary_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_correction_rules(core: CoreState<'_>) -> Result<Vec<CorrectionRule>, String> {
    core.list_correction_rules().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_correction_rule(
    core: CoreState<'_>,
    pattern: String,
    replacement: String,
) -> Result<CorrectionRule, String> {
    core.add_correction_rule(pattern, replacement)
        .map_err(|e| e.to_string())
}

/// 卡片上点了勾：把这个词收进词汇表，打「自动收集」标记，随时能在词汇表页删掉。
#[tauri::command]
pub fn accept_pending_correction(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
    id: String,
) {
    match core.accept_pending_correction(&id) {
        Ok(Some(suggestion)) => {
            log::info!(
                "[cursor-context] learned vocabulary entry: {:?} (was {:?})",
                suggestion.replacement,
                suggestion.pattern
            );
            coord.refresh_vocab_suggestion_presentation(!core.pending_corrections().is_empty());
        }
        Ok(None) => {}
        Err(error) => log::warn!("[cursor-context] accept learned vocabulary failed: {error}"),
    }
}

/// 卡片上点了叉：丢掉这一条，什么都不记（没有拒绝名单）。
#[tauri::command]
pub fn reject_pending_correction(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
    id: String,
) {
    if core.reject_pending_correction(&id) {
        coord.refresh_vocab_suggestion_presentation(!core.pending_corrections().is_empty());
    }
}

/// 卡片 10 秒到期，或新一轮听写开始。
#[tauri::command]
pub fn dismiss_vocab_suggestions(core: CoreState<'_>, coord: CoordinatorState<'_>) {
    core.dismiss_pending_corrections();
    coord.refresh_vocab_suggestion_presentation(false);
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
pub fn remove_correction_rule(core: CoreState<'_>, id: String) -> Result<(), String> {
    core.remove_correction_rule(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_correction_rule_enabled(
    core: CoreState<'_>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    core.set_correction_rule_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_vocab_presets(core: CoreState<'_>) -> Result<VocabPresetStore, String> {
    core.list_vocabulary_presets().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_vocab_presets(core: CoreState<'_>, store: VocabPresetStore) -> Result<(), String> {
    core.save_vocabulary_presets(&store)
        .map_err(|e| e.to_string())
}
