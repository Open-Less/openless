//! Official-service cloud snapshots. Credentials and device-specific settings never enter the DTO.

use crate::cloud_sync_transaction::RestoreTransaction;
use crate::cloud_sync_types::*;
use crate::cloud_sync_validation::validate_payload;
use crate::credentials::SecretValue;
use crate::marketplace::MarketplaceService;
use crate::persistence::read_or_default;
use crate::{
    BackendError, BackendErrorCode, BackendRepositories, CorrectionRule, DictionaryEntry,
    StylePack, StylePackKind, UserPreferences,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

pub use crate::cloud_sync_types::{SyncFontScale, SyncLocale};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudSyncUiPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<SyncLocale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_scale: Option<SyncFontScale>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncCounts {
    pub dictionary: usize,
    pub corrections: usize,
    pub style_packs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncStatus {
    pub schema_version: u32,
    pub revision: u64,
    pub updated_at: Option<String>,
    pub has_snapshot: bool,
    pub counts: CloudSyncCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncRestoreResult {
    pub status: CloudSyncStatus,
    pub ui_preferences: CloudSyncUiPreferences,
}

pub(crate) struct CloudSyncService {
    marketplace: Arc<MarketplaceService>,
    repositories: BackendRepositories,
    settings_gate: Arc<Mutex<()>>,
    operation: tokio::sync::Mutex<()>,
}

impl CloudSyncService {
    pub(crate) fn new(
        marketplace: Arc<MarketplaceService>,
        repositories: BackendRepositories,
        settings_gate: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            marketplace,
            repositories,
            settings_gate,
            operation: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) async fn status(&self) -> Result<CloudSyncStatus, BackendError> {
        let _operation = self.operation.try_lock().map_err(|_| busy())?;
        let (snapshot, _) = self.fetch().await?;
        Ok(status(&snapshot))
    }

    pub(crate) async fn upload(
        &self,
        base_revision: u64,
        ui: CloudSyncUiPreferences,
    ) -> Result<CloudSyncStatus, BackendError> {
        let _operation = self.operation.try_lock().map_err(|_| busy())?;
        validate_base_revision(base_revision)?;
        let token = self.marketplace.read_access_token().await?;
        let payload = self.capture(ui)?;
        validate_payload(&payload).map_err(invalid)?;
        let body = serde_json::to_vec(&CloudSyncPutRequest {
            schema_version: SYNC_SCHEMA_VERSION,
            base_revision,
            payload,
        })
        .map_err(|_| invalid("无法编码本机同步数据。"))?;
        if body.len() > SYNC_MAX_BODY_BYTES {
            return Err(invalid("同步数据超过 2 MiB，请减少词条或风格包后重试。"));
        }
        let snapshot = self
            .request(reqwest::Method::PUT, Some(body), &token)
            .await?;
        Ok(status(&snapshot))
    }

    pub(crate) async fn delete(&self, base_revision: u64) -> Result<CloudSyncStatus, BackendError> {
        let _operation = self.operation.try_lock().map_err(|_| busy())?;
        validate_base_revision(base_revision)?;
        let token = self.marketplace.read_access_token().await?;
        let body = serde_json::to_vec(&CloudSyncDeleteRequest { base_revision })
            .map_err(|_| invalid("无法编码云端删除请求。"))?;
        let snapshot = self
            .request(reqwest::Method::DELETE, Some(body), &token)
            .await?;
        Ok(status(&snapshot))
    }

    pub(crate) async fn restore(&self) -> Result<CloudSyncRestoreResult, BackendError> {
        let _operation = self.operation.try_lock().map_err(|_| busy())?;
        let (snapshot, token) = self.fetch().await?;
        let payload = snapshot.payload.as_ref().ok_or_else(|| {
            BackendError::new(BackendErrorCode::InvalidState, "云端还没有可恢复的备份。")
        })?;
        // A sign-out or account switch while downloading must not apply the previous account's data.
        if self.marketplace.read_access_token().await? != token {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "GitHub 账号已切换，请重新读取云端备份。",
            ));
        }
        self.restore_local(payload)?;
        Ok(CloudSyncRestoreResult {
            status: status(&snapshot),
            ui_preferences: CloudSyncUiPreferences {
                locale: payload.preferences.locale,
                font_scale: payload.preferences.font_scale,
            },
        })
    }

    async fn fetch(&self) -> Result<(CloudSyncSnapshot, SecretValue), BackendError> {
        let token = self.marketplace.read_access_token().await?;
        let snapshot = self.request(reqwest::Method::GET, None, &token).await?;
        Ok((snapshot, token))
    }

    async fn request(
        &self,
        method: reqwest::Method,
        body: Option<Vec<u8>>,
        token: &SecretValue,
    ) -> Result<CloudSyncSnapshot, BackendError> {
        let url = self.marketplace.cloud_sync_url("me/sync")?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid("云同步服务必须使用 HTTPS。"));
        }
        let is_read = method == reqwest::Method::GET;
        let mut request = crate::net::credential_http_for_url(url.as_str())
            .request(method, url)
            .bearer_auth(token.expose_secret())
            .timeout(Duration::from_secs(30));
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        // Mutations are never retried automatically: a dropped response can follow a committed write.
        let response = request.send().await.map_err(|_| {
            BackendError::new(
                if is_read {
                    BackendErrorCode::Provider
                } else {
                    BackendErrorCode::OutcomeUnknown
                },
                if is_read {
                    "无法连接云同步服务，请稍后重试。"
                } else {
                    "云端操作结果未确认，请刷新云端状态后再试。"
                },
            )
        })?;
        let http_status = response.status();
        if http_status.is_redirection() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "云同步服务返回了不受支持的重定向。",
            ));
        }
        if matches!(http_status.as_u16(), 401 | 403) {
            return Err(BackendError::new(
                BackendErrorCode::PermissionDenied,
                "GitHub 登录已失效，请重新登录后同步。",
            ));
        }
        if matches!(http_status.as_u16(), 404 | 405 | 501) {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "官方服务暂未启用云同步，请稍后重试。",
            ));
        }
        if http_status == reqwest::StatusCode::CONFLICT {
            let bytes = read_response(response, 8192).await?;
            let conflict: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
            return Err(BackendError {
                code: BackendErrorCode::Busy,
                message: "云端备份已被其他设备更新，请刷新后再选择上传或恢复。".into(),
                retryable: false,
                details: Some(
                    serde_json::json!({"reason":"revision_conflict","currentRevision":conflict.get("revision").and_then(serde_json::Value::as_u64)}),
                ),
            });
        }
        if !http_status.is_success() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!(
                    "云同步请求失败（HTTP {}），本机数据保持不变。",
                    http_status.as_u16()
                ),
            ));
        }
        let bytes = read_response(response, SYNC_MAX_BODY_BYTES + 1024).await?;
        let snapshot: CloudSyncSnapshot =
            serde_json::from_slice(&bytes).map_err(|_| invalid_remote())?;
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn capture(&self, ui: CloudSyncUiPreferences) -> Result<CloudSyncPayload, BackendError> {
        let _settings = self
            .settings_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // This order is also used by restore; no store lock is held across a network await.
        let (_dictionary_guard, dictionary_path) =
            self.repositories.vocabulary.cloud_sync_access()?;
        let (_corrections_guard, corrections_path) =
            self.repositories.correction_rules.cloud_sync_access()?;
        let (packs, _, _) = self.repositories.style_packs.cloud_sync_access()?;
        let (preferences, _) = self.repositories.preferences.cloud_sync_access();
        let dictionary: Vec<DictionaryEntry> = read_or_default(dictionary_path)?;
        let corrections: Vec<CorrectionRule> = read_or_default(corrections_path)?;
        let style_packs = packs
            .iter()
            .map(|pack| {
                let icon = self.repositories.style_packs.icon_data_url_for_pack(pack)?;
                Ok(to_wire_pack(
                    pack,
                    icon.and_then(|icon| {
                        icon.strip_prefix("data:image/png;base64,")
                            .map(str::to_owned)
                    }),
                ))
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        Ok(CloudSyncPayload {
            dictionary: dictionary
                .into_iter()
                .map(|entry| SyncDictionaryEntry {
                    id: entry.id,
                    phrase: entry.phrase,
                    note: entry.note,
                    enabled: entry.enabled,
                    hits: entry.hits,
                    created_at: entry.created_at,
                })
                .collect(),
            corrections: corrections
                .into_iter()
                .map(|rule| SyncCorrectionRule {
                    id: rule.id,
                    pattern: rule.pattern,
                    replacement: rule.replacement,
                    enabled: rule.enabled,
                    created_at: rule.created_at,
                    source: rule.source,
                })
                .collect(),
            style_packs,
            preferences: capture_preferences(&preferences, ui),
        })
    }

    fn restore_local(&self, payload: &CloudSyncPayload) -> Result<(), BackendError> {
        validate_payload(payload).map_err(|_| invalid_remote())?;
        let mut next_packs = validated_native_packs(payload)?;
        let dictionary: Vec<DictionaryEntry> = payload
            .dictionary
            .iter()
            .map(|entry| DictionaryEntry {
                id: entry.id.clone(),
                phrase: entry.phrase.clone(),
                note: entry.note.clone(),
                enabled: entry.enabled,
                hits: entry.hits,
                created_at: entry.created_at.clone(),
            })
            .collect();
        let corrections: Vec<CorrectionRule> = payload
            .corrections
            .iter()
            .map(|rule| CorrectionRule {
                id: rule.id.clone(),
                pattern: rule.pattern.clone(),
                replacement: rule.replacement.clone(),
                enabled: rule.enabled,
                created_at: rule.created_at.clone(),
                source: rule.source,
            })
            .collect();
        for rule in &corrections {
            crate::correction::validate_correction_rule_syntax(&rule.pattern, &rule.replacement)
                .map_err(|_| invalid_remote())?;
        }
        let _settings = self
            .settings_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_dictionary_guard, dictionary_path) =
            self.repositories.vocabulary.cloud_sync_access()?;
        let (_corrections_guard, corrections_path) =
            self.repositories.correction_rules.cloud_sync_access()?;
        let (mut packs, packs_path, assets) = self.repositories.style_packs.cloud_sync_access()?;
        let (mut preferences, preferences_path) = self.repositories.preferences.cloud_sync_access();
        let mut next_preferences = preferences.clone();
        apply_preferences(&mut next_preferences, &payload.preferences);
        crate::sync_style_pack_preferences(&mut next_preferences, &next_packs);
        if !crate::SettingsEffectPlan::between(&preferences, &next_preferences).is_empty() {
            return Err(invalid("云端偏好包含不能跨设备还原的运行设置。"));
        }
        let mut writes = Vec::new();
        for (wire, pack) in payload.style_packs.iter().zip(next_packs.iter_mut()) {
            // `validated_native_packs` retains source order; normalisation happens after icon assignment.
            if let Some(encoded) = &wire.icon_png_base64 {
                let png = STANDARD.decode(encoded).map_err(|_| invalid_remote())?;
                crate::style_pack_archive::validate_icon_content("png", &png)
                    .map_err(|_| invalid_remote())?;
                let target = owned_icon_target(assets, &pack.id)?;
                pack.icon_path = Some(target.to_string_lossy().into_owned());
                writes.push((target, png));
            }
        }
        crate::style_pack_store::normalize_cloud_style_packs(&mut next_packs);
        crate::sync_style_pack_preferences(&mut next_preferences, &next_packs);
        writes.extend([
            (dictionary_path.to_path_buf(), encode(&dictionary)?),
            (corrections_path.to_path_buf(), encode(&corrections)?),
            (packs_path.to_path_buf(), encode(&next_packs)?),
            (preferences_path.to_path_buf(), encode(&next_preferences)?),
        ]);
        RestoreTransaction::prepare(writes)?.commit()?;
        *packs = next_packs;
        *preferences = next_preferences;
        Ok(())
    }
}

