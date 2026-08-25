use super::*;
use crate::coordinator_state::SessionId;

#[tauri::command]
pub fn get_selection_voice_intent_prompt(
    coord: CoordinatorState<'_>,
) -> Option<crate::coordinator::selection_voice_session::SelectionVoiceIntentPromptPayload> {
    coord.selection_voice_intent_prompt()
}

#[tauri::command]
pub async fn confirm_selection_voice_intent_prompt(
    coord: CoordinatorState<'_>,
    intent: String,
) -> Result<(), String> {
    coord.confirm_selection_voice_intent_prompt(intent).await
}

#[tauri::command]
pub fn cancel_selection_voice_intent_prompt(coord: CoordinatorState<'_>) {
    coord.cancel_selection_voice_intent_prompt();
}

#[tauri::command]
pub fn get_selection_voice_preview(
    coord: CoordinatorState<'_>,
    qa_session_id: SessionId,
) -> Option<crate::coordinator::selection_voice_session::SelectionVoicePreviewPayload> {
    coord.selection_voice_preview(qa_session_id)
}

#[tauri::command]
pub fn confirm_selection_voice_preview(
    coord: CoordinatorState<'_>,
    text: String,
    qa_session_id: SessionId,
) -> Result<(), String> {
    coord.confirm_selection_voice_preview(text, Some(qa_session_id))
}

#[tauri::command]
pub fn revert_selection_voice_preview(
    coord: CoordinatorState<'_>,
    qa_session_id: SessionId,
) -> Result<(), String> {
    coord.revert_selection_voice_preview(qa_session_id)
}
