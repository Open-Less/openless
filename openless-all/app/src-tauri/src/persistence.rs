//! Local persistence: history JSON, user preferences JSON, vocab JSON, and
//! platform-backed credentials vault.
//!
//! Storage roots:
//! - macOS:   `~/Library/Application Support/OpenLess`
//! - Windows: `%APPDATA%\OpenLess`
//! - Linux:   `$XDG_DATA_HOME/OpenLess` or `~/.local/share/OpenLess`
//!
//! Credential storage policy: provider credentials are stored in the OS
//! credential vault (macOS Keychain, Windows Credential Manager, Linux keyring).
//! A legacy plaintext JSON file is read once as a migration source and removed
//! after a successful vault write; new writes never persist plaintext secrets.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{DictationSession, DictionaryEntry, UserPreferences, VocabPresetStore};

const HISTORY_CAP: usize = 200;
const HISTORY_FILE: &str = "history.json";
const PREFERENCES_FILE: &str = "preferences.json";
/// 与 Swift `Sources/OpenLessPersistence/DictionaryStore.swift` 同名，
/// 让旧版词汇表在升级后无缝继承。**不要**改成 `vocab.json`，会丢用户数据。
const VOCAB_FILE: &str = "dictionary.json";
const VOCAB_PRESETS_FILE: &str = "vocab-presets.json";

/// 旧版 plaintext JSON 凭据路径。仅作为迁移来源；成功写入系统凭据库后会删除。
const LEGACY_CREDS_DIR: &str = ".openless";
const LEGACY_CREDS_FILE: &str = "credentials.json";

const KEYRING_CREDENTIALS_ACCOUNT: &str = "credentials.v1";
const KEYRING_CREDENTIALS_CHUNK_PREFIX: &str = "credentials.v1.chunk.";
// Windows Credential Manager caps one credential blob at 2560 bytes. keyring stores
// passwords as UTF-16 on Windows, so keep each JSON chunk comfortably below that.
const KEYRING_CHUNK_MAX_UTF16_UNITS: usize = 1000;

static CREDENTIALS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn credentials_lock() -> &'static Mutex<()> {
    CREDENTIALS_LOCK.get_or_init(|| Mutex::new(()))
}

// ───────────────────────── path helpers ─────────────────────────

fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("OpenLess"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        Ok(PathBuf::from(appdata).join("OpenLess"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("OpenLess"));
            }
        }
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("OpenLess"))
    }
}

fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create dir failed: {}", dir.display()))?;
    Ok(())
}

/// 本地 ASR 模型根目录：`<data_dir>/models/qwen3-asr/`。
/// 子目录 = 模型 id（如 `qwen3-asr-0.6b`），存 antirez `download_model.sh`
/// 列出的 5–7 个文件。
pub fn local_models_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("models").join("qwen3-asr");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Foundry Local 下载与缓存根目录。DLL 和模型都不打进安装包，和 Qwen3-ASR
/// 一样放在 OpenLess 的 models 目录下，卸载清理用户数据时可以一起删除。
#[cfg(target_os = "windows")]
pub fn foundry_local_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("models").join("foundry-local");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_native_runtime_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("runtime");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_model_cache_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?;
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_app_data_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("app-data");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_logs_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("logs");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Atomic write: write to `*.tmp` first, then rename onto the target path.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tmp failed: {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("rename failed: {}", path.display()))?;
    Ok(())
}

fn read_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice::<T>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))
}

// ───────────────────────── credentials vault ─────────────────────────
//
// 正常读写走系统凭据库；旧 plaintext JSON 只作为迁移来源。为保持多 provider
// schema 与 active provider 状态，凭据库里保存一个 v1 JSON payload；payload 会按平台
// 凭据库限制拆成多个条目，避免 Windows 单条凭据 2560 bytes 限制。
//
// v1 schema：
//   {
//     "version": 1,
//     "active": { "asr": "<id>", "llm": "<id>" },
//     "providers": {
//       "asr": { "<id>": { "appKey", "accessKey", "resourceId", "apiKey", "baseURL", "model" } },
//       "llm": { "<id>": { "displayName", "apiKey", "baseURL", "model", "temperature", "extraHeaders" } }
//     }
//   }
//
// "ark.api_key"/"volcengine.app_key" 等账户名按 Swift 语义路由到 active provider。

use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsRoot {
    #[serde(default = "credsroot_default_version")]
    version: u32,
    #[serde(default)]
    active: CredsActive,
    #[serde(default)]
    providers: CredsProviders,
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
}

