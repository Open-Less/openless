#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Credentials vault.
//!
//! 正常读写走系统凭据库；旧 plaintext JSON 只作为迁移来源。为保持多 provider
//! schema 与 active provider 状态，凭据库里保存一个 JSON payload。macOS 使用一个稳定
//! 条目，避免每个分片分别要求授权；Windows 按单条凭据 2560 bytes 的限制拆分。
//!
//! v1 schema：
//!   {
//!     "version": 1,
//!     "active": { "asr": "<id>", "llm": "<id>" },
//!     "providers": {
//!       "asr": { "<id>": { "appKey", "accessKey", "resourceId", "apiKey", "baseURL", "model", "vocabularyId" } },
//!       "llm": { "<id>": { "displayName", "apiKey", "baseURL", "model", "temperature", "extraHeaders" } }
//!     },
//!     "marketplace": { "githubAccessToken": "<desktop-only secret>" }
//!   }
//!
//! Android stores the same payload in a versioned AES-GCM envelope whose key is
//! non-exportable from Android Keystore. Marketplace OAuth remains
//! process-memory-only and is deliberately stripped from `credentials.enc.json`.
//!
//! "ark.api_key"/"volcengine.app_key" 等账户名按 Swift 语义路由到 active provider。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub use openless_core::{ChannelKind, ChannelSummary, ChannelTestSummary};

// `anyhow!` is only invoked from the keyring (non-Android) code paths; gating the
// import keeps the Android build free of an unused-import warning.
#[cfg(not(target_os = "android"))]
use anyhow::anyhow;

/// 旧版 plaintext JSON 凭据路径。仅作为迁移来源；成功写入系统凭据库后会删除。
const LEGACY_CREDS_DIR: &str = ".openless";
const LEGACY_CREDS_FILE: &str = "credentials.json";

const KEYRING_CREDENTIALS_ACCOUNT: &str = "credentials.v1";
const KEYRING_CREDENTIALS_CHUNK_PREFIX: &str = "credentials.v1.chunk.";
const KEYRING_SINGLE_CREDENTIALS_ACCOUNT: &str = "credentials.v2";
#[cfg(target_os = "android")]
const ANDROID_CREDENTIALS_FILE: &str = "credentials.enc.json";
const RESERVED_EXTRA_HEADER_NAMES: &[&str] = &[
    "authorization",
    "content-type",
    "accept",
    "host",
    "content-length",
];
// Windows Credential Manager caps one credential blob at 2560 bytes. keyring stores
// passwords as UTF-16 on Windows, so keep each JSON chunk comfortably below that.
#[cfg(any(not(any(target_os = "macos", target_os = "android")), test))]
const KEYRING_CHUNK_MAX_UTF16_UNITS: usize = 1000;

static CREDENTIALS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SYNC_WRITE_GATE: OnceLock<
    Mutex<Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>>>,
> = OnceLock::new();

// Keychain 访问节流：每进程只补写/扫描一次，避免重复授权弹窗。
#[cfg(not(target_os = "android"))]
static SINGLE_ITEM_MIGRATION_ATTEMPTED: AtomicBool = AtomicBool::new(false);
#[cfg(not(target_os = "android"))]
static LEGACY_KEYRING_PROBE: OnceLock<Result<Option<CredsRoot>, String>> = OnceLock::new();
#[cfg(any(not(target_os = "android"), test))]
static VAULT_SOURCE_LOGGED: AtomicBool = AtomicBool::new(false);

// A rejected Marketplace token must become unusable before best-effort durable
// deletion starts. Keychain/credential-manager deletion can fail or prompt, so
// this process-local tombstone is authoritative for every read until a newly
// verified token has been saved successfully.
static MARKETPLACE_TOKEN_REJECTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "android")]
static ANDROID_MARKETPLACE_TOKEN: OnceLock<Mutex<Option<MarketplaceGithubToken>>> = OnceLock::new();

#[cfg(target_os = "android")]
static ANDROID_MARKETPLACE_LEGACY_SCRUBBED: OnceLock<Mutex<bool>> = OnceLock::new();

fn credentials_lock() -> &'static Mutex<()> {
    CREDENTIALS_LOCK.get_or_init(|| Mutex::new(()))
}

fn sync_write_gate() -> Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>> {
    SYNC_WRITE_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .clone()
}

fn install_sync_write_gate(
    installed: &mut Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>>,
    gate: std::sync::Arc<openless_core::credentials::SyncWriteGate>,
) -> Result<()> {
    if let Some(current) = installed.as_ref() {
        anyhow::ensure!(
            std::sync::Arc::ptr_eq(current, &gate),
            "credential vault already has a different encrypted sync gate"
        );
    } else {
        *installed = Some(gate);
    }
    Ok(())
}

fn sync_access_error(
    error: openless_core::cloud_sync_e2ee_documents::DocumentError,
) -> anyhow::Error {
    use openless_core::cloud_sync_e2ee_documents::DocumentError;
    use openless_core::{BackendError, BackendErrorCode};
    let code = if error == DocumentError::SourceChanged {
        BackendErrorCode::Busy
    } else {
        BackendErrorCode::OutcomeUnknown
    };
    BackendError::new(
        code,
        if code == BackendErrorCode::Busy {
            "credential store is busy with encrypted sync"
        } else {
            "credential store requires encrypted sync recovery"
        },
    )
    .into()
}

/// The lease precedes both the vault lock and the initial read. Cancellation of
/// the async caller cannot release it: this entire function runs in its worker.
fn mutate_credentials(
    origin: openless_core::credentials::ChangeOrigin,
    update: impl FnOnce(&mut CredsRoot) -> Result<bool>,
) -> Result<()> {
    mutate_credentials_with(
        sync_write_gate(),
        origin,
        load_credentials_for_update,
        update,
        save_credentials,
    )
}

fn mutate_credentials_with(
    gate: Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>>,
    origin: openless_core::credentials::ChangeOrigin,
    load: impl FnOnce() -> Result<CredsRoot>,
    update: impl FnOnce(&mut CredsRoot) -> Result<bool>,
    persist: impl FnOnce(&CredsRoot) -> Result<()>,
) -> Result<()> {
    let was_unbound = gate.is_none();
    let permit = gate
        .map(|gate| gate.begin_mutation())
        .transpose()
        .map_err(sync_access_error)?;
    let _guard = credentials_lock().lock();
    // First binding may win the lock after this worker observed no gate.
    // Such a worker must retry; it cannot write through a newly installed barrier.
    if was_unbound && sync_write_gate().is_some() {
        return Err(sync_access_error(
            openless_core::cloud_sync_e2ee_documents::DocumentError::SourceChanged,
        ));
    }
    let prepared = (|| -> Result<(CredsRoot, bool)> {
        let mut root = load()?;
        let changed = update(&mut root)?;
        Ok((root, changed))
    })();
    let (root, changed) = match prepared {
        Ok(result) => result,
        Err(error) => {
            if let Some(permit) = permit {
                permit.abort_unmodified().map_err(sync_access_error)?;
            }
            return Err(error);
        }
    };
    if !changed {
        if let Some(permit) = permit {
            permit.abort_unmodified().map_err(sync_access_error)?;
        }
        return Ok(());
    }
    if let Err(error) = persist(&root) {
        // The chunked writer can prove that the old manifest never changed.
        // Other native failures remain uncertain and intentionally retain intent.
        if matches!(
            error.downcast_ref::<VaultCommitFailure>(),
            Some(VaultCommitFailure::Unchanged)
        ) {
            if let Some(permit) = permit {
                permit.abort_unmodified().map_err(sync_access_error)?;
            }
        }
        return Err(error);
    }
    if let Some(permit) = permit {
        permit.commit(origin).map_err(sync_access_error)?;
    }
    Ok(())
}

fn require_sync_exclusive(permit: &openless_core::credentials::ExclusivePermit) -> Result<()> {
    let gate = sync_write_gate()
        .ok_or_else(|| anyhow::anyhow!("encrypted sync credential gate is not bound"))?;
    if !permit.belongs_to(&gate) {
        anyhow::bail!("encrypted sync credential lease belongs to another gate");
    }
    Ok(())
}

fn validate_sync_key(value: &openless_core::SecretValue) -> Result<()> {
    use base64::Engine;
    let encoded = value.expose_secret();
    anyhow::ensure!(encoded.len() == 43, "invalid encrypted sync key encoding");
    let decoded = zeroize::Zeroizing::new(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| anyhow::anyhow!("invalid encrypted sync key encoding"))?,
    );
    anyhow::ensure!(
        decoded.len() == 32
            && base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&*decoded) == encoded,
        "invalid encrypted sync key encoding"
    );
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn read_sync_secret_raw(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<Option<openless_core::SecretValue>> {
    get_keyring_password(account.as_str())?
        .map(|raw| {
            let value = openless_core::SecretValue::new(raw);
            validate_sync_key(&value)?;
            Ok(value)
        })
        .transpose()
}

#[cfg(not(target_os = "android"))]
fn write_sync_secret_raw(
    account: &openless_core::credentials::SyncSecretAccount,
    value: &openless_core::SecretValue,
) -> Result<()> {
    set_keyring_password(account.as_str(), value.expose_secret())
}

#[cfg(not(target_os = "android"))]
fn remove_sync_secret_raw(account: &openless_core::credentials::SyncSecretAccount) -> Result<()> {
    match keyring_entry_for(account.as_str())?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => anyhow::bail!("could not remove encrypted sync key from system credential store"),
    }
}

#[cfg(any(target_os = "android", test))]
struct SyncSecretCrypto<C> {
    inner: C,
    account: String,
}

#[cfg(any(target_os = "android", test))]
impl<C> SyncSecretCrypto<C> {
    fn aad(&self, original: &[u8]) -> Vec<u8> {
        let mut aad = b"openless-e2ee-local-keystore\0v1\0".to_vec();
        aad.extend_from_slice(self.account.as_bytes());
        aad.push(0);
        aad.extend_from_slice(original);
        aad
    }
}

#[cfg(any(target_os = "android", test))]
impl<C: super::android_credentials::AndroidCredentialsCrypto>
    super::android_credentials::AndroidCredentialsCrypto for SyncSecretCrypto<C>
{
    fn seal(
        &mut self,
        plaintext: &[u8],
        aad: &[u8],
    ) -> std::result::Result<
        super::android_credentials::SealedPayload,
        super::android_credentials::CryptoErrorKind,
    > {
        self.inner.seal(plaintext, &self.aad(aad))
    }
    fn open(
        &mut self,
        sealed: &super::android_credentials::SealedPayload,
        aad: &[u8],
    ) -> std::result::Result<Vec<u8>, super::android_credentials::CryptoErrorKind> {
        self.inner.open(sealed, &self.aad(aad))
    }
    fn delete_key(
        &mut self,
    ) -> std::result::Result<(), super::android_credentials::CryptoErrorKind> {
        // This namespace never owns the provider vault's shared master key.
        Ok(())
    }
    fn migration_complete(
        &mut self,
    ) -> std::result::Result<bool, super::android_credentials::CryptoErrorKind> {
        // Sync keys have never had a plaintext format. Reject downgrade without
        // reading or modifying the provider vault's independent migration marker.
        Ok(true)
    }
    fn mark_migration_complete(
        &mut self,
    ) -> std::result::Result<(), super::android_credentials::CryptoErrorKind> {
        Ok(())
    }
}

#[cfg(any(target_os = "android", test))]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AndroidSyncKeyRecord {
    version: u32,
    account: String,
    value: String,
}

#[cfg(any(target_os = "android", test))]
impl Drop for AndroidSyncKeyRecord {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.value.zeroize();
    }
}

