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

/// 待用户确认的纠正建议（Tier2 那一档）。
///
/// 只在内存里，重启即空 —— 建议本身是易逝的，用户下次犯同样的错会再产生一条。
#[tauri::command]
pub fn list_pending_corrections(coord: CoordinatorState<'_>) -> Vec<crate::types::PendingCorrection> {
    coord.list_pending_corrections()
}

/// 接受一条建议。落库路径与自动收集完全一致 —— 规则 + 热词 + 查重，同样打 `learned`
/// 标记，用户随时能在词汇表里看到并删掉。
#[tauri::command]
pub fn accept_pending_correction(coord: CoordinatorState<'_>, id: String) {
    coord.accept_pending_correction(&id);
}

#[tauri::command]
pub fn dismiss_pending_correction(coord: CoordinatorState<'_>, id: String) {
    coord.dismiss_pending_correction(&id);
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
