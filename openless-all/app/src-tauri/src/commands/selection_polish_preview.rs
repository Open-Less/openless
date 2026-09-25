use super::*;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPolishPreviewPayload {
    pub text: String,
    pub source_text: String,
}

fn map_selection_preview(
    snapshot: openless_core::SelectionSnapshot,
) -> Option<SelectionPolishPreviewPayload> {
    match (snapshot.preview_text, snapshot.source_text) {
        (Some(text), Some(source_text)) => {
            Some(SelectionPolishPreviewPayload { text, source_text })
        }
        _ => None,
    }
}

#[tauri::command]
pub async fn get_selection_polish_preview(
    core: CoreState<'_>,
) -> Result<Option<SelectionPolishPreviewPayload>, String> {
    // 面板懒创建后会自己拉一次；只有后端确实处在「润色结果」模式时才给负载，
    // 免把残留的选区快照当成新请求（面板会被意外切到润色模式）。
    if !crate::selection_polish_preview_pending() {
        return Ok(None);
    }
    let snapshot = core
        .services()
        .selection
        .snapshot()
        .await
        .map_err(|error| error.message)?;
    Ok(map_selection_preview(snapshot))
}

#[tauri::command]
pub async fn confirm_selection_polish_preview(
    core: CoreState<'_>,
    text: String,
) -> Result<(), String> {
    crate::clear_selection_polish_preview_pending();
    let snapshot = core
        .services()
        .selection
        .snapshot()
        .await
        .map_err(|error| error.message)?;
    let session_id = snapshot
        .session_id
        .ok_or_else(|| "selection preview is not active".to_string())?;
    core.services()
        .selection
        .confirm(session_id, Some(text))
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
pub async fn cancel_selection_polish_preview(core: CoreState<'_>) -> Result<(), String> {
    crate::clear_selection_polish_preview_pending();
    core.services()
        .selection
        .cancel(None)
        .await
        .map_err(|error| error.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_preview_wire_uses_legacy_camel_case_fields() {
        let payload = map_selection_preview(openless_core::SelectionSnapshot {
            phase: openless_core::SelectionPhase::Preview,
            session_id: Some(openless_core::SessionId::new()),
            source_text: Some("source".to_string()),
            preview_text: Some("preview".to_string()),
            instruction: None,
            insert_outcome: None,
            revert_outcome: None,
        })
        .expect("preview snapshot should map to the legacy payload");

        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({ "text": "preview", "sourceText": "source" })
        );
    }
}