fn status(snapshot: &CloudSyncSnapshot) -> CloudSyncStatus {
    CloudSyncStatus {
        schema_version: snapshot.schema_version,
        revision: snapshot.revision,
        updated_at: snapshot.updated_at.clone(),
        has_snapshot: snapshot.payload.is_some(),
        counts: snapshot
            .payload
            .as_ref()
            .map(|payload| CloudSyncCounts {
                dictionary: payload.dictionary.len(),
                corrections: payload.corrections.len(),
                style_packs: payload.style_packs.len(),
            })
            .unwrap_or_default(),
    }
}

async fn read_response(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, BackendError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(invalid_remote());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            BackendError::new(
                BackendErrorCode::Provider,
                "读取云端备份失败，本机数据保持不变。",
            )
        })?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(invalid_remote());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_snapshot(snapshot: &CloudSyncSnapshot) -> Result<(), BackendError> {
    if snapshot.schema_version != SYNC_SCHEMA_VERSION
        || snapshot.revision > SYNC_MAX_REVISION
        || (snapshot.revision == 0 && (snapshot.payload.is_some() || snapshot.updated_at.is_some()))
        || (snapshot.revision > 0
            && snapshot
                .updated_at
                .as_deref()
                .is_none_or(|at| chrono::DateTime::parse_from_rfc3339(at).is_err()))
    {
        return Err(invalid_remote());
    }
    if let Some(payload) = &snapshot.payload {
        validate_payload(payload).map_err(|_| invalid_remote())?;
    }
    Ok(())
}