#[cfg(any(target_os = "android", test))]
fn read_android_sync_key_with_crypto(
    path: &Path,
    account: &openless_core::credentials::SyncSecretAccount,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<Option<openless_core::SecretValue>> {
    use super::android_credentials::ReadOutcome;
    match std::fs::metadata(path) {
        Ok(meta) => anyhow::ensure!(
            meta.len() <= 16_384,
            "encrypted sync key envelope exceeds limit"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => anyhow::bail!("could not inspect encrypted sync key envelope"),
    }
    let bytes = match super::android_credentials::read(path, crypto)
        .map_err(|_| anyhow::anyhow!("could not open Android Keystore sync key"))?
    {
        ReadOutcome::Missing => return Ok(None),
        ReadOutcome::Legacy(mut bytes) => {
            use zeroize::Zeroize;
            bytes.zeroize();
            anyhow::bail!("legacy plaintext sync keys are not supported");
        }
        ReadOutcome::Plaintext(bytes) => zeroize::Zeroizing::new(bytes),
    };
    let mut record: AndroidSyncKeyRecord = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid encrypted sync key record"))?;
    anyhow::ensure!(
        record.version == 1 && record.account == account.as_str(),
        "encrypted sync key binding mismatch"
    );
    let value = openless_core::SecretValue::new(std::mem::take(&mut record.value));
    validate_sync_key(&value)?;
    Ok(Some(value))
}

#[cfg(any(target_os = "android", test))]
fn write_android_sync_key_with_crypto(
    path: &Path,
    account: &openless_core::credentials::SyncSecretAccount,
    value: &openless_core::SecretValue,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<()> {
    validate_sync_key(value)?;
    let record = AndroidSyncKeyRecord {
        version: 1,
        account: account.as_str().into(),
        value: value.expose_secret().into(),
    };
    let bytes = zeroize::Zeroizing::new(
        serde_json::to_vec(&record)
            .map_err(|_| anyhow::anyhow!("cannot encode encrypted sync key record"))?,
    );
    super::android_credentials::write_verified(path, &bytes, crypto)
        .map_err(|_| anyhow::anyhow!("could not save Android Keystore sync key"))
}

#[cfg(target_os = "android")]
fn android_sync_key_path(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<PathBuf> {
    let credential_path = android_credentials_path()?;
    let parent = credential_path
        .parent()
        .context("Android credential directory is unavailable")?;
    Ok(parent
        .join("sync-secrets")
        .join(format!("{}.json", account.as_str())))
}

#[cfg(target_os = "android")]
fn read_sync_secret_raw(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<Option<openless_core::SecretValue>> {
    let mut crypto = SyncSecretCrypto {
        inner: super::android_credentials::AndroidKeystoreCrypto,
        account: account.as_str().into(),
    };
    read_android_sync_key_with_crypto(&android_sync_key_path(account)?, account, &mut crypto)
}

#[cfg(target_os = "android")]
fn write_sync_secret_raw(
    account: &openless_core::credentials::SyncSecretAccount,
    value: &openless_core::SecretValue,
) -> Result<()> {
    let mut crypto = SyncSecretCrypto {
        inner: super::android_credentials::AndroidKeystoreCrypto,
        account: account.as_str().into(),
    };
    write_android_sync_key_with_crypto(
        &android_sync_key_path(account)?,
        account,
        value,
        &mut crypto,
    )
}

#[cfg(target_os = "android")]
fn remove_sync_secret_raw(account: &openless_core::credentials::SyncSecretAccount) -> Result<()> {
    let path = android_sync_key_path(account)?;
    // Remove only this binding, never the shared non-exportable Keystore master key.
    for item in [
        path.clone(),
        path.with_extension("json.tmp"),
        path.with_extension("json.pending"),
    ] {
        super::android_credentials::secure_remove(&item)
            .map_err(|_| anyhow::anyhow!("could not remove Android sync key envelope"))?;
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn android_marketplace_token() -> &'static Mutex<Option<MarketplaceGithubToken>> {
    ANDROID_MARKETPLACE_TOKEN.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn android_marketplace_legacy_scrubbed() -> &'static Mutex<bool> {
    ANDROID_MARKETPLACE_LEGACY_SCRUBBED.get_or_init(|| Mutex::new(false))
}

/// Cache successful reads for this process and refresh only after durable writes.
/// Failed reads remain retryable and must not become a cached empty configuration.
/// External Keychain edits take effect on the next app launch.
static CREDENTIALS_CACHE: OnceLock<Mutex<Option<CredsRoot>>> = OnceLock::new();
static LAST_VAULT_READ_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static LAST_VAULT_READ_ERROR_LOGGED: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn credentials_cache() -> &'static Mutex<Option<CredsRoot>> {
    CREDENTIALS_CACHE.get_or_init(|| Mutex::new(None))
}

fn last_vault_read_error_slot() -> &'static Mutex<Option<String>> {
    LAST_VAULT_READ_ERROR.get_or_init(|| Mutex::new(None))
}

fn last_vault_read_error_logged_slot() -> &'static Mutex<Option<String>> {
    LAST_VAULT_READ_ERROR_LOGGED.get_or_init(|| Mutex::new(None))
}

fn store_credentials_cache(root: &CredsRoot) {
    *credentials_cache().lock() = Some(root.clone());
    clear_vault_read_error();
}

fn record_vault_read_failure(error: &anyhow::Error) {
    // Serde errors may echo a malformed field's raw string (including a secret).
    // Preserve native Keystore error categories while keeping payload diagnostics local.
    let chain = if error.downcast_ref::<serde_json::Error>().is_some() {
        "credential payload could not be decoded".to_string()
    } else {
        format!("{error:#}")
    };
    *last_vault_read_error_slot().lock() = Some(chain.clone());
    let mut logged = last_vault_read_error_logged_slot().lock();
    if logged.as_deref() != Some(chain.as_str()) {
        log::warn!("[vault] credential read failed: {chain}");
        *logged = Some(chain);
    }
}

/// Mutations must not persist an empty default over an unreadable envelope.
/// Returning `Err` lets Core surface Persistence after a real Keystore retry.
#[cfg(any(target_os = "android", test))]
fn android_credentials_root_for_update(
    loader: impl FnOnce() -> Result<Option<CredsRoot>>,
) -> Result<CredsRoot> {
    match loader() {
        Ok(loaded) => {
            let root = loaded.unwrap_or_default();
            clear_vault_read_error();
            store_credentials_cache(&root);
            Ok(root)
        }
        Err(error) => {
            record_vault_read_failure(&error);
            Err(error)
        }
    }
}

fn clear_vault_read_error() {
    *last_vault_read_error_slot().lock() = None;
    *last_vault_read_error_logged_slot().lock() = None;
}

#[cfg(test)]
fn reset_credentials_cache_for_tests() {
    *credentials_cache().lock() = None;
    clear_vault_read_error();
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsRoot {
    #[serde(default = "credsroot_default_version")]
    version: u32,
    #[serde(default)]
    active: CredsActive,
    #[serde(default)]
    providers: CredsProviders,
    /// 多模态识别管线（issue #902）专用凭据命名空间，与 asr/llm 完全隔离：
    /// 运行时只在 `pipeline_mode == multimodal` 时读取，切换模式不删除。
    #[serde(default)]
    omni: CredsOmni,
    #[serde(default, skip_serializing_if = "is_zero")]
    metadata_revision: u64,
    #[serde(default, skip_serializing_if = "CredsMarketplace::is_empty")]
    marketplace: CredsMarketplace,
}

fn credsroot_default_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CredsActive {
    #[serde(default = "creds_default_asr")]
    asr: String,
    #[serde(default = "creds_default_llm")]
    llm: String,
}

impl Default for CredsActive {
    fn default() -> Self {
        Self {
            asr: creds_default_asr(),
            llm: creds_default_llm(),
        }
    }
}

fn creds_default_asr() -> String {
    #[cfg(target_os = "windows")]
    {
        return crate::asr::local::foundry::PROVIDER_ID.into();
    }
    #[cfg(not(target_os = "windows"))]
    {
        "volcengine".into()
    }
}
fn creds_default_llm() -> String {
    "ark".into()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct CredsProviders {
    #[serde(default)]
    asr: HashMap<String, CredsAsrEntry>,
    #[serde(default)]
    llm: HashMap<String, CredsLlmEntry>,
}

/// 多模态（Omni）模型配置：一个 active provider + 按 provider 隔离的 entry。
/// entry 字段形状与 LLM 对齐（API Key / Base URL / Model / 温度 / 额外请求头），
/// 但存放在独立命名空间，绝不与 `providers.llm` 共享槽位。
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CredsOmni {
    #[serde(default = "creds_default_omni")]
    active: String,
    #[serde(default)]
    providers: HashMap<String, CredsOmniEntry>,
}

impl Default for CredsOmni {
    fn default() -> Self {
        Self {
            active: creds_default_omni(),
            providers: HashMap::new(),
        }
    }
}

fn creds_default_omni() -> String {
    "custom".into()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
#[allow(non_snake_case)]
struct CredsOmniEntry {
    #[serde(flatten)]
    channel: ChannelMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    displayName: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraHeaders: Option<HashMap<String, String>>,
}

impl CredsOmniEntry {
    fn is_empty(&self) -> bool {
        if self.channel.providerType.is_some() {
            return false;
        }
        self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.temperature.is_none()
            && self
                .extraHeaders
                .as_ref()
                .map(|h| h.is_empty())
                .unwrap_or(true)
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsMarketplace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    githubAccessToken: Option<MarketplaceGithubToken>,
}

impl CredsMarketplace {
    fn is_empty(&self) -> bool {
        self.githubAccessToken.is_none()
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(transparent)]
struct MarketplaceGithubToken(String);

impl std::fmt::Debug for MarketplaceGithubToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// 渠道卡片的公共元信息 —— ASR / LLM 两侧共用同一套语义：
///   - `providerType` 是**协议路由 key**（deepseek / volcengine / bailian ...），
///     必须独立于 map key：一个供应商可以有多张卡片（多把 key），此时 map key 是
///     uuid，而 providerType 仍指向同一个厂商实现。
///     `None` = v1 老数据，此时 map key 本身就是 providerType（见 `channel_provider_type`）。
///   - `order` 越小越优先，启用列表的第一个即"当前使用"。
///   - 关闭的渠道会被自动排到末尾（见 `commands::channels::toggle`）。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[allow(non_snake_case)]
struct ChannelMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    providerType: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<u32>,
    /// 缺省 `true`：v1 老数据迁移后一律视为启用。
    #[serde(default = "channel_default_enabled", skip_serializing_if = "is_true")]
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lastTest: Option<ChannelTest>,
}

/// 手写 `Default` 而不是 derive：`bool::default()` 是 `false`，而 `write_account`
/// 用 `map.entry(id).or_default()` 创建 entry —— derive 会让新写入的渠道一出生就是
/// 禁用状态，Core directory 会忽略它，表现为"填了 key 却不生效"。
impl Default for ChannelMeta {
    fn default() -> Self {
        Self {
            providerType: None,
            order: None,
            enabled: channel_default_enabled(),
            lastTest: None,
        }
    }
}

fn channel_default_enabled() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// 「测试连通」的结果，持久化以便重启后仍能看到上次测试的延迟。
/// `error` 同时承担 P0 的失败标红（测试失败）与 P2 的运行时失败标红。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[allow(non_snake_case)]
struct ChannelTest {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latencyMs: Option<u32>,
    /// Unix 秒。
    at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsAsrEntry {
    #[serde(flatten)]
    channel: ChannelMeta,
    /// 用户给这张卡片取的名字；空则前端回落到 preset 显示名。
    #[serde(skip_serializing_if = "Option::is_none")]
    displayName: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    appKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accessKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resourceId: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authMode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    volcengineService: Option<String>,
    /// ASR API Key —— 普通服务 API Key 鉴权或 Agent Plan 使用，与旧版 Access Token 槽位
    /// (`accessKey`) 隔离，避免不同鉴权方式的凭据互相污染。
    #[serde(skip_serializing_if = "Option::is_none")]
    volcengineApiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vocabularyId: Option<String>,
    /// 通用 OpenAI 兼容 ASR(openai-compatible)的高级配置 JSON:
    /// `{"verboseJson": bool, "chunkDurationMs": number|null}`。
    /// 仅该预设读取;命名厂商的怪癖开关保持硬编码,不受此字段影响。
    #[serde(skip_serializing_if = "Option::is_none")]
    advancedConfig: Option<String>,
    /// 讯飞开放平台应用 ID（RTASR/IFASR 鉴权用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    xfyunAppId: Option<String>,
    /// 讯飞实时语音转写 APIKey（接口密钥）。
    #[serde(skip_serializing_if = "Option::is_none")]
    xfyunApiKey: Option<String>,
    /// 腾讯云账号 AppID（实时语音识别 WebSocket 路径参数）。
    #[serde(skip_serializing_if = "Option::is_none")]
    tencentCloudAppId: Option<String>,
    /// 腾讯云 API 密钥 SecretID。
    #[serde(skip_serializing_if = "Option::is_none")]
    tencentCloudSecretId: Option<String>,
    /// 腾讯云 API 密钥 SecretKey。
    #[serde(skip_serializing_if = "Option::is_none")]
    tencentCloudSecretKey: Option<String>,
}

impl CredsAsrEntry {
    fn is_empty(&self) -> bool {
        // 渠道卡片（providerType 已写入）永远不算空：用户可能刚点「添加渠道」、
        // 名字都取好了还没填 key，此时被 clean_credentials 的 retain 静默删掉
        // 就是"卡片自己消失了"。渠道只能由用户显式删除（或由
        // `delete_channel_if_blank` 回收一张什么都没填的草稿）。
        if self.channel.providerType.is_some() {
            return false;
        }
        self.has_no_content()
    }

    /// 除渠道元信息外，用户是否一个字都没填。草稿回收用。
    fn has_no_content(&self) -> bool {
        self.displayName.as_deref().unwrap_or("").is_empty()
            && self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.appKey.as_deref().unwrap_or("").is_empty()
            && self.accessKey.as_deref().unwrap_or("").is_empty()
            && self.resourceId.as_deref().unwrap_or("").is_empty()
            && self.volcengineService.as_deref().unwrap_or("").is_empty()
            && self.authMode.as_deref().unwrap_or("").is_empty()
            && self.volcengineApiKey.as_deref().unwrap_or("").is_empty()
            && self.vocabularyId.as_deref().unwrap_or("").is_empty()
            && self.advancedConfig.as_deref().unwrap_or("").is_empty()
            && self.xfyunAppId.as_deref().unwrap_or("").is_empty()
            && self.xfyunApiKey.as_deref().unwrap_or("").is_empty()
            && self.tencentCloudAppId.as_deref().unwrap_or("").is_empty()
            && self
                .tencentCloudSecretId
                .as_deref()
                .unwrap_or("")
                .is_empty()
            && self
                .tencentCloudSecretKey
                .as_deref()
                .unwrap_or("")
                .is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsLlmEntry {
    #[serde(flatten)]
    channel: ChannelMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    displayName: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraHeaders: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requestFormat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messagesThinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maxTokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinkingBudget: Option<String>,
}

impl CredsLlmEntry {
    fn protocol_option(&mut self, account: &str) -> Result<&mut Option<String>> {
        use openless_core::llm_protocol::*;
        match account {
            REQUEST_FORMAT_ACCOUNT => Ok(&mut self.requestFormat),
            MESSAGES_THINKING_ACCOUNT => Ok(&mut self.messagesThinking),
            MAX_TOKENS_ACCOUNT => Ok(&mut self.maxTokens),
            THINKING_BUDGET_ACCOUNT => Ok(&mut self.thinkingBudget),
            _ => anyhow::bail!("unsupported LLM protocol option"),
        }
    }

    fn is_empty(&self) -> bool {
        // 同 CredsAsrEntry::is_empty —— 渠道卡片只能由用户显式删除。
        if self.channel.providerType.is_some() {
            return false;
        }
        self.has_no_content()
    }

    /// 除渠道元信息外，用户是否一个字都没填。草稿回收用。
    fn has_no_content(&self) -> bool {
        self.displayName.as_deref().unwrap_or("").is_empty()
            && self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.temperature.is_none()
            && self.requestFormat.is_none()
            && self.messagesThinking.is_none()
            && self.maxTokens.is_none()
            && self.thinkingBudget.is_none()
            && self
                .extraHeaders
                .as_ref()
                .map(|h| h.is_empty())
                .unwrap_or(true)
    }
}

/// ASR / LLM 两种 entry 共享渠道元信息的读写口子，让迁移与排序逻辑只写一遍。
trait HasChannelMeta {
    fn meta(&self) -> &ChannelMeta;
    fn meta_mut(&mut self) -> &mut ChannelMeta;
    /// 用户是否往这张卡里填过东西 —— 迁移排序时用来避免把空卡片排到第一。
    fn is_blank(&self) -> bool;
}

impl HasChannelMeta for CredsAsrEntry {
    fn meta(&self) -> &ChannelMeta {
        &self.channel
    }
    fn meta_mut(&mut self) -> &mut ChannelMeta {
        &mut self.channel
    }
    fn is_blank(&self) -> bool {
        self.has_no_content()
    }
}

impl HasChannelMeta for CredsLlmEntry {
    fn meta(&self) -> &ChannelMeta {
        &self.channel
    }
    fn meta_mut(&mut self) -> &mut ChannelMeta {
        &mut self.channel
    }
    fn is_blank(&self) -> bool {
        self.has_no_content()
    }
}

/// 渠道的协议路由 key。v1 老数据没有 `providerType`，此时 map key 本身就是厂商 id。
///
/// **这是渠道化最容易漏的一处**：`coordinator::resolve_effective_asr_provider` 和
/// `commands/providers.rs` 里几十处 `== PROVIDER_ID` 的比较全都依赖它，
/// 拿成 channel id（uuid）会让整个 ASR 路由失效。
fn channel_provider_type<'a, V: HasChannelMeta>(key: &'a str, entry: &'a V) -> &'a str {
    entry.meta().providerType.as_deref().unwrap_or(key)
}

/// 仅供 v1 -> v2 migration 修复遗失的 active；运行期 active policy 在 Core。
fn current_channel_id<V: HasChannelMeta>(map: &HashMap<String, V>) -> Option<String> {
    map.iter()
        .filter(|(_, entry)| entry.meta().enabled)
        .min_by(|(left_key, left), (right_key, right)| {
            let left_order = left.meta().order.unwrap_or(u32::MAX);
            let right_order = right.meta().order.unwrap_or(u32::MAX);
            left_order
                .cmp(&right_order)
                .then_with(|| left_key.as_str().cmp(right_key.as_str()))
        })
        .map(|(key, _)| key.clone())
}

/// v1（一个 preset 一个槽）→ v2（渠道卡片）。
///
/// 幂等的两个支点：
///   1. 迁移出来的渠道 **id 直接沿用原 preset id**，不生成 uuid —— 老用户的 map key
///      一个字节都不变，重复执行结果完全一致；新 id 由 Core directory 统一分配。
///   2. 已带 `providerType` 的 entry 一律跳过。
///
/// order 按「原 active 排第一，其余按 id 字母序」分配。迁移 reader 不读取当前 Core
/// descriptor 的展示顺序；字母序对同一份 v1 数据始终确定，能保证重复迁移幂等。
fn migrate_channel_map<V: HasChannelMeta>(map: &mut HashMap<String, V>, active: &str) -> bool {
    if map.is_empty()
        || map
            .values()
            .all(|entry| entry.meta().providerType.is_some())
    {
        return false;
    }

    let mut keys: Vec<String> = map.keys().cloned().collect();
    // 排序优先级（false < true，所以"是"排前面）：
    //   1. 原来的 active —— 升级前用哪个，升级后还用哪个；
    //   2. **填过凭据的** —— `active` 指向一个已不存在的 entry 是真实会发生的
    //      （前端 prefs 与凭据库里的 active 是两份数据，历史上可能不同步）。这时若纯按
    //      字母序挑，很容易把一张空卡排到第一，用户升级后就看到"未配置"，而他配好的
    //      那张其实还在列表下面躺着；
    //   3. 字母序 —— 兜底，保证结果确定、迁移幂等。
    let is_blank: std::collections::HashMap<&String, bool> = map
        .iter()
        .map(|(key, entry)| (key, entry.is_blank()))
        .collect();
    keys.sort_by(|left, right| {
        let key_of = |key: &String| {
            (
                key != active,
                is_blank.get(key).copied().unwrap_or(true),
                key.clone(),
            )
        };
        key_of(left).cmp(&key_of(right))
    });

    let mut changed = false;
    for (index, key) in keys.iter().enumerate() {
        let provider_type = key.clone();
        let Some(entry) = map.get_mut(key) else {
            continue;
        };
        let meta = entry.meta_mut();
        if meta.providerType.is_none() {
            meta.providerType = Some(provider_type);
            changed = true;
        }
        if meta.order.is_none() {
            meta.order = Some(index as u32);
            changed = true;
        }
    }
    changed
}

/// 渠道 schema 版本：1 = 一个 preset 一个槽；2 = 渠道卡片。
const CHANNELS_SCHEMA_VERSION: u32 = 2;

/// Early Omni vaults used derive(Default), bypassing the serde "custom" default.
/// Legacy account imports could consequently write a complete provider under "".
/// Normalize the loaded copy only; a later gated save owns persistence. Never
/// combine two different endpoint/key/model configurations or discard either one.
fn migrate_legacy_omni_slot(omni: &mut CredsOmni) -> bool {
    let Some(legacy) = omni.providers.get("") else {
        return false;
    };
    if !matches!(
        legacy.channel.providerType.as_deref(),
        None | Some("" | "custom")
    ) {
        return false;
    }
    let mut recovered = legacy.clone();
    recovered.channel.providerType = Some(creds_default_omni());
    if let Some(current) = omni.providers.get("custom") {
        if !matches!(
            current.channel.providerType.as_deref(),
            None | Some("" | "custom")
        ) {
            return false;
        }
        let mut normalized = current.clone();
        normalized.channel.providerType = Some(creds_default_omni());
        if normalized != recovered {
            return false;
        }
    }
    omni.providers.remove("");
    omni.providers.insert(creds_default_omni(), recovered);
    if omni.active.is_empty() {
        omni.active = creds_default_omni();
    }
    // Readers normalize a copy of the same cache; emit this value-free evidence
    // once per process, without logging the source configuration or identifiers.
    static RECOVERY_LOGGED: AtomicBool = AtomicBool::new(false);
    if !RECOVERY_LOGGED.swap(true, Ordering::Relaxed) {
        log::info!("[e2ee-capture] stage=legacy_omni_identity code=recovered");
    }
    true
}

/// 就地把 v1 数据补成渠道卡片。返回是否有实际改动（调用方据此决定要不要落盘）。
fn migrate_channels(root: &mut CredsRoot) -> bool {
    let active_asr = root.active.asr.clone();
    let active_llm = root.active.llm.clone();
    let asr_changed = migrate_channel_map(&mut root.providers.asr, &active_asr);
    let llm_changed = migrate_channel_map(&mut root.providers.llm, &active_llm);

    let seeded = if root.version < CHANNELS_SCHEMA_VERSION {
        let seeded = seed_default_channels(root);
        root.version = CHANNELS_SCHEMA_VERSION;
        seeded
    } else {
        false
    };

    let changed = asr_changed || llm_changed || seeded;
    if changed {
        if !root
            .providers
            .asr
            .get(&root.active.asr)
            .is_some_and(|entry| entry.meta().enabled)
        {
            root.active.asr = current_channel_id(&root.providers.asr).unwrap_or_default();
        }
        if !root
            .providers
            .llm
            .get(&root.active.llm)
            .is_some_and(|entry| entry.meta().enabled)
        {
            root.active.llm = current_channel_id(&root.providers.llm).unwrap_or_default();
        }
    }
    // Omni normalization must not trigger the separate ASR/LLM active fallback.
    migrate_legacy_omni_slot(&mut root.omni) || changed
}

/// 全新安装的平台预置。
///
/// 只有 Windows 需要：那里的默认 ASR 是本地 Foundry，无需任何 key、装上就能用
/// （见 `creds_default_asr`）。渠道化后列表完全由用户添加，不预置的话 Windows 新用户
/// 开箱会一个 ASR 都没有。mac / Linux 的默认是要填 key 的云端厂商，预置一张空卡片
/// 没有意义，交给新手引导。
///
/// 靠 `version < 2` 把"全新安装"和"用户把渠道全删了"区分开：后者 version 已经是 2，
/// 不会被重新种回来。version 的落盘发生在下一次真实写入时（见 `load_credentials`
/// 关于不主动落盘的说明），在此之前每次冷启动都会在内存里重新预置，正是期望行为。
fn seed_default_channels(root: &mut CredsRoot) -> bool {
    #[cfg(target_os = "windows")]
    {
        if root.providers.asr.is_empty() {
            let id = crate::asr::local::foundry::PROVIDER_ID.to_string();
            root.providers.asr.insert(
                id.clone(),
                CredsAsrEntry {
                    channel: ChannelMeta {
                        providerType: Some(id.clone()),
                        order: Some(0),
                        enabled: true,
                        lastTest: None,
                    },
                    ..Default::default()
                },
            );
            root.active.asr = id;
            return true;
        }
    }
    let _ = root;
    false
}

fn active_llm_extra_headers(root: &CredsRoot) -> HashMap<String, String> {
    root.providers
        .llm
        .get(&root.active.llm)
        .and_then(|entry| entry.extraHeaders.clone())
        .unwrap_or_default()
}

fn omni_extra_headers(root: &CredsRoot, provider_id: &str) -> HashMap<String, String> {
    root.omni
        .providers
        .get(provider_id)
        .and_then(|entry| entry.extraHeaders.clone())
        .unwrap_or_default()
}

fn active_omni_extra_headers(root: &CredsRoot) -> HashMap<String, String> {
    omni_extra_headers(root, &root.omni.active)
}

fn is_valid_llm_temperature(temperature: f64) -> bool {
    temperature.is_finite() && (0.0..=2.0).contains(&temperature)
}

fn active_llm_temperature_value(root: &CredsRoot) -> Option<f64> {
    root.providers
        .llm
        .get(&root.active.llm)
        .and_then(|entry| entry.temperature)
        .filter(|temperature| is_valid_llm_temperature(*temperature))
}

fn active_llm_temperature(root: &CredsRoot) -> Option<f32> {
    active_llm_temperature_value(root).map(|temperature| temperature as f32)
}

fn active_llm_temperature_string(root: &CredsRoot) -> Option<String> {
    active_llm_temperature_value(root).map(|temperature| temperature.to_string())
}

fn set_llm_temperature_for_provider_in_root(
    root: &mut CredsRoot,
    provider_id: &str,
    temperature: Option<f64>,
) {
    root.providers
        .llm
        .entry(provider_id.to_string())
        .or_default()
        .temperature = temperature;
}

fn set_llm_extra_headers_for_provider_in_root(
    root: &mut CredsRoot,
    provider_id: &str,
    headers: HashMap<String, String>,
) {
    root.providers
        .llm
        .entry(provider_id.to_string())
        .or_default()
        .extraHeaders = (!headers.is_empty()).then_some(headers);
}

fn active_llm_extra_headers_json(root: &CredsRoot) -> Result<Option<String>> {
    let headers = active_llm_extra_headers(root);
    if headers.is_empty() {
        return Ok(None);
    }
    let ordered = headers.into_iter().collect::<BTreeMap<_, _>>();
    serde_json::to_string(&ordered)
        .map(Some)
        .context("encode LLM extra headers")
}

fn omni_extra_headers_json(root: &CredsRoot, provider_id: &str) -> Result<Option<String>> {
    let headers = omni_extra_headers(root, provider_id);
    if headers.is_empty() {
        return Ok(None);
    }
    let ordered = headers.into_iter().collect::<BTreeMap<_, _>>();
    serde_json::to_string(&ordered)
        .map(Some)
        .context("encode omni extra headers")
}

fn active_omni_extra_headers_json(root: &CredsRoot) -> Result<Option<String>> {
    omni_extra_headers_json(root, &root.omni.active)
}

fn omni_temperature_value(root: &CredsRoot, provider_id: &str) -> Option<f64> {
    root.omni
        .providers
        .get(provider_id)
        .and_then(|entry| entry.temperature)
        .filter(|temperature| is_valid_llm_temperature(*temperature))
}

fn active_omni_temperature_value(root: &CredsRoot) -> Option<f64> {
    omni_temperature_value(root, &root.omni.active)
}

fn active_omni_temperature(root: &CredsRoot) -> Option<f32> {
    active_omni_temperature_value(root).map(|temperature| temperature as f32)
}

fn active_omni_temperature_string(root: &CredsRoot) -> Option<String> {
    active_omni_temperature_value(root).map(|temperature| temperature.to_string())
}

fn omni_temperature_string(root: &CredsRoot, provider_id: &str) -> Option<String> {
    omni_temperature_value(root, provider_id).map(|temperature| temperature.to_string())
}

fn parse_extra_headers_json(value: &str) -> Result<HashMap<String, String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(HashMap::new());
    }

    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(trimmed).context("extra headers must be a JSON object")?;
    let mut headers = HashMap::new();
    for (key, value) in raw {
        let key = key.trim();
        if key.is_empty() {
            anyhow::bail!("extra header name cannot be empty");
        }
        if !is_valid_header_name(key) {
            anyhow::bail!("invalid extra header name: {key}");
        }
        if is_reserved_extra_header_name(key) {
            anyhow::bail!("reserved extra header name cannot be overridden: {key}");
        }
        let Some(value) = value.as_str() else {
            anyhow::bail!("extra header value for {key} must be a string");
        };
        if value.contains('\r') || value.contains('\n') {
            anyhow::bail!("extra header value for {key} cannot contain line breaks");
        }
        headers.insert(key.to_string(), value.to_string());
    }
    Ok(headers)
}

fn parse_llm_temperature(value: &str) -> Result<Option<f64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let temperature: f64 = trimmed.parse().context("temperature must be a number")?;
    if !is_valid_llm_temperature(temperature) {
        if !temperature.is_finite() {
            anyhow::bail!("temperature must be finite");
        }
        anyhow::bail!("temperature must be between 0 and 2");
    }
    Ok(Some(temperature))
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
                    | b'0'..=b'9'
                    | b'a'..=b'z'
                    | b'A'..=b'Z'
            )
        })
}

fn is_reserved_extra_header_name(name: &str) -> bool {
    RESERVED_EXTRA_HEADER_NAMES
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

fn credentials_path() -> Result<PathBuf> {
    // macOS / Linux: ~/.openless/credentials.json (与 Swift 同源)
    // Windows: %APPDATA%\OpenLess\credentials.json (Windows 没有标准 HOME 环境变量)
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        return Ok(PathBuf::from(appdata)
            .join("OpenLess")
            .join(LEGACY_CREDS_FILE));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(LEGACY_CREDS_DIR)
            .join(LEGACY_CREDS_FILE))
    }
}

#[cfg(not(any(target_os = "android", target_os = "macos")))]
fn keyring_entry() -> Result<keyring::Entry> {
    keyring_entry_for(KEYRING_CREDENTIALS_ACCOUNT)
}

#[cfg(not(target_os = "android"))]
fn keyring_entry_for(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(CredentialsVault::SERVICE_NAME, account)
        .context("open system credential vault")
}

#[cfg(target_os = "android")]
fn android_credentials_path() -> Result<PathBuf> {
    let files_dir = crate::android::jni::android::app_files_dir()
        .map_err(|error| anyhow::anyhow!("resolve Android credential directory: {error}"))?;
    Ok(PathBuf::from(files_dir)
        .join("OpenLess")
        .join(ANDROID_CREDENTIALS_FILE))
}

#[cfg(target_os = "android")]
fn android_legacy_credentials_paths(current_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut add_path = |path: PathBuf| {
        if path != current_path && !paths.contains(&path) {
            paths.push(path);
        }
    };
    if let Ok(dir) = std::env::var("TAURI_ANDROID_APP_DATA_DIR") {
        add_path(
            PathBuf::from(dir)
                .join("OpenLess")
                .join(ANDROID_CREDENTIALS_FILE),
        );
    }
    add_path(
        std::env::temp_dir()
            .join("OpenLess")
            .join(ANDROID_CREDENTIALS_FILE),
    );
    paths
}

#[cfg(target_os = "android")]
fn remove_migrated_android_legacy_credentials(current_path: &Path) -> Result<()> {
    for legacy_path in android_legacy_credentials_paths(current_path) {
        super::android_credentials::secure_remove(&legacy_path)
            .map_err(anyhow::Error::new)
            .with_context(|| {
                format!(
                    "remove migrated Android legacy envelope {}",
                    legacy_path.display()
                )
            })?;
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn load_android_credentials() -> Result<Option<CredsRoot>> {
    let path = android_credentials_path()?;
    let mut crypto = super::android_credentials::AndroidKeystoreCrypto;
    let loaded = match load_android_credentials_from_path_with_crypto(&path, &mut crypto)? {
        Some(root) => Some(root),
        None => {
            let mut migrated = None;
            for legacy_path in android_legacy_credentials_paths(&path) {
                if let Some(root) = load_android_credentials_from_source_with_crypto(
                    &legacy_path,
                    &path,
                    &mut crypto,
                )? {
                    migrated = Some(root);
                    break;
                }
            }
            migrated
        }
    };
    if loaded.is_some() {
        remove_migrated_android_legacy_credentials(&path)?;
    }
    *android_marketplace_legacy_scrubbed().lock() = true;
    Ok(loaded)
}

#[cfg(target_os = "android")]
fn load_android_credentials_from_path(path: &Path) -> Result<Option<CredsRoot>> {
    let mut crypto = super::android_credentials::AndroidKeystoreCrypto;
    load_android_credentials_from_path_with_crypto(path, &mut crypto)
}

#[cfg(all(test, not(target_os = "android")))]
fn load_android_credentials_from_path(path: &Path) -> Result<Option<CredsRoot>> {
    let mut crypto = super::android_credentials::TestCrypto::default();
    load_android_credentials_from_path_with_crypto(path, &mut crypto)
}

#[cfg(any(target_os = "android", test))]
fn load_android_credentials_from_path_with_crypto(
    path: &Path,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<Option<CredsRoot>> {
    load_android_credentials_from_source_with_crypto(path, path, crypto)
}

#[cfg(any(target_os = "android", test))]
fn load_android_credentials_from_source_with_crypto(
    source_path: &Path,
    destination_path: &Path,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<Option<CredsRoot>> {
    use super::android_credentials::ReadOutcome;

    let loaded = super::android_credentials::read(source_path, crypto)
        .map_err(anyhow::Error::new)
        .context("read Android credential envelope")?;
    let (bytes, needs_rewrite) = match loaded {
        ReadOutcome::Missing => return Ok(None),
        ReadOutcome::Legacy(bytes) => (bytes, true),
        ReadOutcome::Plaintext(bytes) => (bytes, false),
    };
    let root =
        serde_json::from_slice::<CredsRoot>(&bytes).context("parse Android credential payload")?;
    let cleaned = android_persistable_credentials(&root);
    let contained_marketplace_token = lookup_marketplace_github_token(&root).is_some();
    if needs_rewrite && contained_marketplace_token {
        let sanitized =
            serde_json::to_vec(&cleaned).context("encode bearer-free Android legacy payload")?;
        super::android_credentials::rewrite_legacy_without_bearer(source_path, &sanitized)
            .map_err(anyhow::Error::new)
            .context("scrub Marketplace bearer before Android Keystore migration")?;
    }
    if needs_rewrite || contained_marketplace_token || source_path != destination_path {
        write_android_credentials_envelope_with_crypto(destination_path, &cleaned, crypto)
            .context("migrate Android credential envelope")?;
    }
    if source_path != destination_path {
        super::android_credentials::secure_remove(source_path)
            .map_err(anyhow::Error::new)
            .with_context(|| {
                format!(
                    "remove migrated Android legacy envelope {}",
                    source_path.display()
                )
            })?;
    }
    Ok(Some(cleaned))
}

#[cfg(any(target_os = "android", test))]
fn ensure_android_marketplace_legacy_scrubbed_at(
    path: &Path,
    completed: &Mutex<bool>,
) -> Result<()> {
    let mut completed = completed.lock();
    if *completed {
        return Ok(());
    }
    // Mark completion only after the durable sanitized rewrite (or confirmed
    // absence of a legacy file) succeeds. Any error remains retryable.
    let _ = load_android_credentials_from_path(path)?;
    *completed = true;
    Ok(())
}

#[cfg(any(target_os = "android", test))]
fn get_android_marketplace_token_at(
    path: &Path,
    completed: &Mutex<bool>,
    memory_token: &Mutex<Option<MarketplaceGithubToken>>,
) -> Result<Option<String>> {
    ensure_android_marketplace_legacy_scrubbed_at(path, completed)?;
    Ok(memory_token.lock().as_ref().map(|token| token.0.clone()))
}

#[cfg(target_os = "android")]
fn ensure_android_marketplace_legacy_scrubbed() -> Result<()> {
    let _ = load_android_credentials()?;
    Ok(())
}

#[cfg(target_os = "android")]
fn save_android_credentials(root: &CredsRoot) -> Result<()> {
    let path = android_credentials_path()?;
    write_android_credentials_envelope(&path, root)
}

#[cfg(target_os = "android")]
fn write_android_credentials_envelope(path: &Path, root: &CredsRoot) -> Result<()> {
    let mut crypto = super::android_credentials::AndroidKeystoreCrypto;
    write_android_credentials_envelope_with_crypto(path, root, &mut crypto)
}

#[cfg(any(target_os = "android", test))]
fn write_android_credentials_envelope_with_crypto(
    path: &Path,
    root: &CredsRoot,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<()> {
    let cleaned = android_persistable_credentials(root);
    let json = serde_json::to_vec(&cleaned).context("encode Android credential payload")?;
    super::android_credentials::write_verified(path, &json, crypto)
        .map_err(anyhow::Error::new)
        .context("write Android credential envelope")
}

#[cfg(any(target_os = "android", test))]
fn android_persistable_credentials(root: &CredsRoot) -> CredsRoot {
    let mut cleaned = clean_credentials(root);
    write_marketplace_github_token(&mut cleaned, None);
    cleaned
}

fn clean_credentials(root: &CredsRoot) -> CredsRoot {
    let mut cleaned = root.clone();
    cleaned.providers.asr.retain(|_, v| !v.is_empty());
    cleaned.providers.llm.retain(|_, v| !v.is_empty());
    cleaned.omni.providers.retain(|_, v| !v.is_empty());
    cleaned
}

fn lookup_marketplace_github_token(root: &CredsRoot) -> Option<String> {
    root.marketplace
        .githubAccessToken
        .as_ref()
        .map(|token| token.0.as_str())
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
}

fn write_marketplace_github_token(root: &mut CredsRoot, value: Option<String>) {
    root.marketplace.githubAccessToken = value.and_then(|token| {
        if token.trim().is_empty() {
            None
        } else {
            Some(MarketplaceGithubToken(token))
        }
    });
}

fn marketplace_token_is_rejected() -> bool {
    MARKETPLACE_TOKEN_REJECTED.load(Ordering::SeqCst)
}

fn invalidate_marketplace_token_process_local() {
    // Publish the tombstone first. All token reads happen under
    // `credentials_lock`, so the subsequent cache/memory clear is atomic from
    // the command layer's point of view; the atomic also prevents accidental
    // direct readers from observing the rejected token.
    MARKETPLACE_TOKEN_REJECTED.store(true, Ordering::SeqCst);
    if let Some(root) = credentials_cache().lock().as_mut() {
        write_marketplace_github_token(root, None);
    }
    #[cfg(target_os = "android")]
    {
        *android_marketplace_token().lock() = None;
    }
}

fn invalidate_marketplace_token_with(durable_delete: impl FnOnce() -> Result<()>) -> Result<()> {
    invalidate_marketplace_token_process_local();
    durable_delete()
}

fn mark_marketplace_token_verified() {
    MARKETPLACE_TOKEN_REJECTED.store(false, Ordering::SeqCst);
}

#[cfg(any(not(target_os = "android"), test))]
fn set_marketplace_token_with(
    gate: Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>>,
    value: &str,
    load: impl FnOnce() -> Result<CredsRoot>,
    persist: impl FnOnce(&CredsRoot) -> Result<()>,
) -> Result<()> {
    mutate_credentials_with(
        gate,
        openless_core::credentials::ChangeOrigin::LocalOnly,
        load,
        |root| {
            write_marketplace_github_token(root, Some(value.to_string()));
            Ok(true)
        },
        |root| {
            persist(root)?;
            // Keep verification in the same vault critical section as publication.
            if value.trim().is_empty() {
                invalidate_marketplace_token_process_local();
            } else {
                mark_marketplace_token_verified();
            }
            Ok(())
        },
    )
}

#[cfg(any(not(target_os = "android"), test))]
fn remove_marketplace_token_with(
    gate: Option<std::sync::Arc<openless_core::credentials::SyncWriteGate>>,
    load: impl FnOnce() -> Result<CredsRoot>,
    persist: impl FnOnce(&CredsRoot) -> Result<()>,
) -> Result<()> {
    // Block new reads immediately, including when the gate is busy with restore.
    invalidate_marketplace_token_process_local();
    let result = mutate_credentials_with(
        gate,
        openless_core::credentials::ChangeOrigin::LocalOnly,
        load,
        |root| {
            // A concurrent login could have finished while this worker waited for the lock.
            invalidate_marketplace_token_process_local();
            write_marketplace_github_token(root, None);
            Ok(true)
        },
        persist,
    );
    if result.is_err() {
        // Read/gate failures happen before the update closure. Reassert the local
        // revocation after any earlier in-flight login has left its vault section.
        let _guard = credentials_lock().lock();
        invalidate_marketplace_token_process_local();
    }
    result
}

fn read_legacy_credentials_file(path: &Path) -> Option<CredsRoot> {
    if !path.exists() {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[vault] read legacy {} failed: {}", path.display(), e);
            return None;
        }
    };
    match serde_json::from_slice::<CredsRoot>(&bytes) {
        Ok(root) => Some(root),
        Err(e) => {
            log::warn!("[vault] parse legacy {} failed: {}", path.display(), e);
            None
        }
    }
}

fn remove_legacy_credentials_file() -> Result<()> {
    let Ok(path) = credentials_path() else {
        return Ok(());
    };
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove legacy credentials file {}", path.display()))?;
    }
    Ok(())
}

fn remove_legacy_credentials_file_best_effort() {
    if let Err(e) = remove_legacy_credentials_file() {
        log::warn!("[vault] remove legacy credentials file failed: {e}");
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CredsChunkManifest {
    openless_credentials_storage: String,
    version: u32,
    /// Every new Windows write uses a private generation; old stable chunks remain
    /// readable until one atomic manifest update commits the new complete payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
    chunks: usize,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultCommitFailure {
    #[error("credential vault write failed before commit")]
    Unchanged,
    #[error("credential vault commit result requires reconciliation")]
    Unknown,
}

/// Each candidate lives under a new UUID namespace. The single manifest is the
/// only publication point: readers never guess a generation from orphan chunks.
#[cfg(any(not(any(target_os = "macos", target_os = "android")), test))]
fn save_chunked_credentials_with(
    json: &str,
    mut read: impl FnMut(&str) -> Result<Option<String>>,
    mut write: impl FnMut(&str, &str) -> Result<()>,
    mut delete: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let previous_head =
        read(KEYRING_CREDENTIALS_ACCOUNT).map_err(|_| VaultCommitFailure::Unchanged)?;
    let previous = match previous_head.as_deref() {
        Some(head) => match read_chunk_manifest(head) {
            Some(manifest) if manifest.chunks > 0 => Some(manifest),
            Some(_) => return Err(VaultCommitFailure::Unchanged.into()),
            None => {
                decode_single_credentials(head).map_err(|_| VaultCommitFailure::Unchanged)?;
                None
            }
        },
        None => None,
    };
    decode_single_credentials(json).map_err(|_| VaultCommitFailure::Unchanged)?;
    let generation = uuid::Uuid::new_v4().to_string();
    let chunks = chunk_json_payload(json);
    let accounts = (0..chunks.len())
        .map(|index| chunk_account(Some(&generation), index))
        .collect::<Vec<_>>();
    let candidate = CredsChunkManifest {
        openless_credentials_storage: "chunked".into(),
        version: 1,
        generation: Some(generation),
        chunks: chunks.len(),
    };
    let candidate_head =
        serde_json::to_string(&candidate).map_err(|_| VaultCommitFailure::Unchanged)?;
    let prepared = (|| -> Result<()> {
        for (account, chunk) in accounts.iter().zip(&chunks) {
            write(account, chunk)?;
        }
        let mut recovered = zeroize::Zeroizing::new(String::with_capacity(json.len()));
        for account in &accounts {
            let chunk =
                zeroize::Zeroizing::new(read(account)?.ok_or(VaultCommitFailure::Unchanged)?);
            if recovered.len().saturating_add(chunk.len()) > json.len() {
                return Err(VaultCommitFailure::Unchanged.into());
            }
            recovered.push_str(&chunk);
        }
        if recovered.as_str() != json {
            return Err(VaultCommitFailure::Unchanged.into());
        }
        Ok(())
    })();
    if prepared.is_err() {
        for account in &accounts {
            let _ = delete(account);
        }
        return Err(VaultCommitFailure::Unchanged.into());
    }

    // Even a failed native call may have committed. Confirm the exact head before
    // deciding whether a candidate may be discarded, or old chunks may be pruned.
    let _publication = write(KEYRING_CREDENTIALS_ACCOUNT, &candidate_head);
    match read(KEYRING_CREDENTIALS_ACCOUNT) {
        Ok(Some(head)) if head == candidate_head => {}
        Ok(head) if head == previous_head => {
            for account in &accounts {
                let _ = delete(account);
            }
            return Err(VaultCommitFailure::Unchanged.into());
        }
        _ => return Err(VaultCommitFailure::Unknown.into()),
    }
    if let Some(previous) = previous {
        for index in 0..previous.chunks {
            let _ = delete(&chunk_account(previous.generation.as_deref(), index));
        }
    }
    Ok(())
}

/// Legacy UUID-prefixed and current stable chunk names share one reader.
fn chunk_account(generation: Option<&str>, index: usize) -> String {
    match generation {
        Some(gen) => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{gen}.{index}"),
        None => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{index}"),
    }
}

#[cfg(any(not(any(target_os = "macos", target_os = "android")), test))]
fn chunk_json_payload(json: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0usize;
    for ch in json.chars() {
        let units = ch.len_utf16();
        if !current.is_empty() && current_units + units > KEYRING_CHUNK_MAX_UTF16_UNITS {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        current.push(ch);
        current_units += units;
    }
    if !current.is_empty() || json.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn read_chunk_manifest(json: &str) -> Option<CredsChunkManifest> {
    let manifest = serde_json::from_str::<CredsChunkManifest>(json).ok()?;
    if manifest.openless_credentials_storage == "chunked" && manifest.version == 1 {
        Some(manifest)
    } else {
        None
    }
}

/// Windows Credential Manager (`CredReadW`) can transiently fail right after
/// login / under contention when we read the manifest entry plus every chunk
/// entry in quick succession. A single failed read makes the whole credential
/// set look empty → `load_keyring_credentials` returns `Err` → `load_credentials`
/// falls back to an empty default → Overview shows「火山引擎未配置」even though the
/// secrets are present (the next dictation re-reads and succeeds, which is why the
/// bug is *probabilistic* and the app "实际可以正常使用"). The more chunks a
/// credential set spans, the more reads per load, the higher the odds at least
/// one trips. Retry transient errors a few times with short backoff.
///
/// macOS / Linux keep the original single-shot behavior on purpose: their read
/// errors are ACL denials that won't heal on retry, and the un-cached error path
/// already retries on the next call — adding sleeps there would only slow the
/// macOS first-launch Keychain authorization flow.
#[cfg(target_os = "windows")]
const KEYRING_READ_RETRY_ATTEMPTS: usize = 4;
#[cfg(target_os = "windows")]
const KEYRING_READ_RETRY_BACKOFF_MS: u64 = 60;

#[cfg(not(target_os = "android"))]
fn get_keyring_password(account: &str) -> Result<Option<String>> {
    #[cfg(target_os = "windows")]
    {
        let mut attempt = 0usize;
        loop {
            match keyring_entry_for(account)?.get_password() {
                Ok(value) => return Ok(Some(value)),
                // NoEntry is a definitive "not stored" answer, never a transient
                // failure — return immediately so genuinely-unconfigured providers
                // don't pay the retry latency.
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(e) => {
                    attempt += 1;
                    if attempt >= KEYRING_READ_RETRY_ATTEMPTS {
                        return Err(anyhow!(e))
                            .with_context(|| format!("read system credential vault {account}"));
                    }
                    log::warn!(
                        "[vault] transient credential read for {account} failed \
                         (attempt {attempt}/{KEYRING_READ_RETRY_ATTEMPTS}): {e}; retrying"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        KEYRING_READ_RETRY_BACKOFF_MS * attempt as u64,
                    ));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        match keyring_entry_for(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => {
                Err(anyhow!(e)).with_context(|| format!("read system credential vault {account}"))
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn delete_keyring_password(account: &str) {
    match keyring_entry_for(account).and_then(|entry| {
        entry
            .delete_credential()
            .with_context(|| format!("delete system credential vault {account}"))
    }) {
        Ok(()) | Err(_) => {}
    }
}

#[cfg(not(target_os = "android"))]
fn load_keyring_credentials() -> Result<Option<CredsRoot>> {
    load_keyring_credentials_with(
        get_keyring_password,
        set_keyring_password_for_migration,
        cfg!(target_os = "macos"),
    )
}

/// credentials.v2 补写每进程只尝试一次。
#[cfg(not(target_os = "android"))]
fn set_keyring_password_for_migration(account: &str, value: &str) -> Result<()> {
    if SINGLE_ITEM_MIGRATION_ATTEMPTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    set_keyring_password(account, value)
}

/// 首次成功定位凭据来源时记一条日志，用于诊断「未配置」误报。
#[cfg(any(not(target_os = "android"), test))]
fn log_vault_source_once(source: &str) {
    if !VAULT_SOURCE_LOGGED.swap(true, Ordering::SeqCst) {
        log::info!("[vault] credential source: {source}");
    }
}

#[cfg(not(target_os = "android"))]
fn set_keyring_password(account: &str, value: &str) -> Result<()> {
    keyring_entry_for(account)?
        .set_password(value)
        .with_context(|| format!("write system credential vault {account}"))
}

#[cfg(any(not(target_os = "android"), test))]
fn load_keyring_credentials_with(
    mut read: impl FnMut(&str) -> Result<Option<String>>,
    mut write: impl FnMut(&str, &str) -> Result<()>,
    consolidate: bool,
) -> Result<Option<CredsRoot>> {
    if consolidate {
        if let Some(json) = read(KEYRING_SINGLE_CREDENTIALS_ACCOUNT)? {
            log_vault_source_once("credentials.v2 (single item)");
            return decode_single_credentials(&json).map(Some);
        }
    }
    let Some(json_or_manifest) = read(KEYRING_CREDENTIALS_ACCOUNT)? else {
        log_vault_source_once("empty (no stored credentials)");
        return Ok(None);
    };

    let json = if let Some(manifest) = read_chunk_manifest(&json_or_manifest) {
        log_vault_source_once("credentials.v1 chunks");
        let mut json = String::new();
        for index in 0..manifest.chunks {
            let account = chunk_account(manifest.generation.as_deref(), index);
            let chunk = read(&account)?
                .ok_or_else(|| anyhow::anyhow!("missing system credential vault chunk {index}"))?;
            json.push_str(&chunk);
        }
        json
    } else {
        json_or_manifest
    };

    let root = decode_single_credentials(&json)?;
    if consolidate {
        // macOS has no Windows blob limit. Create one app-owned item after the
        // complete old payload was read with the user's authorization. Retain the
        // old manifest/chunks without changing their ACLs or asking to delete them.
        // A denied migration does not invalidate the successfully read data.
        if let Err(error) = write(KEYRING_SINGLE_CREDENTIALS_ACCOUNT, &json) {
            log::warn!("[vault] single-item credential migration deferred: {error}");
        }
    }
    Ok(Some(root))
}

fn decode_single_credentials(json: &str) -> Result<CredsRoot> {
    // Serde defaults alone would accept unrelated JSON as an empty configuration.
    let payload: serde_json::Value =
        serde_json::from_str(json).context("decode system credential vault payload")?;
    anyhow::ensure!(
        payload
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .is_some()
            && payload
                .get("active")
                .is_some_and(serde_json::Value::is_object)
            && payload
                .get("providers")
                .is_some_and(serde_json::Value::is_object),
        "invalid system credential vault payload"
    );
    serde_json::from_value(payload).context("decode system credential vault payload")
}

#[cfg(not(target_os = "android"))]
fn load_legacy_keyring_credentials_for_update() -> Result<CredsRoot> {
    let mut root = CredsRoot::default();
    for account in CredentialAccount::all() {
        let legacy_account = account.keyring_account();
        match get_keyring_password(legacy_account) {
            Ok(Some(value)) => write_account(&mut root, *account, Some(value)),
            Ok(None) => {}
            Err(e) => return Err(e.context(format!("read legacy vault {legacy_account}"))),
        }
    }
    Ok(clean_credentials(&root))
}

#[cfg(not(target_os = "android"))]
fn remove_legacy_keyring_credentials() {
    for account in CredentialAccount::all() {
        delete_keyring_password(account.keyring_account());
    }
}

fn legacy_vault_has_credentials(root: &CredsRoot) -> bool {
    !root.providers.asr.is_empty() || !root.providers.llm.is_empty()
}

fn migrate_legacy_sources_for_update() -> Result<CredsRoot> {
    if let Some(legacy) = credentials_path()
        .ok()
        .and_then(|path| read_legacy_credentials_file(&path))
    {
        save_credentials(&legacy)?;
        let persisted = credentials_cache()
            .lock()
            .as_ref()
            .cloned()
            .unwrap_or(legacy);
        #[cfg(not(target_os = "android"))]
        remove_legacy_keyring_credentials();
        return Ok(persisted);
    }

    #[cfg(not(target_os = "android"))]
    {
        // 旧版逐账户条目扫描每进程只跑一次并缓存；失败时重放同一错误，不重扫。
        let probe = LEGACY_KEYRING_PROBE.get_or_init(|| {
            migrate_legacy_keyring_accounts().map_err(|error| format!("{error:#}"))
        });
        return match probe {
            Ok(Some(root)) => Ok(root.clone()),
            Ok(None) => Ok(CredsRoot::default()),
            Err(message) => Err(anyhow!(message.clone())),
        };
    }

    #[cfg(target_os = "android")]
    Ok(CredsRoot::default())
}

/// 扫描旧版逐账户 Keychain 条目并迁移到当前存储。Some = 找到旧凭据。
#[cfg(not(target_os = "android"))]
fn migrate_legacy_keyring_accounts() -> Result<Option<CredsRoot>> {
    let legacy_vault = load_legacy_keyring_credentials_for_update()?;
    if !legacy_vault_has_credentials(&legacy_vault) {
        return Ok(None);
    }
    save_credentials(&legacy_vault)?;
    let persisted = credentials_cache()
        .lock()
        .as_ref()
        .cloned()
        .unwrap_or(legacy_vault);
    remove_legacy_keyring_credentials();
    Ok(Some(persisted))
}

fn load_credentials_into_cache_with(
    loader: impl FnOnce() -> Result<Option<CredsRoot>>,
) -> CredsRoot {
    match loader() {
        Ok(root) => {
            let root = root.unwrap_or_default();
            clear_vault_read_error();
            store_credentials_cache(&root);
            root
        }
        Err(e) => {
            // Do not cache the fallback. In particular, a failed legacy-token
            // scrub must be retried by the next startup/getter call rather than
            // hidden for the rest of the process.
            record_vault_read_failure(&e);
            CredsRoot::default()
        }
    }
}

/// 补齐渠道元数据只改内存；下次保存时一并持久化，不在每次启动重写凭据。
/// macOS 的旧存储分片由底层读取器在成功授权后单独合并一次。
fn load_credentials() -> CredsRoot {
    let mut root = load_credentials_raw();
    migrate_channels(&mut root);
    root
}

fn load_credentials_for_update() -> Result<CredsRoot> {
    let mut root = load_credentials_for_update_raw()?;
    migrate_channels(&mut root);
    Ok(root)
}

/// Pending restore startup may inspect existing sources, but the restore lease
/// exclusively owns their eventual migration/replacement. Never synthesize an
/// empty vault from a failed native read or a corrupt legacy source.
fn load_credentials_for_recovery_readonly() -> Result<CredsRoot> {
    #[cfg(not(target_os = "android"))]
    let mut root = load_desktop_credentials_readonly_with(
        get_keyring_password,
        || {
            let path = credentials_path()?;
            match std::fs::read(path) {
                Ok(bytes) => {
                    let bytes = zeroize::Zeroizing::new(bytes);
                    let json = std::str::from_utf8(&bytes)
                        .context("invalid legacy credential encoding")?;
                    decode_single_credentials(json).map(Some)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => anyhow::bail!("could not read legacy credential source"),
            }
        },
        cfg!(target_os = "macos"),
    )?;
    #[cfg(target_os = "android")]
    let mut root = {
        let path = android_credentials_path()?;
        let mut crypto = super::android_credentials::AndroidKeystoreCrypto;
        let mut loaded = load_android_credentials_readonly_at(&path, &mut crypto)?;
        if loaded.is_none() {
            for source in android_legacy_credentials_paths(&path) {
                loaded = load_android_credentials_readonly_at(&source, &mut crypto)?;
                if loaded.is_some() {
                    break;
                }
            }
        }
        loaded.unwrap_or_default()
    };
    migrate_channels(&mut root);
    Ok(root)
}

#[cfg(any(not(target_os = "android"), test))]
fn load_desktop_credentials_readonly_with(
    mut read: impl FnMut(&str) -> Result<Option<String>>,
    legacy_file: impl FnOnce() -> Result<Option<CredsRoot>>,
    prefer_single: bool,
) -> Result<CredsRoot> {
    if prefer_single {
        if let Some(json) = read(KEYRING_SINGLE_CREDENTIALS_ACCOUNT)? {
            let json = zeroize::Zeroizing::new(json);
            return decode_single_credentials(&json);
        }
    }
    if let Some(root) = load_keyring_credentials_with(
        &mut read,
        |_, _| anyhow::bail!("read-only credential loading cannot consolidate"),
        false,
    )? {
        return Ok(root);
    }
    if let Some(root) = legacy_file()? {
        return Ok(root);
    }
    let mut root = CredsRoot::default();
    for account in CredentialAccount::all() {
        if let Some(value) = read(account.keyring_account())? {
            write_account(&mut root, *account, Some(value));
        }
    }
    Ok(clean_credentials(&root))
}

#[cfg(any(target_os = "android", test))]
fn load_android_credentials_readonly_at(
    path: &Path,
    crypto: &mut impl super::android_credentials::AndroidCredentialsCrypto,
) -> Result<Option<CredsRoot>> {
    use super::android_credentials::ReadOutcome;
    let bytes =
        match super::android_credentials::read_only(path, crypto).map_err(anyhow::Error::new)? {
            ReadOutcome::Missing => return Ok(None),
            ReadOutcome::Plaintext(bytes) | ReadOutcome::Legacy(bytes) => {
                zeroize::Zeroizing::new(bytes)
            }
        };
    let json =
        std::str::from_utf8(&bytes).context("invalid read-only Android credential encoding")?;
    let root = decode_single_credentials(json)?;
    // Legacy OAuth remains excluded even while the encrypted restore barrier
    // temporarily prevents rewriting its old source envelope.
    Ok(Some(android_persistable_credentials(&root)))
}

fn load_credentials_for_sync_binding_with(
    readonly_required: bool,
    readonly: impl FnOnce() -> Result<CredsRoot>,
    ordinary: impl FnOnce() -> Result<CredsRoot>,
) -> Result<CredsRoot> {
    if readonly_required {
        readonly()
    } else {
        ordinary()
    }
}

/// Bound readers may run before recovery or while an exclusive restore is
/// preparing. Only inspect complete sources; the next gated save can migrate
/// legacy storage. A failed read leaves no default cache and remains retryable.
fn load_credentials_readonly_into_cache_with(
    loader: impl FnOnce() -> Result<CredsRoot>,
) -> Result<CredsRoot> {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return Ok(cached);
    }
    match loader() {
        Ok(root) => {
            clear_vault_read_error();
            store_credentials_cache(&root);
            Ok(root)
        }
        Err(error) => {
            record_vault_read_failure(&error);
            Err(error)
        }
    }
}

fn load_credentials_raw() -> CredsRoot {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return cached;
    }

    if sync_write_gate().is_some() {
        return load_credentials_readonly_into_cache_with(load_credentials_for_recovery_readonly)
            .unwrap_or_default();
    }

    #[cfg(target_os = "android")]
    {
        return load_credentials_into_cache_with(load_android_credentials);
    }

    #[cfg(not(target_os = "android"))]
    load_credentials_into_cache_with(|| {
        // Legacy accounts are probed only after a definitive NoEntry. Retrying
        // them after an authorization error creates more prompts, not a fallback.
        match load_keyring_credentials()? {
            Some(root) => {
                remove_legacy_credentials_file_best_effort();
                Ok(Some(root))
            }
            None => migrate_legacy_sources_for_update().map(Some),
        }
    })
}

fn load_credentials_for_update_raw() -> Result<CredsRoot> {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return Ok(cached);
    }

    load_credentials_for_sync_binding_with(
        sync_write_gate().is_some(),
        || load_credentials_readonly_into_cache_with(load_credentials_for_recovery_readonly),
        load_credentials_for_update_unbound,
    )
}

fn load_credentials_for_update_unbound() -> Result<CredsRoot> {
    #[cfg(target_os = "android")]
    {
        return android_credentials_root_for_update(load_android_credentials);
    }

    #[cfg(not(target_os = "android"))]
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 同 load_credentials：不再每次 update 都尝试 delete legacy keyring
            // entries，避免反复触发 macOS Keychain ACL 弹窗。
            remove_legacy_credentials_file_best_effort();
            clear_vault_read_error();
            store_credentials_cache(&root);
            Ok(root)
        }
        Ok(None) => {
            // migrate_legacy_sources_for_update 内部如果实际 migrate 会调
            // save_credentials，cache 会被刷新；如果只返回 default root（没 legacy），
            // 我们这里再显式 cache 一次防御性补一下。
            let root = migrate_legacy_sources_for_update()?;
            clear_vault_read_error();
            store_credentials_cache(&root);
            Ok(root)
        }
        // 错误路径不缓存 —— 同 load_credentials 注释；让下次读重试 keyring。
        Err(e) => {
            record_vault_read_failure(&e);
            Err(e)
        }
    }
}

fn finish_credential_write(root: &CredsRoot, outcome: Result<()>) -> Result<()> {
    match outcome {
        Ok(()) => {
            store_credentials_cache(root);
            Ok(())
        }
        Err(error) => {
            // A native write may have committed even though confirmation failed.
            // Reconciliation must read the OS store rather than a stale process cache.
            if !matches!(
                error.downcast_ref::<VaultCommitFailure>(),
                Some(VaultCommitFailure::Unchanged)
            ) {
                *credentials_cache().lock() = None;
            }
            Err(error)
        }
    }
}

fn save_credentials(root: &CredsRoot) -> Result<()> {
    let mut cleaned = clean_credentials(root);
    let current_revision = credentials_cache()
        .lock()
        .as_ref()
        .map(|cached| cached.metadata_revision)
        .unwrap_or(0);
    if cleaned.metadata_revision <= current_revision {
        cleaned.metadata_revision = current_revision.saturating_add(1);
    }
    let cleaned = cleaned;

    #[cfg(target_os = "android")]
    {
        finish_credential_write(&cleaned, save_android_credentials(&cleaned))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let json = serde_json::to_string(&cleaned).context("encode credentials failed")?;
        // Updating this stable item keeps the user's authorization attached to it.
        // Do not recreate entries or rewrite/delete legacy chunks on each save.
        finish_credential_write(
            &cleaned,
            set_keyring_password(KEYRING_SINGLE_CREDENTIALS_ACCOUNT, &json),
        )?;
        remove_legacy_credentials_file_best_effort();
        return Ok(());
    }

    #[cfg(not(any(target_os = "android", target_os = "macos")))]
    {
        let json = serde_json::to_string(&cleaned).context("encode credentials failed")?;
        let outcome = save_chunked_credentials_with(
            &json,
            get_keyring_password,
            set_keyring_password,
            |account| {
                delete_keyring_password(account);
                Ok(())
            },
        );
        finish_credential_write(&cleaned, outcome)?;
        remove_legacy_credentials_file_best_effort();
        Ok(())
    }
}

fn lookup_account(root: &CredsRoot, account: CredentialAccount) -> Option<String> {
    let asr = root.providers.asr.get(&root.active.asr);
    let llm = root.providers.llm.get(&root.active.llm);
    let omni = root.omni.providers.get(&root.omni.active);
    let pick = |s: &Option<String>| s.as_ref().filter(|v| !v.is_empty()).cloned();
    match account {
        CredentialAccount::VolcengineAppKey => {
            asr.and_then(|e| pick(&e.appKey).or_else(|| pick(&e.apiKey)))
        }
        CredentialAccount::VolcengineAccessKey => asr.and_then(|e| pick(&e.accessKey)),
        CredentialAccount::VolcengineResourceId => asr.and_then(|e| pick(&e.resourceId)),
        CredentialAccount::VolcengineService => asr.and_then(|e| pick(&e.volcengineService)),
        CredentialAccount::VolcengineAuthMode => asr.and_then(|e| pick(&e.authMode)),
        CredentialAccount::VolcengineApiKey => asr.and_then(|e| pick(&e.volcengineApiKey)),
        CredentialAccount::ArkApiKey => llm.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::ArkModelId => llm.and_then(|e| pick(&e.model)),
        CredentialAccount::ArkEndpoint => llm.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrApiKey => asr.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::AsrEndpoint => asr.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrModel => asr.and_then(|e| pick(&e.model)),
        CredentialAccount::AsrVocabularyId => asr.and_then(|e| pick(&e.vocabularyId)),
        CredentialAccount::AsrAdvancedConfig => asr.and_then(|e| pick(&e.advancedConfig)),
        CredentialAccount::XfyunAppId => asr.and_then(|e| pick(&e.xfyunAppId)),
        CredentialAccount::XfyunApiKey => asr.and_then(|e| pick(&e.xfyunApiKey)),
        CredentialAccount::TencentCloudAppId => asr.and_then(|e| pick(&e.tencentCloudAppId)),
        CredentialAccount::TencentCloudSecretId => asr.and_then(|e| pick(&e.tencentCloudSecretId)),
        CredentialAccount::TencentCloudSecretKey => {
            asr.and_then(|e| pick(&e.tencentCloudSecretKey))
        }
        CredentialAccount::OmniApiKey => omni.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::OmniEndpoint => omni.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::OmniModel => omni.and_then(|e| pick(&e.model)),
    }
}

fn lookup_omni_account(
    root: &CredsRoot,
    provider_id: &str,
    account: CredentialAccount,
) -> Result<Option<String>> {
    let entry = root.omni.providers.get(provider_id);
    let pick = |value: &Option<String>| value.as_ref().filter(|v| !v.is_empty()).cloned();
    let value = match account {
        CredentialAccount::OmniApiKey => entry.and_then(|entry| pick(&entry.apiKey)),
        CredentialAccount::OmniEndpoint => entry.and_then(|entry| pick(&entry.baseURL)),
        CredentialAccount::OmniModel => entry.and_then(|entry| pick(&entry.model)),
        _ => anyhow::bail!("credential account is not Omni-scoped"),
    };
    Ok(value)
}

fn write_omni_account(
    root: &mut CredsRoot,
    provider_id: &str,
    account: CredentialAccount,
    value: Option<String>,
) -> Result<()> {
    let entry = root
        .omni
        .providers
        .entry(provider_id.to_string())
        .or_default();
    let normalized = value.and_then(|value| (!value.is_empty()).then_some(value));
    match account {
        CredentialAccount::OmniApiKey => entry.apiKey = normalized,
        CredentialAccount::OmniEndpoint => entry.baseURL = normalized,
        CredentialAccount::OmniModel => entry.model = normalized,
        _ => anyhow::bail!("credential account is not Omni-scoped"),
    }
    Ok(())
}

fn write_account(root: &mut CredsRoot, account: CredentialAccount, value: Option<String>) {
    let asr_id = root.active.asr.clone();
    let llm_id = root.active.llm.clone();
    let omni_id = root.omni.active.clone();
    let normalized = value.and_then(|v| if v.is_empty() { None } else { Some(v) });
    match account {
        CredentialAccount::VolcengineAppKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.appKey = normalized;
        }
        CredentialAccount::VolcengineAccessKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.accessKey = normalized;
        }
        CredentialAccount::VolcengineResourceId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.resourceId = normalized;
        }
        CredentialAccount::VolcengineService => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.volcengineService = normalized;
        }
        CredentialAccount::VolcengineAuthMode => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.authMode = normalized;
        }
        CredentialAccount::VolcengineApiKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.volcengineApiKey = normalized;
        }
        CredentialAccount::ArkApiKey => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.apiKey = normalized;
        }
        CredentialAccount::ArkModelId => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.model = normalized;
        }
        CredentialAccount::ArkEndpoint => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.baseURL = normalized;
        }
        CredentialAccount::AsrApiKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.apiKey = normalized;
        }
        CredentialAccount::AsrEndpoint => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.baseURL = normalized;
        }
        CredentialAccount::AsrModel => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.model = normalized;
        }
        CredentialAccount::AsrVocabularyId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.vocabularyId = normalized;
        }
        CredentialAccount::AsrAdvancedConfig => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.advancedConfig = normalized;
        }
        CredentialAccount::XfyunAppId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.xfyunAppId = normalized;
        }
        CredentialAccount::XfyunApiKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.xfyunApiKey = normalized;
        }
        CredentialAccount::TencentCloudAppId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.tencentCloudAppId = normalized;
        }
        CredentialAccount::TencentCloudSecretId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.tencentCloudSecretId = normalized;
        }
        CredentialAccount::TencentCloudSecretKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.tencentCloudSecretKey = normalized;
        }
        CredentialAccount::OmniApiKey => {
            let entry = root.omni.providers.entry(omni_id).or_default();
            entry.apiKey = normalized;
        }
        CredentialAccount::OmniEndpoint => {
            let entry = root.omni.providers.entry(omni_id).or_default();
            entry.baseURL = normalized;
        }
        CredentialAccount::OmniModel => {
            let entry = root.omni.providers.entry(omni_id).or_default();
            entry.model = normalized;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CredentialAccount {
    VolcengineAppKey,
    VolcengineAccessKey,
    VolcengineResourceId,
    VolcengineService,
    VolcengineAuthMode,
    /// ASR API Key（普通服务 API Key 鉴权或 Agent Plan 使用，独立于旧版 Access Token 槽位）。
    VolcengineApiKey,
    ArkApiKey,
    ArkModelId,
    ArkEndpoint,
    /// Active ASR provider's API key (used by Whisper-compatible providers).
    AsrApiKey,
    /// Active ASR provider's base URL.
    AsrEndpoint,
    /// Active ASR provider's model name.
    AsrModel,
    /// Active ASR provider's optional hotword vocabulary ID.
    AsrVocabularyId,
    /// 通用 OpenAI 兼容 ASR 的高级配置 JSON（verboseJson / chunkDurationMs）。
    AsrAdvancedConfig,
    /// 讯飞开放平台应用 ID。
    XfyunAppId,
    /// 讯飞实时语音转写 APIKey。
    XfyunApiKey,
    /// 腾讯云账号 AppID。
    TencentCloudAppId,
    /// 腾讯云 API 密钥 SecretID。
    TencentCloudSecretId,
    /// 腾讯云 API 密钥 SecretKey。
    TencentCloudSecretKey,
    /// 多模态（Omni）模型的 API Key。仅多模态管线读取。
    OmniApiKey,
    /// 多模态（Omni）模型的 Base URL。
    OmniEndpoint,
    /// 多模态（Omni）模型的 model id。
    OmniModel,
}

impl CredentialAccount {
    /// Account names match the Swift `CredentialAccount` constants exactly so
    /// existing Keychain entries written by the macOS Swift app remain
    /// readable after upgrade.
    pub fn keyring_account(&self) -> &'static str {
        match self {
            CredentialAccount::VolcengineAppKey => "volcengine.app_key",
            CredentialAccount::VolcengineAccessKey => "volcengine.access_key",
            CredentialAccount::VolcengineResourceId => "volcengine.resource_id",
            CredentialAccount::VolcengineService => "volcengine.service",
            CredentialAccount::VolcengineAuthMode => "volcengine.auth_mode",
            CredentialAccount::VolcengineApiKey => "volcengine.api_key",
            CredentialAccount::ArkApiKey => "ark.api_key",
            CredentialAccount::ArkModelId => "ark.model_id",
            CredentialAccount::ArkEndpoint => "ark.endpoint",
            CredentialAccount::AsrApiKey => "asr.api_key",
            CredentialAccount::AsrEndpoint => "asr.endpoint",
            CredentialAccount::AsrModel => "asr.model",
            CredentialAccount::AsrVocabularyId => "asr.vocabulary_id",
            CredentialAccount::AsrAdvancedConfig => "asr.advanced_config",
            CredentialAccount::XfyunAppId => "xfyun.app_id",
            CredentialAccount::XfyunApiKey => "xfyun.api_key",
            CredentialAccount::TencentCloudAppId => "tencent_cloud.app_id",
            CredentialAccount::TencentCloudSecretId => "tencent_cloud.secret_id",
            CredentialAccount::TencentCloudSecretKey => "tencent_cloud.secret_key",
            CredentialAccount::OmniApiKey => "omni.api_key",
            CredentialAccount::OmniEndpoint => "omni.endpoint",
            CredentialAccount::OmniModel => "omni.model",
        }
    }

    pub fn all() -> &'static [CredentialAccount] {
        &[
            CredentialAccount::VolcengineAppKey,
            CredentialAccount::VolcengineAccessKey,
            CredentialAccount::VolcengineResourceId,
            CredentialAccount::VolcengineService,
            CredentialAccount::VolcengineAuthMode,
            CredentialAccount::VolcengineApiKey,
            CredentialAccount::ArkApiKey,
            CredentialAccount::ArkModelId,
            CredentialAccount::ArkEndpoint,
            CredentialAccount::AsrApiKey,
            CredentialAccount::AsrEndpoint,
            CredentialAccount::AsrModel,
            CredentialAccount::AsrVocabularyId,
            CredentialAccount::AsrAdvancedConfig,
            CredentialAccount::XfyunAppId,
            CredentialAccount::XfyunApiKey,
            CredentialAccount::TencentCloudAppId,
            CredentialAccount::TencentCloudSecretId,
            CredentialAccount::TencentCloudSecretKey,
            CredentialAccount::OmniApiKey,
            CredentialAccount::OmniEndpoint,
            CredentialAccount::OmniModel,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsSnapshot {
    pub volcengine_app_key: Option<String>,
    pub volcengine_access_key: Option<String>,
    pub volcengine_resource_id: Option<String>,
    pub volcengine_service: Option<String>,
    pub volcengine_auth_mode: Option<String>,
    pub volcengine_api_key: Option<String>,
    pub asr_api_key: Option<String>,
    pub asr_endpoint: Option<String>,
    pub asr_model: Option<String>,
    pub xfyun_app_id: Option<String>,
    pub xfyun_api_key: Option<String>,
    pub tencent_cloud_app_id: Option<String>,
    pub tencent_cloud_secret_id: Option<String>,
    pub tencent_cloud_secret_key: Option<String>,
    pub ark_api_key: Option<String>,
    pub ark_model_id: Option<String>,
    pub ark_endpoint: Option<String>,
    pub active_omni_provider: String,
    pub omni_api_key: Option<String>,
    pub omni_endpoint: Option<String>,
    pub omni_model: Option<String>,
}

/// Provider identities and their fields from the same successful vault read.
pub(crate) struct CredentialConfigurationSnapshot {
    pub active_asr_provider: String,
    pub active_llm_provider: String,
    pub credentials: CredentialsSnapshot,
}

fn configuration_snapshot_with(
    include_omni: bool,
    load: impl FnOnce() -> Result<CredsRoot>,
) -> Result<CredentialConfigurationSnapshot> {
    let root = load()?;
    let asr_id = &root.active.asr;
    let llm_id = &root.active.llm;
    Ok(CredentialConfigurationSnapshot {
        active_asr_provider: root
            .providers
            .asr
            .get(asr_id)
            .map(|entry| channel_provider_type(asr_id, entry))
            .unwrap_or(asr_id)
            .to_owned(),
        active_llm_provider: root
            .providers
            .llm
            .get(llm_id)
            .map(|entry| channel_provider_type(llm_id, entry))
            .unwrap_or(llm_id)
            .to_owned(),
        credentials: credentials_snapshot(&root, include_omni),
    })
}

fn credentials_snapshot(root: &CredsRoot, include_omni: bool) -> CredentialsSnapshot {
    CredentialsSnapshot {
        volcengine_app_key: lookup_account(root, CredentialAccount::VolcengineAppKey),
        volcengine_access_key: lookup_account(root, CredentialAccount::VolcengineAccessKey),
        volcengine_resource_id: lookup_account(root, CredentialAccount::VolcengineResourceId),
        volcengine_service: lookup_account(root, CredentialAccount::VolcengineService),
        volcengine_auth_mode: lookup_account(root, CredentialAccount::VolcengineAuthMode),
        volcengine_api_key: lookup_account(root, CredentialAccount::VolcengineApiKey),
        asr_api_key: lookup_account(root, CredentialAccount::AsrApiKey),
        asr_endpoint: lookup_account(root, CredentialAccount::AsrEndpoint),
        asr_model: lookup_account(root, CredentialAccount::AsrModel),
        xfyun_app_id: lookup_account(root, CredentialAccount::XfyunAppId),
        xfyun_api_key: lookup_account(root, CredentialAccount::XfyunApiKey),
        tencent_cloud_app_id: lookup_account(root, CredentialAccount::TencentCloudAppId),
        tencent_cloud_secret_id: lookup_account(root, CredentialAccount::TencentCloudSecretId),
        tencent_cloud_secret_key: lookup_account(root, CredentialAccount::TencentCloudSecretKey),
        ark_api_key: lookup_account(root, CredentialAccount::ArkApiKey),
        ark_model_id: lookup_account(root, CredentialAccount::ArkModelId),
        ark_endpoint: lookup_account(root, CredentialAccount::ArkEndpoint),
        active_omni_provider: root.omni.active.clone(),
        omni_api_key: include_omni
            .then(|| lookup_account(root, CredentialAccount::OmniApiKey))
            .flatten(),
        omni_endpoint: include_omni
            .then(|| lookup_account(root, CredentialAccount::OmniEndpoint))
            .flatten(),
        omni_model: include_omni
            .then(|| lookup_account(root, CredentialAccount::OmniModel))
            .flatten(),
    }
}

impl From<&ChannelTest> for ChannelTestSummary {
    fn from(value: &ChannelTest) -> Self {
        Self {
            ok: value.ok,
            latency_ms: value.latencyMs,
            at: value.at,
            error: value.error.clone(),
        }
    }
}

impl From<&ChannelTestSummary> for ChannelTest {
    fn from(value: &ChannelTestSummary) -> Self {
        Self {
            ok: value.ok,
            latencyMs: value.latency_ms,
            at: value.at,
            error: value.error.clone(),
        }
    }
}

fn channel_summaries<V: HasChannelMeta>(
    map: &HashMap<String, V>,
    name_of: impl Fn(&V) -> String,
) -> Vec<ChannelSummary> {
    let mut list: Vec<ChannelSummary> = map
        .iter()
        .map(|(id, entry)| {
            let meta = entry.meta();
            ChannelSummary {
                id: id.clone(),
                name: name_of(entry),
                provider_type: channel_provider_type(id, entry).to_string(),
                enabled: meta.enabled,
                order: meta.order.unwrap_or(u32::MAX),
                last_test: meta.lastTest.as_ref().map(ChannelTestSummary::from),
            }
        })
        .collect();
    // 与 current_channel_id 同序：order 升序，同 order 按 id 字母序。
    list.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.id.cmp(&right.id))
    });
    list
}

