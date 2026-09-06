use super::*;

const LLM_EXTRA_HEADERS_ACCOUNT: &str = openless_core::credentials::LLM_EXTRA_HEADERS_ACCOUNT;
const LLM_TEMPERATURE_ACCOUNT: &str = openless_core::credentials::LLM_TEMPERATURE_ACCOUNT;
const OMNI_EXTRA_HEADERS_ACCOUNT: &str = openless_core::credentials::OMNI_EXTRA_HEADERS_ACCOUNT;
const OMNI_TEMPERATURE_ACCOUNT: &str = openless_core::credentials::OMNI_TEMPERATURE_ACCOUNT;
const MARKETPLACE_GITHUB_TOKEN_ACCOUNT: &str = "github.oauth_token";

/// Tauri host adapter for the framework-independent core credential port.
///
/// The implementation keeps the existing system vault format while Core's
/// [`openless_core::credentials::CredentialDirectory`] owns channel policy.
/// Secrets cross the boundary only as [`openless_core::SecretValue`].
pub(crate) struct SystemCredentialStore {
    model_store: Option<std::sync::Arc<openless_core::ModelStore>>,
    directory: openless_core::credentials::CredentialDirectory,
}

impl SystemCredentialStore {
    pub(crate) fn new(model_store: Option<std::sync::Arc<openless_core::ModelStore>>) -> Self {
        let metadata_store: std::sync::Arc<
            dyn openless_core::credentials::CredentialMetadataStore,
        > = std::sync::Arc::new(SystemCredentialMetadataStore);
        Self {
            model_store,
            directory: openless_core::credentials::CredentialDirectory::new(metadata_store),
        }
    }
}

struct SystemCredentialMetadataStore;

impl openless_core::credentials::CredentialMetadataStore for SystemCredentialMetadataStore {
    fn load_metadata(
        &self,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<openless_core::CredentialMetadata, openless_core::BackendError>,
    > {
        run_credential_task(|| Ok(CredentialsVault::load_metadata()))
    }

    fn save_metadata(
        &self,
        metadata: openless_core::CredentialMetadata,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        run_credential_task(move || {
            CredentialsVault::save_metadata(metadata).map_err(credential_persistence_error)
        })
    }

    fn channel_has_secrets(
        &self,
        kind: openless_core::ChannelKind,
        channel_id: String,
    ) -> futures_util::future::BoxFuture<'static, Result<bool, openless_core::BackendError>> {
        run_credential_task(move || Ok(CredentialsVault::channel_has_secrets(kind, &channel_id)))
    }
}

pub(crate) fn sync_active_asr_provider_to_vault(provider: &str) -> Result<(), String> {
    if CredentialsVault::get_active_asr() == provider {
        return Ok(());
    }
    CredentialsVault::set_active_asr_provider(provider).map_err(|error| error.to_string())
}

pub(crate) fn active_apple_speech_asr_is_supported(provider: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = provider;
        false
    }
}

pub(crate) fn active_foundry_asr_is_supported(provider: &str) -> bool {
    #[cfg(all(not(mobile), target_os = "windows"))]
    {
        provider == FOUNDRY_LOCAL_PROVIDER_ID
    }
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    {
        let _ = provider;
        false
    }
}

pub(crate) fn active_sherpa_asr_is_supported(provider: &str) -> bool {
    #[cfg(all(not(mobile), target_os = "windows"))]
    {
        provider == crate::asr::local::sherpa::PROVIDER_ID
    }
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    {
        let _ = provider;
        false
    }
}

impl openless_core::CredentialStore for SystemCredentialStore {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<CredentialsStatus, openless_core::BackendError>,
    > {
        let model_store = self.model_store.clone();
        run_credential_task(move || Ok(credentials_status(preferences, model_store.as_deref())))
    }