impl CredsAsrEntry {
    fn is_empty(&self) -> bool {
        self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.appKey.as_deref().unwrap_or("").is_empty()
            && self.accessKey.as_deref().unwrap_or("").is_empty()
            && self.resourceId.as_deref().unwrap_or("").is_empty()
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

fn keyring_entry() -> Result<keyring::Entry> {
    keyring_entry_for(KEYRING_CREDENTIALS_ACCOUNT)
}

fn keyring_entry_for(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(CredentialsVault::SERVICE_NAME, account)
        .context("open system credential vault")
}

fn clean_credentials(root: &CredsRoot) -> CredsRoot {
    let mut cleaned = root.clone();
    cleaned.providers.asr.retain(|_, v| !v.is_empty());
    cleaned.providers.llm.retain(|_, v| !v.is_empty());
    cleaned
}

fn read_legacy_credentials_file(path: &Path) -> Option<CredsRoot> {
    if !path.exists() {
        return None;
    }
    let bytes = match fs::read(path) {
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
        fs::remove_file(&path)
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

fn get_keyring_password(account: &str) -> Result<Option<String>> {
    match keyring_entry_for(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            Err(anyhow!(e)).with_context(|| format!("read system credential vault {account}"))
        }
    }
}

fn delete_keyring_password(account: &str) {
    match keyring_entry_for(account).and_then(|entry| {
        entry
            .delete_credential()
            .with_context(|| format!("delete system credential vault {account}"))
    }) {
        Ok(()) | Err(_) => {}
    }
}

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

fn load_legacy_keyring_credentials() -> CredsRoot {
    match load_legacy_keyring_credentials_for_update() {
        Ok(root) => root,
        Err(e) => {
            log::warn!("[vault] read legacy vault credentials failed: {e}");
            CredsRoot::default()
        }
    }
}

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

    let legacy_vault = load_legacy_keyring_credentials();
    if legacy_vault_has_credentials(&legacy_vault) {
        return legacy_vault;
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
        remove_legacy_keyring_credentials();
        return Ok(legacy);
    }

    let legacy_vault = load_legacy_keyring_credentials_for_update()?;
    if legacy_vault_has_credentials(&legacy_vault) {
        save_credentials(&legacy_vault)?;
        remove_legacy_keyring_credentials();
        return Ok(legacy_vault);
    }

    Ok(CredsRoot::default())
}

fn load_credentials() -> CredsRoot {
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 不在这里调 remove_legacy_keyring_credentials() —— 它内部对 9 个
            // 旧 account 各做一次 keyring delete，每次 delete 在 macOS Keychain
            // 上仍要触发 ACL 检查。第一次成功 load 时 legacy entries 通常已经
            // 被 migrate_legacy_sources_for_update 清理过了；这里若再无脑跑，
            // 只会反复弹「OpenLess 想删除 X」十几次。文件 legacy（plaintext
            // JSON）不需要 ACL，可继续 best-effort 删除。
            remove_legacy_credentials_file_best_effort();
            root
        }
        Ok(None) => migrate_legacy_sources(),
        Err(e) => {
            log::warn!("[vault] system credential read failed: {e}");
            load_legacy_sources_without_migration()
        }
    }
}

fn load_credentials_for_update() -> Result<CredsRoot> {
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 同 load_credentials：不再每次 update 都尝试 delete legacy keyring
            // entries，避免反复触发 macOS Keychain ACL 弹窗。
            remove_legacy_credentials_file_best_effort();
            Ok(root)
        }
        Ok(None) => migrate_legacy_sources_for_update(),
        Err(e) => Err(e),
    }
}

fn save_credentials(root: &CredsRoot) -> Result<()> {
    let cleaned = clean_credentials(root);
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
    Ok(())
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
    }
}

// ───────────────────────── HistoryStore ─────────────────────────

pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(HISTORY_FILE),
            lock: Mutex::new(()),
        })
    }

    pub fn list(&self) -> Result<Vec<DictationSession>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn append(&self, session: DictationSession) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        // Prepend so the newest session is at index 0, matching the Swift impl.
        sessions.insert(0, session);
        if sessions.len() > HISTORY_CAP {
            sessions.truncate(HISTORY_CAP);
        }
        self.write_locked(&sessions)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        let original_len = sessions.len();
        sessions.retain(|s| s.id != id);
        if sessions.len() == original_len {
            return Ok(());
        }
        self.write_locked(&sessions)
    }

    pub fn clear(&self) -> Result<()> {
        let _guard = self.lock.lock();
        self.write_locked(&Vec::<DictationSession>::new())
    }

    fn read_locked(&self) -> Result<Vec<DictationSession>> {
        read_or_default::<Vec<DictationSession>>(&self.path)
    }

    fn write_locked(&self, sessions: &[DictationSession]) -> Result<()> {
        let json = serde_json::to_vec_pretty(sessions).context("encode history failed")?;
        atomic_write(&self.path, &json)
    }
}

// ───────────────────────── PreferencesStore ─────────────────────────

pub struct PreferencesStore {
    path: PathBuf,
    state: Mutex<UserPreferences>,
}

