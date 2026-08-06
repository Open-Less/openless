//! Qwen3-ASR 模型下载管理 —— 并发分块 + 断点续传。
//!
//! 设计要点（与 huggingface_hub / aria2 / hf_transfer 同款）：
//! - **HTTP Range 分块**：32 MB 一块，避免长连接被 CDN 中途踢
//! - **N 并发**：4 个 worker 同时下不同 range，绕过 HF CDN 单连接限速
//! - **sparse 文件 + seek+write**：每块知道自己的 offset 直接写到位
//! - **`.partial.idx` 哨兵**：每完成一块原子追加索引；下次只下未完成的块
//! - **per-chunk retry**：4 次指数退避（1s/4s/16s）
//! - **服务端忽略 Range 返回 200 防御**：检测到非 206 直接 fail，让 retry 处理
//! - **取消尊重**：每块边界 + 每流块边界检查 AtomicBool

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::models::{model_dir, ModelId, READY_SENTINEL};

/// 进度事件最小发射间隔（毫秒）。HTTP 每 chunk 回调一次 on_progress，若全量
/// 转发，前端每秒收到上百个 IPC 事件、进度条高频刷新会「抽搐」（issue 见
/// LocalAsr 下载浮层）。按 ≥150ms 节流后肉眼平滑（约 6-7 次/秒），首条进度
/// 与 phase 事件（started/finished/cancelled/failed）不受此限。
pub(crate) const PROGRESS_EMIT_MIN_INTERVAL_MS: u64 = 150;

/// 当前 Unix 毫秒时间戳（进度节流用）。
pub(crate) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 下载源镜像。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Mirror {
    Huggingface,
    HfMirror,
}

impl Default for Mirror {
    fn default() -> Self {
        Mirror::Huggingface
    }
}

impl Mirror {
    pub fn base_url(self) -> &'static str {
        match self {
            Mirror::Huggingface => "https://huggingface.co",
            Mirror::HfMirror => "https://hf-mirror.com",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "hf-mirror" => Mirror::HfMirror,
            _ => Mirror::Huggingface,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mirror::Huggingface => "huggingface",
            Mirror::HfMirror => "hf-mirror",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub model_id: String,
    pub mirror: String,
    pub files: Vec<RemoteFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: Option<u64>,
}

pub async fn fetch_remote_info(model_id: ModelId, mirror: Mirror) -> Result<RemoteInfo> {
    let client = build_client()?;
    let files = fetch_file_list(&client, model_id.hf_repo(), mirror).await?;
    let total_bytes = files.iter().map(|f| f.size).sum();
    Ok(RemoteInfo {
        model_id: model_id.as_str().into(),
        mirror: mirror.as_str().into(),
        files,
        total_bytes,
    })
}

async fn fetch_file_list(
    client: &reqwest::Client,
    repo: &str,
    mirror: Mirror,
) -> Result<Vec<RemoteFile>> {
    let url = format!("{}/api/models/{}/tree/main", mirror.base_url(), repo);
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("HF tree API GET 失败: {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HF tree API HTTP {}: {url}", resp.status());
    }
    let entries: Vec<HfTreeEntry> = resp
        .json()
        .await
        .with_context(|| format!("HF tree JSON 解码失败: {url}"))?;
    let files: Vec<RemoteFile> = entries
        .into_iter()
        .filter(|e| e.entry_type == "file" && keep_file(&e.path))
        .map(|e| RemoteFile {
            path: e.path,
            size: e.size.unwrap_or(0),
        })
        .collect();
    if files.is_empty() {
        anyhow::bail!("HF tree 返回空文件列表 (repo={repo})");
    }
    Ok(files)
}

fn keep_file(path: &str) -> bool {
    if path.starts_with('.') {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".md")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
    {
        return false;
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(
        ext,
        "json" | "safetensors" | "txt" | "bin" | "model" | "tiktoken"
    )
}

/// HF 模型卡片（下载量 / 收藏 / 简介）——下载弹窗右侧展示用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelCard {
    pub model_id: String,
    pub mirror: String,
    pub downloads: u64,
    pub likes: u64,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct HfApiModelCard {
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "cardData")]
    card_data: Option<HfApiCardData>,
}

#[derive(Debug, Deserialize)]
struct HfApiCardData {
    #[serde(default)]
    summary: Option<String>,
}

/// 拉取 HF 模型卡片：GET `{mirror}/api/models/{repo}` 拿 downloads / likes /
/// cardData.summary。summary 缺失时回退读 README 首个非空段落当简介；
/// 描述统一截断到 [`HF_CARD_DESC_MAX_CHARS`]，防超长文本把弹窗撑爆。
pub async fn fetch_hf_card(model_id: ModelId, mirror: Mirror) -> Result<HfModelCard> {
    let client = build_client()?;
    let repo = model_id.hf_repo();
    let url = format!("{}/api/models/{}", mirror.base_url(), repo);
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("HF model card API GET 失败: {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HF model card API HTTP {}: {url}", resp.status());
    }
    let api: HfApiModelCard = resp
        .json()
        .await
        .with_context(|| format!("HF model card JSON 解码失败: {url}"))?;

    let mut description = api
        .card_data
        .as_ref()
        .and_then(|c| c.summary.clone())
        .unwrap_or_default();
    if description.trim().is_empty() {
        description = fetch_readme_first_paragraph(&client, repo, mirror).await?;
    }

    Ok(HfModelCard {
        model_id: model_id.as_str().into(),
        mirror: mirror.as_str().into(),
        downloads: api.downloads,
        likes: api.likes,
        description: truncate_description(&description),
    })
}

