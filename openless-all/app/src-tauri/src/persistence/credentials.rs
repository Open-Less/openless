#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Credentials vault.
//!
//! 正常读写走系统凭据库；旧 plaintext JSON 只作为迁移来源。为保持多 provider
//! schema 与 active provider 状态，凭据库里保存一个 v1 JSON payload；payload 会按平台
//! 凭据库限制拆成多个条目，避免 Windows 单条凭据 2560 bytes 限制。
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

// `anyhow!` is only invoked from the keyring (non-Android) code paths; gating the
// import keeps the Android build free of an unused-import warning.
#[cfg(not(target_os = "android"))]
use anyhow::anyhow;

/// 旧版 plaintext JSON 凭据路径。仅作为迁移来源；成功写入系统凭据库后会删除。
const LEGACY_CREDS_DIR: &str = ".openless";
const LEGACY_CREDS_FILE: &str = "credentials.json";

const KEYRING_CREDENTIALS_ACCOUNT: &str = "credentials.v1";
const KEYRING_CREDENTIALS_CHUNK_PREFIX: &str = "credentials.v1.chunk.";
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
const KEYRING_CHUNK_MAX_UTF16_UNITS: usize = 1000;

static CREDENTIALS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

#[cfg(target_os = "android")]
fn android_marketplace_token() -> &'static Mutex<Option<MarketplaceGithubToken>> {
    ANDROID_MARKETPLACE_TOKEN.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn android_marketplace_legacy_scrubbed() -> &'static Mutex<bool> {
    ANDROID_MARKETPLACE_LEGACY_SCRUBBED.get_or_init(|| Mutex::new(false))
}

/// Process-wide credentials cache.
///
/// Without this cache every `CredentialsVault::get_*` / `snapshot` call hits
/// `load_credentials()` → `load_keyring_credentials()` which reads the
/// manifest entry plus every chunk entry from the OS keyring. On macOS each
/// distinct keychain entry has its own ACL — so an ad-hoc-signed binary (or
/// any binary whose ACL grants haven't been set up yet) prompts on every read
/// of every entry. A single dictation cycle reads credentials 5–10 times,
/// times (1 manifest + N chunks) entries → tens of "OpenLess wants to use
/// the keychain" prompts per recording.
///
/// With this cache the first read populates `Some(CredsRoot)` and every
/// subsequent read in the same process is silent. `save_credentials` keeps
/// the cache in sync after writes so Settings → Recording credential edits
/// take effect immediately.
///
/// Cross-process changes (e.g. user edits via `security` CLI, or another
/// instance of the app — single-instance is enforced but defense in depth)
/// will be invisible until the next process launch. Acceptable trade-off
/// per the credential vault contract: the keyring is owned by this app.
static CREDENTIALS_CACHE: OnceLock<Mutex<Option<CredsRoot>>> = OnceLock::new();

fn credentials_cache() -> &'static Mutex<Option<CredsRoot>> {
    CREDENTIALS_CACHE.get_or_init(|| Mutex::new(None))
}

fn store_credentials_cache(root: &CredsRoot) {
    *credentials_cache().lock() = Some(root.clone());
}

