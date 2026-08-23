use super::*;

#[tauri::command]
pub fn get_selection_voice_preview(
    coord: CoordinatorState<'_>,
) -> Option<crate::coordinator::selection_voice_session::SelectionVoicePreviewPayload> {
    coord.selection_voice_preview()
}

#[tauri::command]
pub fn confirm_selection_voice_preview(
    coord: CoordinatorState<'_>,
    text: String,
) -> Result<(), String> {
    coord.confirm_selection_voice_preview(text)
}

#[tauri::command]
pub fn cancel_selection_voice_preview(coord: CoordinatorState<'_>) {
    coord.cancel_selection_voice_preview();
}

#[tauri::command]
pub fn set_selection_voice_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    let previous = coord.prefs().get();
    let mut next = previous.clone();
    next.selection_voice_hotkey = binding;
    reject_hotkey_collisions(&next)?;
    coord.prefs().set(next).map_err(|e| e.to_string())?;
    if let Err(error) = coord.try_update_selection_voice_hotkey_binding() {
        if let Err(rollback_error) = coord.prefs().set(previous) {
            return Err(format!(
                "{error}; additionally failed to restore previous Selection Voice shortcut: {rollback_error}"
            ));
        }
        coord.update_selection_voice_hotkey_binding();
        return Err(error);
    }
    Ok(())
}
