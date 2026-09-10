use super::*;

use std::path::PathBuf;

use openless_core::{LocalAsrMirror, LocalAsrRuntime, LocalAsrTarget};

fn core_error(error: openless_core::BackendError) -> String {
    error.to_string()
}

fn target(runtime: LocalAsrRuntime, model_id: String) -> Result<LocalAsrTarget, String> {
    LocalAsrTarget::parse(runtime, model_id).map_err(core_error)
}

fn parse_mirror(value: Option<String>) -> Option<LocalAsrMirror> {
    value.map(|value| LocalAsrMirror::from_legacy(&value))
}

fn display_path(path: PathBuf) -> String {
    path.display().to_string()
}

#[tauri::command]
pub async fn local_asr_activate(
    backend: CoreState<'_>,
    request: openless_core::LocalAsrActivationRequest,
) -> Result<openless_core::LocalAsrActivationResult, String> {
    backend
        .activate_local_asr(request)
        .await
        .map_err(core_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrSettings {
    pub provider_id: String,
    pub active_model: String,
    pub mirror: String,
    pub models_base_dir: Option<String>,
    pub models_root_dir: String,
    /// macOS/Linux 编入本地 Qwen3-ASR C 引擎；MLX 仅在 macOS 可用。
    pub engine_available: bool,
}

impl From<openless_core::LocalAsrSettings> for LocalAsrSettings {
    fn from(settings: openless_core::LocalAsrSettings) -> Self {
        Self {
            provider_id: settings.provider_id,
            active_model: settings.active_model,
            mirror: settings.mirror.as_str().into(),
            models_base_dir: settings.models_base_dir.map(display_path),
            models_root_dir: display_path(settings.models_root_dir),
            engine_available: settings.engine_available,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrStorageSettings {
    pub models_base_dir: Option<String>,
    pub models_root_dir: String,
    pub is_default: bool,
    pub restart_required: bool,
}

impl From<openless_core::LocalAsrStorageSettings> for LocalAsrStorageSettings {
    fn from(settings: openless_core::LocalAsrStorageSettings) -> Self {
        Self {
            models_base_dir: settings.models_base_dir.map(display_path),
            models_root_dir: display_path(settings.models_root_dir),
            is_default: settings.is_default,
            restart_required: settings.restart_required,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrModelStatus {
    pub id: String,
    pub hf_repo: String,
    pub downloaded_bytes: u64,
    pub is_downloaded: bool,
}

impl From<openless_core::LocalAsrModel> for LocalAsrModelStatus {
    fn from(model: openless_core::LocalAsrModel) -> Self {
        Self {
            id: model.target.model_id().to_string(),
            hf_repo: model.repository.unwrap_or_default(),
            downloaded_bytes: model.downloaded_bytes,
            is_downloaded: model.installed,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrRemoteFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrRemoteInfo {
    pub model_id: String,
    pub mirror: String,
    pub files: Vec<LocalAsrRemoteFile>,
    pub total_bytes: u64,
}

impl From<openless_core::LocalAsrRemoteInfo> for LocalAsrRemoteInfo {
    fn from(info: openless_core::LocalAsrRemoteInfo) -> Self {
        Self {
            model_id: info.target.model_id().to_string(),
            mirror: info.mirror.as_str().into(),
            files: info
                .files
                .into_iter()
                .map(|file| LocalAsrRemoteFile {
                    path: file.path,
                    size: file.size_bytes,
                })
                .collect(),
            total_bytes: info.total_bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelCard {
    pub model_id: String,
    pub mirror: String,
    pub downloads: u64,
    pub likes: u64,
    pub description: String,
}

impl From<openless_core::LocalAsrModelCard> for HfModelCard {
    fn from(card: openless_core::LocalAsrModelCard) -> Self {
        Self {
            model_id: card.target.model_id().to_string(),
            mirror: card.mirror.as_str().into(),
            downloads: card.downloads,
            likes: card.likes,
            description: card.description,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrTestResult {
    pub backend: String,
    pub model_id: String,
    pub expected_text: String,
    pub transcribed_text: String,
    pub audio_ms: u64,
    pub load_ms: u64,
    pub transcribe_ms: u64,
}

impl From<openless_core::LocalAsrTestResult> for LocalAsrTestResult {
    fn from(result: openless_core::LocalAsrTestResult) -> Self {
        Self {
            backend: result.backend,
            model_id: result.target.model_id().to_string(),
            expected_text: result.expected_text,
            transcribed_text: result.transcribed_text,
            audio_ms: result.audio_ms,
            load_ms: result.load_ms,
            transcribe_ms: result.transcribe_ms,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrEngineStatus {
    pub loaded: bool,
    pub model_id: Option<String>,
    pub keep_loaded_secs: u32,
}

impl From<openless_core::LocalAsrRuntimeStatus> for LocalAsrEngineStatus {
    fn from(status: openless_core::LocalAsrRuntimeStatus) -> Self {
        Self {
            loaded: status.loaded,
            model_id: status.model_id,
            keep_loaded_secs: status.keep_loaded_secs,
        }
    }
}

#[tauri::command]
pub async fn local_asr_get_settings(backend: CoreState<'_>) -> Result<LocalAsrSettings, String> {
    backend
        .services()
        .local_asr
        .settings(LocalAsrRuntime::Generic)
        .await
        .map(LocalAsrSettings::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_storage_settings(
    backend: CoreState<'_>,
) -> Result<LocalAsrStorageSettings, String> {
    backend
        .services()
        .local_asr
        .storage_settings()
        .await
        .map(LocalAsrStorageSettings::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_set_models_base_dir(
    backend: CoreState<'_>,
    models_base_dir: Option<String>,
) -> Result<LocalAsrStorageSettings, String> {
    let path = models_base_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    backend
        .services()
        .local_asr
        .set_models_base_dir(path)
        .await
        .map(LocalAsrStorageSettings::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_set_active_model(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_active_model(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_set_mirror(backend: CoreState<'_>, mirror: String) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_mirror(LocalAsrMirror::from_legacy(&mirror))
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_list_models(
    backend: CoreState<'_>,
) -> Result<Vec<LocalAsrModelStatus>, String> {
    backend
        .services()
        .local_asr
        .list_models(LocalAsrRuntime::Generic)
        .await
        .map(|models| models.into_iter().map(LocalAsrModelStatus::from).collect())
        .map_err(core_error)
}

/// 实时读取模型文件清单与总尺寸，避免前端硬编码远端元数据。
#[tauri::command]
pub async fn local_asr_fetch_remote_info(
    backend: CoreState<'_>,
    model_id: String,
    mirror: Option<String>,
) -> Result<LocalAsrRemoteInfo, String> {
    backend
        .services()
        .local_asr
        .remote_info(
            target(LocalAsrRuntime::Generic, model_id)?,
            parse_mirror(mirror),
        )
        .await
        .map(LocalAsrRemoteInfo::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_fetch_hf_card(
    backend: CoreState<'_>,
    model_id: String,
    mirror: Option<String>,
) -> Result<HfModelCard, String> {
    backend
        .services()
        .local_asr
        .model_card(
            target(LocalAsrRuntime::Generic, model_id)?,
            parse_mirror(mirror),
        )
        .await
        .map(HfModelCard::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_download_model(
    backend: CoreState<'_>,
    model_id: String,
    mirror: Option<String>,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .start_download(
            target(LocalAsrRuntime::Generic, model_id)?,
            parse_mirror(mirror),
        )
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_cancel_download(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .cancel_download(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_delete_model(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .delete_model(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_model_dir(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<String, String> {
    backend
        .services()
        .local_asr
        .model_dir(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map(display_path)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_reveal_model_dir(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<(), String> {
    let dir = backend
        .services()
        .local_asr
        .model_dir(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map_err(core_error)?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create {} failed: {error}", dir.display()))?;
    open_path_in_file_manager(&dir)
}

#[tauri::command]
pub async fn local_asr_reveal_models_root(backend: CoreState<'_>) -> Result<(), String> {
    let settings = backend
        .services()
        .local_asr
        .storage_settings()
        .await
        .map_err(core_error)?;
    open_path_in_file_manager(&settings.models_root_dir)
}

#[tauri::command]
pub async fn local_asr_test_model(
    backend: CoreState<'_>,
    model_id: String,
) -> Result<LocalAsrTestResult, String> {
    backend
        .services()
        .local_asr
        .test_model(target(LocalAsrRuntime::Generic, model_id)?)
        .await
        .map(LocalAsrTestResult::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_engine_status(
    backend: CoreState<'_>,
) -> Result<LocalAsrEngineStatus, String> {
    backend
        .services()
        .local_asr
        .runtime_status(LocalAsrRuntime::Generic)
        .await
        .map(LocalAsrEngineStatus::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_release_engine(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::Generic)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_preload(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn local_asr_set_keep_loaded_secs(
    backend: CoreState<'_>,
    seconds: u32,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_keep_loaded_secs(LocalAsrRuntime::Generic, seconds)
        .await
        .map_err(core_error)
}

#[cfg(test)]
mod wire_contract_tests {
    use super::*;

    #[test]
    fn generic_settings_keep_the_legacy_react_wire_shape() {
        let core = openless_core::LocalAsrSettings {
            runtime: openless_core::LocalAsrRuntime::Generic,
            provider_id: "local-qwen3".into(),
            active_model: "qwen3-asr-0.6b".into(),
            mirror: openless_core::LocalAsrMirror::HfMirror,
            models_base_dir: None,
            models_root_dir: std::path::PathBuf::from("C:/models"),
            engine_available: true,
            language_hint: None,
            runtime_source: None,
            keep_loaded_secs: 300,
        };

        let value = serde_json::to_value(LocalAsrSettings::from(core)).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "providerId": "local-qwen3",
                "activeModel": "qwen3-asr-0.6b",
                "mirror": "hf-mirror",
                "modelsBaseDir": null,
                "modelsRootDir": "C:/models",
                "engineAvailable": true,
            })
        );
    }
}