fn credential_metadata(root: &CredsRoot) -> openless_core::CredentialMetadata {
    openless_core::CredentialMetadata::from_parts(
        channel_summaries(&root.providers.asr, |entry| {
            entry.displayName.clone().unwrap_or_default()
        }),
        channel_summaries(&root.providers.llm, |entry| {
            entry.displayName.clone().unwrap_or_default()
        }),
        root.active.asr.clone(),
        root.active.llm.clone(),
        root.omni.active.clone(),
        root.metadata_revision,
    )
}

fn replace_channel_metadata<V: Default + HasChannelMeta>(
    map: &mut HashMap<String, V>,
    channels: Vec<ChannelSummary>,
    mut set_name: impl FnMut(&mut V, Option<String>),
) {
    let mut previous = std::mem::take(map);
    for channel in channels {
        let mut entry = previous.remove(&channel.id).unwrap_or_default();
        let meta = entry.meta_mut();
        meta.providerType = Some(channel.provider_type);
        meta.order = Some(channel.order);
        meta.enabled = channel.enabled;
        meta.lastTest = channel.last_test.as_ref().map(ChannelTest::from);
        let name = (!channel.name.trim().is_empty()).then(|| channel.name.trim().to_string());
        set_name(&mut entry, name);
        map.insert(channel.id, entry);
    }
}