/// 拉取仓库 README 首个非空段落；README 缺失 / 非 200 / 无内容时返回空串。
async fn fetch_readme_first_paragraph(
    client: &reqwest::Client,
    repo: &str,
    mirror: Mirror,
) -> Result<String> {
    let url = format!("{}/{}/raw/main/README.md", mirror.base_url(), repo);
    let resp = client.get(&url).send().await;
    let text = match resp {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => return Ok(String::new()),
    };
    Ok(first_readme_paragraph(&text))
}

/// 简介最大字符数（按 char 计，避免切在 UTF-8 中间）。
pub(crate) const HF_CARD_DESC_MAX_CHARS: usize = 280;

/// 纯函数：README markdown → 首个有实质内容的段落。跳过 yaml front-matter、
/// 标题行（`#` 开头）、图片（`!` 开头）、表格（`|` 开头）、分隔线（`---`）、
/// HTML 标签行（`<div`/`<p`/`<img`…，badges 区常见）与整行为 markdown 链接
/// 的行（`[![badge](…)](…)` / `[中文](…)` 语言切换行）；段落内多行合并成
/// 一句，并剥掉行内链接 / 强调符。便于单测。
pub(crate) fn first_readme_paragraph(markdown: &str) -> String {
    for block in markdown.split("\n\n") {
        let block = block.trim();
        if block.is_empty() || block.starts_with("---") {
            continue;
        }
        let mut parts: Vec<String> = Vec::new();
        for raw_line in block.lines() {
            let line = raw_line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with('!')
                || line.starts_with('|')
                || line.starts_with("---")
                || line.starts_with('<')
                || is_link_only_line(line)
            {
                continue;
            }
            let stripped = strip_markdown_inline(line);
            if !stripped.is_empty() {
                parts.push(stripped);
            }
        }
        if parts.is_empty() {
            continue;
        }
        return truncate_description(&parts.join(" "));
    }
    String::new()
}

/// 整行是否只有 markdown 链接（badges 链 `[![a](u)](v)`、语言切换行
/// `[中文](url) | [English](url)`）。逐个剥离 `[text](url)`，检查链接之间
/// 与行首尾只允许纯分隔符（`|`、逗号、顿号、空白）；badge 链（img.shields.io）
/// 剥不干净（嵌套 `]` 残留括号碎片），直接按特征跳过。
fn is_link_only_line(line: &str) -> bool {
    if line.contains("img.shields.io") || line.trim_start().starts_with("[![") {
        return true;
    }
    let mut rest = line;
    loop {
        let Some(open) = rest.find('[') else { break };
        if !is_separator_only(&rest[..open]) {
            return false;
        }
        let tail = &rest[open + 1..];
        let Some(close) = tail.find("](") else {
            return false;
        };
        let after = &tail[close + 2..];
        let Some(end) = after.find(')') else {
            return false;
        };
        rest = &after[end + 1..];
    }
    is_separator_only(rest)
}

/// 片段是否只含分隔符 / 空白（链接行允许的行首、行尾与链接间间隔）。
fn is_separator_only(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_whitespace() || matches!(c, '|' | ',' | '·' | '、'))
}

/// 剥掉行内 markdown 语法，保留链接显示文本：`[text](url)` → `text`、
/// `![alt](url)` → 空（`!` 在 `[` 前面，图片 alt 不保留）、
/// `` `code` `` / `**bold**` / `*italic*` / `_x_` → 裸文本。
fn strip_markdown_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        if let Some(close) = tail.find("](") {
            let text = &tail[..close];
            let after = &tail[close + 2..];
            if let Some(end) = after.find(')') {
                let is_image = out.ends_with('!');
                if is_image {
                    out.pop(); // 图片标记 `!` 在链接外，随 alt 一起丢弃
                }
                let text = text.trim();
                if !is_image && !text.is_empty() {
                    out.push_str(text);
                }
                rest = &after[end + 1..];
                continue;
            }
        }
        // 不是链接结构的 `[`：原样保留继续扫。
        out.push('[');
        rest = tail;
    }
    out.push_str(rest);
    out.replace("**", "")
        .replace('`', "")
        .replace('*', "")
        .replace('_', "")
}

/// 纯函数：描述截断到 [`HF_CARD_DESC_MAX_CHARS`]，超长加省略号。
pub(crate) fn truncate_description(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= HF_CARD_DESC_MAX_CHARS {
        return text.to_string();
    }
    let truncated: String = text.chars().take(HF_CARD_DESC_MAX_CHARS).collect();
    format!("{truncated}…")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model_id: String,
    pub file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub phase: DownloadPhase,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadPhase {
    Started,
    Progress,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Default)]
pub struct DownloadManager {
    cancel_flags: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
}