fn validated_native_packs(payload: &CloudSyncPayload) -> Result<Vec<StylePack>, BackendError> {
    payload
        .style_packs
        .iter()
        .map(|pack| {
            if pack.id == "."
                || pack.id == ".."
                || !pack
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                || (pack.kind == StylePackKind::Builtin
                    && pack.id != crate::builtin_style_pack_id(pack.base_mode))
                || (pack.kind == StylePackKind::Imported && pack.id.starts_with("builtin."))
            {
                return Err(invalid_remote());
            }
            Ok(StylePack {
                id: pack.id.clone(),
                name: pack.name.clone(),
                description: pack.description.clone(),
                author: pack.author.clone(),
                version: pack.version.clone(),
                kind: pack.kind,
                base_mode: pack.base_mode,
                selection_prompt: pack.selection_prompt.clone(),
                prompt: pack.prompt.clone(),
                examples: pack
                    .examples
                    .iter()
                    .map(|example| crate::StylePackExample {
                        title: example.title.clone(),
                        input: example.input.clone(),
                        output: example.output.clone(),
                    })
                    .collect(),
                tags: pack.tags.clone(),
                icon_path: None,
                created_at: pack.created_at.clone(),
                updated_at: pack.updated_at.clone(),
                enabled: pack.enabled,
                active: false,
                recommended_model: pack.recommended_model.clone(),
                compatible_app_version: pack.compatible_app_version.clone(),
                origin_pack_id: pack.origin_pack_id.clone(),
                origin_author_login: pack.origin_author_login.clone(),
            })
        })
        .collect()
}

