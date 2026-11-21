//! Thin product adapter over Core's style store, marketplace and preferences.
//! Archive sources are staged by the App in its private data directory, never
//! in the keyboard container. No editor, transport, or prompt rules live here.
use super::*;
use openless_core::credentials::{
    CredentialKey, CredentialNamespace, LLM_API_KEY_ACCOUNT, LLM_MODEL_ACCOUNT,
};
use openless_core::domains::{MarketplaceQuery, ProviderKind};
use openless_core::shared_types::{effective_pipeline_mode, translation_effective, PipelineMode};
use openless_core::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::Path;

fn invalid(message: &str) -> BackendError {
    error(BackendErrorCode::InvalidArgument, message)
}
fn value<T: serde::Serialize>(value: T) -> Result<Value, BackendError> {
    serde_json::to_value(value).map_err(|_| invalid("invalid style service response"))
}
fn catalog() -> Value {
    // Same immutable input used by Core language_catalog.rs and desktop UI.
    serde_json::from_str(include_str!("../../../app/contract/language-catalog.json"))
        .expect("bundled language catalog is valid")
}
pub(super) fn prepare_staging(data_dir: &Path) -> Result<PathBuf, BackendError> {
    let directory = data_dir.join("style-imports");
    if !directory.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| {
                error(
                    BackendErrorCode::Persistence,
                    "cannot create private style staging directory",
                )
            })?;
    }
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|_| invalid("style staging directory unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid(
            "style staging must be a private directory, not a link",
        ));
    }
    directory
        .canonicalize()
        .map_err(|_| invalid("style staging directory unavailable"))
}

fn staged_archive(directory: &Path, name: &str) -> Result<Vec<u8>, BackendError> {
    if name.is_empty()
        || name.len() > 128
        || !name.ends_with(".zip")
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || name.starts_with('.')
    {
        return Err(invalid(
            "use a bounded ZIP filename in private style staging",
        ));
    }
    let path = directory.join(name);
    let before = fs::symlink_metadata(&path).map_err(|_| invalid("staged archive not found"))?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.len() > STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64
    {
        return Err(invalid(
            "staged archive must be a bounded regular file, not a link",
        ));
    }
    let file = File::open(&path).map_err(|_| invalid("staged archive cannot be opened"))?;
    let after = file
        .metadata()
        .map_err(|_| invalid("staged archive unavailable"))?;
    // Check the opened identity before reading, including replacement/link races.
    if after.dev() != before.dev() || after.ino() != before.ino() || after.nlink() != 1 {
        return Err(invalid("staged archive changed while opening"));
    }
    let mut bytes = Vec::new();
    file.take(STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("staged archive cannot be read"))?;
    if bytes.len() > STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES {
        return Err(invalid("staged archive exceeds compressed size limit"));
    }
    Ok(bytes)
}