fn apply_credential_metadata(
    root: &mut CredsRoot,
    metadata: openless_core::CredentialMetadata,
) -> Result<()> {
    let expected = root.metadata_revision.saturating_add(1);
    if metadata.revision() != expected {
        anyhow::bail!(
            "credential metadata revision conflict: expected {expected}, got {}",
            metadata.revision()
        );
    }
    replace_channel_metadata(
        &mut root.providers.asr,
        metadata.list_channels(ChannelKind::Asr),
        |entry, name| entry.displayName = name,
    );
    replace_channel_metadata(
        &mut root.providers.llm,
        metadata.list_channels(ChannelKind::Llm),
        |entry, name| entry.displayName = name,
    );
    root.active.asr = metadata.active_provider(openless_core::ProviderSlot::Asr);
    root.active.llm = metadata.active_provider(openless_core::ProviderSlot::Llm);
    root.omni.active = metadata.active_provider(openless_core::ProviderSlot::Omni);
    root.metadata_revision = metadata.revision();
    Ok(())
}

fn channel_has_secrets(root: &CredsRoot, kind: ChannelKind, id: &str) -> bool {
    match kind {
        ChannelKind::Asr => root
            .providers
            .asr
            .get(id)
            .is_some_and(|entry| !entry.has_no_content()),
        ChannelKind::Llm => root
            .providers
            .llm
            .get(id)
            .is_some_and(|entry| !entry.has_no_content()),
    }
}

fn canonical_headers(headers: &Option<HashMap<String, String>>) -> Result<Option<String>> {
    headers
        .as_ref()
        .map(|values| {
            let ordered = values.iter().collect::<BTreeMap<_, _>>();
            serde_json::to_string(&ordered).context("encode sync provider headers")
        })
        .transpose()
}

fn sync_channel(
    id: &str,
    namespace: openless_core::credentials::SyncNamespace,
    meta: &ChannelMeta,
    name: &Option<String>,
    active: &str,
) -> openless_core::credentials::SyncChannel {
    openless_core::credentials::SyncChannel {
        id: id.into(),
        namespace,
        provider_type: meta.providerType.clone().unwrap_or_else(|| id.into()),
        name: name.clone().unwrap_or_default(),
        enabled: meta.enabled,
        order: meta.order.unwrap_or(u32::MAX),
        active: id == active,
    }
}

/// Project actual storage fields, not lookup_account's compatibility fallbacks.
/// In particular appKey=None must not become a duplicated apiKey after a restore.
fn export_sync_credentials_root(
    root: &CredsRoot,
) -> Result<openless_core::credentials::SyncCredentials> {
    use openless_core::credentials::{SyncCredentialRecord, SyncCredentials, SyncNamespace};
    let mut snapshot = SyncCredentials {
        channels: Vec::new(),
        credentials: Vec::new(),
    };
    macro_rules! accounts {
        ($entry:expr, $( $name:literal => $field:ident ),+ $(,)?) => {{
            let mut values = BTreeMap::new();
            $(if let Some(value) = &$entry.$field { values.insert($name.into(), value.clone()); })+
            values
        }};
    }
    for (id, entry) in &root.providers.asr {
        snapshot.channels.push(sync_channel(
            id,
            SyncNamespace::Asr,
            &entry.channel,
            &entry.displayName,
            &root.active.asr,
        ));
        snapshot.credentials.push(SyncCredentialRecord { channel_id: id.clone(), namespace: SyncNamespace::Asr,
            accounts: accounts!(entry,
                "asr.api_key"=>apiKey, "asr.endpoint"=>baseURL, "asr.model"=>model,
                "asr.vocabulary_id"=>vocabularyId, "asr.advanced_config"=>advancedConfig,
                "volcengine.app_key"=>appKey, "volcengine.access_key"=>accessKey,
                "volcengine.resource_id"=>resourceId, "volcengine.service"=>volcengineService,
                "volcengine.auth_mode"=>authMode, "volcengine.api_key"=>volcengineApiKey,
                "xfyun.app_id"=>xfyunAppId, "xfyun.api_key"=>xfyunApiKey,
                "tencent_cloud.app_id"=>tencentCloudAppId, "tencent_cloud.secret_id"=>tencentCloudSecretId,
                "tencent_cloud.secret_key"=>tencentCloudSecretKey),
        });
    }
    for (id, entry) in &root.providers.llm {
        snapshot.channels.push(sync_channel(
            id,
            SyncNamespace::Llm,
            &entry.channel,
            &entry.displayName,
            &root.active.llm,
        ));
        let mut values = accounts!(entry, "ark.api_key"=>apiKey, "ark.endpoint"=>baseURL, "ark.model_id"=>model,
            "ark.request_format"=>requestFormat, "ark.messages_thinking"=>messagesThinking,
            "ark.max_tokens"=>maxTokens, "ark.thinking_budget"=>thinkingBudget);
        if let Some(value) = entry.temperature {
            values.insert("ark.temperature".into(), value.to_string());
        }
        if let Some(value) = canonical_headers(&entry.extraHeaders)? {
            values.insert("ark.extra_headers".into(), value);
        }
        snapshot.credentials.push(SyncCredentialRecord {
            channel_id: id.clone(),
            namespace: SyncNamespace::Llm,
            accounts: values,
        });
    }
    let mut omni_ids = root.omni.providers.keys().cloned().collect::<Vec<_>>();
    // Omni has a fixed-provider selector, which may be persisted before any key.
    if !root.omni.active.is_empty() && !omni_ids.contains(&root.omni.active) {
        omni_ids.push(root.omni.active.clone());
    }
    omni_ids.sort();
    for (index, id) in omni_ids.iter().enumerate() {
        let empty = CredsOmniEntry::default();
        let entry = root.omni.providers.get(id).unwrap_or(&empty);
        let mut channel = sync_channel(
            id,
            SyncNamespace::Omni,
            &entry.channel,
            &entry.displayName,
            &root.omni.active,
        );
        if entry.channel.order.is_none() {
            channel.order = u32::try_from(index).context("too many Omni providers")?;
        }
        snapshot.channels.push(channel);
        let mut values =
            accounts!(entry, "omni.api_key"=>apiKey, "omni.endpoint"=>baseURL, "omni.model"=>model);
        if let Some(value) = entry.temperature {
            values.insert("omni.temperature".into(), value.to_string());
        }
        if let Some(value) = canonical_headers(&entry.extraHeaders)? {
            values.insert("omni.extra_headers".into(), value);
        }
        snapshot.credentials.push(SyncCredentialRecord {
            channel_id: id.clone(),
            namespace: SyncNamespace::Omni,
            accounts: values,
        });
    }
    snapshot
        .channels
        .sort_by(|a, b| (a.namespace, a.order, &a.id).cmp(&(b.namespace, b.order, &b.id)));
    snapshot
        .credentials
        .sort_by(|a, b| (a.namespace, &a.channel_id).cmp(&(b.namespace, &b.channel_id)));
    snapshot.validate().map_err(|error| {
        // Recheck the pure validator only to retain its fixed, value-free cause.
        // The original rejection and BackendError remain unchanged.
        if let Err(cause) = openless_core::cloud_sync_e2ee_documents::validate_credential_set(
            &snapshot.channels,
            &snapshot.credentials,
        ) {
            log::warn!("[e2ee-capture] stage=credential_validation code={cause}");
        }
        anyhow::Error::new(error)
    })?;
    Ok(snapshot)
}