impl DownloadManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(self: &Arc<Self>, app: AppHandle, model_id: ModelId, mirror: Mirror) {
        let key = model_id.as_str().to_string();
        let flag = {
            let mut flags = self.cancel_flags.lock();
            if flags.contains_key(&key) {
                log::info!("[local-asr] download already in progress: {key}");
                return;
            }
            let f = Arc::new(AtomicBool::new(false));
            flags.insert(key.clone(), Arc::clone(&f));
            f
        };

        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = run_download(&app, model_id, mirror, Arc::clone(&flag)).await;
            manager.cancel_flags.lock().remove(&key);
            match result {
                Ok(()) => log::info!("[local-asr] download finished: {key}"),
                Err(e) => log::error!("[local-asr] download failed: {key}: {e:#}"),
            }
        });
    }

    pub fn cancel(&self, model_id: ModelId) {
        if let Some(flag) = self.cancel_flags.lock().get(model_id.as_str()) {
            flag.store(true, Ordering::SeqCst);
            log::info!("[local-asr] cancel requested for {}", model_id.as_str());
        } else {
            log::info!(
                "[local-asr] cancel requested for {} but no active download",
                model_id.as_str()
            );
        }
    }

    pub fn is_active(&self, model_id: ModelId) -> bool {
        self.cancel_flags.lock().contains_key(model_id.as_str())
    }
}

pub(crate) fn build_client() -> Result<reqwest::Client> {
    // native-tls (macOS=SecureTransport) 不像 rustls 那样把 CDN unclean close
    // 当致命错误。Android/iOS 无 native-tls feature，走默认 rustls。
    //
    // User-Agent 用 aria2 的——hfd（hf-mirror 官方推荐）就是 aria2 包装，
    // 实测 aria2 UA 在 HF 反滥用规则里走白名单不挨 throttle；自定义 UA
    // (`openless/x`) 在 sustained 传输后会被 mirror 主动切流。
    let mut builder = reqwest::Client::builder()
        .user_agent("aria2/1.36.0")
        .connect_timeout(std::time::Duration::from_secs(30))
        .pool_idle_timeout(std::time::Duration::from_secs(60));
    if !crate::net::use_system_proxy() {
        builder = builder.no_proxy();
    }
    #[cfg(not(mobile))]
    {
        builder = builder.use_native_tls();
    }
    builder.build().context("build reqwest client failed")
}

/// 用户主动取消下载后，清理断点续传产物（`<file>.partial` sparse 文件 +
/// `<file>.partial.idx` 块索引）。`.partial` 按 `set_len` 预分配了目标全长
/// —— 1.7B 模型即使只下了 1% 也占 1.7GB 逻辑大小，不删会让用户以为
/// 「取消失效」且磁盘占用虚高。仅用户取消（非 worker 自 abort）时调用；
/// worker 失败触发的中止保留续传点，重试可直接续传。
pub(crate) fn remove_partial_artifacts(dir: &Path, dest_paths: &[String]) {
    for path in dest_paths {
        let dest = dir.join(path);
        let _ = std::fs::remove_file(dest.with_extension("partial"));
        let _ = std::fs::remove_file(dest.with_extension("partial.idx"));
    }
}

/// 判定一个「已存在」的目标文件是否完整可信，纯函数便于单测（#686）。/// - 大小一致 → 完整；
/// - 大小不符（截断 / 损坏 / 超大）→ 不完整，应删除重下；
/// - `expected_size == 0`（HF 未给出大小）→ 退回旧行为「存在即信任」，避免对未知大小
///   的文件反复重下。
fn existing_file_is_complete(actual_size: u64, expected_size: u64) -> bool {
    if expected_size == 0 {
        return true;
    }
    actual_size == expected_size
}

/// 读盘取实际大小后按 [`existing_file_is_complete`] 判定。元数据取不到（文件刚被删 / 无权限）
/// 视为不完整。读盘方式与 `partial_actual_size` 一致。
fn dest_file_is_complete(dest: &Path, expected_size: u64) -> bool {
    match std::fs::metadata(dest) {
        Ok(m) => existing_file_is_complete(m.len(), expected_size),
        Err(_) => false,
    }
}

