use super::CoordinatorState;

/// Opens the desktop Voice Edit panel or asks Android's main WebView to show
/// its embedded equivalent. The session starts only after the panel invokes
/// `start_voice_edit_session`.
#[tauri::command]
pub fn voice_edit_window_open(
    window: tauri::Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Voice Edit can only be opened from the main window".to_string());
    }
    coord.tauri_host().show_voice_edit();
    Ok(())
}

#[tauri::command]
pub fn voice_edit_window_close(
    window: tauri::Window,
    coord: CoordinatorState<'_>,
    session_id: Option<openless_core::SessionId>,
) -> Result<(), String> {
    if window.label() != "voice-edit" && window.label() != "main" {
        return Err("Voice Edit can only be closed from its panel".to_string());
    }
    coord.voice_edit_session_can_close(session_id)?;
    coord.tauri_host().hide_voice_edit();
    Ok(())
}

#[tauri::command]
pub async fn start_voice_edit_session(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.start_voice_edit_session().await
}
#[tauri::command]
pub async fn finalize_voice_edit_dictation(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_dictation(session_id).await
}
#[tauri::command]
pub async fn start_voice_edit_instruction(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.start_voice_edit_instruction(session_id).await
}
#[tauri::command]
pub async fn finalize_voice_edit_instruction(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_instruction(session_id).await
}
#[tauri::command]
pub async fn stop_voice_edit_instruction(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_instruction(session_id).await
}
#[tauri::command]
pub async fn commit_voice_edit_session(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.commit_voice_edit_session(session_id).await
}
#[tauri::command]
pub async fn commit_voice_edit(
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.commit_voice_edit_session(session_id).await
}
#[tauri::command]
pub async fn cancel_voice_edit_session(
    coord: CoordinatorState<'_>,
    session_id: Option<openless_core::SessionId>,
) -> Result<Option<openless_core::VoiceEditSnapshot>, String> {
    coord.cancel_voice_edit_session(session_id).await
}
#[tauri::command]
pub fn get_voice_edit_state(
    coord: CoordinatorState<'_>,
) -> Option<openless_core::VoiceEditSnapshot> {
    coord.voice_edit_session_snapshot()
}
