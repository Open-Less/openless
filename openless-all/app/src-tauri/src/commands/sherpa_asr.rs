use super::*;

pub(crate) fn active_sherpa_model_from_prefs(prefs: &UserPreferences) -> String {
    if sherpa_model_alias_is_known(&prefs.sherpa_onnx_model) {
        prefs.sherpa_onnx_model.clone()
    } else {
        SHERPA_DEFAULT_MODEL_ALIAS.to_string()
    }
}

pub(crate) fn validate_sherpa_model_alias(model_alias: &str) -> Result<(), String> {
    if sherpa_model_alias_is_known(model_alias) {
        Ok(())
    } else {
        Err(format!("unknown sherpa-onnx model alias: {model_alias}"))
    }
}

pub(crate) fn normalize_sherpa_language_hint(language_hint: &str) -> Result<String, String> {
    let normalized = language_hint.trim().to_lowercase();
    if normalized.is_empty()
        || normalized
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '-')
    {
        Ok(normalized)
    } else {
        Err("language hint must be empty or BCP-47 lowercase code".to_string())
    }
}

#[tauri::command]
pub async fn sherpa_onnx_asr_status(
    coord: CoordinatorState<'_>,
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
) -> Result<SherpaRuntimeStatus, String> {
    let prefs = coord.prefs().get();
    let active_model = active_sherpa_model_from_prefs(&prefs);
    Ok(runtime.status_snapshot(&active_model).await)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_catalog(
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
) -> Result<Vec<SherpaCatalogModel>, String> {
    runtime
        .catalog_snapshot()
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn sherpa_onnx_asr_fetch_remote_info(
    model_alias: String,
    mirror: Option<String>,
) -> Result<SherpaRemoteInfo, String> {
    validate_sherpa_model_alias(&model_alias)?;
    let mirror = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    fetch_sherpa_remote_info(&model_alias, mirror)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn sherpa_onnx_asr_download_model(
    app: AppHandle,
    manager: State<'_, Arc<SherpaDownloadManager>>,
    model_alias: String,
    mirror: Option<String>,
) -> Result<(), String> {
    validate_sherpa_model_alias(&model_alias)?;
    let mirror = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    manager.start(app, model_alias, mirror);
    Ok(())
}

#[tauri::command]
pub fn sherpa_onnx_asr_cancel_download(
    manager: State<'_, Arc<SherpaDownloadManager>>,
    model_alias: String,
) -> Result<(), String> {
    validate_sherpa_model_alias(&model_alias)?;
    manager.cancel(&model_alias);
    Ok(())
}

#[tauri::command]
pub fn sherpa_onnx_asr_set_model(
    coord: CoordinatorState<'_>,
    model_alias: String,
) -> Result<(), String> {
    validate_sherpa_model_alias(&model_alias)?;
    let mut prefs = coord.prefs().get();
    if prefs.sherpa_onnx_model == model_alias {
        return Ok(());
    }
    prefs.sherpa_onnx_model = model_alias;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sherpa_onnx_asr_set_language_hint(
    coord: CoordinatorState<'_>,
    language_hint: String,
) -> Result<(), String> {
    let normalized = normalize_sherpa_language_hint(&language_hint)?;
    let mut prefs = coord.prefs().get();
    if prefs.sherpa_onnx_language_hint == normalized {
        return Ok(());
    }
    prefs.sherpa_onnx_language_hint = normalized;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sherpa_onnx_asr_prepare(
    app: AppHandle,
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
    model_alias: String,
) -> Result<String, String> {
    validate_sherpa_model_alias(&model_alias)?;
    let progress_app = app.clone();
    let result = runtime
        .ensure_loaded_with_progress(&model_alias, move |payload| {
            emit_sherpa_prepare_progress(&progress_app, payload);
        })
        .await;
    match result {
        Ok(loaded) => Ok(loaded),
        Err(error) => {
            let message = format!("{error:#}");
            emit_sherpa_prepare_progress(
                &app,
                SherpaPrepareProgressPayload::failed(
                    model_alias,
                    "sherpa-onnx prepare failed",
                    message.clone(),
                ),
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub fn sherpa_onnx_asr_cancel_prepare(
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
) -> Result<(), String> {
    runtime.request_cancel_prepare();
    Ok(())
}

#[tauri::command]
pub async fn sherpa_onnx_asr_release(
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
) -> Result<(), String> {
    runtime.release_now().await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn sherpa_onnx_asr_model_dir(model_alias: String) -> Result<String, String> {
    validate_sherpa_model_alias(&model_alias)?;
    SherpaOnnxRuntime::model_dir_for_alias(&model_alias)
        .map(|path| path.display().to_string())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn sherpa_onnx_asr_delete_model(
    runtime: State<'_, Arc<SherpaOnnxRuntime>>,
    model_alias: String,
) -> Result<(), String> {
    validate_sherpa_model_alias(&model_alias)?;
    runtime
        .delete_model(&model_alias)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn sherpa_onnx_asr_reveal_model_dir(model_alias: String) -> Result<(), String> {
    validate_sherpa_model_alias(&model_alias)?;
    let dir = SherpaOnnxRuntime::model_dir_for_alias(&model_alias).map_err(|e| format!("{e:#}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {} failed: {e}", dir.display()))?;
    open_path_in_file_manager(&dir)
}

fn emit_sherpa_prepare_progress(app: &AppHandle, payload: SherpaPrepareProgressPayload) {
    if let Err(error) = app.emit("sherpa-onnx-asr-prepare-progress", payload) {
        log::warn!("[sherpa-asr] emit prepare progress failed: {error}");
    }
}