async fn run_download(
    app: &AppHandle,
    model_id: ModelId,
    mirror: Mirror,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let dir = model_dir(model_id)?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create model dir failed: {}", dir.display()))?;

    let client = build_client()?;
    let info = match fetch_remote_info(model_id, mirror).await {
        Ok(i) => i,
        Err(e) => {
            emit(
                app,
                DownloadProgress {
                    model_id: model_id.as_str().into(),
                    file: String::new(),
                    file_index: 0,
                    file_count: 0,
                    bytes_downloaded: 0,
                    bytes_total: 0,
                    phase: DownloadPhase::Failed,
                    error: Some(format!("拉文件清单失败: {e:#}")),
                },
            );
            return Err(e);
        }
    };
    let total_bytes = info.total_bytes;
    let file_count = info.files.len();

    emit(
        app,
        DownloadProgress {
            model_id: model_id.as_str().into(),
            file: String::new(),
            file_index: 0,
            file_count,
            bytes_downloaded: super::models::downloaded_bytes(model_id),
            bytes_total: total_bytes,
            phase: DownloadPhase::Started,
            error: None,
        },
    );

    // 多文件并发（aria2 -j 5 同款思路）：每个文件已下字节用 AtomicU64 累加，
    // 总进度 = 各文件已下字节之和 + 历史已完成文件大小。让小文件不阻塞大文件，
    // 也让大文件下半段（CDN throttle 时）剩余带宽喂别的文件。
    {
        std::fs::create_dir_all(&dir).ok();
        for file in &info.files {
            if let Some(parent) = dir.join(&file.path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
    }

    let in_flight_bytes: Arc<Vec<AtomicU64>> =
        Arc::new(info.files.iter().map(|_| AtomicU64::new(0)).collect());
    let already_done_bytes: u64 = info
        .files
        .iter()
        .map(|f| {
            let d = dir.join(&f.path);
            // 只把「已存在且大小完整」的文件计入已完成字节，与下面的跳过判定一致：
            // 截断/损坏的残留文件会被重下，不应计入进度基线（#686）。
            if dest_file_is_complete(&d, f.size) {
                f.size
            } else {
                0
            }
        })
        .sum();

    let semaphore = Arc::new(tokio::sync::Semaphore::new(PARALLEL_FILES));
    let mut futs = futures_util::stream::FuturesUnordered::new();

    for (idx, file) in info.files.iter().enumerate() {
        let dest = dir.join(&file.path);
        if dest.exists() {
            if dest_file_is_complete(&dest, file.size) {
                // 已存在且大小完整 → 跳过；前面 already_done_bytes 已计入。
                continue;
            }
            // 已存在但大小不符（上次下载被中断 / 文件被外部损坏）→ 删除残留重下，
            // 否则截断文件会被信任为完整、模型加载时才以含糊错误失败（#686）。
            log::warn!(
                "[asr-dl] {} exists but size mismatch (expected {}), re-downloading",
                file.path,
                file.size
            );
            let _ = std::fs::remove_file(&dest);
        }
        let url = format!(
            "{}/{}/resolve/main/{}",
            mirror.base_url(),
            model_id.hf_repo(),
            file.path
        );
        let semaphore = Arc::clone(&semaphore);
        let client = client.clone();
        let cancel = Arc::clone(&cancel);
        let app = app.clone();
        let in_flight_bytes = Arc::clone(&in_flight_bytes);
        let model_id_str = model_id.as_str().to_string();
        let file_path = file.path.clone();
        let file_size = file.size;
        let _model_id = model_id; // copy of Copy for closure use
        let total_bytes_cap = total_bytes;
        let already_done = already_done_bytes;

        futs.push(tauri::async_runtime::spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return Err(anyhow::anyhow!("semaphore closed")),
            };
            if cancel.load(Ordering::SeqCst) {
                return Ok(());
            }
            // 进度回调：把该文件实时已下字节写到 in_flight_bytes[idx]，
            // 然后求所有 in_flight 之和 + already_done = 全模型总进度。
            let app_emit = app.clone();
            let model_id_emit = model_id_str.clone();
            let file_path_emit = file_path.clone();
            let in_flight_for_cb = Arc::clone(&in_flight_bytes);
            let last_emit = Arc::new(AtomicU64::new(0));
            let on_progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |bytes_in_file| {
                in_flight_for_cb[idx].store(bytes_in_file, Ordering::Relaxed);
                // 节流：距上次 emit < 150ms 的中间进度直接丢弃（高频事件会让
                // 前端进度条抽搐），in_flight 仍照常累计，下次 emit 带的是最新值。
                let now = now_millis();
                if now - last_emit.load(Ordering::Relaxed) < PROGRESS_EMIT_MIN_INTERVAL_MS {
                    return;
                }
                last_emit.store(now, Ordering::Relaxed);
                let total_in_flight: u64 = in_flight_for_cb
                    .iter()
                    .map(|a| a.load(Ordering::Relaxed))
                    .sum();
                let _ = app_emit.emit(
                    "local-asr-download-progress",
                    DownloadProgress {
                        model_id: model_id_emit.clone(),
                        file: file_path_emit.clone(),
                        file_index: idx,
                        file_count,
                        bytes_downloaded: already_done + total_in_flight,
                        bytes_total: total_bytes_cap,
                        phase: DownloadPhase::Progress,
                        error: None,
                    },
                );
            });

            let result = download_one(
                &client,
                &url,
                &dest,
                file_size,
                Arc::clone(&cancel),
                on_progress,
            )
            .await;
            // 文件下完 → 该 in_flight 永久 = file_size（避免 race 在 emit 时漏算）
            if result.is_ok() {
                in_flight_bytes[idx].store(file_size, Ordering::Relaxed);
            }
            result.with_context(|| format!("file {file_path}"))
        }));
    }

    // 区分"用户主动取消" vs "我们因为某个 worker 失败了主动 abort 其它 worker"：
    // 都共用同一个 cancel AtomicBool（worker 端只看一个 flag 就够），但外层用
    // `self_aborted` 记是哪种情况，决定最后 emit Cancelled 还是 Failed。
    let mut first_err: Option<anyhow::Error> = None;
    let mut self_aborted = false;
    while let Some(joined) = futs.next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
                // 一个 worker 失败 → 让其它 worker 立即停，免得它们继续吃带宽
                // 然后用户还得等到所有任务完成才看到失败。
                if !cancel.load(Ordering::SeqCst) {
                    log::warn!("[local-asr] one file failed; aborting other workers");
                    cancel.store(true, Ordering::SeqCst);
                    self_aborted = true;
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(anyhow::anyhow!("join: {e}"));
                }
            }
        }
    }

    // 用户主动 cancel（不是我们因为错误自己 set 的）→ Cancelled
    if cancel.load(Ordering::SeqCst) && !self_aborted {
        // 取消 = 放弃该模型：清掉 .partial/.partial.idx，避免残留稀疏大文件
        // 占满磁盘（用户取消意图明确，不留续传点）。
        let dest_paths: Vec<String> = info.files.iter().map(|f| f.path.clone()).collect();
        remove_partial_artifacts(&dir, &dest_paths);
        emit_cancelled(app, model_id, "", 0, file_count, total_bytes);
        return Ok(());
    }
    if let Some(e) = first_err {
        emit(
            app,
            DownloadProgress {
                model_id: model_id.as_str().into(),
                file: String::new(),
                file_index: 0,
                file_count,
                bytes_downloaded: super::models::downloaded_bytes(model_id),
                bytes_total: total_bytes,
                phase: DownloadPhase::Failed,
                error: Some(format!("{e:#}")),
            },
        );
        return Err(e);
    }

    let sentinel = dir.join(READY_SENTINEL);
    std::fs::write(&sentinel, b"")
        .with_context(|| format!("write sentinel failed: {}", sentinel.display()))?;

    emit(
        app,
        DownloadProgress {
            model_id: model_id.as_str().into(),
            file: String::new(),
            file_index: file_count,
            file_count,
            bytes_downloaded: super::models::downloaded_bytes(model_id),
            bytes_total: total_bytes,
            phase: DownloadPhase::Finished,
            error: None,
        },
    );
    Ok(())
}