/// Classify typed causes without formatting an error, credential attribute or blob.
fn sync_capture_read_error_code(error: &anyhow::Error) -> &'static str {
    let mut fallback = "native_store_failed";
    for cause in error.chain() {
        if let Some(code) = sync_keyring_error_code(cause) {
            if code != "keyring_platform_failure" {
                return code;
            }
            fallback = code;
        }
        if let Some(error) = cause.downcast_ref::<serde_json::Error>() {
            use serde_json::error::Category;
            return match error.classify() {
                Category::Io => error
                    .io_error_kind()
                    .map(sync_io_error_code)
                    .unwrap_or("json_io"),
                Category::Syntax => "json_syntax",
                Category::Data => "json_data",
                Category::Eof => "json_eof",
            };
        }
        if let Some(error) = cause.downcast_ref::<std::io::Error>() {
            return sync_io_error_code(error.kind());
        }
        if cause.is::<std::str::Utf8Error>() || cause.is::<std::string::FromUtf8Error>() {
            return "invalid_utf8";
        }
        if let Some(error) = cause.downcast_ref::<openless_core::BackendError>() {
            use openless_core::BackendErrorCode;
            return match error.code {
                BackendErrorCode::PermissionDenied => "backend_permission_denied",
                BackendErrorCode::InvalidArgument => "backend_invalid_argument",
                BackendErrorCode::Persistence => "backend_persistence",
                BackendErrorCode::OutcomeUnknown => "backend_outcome_unknown",
                BackendErrorCode::Unsupported => "backend_unsupported",
                _ => "backend_failed",
            };
        }
    }
    fallback
}

fn sync_io_error_code(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::PermissionDenied => "io_permission_denied",
        ErrorKind::NotFound => "io_not_found",
        ErrorKind::InvalidData => "io_invalid_data",
        ErrorKind::InvalidInput => "io_invalid_input",
        ErrorKind::UnexpectedEof => "io_unexpected_eof",
        ErrorKind::TimedOut => "io_timed_out",
        ErrorKind::WouldBlock => "io_would_block",
        ErrorKind::Interrupted => "io_interrupted",
        _ => "io_other",
    }
}

fn sync_keyring_error_code(_cause: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    #[cfg(not(target_os = "android"))]
    if let Some(error) = _cause.downcast_ref::<keyring::Error>() {
        return Some(match error {
            keyring::Error::NoStorageAccess(_) => "keyring_access_denied",
            keyring::Error::NoEntry => "keyring_no_entry",
            keyring::Error::BadEncoding(_) => "keyring_bad_encoding",
            keyring::Error::TooLong(_, _) => "keyring_attribute_too_long",
            keyring::Error::Invalid(_, _) => "keyring_invalid_attribute",
            keyring::Error::Ambiguous(_) => "keyring_ambiguous",
            keyring::Error::PlatformFailure(_) => "keyring_platform_failure",
            _ => "keyring_other",
        });
    }
    None
}

fn sync_identity_error_code(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("empty")
    } else if value.len() > 512 {
        Some("too_long")
    } else if matches!(value, "." | "..") {
        Some("dot_segment")
    } else if value.chars().any(char::is_control) {
        Some("control_character")
    } else if value.contains(['/', '\\']) {
        Some("path_separator")
    } else {
        None
    }
}

fn capture_sync_credentials_with(
    load: impl FnOnce() -> Result<CredsRoot>,
) -> Result<openless_core::credentials::SyncCredentials> {
    let root = load().inspect_err(|error| {
        let code = sync_capture_read_error_code(error);
        log::warn!("[e2ee-capture] stage=credential_load code={code}");
    })?;
    export_sync_credentials_root(&root).inspect_err(|_| {
        // Metadata counts identify invalid legacy state without exposing channel names,
        // accounts, secret values, provider IDs, endpoints, or underlying error bodies.
        let disabled_active = usize::from(root.providers.asr.get(&root.active.asr).is_some_and(|entry| !entry.channel.enabled))
            + usize::from(root.providers.llm.get(&root.active.llm).is_some_and(|entry| !entry.channel.enabled))
            + usize::from(root.omni.providers.get(&root.omni.active).is_some_and(|entry| !entry.channel.enabled));
        let mismatched_omni = root.omni.providers.iter().filter(|(id,entry)| entry.channel.providerType.as_ref().is_some_and(|provider|provider!=*id)).count();
        let identities = root.providers.asr.iter().map(|(id, entry)| ("asr", id, &entry.channel))
            .chain(root.providers.llm.iter().map(|(id, entry)| ("llm", id, &entry.channel)))
            .chain(root.omni.providers.iter().map(|(id, entry)| ("omni", id, &entry.channel)));
        for (namespace, id, meta) in identities {
            for (field, value) in [("channel_id", Some(id.as_str())), ("provider_type", meta.providerType.as_deref())] {
                if let Some(code) = value.and_then(sync_identity_error_code) {
                    log::warn!("[e2ee-capture] stage=credential_identity namespace={namespace} field={field} code={code}");
                }
            }
        }
        let invalid_id = |value: &str| sync_identity_error_code(value).is_some();
        let invalid_identities = root.providers.asr.iter().filter(|(id,entry)|invalid_id(id) || entry.channel.providerType.as_deref().is_some_and(invalid_id)).count()
            + root.providers.llm.iter().filter(|(id,entry)|invalid_id(id) || entry.channel.providerType.as_deref().is_some_and(invalid_id)).count()
            + root.omni.providers.iter().filter(|(id,entry)|invalid_id(id) || entry.channel.providerType.as_deref().is_some_and(invalid_id)).count();
        log::warn!("[e2ee-capture] stage=credential_projection code=invalid_snapshot disabled_active={disabled_active} mismatched_omni={mismatched_omni} invalid_identities={invalid_identities}");
    })
}

fn restored_channel_meta(channel: &openless_core::credentials::SyncChannel) -> ChannelMeta {
    ChannelMeta {
        providerType: Some(channel.provider_type.clone()),
        order: Some(channel.order),
        enabled: channel.enabled,
        lastTest: None,
    }
}

