use super::*;

#[tauri::command]
pub fn add_learned_vocab(core: CoreState<'_>, phrase: String) -> Result<(), String> {
    core.add_learned_vocabulary(phrase)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

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
pub fn update_vocab(core: CoreState<'_>, id: String, phrase: String) -> Result<(), String> {
    core.update_vocabulary_phrase(&id, phrase)
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

/// Checkmark clicked on the card: add the word to the vocabulary with the "auto-collected"
/// marker; it can be deleted from the vocabulary page at any time.
#[tauri::command]
pub fn accept_pending_correction(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
    id: String,
) -> Result<(), String> {
    match core.accept_pending_correction(&id) {
        Ok(Some(_)) => {
            coord.refresh_vocab_suggestion_presentation(!core.pending_corrections().is_empty());
        }
        Ok(None) => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

/// X clicked on the card: drop this entry and record nothing (there is no reject list).
#[tauri::command]
pub fn reject_pending_correction(core: CoreState<'_>, coord: CoordinatorState<'_>, id: String) {
    if core.reject_pending_correction(&id) {
        coord.refresh_vocab_suggestion_presentation(!core.pending_corrections().is_empty());
    }
}

/// The card expired after 10 seconds, or a new dictation round started.
#[tauri::command]
pub fn dismiss_vocab_suggestions(core: CoreState<'_>, coord: CoordinatorState<'_>) {
    core.dismiss_pending_corrections();
    coord.refresh_vocab_suggestion_presentation(false);
}

/// "Copy" clicked on the insertion-failure fallback card.
///
/// **Goes through the backend, not the frontend's `navigator.clipboard`**: the card floats
/// above another app and the button deliberately `preventDefault`s so it never takes focus
/// (taking focus would destroy the cursor where the user is writing), and calling
/// `navigator.clipboard.writeText` from an unfocused document throws
/// `Document is not focused`.
#[tauri::command]
pub fn copy_text_to_clipboard(text: String) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    crate::insertion::copy_text_to_clipboard(&text)
}

/// The fallback card closed itself (user clicked close / TTL expired).
#[tauri::command]
pub fn dismiss_insert_fallback_card(coord: CoordinatorState<'_>) {
    coord.dismiss_insert_fallback_card();
}

/// The frontend reports the card height from real line-wrapping; presentation_id ignores late
/// ResizeObserver callbacks from stale components.
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