// 这三个数贴合 aria2 / hf_xet 实测：8MB chunk 让单连接寿命 5–20s（CDN 容易 throttle 的临界点之下），
// 单文件 8 并发跟 hf_xet 默认基本对齐；多文件并发 3 个填满带宽且不超过 hf-mirror 的 per-IP 阈值。
const CHUNK_SIZE: u64 = 8 * 1024 * 1024;
const PARALLEL: usize = 8;
const PER_CHUNK_ATTEMPTS: u32 = 4;
const PARALLEL_FILES: usize = 3;

/// `.partial` 文件的真实已下字节（不是 sparse 逻辑大小）。
/// 有 `.partial.idx` → chunked 模式，按 idx 里 chunk 数还原；
/// 没有 → append/single-stream 模式，partial 是 dense，meta.len() 即真实字节。
pub fn partial_actual_size(partial: &Path) -> u64 {
    let total_size = match std::fs::metadata(partial) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return 0;
        }
        Err(e) => {
            eprintln!(
                "[local-asr] partial_actual_size: stat partial failed ({}): {}",
                partial.display(),
                e
            );
            return 0;
        }
    };
    if total_size == 0 {
        return 0;
    }
    let idx_path = partial.with_extension("partial.idx");
    if !idx_path.exists() {
        return total_size;
    }
    let content = match std::fs::read_to_string(&idx_path) {
        Ok(s) => s,
        Err(e) => {
            // idx 不可读 → 不知道哪些 chunk 已落盘，sparse 全长不可信，只能回 0。
            // 但日志要留，否则进度条无故归零没法排查。
            eprintln!(
                "[local-asr] partial_actual_size: read idx failed ({}): {}",
                idx_path.display(),
                e
            );
            return 0;
        }
    };
    let mut seen: HashSet<usize> = HashSet::new();
    let mut total: u64 = 0;
    for line in content.lines() {
        let Ok(idx) = line.trim().parse::<usize>() else {
            continue;
        };
        if !seen.insert(idx) {
            continue;
        }
        let start = (idx as u64).saturating_mul(CHUNK_SIZE);
        if start >= total_size {
            continue;
        }
        // 最后一块可能不到 CHUNK_SIZE
        let end = (start + CHUNK_SIZE).min(total_size);
        total += end - start;
    }
    total
}