    fn read(
        &self,
        key: openless_core::CredentialKey,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<Option<openless_core::SecretValue>, openless_core::BackendError>,
    > {
        run_credential_task(move || {
            read_vault_credential(&key).map(|value| value.map(openless_core::SecretValue::new))
        })
    }

    fn write(
        &self,
        key: openless_core::CredentialKey,
        value: openless_core::SecretValue,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        run_credential_task(move || write_vault_credential(&key, value.expose_secret()))
    }

    fn remove(
        &self,
        key: openless_core::CredentialKey,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        run_credential_task(move || write_vault_credential(&key, ""))
    }

    fn list_channels(
        &self,
        kind: openless_core::ChannelKind,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<Vec<openless_core::ChannelSummary>, openless_core::BackendError>,
    > {
        let directory = self.directory.clone();
        Box::pin(async move { directory.list_channels(kind).await })
    }

    fn mutate_channel(
        &self,
        mutation: openless_core::ChannelMutation,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<openless_core::ChannelMutationResult, openless_core::BackendError>,
    > {
        let directory = self.directory.clone();
        Box::pin(async move { directory.mutate_channel(mutation).await })
    }

    fn active_provider(
        &self,
        slot: openless_core::ProviderSlot,
    ) -> futures_util::future::BoxFuture<'static, Result<String, openless_core::BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.active_provider(slot).await })
    }

    fn set_active_provider(
        &self,
        slot: openless_core::ProviderSlot,
        provider_id: String,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.set_active_provider(slot, provider_id).await })
    }
}

fn run_credential_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, openless_core::BackendError> + Send + 'static,
) -> futures_util::future::BoxFuture<'static, Result<T, openless_core::BackendError>> {
    Box::pin(async move {
        tauri::async_runtime::spawn_blocking(task)
            .await
            .map_err(|error| {
                openless_core::BackendError::new(
                    openless_core::BackendErrorCode::Internal,
                    format!("credential worker failed: {error}"),
                )
            })?
    })
}

fn credentials_status(
    preferences: UserPreferences,
    model_store: Option<&openless_core::ModelStore>,
) -> CredentialsStatus {
    let pipeline_mode = openless_core::shared_types::effective_pipeline_mode(
        preferences.multimodal_pipeline_enabled,
        preferences.pipeline_mode,
    );
    let snap = CredentialsVault::snapshot_for_pipeline(
        pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal,
    );
    let active_asr_provider = CredentialsVault::get_active_asr();
    let active_llm_provider = CredentialsVault::get_active_llm();
    let configuration = credential_configuration(
        &snap,
        &active_llm_provider,
        CodexOAuthCredentials::load_default().is_ok(),
    );
    let volcengine_configured =
        openless_core::provider_rules::volcengine_configured(&configuration);
    let asr_configured = openless_core::provider_rules::asr_configured(
        &active_asr_provider,
        &configuration,
        local_asr_configured(&active_asr_provider, model_store),
    );
    let llm_configured =
        openless_core::provider_rules::llm_configured(&active_llm_provider, &configuration);
    let omni_configured = pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal
        && openless_core::provider_rules::omni_configured(
            &snap.active_omni_provider,
            &configuration,
        );
    CredentialsStatus {
        active_asr_provider,
        active_llm_provider,
        pipeline_mode,
        asr_configured,
        llm_configured,
        omni_configured,
        volcengine_configured,
        ark_configured: llm_configured,
    }
}