fn to_wire_pack(pack: &StylePack, icon_png_base64: Option<String>) -> SyncStylePack {
    SyncStylePack {
        id: pack.id.clone(),
        name: pack.name.clone(),
        description: pack.description.clone(),
        author: pack.author.clone(),
        version: pack.version.clone(),
        kind: pack.kind,
        base_mode: pack.base_mode,
        selection_prompt: pack.selection_prompt.clone(),
        prompt: pack.prompt.clone(),
        examples: pack
            .examples
            .iter()
            .map(|example| SyncStylePackExample {
                title: example.title.clone(),
                input: example.input.clone(),
                output: example.output.clone(),
            })
            .collect(),
        tags: pack.tags.clone(),
        icon_png_base64,
        created_at: pack.created_at.clone(),
        updated_at: pack.updated_at.clone(),
        enabled: pack.enabled,
        recommended_model: pack.recommended_model.clone(),
        compatible_app_version: pack.compatible_app_version.clone(),
        origin_pack_id: pack.origin_pack_id.clone(),
        origin_author_login: pack.origin_author_login.clone(),
    }
}

fn capture_preferences(
    preferences: &UserPreferences,
    ui: CloudSyncUiPreferences,
) -> SyncPreferences {
    SyncPreferences {
        theme_mode: Some(preferences.theme_mode),
        capsule_style: Some(preferences.capsule_style),
        default_mode: Some(preferences.default_mode),
        enabled_modes: Some(preferences.enabled_modes.clone()),
        active_style_pack_id: Some(preferences.active_style_pack_id.clone()),
        selection_polish_style_pack_id: Some(preferences.selection_polish_style_pack_id.clone()),
        working_languages: Some(preferences.working_languages.clone()),
        translation_target_language: Some(preferences.translation_target_language.clone()),
        chinese_script_preference: Some(preferences.chinese_script_preference),
        output_language_preference: Some(preferences.output_language_preference),
        qa_save_history: Some(preferences.qa_save_history),
        show_capsule: Some(preferences.show_capsule),
        audio_cue_on_record: Some(preferences.audio_cue_on_record),
        mute_during_recording: Some(preferences.mute_during_recording),
        silence_auto_stop_enabled: Some(preferences.silence_auto_stop_enabled),
        silence_auto_stop_seconds: Some(preferences.silence_auto_stop_seconds),
        show_overview_activity_heatmap: Some(preferences.show_overview_activity_heatmap),
        llm_thinking_enabled: Some(preferences.llm_thinking_enabled),
        locale: ui.locale,
        font_scale: ui.font_scale,
    }
}