pub(crate) async fn download_one(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    total_size: u64,
    cancel: Arc<AtomicBool>,
    on_progress: Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let partial = dest.with_extension("partial");
    let idx_path = partial.with_extension("partial.idx");

    // 文件大小未知（HF 没给 size）→ 退化为单连接整文件下，行为同最早的实现
    if total_size == 0 {
        return single_stream_download(client, url, dest, cancel, on_progress).await;
    }

    // 远端文件 ≤ 一个 chunk 大小：直接单 chunk，不走 sparse + idx
    if total_size <= CHUNK_SIZE {
        let result = chunk_with_retry(
            client,
            url,
            &partial,
            0,
            total_size - 1,
            &cancel,
            &on_progress,
        )
        .await;
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("cancelled");
        }
        result?;
        finalize(&partial, dest, &idx_path).await?;
        return Ok(());
    }

    // 1. 计算 chunk 计划
    let chunks: Vec<(usize, u64, u64)> = chunk_plan(total_size);
    let _total_chunks = chunks.len();

    // 2. 读已完成的 chunk 索引
    let done_set = read_idx(&idx_path);

    // 3. 预先把 .partial 撑到最终大小（sparse 文件，holes = 零字节）
    if !partial.exists() || std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0) != total_size
    {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&partial)
            .with_context(|| format!("create partial failed: {}", partial.display()))?;
        f.set_len(total_size)
            .with_context(|| format!("set_len partial failed: {}", partial.display()))?;
    }
    // 模式标记：sparse partial 必须配对 .partial.idx（哪怕空），
    // 否则 walk_files 看到 partial 有但 idx 无，会把 sparse 全长当成已下完。
    if !idx_path.exists() {
        std::fs::write(&idx_path, b"")
            .with_context(|| format!("touch partial.idx failed: {}", idx_path.display()))?;
    }

    // 4. 总计已下字节（用于初始化进度）
    let initial_done: u64 = chunks
        .iter()
        .filter(|(i, _, _)| done_set.contains(i))
        .map(|(_, s, e)| e - s + 1)
        .sum();
    let bytes_in_file = Arc::new(AtomicU64::new(initial_done));
    on_progress(initial_done);

    // 5. 调度 N 并发 worker
    let remaining: Vec<(usize, u64, u64)> = chunks
        .into_iter()
        .filter(|(i, _, _)| !done_set.contains(i))
        .collect();

    if remaining.is_empty() {
        finalize(&partial, dest, &idx_path).await?;
        return Ok(());
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
    let idx_path_arc = Arc::new(idx_path.clone());
    let partial_arc = Arc::new(partial.clone());
    let url_arc: Arc<str> = Arc::from(url);
    let client = client.clone();
    let mut futs = futures_util::stream::FuturesUnordered::new();

    for (chunk_idx, start, end) in remaining {
        let permit_owned = Arc::clone(&semaphore);
        let client = client.clone();
        let url_arc = Arc::clone(&url_arc);
        let partial_arc = Arc::clone(&partial_arc);
        let idx_path_arc = Arc::clone(&idx_path_arc);
        let cancel = Arc::clone(&cancel);
        let bytes_in_file = Arc::clone(&bytes_in_file);
        let on_progress = Arc::clone(&on_progress);

        futs.push(tauri::async_runtime::spawn(async move {
            let _permit = match permit_owned.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return Err(anyhow::anyhow!("semaphore closed")),
            };
            let result = chunk_with_retry_seek(
                &client,
                &url_arc,
                &partial_arc,
                start,
                end,
                &cancel,
                &bytes_in_file,
                &on_progress,
            )
            .await;
            if result.is_ok() {
                if let Err(e) = append_idx(&idx_path_arc, chunk_idx) {
                    log::warn!("[local-asr] append .partial.idx failed: {e:#}");
                }
            }
            result
        }));
    }

    let mut first_err: Option<anyhow::Error> = None;
    while let Some(joined) = futs.next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(anyhow::anyhow!("join: {e}"));
                }
            }
        }
    }

    if cancel.load(Ordering::SeqCst) {
        anyhow::bail!("cancelled");
    }
    if let Some(e) = first_err {
        return Err(e);
    }

    // 6. 校验 + 落盘
    let actual = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
    if actual != total_size {
        anyhow::bail!("downloaded size {actual} != expected {total_size}");
    }
    finalize(&partial, dest, &idx_path).await?;
    Ok(())
}

fn chunk_plan(total: u64) -> Vec<(usize, u64, u64)> {
    let mut v = Vec::new();
    let mut s = 0u64;
    let mut idx = 0usize;
    while s < total {
        let e = (s + CHUNK_SIZE - 1).min(total - 1);
        v.push((idx, s, e));
        s = e + 1;
        idx += 1;
    }
    v
}

fn read_idx(path: &Path) -> HashSet<usize> {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };
    content
        .lines()
        .filter_map(|l| l.trim().parse::<usize>().ok())
        .collect()
}

fn append_idx(path: &Path, idx: usize) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{idx}")
}

async fn finalize(partial: &Path, dest: &Path, idx_path: &Path) -> Result<()> {
    tokio::fs::rename(partial, dest)
        .await
        .with_context(|| format!("rename partial → final failed: {}", dest.display()))?;
    let _ = std::fs::remove_file(idx_path);
    Ok(())
}

