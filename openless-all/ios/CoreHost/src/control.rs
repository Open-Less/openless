use super::*;
use openless_core::credentials::*;
use openless_core::domains::{ProviderKind, ProviderRequest, SelectionPolishRequest};
use openless_core::events::{BackendEventKind, EventRecvError};
use openless_core::provider_rules::{provider_descriptor, provider_descriptors};
use openless_core::settings::{NoopSettingsRuntime, SettingsUpdateOptions};
use openless_core::shared_types::PipelineMode;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum VocabularySourceFilter {
    #[default]
    All,
    Learned,
    Manual,
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ControlAction {
    AccountSync { request: Value },
    VocabularyBridge { request: Value },
    Snapshot,
    AudioSettings,
    UpdateAudioSettings { enabled: bool, dsp: openless_core::AudioDspConfig },
    StyleLibrary,
    StyleDetail { id: String },
    SetStyleEnabled { id: String, enabled: bool },
    ActivateStyle { id: String },
    RemoveStyle { id: String },
    ImportStyleArchive { staged_file_name: String },
    MarketplaceList { query: Option<String>, sort: Option<String>, limit: Option<u32> },
    MarketplaceDetail { id: String },
    MarketplaceInstall { id: String },
    CapabilitySnapshot,
    TranslationConfiguration,
    SetTranslationTarget { target_language: String },
    SetTranslationTargets { targets: Vec<String> },
    AddTranslationTarget { target: String },
    DeleteTranslationTarget { target: String },
    ReorderTranslationTargets { targets: Vec<String> },
    SelectTranslationTarget { target: Option<String> },
    Library,
    SearchHistory { #[serde(default)] query: String },
    HistoryDetail { id: String },
    DeleteHistory { id: String },
    ClearLibraryHistory,
    AudioAsset { id: String },
    RepolishHistory { id: String, style_pack_id: String },
    Vocabulary {
        #[serde(default)] offset: usize,
        #[serde(default)] query: String,
        #[serde(default)] source: VocabularySourceFilter,
    },
    AddVocabulary { phrase: String, note: Option<String> },
    RemoveVocabulary { id: String },
    UpdateVocabulary { id: String, phrase: String },
    SetVocabularyEnabled { id: String, enabled: bool },
    SaveChannel {
        kind: ProviderKind,
        #[serde(rename = "channelID")]
        channel_id: Option<String>,
        provider_type: String,
        name: String,
        endpoint: String,
        model: String,
        api_key: Option<String>,
        /// Provider-specific account names, restricted to the Core accounts below.
        #[serde(default)]
        credential_fields: HashMap<String, String>,
    },
    SetActive {
        kind: ProviderKind,
        #[serde(rename = "channelID")]
        channel_id: String,
    },
    UpdateSettings {
        pipeline_mode: PipelineMode,
        polish_enabled: bool,
    },
    ListModels {
        kind: ProviderKind,
        #[serde(rename = "channelID")]
        channel_id: String,
    },
    Validate {
        kind: ProviderKind,
        #[serde(rename = "channelID")]
        channel_id: String,
    },
    DeleteChannel {
        kind: ProviderKind,
        #[serde(rename = "channelID")]
        channel_id: String,
    },
    Rewrite {
        text: String,
        instruction: String,
    },
    #[serde(other)]
    Unknown,
}
fn invalid(message: &str) -> BackendError {
    error(BackendErrorCode::InvalidArgument, message)
}
fn channel_kind(kind: ProviderKind) -> ChannelKind {
    match kind {
        ProviderKind::Asr => ChannelKind::Asr,
        _ => ChannelKind::Llm,
    }
}
fn slot(kind: ProviderKind) -> ProviderSlot {
    match kind {
        ProviderKind::Asr => ProviderSlot::Asr,
        ProviderKind::Llm => ProviderSlot::Llm,
        ProviderKind::Omni => ProviderSlot::Omni,
    }
}
fn accounts(
    kind: ProviderKind,
) -> (
    CredentialNamespace,
    &'static str,
    &'static str,
    &'static str,
) {
    match kind {
        ProviderKind::Asr => (
            CredentialNamespace::Asr,
            ASR_ENDPOINT_ACCOUNT,
            ASR_MODEL_ACCOUNT,
            ASR_API_KEY_ACCOUNT,
        ),
        ProviderKind::Llm => (
            CredentialNamespace::Llm,
            LLM_ENDPOINT_ACCOUNT,
            LLM_MODEL_ACCOUNT,
            LLM_API_KEY_ACCOUNT,
        ),
        ProviderKind::Omni => (
            CredentialNamespace::Omni,
            OMNI_ENDPOINT_ACCOUNT,
            OMNI_MODEL_ACCOUNT,
            OMNI_API_KEY_ACCOUNT,
        ),
    }
}
fn extra_accounts(kind: ProviderKind) -> &'static [&'static str] {
    match kind {
        ProviderKind::Asr => &[
            ASR_VOCABULARY_ID_ACCOUNT,
            ASR_ADVANCED_CONFIG_ACCOUNT,
            VOLCENGINE_APP_KEY_ACCOUNT,
            VOLCENGINE_ACCESS_KEY_ACCOUNT,
            VOLCENGINE_RESOURCE_ID_ACCOUNT,
            VOLCENGINE_SERVICE_ACCOUNT,
            VOLCENGINE_AUTH_MODE_ACCOUNT,
            VOLCENGINE_API_KEY_ACCOUNT,
            XFYUN_APP_ID_ACCOUNT,
            XFYUN_API_KEY_ACCOUNT,
            TENCENT_CLOUD_APP_ID_ACCOUNT,
            TENCENT_CLOUD_SECRET_ID_ACCOUNT,
            TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
        ],
        ProviderKind::Llm => &[LLM_EXTRA_HEADERS_ACCOUNT, LLM_TEMPERATURE_ACCOUNT],
        ProviderKind::Omni => &[OMNI_EXTRA_HEADERS_ACCOUNT, OMNI_TEMPERATURE_ACCOUNT],
    }
}
pub(super) fn supported(kind: ProviderKind, provider: &str) -> bool {
    match kind {
        ProviderKind::Asr => {
            ["ios-sensevoice", "apple-speech"].contains(&provider)
                || SHARED_CLOUD_ASR_PROVIDER_TYPES.contains(&provider)
        }
        ProviderKind::Llm => provider != "codex_oauth" && SHARED_CLOUD_LLM_PROVIDER_TYPES.contains(&provider),
        ProviderKind::Omni => SHARED_OMNI_PROVIDER_TYPES.contains(&provider),
    }
}

impl Host {
    fn vocabulary_metadata_directory(&self) -> Result<&std::path::Path, BackendError> {
        self.recordings_directory.parent().ok_or_else(|| invalid("vocabulary metadata directory unavailable"))
    }
    pub(crate) fn dispatch_control(self: &Arc<Self>, request_id: String, action: ControlAction) {
        let fail = |message: &str| {
            self.broker.emit(
                json!({"kind":"controlResult","requestID":request_id,"ok":false,"message":message}),
            )
        };
        if request_id.trim().is_empty() {
            fail("requestID must not be blank");
            return;
        }
        let product_action = matches!(action,
            ControlAction::AudioSettings | ControlAction::UpdateAudioSettings { .. } |
            ControlAction::AccountSync { .. } | ControlAction::VocabularyBridge { .. } | ControlAction::Library | ControlAction::SearchHistory { .. } | ControlAction::HistoryDetail { .. }
            | ControlAction::DeleteHistory { .. } | ControlAction::ClearLibraryHistory
            | ControlAction::AudioAsset { .. } | ControlAction::Vocabulary { .. }
            | ControlAction::AddVocabulary { .. } | ControlAction::RemoveVocabulary { .. }
            | ControlAction::UpdateVocabulary { .. } | ControlAction::SetVocabularyEnabled { .. }
            | ControlAction::StyleLibrary | ControlAction::StyleDetail { .. }
            | ControlAction::SetStyleEnabled { .. } | ControlAction::ActivateStyle { .. }
            | ControlAction::RemoveStyle { .. } | ControlAction::ImportStyleArchive { .. }
            | ControlAction::MarketplaceList { .. } | ControlAction::MarketplaceDetail { .. }
            | ControlAction::MarketplaceInstall { .. } | ControlAction::CapabilitySnapshot
            | ControlAction::TranslationConfiguration | ControlAction::SetTranslationTarget { .. }
            | ControlAction::SetTranslationTargets { .. } | ControlAction::AddTranslationTarget { .. }
            | ControlAction::DeleteTranslationTarget { .. } | ControlAction::ReorderTranslationTargets { .. }
            | ControlAction::SelectTranslationTarget { .. });
        if !self.developer_mode && !product_action && !matches!(action, ControlAction::Snapshot) {
            fail("developer mode required");
            return;
        }
        // Admission and management share a synchronous gate, including the
        // interval before Core has installed its own session. No start/save race.
        let admission = self.admission.lock().unwrap();
        if admission.is_some() && !matches!(action,
            ControlAction::AudioSettings | ControlAction::Snapshot | ControlAction::Library | ControlAction::SearchHistory { .. }
            | ControlAction::HistoryDetail { .. } | ControlAction::Vocabulary { .. }
            | ControlAction::StyleLibrary | ControlAction::StyleDetail { .. }
            | ControlAction::CapabilitySnapshot | ControlAction::TranslationConfiguration) {
            drop(admission);
            fail("recording or processing is active");
            return;
        }
        if self.control_busy.swap(true, Ordering::AcqRel) {
            drop(admission);
            fail("another control request is active");
            return;
        }
        drop(admission);
        let owner = self.clone();
        let marketplace_action = matches!(action, ControlAction::MarketplaceList { .. }
            | ControlAction::MarketplaceDetail { .. } | ControlAction::MarketplaceInstall { .. });
        self.executor.spawn(async move {
            let result = owner.control(action).await;
            // Release before invoking Swift so it can immediately send Start.
            owner.control_busy.store(false, Ordering::Release);
            match result {
                Ok(value) => owner.broker.emit(json!({"kind":"controlResult","requestID":request_id,"ok":true,"json":value.to_string()})),
                // Provider errors can contain URLs/server responses. Never echo
                // those across the FFI where a key could have been reflected.
                Err(err) => owner.broker.emit(json!({"kind":"controlResult","requestID":request_id,"ok":false,
                    "message":if marketplace_action && err.code == BackendErrorCode::Provider {
                        "Marketplace request failed; network, server or archive verification may have failed".into()
                    } else { public_error_message(&err) },"code":format!("{:?}",err.code)})),
            }
        });
    }
    async fn require_channel(&self, kind: ProviderKind, id: &str) -> Result<String, BackendError> {
        if kind == ProviderKind::Omni {
            if !SHARED_OMNI_PROVIDER_TYPES.contains(&id) {
                return Err(invalid("unknown Omni provider type"));
            }
            return Ok(id.into());
        }
        self.backend
            .list_channels(channel_kind(kind))
            .await?
            .into_iter()
            .find(|c| c.id == id && c.enabled)
            .map(|c| c.provider_type)
            .ok_or_else(|| invalid("unknown or disabled channelID"))
    }
    async fn read_field(
        &self,
        kind: ProviderKind,
        id: &str,
        account: &str,
    ) -> Result<Option<String>, BackendError> {
        let key = CredentialKey::new(accounts(kind).0, Some(id.into()), account)?;
        Ok(self
            .backend
            .read_credential(key)
            .await?
            .map(SecretValue::into_exposed))
    }
    async fn configuration(&self, kind: ProviderKind, id: &str) -> Result<Value, BackendError> {
        let (_, endpoint, model, api_key) = accounts(kind);
        // Do not serialize arbitrary credentials; only these safe fields leave
        // the vault. Provider-specific credentials are presence flags only.
        let mut has_credentials = serde_json::Map::new();
        for account in extra_accounts(kind) {
            has_credentials.insert(
                (*account).into(),
                json!(self
                    .read_field(kind, id, account)
                    .await?
                    .is_some_and(|v| !v.trim().is_empty())),
            );
        }
        Ok(
            json!({"endpoint":self.read_field(kind,id,endpoint).await?.unwrap_or_default(),"model":self.read_field(kind,id,model).await?.unwrap_or_default(),"hasAPIKey":self.read_field(kind,id,api_key).await?.is_some_and(|v| !v.trim().is_empty()),"hasCredentials":has_credentials}),
        )
    }
    async fn control_snapshot(&self) -> Result<Value, BackendError> {
        if !self.developer_mode {
            return Ok(
                json!({"developerMode":false,"asr":{"providerType":"ios-sensevoice","nativeProvider":"sensevoice","configured":true},"pipelineMode":"traditional","polishEnabled":false}),
            );
        }
        let mut channels = serde_json::Map::new();
        for kind in [ProviderKind::Asr, ProviderKind::Llm] {
            let mut entries = vec![];
            for channel in self.backend.list_channels(channel_kind(kind)).await? {
                let configuration = self.configuration(kind, &channel.id).await?;
                let mut value =
                    serde_json::to_value(&channel).map_err(|_| invalid("invalid channel"))?;
                value
                    .as_object_mut()
                    .unwrap()
                    .extend(configuration.as_object().unwrap().clone());
                value["supported"] = json!(supported(kind, &channel.provider_type));
                entries.push(value);
            }
            channels.insert(
                if kind == ProviderKind::Asr {
                    "asr"
                } else {
                    "llm"
                }
                .into(),
                json!(entries),
            );
        }
        let mut omni = vec![];
        for provider in SHARED_OMNI_PROVIDER_TYPES {
            let mut value = self.configuration(ProviderKind::Omni, provider).await?;
            value["id"] = json!(provider);
            value["providerType"] = json!(provider);
            omni.push(value);
        }
        let mut descriptors = vec![];
        for kind in [ProviderKind::Asr, ProviderKind::Llm, ProviderKind::Omni] {
            for descriptor in provider_descriptors(kind) {
                let mut value =
                    serde_json::to_value(&descriptor).map_err(|_| invalid("invalid descriptor"))?;
                value["supported"] = json!(supported(kind, descriptor.provider_type.as_str()));
                descriptors.push(value);
            }
        }
        let mut sensevoice =
            serde_json::to_value(provider_descriptor(ProviderKind::Asr, "apple-speech").unwrap())
                .unwrap();
        sensevoice["providerType"] = json!("ios-sensevoice");
        sensevoice["labelKey"] = json!("SenseVoice (iOS)");
        sensevoice["validationProbe"] = json!("unsupported");
        sensevoice["supported"] = json!(true);
        descriptors.push(sensevoice);
        let preferences = self.backend.get_preferences();
        Ok(
            json!({"developerMode":true,"preferences":preferences,"polishEnabled":preferences.active_style_pack_id != "builtin.raw", "channels":channels,"active":{"asr":self.backend.active_provider(ProviderSlot::Asr).await?,"llm":self.backend.active_provider(ProviderSlot::Llm).await?,"omni":self.backend.active_provider(ProviderSlot::Omni).await?},"providerDescriptors":descriptors,"omniConfigurations":omni,"credentialsStatus":self.backend.get_credentials_status().await?}),
        )
    }
    pub(super) async fn control(&self, action: ControlAction) -> Result<Value, BackendError> {
        match action {
            ControlAction::AudioSettings => {
                let prefs = self.backend.get_preferences();
                Ok(json!({"enabled":prefs.audio_enhancement_enabled,"dsp":prefs.audio_dsp}))
            }
            ControlAction::UpdateAudioSettings { enabled, dsp } => {
                let mut prefs = self.backend.get_preferences();
                prefs.audio_enhancement_enabled = enabled;
                prefs.audio_dsp = dsp.normalized();
                prefs.audio_dsp.android_aec_enabled = false;
                prefs.audio_dsp.android_noise_suppression_enabled = false;
                prefs.audio_dsp.android_agc_enabled = false;
                self.backend.update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)?;
                let prefs = self.backend.get_preferences();
                Ok(json!({"enabled":prefs.audio_enhancement_enabled,"dsp":prefs.audio_dsp}))
            }
            ControlAction::AccountSync { request } => self.account_sync.execute(request).await,
            ControlAction::VocabularyBridge { request } => vocabulary_bridge::execute(&self.backend, self.vocabulary_metadata_directory()?, request),
            ControlAction::Snapshot => self.control_snapshot().await,
            ControlAction::StyleLibrary => self.style_library(),
            ControlAction::StyleDetail { id } => self.style_detail(&id),
            ControlAction::SetStyleEnabled { id, enabled } => self.set_style_enabled(&id, enabled),
            ControlAction::ActivateStyle { id } => self.activate_style(&id),
            ControlAction::RemoveStyle { id } => self.remove_style(&id),
            ControlAction::ImportStyleArchive { staged_file_name } => self.import_style_archive(&staged_file_name),
            ControlAction::MarketplaceList { query, sort, limit } => self.marketplace_list(query, sort, limit).await,
            ControlAction::MarketplaceDetail { id } => self.marketplace_detail(id).await,
            ControlAction::MarketplaceInstall { id } => self.marketplace_install(id).await,
            ControlAction::CapabilitySnapshot => self.capability_snapshot().await,
            ControlAction::TranslationConfiguration => Ok(self.translation_configuration()),
            ControlAction::SetTranslationTarget { target_language } => self.set_translation_target(&target_language),
            ControlAction::SetTranslationTargets { targets } => { self.backend.set_translation_targets(targets)?; Ok(self.translation_configuration()) }
            ControlAction::AddTranslationTarget { target } => { self.backend.add_translation_target(target)?; Ok(self.translation_configuration()) }
            ControlAction::DeleteTranslationTarget { target } => { self.backend.delete_translation_target(target)?; Ok(self.translation_configuration()) }
            ControlAction::ReorderTranslationTargets { targets } => { self.backend.reorder_translation_targets(targets)?; Ok(self.translation_configuration()) }
            ControlAction::SelectTranslationTarget { target } => { self.backend.select_translation_target(target)?; Ok(self.translation_configuration()) }
            ControlAction::Library => {
                let mut value = self.library_snapshot()?;
                value["canPolish"] = json!(self.developer_mode && self.backend.get_credentials_status().await?.llm_configured);
                Ok(value)
            }
            ControlAction::SearchHistory { query } => self.search_history(&query),
            ControlAction::HistoryDetail { id } =>
                serde_json::to_value(self.history_entry(&id)?).map_err(|_| invalid("invalid history entry")),
            ControlAction::DeleteHistory { id } => {
                self.delete_history_with_recordings(&id)?;
                Ok(json!({}))
            }
            ControlAction::ClearLibraryHistory => {
                self.clear_history_with_recordings()?;
                Ok(json!({}))
            }
            ControlAction::AudioAsset { id } => {
                if self.history_entry(&id)?.has_audio_recording != Some(true) {
                    return Err(invalid("this history entry has no audio recording"));
                }
                let path = recordings::asset_path(&self.recordings_directory, &id)?;
                Ok(json!({"path":path}))
            }
            ControlAction::RepolishHistory { id, style_pack_id } => self.repolish_history(&id, style_pack_id).await,
            ControlAction::Vocabulary { offset, query, source } => {
                if query.len() > 512 { return Err(invalid("search query too long")); }
                let query = query.trim().to_lowercase();
                // Same classification as Core's ASR vocabulary ordering. This is
                // a domain category, not a claim about which device added it.
                let entries: Vec<_> = self.backend.list_vocabulary()?.into_iter()
                    .filter(|entry| {
                        let learned = entry.note.as_deref() == Some(openless_core::shared_types::LEARNED_VOCAB_NOTE);
                        (match source {
                            VocabularySourceFilter::All => true,
                            VocabularySourceFilter::Learned => learned,
                            VocabularySourceFilter::Manual => !learned,
                        }) && (query.is_empty() || entry.phrase.to_lowercase().contains(&query))
                    }).collect();
                let total = entries.len();
                let page: Vec<_> = entries.into_iter().skip(offset).take(100).map(|entry| {
                    let learned = entry.note.as_deref() == Some(openless_core::shared_types::LEARNED_VOCAB_NOTE);
                    let mut value = serde_json::to_value(entry).expect("dictionary entry is serializable");
                    value["source"] = json!(if learned { "learned" } else { "manual" });
                    value
                }).collect();
                Ok(json!({"entries":page,"total":total}))
            }
            ControlAction::AddVocabulary { phrase, note } => {
                if phrase.len() > 512 || phrase.trim().is_empty() || phrase.contains(['\n', '\r'])
                    || note.as_ref().is_some_and(|note| note.len() > 2048) {
                    return Err(invalid("enter a single bounded vocabulary phrase"));
                }
                let entry = self.backend.add_vocabulary_if_absent(phrase.trim().into(), note)?;
                let keyboard_confirmed = if let Some(entry) = &entry {
                    vocabulary_bridge::confirm_explicit_edit(&self.backend, self.vocabulary_metadata_directory()?, &entry.id, &entry.phrase)?
                } else { false };
                Ok(json!({"entry":entry,"keyboardConfirmed":keyboard_confirmed}))
            }
            ControlAction::RemoveVocabulary { id } => {
                vocabulary_bridge::invalidate_entry(self.vocabulary_metadata_directory()?, &id)?;
                self.backend.remove_vocabulary(&id)?; Ok(json!({}))
            }
            ControlAction::UpdateVocabulary { id, phrase } => {
                if phrase.len() > 512 || phrase.trim().is_empty() || phrase.contains(['\n', '\r']) {
                    return Err(invalid("enter a single bounded vocabulary phrase"));
                }
                vocabulary_bridge::invalidate_entry(self.vocabulary_metadata_directory()?, &id)?;
                self.backend.update_vocabulary_phrase(&id, phrase.trim().into())?;
                let keyboard_confirmed = vocabulary_bridge::confirm_explicit_edit(&self.backend, self.vocabulary_metadata_directory()?, &id, phrase.trim())?;
                Ok(json!({"keyboardConfirmed":keyboard_confirmed}))
            }
            ControlAction::SetVocabularyEnabled { id, enabled } => {
                vocabulary_bridge::invalidate_entry(self.vocabulary_metadata_directory()?, &id)?;
                self.backend.set_vocabulary_enabled(&id, enabled)?; Ok(json!({}))
            }
            ControlAction::SaveChannel {
                kind,
                channel_id,
                provider_type,
                name,
                endpoint,
                model,
                api_key,
                credential_fields,
            } => {
                if !supported(kind, &provider_type) {
                    return Err(error(
                        BackendErrorCode::Unsupported,
                        "provider is not supported by the iOS host",
                    ));
                }
                if credential_fields
                    .keys()
                    .any(|key| !extra_accounts(kind).contains(&key.as_str()))
                {
                    return Err(invalid("unsupported credentialFields account"));
                }
                let id = if kind == ProviderKind::Omni {
                    if channel_id.as_ref().is_some_and(|id| id != &provider_type) {
                        return Err(invalid("Omni channelID must equal providerType"));
                    }
                    provider_type
                } else if let Some(id) = channel_id {
                    self.require_channel(kind, &id).await?;
                    self.backend
                        .set_channel_provider_type(channel_kind(kind), id.clone(), provider_type)
                        .await?;
                    self.backend
                        .rename_channel(channel_kind(kind), id.clone(), name)
                        .await?;
                    id
                } else {
                    self.backend
                        .create_channel(channel_kind(kind), provider_type, name)
                        .await?
                };
                let (ns, endpoint_account, model_account, key_account) = accounts(kind);
                let mut fields = vec![
                    (endpoint_account.to_string(), endpoint),
                    (model_account.to_string(), model),
                ];
                if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
                    fields.push((key_account.into(), key));
                }
                // Empty/missing secret fields preserve existing values. Endpoint
                // and model are explicit values: empty resets to Core defaults.
                fields.extend(
                    credential_fields
                        .into_iter()
                        .filter(|(_, value)| !value.trim().is_empty()),
                );
                for (account, value) in fields {
                    self.backend
                        .set_credential(
                            CredentialKey::new(ns, Some(id.clone()), account)?,
                            SecretValue::new(value),
                        )
                        .await?;
                }
                Ok(json!({"channelID":id}))
            }
            ControlAction::SetActive { kind, channel_id } => {
                self.require_channel(kind, &channel_id).await?;
                self.backend
                    .set_active_provider(slot(kind), channel_id)
                    .await?;
                Ok(json!({}))
            }
            ControlAction::UpdateSettings {
                pipeline_mode,
                polish_enabled,
            } => {
                let mut preferences = self.backend.get_preferences();
                preferences.pipeline_mode = pipeline_mode;
                preferences.multimodal_pipeline_enabled = pipeline_mode == PipelineMode::Multimodal;
                if !polish_enabled {
                    preferences.active_style_pack_id = "builtin.raw".into();
                    preferences.default_mode = PolishMode::Raw;
                } else if preferences.active_style_pack_id == "builtin.raw" {
                    preferences.active_style_pack_id = "builtin.light".into();
                    preferences.default_mode = PolishMode::Light;
                }
                let outcome = self.backend.update_settings(
                    preferences,
                    SettingsUpdateOptions::STRICT,
                    &NoopSettingsRuntime,
                )?;
                serde_json::to_value(outcome).map_err(|_| invalid("invalid settings result"))
            }
            ControlAction::ListModels { kind, channel_id } => {
                self.provider_request(kind, channel_id, false).await
            }
            ControlAction::Validate { kind, channel_id } => {
                self.provider_request(kind, channel_id, true).await
            }
            ControlAction::DeleteChannel { kind, channel_id } => {
                if kind == ProviderKind::Omni {
                    return Err(error(
                        BackendErrorCode::Unsupported,
                        "Core does not support deleting Omni configurations",
                    ));
                }
                self.require_channel(kind, &channel_id).await?;
                self.backend
                    .delete_channel(channel_kind(kind), channel_id.clone())
                    .await?;
                // Core directory removes metadata, not credential records.
                let (ns, endpoint, model, key) = accounts(kind);
                for account in [endpoint, model, key]
                    .into_iter()
                    .chain(extra_accounts(kind).iter().copied())
                {
                    self.backend
                        .remove_credential(CredentialKey::new(
                            ns,
                            Some(channel_id.clone()),
                            account,
                        )?)
                        .await?;
                }
                Ok(json!({}))
            }
            ControlAction::Rewrite { text, instruction } => {
                if text.trim().is_empty()
                    || instruction.trim().is_empty()
                    || text.len() > 128 * 1024
                    || instruction.len() > 16 * 1024
                {
                    return Err(invalid(
                        "rewrite requires bounded nonempty text and instruction",
                    ));
                }
                if !self.backend.get_credentials_status().await?.llm_configured {
                    return Err(invalid("configured LLM required"));
                }
                let selection = &self.backend.services().selection;
                let session = selection
                    .begin_polish(SelectionPolishRequest {
                        selected_text: Some(text),
                        mode: PolishMode::Light,
                        instruction: Some(instruction),
                    })
                    .await?;
                let snapshot = selection.snapshot().await?;
                let result = snapshot.preview_text.ok_or_else(|| {
                    error(
                        BackendErrorCode::InvalidState,
                        "Core selection did not produce a preview",
                    )
                });
                let _ = selection.cancel(Some(session)).await;
                Ok(json!({"text":result?}))
            }
            ControlAction::Unknown => Err(invalid("unknown control action")),
        }
    }
    async fn provider_request(
        &self,
        kind: ProviderKind,
        channel_id: String,
        validate: bool,
    ) -> Result<Value, BackendError> {
        self.require_channel(kind, &channel_id).await?;
        if kind == ProviderKind::Omni
            && self.backend.active_provider(ProviderSlot::Omni).await? != channel_id
        {
            return Err(invalid(
                "select this Omni provider before listModels/validate",
            ));
        }
        let request = ProviderRequest {
            kind,
            channel_id: (kind != ProviderKind::Omni).then_some(channel_id),
            thinking_enabled: self.backend.get_preferences().llm_thinking_enabled,
        };
        if validate {
            serde_json::to_value(self.backend.services().provider.validate(request).await?)
                .map_err(|_| invalid("invalid validation result"))
        } else {
            serde_json::to_value(
                self.backend
                    .services()
                    .provider
                    .list_models(request)
                    .await?,
            )
            .map_err(|_| invalid("invalid models result"))
        }
    }
}