fn read_vault_credential(
    key: &openless_core::CredentialKey,
) -> Result<Option<String>, openless_core::BackendError> {
    let result = match (key.namespace, key.account.as_str()) {
        (openless_core::CredentialNamespace::Llm, LLM_EXTRA_HEADERS_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => serde_json::to_string(
                    &CredentialsVault::get_llm_extra_headers_for_channel(provider),
                )
                .map(Some)
                .map_err(anyhow::Error::from),
                None => CredentialsVault::get_active_llm_extra_headers_json(),
            }
        }
        (openless_core::CredentialNamespace::Llm, LLM_TEMPERATURE_ACCOUNT) => {
            Ok(match key.provider_id.as_deref() {
                Some(provider) => CredentialsVault::get_llm_temperature_for_channel(provider)
                    .map(|value| value.to_string()),
                None => CredentialsVault::get_active_llm_temperature_string(),
            })
        }
        (openless_core::CredentialNamespace::Omni, OMNI_EXTRA_HEADERS_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::get_omni_extra_headers_json_for_provider(provider)
                }
                None => CredentialsVault::get_active_omni_extra_headers_json(),
            }
        }
        (openless_core::CredentialNamespace::Omni, OMNI_TEMPERATURE_ACCOUNT) => {
            Ok(match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::get_omni_temperature_string_for_provider(provider)
                }
                None => CredentialsVault::get_active_omni_temperature_string(),
            })
        }
        (openless_core::CredentialNamespace::Marketplace, MARKETPLACE_GITHUB_TOKEN_ACCOUNT) => {
            CredentialsVault::get_marketplace_github_token()
        }
        (openless_core::CredentialNamespace::Application, _) => {
            return Err(invalid_credential_key(key));
        }
        _ => {
            let account = parse_vault_account(key)?;
            if let Some(provider) = key.provider_id.as_deref() {
                match account_provider_kind(account) {
                    CredentialProviderKind::Asr => {
                        CredentialsVault::get_for_asr_provider(provider, account)
                    }
                    CredentialProviderKind::Llm => {
                        CredentialsVault::get_for_llm_provider(provider, account)
                    }
                    CredentialProviderKind::Omni => {
                        CredentialsVault::get_for_omni_provider(provider, account)
                    }
                }
            } else {
                CredentialsVault::get(account)
            }
        }
    };
    result.map_err(credential_persistence_error)
}

fn write_vault_credential(
    key: &openless_core::CredentialKey,
    value: &str,
) -> Result<(), openless_core::BackendError> {
    let result = match (key.namespace, key.account.as_str()) {
        (openless_core::CredentialNamespace::Llm, LLM_EXTRA_HEADERS_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::set_llm_extra_headers_json_for_provider(provider, value)
                }
                None => CredentialsVault::set_active_llm_extra_headers_json(value),
            }
        }
        (openless_core::CredentialNamespace::Llm, LLM_TEMPERATURE_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::set_llm_temperature_for_provider(provider, value)
                }
                None => CredentialsVault::set_active_llm_temperature(value),
            }
        }
        (openless_core::CredentialNamespace::Omni, OMNI_EXTRA_HEADERS_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::set_omni_extra_headers_json_for_provider(provider, value)
                }
                None => CredentialsVault::set_active_omni_extra_headers_json(value),
            }
        }
        (openless_core::CredentialNamespace::Omni, OMNI_TEMPERATURE_ACCOUNT) => {
            match key.provider_id.as_deref() {
                Some(provider) => {
                    CredentialsVault::set_omni_temperature_for_provider(provider, value)
                }
                None => CredentialsVault::set_active_omni_temperature(value),
            }
        }
        (openless_core::CredentialNamespace::Marketplace, MARKETPLACE_GITHUB_TOKEN_ACCOUNT) => {
            if value.trim().is_empty() {
                CredentialsVault::remove_marketplace_github_token()
            } else {
                CredentialsVault::set_marketplace_github_token(value)
            }
        }
        (openless_core::CredentialNamespace::Application, _) => {
            return Err(invalid_credential_key(key));
        }
        _ => {
            let account = parse_vault_account(key)?;
            if let Some(provider) = key.provider_id.as_deref() {
                match account_provider_kind(account) {
                    CredentialProviderKind::Asr => {
                        CredentialsVault::set_for_asr_provider(provider, account, value)
                    }
                    CredentialProviderKind::Llm => {
                        CredentialsVault::set_for_llm_provider(provider, account, value)
                    }
                    CredentialProviderKind::Omni => {
                        CredentialsVault::set_for_omni_provider(provider, account, value)
                    }
                }
            } else if value.is_empty() {
                CredentialsVault::remove(account)
            } else {
                CredentialsVault::set(account, value)
            }
        }
    };
    result.map_err(credential_persistence_error)
}

fn parse_vault_account(
    key: &openless_core::CredentialKey,
) -> Result<CredentialAccount, openless_core::BackendError> {
    let account = parse_account(&key.account).map_err(|_| invalid_credential_key(key))?;
    let expected_namespace = match account {
        CredentialAccount::ArkApiKey
        | CredentialAccount::ArkModelId
        | CredentialAccount::ArkEndpoint => openless_core::CredentialNamespace::Llm,
        CredentialAccount::OmniApiKey
        | CredentialAccount::OmniEndpoint
        | CredentialAccount::OmniModel => openless_core::CredentialNamespace::Omni,
        _ => openless_core::CredentialNamespace::Asr,
    };
    if key.namespace != expected_namespace {
        return Err(invalid_credential_key(key));
    }
    Ok(account)
}

