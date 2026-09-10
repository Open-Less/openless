use super::*;

/// Development-only entry point for exercising the selection-polish workflow.
#[tauri::command]
pub async fn run_selection_polish_for_dev(core: CoreState<'_>) -> Result<(), String> {
    core.services()
        .selection
        .begin_polish(openless_core::SelectionPolishRequest {
            selected_text: None,
            mode: PolishMode::Raw,
            instruction: None,
        })
        .await
        .map(|_| ())
        .map_err(|error| error.message)
}
