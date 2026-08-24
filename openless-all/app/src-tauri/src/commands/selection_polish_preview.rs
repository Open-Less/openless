use super::*;
use crate::coordinator::selection_polish::SelectionPolishPreviewPayload;

#[tauri::command]
pub fn get_selection_polish_preview(
    coord: CoordinatorState<'_>,
) -> Option<SelectionPolishPreviewPayload> {
    coord.selection_polish_preview()
}

#[tauri::command]
pub async fn confirm_selection_polish_preview(
    coord: CoordinatorState<'_>,
    text: String,
) -> Result<(), String> {
    coord.confirm_selection_polish_preview(text).await
}

#[tauri::command]
pub fn cancel_selection_polish_preview(coord: CoordinatorState<'_>) {
    coord.cancel_selection_polish_preview();
}
