use super::*;
use crate::coordinator::selection_correction::{
    SelectionCorrectionAction, SelectionCorrectionBubblePayload,
};

#[tauri::command]
pub fn get_selection_correction(
    coord: CoordinatorState<'_>,
) -> Option<SelectionCorrectionBubblePayload> {
    coord.selection_correction_payload()
}

#[tauri::command]
pub async fn start_selection_correction(
    coord: CoordinatorState<'_>,
    action: SelectionCorrectionAction,
) -> Result<(), String> {
    coord.start_selection_correction(action).await
}

#[tauri::command]
pub async fn stop_selection_correction(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.stop_selection_correction().await
}

#[tauri::command]
pub fn cancel_selection_correction(coord: CoordinatorState<'_>) {
    coord.cancel_selection_correction();
}

#[tauri::command]
pub fn dismiss_selection_correction(coord: CoordinatorState<'_>) {
    coord.dismiss_selection_correction();
}
