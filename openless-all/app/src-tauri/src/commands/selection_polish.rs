use super::*;

/// Development-only entry point for exercising the selection-polish workflow.
#[tauri::command]
pub async fn run_selection_polish_for_dev(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.trigger_selection_polish_for_dev().await
}