#[cfg(test)]
fn reset_credentials_cache_for_tests() {
    *credentials_cache().lock() = None;
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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsAsrEntry {
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
    vocabularyId: Option<String>,
}

impl CredsAsrEntry {
    fn is_empty(&self) -> bool {
        self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.appKey.as_deref().unwrap_or("").is_empty()
            && self.accessKey.as_deref().unwrap_or("").is_empty()
            && self.resourceId.as_deref().unwrap_or("").is_empty()
            && self.vocabularyId.as_deref().unwrap_or("").is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsLlmEntry {
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

impl CredsLlmEntry {
    fn is_empty(&self) -> bool {
        self.displayName.as_deref().unwrap_or("").is_empty()
            && self.apiKey.as_deref().unwrap_or("").is_empty()
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

fn active_llm_extra_headers(root: &CredsRoot) -> HashMap<String, String> {
    root.providers
        .llm
        .get(&root.active.llm)
        .and_then(|entry| entry.extraHeaders.clone())
        .unwrap_or_default()
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

#[cfg(not(target_os = "android"))]
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
    let root = serde_json::from_slice::<CredsRoot>(&bytes)
        .context("parse Android credential payload")?;
    let cleaned = android_persistable_credentials(&root);
    let contained_marketplace_token = lookup_marketplace_github_token(&root).is_some();
    if needs_rewrite && contained_marketplace_token {
        let sanitized = serde_json::to_vec(&cleaned)
            .context("encode bearer-free Android legacy payload")?;
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
    /// 旧版本（v1 早期）每次 save 都生成新 UUID 作为 chunk account 命名前缀，
    /// 这让 macOS Keychain 的「始终允许」每次保存后失效 → 反复弹 ACL 弹窗。
    /// 现在 save 总用稳定 chunk.{index} 名，此字段仅向后兼容旧 manifest 读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
    chunks: usize,
}

/// 旧版（generation=Some）：`credentials.v1.chunk.<UUID>.{index}`
/// 新版（generation=None）：`credentials.v1.chunk.{index}` —— 稳定名，ACL 长期有效
fn chunk_account(generation: Option<&str>, index: usize) -> String {
    match generation {
        Some(gen) => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{gen}.{index}"),
        None => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{index}"),
    }
}

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
    let Some(json_or_manifest) = get_keyring_password(KEYRING_CREDENTIALS_ACCOUNT)? else {
        return Ok(None);
    };

    let manifest = read_chunk_manifest(&json_or_manifest)
        .ok_or_else(|| anyhow!("invalid system credential vault manifest"))?;
    let mut json = String::new();
    for index in 0..manifest.chunks {
        let account = chunk_account(manifest.generation.as_deref(), index);
        let chunk = get_keyring_password(&account)?
            .ok_or_else(|| anyhow!("missing system credential vault chunk {index}"))?;
        json.push_str(&chunk);
    }

    serde_json::from_str::<CredsRoot>(&json)
        .map(Some)
        .context("decode system credential vault payload")
}

#[cfg(not(target_os = "android"))]
fn load_legacy_keyring_credentials() -> CredsRoot {
    match load_legacy_keyring_credentials_for_update() {
        Ok(root) => root,
        Err(e) => {
            log::warn!("[vault] read legacy vault credentials failed: {e}");
            CredsRoot::default()
        }
    }
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

fn load_legacy_credentials() -> Option<CredsRoot> {
    credentials_path()
        .ok()
        .and_then(|p| read_legacy_credentials_file(&p))
}

fn legacy_vault_has_credentials(root: &CredsRoot) -> bool {
    !root.providers.asr.is_empty() || !root.providers.llm.is_empty()
}

fn load_legacy_sources_without_migration() -> CredsRoot {
    if let Some(legacy) = load_legacy_credentials() {
        return legacy;
    }

    #[cfg(not(target_os = "android"))]
    {
        let legacy_vault = load_legacy_keyring_credentials();
        if legacy_vault_has_credentials(&legacy_vault) {
            return legacy_vault;
        }
    }

    CredsRoot::default()
}

fn migrate_legacy_sources() -> CredsRoot {
    match migrate_legacy_sources_for_update() {
        Ok(root) => root,
        Err(e) => {
            log::warn!("[vault] legacy credential migration failed: {e}");
            load_legacy_sources_without_migration()
        }
    }
}

fn migrate_legacy_sources_for_update() -> Result<CredsRoot> {
    if let Some(legacy) = load_legacy_credentials() {
        save_credentials(&legacy)?;
        #[cfg(not(target_os = "android"))]
        remove_legacy_keyring_credentials();
        return Ok(legacy);
    }

    #[cfg(not(target_os = "android"))]
    {
        let legacy_vault = load_legacy_keyring_credentials_for_update()?;
        if legacy_vault_has_credentials(&legacy_vault) {
            save_credentials(&legacy_vault)?;
            remove_legacy_keyring_credentials();
            return Ok(legacy_vault);
        }
    }

    Ok(CredsRoot::default())
}

#[cfg(any(target_os = "android", test))]
fn load_android_credentials_into_cache_with(
    loader: impl FnOnce() -> Result<Option<CredsRoot>>,
) -> CredsRoot {
    match loader() {
        Ok(root) => {
            let root = root.unwrap_or_default();
            store_credentials_cache(&root);
            root
        }
        Err(e) => {
            // Do not cache the fallback. In particular, a failed legacy-token
            // scrub must be retried by the next startup/getter call rather than
            // hidden for the rest of the process.
            log::warn!("[vault] android credential read failed: {e}");
            CredsRoot::default()
        }
    }
}

fn load_credentials() -> CredsRoot {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return cached;
    }

    #[cfg(target_os = "android")]
    {
        return load_android_credentials_into_cache_with(load_android_credentials);
    }

    #[cfg(not(target_os = "android"))]
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 不在这里调 remove_legacy_keyring_credentials() —— 它内部对每个
            // 旧 account 各做一次 keyring delete，每次 delete 在 macOS Keychain
            // 上仍要触发 ACL 检查。第一次成功 load 时 legacy entries 通常已经
            // 被 migrate_legacy_sources_for_update 清理过了；这里若再无脑跑，
            // 只会反复弹「OpenLess 想删除 X」十几次。文件 legacy（plaintext
            // JSON）不需要 ACL，可继续 best-effort 删除。
            remove_legacy_credentials_file_best_effort();
            store_credentials_cache(&root);
            root
        }
        Ok(None) => {
            // 没有现成 chunked manifest —— 走 migrate（如果有 legacy 则写入并返回写后的 root）。
            // migrate_legacy_sources 内部 save_credentials 已经会刷 cache，这里再补一次
            // 是为了「无 legacy 也无 manifest」走默认 root 的路径也能进 cache。
            let root = migrate_legacy_sources();
            store_credentials_cache(&root);
            root
        }
        Err(e) => {
            // **不缓存 keyring 错误路径下的 fallback**。Keychain 可能只是临时不可读
            // （用户尚未在第一次弹窗里点同意 / DataProtection 错误 / login keychain
            // 还没 unlock）；如果在这里把 legacy fallback 写进 cache，等用户授权后
            // 我们就再也不会重读 keyring，整个进程生命周期里都拿 stale 数据。下次
            // 调用让它再尝试一次 keyring。pr_agent feedback on PR #394。
            log::warn!("[vault] system credential read failed: {e}");
            load_legacy_sources_without_migration()
        }
    }
}

fn load_credentials_for_update() -> Result<CredsRoot> {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return Ok(cached);
    }

    #[cfg(target_os = "android")]
    {
        let root = match load_android_credentials()? {
            Some(root) => root,
            None => CredsRoot::default(),
        };
        store_credentials_cache(&root);
        return Ok(root);
    }

    #[cfg(not(target_os = "android"))]
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 同 load_credentials：不再每次 update 都尝试 delete legacy keyring
            // entries，避免反复触发 macOS Keychain ACL 弹窗。
            remove_legacy_credentials_file_best_effort();
            store_credentials_cache(&root);
            Ok(root)
        }
        Ok(None) => {
            // migrate_legacy_sources_for_update 内部如果实际 migrate 会调
            // save_credentials，cache 会被刷新；如果只返回 default root（没 legacy），
            // 我们这里再显式 cache 一次防御性补一下。
            let root = migrate_legacy_sources_for_update()?;
            store_credentials_cache(&root);
            Ok(root)
        }
        // 错误路径不缓存 —— 同 load_credentials 注释；让下次读重试 keyring。
        Err(e) => Err(e),
    }
}

fn save_credentials(root: &CredsRoot) -> Result<()> {
    let cleaned = clean_credentials(root);

    #[cfg(target_os = "android")]
    {
        save_android_credentials(&cleaned)?;
        store_credentials_cache(&cleaned);
        return Ok(());
    }

    #[cfg(not(target_os = "android"))]
    {
        let json = serde_json::to_string(&cleaned).context("encode credentials failed")?;
        let previous_manifest = get_keyring_password(KEYRING_CREDENTIALS_ACCOUNT)
            .ok()
            .flatten()
            .and_then(|value| read_chunk_manifest(&value));
        let chunks = chunk_json_payload(&json);

        // 先写所有 chunks（稳定名），再写 manifest —— 保证 partial-write 不会让
        // manifest 指向不完整 chunks。stable name 让 macOS Keychain ACL 一次允许后
        // 长期有效，不再因 UUID 轮换反复弹窗（这是 PR #277 早期 UUID-rotation
        // 设计的回退）。
        for (index, chunk) in chunks.iter().enumerate() {
            let account = chunk_account(None, index);
            keyring_entry_for(&account)?
                .set_password(chunk)
                .with_context(|| format!("write system credential vault chunk {index}"))?;
        }

        let manifest = CredsChunkManifest {
            openless_credentials_storage: "chunked".to_string(),
            version: 1,
            generation: None,
            chunks: chunks.len(),
        };
        let manifest_json =
            serde_json::to_string(&manifest).context("encode credential manifest failed")?;
        keyring_entry()?
            .set_password(&manifest_json)
            .context("write system credential vault manifest")?;

        // 清理旧 chunks：
        // 1) 旧 manifest 用 UUID generation → 那一代 chunks 全删（迁移到 stable name）
        // 2) 旧 manifest 也是 stable name，但 chunks 数量比这次多 → 删多余的 idx
        if let Some(previous) = previous_manifest {
            match previous.generation.as_deref() {
                Some(prev_gen) => {
                    for index in 0..previous.chunks {
                        delete_keyring_password(&chunk_account(Some(prev_gen), index));
                    }
                }
                None => {
                    for index in chunks.len()..previous.chunks {
                        delete_keyring_password(&chunk_account(None, index));
                    }
                }
            }
        }

        remove_legacy_credentials_file_best_effort();
        // 写完成功后立刻刷新 process cache —— 同进程后续读不再回 Keychain。
        // 见 CREDENTIALS_CACHE 的 doc。
        store_credentials_cache(&cleaned);
        Ok(())
    }
}

fn lookup_account(root: &CredsRoot, account: CredentialAccount) -> Option<String> {
    let asr = root.providers.asr.get(&root.active.asr);
    let llm = root.providers.llm.get(&root.active.llm);
    let pick = |s: &Option<String>| s.as_ref().filter(|v| !v.is_empty()).cloned();
    match account {
        CredentialAccount::VolcengineAppKey => {
            asr.and_then(|e| pick(&e.appKey).or_else(|| pick(&e.apiKey)))
        }
        CredentialAccount::VolcengineAccessKey => asr.and_then(|e| pick(&e.accessKey)),
        CredentialAccount::VolcengineResourceId => asr.and_then(|e| pick(&e.resourceId)),
        CredentialAccount::ArkApiKey => llm.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::ArkModelId => llm.and_then(|e| pick(&e.model)),
        CredentialAccount::ArkEndpoint => llm.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrApiKey => asr.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::AsrEndpoint => asr.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrModel => asr.and_then(|e| pick(&e.model)),
        CredentialAccount::AsrVocabularyId => asr.and_then(|e| pick(&e.vocabularyId)),
    }
}

fn write_account(root: &mut CredsRoot, account: CredentialAccount, value: Option<String>) {
    let asr_id = root.active.asr.clone();
    let llm_id = root.active.llm.clone();
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
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CredentialAccount {
    VolcengineAppKey,
    VolcengineAccessKey,
    VolcengineResourceId,
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
            CredentialAccount::ArkApiKey => "ark.api_key",
            CredentialAccount::ArkModelId => "ark.model_id",
            CredentialAccount::ArkEndpoint => "ark.endpoint",
            CredentialAccount::AsrApiKey => "asr.api_key",
            CredentialAccount::AsrEndpoint => "asr.endpoint",
            CredentialAccount::AsrModel => "asr.model",
            CredentialAccount::AsrVocabularyId => "asr.vocabulary_id",
        }
    }

    pub fn all() -> &'static [CredentialAccount] {
        &[
            CredentialAccount::VolcengineAppKey,
            CredentialAccount::VolcengineAccessKey,
            CredentialAccount::VolcengineResourceId,
            CredentialAccount::ArkApiKey,
            CredentialAccount::ArkModelId,
            CredentialAccount::ArkEndpoint,
            CredentialAccount::AsrApiKey,
            CredentialAccount::AsrEndpoint,
            CredentialAccount::AsrModel,
            CredentialAccount::AsrVocabularyId,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsSnapshot {
    pub volcengine_app_key: Option<String>,
    pub volcengine_access_key: Option<String>,
    pub volcengine_resource_id: Option<String>,
    pub asr_api_key: Option<String>,
    pub asr_endpoint: Option<String>,
    pub asr_model: Option<String>,
    pub ark_api_key: Option<String>,
    pub ark_model_id: Option<String>,
    pub ark_endpoint: Option<String>,
}

/// 凭据存储——系统凭据库；旧 JSON 文件只作为迁移来源。
pub struct CredentialsVault;

impl CredentialsVault {
    /// 系统凭据库 service name；macOS 下对应 Keychain service。
    pub const SERVICE_NAME: &'static str = "com.openless.app";

    pub fn get(account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        Ok(lookup_account(&load_credentials(), account))
    }

    pub fn set(account: CredentialAccount, value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let v = if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };
        write_account(&mut root, account, v);
        save_credentials(&root)
    }

    pub fn get_for_asr_provider(id: &str, account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials();
        root.active.asr = id.to_string();
        Ok(lookup_account(&root, account))
    }

    pub fn set_for_asr_provider(id: &str, account: CredentialAccount, value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let active = root.active.asr.clone();
        root.active.asr = id.to_string();
        let value = (!value.is_empty()).then(|| value.to_string());
        write_account(&mut root, account, value);
        root.active.asr = active;
        save_credentials(&root)
    }

    pub fn remove(account: CredentialAccount) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        write_account(&mut root, account, None);
        save_credentials(&root)
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
        Ok(lookup_marketplace_github_token(&load_credentials()))
    }

    pub fn set_marketplace_github_token(value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        #[cfg(target_os = "android")]
        {
            ensure_android_marketplace_legacy_scrubbed()?;
            *android_marketplace_token().lock() =
                (!value.trim().is_empty()).then(|| MarketplaceGithubToken(value.to_string()));
            if value.trim().is_empty() {
                invalidate_marketplace_token_process_local();
            } else {
                mark_marketplace_token_verified();
            }
            return Ok(());
        }
        #[cfg(not(target_os = "android"))]
        {
            let mut root = load_credentials_for_update()?;
            write_marketplace_github_token(&mut root, Some(value.to_string()));
            save_credentials(&root)?;
            mark_marketplace_token_verified();
            Ok(())
        }
    }

    pub fn remove_marketplace_github_token() -> Result<()> {
        let _guard = credentials_lock().lock();
        invalidate_marketplace_token_with(|| {
            #[cfg(target_os = "android")]
            {
                // Retry the durable legacy scrub on every logout until it has
                // actually completed. Process memory is already invalidated.
                return ensure_android_marketplace_legacy_scrubbed();
            }
            #[cfg(not(target_os = "android"))]
            {
                let mut root = load_credentials_for_update()?;
                write_marketplace_github_token(&mut root, None);
                save_credentials(&root)
            }
        })
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

    pub fn get_active_asr() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().active.asr
    }

    pub fn set_active_asr_provider(id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.asr = id.to_string();
        save_credentials(&root)
    }

    pub fn set_active_llm_provider(id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.llm = id.to_string();
        save_credentials(&root)
    }

    pub fn get_active_llm() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().active.llm
    }

    pub fn get_active_llm_extra_headers() -> HashMap<String, String> {
        let _guard = credentials_lock().lock();
        active_llm_extra_headers(&load_credentials())
    }

    pub fn get_active_llm_extra_headers_json() -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        active_llm_extra_headers_json(&load_credentials())
    }

    pub fn set_active_llm_extra_headers_json(value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let headers = parse_extra_headers_json(value)?;
        let mut root = load_credentials_for_update()?;
        let entry = root.providers.llm.entry(root.active.llm.clone()).or_default();
        entry.extraHeaders = if headers.is_empty() {
            None
        } else {
            Some(headers)
        };
        save_credentials(&root)
    }

    pub fn snapshot() -> CredentialsSnapshot {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        CredentialsSnapshot {
            volcengine_app_key: lookup_account(&root, CredentialAccount::VolcengineAppKey),
            volcengine_access_key: lookup_account(&root, CredentialAccount::VolcengineAccessKey),
            volcengine_resource_id: lookup_account(&root, CredentialAccount::VolcengineResourceId),
            asr_api_key: lookup_account(&root, CredentialAccount::AsrApiKey),
            asr_endpoint: lookup_account(&root, CredentialAccount::AsrEndpoint),
            asr_model: lookup_account(&root, CredentialAccount::AsrModel),
            ark_api_key: lookup_account(&root, CredentialAccount::ArkApiKey),
            ark_model_id: lookup_account(&root, CredentialAccount::ArkModelId),
            ark_endpoint: lookup_account(&root, CredentialAccount::ArkEndpoint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        android_persistable_credentials, chunk_json_payload, credentials_cache,
        get_android_marketplace_token_at, load_android_credentials_from_path,
        load_android_credentials_from_path_with_crypto, load_android_credentials_into_cache_with,
        lookup_marketplace_github_token, parse_extra_headers_json,
        reset_credentials_cache_for_tests, write_marketplace_github_token, CredsRoot,
        MarketplaceGithubToken, KEYRING_CHUNK_MAX_UTF16_UNITS,
    };
    #[cfg(not(windows))]
    use super::load_android_credentials_from_source_with_crypto;
    use anyhow::anyhow;
    use parking_lot::Mutex;

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
        assert!(load_android_credentials_from_path_with_crypto(&destination_path, &mut crypto)
            .unwrap()
            .is_some());
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
        crypto.fail_next_seal = Some(
            super::super::android_credentials::CryptoErrorKind::TemporarilyUnavailable,
        );

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
        let first = load_android_credentials_into_cache_with(|| {
            Err(anyhow!("injected startup scrub failure"))
        });
        assert!(lookup_marketplace_github_token(&first).is_none());
        assert!(credentials_cache().lock().is_none());

        let dir =
            std::env::temp_dir().join(format!("openless-android-startup-{}", uuid::Uuid::new_v4()));
        let path = dir.join("credentials.enc.json");
        write_legacy_android_envelope(&path, "gho_legacy_startup_secret");
        let second =
            load_android_credentials_into_cache_with(|| load_android_credentials_from_path(&path));

        assert!(lookup_marketplace_github_token(&second).is_none());
        assert!(credentials_cache().lock().is_some());
        assert_android_secret_unrecoverable(&path, "gho_legacy_startup_secret");
        *credentials_cache().lock() = Some(CredsRoot::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
