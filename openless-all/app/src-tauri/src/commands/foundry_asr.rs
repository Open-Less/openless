use super::*;

use openless_core::{FoundryRuntimeSource, LocalAsrRuntime, LocalAsrTarget};

fn core_error(error: openless_core::BackendError) -> String {
    error.to_string()
}

fn foundry_target(model_alias: String) -> Result<LocalAsrTarget, String> {
    LocalAsrTarget::parse(LocalAsrRuntime::Foundry, model_alias).map_err(core_error)
}

pub(crate) fn active_foundry_model_from_prefs(prefs: &UserPreferences) -> String {
    LocalAsrTarget::parse(
        LocalAsrRuntime::Foundry,
        prefs.foundry_local_asr_model.clone(),
    )
    .map(|target| target.model_id().to_string())
    .unwrap_or_else(|_| LocalAsrRuntime::Foundry.default_model().to_string())
}

pub(crate) fn validate_foundry_model_alias(model_alias: &str) -> Result<(), String> {
    LocalAsrTarget::parse(LocalAsrRuntime::Foundry, model_alias)
        .map(|_| ())
        .map_err(core_error)
}

pub(crate) fn normalize_foundry_language_hint(language_hint: &str) -> Result<String, String> {
    openless_core::normalize_foundry_language_hint(language_hint).map_err(core_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundryStatusWire {
    pub provider_id: String,
    pub available: bool,
    pub runtime_ready: bool,
    pub runtime_source: String,
    pub active_model: String,
    pub loaded_model_id: Option<String>,
    pub endpoint: Option<String>,
    pub error: Option<String>,
}

impl From<openless_core::LocalAsrRuntimeStatus> for FoundryStatusWire {
    fn from(status: openless_core::LocalAsrRuntimeStatus) -> Self {
        Self {
            provider_id: status.provider_id,
            available: status.available,
            runtime_ready: status.loaded,
            runtime_source: status.runtime_source.unwrap_or_default().as_str().into(),
            active_model: status.active_model,
            loaded_model_id: status.model_id,
            endpoint: status.endpoint,
            error: status.error,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundryCatalogWire {
    pub alias: String,
    pub display_name: String,
    pub cached: bool,
    pub file_size_mb: Option<u64>,
}

impl From<openless_core::LocalAsrModel> for FoundryCatalogWire {
    fn from(model: openless_core::LocalAsrModel) -> Self {
        Self {
            alias: model.target.model_id().to_string(),
            display_name: model.display_name,
            cached: model.installed,
            file_size_mb: model.size_bytes.map(|bytes| bytes / (1024 * 1024)),
        }
    }
}

#[tauri::command]
pub async fn foundry_local_asr_status(backend: CoreState<'_>) -> Result<FoundryStatusWire, String> {
    backend
        .services()
        .local_asr
        .runtime_status(LocalAsrRuntime::Foundry)
        .await
        .map(FoundryStatusWire::from)
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_catalog(
    backend: CoreState<'_>,
) -> Result<Vec<FoundryCatalogWire>, String> {
    backend
        .services()
        .local_asr
        .list_models(LocalAsrRuntime::Foundry)
        .await
        .map(|models| models.into_iter().map(FoundryCatalogWire::from).collect())
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_set_model(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_active_model(foundry_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_set_language_hint(
    backend: CoreState<'_>,
    language_hint: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_language_hint(LocalAsrRuntime::Foundry, language_hint)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_set_runtime_source(
    backend: CoreState<'_>,
    source: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .set_foundry_runtime_source(FoundryRuntimeSource::from_legacy(&source))
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_prepare(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<String, String> {
    backend
        .services()
        .local_asr
        .prepare(foundry_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_cancel_prepare(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .cancel_prepare(LocalAsrRuntime::Foundry)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_release(backend: CoreState<'_>) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::Foundry)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_model_dir(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<String, String> {
    backend
        .services()
        .local_asr
        .model_dir(foundry_target(model_alias)?)
        .await
        .map(|path| path.display().to_string())
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_delete_model(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    backend
        .services()
        .local_asr
        .delete_model(foundry_target(model_alias)?)
        .await
        .map_err(core_error)
}

#[tauri::command]
pub async fn foundry_local_asr_reveal_model_dir(
    backend: CoreState<'_>,
    model_alias: String,
) -> Result<(), String> {
    let dir = backend
        .services()
        .local_asr
        .model_dir(foundry_target(model_alias)?)
        .await
        .map_err(core_error)?;
    open_path_in_file_manager(&dir)
}

#[cfg(test)]
mod wire_contract_tests {
    use super::*;

    #[test]
    fn foundry_status_keeps_the_legacy_react_wire_shape() {
        let status = FoundryStatusWire::from(openless_core::LocalAsrRuntimeStatus {
            runtime: LocalAsrRuntime::Foundry,
            provider_id: "foundry-local-whisper".into(),
            available: true,
            loaded: false,
            active_model: "whisper-small".into(),
            model_id: None,
            keep_loaded_secs: 300,
            runtime_source: Some(FoundryRuntimeSource::OrtNightly),
            endpoint: None,
            operation: None,
            error: None,
            last_error: None,
            last_prepare_ms: None,
            last_transcribe_ms: None,
            last_audio_ms: None,
        });

        let value = serde_json::to_value(status).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "providerId": "foundry-local-whisper",
                "available": true,
                "runtimeReady": false,
                "runtimeSource": "ort-nightly",
                "activeModel": "whisper-small",
                "loadedModelId": null,
                "endpoint": null,
                "error": null,
            })
        );
    }
}