fn invalid_credential_key(key: &openless_core::CredentialKey) -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::InvalidArgument,
        format!("unsupported credential account: {}", key.account),
    )
}

fn credential_persistence_error(error: anyhow::Error) -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::Persistence,
        format!("credential vault operation failed: {error}"),
    )
}

#[tauri::command]
pub async fn get_credentials(core: CoreState<'_>) -> Result<CredentialsStatus, String> {
    core.get_credentials_status()
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn asr_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    asr_configured_for_provider_with_model_store(provider, snap, None)
}

fn asr_configured_for_provider_with_model_store(
    provider: &str,
    snap: &CredentialsSnapshot,
    model_store: Option<&openless_core::ModelStore>,
) -> bool {
    openless_core::provider_rules::asr_configured(
        provider,
        &credential_configuration(snap, "", false),
        local_asr_configured(provider, model_store),
    )
}

fn local_asr_configured(
    provider: &str,
    model_store: Option<&openless_core::ModelStore>,
) -> Option<bool> {
    if crate::asr::local::is_local_whisper(provider) {
        #[cfg(target_os = "macos")]
        {
            let model_id = crate::persistence::PreferencesStore::new()
                .ok()
                .map(|store| store.get().local_whisper_active_model)
                .filter(|id| {
                    crate::asr::local::ModelId::from_wire_id(id)
                        .map(|model| model.is_whisper())
                        .unwrap_or(false)
                })
                .unwrap_or_else(|| crate::asr::local::WHISPER_MODEL_ID.to_string());
            return Some(model_store.is_some_and(|store| {
                crate::asr::local::whisper_model_ready_for_model(store, &model_id)
            }));
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Some(false);
        }
    }
    if crate::asr::local::is_local_qwen3(provider) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            return Some(crate::asr::local::qwen_backend_for_provider(provider).is_some());
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return Some(false);
        }
    }
    if provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID {
        return Some(active_apple_speech_asr_is_supported(provider));
    }
    if provider == crate::asr::local::foundry::PROVIDER_ID {
        return Some(active_foundry_asr_is_supported(provider));
    }
    if provider == crate::asr::local::sherpa::PROVIDER_ID {
        return Some(active_sherpa_asr_is_supported(provider));
    }
    None
}

pub(crate) fn llm_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    openless_core::provider_rules::llm_configured(
        provider,
        &credential_configuration(
            snap,
            provider,
            CodexOAuthCredentials::load_default().is_ok(),
        ),
    )
}

