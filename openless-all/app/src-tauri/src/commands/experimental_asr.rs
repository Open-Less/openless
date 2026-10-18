use super::*;
use openless_core::experimental_asr::{
    ExperimentalAsrModel, ExperimentalAsrPhase, ExperimentalAsrService, ExperimentalAsrStatus,
};

fn service(core: &CoreState<'_>) -> Result<Arc<ExperimentalAsrService>, String> {
    core.services()
        .experimental_asr
        .clone()
        .ok_or_else(|| "Experimental local ASR is unavailable on this host".into())
}
#[tauri::command]
pub async fn experimental_asr_status(
    core: CoreState<'_>,
    window: Window,
) -> Result<Vec<ExperimentalAsrStatus>, String> {
    ensure_main_window(&window)?;
    if let Some(service) = &core.services().experimental_asr {
        return service.status().map_err(|e| e.to_string());
    }
    Ok(ExperimentalAsrModel::ALL
        .into_iter()
        .map(|model| ExperimentalAsrStatus {
            model,
            enabled: false,
            phase: ExperimentalAsrPhase::Disabled,
            available: false,
            unavailable_reason: Some("desktopRuntimeRequired".into()),
            downloaded_bytes: 0,
            total_bytes: model.files().iter().map(|f| f.size_bytes).sum(),
            channel_id: None,
            needs_guidance: false,
            error: None,
        })
        .collect())
}
#[tauri::command]
pub async fn experimental_asr_set_enabled(
    core: CoreState<'_>,
    window: Window,
    model: ExperimentalAsrModel,
    enabled: bool,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    service(&core)?
        .set_enabled(model, enabled)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn experimental_asr_delete(
    core: CoreState<'_>,
    window: Window,
    model: ExperimentalAsrModel,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    service(&core)?
        .delete_model(model)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn experimental_asr_acknowledge(
    core: CoreState<'_>,
    window: Window,
    model: ExperimentalAsrModel,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    service(&core)?
        .acknowledge(model)
        .map_err(|e| e.to_string())
}