/// 单 chunk + per-chunk retry。append 模式（一次性写到底，给小文件路径）。
async fn chunk_with_retry(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    range_start: u64,
    range_end: u64,
    cancel: &AtomicBool,
    on_progress: &Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=PER_CHUNK_ATTEMPTS {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("cancelled");
        }
        match try_download_range_append(
            client,
            url,
            partial,
            range_start,
            range_end,
            cancel,
            on_progress,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                last_err = Some(e);
                if attempt < PER_CHUNK_ATTEMPTS && !cancel.load(Ordering::SeqCst) {
                    let backoff = std::time::Duration::from_secs(1u64 << (2 * (attempt - 1)));
                    log::warn!(
                        "[local-asr] small-file chunk attempt {attempt}/{PER_CHUNK_ATTEMPTS} failed: {msg}; sleep {:?}",
                        backoff
                    );
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
    Err(last_err
        .unwrap_or_else(|| anyhow::anyhow!("chunk failed after {PER_CHUNK_ATTEMPTS} attempts")))
}

async fn try_download_range_append(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    range_start: u64,
    range_end: u64,
    cancel: &AtomicBool,
    on_progress: &Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let mut req = client.get(url);
    req = req.header("Range", format!("bytes={range_start}-{range_end}"));
    let resp = req
        .send()
        .await
        .with_context(|| format!("HTTP GET {url} failed"))?;
    let status = resp.status();
    if status.as_u16() != 200 && status.as_u16() != 206 {
        anyhow::bail!("HTTP {status} for {url}");
    }
    let effective_start = if status.as_u16() == 200 {
        0
    } else {
        range_start
    };

    // 截断 partial 到本次 attempt 的起点，再 seek 写入。
    // 老 append 实现的 bug：若上一次 attempt 已写了部分字节后失败，retry 拿到的还是
    // 完整 chunk → append → 文件比应有大小多 N 字节 → 永久损坏。
    // 小文件路径每个 chunk 是整个文件（≤ 32MB），用 truncate 重写最直白。
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(partial)
        .await
        .with_context(|| format!("open partial failed: {}", partial.display()))?;
    file.set_len(effective_start)
        .await
        .with_context(|| format!("truncate partial failed: {}", partial.display()))?;
    file.seek(std::io::SeekFrom::Start(effective_start))
        .await
        .with_context(|| format!("seek partial failed: {}", partial.display()))?;

    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.ok();
            anyhow::bail!("cancelled");
        }
        let bytes = chunk.context("read stream chunk failed")?;
        file.write_all(&bytes).await.context("write chunk failed")?;
        written += bytes.len() as u64;
        on_progress(effective_start + written);
    }
    file.flush().await.ok();
    Ok(())
}

/// 大文件并发版：seek 到 chunk 起点写入，**不**append。`bytes_in_file`
/// 是跨所有并发任务累加的总进度。
async fn chunk_with_retry_seek(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    range_start: u64,
    range_end: u64,
    cancel: &AtomicBool,
    bytes_in_file: &Arc<AtomicU64>,
    on_progress: &Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=PER_CHUNK_ATTEMPTS {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("cancelled");
        }
        match try_download_range_seek(
            client,
            url,
            partial,
            range_start,
            range_end,
            cancel,
            bytes_in_file,
            on_progress,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                last_err = Some(e);
                if attempt < PER_CHUNK_ATTEMPTS && !cancel.load(Ordering::SeqCst) {
                    let backoff = std::time::Duration::from_secs(1u64 << (2 * (attempt - 1)));
                    log::warn!(
                        "[local-asr] chunk [{range_start}-{range_end}] attempt {attempt}/{PER_CHUNK_ATTEMPTS} failed: {msg}; sleep {:?}",
                        backoff
                    );
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!(
            "chunk [{range_start}-{range_end}] failed after {PER_CHUNK_ATTEMPTS} attempts"
        )
    }))
}

async fn try_download_range_seek(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    range_start: u64,
    range_end: u64,
    cancel: &AtomicBool,
    bytes_in_file: &Arc<AtomicU64>,
    on_progress: &Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let resp = client
        .get(url)
        .header("Range", format!("bytes={range_start}-{range_end}"))
        .send()
        .await
        .with_context(|| format!("HTTP GET {url} failed"))?;

    let status = resp.status();
    // 并发 seek 模式严格要求 206。服务端忽略 Range 返回 200 + 全文件会
    // 把整个文件写到 range_start 偏移导致灾难性后果，此时直接 fail，
    // 让外层 retry 再试一次。
    if status.as_u16() != 206 {
        anyhow::bail!("expected HTTP 206 Partial Content for ranged GET, got {status}");
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(false) // 文件已经被 set_len 创建好了，这里仅写入
        .open(partial)
        .await
        .with_context(|| format!("open partial for seek failed: {}", partial.display()))?;
    file.seek(std::io::SeekFrom::Start(range_start))
        .await
        .with_context(|| format!("seek to {range_start} failed"))?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.ok();
            anyhow::bail!("cancelled");
        }
        let bytes = chunk.context("read stream chunk failed")?;
        file.write_all(&bytes).await.context("write chunk failed")?;
        let new_total =
            bytes_in_file.fetch_add(bytes.len() as u64, Ordering::Relaxed) + bytes.len() as u64;
        on_progress(new_total);
    }
    file.flush().await.ok();
    Ok(())
}

/// total_size 未知时的退化路径：单 GET 整文件。HF 给的 size 几乎总是有，
/// 这条只是保险。
async fn single_stream_download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<()> {
    let partial = PathBuf::from(dest).with_extension("partial");
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status} for {url}");
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&partial)
        .await?;
    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("cancelled");
        }
        let bytes = chunk?;
        file.write_all(&bytes).await?;
        total += bytes.len() as u64;
        on_progress(total);
    }
    file.flush().await.ok();
    drop(file);
    tokio::fs::rename(&partial, dest).await?;
    Ok(())
}

fn emit(app: &AppHandle, payload: DownloadProgress) {
    if let Err(e) = app.emit("local-asr-download-progress", payload) {
        log::warn!("[local-asr] emit progress failed: {e}");
    }
}