pub(crate) fn forward_previews(
    backend: &Arc<OpenLessBackend>,
    broker: Arc<Broker>,
    executor: &Handle,
) {
    let mut subscription = backend.subscribe();
    executor.spawn(async move {
        let mut texts: HashMap<(SessionId, &'static str), openless_core::types::TranscriptAccumulator> = HashMap::new();
        loop {
            let event = match subscription.recv().await {
                Ok(event) => event,
                Err(EventRecvError::Lagged(_)) => { texts.clear(); continue; },
                Err(_) => break,
            };
            if broker.closed.load(Ordering::Acquire) { break; }
            let Some(session) = event.session_id else { continue; };
            let (stage, delta) = match event.kind {
                BackendEventKind::TranscriptDelta(delta) => ("asr", delta),
                BackendEventKind::PolishDelta(delta) => ("llm", openless_core::types::TranscriptDelta { text:delta.text, offset:delta.offset, is_final:delta.is_final }),
                _ => continue,
            };
            // Deltas use character offsets, not bytes. ASR and LLM must not
            // share an accumulator or feed the native transcript input again.
            texts.retain(|(id, _), _| *id == session);
            let text = texts.entry((session,stage)).or_default();
            if text.apply(&delta).is_ok() {
                broker.emit(json!({"kind":"preview","sessionID":session.to_string(),"stage":stage,"text":text.text(),"isFinal":delta.is_final}));
            }
        }
    });
}