fn apply_preferences(preferences: &mut UserPreferences, incoming: &SyncPreferences) {
    macro_rules! apply { ($($field:ident),+ $(,)?) => { $(if let Some(value) = &incoming.$field { preferences.$field = value.clone(); })+ }; }
    apply!(
        theme_mode,
        capsule_style,
        default_mode,
        enabled_modes,
        active_style_pack_id,
        selection_polish_style_pack_id,
        working_languages,
        translation_target_language,
        chinese_script_preference,
        output_language_preference,
        qa_save_history,
        show_capsule,
        audio_cue_on_record,
        mute_during_recording,
        silence_auto_stop_enabled,
        silence_auto_stop_seconds,
        show_overview_activity_heatmap,
        llm_thinking_enabled
    );
}

fn owned_icon_target(assets: &Path, id: &str) -> Result<PathBuf, BackendError> {
    std::fs::create_dir_all(assets).map_err(|_| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "create style pack asset root",
        )
    })?;
    let root = assets.canonicalize().map_err(|_| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "resolve style pack asset root",
        )
    })?;
    let directory = root.join(id);
    std::fs::create_dir_all(&directory).map_err(|_| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "create style pack icon directory",
        )
    })?;
    let directory = directory.canonicalize().map_err(|_| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "resolve style pack icon directory",
        )
    })?;
    if !directory.starts_with(&root) {
        return Err(invalid_remote());
    }
    Ok(directory.join(format!("icon-{}.png", uuid::Uuid::new_v4().simple())))
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, BackendError> {
    serde_json::to_vec_pretty(value).map_err(|_| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "encode local cloud restore document",
        )
    })
}
fn invalid(message: &str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, message)
}
fn invalid_remote() -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        "云端备份格式不兼容或超出限制，本机数据保持不变。",
    )
}
fn busy() -> BackendError {
    BackendError::new(BackendErrorCode::Busy, "已有云同步操作正在进行。")
}
fn validate_base_revision(revision: u64) -> Result<(), BackendError> {
    if revision >= SYNC_MAX_REVISION {
        Err(invalid("云端版本号无效，请先刷新云端状态。"))
    } else {
        Ok(())
    }
}