impl PreferencesStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(PREFERENCES_FILE);
        let prefs = if path.exists() {
            read_or_default::<UserPreferences>(&path).unwrap_or_else(|e| {
                log::warn!(
                    "[prefs] load {} failed, using defaults: {}",
                    path.display(),
                    e
                );
                UserPreferences::default()
            })
        } else {
            UserPreferences::default()
        };
        Ok(Self {
            path,
            state: Mutex::new(prefs),
        })
    }

    pub fn get(&self) -> UserPreferences {
        self.state.lock().clone()
    }

    pub fn set(&self, prefs: UserPreferences) -> Result<()> {
        let json = serde_json::to_vec_pretty(&prefs).context("encode prefs failed")?;
        atomic_write(&self.path, &json)?;
        let mut guard = self.state.lock();
        *guard = prefs;
        Ok(())
    }
}

// ───────────────────────── DictionaryStore ─────────────────────────

pub struct DictionaryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl DictionaryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(VOCAB_FILE),
            lock: Mutex::new(()),
        })
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn add(&self, phrase: String, note: Option<String>) -> Result<DictionaryEntry> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let entry = DictionaryEntry {
            id: Uuid::new_v4().to_string(),
            phrase,
            note,
            enabled: true,
            hits: 0,
            created_at: Utc::now().to_rfc3339(),
        };
        entries.insert(0, entry.clone());
        self.write_locked(&entries)?;
        Ok(entry)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Ok(());
        }
        self.write_locked(&entries)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry.id == id {
                entry.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("dictionary entry {} not found", id));
        }
        self.write_locked(&entries)
    }

    /// 扫描一段最终文本，对每个 enabled 词条按出现次数累加 `hits`。
    ///
    /// 匹配是大小写不敏感的子串扫描：「Hello hello HELLO」算 3 次。
    /// 返回本次累加的总命中数，方便调用方记录到 history.dictionary_entry_count。
    pub fn record_hits(&self, text: &str) -> Result<u64> {
        if text.is_empty() {
            return Ok(0);
        }
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        if entries.is_empty() {
            return Ok(0);
        }
        let haystack = text.to_lowercase();
        let mut total: u64 = 0;
        let mut changed = false;
        for entry in entries.iter_mut() {
            if !entry.enabled {
                continue;
            }
            let needle = entry.phrase.trim().to_lowercase();
            if needle.is_empty() {
                continue;
            }
            let count = count_occurrences(&haystack, &needle);
            if count > 0 {
                entry.hits = entry.hits.saturating_add(count);
                total = total.saturating_add(count);
                changed = true;
            }
        }
        if changed {
            self.write_locked(&entries)?;
        }
        Ok(total)
    }

    fn read_locked(&self) -> Result<Vec<DictionaryEntry>> {
        read_or_default::<Vec<DictionaryEntry>>(&self.path)
    }

    fn write_locked(&self, entries: &[DictionaryEntry]) -> Result<()> {
        let json = serde_json::to_vec_pretty(entries).context("encode vocab failed")?;
        atomic_write(&self.path, &json)
    }
}

/// 统计 `needle` 在 `haystack` 中的非重叠出现次数。两侧调用前都应已转小写。
fn count_occurrences(haystack: &str, needle: &str) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count: u64 = 0;
    let mut start = 0usize;
    while let Some(pos) = haystack[start..].find(needle) {
        count = count.saturating_add(1);
        start = start + pos + needle.len();
        if start >= haystack.len() {
            break;
        }
    }
    count
}

pub fn list_vocab_presets() -> Result<VocabPresetStore> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    read_or_default::<VocabPresetStore>(&dir.join(VOCAB_PRESETS_FILE))
}

pub fn save_vocab_presets(store: &VocabPresetStore) -> Result<()> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    let path = dir.join(VOCAB_PRESETS_FILE);
    let json = serde_json::to_vec_pretty(store).context("encode vocab presets failed")?;
    atomic_write(&path, &json)
}

// ───────────────────────── CredentialsVault ─────────────────────────

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

    pub fn remove(account: CredentialAccount) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        write_account(&mut root, account, None);
        save_credentials(&root)
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
        chunk_json_payload, list_vocab_presets, save_vocab_presets, KEYRING_CHUNK_MAX_UTF16_UNITS,
    };
    use crate::types::{VocabPreset, VocabPresetStore};
    use std::fs;
    use std::path::PathBuf;

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
    fn vocab_presets_roundtrip_json_file() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        // Linux path helper uses XDG_DATA_HOME first.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &tmp);
        }
        let store = VocabPresetStore {
            custom: vec![VocabPreset {
                id: "test".into(),
                name: "测试".into(),
                phrases: vec!["PR".into(), "CI".into()],
            }],
            overrides: vec![],
            disabled_builtin_preset_ids: vec!["chef".into()],
        };
        save_vocab_presets(&store).expect("save presets");
        let loaded = list_vocab_presets().expect("list presets");
        assert_eq!(loaded.custom.len(), 1);
        assert_eq!(loaded.custom[0].id, "test");
        assert_eq!(
            loaded.custom[0].phrases,
            vec!["PR".to_string(), "CI".to_string()]
        );
        assert_eq!(loaded.disabled_builtin_preset_ids, vec!["chef".to_string()]);
        let _ = fs::remove_dir_all(&tmp);
    }
}