impl Host {
    pub(super) fn style_staging_directory(&self) -> Result<PathBuf, BackendError> {
        prepare_staging(
            self.recordings_directory
                .parent()
                .expect("recordings have a private data root"),
        )
    }
    pub(super) fn style_library(&self) -> Result<Value, BackendError> {
        let prefs = self.backend.get_preferences();
        let packs: Vec<_> = self.backend.list_style_packs(&prefs.active_style_pack_id)?
            .into_iter().map(|pack| json!({
                "id":pack.id,"name":pack.name,"description":pack.description,
                "version":pack.version,"kind":pack.kind,"baseMode":pack.base_mode,
                "enabled":pack.enabled,"active":pack.active,"recommendedModel":pack.recommended_model,
                "originPackId":pack.origin_pack_id,"originAuthorLogin":pack.origin_author_login
            })).collect();
        Ok(
            json!({"packs":packs,"activeStylePackId":prefs.active_style_pack_id,
            "archive":{"format":"zip","maxCompressedBytes":STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES,
                "stagingDirectory":self.style_staging_directory()?},
            "editorSupported":false,"updateSupported":false,"repeatImportBehavior":"uniqueId"}),
        )
    }
    pub(super) fn style_detail(&self, id: &str) -> Result<Value, BackendError> {
        let mut pack = self.backend.get_style_pack(id)?;
        pack.active = self.backend.get_preferences().active_style_pack_id == id;
        let runtime = self.backend.preview_style_pack_runtime(&pack);
        Ok(json!({"pack":pack,"runtime":runtime}))
    }
    pub(super) fn set_style_enabled(&self, id: &str, enabled: bool) -> Result<Value, BackendError> {
        if !self.developer_mode && id == "builtin.raw" && !enabled {
            return Err(error(
                BackendErrorCode::PermissionDenied,
                "ordinary mode requires the raw style to remain enabled",
            ));
        }
        Ok(json!({"pack":self.backend.set_style_pack_enabled(id, enabled)?}))
    }
    pub(super) fn activate_style(&self, id: &str) -> Result<Value, BackendError> {
        // Ordinary mode remains bundled local/raw, including after imports.
        if !self.developer_mode && id != "builtin.raw" {
            return Err(error(
                BackendErrorCode::PermissionDenied,
                "developer mode required for LLM styles",
            ));
        }
        Ok(json!({"pack":self.backend.activate_style_pack(id)?}))
    }
    pub(super) fn remove_style(&self, id: &str) -> Result<Value, BackendError> {
        value(self.backend.remove_style_pack(id)?)
    }
    pub(super) fn import_style_archive(&self, name: &str) -> Result<Value, BackendError> {
        let bytes = staged_archive(&self.style_staging_directory()?, name)?;
        // Core checks the ZIP's physical directory, entry names, inflated sizes,
        // manifest, prompt, examples and icons before writing the style store.
        Ok(json!({"pack":self.backend.import_style_pack_bytes(&bytes)?}))
    }
    pub(super) async fn marketplace_list(
        &self,
        query: Option<String>,
        sort: Option<String>,
        limit: Option<u32>,
    ) -> Result<Value, BackendError> {
        Ok(
            json!({"items":self.backend.services().marketplace.list(MarketplaceQuery { query, sort, limit }).await?}),
        )
    }
    pub(super) async fn marketplace_detail(&self, id: String) -> Result<Value, BackendError> {
        Ok(json!({"detail":self.backend.services().marketplace.detail(id).await?}))
    }
    pub(super) async fn marketplace_install(&self, id: String) -> Result<Value, BackendError> {
        Ok(json!({"pack":self.backend.services().marketplace.install(id).await?}))
    }
    pub(super) fn translation_configuration(&self) -> Value {
        let prefs = self.backend.get_preferences();
        let effective_targets: Vec<_> = prefs.translation_target_languages.iter()
            .filter(|target| translation_effective(true, target, &prefs.working_languages)).cloned().collect();
        json!({"catalog":catalog(),"targetLanguage":prefs.translation_target_language,
            "targetLanguages":prefs.translation_target_languages,"effectiveTargetLanguages":effective_targets,
            "workingLanguages":prefs.working_languages,"maxTargets":openless_core::language_catalog::translation_language_names().len(),"multipleTargetsSupported":true,
            "targetOrderingSupported":true,"providerLanguageSupport":"notVerified",
            "effective":translation_effective(true, &prefs.translation_target_language, &prefs.working_languages)})
    }
    pub(super) fn set_translation_target(&self, target: &str) -> Result<Value, BackendError> {
        let target = target.trim();
        let prefs = self.backend.get_preferences();
        // Existing custom names remain visible/removable. New values use the
        // canonical catalog name (not ASR locale IDs or a Swift-side rule).
        if !target.is_empty()
            && target != prefs.translation_target_language
            && !catalog()
                .as_array()
                .unwrap()
                .iter()
                .any(|language| language["nativeName"] == target)
        {
            return Err(invalid(
                "choose a native language name from the shared catalog, or empty to disable",
            ));
        }
        if target.is_empty() {
            self.backend.select_translation_target(None)?;
        } else {
            self.backend.add_translation_target(target.into())?;
            self.backend.select_translation_target(Some(target.into()))?;
        }
        Ok(self.translation_configuration())
    }
    pub(super) async fn capability_snapshot(&self) -> Result<Value, BackendError> {
        let prefs = self.backend.get_preferences();
        let status = self.backend.get_credentials_status().await?;
        let selected = self.backend.get_style_pack(&prefs.active_style_pack_id)?;
        let enabled = self.developer_mode && prefs.active_style_pack_id != "builtin.raw";
        let channel_id = self.backend.active_provider(ProviderSlot::Llm).await?;
        let channel = self
            .backend
            .list_channels(ChannelKind::Llm)
            .await?
            .into_iter()
            .find(|channel| channel.id == channel_id && channel.enabled);
        let provider_supported = channel
            .as_ref()
            .is_some_and(|channel| control::supported(ProviderKind::Llm, &channel.provider_type));
        let mut model = String::new();
        let mut api_key_present = false;
        if !channel_id.is_empty() {
            model = self
                .backend
                .read_credential(CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(channel_id.clone()),
                    LLM_MODEL_ACCOUNT,
                )?)
                .await?
                .map(|value| value.into_exposed())
                .unwrap_or_default();
            api_key_present = self
                .backend
                .read_credential(CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(channel_id),
                    LLM_API_KEY_ACCOUNT,
                )?)
                .await?
                .is_some_and(|value| !value.expose_secret().trim().is_empty());
        }
        let configured = status.llm_configured;
        let pipeline_mode =
            effective_pipeline_mode(prefs.multimodal_pipeline_enabled, prefs.pipeline_mode);
        let traditional = pipeline_mode == PipelineMode::Traditional;
        let provider_ready = self.developer_mode && traditional && configured && provider_supported;
        let can_attempt = enabled && provider_ready && selected.enabled;
        let can_translate = provider_ready && prefs.translation_target_languages.iter()
            .any(|target| translation_effective(true, target, &prefs.working_languages));
        let availability = if !enabled {
            "disabled"
        } else if !traditional {
            "requiresTraditionalPipeline"
        } else if !configured {
            "missingConfiguration"
        } else if !provider_supported {
            "unsupportedProvider"
        } else {
            "configuredNotVerified"
        };
        Ok(
            json!({"schemaVersion":1,"developerMode":self.developer_mode,
            "localRaw":{"available":true,"asrProviderType":"ios-sensevoice","requiresCloudCredentials":false,
                "availabilityMeaning":"registeredNativeRoute","verification":"nativeAdapterNotProbed"},
            "pipelineMode":pipeline_mode,"credentialsStatus":status,
            "asr":{"providerType":status.active_asr_provider,"configured":status.asr_configured,"verification":"notRecorded"},
            "omni":{"enabled":self.developer_mode && !traditional,"configured":status.omni_configured,
                "providerType":self.backend.active_provider(ProviderSlot::Omni).await?,
                "verification":"notRecorded","modelAvailability":"unknown","entitlement":"unknown"},
            "selectedStyle":{"id":selected.id,"name":selected.name,"baseMode":selected.base_mode,
                "enabled":selected.enabled,"recommendedModel":selected.recommended_model},
            "llm":{"enabled":enabled,"configured":configured,"providerSupported":provider_supported,
                "providerType":channel.map(|channel| channel.provider_type),"model":model,
                "apiKeyPresent":api_key_present,"availability":availability,"canAttemptPolish":can_attempt,
                "providerConfiguredAndEnabled":provider_ready,
                "canAttemptTranslation":can_translate,
                "canAttemptHelpWrite":can_attempt,
                "verification":"notRecorded","modelAvailability":"unknown","entitlement":"unknown"},
            "translation":self.translation_configuration(),
            "verificationLimits":["No request is made by this snapshot", "Configured credentials do not prove model access, entitlement or network availability", "validate is developer-only and its result is not persisted; SenseVoice validation is unsupported", "recommendedModel is advisory, not a model permission grant"]}),
        )
    }
}

#[cfg(test)]
#[path = "style_library_tests.rs"]
mod tests;
