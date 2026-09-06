use super::*;

use openless_core::{LocalAsrMirror, LocalAsrRuntime, LocalAsrTarget};

fn core_error(error: openless_core::BackendError) -> String {
    error.to_string()
}

fn sherpa_target(model_alias: String) -> Result<LocalAsrTarget, String> {
    LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, model_alias).map_err(core_error)
}

fn parse_mirror(value: Option<String>) -> Option<LocalAsrMirror> {
    value.map(|value| LocalAsrMirror::from_legacy(&value))
}

pub(crate) fn active_sherpa_model_from_prefs(prefs: &UserPreferences) -> String {
    LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, prefs.sherpa_onnx_model.clone())
        .map(|target| target.model_id().to_string())
        .unwrap_or_else(|_| LocalAsrRuntime::SherpaOnnx.default_model().to_string())
}

pub(crate) fn validate_sherpa_model_alias(model_alias: &str) -> Result<(), String> {
    LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, model_alias)
        .map(|_| ())
        .map_err(core_error)
}

pub(crate) fn normalize_sherpa_language_hint(language_hint: &str) -> Result<String, String> {
    openless_core::normalize_sherpa_language_hint(language_hint).map_err(core_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaStatusWire {
    pub provider_id: String,
    pub available: bool,
    pub runtime_ready: bool,
    pub active_model: String,
    pub loaded_model_id: Option<String>,
    pub error: Option<String>,
    pub last_prepare_ms: Option<u64>,
    pub last_transcribe_ms: Option<u64>,
    pub last_audio_ms: Option<u64>,
    pub last_error: Option<String>,
}

impl From<openless_core::LocalAsrRuntimeStatus> for SherpaStatusWire {
    fn from(status: openless_core::LocalAsrRuntimeStatus) -> Self {
        Self {
            provider_id: status.provider_id,
            available: status.available,
            runtime_ready: status.loaded,
            active_model: status.active_model,
            loaded_model_id: status.model_id,
            error: status.error,
            last_prepare_ms: status.last_prepare_ms,
            last_transcribe_ms: status.last_transcribe_ms,
            last_audio_ms: status.last_audio_ms,
            last_error: status.last_error,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SherpaFamilyWire {
    SenseVoice,
    Paraformer,
    Whisper,
    Qwen3Asr,
    Zipformer,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SherpaModeWire {
    Offline,
    Online,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaCatalogWire {
    pub alias: String,
    pub display_name: String,
    pub family: SherpaFamilyWire,
    pub mode: SherpaModeWire,
    pub languages: Vec<String>,
    pub cached: bool,
    pub downloaded_bytes: u64,
    pub file_size_mb: Option<u64>,
}

impl TryFrom<openless_core::LocalAsrModel> for SherpaCatalogWire {
    type Error = String;

    fn try_from(model: openless_core::LocalAsrModel) -> Result<Self, Self::Error> {
        let family = match model.family.as_str() {
            "sense_voice" => SherpaFamilyWire::SenseVoice,
            "paraformer" => SherpaFamilyWire::Paraformer,
            "whisper" => SherpaFamilyWire::Whisper,
            "qwen3_asr" => SherpaFamilyWire::Qwen3Asr,
            "zipformer" => SherpaFamilyWire::Zipformer,
            family => return Err(format!("core returned unsupported Sherpa family: {family}")),
        };
        let mode = match model.mode.as_deref() {
            Some("online") => SherpaModeWire::Online,
            Some("offline") => SherpaModeWire::Offline,
            mode => return Err(format!("core returned unsupported Sherpa mode: {mode:?}")),
        };
        Ok(Self {
            alias: model.target.model_id().to_string(),
            display_name: model.display_name,
            family,
            mode,
            languages: model.languages,
            cached: model.installed,
            downloaded_bytes: model.downloaded_bytes,
            file_size_mb: model.size_bytes.map(|bytes| bytes / (1024 * 1024)),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaRemoteFileWire {
    pub path: String,
    pub local_path: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaRemoteInfoWire {
    pub model_alias: String,
    pub mirror: String,
    pub files: Vec<SherpaRemoteFileWire>,
    pub total_bytes: u64,
}

impl From<openless_core::LocalAsrRemoteInfo> for SherpaRemoteInfoWire {
    fn from(info: openless_core::LocalAsrRemoteInfo) -> Self {
        Self {
            model_alias: info.target.model_id().to_string(),
            mirror: info.mirror.as_str().into(),
            files: info
                .files
                .into_iter()
                .map(|file| SherpaRemoteFileWire {
                    path: file.path.clone(),
                    local_path: file.local_path.unwrap_or(file.path),
                    size: file.size_bytes,
                    sha256: file.sha256,
                })
                .collect(),
            total_bytes: info.total_bytes,
        }
    }
}

#[tauri::command]
pub async fn sherpa_onnx_asr_status(backend: CoreState<'_>) -> Result<SherpaStatusWire, String> {
    backend
        .services()
        .local_asr
        .runtime_status(LocalAsrRuntime::SherpaOnnx)
        .await
        .map(SherpaStatusWire::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_catalog(
    backend: CoreState<'_>,
) -> Result<Vec<SherpaCatalogWire>, String> {
    let models = backend
        .services()
        .local_asr
        .list_models(LocalAsrRuntime::SherpaOnnx)
        .await
        .map_err(core_error)?;
    models
        .into_iter()
        .map(SherpaCatalogWire::try_from)
        .collect()
}

#[tauri::command]
pub async fn sherpa_onnx_asr_fetch_remote_info(
    backend: CoreState<'_>,
    model_alias: String,
    mirror: Option<String>,
) -> Result<SherpaRemoteInfoWire, String> {
    backend
        .services()
        .local_asr
        .remote_info(sherpa_target(model_alias)?, parse_mirror(mirror))
        .await
        .map(SherpaRemoteInfoWire::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_download_model(
    backend: CoreState<'_>,
    model_alias: String,
    mirror: Option<String>,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .start_download(sherpa_target(model_alias)?, parse_mirror(mirror))
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_cancel_download(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .cancel_download(sherpa_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_set_model(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_active_model(sherpa_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_set_language_hint(
    backend: CoreState<'_>,
    language_hint: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_language_hint(LocalAsrRuntime::SherpaOnnx, language_hint)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_prepare(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<String, String> {
    backend
        .services()
        .local_asr
        .prepare(sherpa_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_cancel_prepare(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .cancel_prepare(LocalAsrRuntime::SherpaOnnx)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_release(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::SherpaOnnx)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_model_dir(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<String, String> {
    backend
        .services()
        .local_asr
        .model_dir(sherpa_target(model_alias)?)
        .await
        .map(|path| path.display().to_string())
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_delete_model(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .delete_model(sherpa_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn sherpa_onnx_asr_reveal_model_dir(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    let dir = backend
        .services()
        .local_asr
        .model_dir(sherpa_target(model_alias)?)
        .await
        .map_err(core_error)?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create {} failed: {error}", dir.display()))?;
    open_path_in_file_manager(&dir)
}

#[cfg(test)]
mod wire_contract_tests {
    use super::*;

    #[test]
    fn sherpa_catalog_keeps_family_mode_and_legacy_field_names() {
        let model = openless_core::LocalAsrModel {
            target: LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh")
                .unwrap(),
            display_name: "SenseVoice Small".into(),
            family: "sense_voice".into(),
            mode: Some("offline".into()),
            repository: Some("repo/model".into()),
            languages: vec!["zh".into(), "en".into()],
            installed: false,
            downloaded_bytes: 7,
            size_bytes: Some(230 * 1024 * 1024),
        };

        let value = serde_json::to_value(SherpaCatalogWire::try_from(model).unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "alias": "sense-voice-small-zh",
                "displayName": "SenseVoice Small",
                "family": "senseVoice",
                "mode": "offline",
                "languages": ["zh", "en"],
                "cached": false,
                "downloadedBytes": 7,
                "fileSizeMb": 230,
            })
        );
    }

    #[test]
    fn sherpa_catalog_rejects_unknown_core_values_without_panicking() {
        let model = openless_core::LocalAsrModel {
            target: LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh")
                .unwrap(),
            display_name: "Unexpected".into(),
            family: "future_family".into(),
            mode: Some("offline".into()),
            repository: None,
            languages: Vec::new(),
            installed: false,
            downloaded_bytes: 0,
            size_bytes: None,
        };

        let error = SherpaCatalogWire::try_from(model).unwrap_err();
        assert!(error.contains("unsupported Sherpa family"));
    }
}
