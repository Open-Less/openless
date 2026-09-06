use super::*;
use crate::coordinator_state::SessionId;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoiceIntentPromptPayload {
    pub instruction: String,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoicePreviewPayload {
    pub text: String,
    pub source_text: String,
    pub summary: Option<String>,
}

fn selection_voice_error(error: openless_core::BackendError) -> String {
    use openless_core::BackendErrorCode;

    match error.code {
        BackendErrorCode::Busy => "selectionVoiceBusy".to_string(),
        BackendErrorCode::Cancelled => "selectionVoicePreviewUnavailable".to_string(),
        BackendErrorCode::InvalidArgument if error.message.contains("intent") => error
            .message
            .rsplit_once(':')
            .map(|(_, intent)| format!("selectionVoiceInvalidIntent:{}", intent.trim()))
            .unwrap_or_else(|| "selectionVoiceInvalidIntent".to_string()),
        BackendErrorCode::InvalidState if error.message.contains("intent prompt") => {
            "selectionVoiceIntentPromptUnavailable".to_string()
        }
        BackendErrorCode::InvalidState if error.message.contains("preview") => {
            "selectionVoicePreviewUnavailable".to_string()
        }
        _ => error.message,
    }
}

async fn qa_preview_owner(
    core: &openless_core::OpenLessBackend,
    qa_session_id: SessionId,
) -> Result<openless_core::SessionId, String> {
    let snapshot = core
        .services()
        .qa
        .snapshot()
        .await
        .map_err(|error| error.message)?;
    if snapshot.session_id.map(openless_core::SessionId::as_uuid) != Some(qa_session_id) {
        return Err("selectionVoicePreviewUnavailable".to_string());
    }
    snapshot
        .conversation_id
        .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())
}

#[tauri::command]
pub async fn get_selection_voice_intent_prompt(
    core: CoreState<'_>,
) -> Result<Option<SelectionVoiceIntentPromptPayload>, String> {
    let snapshot = core
        .services()
        .selection_voice
        .snapshot()
        .await
        .map_err(selection_voice_error)?;
    Ok(snapshot
        .intent_prompt
        .map(|prompt| SelectionVoiceIntentPromptPayload {
            instruction: prompt.instruction,
            source_text: prompt.source_text,
        }))
}

#[tauri::command]
pub async fn confirm_selection_voice_intent_prompt(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
    intent: String,
) -> Result<(), String> {
    let session_id = core
        .services()
        .selection_voice
        .snapshot()
        .await
        .map_err(selection_voice_error)?
        .intent_prompt
        .map(|prompt| prompt.session_id)
        .ok_or_else(|| "selectionVoiceIntentPromptUnavailable".to_string())?;
    let disposition = core
        .services()
        .selection_voice
        .confirm_intent(session_id, intent)
        .await
        .map_err(selection_voice_error)?;
    coord
        .continue_confirmed_selection_voice_intent(session_id, disposition)
        .await
}

#[tauri::command]
pub async fn cancel_selection_voice_intent_prompt(core: CoreState<'_>) -> Result<(), String> {
    let Some(session_id) = core
        .services()
        .selection_voice
        .snapshot()
        .await
        .map_err(selection_voice_error)?
        .session_id
    else {
        // 没有当时的 generation 就没有资源可取消；不能让一次迟到
        // 的无目标请求在 await 之后清掉用户刚开启的新会话。
        return Ok(());
    };
    core.services()
        .selection_voice
        .cancel(Some(session_id))
        .await
        .map_err(selection_voice_error)
}

#[tauri::command]
pub async fn get_selection_voice_preview(
    core: CoreState<'_>,
    qa_session_id: SessionId,
) -> Result<Option<SelectionVoicePreviewPayload>, String> {
    let owner_session_id = qa_preview_owner(&core, qa_session_id).await?;
    let preview = core
        .services()
        .selection_voice
        .preview(Some(owner_session_id))
        .await
        .map_err(selection_voice_error)?;
    Ok(preview.map(|preview| SelectionVoicePreviewPayload {
        text: preview.text,
        source_text: preview.source_text,
        summary: preview.summary,
    }))
}

#[tauri::command]
pub async fn confirm_selection_voice_preview(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
    text: String,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let owner_session_id = qa_preview_owner(&core, qa_session_id).await?;
    let ticket = core
        .services()
        .selection_voice
        .begin_preview_apply(Some(owner_session_id), text)
        .await
        .map_err(selection_voice_error)?;
    let outcome = match coord.apply_selection_voice_preview_ticket(&ticket) {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = core
                .services()
                .selection_voice
                .finish_preview_apply(
                    ticket.ticket_id,
                    openless_core::SelectionVoiceApplyOutcome::Failed,
                )
                .await;
            return Err(error);
        }
    };
    core.services()
        .selection_voice
        .finish_preview_apply(ticket.ticket_id, outcome)
        .await
        .map_err(selection_voice_error)?;
    coord.finish_selection_voice_preview_host(ticket.session_id);
    core.services()
        .qa
        .dismiss()
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
pub async fn revert_selection_voice_preview(
    core: CoreState<'_>,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let owner_session_id = qa_preview_owner(&core, qa_session_id).await?;
    let owner = Some(owner_session_id);
    core.services()
        .selection_voice
        .revert_preview(owner)
        .await
        .map_err(|error| {
            if error.code == openless_core::BackendErrorCode::InvalidState {
                "selectionVoiceRevertUnavailable".to_string()
            } else {
                selection_voice_error(error)
            }
        })?;
    let text = core
        .services()
        .selection_voice
        .preview(owner)
        .await
        .map_err(selection_voice_error)?
        .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())?
        .text;
    core.services()
        .qa
        .replace_last_answer(text, false)
        .await
        .map_err(|error| error.message)
}

#[cfg(test)]
mod tests {
    use super::{SelectionVoiceIntentPromptPayload, SelectionVoicePreviewPayload};

    #[test]
    fn preview_payload_wire_fixture_is_stable() {
        let payload = SelectionVoicePreviewPayload {
            text: "preview".into(),
            source_text: "source".into(),
            summary: Some("summary".into()),
        };
        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({
                "text": "preview",
                "sourceText": "source",
                "summary": "summary"
            })
        );
    }

    #[test]
    fn intent_prompt_payload_wire_fixture_is_stable() {
        let payload = SelectionVoiceIntentPromptPayload {
            instruction: "instruction".into(),
            source_text: "source".into(),
        };
        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({
                "instruction": "instruction",
                "sourceText": "source"
            })
        );
    }
}