fn configured(field: &Option<String>) -> bool {
    field
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// 多模态（Omni）模型是否已配置：OpenAI 兼容通道要求 API Key + Base URL + Model；
/// Gemini 通道要求 API Key + Model（Base URL 为空时后端走官方默认）。
pub(crate) fn omni_configured_for_active_provider(snap: &CredentialsSnapshot) -> bool {
    openless_core::provider_rules::omni_configured(
        &snap.active_omni_provider,
        &credential_configuration(snap, "", false),
    )
}

fn credential_configuration(
    snap: &CredentialsSnapshot,
    llm_provider: &str,
    codex_oauth: bool,
) -> openless_core::provider_rules::CredentialConfiguration {
    let llm_endpoint = snap
        .ark_endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    openless_core::provider_rules::CredentialConfiguration {
        asr_api_key: configured(&snap.asr_api_key),
        asr_endpoint: configured(&snap.asr_endpoint),
        asr_model: configured(&snap.asr_model),
        volcengine_auth_mode: snap.volcengine_auth_mode.clone(),
        volcengine_app_key: configured(&snap.volcengine_app_key),
        volcengine_access_key: configured(&snap.volcengine_access_key),
        volcengine_api_key: configured(&snap.volcengine_api_key),
        volcengine_resource_id: configured(&snap.volcengine_resource_id),
        xfyun_app_id: configured(&snap.xfyun_app_id),
        xfyun_api_key: configured(&snap.xfyun_api_key),
        llm_api_key: configured(&snap.ark_api_key),
        llm_endpoint: llm_endpoint.is_some(),
        llm_endpoint_matches_default: llm_endpoint.is_some_and(|endpoint| {
            openless_core::provider_rules::default_llm_endpoint(llm_provider).is_some_and(
                |default| openless_core::provider_rules::equivalent_endpoint(endpoint, default),
            )
        }),
        llm_model: configured(&snap.ark_model_id),
        codex_oauth,
        omni_api_key: configured(&snap.omni_api_key),
        omni_endpoint: configured(&snap.omni_endpoint),
        omni_model: configured(&snap.omni_model),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(mobile))]
pub(crate) struct LocalAsrReleasePlan {
    pub(crate) qwen: bool,
    pub(crate) whisper: bool,
    pub(crate) foundry: bool,
    pub(crate) sherpa: bool,
}

#[cfg(not(mobile))]
pub(crate) fn local_asr_release_plan_for_provider(provider: &str) -> LocalAsrReleasePlan {
    LocalAsrReleasePlan {
        qwen: !crate::asr::local::is_local_qwen3(provider),
        whisper: !crate::asr::local::is_local_whisper(provider),
        foundry: provider != FOUNDRY_LOCAL_PROVIDER_ID,
        sherpa: provider != crate::asr::local::sherpa::PROVIDER_ID,
    }
}

#[cfg(not(mobile))]
pub(crate) async fn release_foundry_runtime_if_inactive(
    runtime: &Arc<FoundryLocalRuntime>,
    release_foundry: bool,
) {
    if release_foundry {
        runtime.request_cancel_prepare();
        if let Err(error) = runtime.release_now().await {
            log::warn!("[foundry-asr] release inactive runtime failed: {error:#}");
        }
    }
}

#[cfg(not(mobile))]
pub(crate) async fn release_sherpa_runtime_if_inactive(
    runtime: &Arc<SherpaOnnxRuntime>,
    release_sherpa: bool,
) {
    if release_sherpa {
        runtime.request_cancel_prepare();
        if let Err(error) = runtime.release_now().await {
            log::warn!("[sherpa-asr] release inactive runtime failed: {error:#}");
        }
    }
}

#[tauri::command]
pub async fn set_credential(
    core: CoreState<'_>,
    window: Window,
    account: String,
    value: String,
    provider: Option<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let key = credential_key(&account, provider)?;
    if value.is_empty() {
        core.remove_credential(key)
            .await
            .map_err(|error| error.to_string())?;
    } else {
        core.set_credential(key, openless_core::SecretValue::new(value))
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn set_active_asr_provider(core: CoreState<'_>, provider: String) -> Result<(), String> {
    if crate::asr::local::is_local_qwen3(&provider)
        || crate::asr::local::is_local_whisper(&provider)
        || provider == crate::asr::local::sherpa::PROVIDER_ID
        || provider == crate::asr::local::foundry::PROVIDER_ID
        || provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    {
        return Err("Local ASR is not available on mobile".to_string());
    }
    if core
        .active_provider(openless_core::ProviderSlot::Asr)
        .await
        .map_err(|error| error.to_string())?
        == provider
    {
        return Ok(());
    }
    core.set_active_provider(openless_core::ProviderSlot::Asr, provider)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn set_active_asr_provider(core: CoreState<'_>, provider: String) -> Result<(), String> {
    if crate::asr::local::is_local_qwen3(&provider)
        && crate::asr::local::qwen_backend_for_provider(&provider).is_none()
    {
        return Err("所选本地 Qwen3-ASR 后端不支持当前系统".to_string());
    }
    if crate::asr::local::is_local_whisper(&provider) && !cfg!(target_os = "macos") {
        return Err("本地 Whisper 当前仅支持 macOS".to_string());
    }
    if provider == FOUNDRY_LOCAL_PROVIDER_ID && !active_foundry_asr_is_supported(&provider) {
        return Err("Foundry Local Whisper is only available on Windows".to_string());
    }
    if provider == crate::asr::local::sherpa::PROVIDER_ID
        && !active_sherpa_asr_is_supported(&provider)
    {
        return Err("sherpa-onnx local ASR is only available on Windows".to_string());
    }
    if provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
        && !active_apple_speech_asr_is_supported(&provider)
    {
        return Err("Apple Speech recognition is only available on macOS".to_string());
    }
    if core
        .active_provider(openless_core::ProviderSlot::Asr)
        .await
        .map_err(|error| error.to_string())?
        == provider
    {
        return Ok(());
    }
    core.set_active_provider(openless_core::ProviderSlot::Asr, provider.clone())
        .await
        .map_err(|error| error.to_string())?;
    let release_plan = local_asr_release_plan_for_provider(&provider);
    if release_plan.qwen || release_plan.whisper {
        core.services()
            .local_asr
            .release(openless_core::LocalAsrRuntime::Generic)
            .await
            .map_err(|error| error.to_string())?;
    }
    if release_plan.foundry {
        core.services()
            .local_asr
            .release(openless_core::LocalAsrRuntime::Foundry)
            .await
            .map_err(|error| error.to_string())?;
    }
    if release_plan.sherpa {
        core.services()
            .local_asr
            .release(openless_core::LocalAsrRuntime::SherpaOnnx)
            .await
            .map_err(|error| error.to_string())?;
    }
    if crate::asr::local::is_local_qwen3(&provider)
        || crate::asr::local::is_local_whisper(&provider)
    {
        core.services()
            .local_asr
            .preload(openless_core::LocalAsrRuntime::Generic)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_active_llm_provider(core: CoreState<'_>, provider: String) -> Result<(), String> {
    core.set_active_provider(openless_core::ProviderSlot::Llm, provider)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_active_omni_provider(core: CoreState<'_>, provider: String) -> Result<(), String> {
    core.set_active_provider(openless_core::ProviderSlot::Omni, provider)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 读出某个账号的实际值（用于设置页预填表单）。
/// 凭据来自系统凭据库；只允许主设置窗口读取 raw secret，避免胶囊 / QA 等辅助窗口默认暴露。
#[tauri::command]
pub async fn read_credential(
    core: CoreState<'_>,
    window: Window,
    account: String,
    provider: Option<String>,
) -> Result<Option<String>, String> {
    ensure_main_window(&window)?;
    core.read_credential(credential_key(&account, provider)?)
        .await
        .map(|value| value.map(openless_core::SecretValue::into_exposed))
        .map_err(|error| error.to_string())
}

fn credential_key(
    account: &str,
    provider: Option<String>,
) -> Result<openless_core::CredentialKey, String> {
    let namespace = match account {
        LLM_EXTRA_HEADERS_ACCOUNT | LLM_TEMPERATURE_ACCOUNT => {
            openless_core::CredentialNamespace::Llm
        }
        OMNI_EXTRA_HEADERS_ACCOUNT | OMNI_TEMPERATURE_ACCOUNT => {
            openless_core::CredentialNamespace::Omni
        }
        MARKETPLACE_GITHUB_TOKEN_ACCOUNT => openless_core::CredentialNamespace::Marketplace,
        _ => {
            let parsed = parse_account(account)?;
            match parsed {
                CredentialAccount::ArkApiKey
                | CredentialAccount::ArkModelId
                | CredentialAccount::ArkEndpoint => openless_core::CredentialNamespace::Llm,
                CredentialAccount::OmniApiKey
                | CredentialAccount::OmniEndpoint
                | CredentialAccount::OmniModel => openless_core::CredentialNamespace::Omni,
                _ => openless_core::CredentialNamespace::Asr,
            }
        }
    };
    openless_core::CredentialKey::new(namespace, provider, account)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialProviderKind {
    Asr,
    Llm,
    Omni,
}

/// 一个凭据账户所属的 provider map —— 决定显式 provider id 应路由到哪个命名空间。
fn account_provider_kind(account: CredentialAccount) -> CredentialProviderKind {
    match account {
        CredentialAccount::ArkApiKey
        | CredentialAccount::ArkModelId
        | CredentialAccount::ArkEndpoint => CredentialProviderKind::Llm,
        CredentialAccount::VolcengineAppKey
        | CredentialAccount::VolcengineAccessKey
        | CredentialAccount::VolcengineResourceId
        | CredentialAccount::VolcengineAuthMode
        | CredentialAccount::VolcengineApiKey
        | CredentialAccount::AsrApiKey
        | CredentialAccount::AsrEndpoint
        | CredentialAccount::AsrModel
        | CredentialAccount::AsrVocabularyId
        | CredentialAccount::AsrAdvancedConfig
        | CredentialAccount::XfyunAppId
        | CredentialAccount::XfyunApiKey => CredentialProviderKind::Asr,
        CredentialAccount::OmniApiKey
        | CredentialAccount::OmniEndpoint
        | CredentialAccount::OmniModel => CredentialProviderKind::Omni,
    }
}

pub(crate) fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("credential access is only allowed from the main window".to_string())
    }
}

fn parse_account(s: &str) -> Result<CredentialAccount, String> {
    match s {
        "volcengine.app_key" => Ok(CredentialAccount::VolcengineAppKey),
        "volcengine.access_key" => Ok(CredentialAccount::VolcengineAccessKey),
        "volcengine.resource_id" => Ok(CredentialAccount::VolcengineResourceId),
        "volcengine.auth_mode" => Ok(CredentialAccount::VolcengineAuthMode),
        "volcengine.api_key" => Ok(CredentialAccount::VolcengineApiKey),
        "ark.api_key" => Ok(CredentialAccount::ArkApiKey),
        "ark.model_id" => Ok(CredentialAccount::ArkModelId),
        "ark.endpoint" => Ok(CredentialAccount::ArkEndpoint),
        "asr.api_key" => Ok(CredentialAccount::AsrApiKey),
        "asr.endpoint" => Ok(CredentialAccount::AsrEndpoint),
        "asr.model" => Ok(CredentialAccount::AsrModel),
        "asr.vocabulary_id" => Ok(CredentialAccount::AsrVocabularyId),
        "asr.advanced_config" => Ok(CredentialAccount::AsrAdvancedConfig),
        "xfyun.app_id" => Ok(CredentialAccount::XfyunAppId),
        "xfyun.api_key" => Ok(CredentialAccount::XfyunApiKey),
        "omni.api_key" => Ok(CredentialAccount::OmniApiKey),
        "omni.endpoint" => Ok(CredentialAccount::OmniEndpoint),
        "omni.model" => Ok(CredentialAccount::OmniModel),
        _ => Err(format!("unknown account: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omni_credential_keys_preserve_the_explicit_provider_scope() {
        for account in [
            "omni.api_key",
            "omni.endpoint",
            "omni.model",
            OMNI_EXTRA_HEADERS_ACCOUNT,
            OMNI_TEMPERATURE_ACCOUNT,
        ] {
            let key = credential_key(account, Some("frozen-provider".to_string())).unwrap();
            assert_eq!(key.namespace, openless_core::CredentialNamespace::Omni);
            assert_eq!(key.provider_id.as_deref(), Some("frozen-provider"));
        }
        for account in [
            CredentialAccount::OmniApiKey,
            CredentialAccount::OmniEndpoint,
            CredentialAccount::OmniModel,
        ] {
            assert_eq!(account_provider_kind(account), CredentialProviderKind::Omni);
        }
    }

    #[test]
    fn core_llm_accounts_are_supported_by_the_tauri_vault_adapter() {
        for account in [
            openless_core::credentials::LLM_API_KEY_ACCOUNT,
            openless_core::credentials::LLM_MODEL_ACCOUNT,
            openless_core::credentials::LLM_ENDPOINT_ACCOUNT,
        ] {
            let key = openless_core::CredentialKey::new(
                openless_core::CredentialNamespace::Llm,
                Some("channel".to_string()),
                account,
            )
            .unwrap();
            assert!(
                parse_vault_account(&key).is_ok(),
                "Core LLM account must be accepted by the Tauri vault adapter: {account}"
            );
        }
    }
}
