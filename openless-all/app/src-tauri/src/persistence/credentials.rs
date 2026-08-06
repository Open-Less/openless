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
    /// 多模态识别管线（issue #902）专用凭据命名空间，与 asr/llm 完全隔离：
    /// 运行时只在 `pipeline_mode == multimodal` 时读取，切换模式不删除。
    #[serde(default)]
    omni: CredsOmni,
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
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct CredsOmni {
    #[serde(default = "creds_default_omni")]
    active: String,
    #[serde(default)]
    providers: HashMap<String, CredsOmniEntry>,
}

fn creds_default_omni() -> String {
    "custom".into()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsOmniEntry {
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
#[derive(Debug, Serialize, Deserialize, Clone)]
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
/// 禁用状态，`sync_active_channels` 直接忽略它，表现为"填了 key 却不生效"。
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

/// 「测试连通」的结果，持久化以便重启后仍能看到上次测试的延迟。
/// `error` 同时承担 P0 的失败标红（测试失败）与 P2 的运行时失败标红。
#[derive(Debug, Serialize, Deserialize, Clone)]
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
    /// 方舟（Ark）API Key —— 仅 `api_key` 鉴权模式使用，与旧版 Access Token 槽位
    /// (`accessKey`) 隔离，避免两模式切换时残留凭据互相污染。
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
            && self.authMode.as_deref().unwrap_or("").is_empty()
            && self.volcengineApiKey.as_deref().unwrap_or("").is_empty()
            && self.vocabularyId.as_deref().unwrap_or("").is_empty()
            && self.advancedConfig.as_deref().unwrap_or("").is_empty()
            && self.xfyunAppId.as_deref().unwrap_or("").is_empty()
            && self.xfyunApiKey.as_deref().unwrap_or("").is_empty()
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
}

impl CredsLlmEntry {
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

/// 「当前使用」= 启用渠道里 order 最小的那个；order 相同则按 id 字母序，保证确定性。
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
///      一个字节都不变，重复执行结果完全一致（新建卡片才用 uuid）。
///   2. 已带 `providerType` 的 entry 一律跳过。
///
/// order 按「原 active 排第一，其余按 id 字母序」分配。用字母序而不是 preset 表顺序，
/// 是因为后端不知道前端 LLM_PRESETS / ASR_PRESETS 的排列，而字母序是确定的。
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

    asr_changed || llm_changed || seeded
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

/// 把 `active.asr` / `active.llm` 重算成"启用列表的第一个渠道 id"。
///
/// `active` 字段在渠道化后不再是用户直接选择的厂商，而是排序与开关的**派生结果**；
/// `lookup_account` / `write_account` 仍然读它，因此每次改动排序、开关或删除渠道后
/// 都必须调用本函数，否则会出现"列表第一张是 A、实际请求打的是 B"。
///
/// 一个渠道都没启用时清空 active —— 让 `lookup_account` 落到 `None`（未配置），
/// 而不是保留指向已禁用渠道的旧 id（entry 仍在，运行时照常读得到凭据）。
fn sync_active_channels(root: &mut CredsRoot) {
    match current_channel_id(&root.providers.asr) {
        Some(id) => root.active.asr = id,
        // 全部禁用时**清空**而不是保留旧 id：旧 id 对应的 entry 还在（只是 enabled
        // 为 false），`lookup_account` 会命中它，运行时就会继续用已禁用渠道的凭据，
        // 与「第一个启用的 = 当前生效」的心智相悖。清空后 lookup 落到 None（未配置）。
        None => root.active.asr.clear(),
    }
    match current_channel_id(&root.providers.llm) {
        Some(id) => root.active.llm = id,
        None => root.active.llm.clear(),
    }
}

fn active_llm_extra_headers(root: &CredsRoot) -> HashMap<String, String> {
    root.providers
        .llm
        .get(&root.active.llm)
        .and_then(|entry| entry.extraHeaders.clone())
        .unwrap_or_default()
}

fn active_omni_extra_headers(root: &CredsRoot) -> HashMap<String, String> {
    root.omni
        .providers
        .get(&root.omni.active)
        .and_then(|entry| entry.extraHeaders.clone())
        .unwrap_or_default()
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

fn active_omni_extra_headers_json(root: &CredsRoot) -> Result<Option<String>> {
    let headers = active_omni_extra_headers(root);
    if headers.is_empty() {
        return Ok(None);
    }
    let ordered = headers.into_iter().collect::<BTreeMap<_, _>>();
    serde_json::to_string(&ordered)
        .map(Some)
        .context("encode omni extra headers")
}

fn active_omni_temperature_value(root: &CredsRoot) -> Option<f64> {
    root.omni
        .providers
        .get(&root.omni.active)
        .and_then(|entry| entry.temperature)
        .filter(|temperature| is_valid_llm_temperature(*temperature))
}

fn active_omni_temperature(root: &CredsRoot) -> Option<f32> {
    active_omni_temperature_value(root).map(|temperature| temperature as f32)
}

fn active_omni_temperature_string(root: &CredsRoot) -> Option<String> {
    active_omni_temperature_value(root).map(|temperature| temperature.to_string())
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

/// 读凭据并就地补成渠道卡片。
///
/// 迁移**只在内存里做，不主动落盘**：`migrate_channels` 是幂等的（id 沿用原 preset
/// id，不生成 uuid），所以每次读的结果都一致；而启动时写 keyring 会在 macOS 上触发
/// 「OpenLess 想使用钥匙串」的 ACL 弹窗。留给下一次真实写入（用户改配置）顺带固化。
fn load_credentials() -> CredsRoot {
    let mut root = load_credentials_raw();
    migrate_channels(&mut root);
    sync_active_channels(&mut root);
    root
}

fn load_credentials_for_update() -> Result<CredsRoot> {
    let mut root = load_credentials_for_update_raw()?;
    migrate_channels(&mut root);
    sync_active_channels(&mut root);
    Ok(root)
}

fn load_credentials_raw() -> CredsRoot {
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

fn load_credentials_for_update_raw() -> Result<CredsRoot> {
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
    let mut cleaned = clean_credentials(root);
    // 落盘的 active 必须与"启用列表第一个"一致：删除或关闭当前渠道后若不重算，
    // 磁盘上会留下指向已消失渠道的 active，下次冷启动直接读成"未配置"。
    sync_active_channels(&mut cleaned);
    let cleaned = cleaned;

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
    let omni = root.omni.providers.get(&root.omni.active);
    let pick = |s: &Option<String>| s.as_ref().filter(|v| !v.is_empty()).cloned();
    match account {
        CredentialAccount::VolcengineAppKey => {
            asr.and_then(|e| pick(&e.appKey).or_else(|| pick(&e.apiKey)))
        }
        CredentialAccount::VolcengineAccessKey => asr.and_then(|e| pick(&e.accessKey)),
        CredentialAccount::VolcengineResourceId => asr.and_then(|e| pick(&e.resourceId)),
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
        CredentialAccount::OmniApiKey => omni.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::OmniEndpoint => omni.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::OmniModel => omni.and_then(|e| pick(&e.model)),
    }
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
    VolcengineAuthMode,
    /// 方舟（Ark）语音模型 API Key（`api_key` 鉴权模式使用，独立于旧版 Access Token 槽位）。
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
    pub volcengine_auth_mode: Option<String>,
    pub volcengine_api_key: Option<String>,
    pub asr_api_key: Option<String>,
    pub asr_endpoint: Option<String>,
    pub asr_model: Option<String>,
    pub xfyun_app_id: Option<String>,
    pub xfyun_api_key: Option<String>,
    pub ark_api_key: Option<String>,
    pub ark_model_id: Option<String>,
    pub ark_endpoint: Option<String>,
    pub active_omni_provider: String,
    pub omni_api_key: Option<String>,
    pub omni_endpoint: Option<String>,
    pub omni_model: Option<String>,
}

/// 渠道所属的功能面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Asr,
    Llm,
}

impl ChannelKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "asr" => Ok(ChannelKind::Asr),
            "llm" => Ok(ChannelKind::Llm),
            other => anyhow::bail!("unknown channel kind: {other}"),
        }
    }
}

/// 一张渠道卡片对前端的投影。凭据本身不在这里 —— 前端按 id 走
/// `read_credential(account, provider = id)` 单独取，避免密钥随列表批量出栈。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSummary {
    pub id: String,
    /// 用户取的名字；空字符串表示未命名，由前端回落到 preset 显示名。
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub order: u32,
    pub last_test: Option<ChannelTestSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTestSummary {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub at: i64,
    pub error: Option<String>,
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

/// 生成一个未被占用的渠道 id：首选厂商 id 本身，冲突则 `-2` / `-3` 递增。
///
/// 刻意不用 uuid：第一张卡片的 id 就等于 preset id，与 `migrate_channel_map`
/// 的"沿用原 preset id"完全一致；credentials.json 排障时也一眼能看懂是哪家。
fn allocate_channel_id<V>(map: &HashMap<String, V>, provider_type: &str) -> String {
    if !map.contains_key(provider_type) {
        return provider_type.to_string();
    }
    for suffix in 2..u32::MAX {
        let candidate = format!("{provider_type}-{suffix}");
        if !map.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("channel id space exhausted")
}

/// 关闭渠道时把它排到末尾，重新打开时排到**启用组**末尾。
///
/// 重新打开不回原位是刻意的：回原位要额外持久化"关闭前的位置"，而用户重开一张卡片
/// 通常就是想试试它，落到启用组末尾最不打扰当前生效的渠道。
fn reposition_after_toggle<V: HasChannelMeta>(map: &mut HashMap<String, V>, id: &str) {
    let Some(enabled) = map.get(id).map(|entry| entry.meta().enabled) else {
        return;
    };
    let target = if enabled {
        // 启用组末尾 = 最大的启用 order + 1（不含自己）。
        map.iter()
            .filter(|(key, entry)| key.as_str() != id && entry.meta().enabled)
            .filter_map(|(_, entry)| entry.meta().order)
            .max()
            .map(|max| max.saturating_add(1))
            .unwrap_or(0)
    } else {
        // 整个列表末尾。
        map.iter()
            .filter(|(key, _)| key.as_str() != id)
            .filter_map(|(_, entry)| entry.meta().order)
            .max()
            .map(|max| max.saturating_add(1))
            .unwrap_or(0)
    };
    if let Some(entry) = map.get_mut(id) {
        entry.meta_mut().order = Some(target);
    }
    // 关掉的那张要沉到所有启用项之后：把启用项整体前移，重新压实 order。
    compact_orders(map);
}

/// 重排 order 为 0..n 的连续整数，顺序为「启用项在前（按原 order），禁用项在后」。
fn compact_orders<V: HasChannelMeta>(map: &mut HashMap<String, V>) {
    let mut ids: Vec<String> = map.keys().cloned().collect();
    ids.sort_by(|left, right| {
        let left_meta = map.get(left).map(|e| e.meta());
        let right_meta = map.get(right).map(|e| e.meta());
        let key_of = |meta: Option<&ChannelMeta>| {
            let meta = meta.expect("id came from this map");
            // false < true：启用的排前面。
            (!meta.enabled, meta.order.unwrap_or(u32::MAX))
        };
        key_of(left_meta)
            .cmp(&key_of(right_meta))
            .then_with(|| left.cmp(right))
    });
    for (index, id) in ids.iter().enumerate() {
        if let Some(entry) = map.get_mut(id) {
            entry.meta_mut().order = Some(index as u32);
        }
    }
}

/// 新建渠道的 order：排到启用组末尾（禁用项始终在其后，由 compact_orders 保证）。
fn next_order<V: HasChannelMeta>(map: &HashMap<String, V>) -> u32 {
    map.values()
        .filter(|entry| entry.meta().enabled)
        .filter_map(|entry| entry.meta().order)
        .max()
        .map(|max| max.saturating_add(1))
        .unwrap_or(0)
}

/// 按前端给的 id 顺序重排；未提及的渠道保持在末尾（相对顺序不变）。
fn apply_order<V: HasChannelMeta>(map: &mut HashMap<String, V>, ordered_ids: &[String]) {
    for (index, id) in ordered_ids.iter().enumerate() {
        if let Some(entry) = map.get_mut(id) {
            entry.meta_mut().order = Some(index as u32);
        }
    }
    // 没被提到的排到末尾，避免与显式序号撞车。
    let tail_base = ordered_ids.len() as u32;
    let unlisted: Vec<String> = map
        .keys()
        .filter(|id| !ordered_ids.contains(id))
        .cloned()
        .collect();
    for (offset, id) in unlisted.iter().enumerate() {
        if let Some(entry) = map.get_mut(id) {
            entry.meta_mut().order = Some(tail_base.saturating_add(offset as u32));
        }
    }
    // 拖拽后禁用项仍须沉底。
    compact_orders(map);
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

    // ---- 渠道卡片管理 ----

    pub fn list_channels(kind: ChannelKind) -> Vec<ChannelSummary> {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        match kind {
            ChannelKind::Asr => channel_summaries(&root.providers.asr, |entry| {
                entry.displayName.clone().unwrap_or_default()
            }),
            ChannelKind::Llm => channel_summaries(&root.providers.llm, |entry| {
                entry.displayName.clone().unwrap_or_default()
            }),
        }
    }

    /// 新建一张渠道卡片，返回分配到的 id。新卡片排在**启用组末尾**。
    pub fn create_channel(kind: ChannelKind, provider_type: &str, name: &str) -> Result<String> {
        let provider_type = provider_type.trim();
        if provider_type.is_empty() {
            anyhow::bail!("provider type cannot be empty");
        }
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let name = name.trim();

        let id = match kind {
            ChannelKind::Asr => {
                let id = allocate_channel_id(&root.providers.asr, provider_type);
                root.providers.asr.insert(
                    id.clone(),
                    CredsAsrEntry {
                        channel: ChannelMeta {
                            providerType: Some(provider_type.to_string()),
                            order: Some(next_order(&root.providers.asr)),
                            enabled: true,
                            lastTest: None,
                        },
                        displayName: (!name.is_empty()).then(|| name.to_string()),
                        ..Default::default()
                    },
                );
                // 存在禁用项时 `next_order`（启用项 max + 1）可能与其 order 同号，
                // 列表排序会按 id 字母序把它们混排、破坏「禁用沉底」。压实成 0..n。
                compact_orders(&mut root.providers.asr);
                id
            }
            ChannelKind::Llm => {
                let id = allocate_channel_id(&root.providers.llm, provider_type);
                root.providers.llm.insert(
                    id.clone(),
                    CredsLlmEntry {
                        channel: ChannelMeta {
                            providerType: Some(provider_type.to_string()),
                            order: Some(next_order(&root.providers.llm)),
                            enabled: true,
                            lastTest: None,
                        },
                        displayName: (!name.is_empty()).then(|| name.to_string()),
                        ..Default::default()
                    },
                );
                compact_orders(&mut root.providers.llm);
                id
            }
        };

        save_credentials(&root)?;
        Ok(id)
    }

    /// 改一张卡片的厂商。
    ///
    /// 「添加渠道」被合并成单个弹窗后，用户是在**已经建好的草稿卡片上**换供应商的，
    /// 所以这不是内部细节而是常规操作。旧厂商的凭据字段留着不动：不同厂商用不同的
    /// 凭据槽（volcengine.* / xfyun.* / asr.*），互不覆盖，换回去时原样还在。
    pub fn set_channel_provider_type(
        kind: ChannelKind,
        id: &str,
        provider_type: &str,
    ) -> Result<()> {
        let provider_type = provider_type.trim();
        if provider_type.is_empty() {
            anyhow::bail!("provider type cannot be empty");
        }
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let meta = match kind {
            ChannelKind::Asr => root
                .providers
                .asr
                .get_mut(id)
                .map(|entry| entry.meta_mut())
                .with_context(|| format!("unknown ASR channel: {id}"))?,
            ChannelKind::Llm => root
                .providers
                .llm
                .get_mut(id)
                .map(|entry| entry.meta_mut())
                .with_context(|| format!("unknown LLM channel: {id}"))?,
        };
        meta.providerType = Some(provider_type.to_string());
        // 换了厂商，之前那次测试结果就不再代表这张卡片了。
        meta.lastTest = None;
        save_credentials(&root)
    }

    /// 回收一张「什么都没填」的草稿渠道，返回是否真的删了。
    ///
    /// 单弹窗流程下，点开「添加渠道」就会先建一张草稿卡片（凭据必须按渠道 id 写入，
    /// 没有 id 就没处可写）。用户什么都没填就关掉弹窗时用这个把草稿收走，
    /// 免得列表里留下一张空卡片。填过任何一个字段就保留。
    pub fn delete_channel_if_blank(kind: ChannelKind, id: &str) -> Result<bool> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let blank = match kind {
            ChannelKind::Asr => root
                .providers
                .asr
                .get(id)
                .map(|entry| entry.has_no_content())
                .unwrap_or(false),
            ChannelKind::Llm => root
                .providers
                .llm
                .get(id)
                .map(|entry| entry.has_no_content())
                .unwrap_or(false),
        };
        if !blank {
            return Ok(false);
        }
        match kind {
            ChannelKind::Asr => {
                root.providers.asr.remove(id);
                compact_orders(&mut root.providers.asr);
            }
            ChannelKind::Llm => {
                root.providers.llm.remove(id);
                compact_orders(&mut root.providers.llm);
            }
        }
        save_credentials(&root)?;
        Ok(true)
    }

    pub fn rename_channel(kind: ChannelKind, id: &str, name: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let name = name.trim();
        let name = (!name.is_empty()).then(|| name.to_string());
        match kind {
            ChannelKind::Asr => {
                let entry = root
                    .providers
                    .asr
                    .get_mut(id)
                    .with_context(|| format!("unknown ASR channel: {id}"))?;
                entry.displayName = name;
            }
            ChannelKind::Llm => {
                let entry = root
                    .providers
                    .llm
                    .get_mut(id)
                    .with_context(|| format!("unknown LLM channel: {id}"))?;
                entry.displayName = name;
            }
        }
        save_credentials(&root)
    }

    pub fn delete_channel(kind: ChannelKind, id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        match kind {
            ChannelKind::Asr => {
                root.providers
                    .asr
                    .remove(id)
                    .with_context(|| format!("unknown ASR channel: {id}"))?;
                compact_orders(&mut root.providers.asr);
            }
            ChannelKind::Llm => {
                root.providers
                    .llm
                    .remove(id)
                    .with_context(|| format!("unknown LLM channel: {id}"))?;
                compact_orders(&mut root.providers.llm);
            }
        }
        // save_credentials 内部会 sync_active_channels，把 active 顺延到下一张。
        save_credentials(&root)
    }

    pub fn set_channel_enabled(kind: ChannelKind, id: &str, enabled: bool) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        match kind {
            ChannelKind::Asr => {
                let entry = root
                    .providers
                    .asr
                    .get_mut(id)
                    .with_context(|| format!("unknown ASR channel: {id}"))?;
                entry.channel.enabled = enabled;
                reposition_after_toggle(&mut root.providers.asr, id);
            }
            ChannelKind::Llm => {
                let entry = root
                    .providers
                    .llm
                    .get_mut(id)
                    .with_context(|| format!("unknown LLM channel: {id}"))?;
                entry.channel.enabled = enabled;
                reposition_after_toggle(&mut root.providers.llm, id);
            }
        }
        save_credentials(&root)
    }

    /// 按前端给的完整 id 顺序重排。列表里没提到的渠道保持在末尾。
    pub fn reorder_channels(kind: ChannelKind, ordered_ids: &[String]) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        match kind {
            ChannelKind::Asr => apply_order(&mut root.providers.asr, ordered_ids),
            ChannelKind::Llm => apply_order(&mut root.providers.llm, ordered_ids),
        }
        save_credentials(&root)
    }

    /// 记录一次「测试连通」的结果。
    pub fn record_channel_test(
        kind: ChannelKind,
        id: &str,
        ok: bool,
        latency_ms: Option<u32>,
        at: i64,
        error: Option<String>,
    ) -> Result<()> {
        let test = ChannelTest {
            ok,
            latencyMs: latency_ms,
            at,
            error,
        };
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        match kind {
            ChannelKind::Asr => {
                let entry = root
                    .providers
                    .asr
                    .get_mut(id)
                    .with_context(|| format!("unknown ASR channel: {id}"))?;
                entry.channel.lastTest = Some(test);
            }
            ChannelKind::Llm => {
                let entry = root
                    .providers
                    .llm
                    .get_mut(id)
                    .with_context(|| format!("unknown LLM channel: {id}"))?;
                entry.channel.lastTest = Some(test);
            }
        }
        save_credentials(&root)
    }

    /// 某张卡片的厂商 id。「测试连通」要按用户点的那张卡片决定协议，而不是当前生效的那张。
    pub fn get_channel_provider_type(kind: ChannelKind, id: &str) -> Option<String> {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        match kind {
            ChannelKind::Asr => root
                .providers
                .asr
                .get(id)
                .map(|entry| channel_provider_type(id, entry).to_string()),
            ChannelKind::Llm => root
                .providers
                .llm
                .get(id)
                .map(|entry| channel_provider_type(id, entry).to_string()),
        }
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
        let mut root = load_credentials();
        root.active.llm = id.to_string();
        Ok(lookup_account(&root, account))
    }

    pub fn set_for_llm_provider(id: &str, account: CredentialAccount, value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let active = root.active.llm.clone();
        root.active.llm = id.to_string();
        let value = (!value.is_empty()).then(|| value.to_string());
        write_account(&mut root, account, value);
        root.active.llm = active;
        save_credentials(&root)
    }

    pub fn get_active_omni() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().omni.active
    }

    pub fn set_active_omni_provider(id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.omni.active = id.to_string();
        save_credentials(&root)
    }

    pub fn get_active_omni_extra_headers() -> HashMap<String, String> {
        let _guard = credentials_lock().lock();
        active_omni_extra_headers(&load_credentials())
    }

    pub fn get_active_omni_extra_headers_json() -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        active_omni_extra_headers_json(&load_credentials())
    }

    pub fn get_active_omni_temperature() -> Option<f32> {
        let _guard = credentials_lock().lock();
        active_omni_temperature(&load_credentials())
    }

    pub fn get_active_omni_temperature_string() -> Option<String> {
        let _guard = credentials_lock().lock();
        active_omni_temperature_string(&load_credentials())
    }

    pub fn set_active_omni_temperature(value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let temperature = parse_llm_temperature(value)?;
        let mut root = load_credentials_for_update()?;
        let entry = root
            .omni
            .providers
            .entry(root.omni.active.clone())
            .or_default();
        entry.temperature = temperature;
        save_credentials(&root)
    }

    pub fn set_active_omni_extra_headers_json(value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let headers = parse_extra_headers_json(value)?;
        let mut root = load_credentials_for_update()?;
        let entry = root
            .omni
            .providers
            .entry(root.omni.active.clone())
            .or_default();
        entry.extraHeaders = if headers.is_empty() {
            None
        } else {
            Some(headers)
        };
        save_credentials(&root)
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
        let _guard = credentials_lock().lock();
        let temperature = parse_llm_temperature(value)?;
        let mut root = load_credentials_for_update()?;
        let entry = root
            .providers
            .llm
            .entry(root.active.llm.clone())
            .or_default();
        entry.temperature = temperature;
        save_credentials(&root)
    }

    pub fn set_active_llm_extra_headers_json(value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let headers = parse_extra_headers_json(value)?;
        let mut root = load_credentials_for_update()?;
        let entry = root
            .providers
            .llm
            .entry(root.active.llm.clone())
            .or_default();
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
            volcengine_auth_mode: lookup_account(&root, CredentialAccount::VolcengineAuthMode),
            volcengine_api_key: lookup_account(&root, CredentialAccount::VolcengineApiKey),
            asr_api_key: lookup_account(&root, CredentialAccount::AsrApiKey),
            asr_endpoint: lookup_account(&root, CredentialAccount::AsrEndpoint),
            asr_model: lookup_account(&root, CredentialAccount::AsrModel),
            xfyun_app_id: lookup_account(&root, CredentialAccount::XfyunAppId),
            xfyun_api_key: lookup_account(&root, CredentialAccount::XfyunApiKey),
            ark_api_key: lookup_account(&root, CredentialAccount::ArkApiKey),
            ark_model_id: lookup_account(&root, CredentialAccount::ArkModelId),
            ark_endpoint: lookup_account(&root, CredentialAccount::ArkEndpoint),
            active_omni_provider: root.omni.active.clone(),
            omni_api_key: lookup_account(&root, CredentialAccount::OmniApiKey),
            omni_endpoint: lookup_account(&root, CredentialAccount::OmniEndpoint),
            omni_model: lookup_account(&root, CredentialAccount::OmniModel),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::load_android_credentials_from_source_with_crypto;
    use super::{
        android_persistable_credentials, chunk_json_payload, credentials_cache,
        get_android_marketplace_token_at, load_android_credentials_from_path,
        load_android_credentials_from_path_with_crypto, load_android_credentials_into_cache_with,
        lookup_account, lookup_marketplace_github_token, parse_extra_headers_json,
        parse_llm_temperature, reset_credentials_cache_for_tests, write_account,
        write_marketplace_github_token, CredentialAccount, CredsAsrEntry, CredsRoot,
        CredsLlmEntry, MarketplaceGithubToken, KEYRING_CHUNK_MAX_UTF16_UNITS,
    };
    use anyhow::anyhow;
    use parking_lot::Mutex;
    use std::collections::HashMap;

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
        super::sync_active_channels(&mut root);

        // 迁移后凭据读取行为不变 —— 这是老用户升级不炸的底线。
        assert_eq!(
            lookup_account(&root, CredentialAccount::VolcengineAppKey).as_deref(),
            Some("vk")
        );
    }

    #[test]
    fn active_follows_order_and_enabled_not_user_choice() {
        let mut root = v1_root_with_two_asr_providers();
        super::migrate_channels(&mut root);

        // 把 groq 拖到第一。
        root.providers.asr.get_mut("groq").unwrap().channel.order = Some(0);
        root.providers
            .asr
            .get_mut("volcengine")
            .unwrap()
            .channel
            .order = Some(1);
        super::sync_active_channels(&mut root);
        assert_eq!(root.active.asr, "groq");

        // 关掉 groq 后，当前渠道顺延到下一个启用的。
        root.providers.asr.get_mut("groq").unwrap().channel.enabled = false;
        super::sync_active_channels(&mut root);
        assert_eq!(root.active.asr, "volcengine");
    }

    #[test]
    fn every_channel_disabled_clears_active_so_lookup_reports_unconfigured() {
        let mut root = v1_root_with_two_asr_providers();
        super::migrate_channels(&mut root);
        for entry in root.providers.asr.values_mut() {
            entry.channel.enabled = false;
        }
        super::sync_active_channels(&mut root);
        // 清空而不是保留旧 id：entry 还在，保留会让 lookup 继续命中已禁用渠道。
        assert_eq!(root.active.asr, "");
        assert_eq!(
            lookup_account(&root, CredentialAccount::VolcengineAppKey),
            None
        );
    }

    #[test]
    fn every_llm_channel_disabled_clears_active_so_lookup_reports_unconfigured() {
        let mut root = CredsRoot::default();
        root.active.llm = "ark".into();
        root.providers.llm.insert(
            "ark".into(),
            CredsLlmEntry {
                apiKey: Some("sk-ark".into()),
                ..Default::default()
            },
        );
        super::migrate_channels(&mut root);
        for entry in root.providers.llm.values_mut() {
            entry.channel.enabled = false;
        }
        super::sync_active_channels(&mut root);
        assert_eq!(root.active.llm, "");
        assert_eq!(lookup_account(&root, CredentialAccount::ArkApiKey), None);
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
        super::sync_active_channels(&mut root);

        // 迁移只写 providerType / order，凭据一个字节都不动。
        assert_eq!(
            root.providers.asr.get("volcengine").unwrap().appKey.as_deref(),
            Some("vk")
        );
        assert_eq!(
            root.providers.asr.get("volcengine").unwrap().accessKey.as_deref(),
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
        super::sync_active_channels(&mut root);

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

    // ---- 排序 / 开关 ----

    /// 造一组 ASR 渠道：`(id, order, enabled)`。
    fn channels(spec: &[(&str, u32, bool)]) -> HashMap<String, CredsAsrEntry> {
        spec.iter()
            .map(|(id, order, enabled)| {
                (
                    (*id).to_string(),
                    CredsAsrEntry {
                        channel: super::ChannelMeta {
                            providerType: Some((*id).to_string()),
                            order: Some(*order),
                            enabled: *enabled,
                            lastTest: None,
                        },
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    /// 按 order 升序取出 `(id, enabled)`，用来断言列表的可见顺序。
    fn ordered(map: &HashMap<String, CredsAsrEntry>) -> Vec<(String, bool)> {
        let mut list: Vec<_> = map
            .iter()
            .map(|(id, entry)| {
                (
                    id.clone(),
                    entry.channel.enabled,
                    entry.channel.order.unwrap_or(u32::MAX),
                )
            })
            .collect();
        list.sort_by(|left, right| left.2.cmp(&right.2).then_with(|| left.0.cmp(&right.0)));
        list.into_iter()
            .map(|(id, enabled, _)| (id, enabled))
            .collect()
    }

    #[test]
    fn disabling_a_channel_sinks_it_below_every_enabled_one() {
        let mut map = channels(&[("a", 0, true), ("b", 1, true), ("c", 2, true)]);
        map.get_mut("a").unwrap().channel.enabled = false;
        super::reposition_after_toggle(&mut map, "a");

        assert_eq!(
            ordered(&map),
            vec![("b".into(), true), ("c".into(), true), ("a".into(), false),]
        );
    }

    #[test]
    fn re_enabling_a_channel_lands_at_the_end_of_the_enabled_group() {
        let mut map = channels(&[("a", 0, true), ("b", 1, true), ("c", 2, false)]);
        map.get_mut("c").unwrap().channel.enabled = true;
        super::reposition_after_toggle(&mut map, "c");

        // 不回原位、也不抢第一 —— 落到启用组末尾，不打扰当前生效的 a。
        assert_eq!(
            ordered(&map),
            vec![("a".into(), true), ("b".into(), true), ("c".into(), true)]
        );
    }

    #[test]
    fn compact_orders_keeps_disabled_channels_at_the_bottom() {
        let mut map = channels(&[("a", 5, false), ("b", 9, true), ("c", 1, true)]);
        super::compact_orders(&mut map);

        assert_eq!(
            ordered(&map),
            vec![("c".into(), true), ("b".into(), true), ("a".into(), false),]
        );
        // order 压实成 0..n，避免反复拖拽后数值发散。
        let mut orders: Vec<u32> = map
            .values()
            .map(|entry| entry.channel.order.unwrap())
            .collect();
        orders.sort_unstable();
        assert_eq!(orders, vec![0, 1, 2]);
    }

    #[test]
    fn reorder_puts_the_dragged_channel_first_and_drives_active() {
        let mut root = CredsRoot::default();
        root.providers.asr = channels(&[("a", 0, true), ("b", 1, true)]);
        super::apply_order(&mut root.providers.asr, &["b".to_string(), "a".to_string()]);
        super::sync_active_channels(&mut root);

        assert_eq!(ordered(&root.providers.asr)[0].0, "b");
        assert_eq!(root.active.asr, "b");
    }

    #[test]
    fn reorder_tolerates_ids_the_frontend_did_not_mention() {
        let mut map = channels(&[("a", 0, true), ("b", 1, true), ("c", 2, true)]);
        // 前端只发了两个 id（比如 c 是刚被另一个窗口加进来的）。
        super::apply_order(&mut map, &["c".to_string(), "a".to_string()]);

        let order = ordered(&map);
        assert_eq!(order[0].0, "c");
        assert_eq!(order[1].0, "a");
        // 没提到的 b 落到末尾而不是消失或撞车。
        assert_eq!(order[2].0, "b");
    }

    #[test]
    fn new_channel_id_falls_back_to_numbered_suffix_for_same_provider() {
        let mut map = channels(&[("deepseek", 0, true)]);
        let second = super::allocate_channel_id(&map, "deepseek");
        assert_eq!(second, "deepseek-2");

        map.insert(second, Default::default());
        assert_eq!(super::allocate_channel_id(&map, "deepseek"), "deepseek-3");
        // 不同厂商仍拿到干净的 id。
        assert_eq!(super::allocate_channel_id(&map, "groq"), "groq");
    }

    #[test]
    fn new_channel_lands_at_the_end_of_the_enabled_group() {
        // 禁用项的 order 更大，但新卡片要排在启用组末尾，而不是整个列表末尾。
        let map = channels(&[("a", 0, true), ("b", 1, true), ("c", 2, false)]);
        assert_eq!(super::next_order(&map), 2);
    }

    #[test]
    fn create_channel_with_disabled_present_keeps_disabled_at_the_bottom() {
        // 与 `create_channel` 相同的路径：allocate → insert（order = next_order）
        // → compact_orders。修复前新卡与禁用项 `c` 同 order，列表会按 id 字母序
        // 混排；压实后新启用卡在启用组末尾、禁用项仍沉底。
        let mut root = CredsRoot::default();
        root.providers.asr = channels(&[("a", 0, true), ("b", 1, true), ("c", 2, false)]);
        let id = super::allocate_channel_id(&root.providers.asr, "deepseek");
        root.providers.asr.insert(
            id.clone(),
            CredsAsrEntry {
                channel: super::ChannelMeta {
                    providerType: Some("deepseek".into()),
                    order: Some(super::next_order(&root.providers.asr)),
                    enabled: true,
                    lastTest: None,
                },
                ..Default::default()
            },
        );
        super::compact_orders(&mut root.providers.asr);

        assert_eq!(
            ordered(&root.providers.asr),
            vec![
                ("a".into(), true),
                ("b".into(), true),
                ("deepseek".into(), true),
                ("c".into(), false),
            ]
        );
        // order 连续无重复，杜绝与禁用项同号。
        let mut orders: Vec<u32> = root
            .providers
            .asr
            .values()
            .map(|entry| entry.channel.order.unwrap())
            .collect();
        orders.sort_unstable();
        assert_eq!(orders, vec![0, 1, 2, 3]);
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
        super::sync_active_channels(&mut root);
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
