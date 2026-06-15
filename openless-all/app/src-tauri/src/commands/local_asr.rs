use super::*;

use crate::asr::local::{
    download::{fetch_remote_info, RemoteInfo},
    DownloadManager, ModelId, ModelStatus, PROVIDER_ID as LOCAL_PROVIDER_ID,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrSettings {
    pub provider_id: String,
    pub active_model: String,
    pub mirror: String,
    pub models_base_dir: Option<String>,
    pub models_root_dir: String,
    /// macOS 才编入引擎；Windows 端 UI 需要据此把"开始下载"按钮灰掉。
    pub engine_available: bool,
}

#[tauri::command]
pub fn local_asr_get_settings(coord: CoordinatorState<'_>) -> LocalAsrSettings {
    let prefs = coord.prefs().get();
    let models_base_dir = non_empty_string(prefs.local_asr_models_base_dir.clone());
    let models_root_dir = crate::persistence::models_root_for_base_dir(models_base_dir.as_deref())
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    LocalAsrSettings {
        provider_id: LOCAL_PROVIDER_ID.into(),
        active_model: prefs.local_asr_active_model,
        mirror: prefs.local_asr_mirror,
        models_base_dir,
        models_root_dir,
        engine_available: cfg!(target_os = "macos"),
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrStorageSettings {
    pub models_base_dir: Option<String>,
    pub models_root_dir: String,
    pub is_default: bool,
}

#[tauri::command]
pub fn local_asr_storage_settings(
    coord: CoordinatorState<'_>,
) -> Result<LocalAsrStorageSettings, String> {
    let prefs = coord.prefs().get();
    let models_base_dir = non_empty_string(prefs.local_asr_models_base_dir);
    let models_root_dir = crate::persistence::models_root_for_base_dir(models_base_dir.as_deref())
        .map_err(|e| format!("{e:#}"))?
        .display()
        .to_string();
    Ok(LocalAsrStorageSettings {
        is_default: models_base_dir.is_none(),
        models_base_dir,
        models_root_dir,
    })
}

#[tauri::command]
pub async fn local_asr_set_models_base_dir(
    coord: CoordinatorState<'_>,
    qwen_manager: State<'_, Arc<DownloadManager>>,
    foundry_runtime: State<'_, Arc<FoundryLocalRuntime>>,
    sherpa_manager: State<'_, Arc<SherpaDownloadManager>>,
    sherpa_runtime: State<'_, Arc<SherpaOnnxRuntime>>,
    models_base_dir: Option<String>,
) -> Result<LocalAsrStorageSettings, String> {
    let next_base_dir = models_base_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let new_root = crate::persistence::validate_models_base_dir(next_base_dir.as_deref())
        .map_err(|e| format!("{e:#}"))?;

    let prefs = coord.prefs().get();
    let current_base_dir = non_empty_string(prefs.local_asr_models_base_dir.clone());
    let old_root = crate::persistence::models_root_for_base_dir(current_base_dir.as_deref())
        .map_err(|e| format!("{e:#}"))?;
    let same_root = same_path_for_command(&old_root, &new_root);
    if same_root && current_base_dir == next_base_dir {
        return local_asr_storage_settings(coord);
    }
    if !same_root && foundry_runtime.storage_configuration_locked() {
        return Err(
            "Foundry Local has already been initialized in this app session; restart OpenLess before changing the model storage location"
                .to_string(),
        );
    }

    quiesce_local_asr_storage_users(
        coord.inner(),
        qwen_manager.inner(),
        foundry_runtime.inner(),
        sherpa_manager.inner(),
        sherpa_runtime.inner(),
    )
    .await?;
    crate::persistence::migrate_models_root(&old_root, &new_root).map_err(|e| format!("{e:#}"))?;

    let mut prefs = prefs;
    prefs.local_asr_models_base_dir = next_base_dir.clone().unwrap_or_default();
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    local_asr_storage_settings(coord)
}

fn same_path_for_command(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

async fn quiesce_local_asr_storage_users(
    coord: &Arc<Coordinator>,
    qwen_manager: &Arc<DownloadManager>,
    foundry_runtime: &Arc<FoundryLocalRuntime>,
    sherpa_manager: &Arc<SherpaDownloadManager>,
    sherpa_runtime: &Arc<SherpaOnnxRuntime>,
) -> Result<(), String> {
    for model_id in ModelId::all() {
        qwen_manager.cancel(*model_id);
    }
    for model in crate::asr::local::sherpa::MODELS {
        sherpa_manager.cancel(model.alias);
    }
    foundry_runtime.request_cancel_prepare();
    sherpa_runtime.request_cancel_prepare();
    coord.release_local_asr_engine();
    foundry_runtime
        .release_now()
        .await
        .map_err(|e| format!("{e:#}"))?;
    sherpa_runtime
        .release_now()
        .await
        .map_err(|e| format!("{e:#}"))?;

    for _ in 0..50 {
        let qwen_active = ModelId::all()
            .iter()
            .any(|model_id| qwen_manager.is_active(*model_id));
        let sherpa_active = crate::asr::local::sherpa::MODELS
            .iter()
            .any(|model| sherpa_manager.is_active(model.alias));
        if !qwen_active && !sherpa_active {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("local ASR downloads are still stopping; retry after cancellation finishes".to_string())
}

#[tauri::command]
pub fn local_asr_set_active_model(
    coord: CoordinatorState<'_>,
    model_id: String,
) -> Result<(), String> {
    if ModelId::from_str(&model_id).is_none() {
        return Err(format!("unknown model id: {model_id}"));
    }
    let mut prefs = coord.prefs().get();
    prefs.local_asr_active_model = model_id;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_asr_set_mirror(coord: CoordinatorState<'_>, mirror: String) -> Result<(), String> {
    let _normalized = Mirror::from_str(&mirror);
    let mut prefs = coord.prefs().get();
    prefs.local_asr_mirror = mirror;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_asr_list_models() -> Vec<ModelStatus> {
    crate::asr::local::models::list_status()
}

/// 实时去 HuggingFace API 拉某个模型的真实文件清单 + 总尺寸；
/// 前端在显示模型卡时调一次，避免硬编码尺寸过期。
#[tauri::command]
pub async fn local_asr_fetch_remote_info(
    model_id: String,
    mirror: Option<String>,
) -> Result<RemoteInfo, String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    let m = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    fetch_remote_info(id, m).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn local_asr_download_model(
    app: AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
    model_id: String,
    mirror: Option<String>,
) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    let m = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    manager.start(app, id, m);
    Ok(())
}

#[tauri::command]
pub fn local_asr_cancel_download(
    manager: State<'_, Arc<DownloadManager>>,
    model_id: String,
) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    manager.cancel(id);
    Ok(())
}

#[tauri::command]
pub fn local_asr_delete_model(coord: CoordinatorState<'_>, model_id: String) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    // 如果内存里加载的就是要删的这个模型，先释放：否则 mmap 残留指向已 unlink 的文件，
    // 且 RAM 直到下次切模型 / 用户手动按"释放"才回收。
    if coord.local_asr_loaded_model().as_deref() == Some(id.as_str()) {
        coord.release_local_asr_engine();
    }
    crate::asr::local::models::delete_model(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_asr_model_dir(model_id: String) -> Result<String, String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    crate::asr::local::models::model_dir(id)
        .map(|path| path.display().to_string())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn local_asr_reveal_model_dir(model_id: String) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    let dir = crate::asr::local::models::model_dir(id).map_err(|e| format!("{e:#}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {} failed: {e}", dir.display()))?;
    open_path_in_file_manager(&dir)
}

#[tauri::command]
pub fn local_asr_reveal_models_root(coord: CoordinatorState<'_>) -> Result<(), String> {
    let prefs = coord.prefs().get();
    let base_dir = non_empty_string(prefs.local_asr_models_base_dir);
    let dir = crate::persistence::models_root_for_base_dir(base_dir.as_deref())
        .map_err(|e| format!("{e:#}"))?;
    open_path_in_file_manager(&dir)
}

#[tauri::command]
pub async fn local_asr_test_model(
    model_id: String,
) -> Result<crate::asr::local::test_run::TestResult, String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    crate::asr::local::test_run::run_test(id)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrEngineStatus {
    pub loaded: bool,
    pub model_id: Option<String>,
    pub keep_loaded_secs: u32,
}

#[tauri::command]
pub fn local_asr_engine_status(coord: CoordinatorState<'_>) -> LocalAsrEngineStatus {
    let prefs = coord.prefs().get();
    LocalAsrEngineStatus {
        loaded: coord.local_asr_loaded_model().is_some(),
        model_id: coord.local_asr_loaded_model(),
        keep_loaded_secs: prefs.local_asr_keep_loaded_secs,
    }
}

#[tauri::command]
pub fn local_asr_release_engine(coord: CoordinatorState<'_>) {
    coord.release_local_asr_engine();
}

#[tauri::command]
pub fn local_asr_preload(coord: tauri::State<'_, std::sync::Arc<crate::coordinator::Coordinator>>) {
    coord.preload_local_asr_in_background();
}

#[tauri::command]
pub fn local_asr_set_keep_loaded_secs(
    coord: CoordinatorState<'_>,
    seconds: u32,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    prefs.local_asr_keep_loaded_secs = seconds;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.emit_local_asr_engine_status();
    Ok(())
}