fn apply_sync_credentials_root(
    root: &CredsRoot,
    snapshot: &openless_core::credentials::SyncCredentials,
) -> Result<CredsRoot> {
    use openless_core::credentials::SyncNamespace;
    snapshot.validate().map_err(anyhow::Error::new)?;
    let accounts = snapshot
        .credentials
        .iter()
        .map(|record| {
            (
                (record.namespace, record.channel_id.as_str()),
                &record.accounts,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut next = root.clone(); // Preserve Marketplace OAuth and all unrelated local root state.
    next.version = CHANNELS_SCHEMA_VERSION;
    next.providers = CredsProviders::default();
    next.omni.providers.clear();
    next.active.asr.clear();
    next.active.llm.clear();
    next.omni.active.clear();
    for channel in &snapshot.channels {
        let values = accounts
            .get(&(channel.namespace, channel.id.as_str()))
            .context("missing sync credential channel")?;
        let value = |name: &str| values.get(name).cloned();
        let name = (!channel.name.is_empty()).then(|| channel.name.clone());
        match channel.namespace {
            SyncNamespace::Asr => {
                next.providers.asr.insert(
                    channel.id.clone(),
                    CredsAsrEntry {
                        channel: restored_channel_meta(channel),
                        displayName: name,
                        apiKey: value("asr.api_key"),
                        baseURL: value("asr.endpoint"),
                        model: value("asr.model"),
                        vocabularyId: value("asr.vocabulary_id"),
                        advancedConfig: value("asr.advanced_config"),
                        appKey: value("volcengine.app_key"),
                        accessKey: value("volcengine.access_key"),
                        resourceId: value("volcengine.resource_id"),
                        volcengineService: value("volcengine.service"),
                        authMode: value("volcengine.auth_mode"),
                        volcengineApiKey: value("volcengine.api_key"),
                        xfyunAppId: value("xfyun.app_id"),
                        xfyunApiKey: value("xfyun.api_key"),
                        tencentCloudAppId: value("tencent_cloud.app_id"),
                        tencentCloudSecretId: value("tencent_cloud.secret_id"),
                        tencentCloudSecretKey: value("tencent_cloud.secret_key"),
                    },
                );
                if channel.active {
                    next.active.asr = channel.id.clone();
                }
            }
            SyncNamespace::Llm => {
                let mut protocol = openless_core::llm_protocol::LlmProtocolConfig {
                    format: openless_core::llm_protocol::LlmRequestFormat::default_for(
                        &channel.provider_type,
                    ),
                    ..Default::default()
                };
                for key in [
                    "ark.request_format",
                    "ark.messages_thinking",
                    "ark.max_tokens",
                    "ark.thinking_budget",
                ] {
                    if let Some(value) = values.get(key) {
                        protocol.apply(key, value)?;
                    }
                }
                let temperature = values
                    .get("ark.temperature")
                    .map(|value| parse_llm_temperature(value))
                    .transpose()?
                    .flatten();
                let headers = values
                    .get("ark.extra_headers")
                    .map(|value| parse_extra_headers_json(value))
                    .transpose()?;
                protocol.validate()?;
                if let Some(headers) = &headers {
                    protocol.validate_headers(headers)?;
                }
                next.providers.llm.insert(
                    channel.id.clone(),
                    CredsLlmEntry {
                        channel: restored_channel_meta(channel),
                        displayName: name,
                        apiKey: value("ark.api_key"),
                        baseURL: value("ark.endpoint"),
                        model: value("ark.model_id"),
                        temperature,
                        extraHeaders: headers,
                        requestFormat: value("ark.request_format"),
                        messagesThinking: value("ark.messages_thinking"),
                        maxTokens: value("ark.max_tokens"),
                        thinkingBudget: value("ark.thinking_budget"),
                    },
                );
                if channel.active {
                    next.active.llm = channel.id.clone();
                }
            }
            SyncNamespace::Omni => {
                anyhow::ensure!(
                    channel.id == channel.provider_type,
                    "Omni provider identity cannot be remapped"
                );
                let temperature = values
                    .get("omni.temperature")
                    .map(|value| parse_llm_temperature(value))
                    .transpose()?
                    .flatten();
                let headers = values
                    .get("omni.extra_headers")
                    .map(|value| parse_extra_headers_json(value))
                    .transpose()?;
                next.omni.providers.insert(
                    channel.id.clone(),
                    CredsOmniEntry {
                        channel: restored_channel_meta(channel),
                        displayName: name,
                        apiKey: value("omni.api_key"),
                        baseURL: value("omni.endpoint"),
                        model: value("omni.model"),
                        temperature,
                        extraHeaders: headers,
                    },
                );
                if channel.active {
                    next.omni.active = channel.id.clone();
                }
            }
        }
    }
    Ok(next)
}

/// 凭据存储——系统凭据库；旧 JSON 文件只作为迁移来源。
pub struct CredentialsVault;

impl CredentialsVault {
    /// 系统凭据库 service name；macOS 下对应 Keychain service。
    pub const SERVICE_NAME: &'static str = "com.openless.app";

    pub(crate) fn bind_sync_gate(
        gate: std::sync::Arc<openless_core::credentials::SyncWriteGate>,
    ) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut installed = SYNC_WRITE_GATE.get_or_init(|| Mutex::new(None)).lock();
        // Binding is deliberately independent of OS authorization and migration.
        // Status/read/export lazily inspect the native store under this barrier;
        // denial must remain retryable rather than disabling sync construction.
        install_sync_write_gate(&mut installed, gate)
    }

    pub(crate) fn export_sync_credentials_readonly(
    ) -> Result<openless_core::credentials::SyncCredentials> {
        let _guard = credentials_lock().lock();
        // Cold capture uses the same strict read-only loader as bound getters.
        // Legacy sources remain readable without migration, even before startup
        // recovery. Failed OS access leaves the next capture free to retry.
        anyhow::ensure!(
            sync_write_gate().is_some(),
            "encrypted sync credential gate is not bound"
        );
        capture_sync_credentials_with(|| {
            let mut root =
                load_credentials_readonly_into_cache_with(load_credentials_for_recovery_readonly)?;
            migrate_channels(&mut root);
            Ok(root)
        })
    }

    pub(crate) fn export_sync_credentials(
        permit: &openless_core::credentials::ExclusivePermit,
    ) -> Result<openless_core::credentials::SyncCredentials> {
        require_sync_exclusive(permit)?;
        let _guard = credentials_lock().lock();
        capture_sync_credentials_with(load_credentials_for_update)
    }

    pub(crate) fn replace_sync_credentials(
        snapshot: &openless_core::credentials::SyncCredentials,
        permit: &openless_core::credentials::ExclusivePermit,
    ) -> Result<()> {
        require_sync_exclusive(permit)?;
        let _guard = credentials_lock().lock();
        let current = load_credentials_for_update()?;
        let replacement = apply_sync_credentials_root(&current, snapshot)?;
        // The outer restore transaction owns the exclusive generation/receipt.
        save_credentials(&replacement)
    }

    pub(crate) fn read_sync_secret(
        account: &openless_core::credentials::SyncSecretAccount,
    ) -> Result<Option<openless_core::SecretValue>> {
        let _guard = credentials_lock().lock();
        read_sync_secret_raw(account)
    }

    pub(crate) fn write_sync_secret(
        account: &openless_core::credentials::SyncSecretAccount,
        value: &openless_core::SecretValue,
    ) -> Result<()> {
        validate_sync_key(value)?;
        let _guard = credentials_lock().lock();
        if let Some(existing) = read_sync_secret_raw(account)? {
            if existing == *value {
                return Ok(());
            }
            anyhow::bail!("refusing to replace an existing encrypted sync key binding");
        }
        write_sync_secret_raw(account, value)?;
        let stored = read_sync_secret_raw(account)?
            .context("encrypted sync key write could not be verified")?;
        anyhow::ensure!(
            stored == *value,
            "encrypted sync key write verification failed"
        );
        Ok(())
    }

    pub(crate) fn remove_sync_secret(
        account: &openless_core::credentials::SyncSecretAccount,
    ) -> Result<()> {
        let _guard = credentials_lock().lock();
        remove_sync_secret_raw(account)
    }

    /// Last envelope/keyring read failure, if this process has not successfully
    /// loaded credentials since. Distinguishes "vault unreadable" from
    /// "user has not configured a provider" (empty default is volcengine).
    pub fn last_read_error() -> Option<String> {
        last_vault_read_error_slot().lock().clone()
    }

    pub fn load_metadata() -> Result<openless_core::CredentialMetadata> {
        let _guard = credentials_lock().lock();
        Ok(credential_metadata(&load_credentials_for_update()?))
    }

    pub fn save_metadata(metadata: openless_core::CredentialMetadata) -> Result<()> {
        mutate_credentials(
            openless_core::credentials::ChangeOrigin::User,
            move |root| {
                apply_credential_metadata(root, metadata)?;
                Ok(true)
            },
        )
    }

    pub fn channel_has_secrets(kind: ChannelKind, id: &str) -> Result<bool> {
        let _guard = credentials_lock().lock();
        Ok(channel_has_secrets(
            &load_credentials_for_update()?,
            kind,
            id,
        ))
    }

    pub fn get(account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        Ok(lookup_account(&load_credentials_for_update()?, account))
    }

    pub fn set(account: CredentialAccount, value: &str) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            write_account(
                root,
                account,
                (!value.is_empty()).then(|| value.to_string()),
            );
            Ok(true)
        })
    }

    pub fn get_for_asr_provider(id: &str, account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.asr = id.to_string();
        Ok(lookup_account(&root, account))
    }

    pub fn set_for_asr_provider(id: &str, account: CredentialAccount, value: &str) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            let active = root.active.asr.clone();
            root.active.asr = id.to_string();
            write_account(
                root,
                account,
                (!value.is_empty()).then(|| value.to_string()),
            );
            root.active.asr = active;
            Ok(true)
        })
    }

    pub fn remove(account: CredentialAccount) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            write_account(root, account, None);
            Ok(true)
        })
    }

    /// GitHub OAuth token for authenticated marketplace operations.
    ///
    /// This credential deliberately has no generic `CredentialAccount` and is
    /// excluded from `CredentialsSnapshot`, so frontend IPC can never read it.
    pub fn get_marketplace_github_token() -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        if marketplace_token_is_rejected() {
            return Ok(None);
        }
        #[cfg(target_os = "android")]
        {
            let path = android_credentials_path()?;
            return get_android_marketplace_token_at(
                &path,
                android_marketplace_legacy_scrubbed(),
                android_marketplace_token(),
            );
        }
        #[cfg(not(target_os = "android"))]
        Ok(lookup_marketplace_github_token(
            &load_credentials_for_update()?,
        ))
    }

    pub fn set_marketplace_github_token(value: &str) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            let _guard = credentials_lock().lock();
            ensure_android_marketplace_legacy_scrubbed()?;
            *android_marketplace_token().lock() =
                (!value.trim().is_empty()).then(|| MarketplaceGithubToken(value.to_string()));
            if value.trim().is_empty() {
                invalidate_marketplace_token_process_local();
            } else {
                mark_marketplace_token_verified();
            }
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        set_marketplace_token_with(
            sync_write_gate(),
            value,
            load_credentials_for_update,
            save_credentials,
        )
    }

    pub fn remove_marketplace_github_token() -> Result<()> {
        #[cfg(target_os = "android")]
        {
            invalidate_marketplace_token_process_local();
            let _guard = credentials_lock().lock();
            invalidate_marketplace_token_with(ensure_android_marketplace_legacy_scrubbed)
        }
        #[cfg(not(target_os = "android"))]
        remove_marketplace_token_with(
            sync_write_gate(),
            load_credentials_for_update,
            save_credentials,
        )
    }

    #[cfg(test)]
    pub(crate) fn seed_marketplace_github_token_for_tests(value: &str) {
        let _guard = credentials_lock().lock();
        let mut root = CredsRoot::default();
        write_marketplace_github_token(&mut root, Some(value.to_string()));
        store_credentials_cache(&root);
        mark_marketplace_token_verified();
    }

    #[cfg(test)]
    pub(crate) fn reject_marketplace_github_token_for_tests(
        durable_delete: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let _guard = credentials_lock().lock();
        invalidate_marketplace_token_with(durable_delete)
    }

    #[cfg(test)]
    pub(crate) fn reset_marketplace_github_token_for_tests() {
        let _guard = credentials_lock().lock();
        store_credentials_cache(&CredsRoot::default());
        MARKETPLACE_TOKEN_REJECTED.store(false, Ordering::SeqCst);
    }

    /// 当前 ASR 渠道的**厂商 id（providerType）**，不是渠道 id。
    ///
    /// 渠道化后 `active.asr` 存的是渠道 id（多把 key 时是 uuid），但全代码库几十处
    /// `get_active_asr() == crate::asr::bailian::PROVIDER_ID` 式的比较、以及
    /// `coordinator::resolve_effective_asr_provider` 的协议路由，要的都是厂商 id。
    /// 因此这里做一次转换，让那些调用点保持零改动。
    /// 需要渠道 id 本身时用 `get_active_asr_channel_id`。
    pub fn get_active_asr() -> String {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        let id = root.active.asr.clone();
        root.providers
            .asr
            .get(&id)
            .map(|entry| channel_provider_type(&id, entry).to_string())
            .unwrap_or(id)
    }

    pub fn set_active_asr_provider(id: &str) -> Result<()> {
        Self::select_active_provider(openless_core::ProviderSlot::Asr, id)
    }

    pub fn set_active_llm_provider(id: &str) -> Result<()> {
        Self::select_active_provider(openless_core::ProviderSlot::Llm, id)
    }

    /// 当前 LLM 渠道的**厂商 id（providerType）**。理由同 `get_active_asr`。
    pub fn get_active_llm() -> String {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        let id = root.active.llm.clone();
        root.providers
            .llm
            .get(&id)
            .map(|entry| channel_provider_type(&id, entry).to_string())
            .unwrap_or(id)
    }

    /// 指定 LLM 渠道的自定义请求头（测试连通用；不传渠道时用 `get_active_llm_extra_headers`）。
    pub fn get_llm_extra_headers_for_channel(id: &str) -> HashMap<String, String> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials();
        root.active.llm = id.to_string();
        active_llm_extra_headers(&root)
    }

    /// 指定 LLM 渠道的采样温度。
    pub fn get_llm_temperature_for_channel(id: &str) -> Option<f32> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials();
        root.active.llm = id.to_string();
        active_llm_temperature(&root)
    }

    /// 按渠道 id 读 LLM 凭据（编辑非当前卡片时用）。
    ///
    /// ASR 早就有 `get_for_asr_provider`；LLM 侧原本只能读"当前 active"，
    /// 渠道化后必须能读任意一张卡片。
    pub fn get_for_llm_provider(id: &str, account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.llm = id.to_string();
        Ok(lookup_account(&root, account))
    }

    pub fn set_for_llm_provider(id: &str, account: CredentialAccount, value: &str) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            let active = root.active.llm.clone();
            root.active.llm = id.to_string();
            write_account(
                root,
                account,
                (!value.is_empty()).then(|| value.to_string()),
            );
            root.active.llm = active;
            Ok(true)
        })
    }

    pub fn get_active_omni() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().omni.active
    }

    pub fn set_active_omni_provider(id: &str) -> Result<()> {
        Self::select_active_provider(openless_core::ProviderSlot::Omni, id)
    }

    fn select_active_provider(slot: openless_core::ProviderSlot, id: &str) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            let mut metadata = credential_metadata(root);
            let revision = metadata.revision();
            metadata
                .select_active_provider(slot, id.to_string())
                .map_err(anyhow::Error::new)?;
            if metadata.revision() == revision {
                return Ok(false);
            }
            apply_credential_metadata(root, metadata)?;
            Ok(true)
        })
    }

    pub fn get_for_omni_provider(id: &str, account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        lookup_omni_account(&load_credentials(), id, account)
    }

    pub fn set_for_omni_provider(id: &str, account: CredentialAccount, value: &str) -> Result<()> {
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            write_omni_account(
                root,
                id,
                account,
                (!value.is_empty()).then(|| value.to_string()),
            )?;
            Ok(true)
        })
    }

    pub fn get_active_omni_extra_headers() -> HashMap<String, String> {
        let _guard = credentials_lock().lock();
        active_omni_extra_headers(&load_credentials())
    }

    pub fn get_active_omni_extra_headers_json() -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        active_omni_extra_headers_json(&load_credentials())
    }

    pub fn get_omni_extra_headers_json_for_provider(id: &str) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        omni_extra_headers_json(&load_credentials(), id)
    }

    pub fn get_active_omni_temperature() -> Option<f32> {
        let _guard = credentials_lock().lock();
        active_omni_temperature(&load_credentials())
    }

    pub fn get_active_omni_temperature_string() -> Option<String> {
        let _guard = credentials_lock().lock();
        active_omni_temperature_string(&load_credentials())
    }

    pub fn get_omni_temperature_string_for_provider(id: &str) -> Option<String> {
        let _guard = credentials_lock().lock();
        omni_temperature_string(&load_credentials(), id)
    }

    pub fn set_active_omni_temperature(value: &str) -> Result<()> {
        let temperature = parse_llm_temperature(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.omni
                .providers
                .entry(root.omni.active.clone())
                .or_default()
                .temperature = temperature;
            Ok(true)
        })
    }

    pub fn set_omni_temperature_for_provider(id: &str, value: &str) -> Result<()> {
        let temperature = parse_llm_temperature(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.omni
                .providers
                .entry(id.to_string())
                .or_default()
                .temperature = temperature;
            Ok(true)
        })
    }

    pub fn set_active_omni_extra_headers_json(value: &str) -> Result<()> {
        let headers = parse_extra_headers_json(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.omni
                .providers
                .entry(root.omni.active.clone())
                .or_default()
                .extraHeaders = (!headers.is_empty()).then_some(headers);
            Ok(true)
        })
    }

    pub fn set_omni_extra_headers_json_for_provider(id: &str, value: &str) -> Result<()> {
        let headers = parse_extra_headers_json(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.omni
                .providers
                .entry(id.to_string())
                .or_default()
                .extraHeaders = (!headers.is_empty()).then_some(headers);
            Ok(true)
        })
    }

    pub fn get_active_llm_extra_headers() -> HashMap<String, String> {
        let _guard = credentials_lock().lock();
        active_llm_extra_headers(&load_credentials())
    }

    pub fn get_active_llm_extra_headers_json() -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        active_llm_extra_headers_json(&load_credentials())
    }

    pub fn get_active_llm_temperature() -> Option<f32> {
        let _guard = credentials_lock().lock();
        active_llm_temperature(&load_credentials())
    }

    pub fn get_active_llm_temperature_string() -> Option<String> {
        let _guard = credentials_lock().lock();
        active_llm_temperature_string(&load_credentials())
    }

    pub fn set_active_llm_temperature(value: &str) -> Result<()> {
        let temperature = parse_llm_temperature(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.providers
                .llm
                .entry(root.active.llm.clone())
                .or_default()
                .temperature = temperature;
            Ok(true)
        })
    }

    pub fn get_llm_protocol_option(id: Option<&str>, account: &str) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let id = id.unwrap_or(&root.active.llm).to_string();
        match root.providers.llm.get_mut(&id) {
            Some(entry) => Ok(entry.protocol_option(account)?.clone()),
            None => Ok(None),
        }
    }

    pub fn set_llm_protocol_option(id: Option<&str>, account: &str, value: &str) -> Result<()> {
        let mut config = openless_core::llm_protocol::LlmProtocolConfig::default();
        config.apply(account, value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            let id = id.unwrap_or(&root.active.llm).to_string();
            let entry = root.providers.llm.entry(id).or_default();
            *entry.protocol_option(account)? =
                (!value.trim().is_empty()).then(|| value.trim().to_string());
            entry.channel.lastTest = None;
            Ok(true)
        })
    }

    /// 写入指定 LLM 渠道的采样温度，不改变 active 渠道。
    pub fn set_llm_temperature_for_provider(id: &str, value: &str) -> Result<()> {
        let temperature = parse_llm_temperature(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            set_llm_temperature_for_provider_in_root(root, id, temperature);
            Ok(true)
        })
    }

    pub fn set_active_llm_extra_headers_json(value: &str) -> Result<()> {
        let headers = parse_extra_headers_json(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            root.providers
                .llm
                .entry(root.active.llm.clone())
                .or_default()
                .extraHeaders = (!headers.is_empty()).then_some(headers);
            Ok(true)
        })
    }

    /// 写入指定 LLM 渠道的额外请求头，不改变 active 渠道。
    pub fn set_llm_extra_headers_json_for_provider(id: &str, value: &str) -> Result<()> {
        let headers = parse_extra_headers_json(value)?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            set_llm_extra_headers_for_provider_in_root(root, id, headers);
            Ok(true)
        })
    }

    pub fn snapshot() -> CredentialsSnapshot {
        Self::snapshot_for_pipeline(true)
    }

    pub fn snapshot_for_pipeline(include_omni: bool) -> CredentialsSnapshot {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        credentials_snapshot(&root, include_omni)
    }

    pub(crate) fn configuration_snapshot(
        include_omni: bool,
    ) -> Result<CredentialConfigurationSnapshot> {
        let _guard = credentials_lock().lock();
        configuration_snapshot_with(include_omni, load_credentials_for_update)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::load_android_credentials_from_source_with_crypto;
    use super::{
        android_credentials_root_for_update, android_persistable_credentials, chunk_json_payload,
        credentials_cache, get_android_marketplace_token_at, load_android_credentials_from_path,
        load_android_credentials_from_path_with_crypto, load_credentials_into_cache_with,
        lookup_account, lookup_marketplace_github_token, lookup_omni_account,
        omni_extra_headers_json, omni_temperature_string, parse_extra_headers_json,
        parse_llm_temperature, reset_credentials_cache_for_tests,
        set_llm_extra_headers_for_provider_in_root, set_llm_temperature_for_provider_in_root,
        write_account, write_marketplace_github_token, write_omni_account, CredentialAccount,
        CredentialsVault, CredsAsrEntry, CredsLlmEntry, CredsRoot, MarketplaceGithubToken,
        KEYRING_CHUNK_MAX_UTF16_UNITS,
    };
    use anyhow::anyhow;
    use parking_lot::Mutex;
    use std::collections::HashMap;

    #[test]
    fn macos_credentials_read_one_item_after_legacy_chunk_migration() {
        use std::cell::RefCell;

        let mut root = CredsRoot::default();
        root.version = 2;
        write_account(
            &mut root,
            CredentialAccount::AsrApiKey,
            Some("fixture-key".repeat(200)),
        );
        let json = serde_json::to_string(&root).unwrap();
        let chunks = chunk_json_payload(&json);
        assert!(chunks.len() > 1);
        let manifest = super::CredsChunkManifest {
            openless_credentials_storage: "chunked".into(),
            version: 1,
            generation: Some("legacy-generation".into()),
            chunks: chunks.len(),
        };
        let mut entries = HashMap::new();
        entries.insert(
            super::KEYRING_CREDENTIALS_ACCOUNT.to_owned(),
            serde_json::to_string(&manifest).unwrap(),
        );
        for (index, chunk) in chunks.iter().enumerate() {
            entries.insert(
                super::chunk_account(Some("legacy-generation"), index),
                chunk.clone(),
            );
        }
        let entries = RefCell::new(entries);
        let reads = RefCell::new(Vec::new());
        let writes = RefCell::new(Vec::new());
        let read = |account: &str| {
            reads.borrow_mut().push(account.to_owned());
            Ok(entries.borrow().get(account).cloned())
        };
        let write = |account: &str, value: &str| {
            writes.borrow_mut().push(account.to_owned());
            entries
                .borrow_mut()
                .insert(account.to_owned(), value.to_owned());
            Ok(())
        };

        let loaded = super::load_keyring_credentials_with(read, write, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            lookup_account(&loaded, CredentialAccount::AsrApiKey),
            lookup_account(&root, CredentialAccount::AsrApiKey)
        );
        assert_eq!(
            *writes.borrow(),
            [super::KEYRING_SINGLE_CREDENTIALS_ACCOUNT]
        );
        assert!(
            super::read_chunk_manifest(
                entries
                    .borrow()
                    .get(super::KEYRING_CREDENTIALS_ACCOUNT)
                    .unwrap()
            )
            .is_some(),
            "the old manifest stays readable by a previous app version"
        );
        assert!(
            entries
                .borrow()
                .contains_key(&super::chunk_account(Some("legacy-generation"), 0)),
            "migration must leave legacy Keychain entries untouched"
        );

        reads.borrow_mut().clear();
        writes.borrow_mut().clear();
        let loaded = super::load_keyring_credentials_with(read, write, true)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, 2);
        assert_eq!(*reads.borrow(), [super::KEYRING_SINGLE_CREDENTIALS_ACCOUNT]);
        assert!(
            writes.borrow().is_empty(),
            "a migrated vault must not write at startup"
        );
    }

    #[test]
    fn macos_credentials_keep_readable_data_when_consolidation_is_denied() {
        let root = CredsRoot::default();
        let json = serde_json::to_string(&root).unwrap();
        let manifest = serde_json::to_string(&super::CredsChunkManifest {
            openless_credentials_storage: "chunked".into(),
            version: 1,
            generation: None,
            chunks: 1,
        })
        .unwrap();
        let loaded = super::load_keyring_credentials_with(
            |account| {
                if account == super::KEYRING_SINGLE_CREDENTIALS_ACCOUNT {
                    return Ok(None);
                }
                Ok(Some(if account == super::KEYRING_CREDENTIALS_ACCOUNT {
                    manifest.clone()
                } else {
                    json.clone()
                }))
            },
            |_, _| Err(anyhow!("fixture write denied")),
            true,
        )
        .unwrap();
        assert!(
            loaded.is_some(),
            "an optional format migration must not turn readable credentials into an error"
        );
    }

    #[test]
    fn macos_credentials_propagate_read_failures_without_migrating() {
        let missing = super::load_keyring_credentials_with(
            |_| Ok(None),
            |_, _| panic!("missing credentials must not be written"),
            true,
        )
        .unwrap();
        assert!(missing.is_none());
        let mut denied_reads = Vec::new();
        let denied = super::load_keyring_credentials_with(
            |account| {
                denied_reads.push(account.to_owned());
                Err(anyhow!("fixture access denied"))
            },
            |_, _| panic!("unreadable credentials must not be overwritten"),
            true,
        );
        assert!(denied.is_err());
        assert_eq!(
            denied_reads,
            [super::KEYRING_SINGLE_CREDENTIALS_ACCOUNT],
            "an unreadable v2 item must not fall back to stale v1 credentials"
        );
        let malformed = super::load_keyring_credentials_with(
            |_| Ok(Some("{}".into())),
            |_, _| panic!("malformed credentials must not be overwritten"),
            true,
        );
        assert!(malformed.is_err());
    }

    #[test]
    fn macos_credentials_never_migrate_an_incomplete_legacy_payload() {
        let manifest = serde_json::to_string(&super::CredsChunkManifest {
            openless_credentials_storage: "chunked".into(),
            version: 1,
            generation: None,
            chunks: 2,
        })
        .unwrap();
        let result = super::load_keyring_credentials_with(
            |account| match account {
                super::KEYRING_SINGLE_CREDENTIALS_ACCOUNT => Ok(None),
                super::KEYRING_CREDENTIALS_ACCOUNT => Ok(Some(manifest.clone())),
                "credentials.v1.chunk.0" => Ok(Some("{\"version\":1,".into())),
                _ => Err(anyhow!("fixture chunk access denied")),
            },
            |_, _| panic!("incomplete legacy data must not replace a vault"),
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn credential_configuration_snapshot_is_atomic_and_propagates_load_errors() {
        let denied =
            super::configuration_snapshot_with(false, || Err(anyhow!("fixture keychain locked")));
        assert!(
            denied.is_err(),
            "an unreadable vault is not an unconfigured ASR provider"
        );

        let mut root = CredsRoot::default();
        root.active.asr = "my-asr-channel".into();
        write_account(
            &mut root,
            CredentialAccount::AsrApiKey,
            Some("fixture-asr-key".into()),
        );
        root.providers
            .asr
            .get_mut("my-asr-channel")
            .unwrap()
            .channel
            .providerType = Some("bailian".into());
        root.active.llm = "my-llm-channel".into();
        write_account(
            &mut root,
            CredentialAccount::ArkApiKey,
            Some("fixture-llm-key".into()),
        );
        root.providers
            .llm
            .get_mut("my-llm-channel")
            .unwrap()
            .channel
            .providerType = Some("openai".into());
        let loaded = super::configuration_snapshot_with(false, || Ok(root)).unwrap();
        assert_eq!(loaded.active_asr_provider, "bailian");
        assert_eq!(loaded.active_llm_provider, "openai");
        assert_eq!(
            loaded.credentials.asr_api_key.as_deref(),
            Some("fixture-asr-key")
        );
        assert_eq!(
            loaded.credentials.ark_api_key.as_deref(),
            Some("fixture-llm-key")
        );
    }

    #[test]
    fn credential_payload_chunks_stay_under_windows_blob_limit() {
        let payload = format!(
            "{}{}{}",
            "a".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS + 25),
            "😀".repeat(20),
            "b".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS + 25)
        );
        let chunks = chunk_json_payload(&payload);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), payload);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= KEYRING_CHUNK_MAX_UTF16_UNITS));
    }

    #[test]
    fn llm_protocol_options_survive_vault_reload_and_core_projection() {
        use openless_core::llm_protocol::*;
        let mut root = CredsRoot::default();
        let entry = root.providers.llm.entry("channel-b".into()).or_default();
        let values = ["messages", "budget", "8192", "2048"];
        for (account, value) in CONFIG_ACCOUNTS.into_iter().zip(values) {
            *entry.protocol_option(account).unwrap() = Some(value.into());
        }
        assert!(!entry.has_no_content());
        let serialized = serde_json::to_string(&root).unwrap();
        let mut restored: CredsRoot = serde_json::from_str(&serialized).unwrap();
        for (account, value) in CONFIG_ACCOUNTS.into_iter().zip(values) {
            assert_eq!(
                restored
                    .providers
                    .llm
                    .get_mut("channel-b")
                    .unwrap()
                    .protocol_option(account)
                    .unwrap()
                    .as_deref(),
                Some(value)
            );
        }
        let decoded =
            openless_core::credentials_legacy::decode_legacy_credentials(&serialized).unwrap();
        for (account, value) in CONFIG_ACCOUNTS.into_iter().zip(values) {
            assert!(decoded
                .secrets
                .iter()
                .any(
                    |(key, secret)| key.provider_id.as_deref() == Some("channel-b")
                        && key.account == account
                        && secret.expose_secret() == value
                ));
        }
        let old: CredsLlmEntry = serde_json::from_str(r#"{"apiKey":"old-key"}"#).unwrap();
        assert!(old.requestFormat.is_none());
        assert!(!restored.providers.llm.contains_key("channel-a"));
    }

    #[test]
    fn omni_accounts_route_to_omni_namespace_only() {
        // 多模态（Omni）凭据必须与 LLM/ASR 命名空间完全隔离（issue #902）：
        // 写 omni 槽位不影响 ark 槽位；切换 omni active provider 后读到的是
        // 该 provider 自己的 entry，而不是别的 provider 的残留值。
        let mut root = CredsRoot::default();
        root.active.llm = "ark".into();
        root.active.asr = "volcengine".into();
        root.omni.active = "openai".into();

        write_account(
            &mut root,
            CredentialAccount::OmniApiKey,
            Some("omni-key".into()),
        );
        write_account(
            &mut root,
            CredentialAccount::OmniEndpoint,
            Some("https://api.openai.com/v1".into()),
        );
        write_account(
            &mut root,
            CredentialAccount::OmniModel,
            Some("gpt-4o-audio-preview".into()),
        );

        assert_eq!(
            lookup_account(&root, CredentialAccount::OmniApiKey).as_deref(),
            Some("omni-key")
        );
        // 传统 LLM / ASR 槽位必须保持为空。
        assert_eq!(lookup_account(&root, CredentialAccount::ArkApiKey), None);
        assert_eq!(lookup_account(&root, CredentialAccount::AsrApiKey), None);

        // 切到另一个 omni provider：读不到 openai 的 entry（per-provider 隔离）。
        root.omni.active = "custom".into();
        assert_eq!(lookup_account(&root, CredentialAccount::OmniApiKey), None);
        root.omni.active = "openai".into();
        assert_eq!(
            lookup_account(&root, CredentialAccount::OmniModel).as_deref(),
            Some("gpt-4o-audio-preview")
        );

        // 显式 provider id 的运行时读取不得依赖或改写 active provider。
        write_omni_account(
            &mut root,
            "custom",
            CredentialAccount::OmniApiKey,
            Some("custom-key".into()),
        )
        .unwrap();
        write_omni_account(
            &mut root,
            "custom",
            CredentialAccount::OmniEndpoint,
            Some("https://custom.example.com/v1".into()),
        )
        .unwrap();
        write_omni_account(
            &mut root,
            "custom",
            CredentialAccount::OmniModel,
            Some("custom-model".into()),
        )
        .unwrap();
        let custom = root.omni.providers.get_mut("custom").unwrap();
        custom.temperature = Some(0.4);
        custom.extraHeaders = Some(HashMap::from([("x-tenant".into(), "custom".into())]));

        assert_eq!(root.omni.active, "openai");
        assert_eq!(
            lookup_omni_account(&root, "custom", CredentialAccount::OmniApiKey)
                .unwrap()
                .as_deref(),
            Some("custom-key")
        );
        assert_eq!(
            lookup_omni_account(&root, "custom", CredentialAccount::OmniEndpoint)
                .unwrap()
                .as_deref(),
            Some("https://custom.example.com/v1")
        );
        assert_eq!(
            lookup_omni_account(&root, "custom", CredentialAccount::OmniModel)
                .unwrap()
                .as_deref(),
            Some("custom-model")
        );
        assert_eq!(
            omni_temperature_string(&root, "custom").as_deref(),
            Some("0.4")
        );
        assert_eq!(
            omni_extra_headers_json(&root, "custom").unwrap().as_deref(),
            Some(r#"{"x-tenant":"custom"}"#)
        );
    }

    #[test]
    fn traditional_pipeline_snapshot_does_not_extract_omni_secrets() {
        let mut root = CredsRoot::default();
        root.omni.active = "custom".into();
        root.omni.providers.insert(
            "custom".into(),
            super::CredsOmniEntry {
                apiKey: Some("omni-secret".into()),
                baseURL: Some("https://omni.example/v1".into()),
                model: Some("omni-model".into()),
                ..Default::default()
            },
        );

        let hidden = super::credentials_snapshot(&root, false);
        assert_eq!(hidden.omni_api_key, None);
        assert_eq!(hidden.omni_endpoint, None);
        assert_eq!(hidden.omni_model, None);

        let visible = super::credentials_snapshot(&root, true);
        assert_eq!(visible.omni_api_key.as_deref(), Some("omni-secret"));
        assert_eq!(visible.omni_model.as_deref(), Some("omni-model"));
    }

    #[test]
    fn parse_extra_headers_json_rejects_reserved_header_names() {
        for name in [
            "Authorization",
            "content-type",
            "ACCEPT",
            "Host",
            "Content-Length",
        ] {
            let value = format!(r#"{{"{name}":"secret"}}"#);
            let err = parse_extra_headers_json(&value).unwrap_err().to_string();
            assert!(
                err.contains("reserved extra header name"),
                "unexpected error for {name}: {err}"
            );
        }
    }

    #[test]
    fn marketplace_github_token_uses_the_credentials_payload_not_provider_accounts() {
        let mut root = CredsRoot::default();
        assert_eq!(lookup_marketplace_github_token(&root), None);

        write_marketplace_github_token(&mut root, Some("gho_vault_only".to_string()));

        assert_eq!(
            lookup_marketplace_github_token(&root).as_deref(),
            Some("gho_vault_only")
        );
        assert!(root.providers.asr.is_empty());
        assert!(root.providers.llm.is_empty());
    }

    #[test]
    fn asr_advanced_config_round_trips_through_provider_entry() {
        let mut root = CredsRoot::default();
        root.active.asr = "openai-compatible".into();
        write_account(
            &mut root,
            CredentialAccount::AsrAdvancedConfig,
            Some(r#"{"verboseJson":true,"chunkDurationMs":30000}"#.into()),
        );
        assert_eq!(
            lookup_account(&root, CredentialAccount::AsrAdvancedConfig).as_deref(),
            Some(r#"{"verboseJson":true,"chunkDurationMs":30000}"#)
        );

        // 清空即移除该字段，且只影响对应 provider 的 entry。
        write_account(&mut root, CredentialAccount::AsrAdvancedConfig, None);
        assert_eq!(
            lookup_account(&root, CredentialAccount::AsrAdvancedConfig),
            None
        );
        assert!(root.providers.asr["openai-compatible"]
            .advancedConfig
            .is_none());

        // 旧条目（无 advancedConfig 字段）反序列化为 None，不破坏既有数据。
        let legacy: CredsAsrEntry = serde_json::from_str(r#"{"apiKey":"k"}"#).unwrap();
        assert!(legacy.advancedConfig.is_none());
        assert!(!legacy.is_empty());
    }

    #[test]
    fn legacy_credentials_payload_without_marketplace_token_remains_readable() {
        let root: CredsRoot = serde_json::from_str(r#"{"version":1}"#)
            .expect("pre-marketplace credentials should remain compatible");

        assert_eq!(lookup_marketplace_github_token(&root), None);
    }

    #[test]
    fn marketplace_logout_removes_only_the_marketplace_token() {
        let mut root = CredsRoot::default();
        root.active.llm = "configured-provider".to_string();
        write_marketplace_github_token(&mut root, Some("gho_remove_me".to_string()));

        write_marketplace_github_token(&mut root, None);

        assert_eq!(lookup_marketplace_github_token(&root), None);
        assert_eq!(root.active.llm, "configured-provider");
    }

    #[test]
    fn marketplace_token_is_absent_from_serialized_preferences() {
        let token = "gho_must_not_enter_preferences";
        let mut root = CredsRoot::default();
        write_marketplace_github_token(&mut root, Some(token.to_string()));

        let credentials_json = serde_json::to_string(&root).expect("credentials should serialize");
        let preferences_json = serde_json::to_string(&crate::types::UserPreferences::default())
            .expect("preferences should serialize");

        assert!(credentials_json.contains(token));
        assert!(!preferences_json.contains(token));
        assert!(!preferences_json.contains("githubAccessToken"));
        assert!(!format!("{root:?}").contains(token));
    }

    #[test]
    fn android_persistable_credentials_never_contains_marketplace_token_or_account() {
        let token = "gho_android_memory_only";
        let mut root = CredsRoot::default();
        write_marketplace_github_token(&mut root, Some(token.to_string()));

        let persisted = serde_json::to_string(&android_persistable_credentials(&root))
            .expect("android credential payload should serialize");

        assert!(!persisted.contains(token));
        assert!(!persisted.contains("githubAccessToken"));
        assert!(!persisted.contains("marketplace"));
    }

    #[test]
    fn android_legacy_envelope_is_atomically_scrubbed_before_load_returns() {
        use base64::Engine;

        let token = "gho_legacy_android_secret";
        let mut root = CredsRoot::default();
        write_marketplace_github_token(&mut root, Some(token.to_string()));
        let raw = serde_json::to_vec(&root).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        let dir = std::env::temp_dir().join(format!(
            "openless-android-credential-scrub-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials.enc.json");
        std::fs::write(&path, encoded).unwrap();
        let mut crypto = super::super::android_credentials::TestCrypto::default();

        let loaded = load_android_credentials_from_path_with_crypto(&path, &mut crypto)
            .unwrap()
            .expect("credential envelope should load");
        let disk = std::fs::read_to_string(&path).unwrap();
        let loaded_again = load_android_credentials_from_path_with_crypto(&path, &mut crypto)
            .unwrap()
            .expect("migrated credential envelope should load");

        assert_eq!(lookup_marketplace_github_token(&loaded), None);
        assert_eq!(lookup_marketplace_github_token(&loaded_again), None);
        assert!(disk.starts_with('{'));
        assert!(disk.contains("openless-android-credentials"));
        assert!(!disk.contains(token));
        assert!(!disk.contains("githubAccessToken"));
        assert!(!disk.contains("marketplace"));
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn android_legacy_root_migrates_to_private_destination_and_is_erased() {
        use base64::Engine;

        let root_dir = std::env::temp_dir().join(format!(
            "openless-android-cross-root-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let legacy_path = root_dir.join("legacy").join("credentials.enc.json");
        let destination_path = root_dir.join("files").join("credentials.enc.json");
        let plaintext = br#"{"version":1,"providers":{"llm":{"ark":{"apiKey":"sk-migrate"}}}}"#;
        std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        std::fs::write(
            &legacy_path,
            base64::engine::general_purpose::STANDARD.encode(plaintext),
        )
        .unwrap();
        let mut crypto = super::super::android_credentials::TestCrypto::default();

        assert!(load_android_credentials_from_source_with_crypto(
            &legacy_path,
            &destination_path,
            &mut crypto,
        )
        .unwrap()
        .is_some());
        assert!(!legacy_path.exists());
        assert!(std::fs::read_to_string(&destination_path)
            .unwrap()
            .contains("openless-android-credentials"));
        assert!(
            load_android_credentials_from_path_with_crypto(&destination_path, &mut crypto)
                .unwrap()
                .is_some()
        );
        std::fs::remove_dir_all(root_dir).unwrap();
    }

    fn write_legacy_android_envelope(path: &std::path::Path, token: &str) {
        use base64::Engine;

        let mut root = CredsRoot::default();
        write_marketplace_github_token(&mut root, Some(token.to_string()));
        let raw = serde_json::to_vec(&root).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, encoded).unwrap();
    }

    fn assert_android_secret_unrecoverable(path: &std::path::Path, token: &str) {
        use base64::Engine;

        for candidate in [
            path.to_path_buf(),
            path.with_extension("json.tmp"),
            path.with_extension("legacy.tmp"),
        ] {
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            assert!(
                !String::from_utf8_lossy(&bytes).contains(token),
                "raw secret remained in {}",
                candidate.display()
            );
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&bytes) {
                assert!(
                    !String::from_utf8_lossy(&decoded).contains(token),
                    "base64 secret remained in {}",
                    candidate.display()
                );
            }
        }
    }

    #[test]
    fn android_bearer_is_scrubbed_before_failed_keystore_migration_returns() {
        use base64::Engine;

        let token = "gho_must_be_unrecoverable";
        let provider_secret = "sk_generic_credential_survives";
        let raw = format!(
            r#"{{"version":1,"providers":{{"llm":{{"ark":{{"apiKey":"{provider_secret}"}}}}}},"marketplace":{{"githubAccessToken":"{token}"}}}}"#,
        );
        let dir = std::env::temp_dir().join(format!(
            "openless-android-bearer-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let path = dir.join("credentials.enc.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()),
        )
        .unwrap();
        let mut crypto = super::super::android_credentials::TestCrypto::default();
        crypto.fail_next_seal =
            Some(super::super::android_credentials::CryptoErrorKind::TemporarilyUnavailable);

        assert!(load_android_credentials_from_path_with_crypto(&path, &mut crypto).is_err());
        let sanitized = std::fs::read(&path).unwrap();
        let sanitized = base64::engine::general_purpose::STANDARD
            .decode(sanitized)
            .unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();
        assert!(!sanitized.contains(token));
        assert!(!sanitized.contains("githubAccessToken"));
        assert!(sanitized.contains(provider_secret));

        let loaded = load_android_credentials_from_path_with_crypto(&path, &mut crypto)
            .unwrap()
            .expect("sanitized legacy credentials should remain retryable");
        assert_eq!(lookup_marketplace_github_token(&loaded), None);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("openless-android-credentials"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn android_real_getter_scrubs_legacy_disk_token_and_retries_failure() {
        let dir =
            std::env::temp_dir().join(format!("openless-android-getter-{}", uuid::Uuid::new_v4()));
        let path = dir.join("credentials.enc.json");
        std::fs::create_dir_all(&path).unwrap();
        let completed = Mutex::new(false);
        let memory = Mutex::new(Some(MarketplaceGithubToken(
            "gho_process_memory".to_string(),
        )));

        assert!(get_android_marketplace_token_at(&path, &completed, &memory).is_err());
        assert!(!*completed.lock(), "failed scrub must remain retryable");

        std::fs::remove_dir(&path).unwrap();
        write_legacy_android_envelope(&path, "gho_legacy_getter_secret");
        let token = get_android_marketplace_token_at(&path, &completed, &memory).unwrap();

        assert_eq!(token.as_deref(), Some("gho_process_memory"));
        assert!(*completed.lock());
        assert_android_secret_unrecoverable(&path, "gho_legacy_getter_secret");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn android_startup_failure_does_not_cache_default_or_suppress_retry() {
        reset_credentials_cache_for_tests();
        let first = load_credentials_into_cache_with(|| {
            Err(anyhow!("injected startup scrub failure")
                .context("read Android credential envelope"))
        });
        assert!(lookup_marketplace_github_token(&first).is_none());
        assert!(credentials_cache().lock().is_none());
        let first_error =
            CredentialsVault::last_read_error().expect("vault error should be recorded");
        assert!(
            first_error.contains("injected startup scrub failure"),
            "error chain should include the inner cause, got {first_error}"
        );
        assert!(
            first_error.contains("read Android credential envelope"),
            "error chain should include the outer context, got {first_error}"
        );

        let dir =
            std::env::temp_dir().join(format!("openless-android-startup-{}", uuid::Uuid::new_v4()));
        let path = dir.join("credentials.enc.json");
        write_legacy_android_envelope(&path, "gho_legacy_startup_secret");
        let second = load_credentials_into_cache_with(|| load_android_credentials_from_path(&path));

        assert!(lookup_marketplace_github_token(&second).is_none());
        assert!(credentials_cache().lock().is_some());
        assert!(
            CredentialsVault::last_read_error().is_none(),
            "successful read must clear the last vault error"
        );
        assert_android_secret_unrecoverable(&path, "gho_legacy_startup_secret");
        *credentials_cache().lock() = Some(CredsRoot::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_payload_errors_never_echo_private_field_values() {
        reset_credentials_cache_for_tests();
        let error = serde_json::from_str::<CredsRoot>(r#"{"version":"private-canary-secret"}"#)
            .unwrap_err();
        super::record_vault_read_failure(&anyhow::Error::new(error));
        let recorded = CredentialsVault::last_read_error().unwrap();
        assert_eq!(recorded, "credential payload could not be decoded");
        assert!(!recorded.contains("private-canary-secret"));
        super::clear_vault_read_error();
    }

    #[test]
    fn android_for_update_path_retries_and_does_not_cache_on_envelope_error() {
        reset_credentials_cache_for_tests();
        let err = android_credentials_root_for_update(|| {
            Err(anyhow!("temporarily unavailable")
                .context("Android credential authentication or key operation failed")
                .context("read Android credential envelope"))
        })
        .expect_err("mutations must not receive a default root to persist");
        assert!(credentials_cache().lock().is_none());
        let error = CredentialsVault::last_read_error().expect("vault error should be recorded");
        assert!(
            error.contains("temporarily unavailable"),
            "error chain should include the Keystore kind, got {error}"
        );
        let chain = format!("{err:#}");
        assert!(
            chain.contains("temporarily unavailable"),
            "returned error should include the Keystore kind, got {chain}"
        );
    }

    #[test]
    fn parse_llm_temperature_accepts_empty_and_valid_range() {
        assert_eq!(parse_llm_temperature("").unwrap(), None);
        assert_eq!(parse_llm_temperature(" 0.3 ").unwrap(), Some(0.3));
        assert_eq!(parse_llm_temperature("2").unwrap(), Some(2.0));
    }

    #[test]
    fn parse_llm_temperature_rejects_invalid_values() {
        for value in ["abc", "-0.1", "2.1", "NaN", "inf"] {
            assert!(
                parse_llm_temperature(value).is_err(),
                "{value} should be rejected"
            );
        }
    }

    #[test]
    fn llm_extra_headers_and_temperature_writes_stay_on_the_explicit_channel() {
        let mut root = CredsRoot::default();
        root.active.llm = "channel-a".to_string();
        root.providers
            .llm
            .insert("channel-a".to_string(), CredsLlmEntry::default());
        set_llm_temperature_for_provider_in_root(&mut root, "channel-b", Some(0.7));
        set_llm_extra_headers_for_provider_in_root(
            &mut root,
            "channel-b",
            HashMap::from([(String::from("x-tenant"), String::from("b"))]),
        );

        assert_eq!(root.active.llm, "channel-a");
        assert_eq!(root.providers.llm["channel-b"].temperature, Some(0.7));
        assert_eq!(
            root.providers.llm["channel-b"].extraHeaders,
            Some(HashMap::from([(
                String::from("x-tenant"),
                String::from("b")
            )]))
        );
        assert!(root.providers.llm["channel-a"].temperature.is_none());
        assert!(root.providers.llm["channel-a"].extraHeaders.is_none());
    }

    #[test]
    fn active_llm_temperature_ignores_invalid_persisted_values() {
        for temperature in [-0.1, 2.5] {
            let mut root = CredsRoot::default();
            root.providers.llm.insert(
                root.active.llm.clone(),
                super::CredsLlmEntry {
                    temperature: Some(temperature),
                    ..Default::default()
                },
            );

            assert_eq!(super::active_llm_temperature(&root), None);
            assert_eq!(super::active_llm_temperature_string(&root), None);
        }

        let mut root = CredsRoot::default();
        root.providers.llm.insert(
            root.active.llm.clone(),
            super::CredsLlmEntry {
                temperature: Some(0.7),
                ..Default::default()
            },
        );
        assert_eq!(super::active_llm_temperature(&root), Some(0.7));
        assert_eq!(
            super::active_llm_temperature_string(&root).as_deref(),
            Some("0.7")
        );
    }

    // ---- 渠道卡片（v1 → v2）----

    fn v1_root_with_two_asr_providers() -> CredsRoot {
        let mut root = CredsRoot::default();
        root.active.asr = "volcengine".into();
        root.providers.asr.insert(
            "volcengine".into(),
            CredsAsrEntry {
                appKey: Some("vk".into()),
                ..Default::default()
            },
        );
        root.providers.asr.insert(
            "groq".into(),
            CredsAsrEntry {
                apiKey: Some("gk".into()),
                ..Default::default()
            },
        );
        root
    }

    #[test]
    fn migration_keeps_preset_ids_as_channel_ids_and_puts_active_first() {
        let mut root = v1_root_with_two_asr_providers();
        assert!(super::migrate_channels(&mut root));

        // id 沿用原 preset id —— 老用户的 map key 一个字节都不变。
        let volcengine = root
            .providers
            .asr
            .get("volcengine")
            .expect("volcengine kept");
        let groq = root.providers.asr.get("groq").expect("groq kept");

        assert_eq!(
            volcengine.channel.providerType.as_deref(),
            Some("volcengine")
        );
        assert_eq!(groq.channel.providerType.as_deref(), Some("groq"));
        // 原 active 排第一。
        assert_eq!(volcengine.channel.order, Some(0));
        assert_eq!(groq.channel.order, Some(1));
        // v1 老数据一律视为启用。
        assert!(volcengine.channel.enabled);
        assert!(groq.channel.enabled);
    }

    #[test]
    fn migration_is_idempotent() {
        let mut root = v1_root_with_two_asr_providers();
        assert!(super::migrate_channels(&mut root));
        let after_first = serde_json::to_string(&root).expect("encode");

        // 第二次必须无改动（返回 false）且结果逐字节一致。
        assert!(!super::migrate_channels(&mut root));
        assert_eq!(serde_json::to_string(&root).expect("encode"), after_first);
    }

    #[test]
    fn migrated_credentials_still_resolve_through_lookup_account() {
        let mut root = v1_root_with_two_asr_providers();
        super::migrate_channels(&mut root);

        // 迁移后凭据读取行为不变 —— 这是老用户升级不炸的底线。
        assert_eq!(
            lookup_account(&root, CredentialAccount::VolcengineAppKey).as_deref(),
            Some("vk")
        );
    }

    /// `active` 指向一个**不存在的 entry** 是真实会发生的：前端 prefs 里的
    /// `activeAsrProvider` 与凭据库里的 `active.asr` 是两份数据，历史上可能不同步。
    /// 此时迁移只能退而求其次选一张，但**绝不允许动任何凭据** —— 用户的 key 必须原样
    /// 留在各自的 entry 里，用户把想用的那张拖回第一位就能恢复。
    #[test]
    fn migration_never_touches_credentials_even_when_active_points_at_a_missing_entry() {
        let mut root = CredsRoot::default();
        root.active.asr = "stepfun".into(); // 凭据库里并没有这个 entry
        root.providers.asr.insert(
            "volcengine".into(),
            CredsAsrEntry {
                appKey: Some("vk".into()),
                accessKey: Some("ak".into()),
                ..Default::default()
            },
        );
        root.providers.asr.insert(
            "groq".into(),
            CredsAsrEntry {
                apiKey: Some("gk".into()),
                ..Default::default()
            },
        );

        super::migrate_channels(&mut root);

        // 迁移只写 providerType / order，凭据一个字节都不动。
        assert_eq!(
            root.providers
                .asr
                .get("volcengine")
                .unwrap()
                .appKey
                .as_deref(),
            Some("vk")
        );
        assert_eq!(
            root.providers
                .asr
                .get("volcengine")
                .unwrap()
                .accessKey
                .as_deref(),
            Some("ak")
        );
        assert_eq!(
            root.providers.asr.get("groq").unwrap().apiKey.as_deref(),
            Some("gk")
        );
        // 两张卡片都还在，用户可以自己拖回想要的那张。
        assert_eq!(root.providers.asr.len(), 2);
        // active 退到一个真实存在的渠道上，而不是继续指向空气。
        assert!(root.providers.asr.contains_key(&root.active.asr));
    }

    #[test]
    fn migration_prefers_a_configured_channel_over_alphabetical_order() {
        // active 指向一个不存在的 entry；`aaa-empty` 字母序更靠前但一个字都没填，
        // `volcengine` 才是用户真正配好的那张。纯字母序会让用户升级后看到"未配置"。
        let mut root = CredsRoot::default();
        root.active.asr = "stepfun".into();
        root.providers.asr.insert(
            "aaa-empty".into(),
            CredsAsrEntry {
                ..Default::default()
            },
        );
        root.providers.asr.insert(
            "volcengine".into(),
            CredsAsrEntry {
                appKey: Some("vk".into()),
                accessKey: Some("ak".into()),
                resourceId: Some("rid".into()),
                ..Default::default()
            },
        );

        super::migrate_channels(&mut root);

        assert_eq!(root.active.asr, "volcengine");
        // 凭据确实能通过正常读取路径拿到 —— 也就是 UI 上会显示"已配置"。
        assert_eq!(
            lookup_account(&root, CredentialAccount::VolcengineAppKey).as_deref(),
            Some("vk")
        );
    }

    #[test]
    fn freshly_added_channel_survives_clean_credentials() {
        let mut root = CredsRoot::default();
        // 刚点「添加渠道」、名字取好了但还没填 key。
        root.providers.asr.insert(
            "chan-uuid".into(),
            CredsAsrEntry {
                channel: super::ChannelMeta {
                    providerType: Some("groq".into()),
                    order: Some(0),
                    enabled: true,
                    lastTest: None,
                },
                displayName: Some("Groq-备用".into()),
                ..Default::default()
            },
        );

        let cleaned = super::clean_credentials(&root);
        assert!(
            cleaned.providers.asr.contains_key("chan-uuid"),
            "空 key 的新建渠道被 clean_credentials 静默删掉了"
        );
    }

    #[test]
    fn v1_payload_without_channel_fields_still_deserializes() {
        // flatten 的 ChannelMeta 不能破坏老 payload 的反序列化。
        let v1 = r#"{
            "version": 1,
            "active": { "asr": "volcengine", "llm": "ark" },
            "providers": {
                "asr": { "volcengine": { "appKey": "vk", "accessKey": "ak" } },
                "llm": { "ark": { "apiKey": "sk", "model": "deepseek-v3-2" } }
            }
        }"#;
        let root: CredsRoot = serde_json::from_str(v1).expect("v1 payload must still parse");
        assert_eq!(
            root.providers
                .asr
                .get("volcengine")
                .unwrap()
                .appKey
                .as_deref(),
            Some("vk")
        );
        // 缺省即启用，且尚未渠道化。
        let entry = root.providers.asr.get("volcengine").unwrap();
        assert!(entry.channel.enabled);
        assert_eq!(entry.channel.providerType, None);
        // 未迁移时 providerType 回落到 map key。
        assert_eq!(
            super::channel_provider_type("volcengine", entry),
            "volcengine"
        );
    }

    #[test]
    fn metadata_projection_preserves_remaining_secrets_and_removes_deleted_channels() {
        let mut root = CredsRoot::default();
        root.metadata_revision = 4;
        for (id, order, key) in [("deepseek", 0, "key-a"), ("deepseek-2", 1, "key-b")] {
            root.providers.llm.insert(
                id.into(),
                CredsLlmEntry {
                    channel: super::ChannelMeta {
                        providerType: Some("deepseek".into()),
                        order: Some(order),
                        enabled: true,
                        lastTest: None,
                    },
                    apiKey: Some(key.into()),
                    ..Default::default()
                },
            );
        }
        root.active.llm = "deepseek".into();

        let mut metadata = super::credential_metadata(&root);
        metadata
            .apply_channel_mutation(
                openless_core::ChannelMutation::Delete {
                    kind: openless_core::ChannelKind::Llm,
                    id: "deepseek".into(),
                },
                |_| false,
            )
            .unwrap();
        super::apply_credential_metadata(&mut root, metadata).unwrap();

        assert!(!root.providers.llm.contains_key("deepseek"));
        assert_eq!(
            root.providers
                .llm
                .get("deepseek-2")
                .and_then(|entry| entry.apiKey.as_deref()),
            Some("key-b")
        );
        assert_eq!(root.active.llm, "deepseek-2");
        assert_eq!(root.metadata_revision, 5);
    }

    #[test]
    fn stale_metadata_revision_cannot_overwrite_newer_vault_contents() {
        let mut root = CredsRoot {
            metadata_revision: 3,
            ..CredsRoot::default()
        };
        let stale = openless_core::CredentialMetadata::from_parts(
            Vec::new(),
            Vec::new(),
            "",
            "",
            "custom",
            3,
        );

        assert!(super::apply_credential_metadata(&mut root, stale).is_err());
        assert_eq!(root.metadata_revision, 3);
    }

    #[test]
    fn provider_type_is_independent_of_channel_id_for_multi_key_setups() {
        // 同一家两把 key：map key 是 uuid，providerType 都指向 deepseek。
        let mut root = CredsRoot::default();
        for (id, order) in [("uuid-a", 0u32), ("uuid-b", 1)] {
            root.providers.llm.insert(
                id.into(),
                super::CredsLlmEntry {
                    channel: super::ChannelMeta {
                        providerType: Some("deepseek".into()),
                        order: Some(order),
                        enabled: true,
                        lastTest: None,
                    },
                    apiKey: Some(format!("sk-{id}")),
                    ..Default::default()
                },
            );
        }
        root.active.llm = "uuid-a".into();
        assert_eq!(root.active.llm, "uuid-a");

        let entry = root.providers.llm.get(&root.active.llm).unwrap();
        // 协议路由拿到的必须是厂商 id，不是 uuid。
        assert_eq!(
            super::channel_provider_type(&root.active.llm, entry),
            "deepseek"
        );
        assert_eq!(
            lookup_account(&root, CredentialAccount::ArkApiKey).as_deref(),
            Some("sk-uuid-a")
        );
    }
}

#[cfg(test)]
mod encrypted_sync_tests {
    use super::*;
    use openless_core::credentials::{
        ChangeOrigin, SyncNamespace, SyncSecretAccount, SyncWriteGate,
    };
    use std::cell::{Cell, RefCell};

    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("openless-vault-sync-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn root_with_secret(value: &str) -> CredsRoot {
        let mut root = CredsRoot::default();
        root.version = 2;
        root.active.asr = "stable-channel".into();
        root.providers.asr.insert(
            "stable-channel".into(),
            CredsAsrEntry {
                channel: ChannelMeta {
                    providerType: Some("openai-compatible".into()),
                    order: Some(0),
                    enabled: true,
                    lastTest: None,
                },
                apiKey: Some(value.into()),
                ..Default::default()
            },
        );
        root
    }
    fn installed_chunks(json: &str, generation: Option<&str>) -> HashMap<String, String> {
        let chunks = chunk_json_payload(json);
        let manifest = CredsChunkManifest {
            openless_credentials_storage: "chunked".into(),
            version: 1,
            generation: generation.map(str::to_string),
            chunks: chunks.len(),
        };
        let mut values = HashMap::from([(
            KEYRING_CREDENTIALS_ACCOUNT.into(),
            serde_json::to_string(&manifest).unwrap(),
        )]);
        for (i, part) in chunks.into_iter().enumerate() {
            values.insert(chunk_account(generation, i), part);
        }
        values
    }
    fn read_root(values: &RefCell<HashMap<String, String>>) -> CredsRoot {
        load_keyring_credentials_with(
            |key| Ok(values.borrow().get(key).cloned()),
            |_, _| panic!("reader must not write"),
            false,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn sync_gate_binding_is_lazy_and_preserves_pending_recovery() {
        let temp = Temporary::new();
        let path = temp.0.join("generation.json");
        let gate = SyncWriteGate::open(path.clone()).unwrap();
        drop(gate.begin_mutation().unwrap());
        let before = std::fs::read(&path).unwrap();
        let mut installed = None;
        install_sync_write_gate(&mut installed, gate.clone()).unwrap();
        install_sync_write_gate(&mut installed, gate.clone()).unwrap();
        assert!(std::sync::Arc::ptr_eq(installed.as_ref().unwrap(), &gate));
        assert!(gate.recovery_required().unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let other = SyncWriteGate::open(temp.0.join("other.json")).unwrap();
        assert!(install_sync_write_gate(&mut installed, other).is_err());
        assert!(std::sync::Arc::ptr_eq(installed.as_ref().unwrap(), &gate));
    }

    #[test]
    fn readonly_warm_cache_retries_denial_and_only_real_read_clears_failure() {
        reset_credentials_cache_for_tests();
        let failure = load_credentials_readonly_into_cache_with(|| {
            Err(anyhow::anyhow!("fixture native access denied"))
        })
        .unwrap_err();
        assert!(failure.to_string().contains("fixture native access denied"));
        assert!(credentials_cache().lock().is_none());
        let recorded = CredentialsVault::last_read_error().unwrap();

        // A cache hit itself is not a successful OS read. Seed only the cache
        // slot, without the successful-load/save helper's error-latch update.
        *credentials_cache().lock() = Some(root_with_secret("fixture-cached"));
        let cached =
            load_credentials_readonly_into_cache_with(|| panic!("must use cache")).unwrap();
        assert_eq!(
            cached.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("fixture-cached")
        );
        assert_eq!(
            CredentialsVault::last_read_error().as_deref(),
            Some(recorded.as_str())
        );

        *credentials_cache().lock() = None;
        let loaded = load_credentials_readonly_into_cache_with(|| {
            Ok(root_with_secret("fixture-retry-succeeded"))
        })
        .unwrap();
        assert_eq!(
            loaded.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("fixture-retry-succeeded")
        );
        assert!(credentials_cache().lock().is_some());
        assert!(CredentialsVault::last_read_error().is_none());
        reset_credentials_cache_for_tests();
    }

    #[test]
    fn bound_cold_read_preserves_legacy_file_until_a_real_save() {
        let temp = Temporary::new();
        let path = temp.0.join("legacy.json");
        let mut legacy = root_with_secret("fixture-legacy-provider");
        legacy.version = 1;
        legacy.marketplace.githubAccessToken =
            Some(MarketplaceGithubToken("fixture-local-oauth".into()));
        let original = serde_json::to_vec(&legacy).unwrap();
        std::fs::write(&path, &original).unwrap();
        let loaded = load_credentials_for_sync_binding_with(
            true,
            || {
                load_desktop_credentials_readonly_with(
                    |_| Ok(None),
                    || decode_single_credentials(&std::fs::read_to_string(&path)?).map(Some),
                    true,
                )
            },
            || panic!("bound read must not call the migration loader"),
        )
        .unwrap();
        assert_eq!(
            loaded.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("fixture-legacy-provider")
        );
        assert_eq!(
            lookup_marketplace_github_token(&loaded).as_deref(),
            Some("fixture-local-oauth")
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn recovery_binding_uses_only_readers_and_preserves_the_pending_gate() {
        let temp = Temporary::new();
        let path = temp.0.join("generation.json");
        let gate = SyncWriteGate::open(path.clone()).unwrap();
        drop(gate.begin_mutation().unwrap());
        let before = std::fs::read(&path).unwrap();
        let json = serde_json::to_string(&root_with_secret("fixture-source")).unwrap();
        let chunks = installed_chunks(&json, Some("prior-generation"));
        let root = load_credentials_for_sync_binding_with(
            gate.recovery_required().unwrap(),
            || {
                load_desktop_credentials_readonly_with(
                    |account| Ok(chunks.get(account).cloned()),
                    || panic!("complete chunks must not probe a legacy file"),
                    true,
                )
            },
            || panic!("pending startup must not call the migration loader"),
        )
        .unwrap();
        assert_eq!(
            root.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("fixture-source")
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let error = load_credentials_for_sync_binding_with(
            gate.recovery_required().unwrap(),
            || {
                load_desktop_credentials_readonly_with(
                    |_| Err(anyhow::anyhow!("fixture native access denied")),
                    || panic!("native errors must not fall back"),
                    true,
                )
            },
            || panic!("must not migrate after denial"),
        );
        assert!(error.is_err());
        assert!(gate.recovery_required().unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let single = load_desktop_credentials_readonly_with(
            |account| {
                assert_eq!(account, KEYRING_SINGLE_CREDENTIALS_ACCOUNT);
                Ok(Some(json.clone()))
            },
            || panic!("v2 has priority"),
            true,
        )
        .unwrap();
        assert_eq!(
            single.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("fixture-source")
        );
        assert!(load_desktop_credentials_readonly_with(
            |_| Ok(Some("{}".into())),
            || panic!("corrupt source must not fall back"),
            true
        )
        .is_err());
        assert!(load_desktop_credentials_readonly_with(
            |_| Ok(None),
            || Err(anyhow::anyhow!("unreadable legacy source")),
            false
        )
        .is_err());
        let legacy = load_desktop_credentials_readonly_with(
            |account| Ok((account == "asr.api_key").then(|| "legacy-source".into())),
            || Ok(None),
            false,
        )
        .unwrap();
        assert_eq!(
            lookup_account(&legacy, CredentialAccount::AsrApiKey).as_deref(),
            Some("legacy-source")
        );
        let empty =
            load_desktop_credentials_readonly_with(|_| Ok(None), || Ok(None), false).unwrap();
        assert!(empty.providers.asr.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn recovery_android_vault_read_is_strict_and_does_not_scrub_or_migrate() {
        use base64::Engine;
        let temp = Temporary::new();
        let path = temp.0.join("credentials.enc.json");
        let mut root = root_with_secret("private-provider");
        write_marketplace_github_token(&mut root, Some("legacy-oauth".into()));
        let bytes =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&root).unwrap());
        std::fs::write(&path, &bytes).unwrap();
        let mut crypto = super::super::android_credentials::TestCrypto::default();
        let loaded = load_android_credentials_readonly_at(&path, &mut crypto)
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("private-provider")
        );
        assert!(lookup_marketplace_github_token(&loaded).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
        assert_eq!(crypto.delete_key_calls, 0);
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(b"{}"),
        )
        .unwrap();
        assert!(load_android_credentials_readonly_at(&path, &mut crypto).is_err());
        assert!(path.exists());
    }

    #[test]
    fn generation_writer_preserves_old_data_at_every_precommit_failure() {
        let old = serde_json::to_string(&root_with_secret(&"old".repeat(1500))).unwrap();
        let new = serde_json::to_string(&root_with_secret(&"new".repeat(1500))).unwrap();
        let initial = installed_chunks(&old, None);
        for failed_piece in 0..chunk_json_payload(&new).len() {
            let values = RefCell::new(initial.clone());
            let writes = Cell::new(0);
            let result = save_chunked_credentials_with(
                &new,
                |key| Ok(values.borrow().get(key).cloned()),
                |key, value| {
                    if key != KEYRING_CREDENTIALS_ACCOUNT {
                        let n = writes.get();
                        writes.set(n + 1);
                        if n == failed_piece {
                            anyhow::bail!("fixture failure");
                        }
                    }
                    values.borrow_mut().insert(key.into(), value.into());
                    Ok(())
                },
                |key| {
                    values.borrow_mut().remove(key);
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert_eq!(
                read_root(&values).providers.asr["stable-channel"].apiKey,
                root_with_secret(&"old".repeat(1500)).providers.asr["stable-channel"].apiKey
            );
            assert_eq!(
                values.borrow().get(KEYRING_CREDENTIALS_ACCOUNT),
                initial.get(KEYRING_CREDENTIALS_ACCOUNT)
            );
        }
    }

    #[test]
    fn generation_writer_verifies_chunks_and_head_without_claiming_fake_success() {
        let old = serde_json::to_string(&root_with_secret("old")).unwrap();
        let new = serde_json::to_string(&root_with_secret(&"new".repeat(1000))).unwrap();
        for mode in [
            "head_read",
            "chunk_read",
            "corrupt_chunk",
            "head_noop",
            "head_error",
            "head_confirmation",
        ] {
            let values = RefCell::new(installed_chunks(&old, Some("old-generation")));
            let switched = Cell::new(false);
            let result = save_chunked_credentials_with(
                &new,
                |key| {
                    if mode == "head_read" && key == KEYRING_CREDENTIALS_ACCOUNT {
                        anyhow::bail!("fixture");
                    }
                    if mode == "head_confirmation"
                        && key == KEYRING_CREDENTIALS_ACCOUNT
                        && switched.get()
                    {
                        anyhow::bail!("fixture");
                    }
                    if key.starts_with(KEYRING_CREDENTIALS_CHUNK_PREFIX)
                        && !key.contains("old-generation")
                    {
                        if mode == "chunk_read" {
                            anyhow::bail!("fixture");
                        }
                        if mode == "corrupt_chunk" {
                            return Ok(Some("corrupt".into()));
                        }
                    }
                    Ok(values.borrow().get(key).cloned())
                },
                |key, value| {
                    if key == KEYRING_CREDENTIALS_ACCOUNT {
                        if mode == "head_error" {
                            anyhow::bail!("fixture");
                        }
                        if mode == "head_noop" {
                            return Ok(());
                        }
                        switched.set(true);
                    }
                    values.borrow_mut().insert(key.into(), value.into());
                    Ok(())
                },
                |key| {
                    values.borrow_mut().remove(key);
                    Ok(())
                },
            );
            assert!(result.is_err(), "{mode}");
            let expected = if mode == "head_confirmation" {
                "new".repeat(1000)
            } else {
                "old".into()
            };
            assert_eq!(
                read_root(&values).providers.asr["stable-channel"]
                    .apiKey
                    .as_deref(),
                Some(expected.as_str())
            );
        }
    }

    #[test]
    fn generation_commit_survives_lost_ack_and_cleanup_failure_and_migrates_legacy() {
        let old = serde_json::to_string(&root_with_secret("old")).unwrap();
        let new = serde_json::to_string(&root_with_secret("new")).unwrap();
        for initial in [
            installed_chunks(&old, None),
            installed_chunks(&old, Some("previous-generation")),
            HashMap::from([(KEYRING_CREDENTIALS_ACCOUNT.into(), old.clone())]),
        ] {
            let values = RefCell::new(initial);
            save_chunked_credentials_with(
                &new,
                |key| Ok(values.borrow().get(key).cloned()),
                |key, value| {
                    values.borrow_mut().insert(key.into(), value.into());
                    if key == KEYRING_CREDENTIALS_ACCOUNT {
                        anyhow::bail!("lost native ack");
                    }
                    Ok(())
                },
                |_| anyhow::bail!("cleanup denied"),
            )
            .unwrap();
            assert_eq!(
                read_root(&values).providers.asr["stable-channel"]
                    .apiKey
                    .as_deref(),
                Some("new")
            );
            let head = values.borrow()[KEYRING_CREDENTIALS_ACCOUNT].clone();
            assert!(read_chunk_manifest(&head).unwrap().generation.is_some());
        }
    }

    #[test]
    fn full_provider_capture_roundtrips_fixed_ids_and_keeps_local_oauth() {
        let mut root = root_with_secret("asr-primary");
        let asr = root.providers.asr.get_mut("stable-channel").unwrap();
        asr.appKey = None;
        asr.accessKey = Some("legacy-access".into());
        asr.resourceId = Some("resource".into());
        asr.volcengineApiKey = Some("new-service-key".into());
        asr.vocabularyId = Some("vocab".into());
        asr.advancedConfig = Some("{\"verboseJson\":true}".into());
        asr.xfyunAppId = Some("xfyun-id".into());
        asr.xfyunApiKey = Some("xfyun-secret".into());
        asr.tencentCloudAppId = Some("tencent-id".into());
        asr.tencentCloudSecretId = Some("tencent-secret-id".into());
        asr.tencentCloudSecretKey = Some("tencent-secret".into());
        asr.channel.lastTest = Some(ChannelTest {
            ok: true,
            latencyMs: Some(1),
            at: 5,
            error: None,
        });
        root.active.llm = "stable-llm".into();
        root.providers.llm.insert(
            "stable-llm".into(),
            CredsLlmEntry {
                channel: ChannelMeta {
                    providerType: Some("custom".into()),
                    order: Some(5),
                    enabled: true,
                    lastTest: asr.channel.lastTest.clone(),
                },
                displayName: Some("Named provider".into()),
                apiKey: Some("llm-secret".into()),
                baseURL: Some("https://api.example/v1".into()),
                model: Some("model".into()),
                temperature: Some(0.7),
                extraHeaders: Some(HashMap::from([(
                    "x-provider-key".into(),
                    "header-secret".into(),
                )])),
                requestFormat: Some("chat_completions".into()),
                messagesThinking: Some("adaptive".into()),
                maxTokens: Some("4096".into()),
                thinkingBudget: None,
            },
        );
        root.omni.active = "custom".into();
        for id in ["custom", "qwen3-omni", "volcengine-omni"] {
            root.omni.providers.insert(
                id.into(),
                CredsOmniEntry {
                    apiKey: Some(format!("{id}-secret")),
                    baseURL: Some("https://omni.example".into()),
                    model: Some("omni-model".into()),
                    temperature: Some(0.8),
                    extraHeaders: Some(HashMap::from([(
                        "x-extra".into(),
                        "omni-extra-secret".into(),
                    )])),
                    ..Default::default()
                },
            );
        }
        write_marketplace_github_token(&mut root, Some("local-only-oauth".into()));
        let count = Cell::new(0);
        let capture = capture_sync_credentials_with(|| {
            count.set(count.get() + 1);
            Ok(root.clone())
        })
        .unwrap();
        assert_eq!(count.get(), 1);
        assert!(!format!("{capture:?}").contains("secret"));
        assert!(!capture.credentials.iter().any(|record| record
            .accounts
            .values()
            .any(|value| value == "local-only-oauth")));
        assert!(!capture
            .credentials
            .iter()
            .find(|record| record.namespace == SyncNamespace::Asr)
            .unwrap()
            .accounts
            .contains_key("volcengine.app_key"));
        assert_eq!(
            capture
                .channels
                .iter()
                .filter(|c| c.namespace == SyncNamespace::Omni)
                .count(),
            3
        );
        let restored = apply_sync_credentials_root(&root, &capture).unwrap();
        assert_eq!(export_sync_credentials_root(&restored).unwrap(), capture);
        assert_eq!(
            lookup_marketplace_github_token(&restored).as_deref(),
            Some("local-only-oauth")
        );
        assert!(restored
            .providers
            .asr
            .values()
            .all(|entry| entry.channel.lastTest.is_none()));
        assert!(restored
            .providers
            .llm
            .values()
            .all(|entry| entry.channel.lastTest.is_none()));
        let mut excluded = capture.clone();
        excluded.credentials[0]
            .accounts
            .insert("github.oauth_token".into(), "not-allowed".into());
        assert!(apply_sync_credentials_root(&root, &excluded).is_err());
        let mut invalid_protocol = capture.clone();
        let accounts = &mut invalid_protocol
            .credentials
            .iter_mut()
            .find(|record| record.namespace == SyncNamespace::Llm)
            .unwrap()
            .accounts;
        accounts.insert("ark.request_format".into(), "messages".into());
        accounts.insert("ark.messages_thinking".into(), "budget".into());
        accounts.insert("ark.thinking_budget".into(), "4096".into());
        assert!(apply_sync_credentials_root(&root, &invalid_protocol).is_err());
        assert!(
            capture_sync_credentials_with(|| Err(anyhow::anyhow!("unreadable fixture vault")))
                .is_err()
        );
    }

    #[test]
    fn mutation_lease_precedes_read_and_lives_until_actual_save_finishes() {
        let temp = Temporary::new();
        let gate = SyncWriteGate::open(temp.0.join("generation.json")).unwrap();
        mutate_credentials_with(
            Some(gate.clone()),
            ChangeOrigin::User,
            || {
                assert!(gate.try_exclusive().is_err());
                Ok(root_with_secret("old"))
            },
            |root| {
                root.active.asr = "changed".into();
                Ok(true)
            },
            |_| {
                assert!(gate.try_exclusive().is_err());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(gate.generation().unwrap().get(), 1);
        let exclusive = gate.try_exclusive().unwrap();
        assert!(mutate_credentials_with(
            Some(gate.clone()),
            ChangeOrigin::User,
            || panic!("must reject before read"),
            |_| Ok(true),
            |_| Ok(())
        )
        .is_err());
        drop(exclusive);
        assert!(mutate_credentials_with(
            Some(gate.clone()),
            ChangeOrigin::User,
            || Err(anyhow::anyhow!("read failed")),
            |_| Ok(true),
            |_| Ok(())
        )
        .is_err());
        assert!(!gate.recovery_required().unwrap());
        mutate_credentials_with(
            Some(gate.clone()),
            ChangeOrigin::LocalOnly,
            || Ok(root_with_secret("old")),
            |_| Ok(true),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(gate.generation().unwrap().get(), 1);
        assert!(mutate_credentials_with(
            Some(gate.clone()),
            ChangeOrigin::User,
            || Ok(root_with_secret("old")),
            |_| Ok(true),
            |_| Err(VaultCommitFailure::Unknown.into())
        )
        .is_err());
        assert!(gate.recovery_required().unwrap());
    }

    #[test]
    fn unknown_publication_drops_cache_before_reconciliation() {
        let old = root_with_secret("old");
        let new = root_with_secret("new");
        store_credentials_cache(&old);
        assert!(finish_credential_write(&new, Err(VaultCommitFailure::Unchanged.into())).is_err());
        assert_eq!(
            credentials_cache().lock().as_ref().unwrap().providers.asr["stable-channel"]
                .apiKey
                .as_deref(),
            Some("old")
        );
        assert!(finish_credential_write(&new, Err(VaultCommitFailure::Unknown.into())).is_err());
        assert!(credentials_cache().lock().is_none());
        let count = Cell::new(0);
        let loaded = load_credentials_into_cache_with(|| {
            count.set(count.get() + 1);
            Ok(Some(new))
        });
        assert_eq!(count.get(), 1);
        assert_eq!(
            loaded.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("new")
        );
        reset_credentials_cache_for_tests();
    }

    #[test]
    fn failed_revocation_after_concurrent_login_keeps_oauth_unusable() {
        use std::sync::{mpsc, Arc};
        use std::time::{Duration, Instant};
        let root = Arc::new(Mutex::new(root_with_secret("provider-only")));
        store_credentials_cache(&root.lock());
        mark_marketplace_token_verified();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let login_root = root.clone();
        let login = std::thread::spawn(move || {
            set_marketplace_token_with(
                None,
                "new-oauth",
                || Ok(login_root.lock().clone()),
                |next| {
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    *login_root.lock() = next.clone();
                    store_credentials_cache(next);
                    Ok(())
                },
            )
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let revoke = std::thread::spawn(move || {
            remove_marketplace_token_with(
                None,
                || Ok(root.lock().clone()),
                |_| Err(VaultCommitFailure::Unchanged.into()),
            )
        });
        let start = Instant::now();
        while !marketplace_token_is_rejected() && start.elapsed() < Duration::from_secs(5) {
            std::thread::yield_now();
        }
        let saw_revocation = marketplace_token_is_rejected();
        release_tx.send(()).unwrap();
        login.join().unwrap().unwrap();
        assert!(revoke.join().unwrap().is_err());
        assert!(saw_revocation);
        assert!(marketplace_token_is_rejected());
        assert!(credentials_cache()
            .lock()
            .as_ref()
            .and_then(lookup_marketplace_github_token)
            .is_none());
        reset_credentials_cache_for_tests();
        mark_marketplace_token_verified();
    }

    #[cfg(not(windows))]
    #[test]
    fn sync_key_write_does_not_complete_or_delete_main_vault_migration() {
        use super::super::android_credentials::{AndroidCredentialsCrypto, ReadOutcome};
        use base64::Engine;
        let temp = Temporary::new();
        let main_path = temp.0.join("credentials.enc.json");
        let legacy = serde_json::to_vec(&root_with_secret("legacy-provider-secret")).unwrap();
        std::fs::write(
            &main_path,
            base64::engine::general_purpose::STANDARD.encode(&legacy),
        )
        .unwrap();
        let account =
            SyncSecretAccount::new(format!("cloud-sync.e2ee.key.{}", "c".repeat(64))).unwrap();
        let key = openless_core::SecretValue::new(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([8; 32]),
        );
        let mut wrapper = SyncSecretCrypto {
            inner: super::super::android_credentials::TestCrypto::default(),
            account: account.as_str().into(),
        };
        write_android_sync_key_with_crypto(
            &temp.0.join("sync-key.json"),
            &account,
            &key,
            &mut wrapper,
        )
        .unwrap();
        assert!(!wrapper.inner.migration_complete().unwrap());
        assert!(wrapper.migration_complete().unwrap());
        wrapper.delete_key().unwrap();
        assert_eq!(wrapper.inner.delete_key_calls, 0);
        assert!(matches!(
            super::super::android_credentials::read(&main_path, &mut wrapper.inner).unwrap(),
            ReadOutcome::Legacy(_)
        ));
        let migrated =
            load_android_credentials_from_path_with_crypto(&main_path, &mut wrapper.inner)
                .unwrap()
                .unwrap();
        assert_eq!(
            migrated.providers.asr["stable-channel"].apiKey.as_deref(),
            Some("legacy-provider-secret")
        );
        assert!(wrapper.inner.migration_complete().unwrap());
    }

    #[cfg(not(windows))]
    #[test]
    fn android_sync_key_envelope_is_account_bound_and_never_plaintext() {
        use base64::Engine;
        let temp = Temporary::new();
        let path = temp.0.join("sync-key.json");
        let account =
            SyncSecretAccount::new(format!("cloud-sync.e2ee.local.{}", "a".repeat(64))).unwrap();
        let other =
            SyncSecretAccount::new(format!("cloud-sync.e2ee.local.{}", "b".repeat(64))).unwrap();
        let key = openless_core::SecretValue::new(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 32]),
        );
        let mut crypto = SyncSecretCrypto {
            inner: super::super::android_credentials::TestCrypto::default(),
            account: account.as_str().into(),
        };
        write_android_sync_key_with_crypto(&path, &account, &key, &mut crypto).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(key.expose_secret()));
        assert_eq!(
            read_android_sync_key_with_crypto(&path, &account, &mut crypto).unwrap(),
            Some(key.clone())
        );
        crypto.account = other.as_str().into();
        assert!(read_android_sync_key_with_crypto(&path, &other, &mut crypto).is_err());
        assert!(path.exists());
        assert!(super::super::android_credentials::read(&path, &mut crypto.inner).is_err());
        assert!(path.exists());
        assert!(validate_sync_key(&openless_core::SecretValue::new("not-a-key")).is_err());
    }
}

#[cfg(test)]
mod sync_capture_diagnostic_tests {
    use super::*;
    #[test]
    fn typed_diagnostics_never_expose_error_bodies_or_attributes() {
        let private = "fixture-private-value-never-log";
        let cases: Vec<(anyhow::Error, &str)> = vec![
            (
                anyhow::Error::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    private,
                ))
                .context(private),
                "io_permission_denied",
            ),
            (
                anyhow::Error::new(
                    serde_json::from_str::<u32>(&format!("\"{private}\"")).unwrap_err(),
                )
                .context(private),
                "json_data",
            ),
            (
                anyhow::Error::new(serde_json::from_str::<serde_json::Value>("]").unwrap_err()),
                "json_syntax",
            ),
            (
                anyhow::Error::new(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
                "json_eof",
            ),
            (
                anyhow::Error::new(String::from_utf8(vec![255]).unwrap_err()),
                "invalid_utf8",
            ),
            (anyhow::anyhow!(private), "native_store_failed"),
        ];
        for (error, expected) in cases {
            let code = sync_capture_read_error_code(&error);
            assert_eq!(code, expected);
            assert!(!code.contains(private));
        }
    }
    #[cfg(not(target_os = "android"))]
    #[test]
    fn keyring_causes_are_classified_without_formatting_secret_material() {
        let private = "fixture-private-value-never-log";
        let cases: Vec<(keyring::Error, &str)> = vec![
            (
                keyring::Error::NoStorageAccess(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    private,
                ))),
                "keyring_access_denied",
            ),
            (
                keyring::Error::PlatformFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    private,
                ))),
                "io_timed_out",
            ),
            (
                keyring::Error::BadEncoding(private.as_bytes().to_vec()),
                "keyring_bad_encoding",
            ),
            (
                keyring::Error::Invalid(private.into(), private.into()),
                "keyring_invalid_attribute",
            ),
            (keyring::Error::NoEntry, "keyring_no_entry"),
        ];
        for (error, expected) in cases {
            assert_eq!(
                sync_capture_read_error_code(&anyhow::Error::new(error).context(private)),
                expected
            );
        }
    }
    #[test]
    fn projection_diagnostic_does_not_relax_omni_identity_validation() {
        let mut root = CredsRoot::default();
        root.omni.active = "custom".into();
        root.omni.providers.insert(
            "custom".into(),
            CredsOmniEntry {
                channel: ChannelMeta {
                    providerType: Some("different".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let error = export_sync_credentials_root(&root).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<openless_core::BackendError>()
                .unwrap()
                .code,
            openless_core::BackendErrorCode::InvalidArgument
        );
    }

    #[test]
    fn omni_defaults_and_legacy_account_import_use_the_same_custom_slot() {
        let omitted: CredsRoot =
            serde_json::from_str(r#"{"version":1,"active":{},"providers":{}}"#).unwrap();
        let explicit: CredsOmni = serde_json::from_str("{}").unwrap();
        assert_eq!(CredsRoot::default().omni.active, explicit.active);
        assert_eq!(omitted.omni.active, "custom");
        let imported = load_desktop_credentials_readonly_with(
            |account| Ok((account == "omni.api_key").then(|| "legacy-fixture-key".into())),
            || Ok(None),
            true,
        )
        .unwrap();
        assert_eq!(imported.omni.providers.len(), 1);
        assert_eq!(
            imported.omni.providers["custom"].apiKey.as_deref(),
            Some("legacy-fixture-key")
        );
        export_sync_credentials_root(&imported).unwrap();
    }

    fn legacy_empty_omni_slot() -> CredsOmniEntry {
        CredsOmniEntry {
            displayName: Some("Legacy fixture".into()),
            apiKey: Some("legacy-fixture-key".into()),
            baseURL: Some("https://legacy.example/v1".into()),
            model: Some("legacy-model".into()),
            temperature: Some(0.7),
            extraHeaders: Some(HashMap::from([("x-fixture".into(), "header-value".into())])),
            channel: ChannelMeta {
                order: Some(3),
                lastTest: Some(ChannelTest {
                    ok: true,
                    latencyMs: Some(12),
                    at: 7,
                    error: None,
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn legacy_empty_omni_slot_migrates_losslessly_and_preserves_selected_provider() {
        for active in ["", "qwen3-omni"] {
            let mut root = CredsRoot {
                version: CHANNELS_SCHEMA_VERSION,
                ..Default::default()
            };
            root.omni.active = active.into();
            let mut expected = legacy_empty_omni_slot();
            root.omni.providers.insert(String::new(), expected.clone());
            let persisted_before = serde_json::to_value(&root).unwrap();

            // Match a read-only capture: normalize the loaded copy, never the OS source.
            let mut loaded = decode_single_credentials(&persisted_before.to_string()).unwrap();
            assert!(migrate_channels(&mut loaded));
            expected.channel.providerType = Some("custom".into());
            assert_eq!(
                serde_json::to_value(&loaded.omni.providers["custom"]).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert!(!loaded.omni.providers.contains_key(""));
            assert_eq!(
                loaded.omni.active,
                if active.is_empty() { "custom" } else { active }
            );
            assert_eq!(loaded.active.asr, root.active.asr);
            assert_eq!(loaded.active.llm, root.active.llm);
            let captured = capture_sync_credentials_with(|| Ok(loaded.clone())).unwrap();
            let restored = apply_sync_credentials_root(&loaded, &captured).unwrap();
            assert_eq!(export_sync_credentials_root(&restored).unwrap(), captured);
            assert!(!migrate_channels(&mut loaded));
            assert_eq!(serde_json::to_value(&root).unwrap(), persisted_before);
        }
    }

    #[test]
    fn legacy_empty_omni_slot_only_deduplicates_identical_custom_configuration() {
        let mut root = CredsRoot {
            version: CHANNELS_SCHEMA_VERSION,
            ..Default::default()
        };
        root.omni.active = "qwen3-omni".into();
        let legacy = legacy_empty_omni_slot();
        let mut custom = legacy.clone();
        custom.channel.providerType = Some("custom".into());
        root.omni.providers.insert(String::new(), legacy);
        root.omni.providers.insert("custom".into(), custom);
        assert!(migrate_channels(&mut root));
        assert!(!root.omni.providers.contains_key(""));
        assert_eq!(root.omni.active, "qwen3-omni");
        export_sync_credentials_root(&root).unwrap();

        for field in [
            "apiKey",
            "baseURL",
            "model",
            "temperature",
            "extraHeaders",
            "displayName",
            "order",
            "enabled",
            "lastTest",
        ] {
            let mut conflict = root.clone();
            conflict
                .omni
                .providers
                .insert(String::new(), legacy_empty_omni_slot());
            let mut changed = serde_json::to_value(&conflict.omni.providers["custom"]).unwrap();
            changed.as_object_mut().unwrap().remove(field);
            if field == "enabled" {
                changed[field] = serde_json::json!(false);
            }
            conflict
                .omni
                .providers
                .insert("custom".into(), serde_json::from_value(changed).unwrap());
            let before = serde_json::to_value(&conflict).unwrap();
            assert!(
                !migrate_channels(&mut conflict),
                "conflicting {field} must stay untouched"
            );
            assert_eq!(serde_json::to_value(&conflict).unwrap(), before);
            assert!(export_sync_credentials_root(&conflict).is_err());
        }
    }

    #[test]
    fn legacy_omni_repair_does_not_reinterpret_other_invalid_identities() {
        for provider_type in [None, Some(""), Some("custom")] {
            let mut root = CredsRoot {
                version: CHANNELS_SCHEMA_VERSION,
                ..Default::default()
            };
            let mut entry = legacy_empty_omni_slot();
            entry.channel.providerType = provider_type.map(str::to_string);
            root.omni.providers.insert(String::new(), entry);
            assert!(migrate_channels(&mut root));
            export_sync_credentials_root(&root).unwrap();
        }
        for (id, provider_type) in [("", "other"), ("unsafe/path", "unsafe/path")] {
            let mut root = CredsRoot {
                version: CHANNELS_SCHEMA_VERSION,
                ..Default::default()
            };
            let mut entry = legacy_empty_omni_slot();
            entry.channel.providerType = Some(provider_type.into());
            root.omni.providers.insert(id.into(), entry);
            let before = serde_json::to_value(&root).unwrap();
            assert!(!migrate_channels(&mut root));
            assert_eq!(serde_json::to_value(&root).unwrap(), before);
            assert!(export_sync_credentials_root(&root).is_err());
        }
    }

    #[test]
    fn legacy_omni_repair_preserves_restored_empty_and_inactive_snapshots() {
        let root = CredsRoot::default();
        let empty = openless_core::credentials::SyncCredentials {
            channels: vec![],
            credentials: vec![],
        };
        let mut inactive = export_sync_credentials_root(&root).unwrap();
        for channel in &mut inactive.channels {
            channel.active = false;
            channel.enabled = false;
        }
        for snapshot in [empty, inactive] {
            let mut restored = apply_sync_credentials_root(&root, &snapshot).unwrap();
            assert!(restored.omni.active.is_empty());
            assert!(!migrate_channels(&mut restored));
            assert_eq!(export_sync_credentials_root(&restored).unwrap(), snapshot);
        }
    }
}