fn emit_cancelled(
    app: &AppHandle,
    model_id: ModelId,
    fname: &str,
    idx: usize,
    file_count: usize,
    total: u64,
) {
    emit(
        app,
        DownloadProgress {
            model_id: model_id.as_str().into(),
            file: fname.into(),
            file_index: idx,
            file_count,
            bytes_downloaded: super::models::downloaded_bytes(model_id),
            bytes_total: total,
            phase: DownloadPhase::Cancelled,
            error: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{
        existing_file_is_complete, first_readme_paragraph, is_link_only_line,
        remove_partial_artifacts, strip_markdown_inline, truncate_description,
        HF_CARD_DESC_MAX_CHARS,
    };

    #[test]
    fn complete_when_size_matches() {
        assert!(existing_file_is_complete(1024, 1024));
    }

    #[test]
    fn incomplete_when_truncated() {
        assert!(!existing_file_is_complete(512, 1024));
    }

    #[test]
    fn incomplete_when_oversized() {
        assert!(!existing_file_is_complete(2048, 1024));
    }

    #[test]
    fn trusts_existence_when_expected_size_unknown() {
        // HF 未给大小（size == 0）时退回「存在即信任」，避免反复重下。
        assert!(existing_file_is_complete(0, 0));
        assert!(existing_file_is_complete(999, 0));
    }

    #[test]
    fn remove_partial_artifacts_deletes_partials_keeps_complete() {
        // 用户取消后：`<file>.partial` 与 `<file>.partial.idx` 应被清掉，
        // 已完成/完整的目标文件不受影响。
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ol-asr-dl-test-{uniq}"));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.safetensors");
        let partial = dest.with_extension("partial");
        let idx = partial.with_extension("partial.idx");
        let keep = dir.join("config.json");
        for p in [&dest, &partial, &idx, &keep] {
            std::fs::write(p, b"x").unwrap();
        }
        let dest_paths: Vec<String> = vec!["model.safetensors".into()];
        remove_partial_artifacts(&dir, &dest_paths);
        assert!(!partial.exists(), ".partial 应被删除");
        assert!(!idx.exists(), ".partial.idx 应被删除");
        assert!(dest.exists(), "完整目标文件不应被删除");
        assert!(keep.exists(), "未在清单里的文件不应被删除");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_readme_paragraph_skips_front_matter_and_headers() {
        let md = "---\nlicense: apache-2.0\n---\n\n# Qwen3-ASR\n\nThis is the first real paragraph.\n\n## Features\n- fast\n- accurate";
        assert_eq!(
            first_readme_paragraph(md),
            "This is the first real paragraph."
        );
    }

    #[test]
    fn first_readme_paragraph_joins_multiline_paragraph() {
        let md = "# Title\n\nFirst line continues\nonto the second line.\n\n## Next";
        assert_eq!(
            first_readme_paragraph(md),
            "First line continues onto the second line."
        );
    }

    #[test]
    fn first_readme_paragraph_returns_empty_when_only_markup() {
        let md = "# Only headers\n\n---\n\n![image](x.png)";
        assert_eq!(first_readme_paragraph(md), "");
    }

    #[test]
    fn first_readme_paragraph_skips_html_badge_lines() {
        // Qwen3 README 实际结构：HTML 包裹的 badge 区 + 徽章链接行 + 正文。
        let md = "# Qwen3\n\n<p align=\"center\">\n  <img src=\"qwen.png\" width=\"400\">\n</p>\n\n<div align=\"center\">\n  <h4> <a href=\"#\">中文</a> | <a href=\"#\">English</a> </h4>\n</div>\n\n[![Model License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)\n\nQwen3 is a next-generation open model.";
        assert_eq!(
            first_readme_paragraph(md),
            "Qwen3 is a next-generation open model."
        );
    }

    #[test]
    fn first_readme_paragraph_skips_link_only_lines() {
        // 语言切换行与 badge 链整行都是纯链接，不应当正文。
        assert!(is_link_only_line(
            "[中文](https://a.cn) | [English](https://a.io)"
        ));
        assert!(is_link_only_line(
            "[![badge](https://img.shields.io/badge/a-1.svg)](https://x)"
        ));
        assert!(!is_link_only_line(
            "See the [docs](https://d.io) for details"
        ));
    }

    #[test]
    fn strip_markdown_inline_keeps_link_text_drops_markup() {
        assert_eq!(
            strip_markdown_inline("See [Qwen3](https://hf.co/Qwen/Qwen3) docs"),
            "See Qwen3 docs"
        );
        assert_eq!(strip_markdown_inline("![logo](logo.png)"), "");
        assert_eq!(
            strip_markdown_inline("**bold** and `code` and _em_"),
            "bold and code and em"
        );
    }

    #[test]
    fn first_readme_paragraph_strips_inline_links_and_emphasis() {
        let md =
            "# Title\n\nCheck the **official** [Qwen3](https://hf.co/Qwen/Qwen3) page for details.";
        assert_eq!(
            first_readme_paragraph(md),
            "Check the official Qwen3 page for details."
        );
    }

    #[test]
    fn truncate_description_keeps_short_text() {
        assert_eq!(truncate_description("hello world"), "hello world");
        assert_eq!(truncate_description("  padded  "), "padded");
    }

    #[test]
    fn truncate_description_cuts_long_text() {
        let long = "界".repeat(HF_CARD_DESC_MAX_CHARS + 50);
        let out = truncate_description(&long);
        assert_eq!(out.chars().count(), HF_CARD_DESC_MAX_CHARS + 1); // +1 省略号
        assert!(out.ends_with('…'));
    }
}
