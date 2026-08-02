use super::*;
use std::io::Write;

// ─────────────────────────── marketplace (Phase A) ───────────────────────────
//
// 客户端跟 marketplace backend 的 HTTP 客户端封装。Backend URL 走 prefs
// `marketplace_base_url`（默认 http://127.0.0.1:8090 开发；生产用户填 https://api.<domain>）。
// 写操作认证：Rust 从 CredentialsVault 读取 GitHub OAuth token 并附加
// `Authorization: Bearer`。`marketplace_dev_login` 只是前端展示缓存，不是权限来源。
//
// 5 个 IPC：
// - marketplace_list      列表 + 搜索 + 排序
// - marketplace_detail    详情（含完整 prompt）
// - marketplace_install   下载 ZIP + 直接调 import_from_zip 装到本地
// - marketplace_upload    把本地某个 style pack export ZIP → multipart 上传
// - marketplace_like      点赞

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceListItem {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub author_login: String,
    pub version: String,
    pub base_mode: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub like_count: i64,
    pub download_count: i64,
    pub published_at: String,
    pub updated_at: String,
    pub origin_pack_id: Option<String>,
    pub origin_author_login: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceDetail {
    #[serde(flatten)]
    pub summary: MarketplaceListItem,
    pub prompt: String,
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceMyPackItem {
    #[serde(flatten)]
    pub summary: MarketplaceListItem,
    pub state: String,
}

/// 风格市场 backend URL —— 硬编码到生产云端，不再读 prefs。
///
/// 历史上这里读 `prefs.marketplace_base_url`（dev 本地可填 127.0.0.1:8090），
/// 现在风格市场已经稳定部署在 apic.openless.top，把 URL 锁死避免用户误改 / 写错。
/// 参数 `_prefs` 保留是为不动调用点签名；将来需要白名单 / 多 endpoint 时再开口。
pub(crate) const MARKETPLACE_BASE_URL: &str = "https://apic.openless.top";

fn marketplace_url_from_prefs(_prefs: &UserPreferences) -> String {
    MARKETPLACE_BASE_URL.to_string()
}

fn marketplace_dev_user(prefs: &UserPreferences) -> String {
    prefs.marketplace_dev_login.trim().to_string()
}

pub(crate) const MARKETPLACE_REAUTH_REQUIRED: &str =
    "marketplace_auth_required: GitHub sign-in expired or is missing; sign in again";
pub(crate) const MARKETPLACE_REDIRECT_REJECTED: &str =
    "marketplace_authenticated_redirect_rejected";
pub(crate) const MARKETPLACE_PUBLIC_REDIRECT_REJECTED: &str =
    "marketplace_public_redirect_rejected";

fn marketplace_access_token() -> Result<String, String> {
    CredentialsVault::get_marketplace_github_token()
        .map_err(|e| format!("read marketplace credential failed: {e}"))?
        .ok_or_else(|| MARKETPLACE_REAUTH_REQUIRED.to_string())
}

fn with_marketplace_bearer(
    request: reqwest::RequestBuilder,
    token: &str,
) -> reqwest::RequestBuilder {
    request.bearer_auth(token)
}

fn marketplace_bearer_request(
    method: reqwest::Method,
    url: &str,
    token: &str,
) -> reqwest::RequestBuilder {
    with_marketplace_bearer(net::credential_http().request(method, url), token)
}

#[derive(Clone)]
enum MarketplaceAuthenticatedEndpoint {
    Upload {
        pack_id: String,
        origin_pack_id: Option<String>,
        bytes: Vec<u8>,
    },
    Like {
        pack_id: String,
    },
    Delete {
        pack_id: String,
    },
    MyLikes,
    MyPacks,
}

impl MarketplaceAuthenticatedEndpoint {
    fn operation(&self) -> &'static str {
        match self {
            Self::Upload { .. } => "upload",
            Self::Like { .. } => "like",
            Self::Delete { .. } => "delete",
            Self::MyLikes => "my-likes",
            Self::MyPacks => "my-packs",
        }
    }

    fn method(&self) -> reqwest::Method {
        match self {
            Self::Upload { .. } | Self::Like { .. } => reqwest::Method::POST,
            Self::Delete { .. } => reqwest::Method::DELETE,
            Self::MyLikes | Self::MyPacks => reqwest::Method::GET,
        }
    }

    fn path(&self) -> String {
        match self {
            Self::Upload { .. } => "/packs".to_string(),
            Self::Like { pack_id } => format!("/packs/{pack_id}/like"),
            Self::Delete { pack_id } => format!("/packs/{pack_id}"),
            Self::MyLikes => "/me/likes".to_string(),
            Self::MyPacks => "/me/packs".to_string(),
        }
    }

    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(match self {
            Self::Upload { .. } => 30,
            Self::Delete { .. } => 15,
            Self::Like { .. } | Self::MyLikes | Self::MyPacks => 10,
        })
    }

    fn request(&self, base: &str, token: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", base.trim_end_matches('/'), self.path());
        let mut request =
            marketplace_bearer_request(self.method(), &url, token).timeout(self.timeout());
        if let Self::Upload {
            pack_id,
            origin_pack_id,
            bytes,
        } = self
        {
            let part = reqwest::multipart::Part::bytes(bytes.clone())
                .file_name(format!("{pack_id}.zip"))
                .mime_str("application/zip")
                .expect("static ZIP MIME type must be valid");
            let mut form = reqwest::multipart::Form::new().part("file", part);
            if let Some(origin_pack_id) = origin_pack_id {
                form = form.text("origin_pack_id", origin_pack_id.clone());
            }
            request = request.multipart(form);
        }
        request
    }
}

#[derive(Clone)]
enum MarketplacePublicEndpoint {
    List {
        query: Option<String>,
        sort: Option<String>,
        limit: Option<u32>,
    },
    Detail {
        pack_id: String,
    },
    Download {
        pack_id: String,
    },
}

impl MarketplacePublicEndpoint {
    fn operation(&self) -> &'static str {
        match self {
            Self::List { .. } => "marketplace list",
            Self::Detail { .. } => "marketplace detail",
            Self::Download { .. } => "marketplace download",
        }
    }

    fn url(&self, base: &str) -> Result<reqwest::Url, String> {
        let base = base.trim_end_matches('/');
        match self {
            Self::List { query, sort, limit } => {
                let mut url = reqwest::Url::parse(&format!("{base}/packs"))
                    .map_err(|_| "invalid marketplace URL".to_string())?;
                if let Some(query) = query.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    url.query_pairs_mut().append_pair("q", query);
                }
                if let Some(sort) = sort.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    url.query_pairs_mut().append_pair("sort", sort);
                }
                if let Some(limit) = limit {
                    url.query_pairs_mut()
                        .append_pair("limit", &limit.to_string());
                }
                Ok(url)
            }
            Self::Detail { pack_id } => reqwest::Url::parse(&format!("{base}/packs/{pack_id}"))
                .map_err(|_| "invalid marketplace URL".to_string()),
            Self::Download { pack_id } => {
                reqwest::Url::parse(&format!("{base}/packs/{pack_id}/download"))
                    .map_err(|_| "invalid marketplace URL".to_string())
            }
        }
    }

    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(match self {
            Self::List { .. } => 10,
            Self::Detail { .. } => 15,
            Self::Download { .. } => 30,
        })
    }
}

fn marketplace_auth_error_for_status(status: reqwest::StatusCode) -> Option<&'static str> {
    (status == reqwest::StatusCode::UNAUTHORIZED).then_some(MARKETPLACE_REAUTH_REQUIRED)
}

fn marketplace_log_value(value: &str) -> String {
    const MAX_LOG_VALUE_LEN: usize = 1024;
    let mut sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.len() > MAX_LOG_VALUE_LEN {
        let mut end = MAX_LOG_VALUE_LEN;
        while !sanitized.is_char_boundary(end) {
            end -= 1;
        }
        sanitized.truncate(end);
        sanitized.push_str("…(truncated)");
    }
    sanitized
}

fn log_marketplace_install_failure(phase: &str, pack_id: &str, error: &str) {
    log::error!(
        "[marketplace-install] stage=failed phase={} pack_id={} error={}",
        marketplace_log_value(phase),
        marketplace_log_value(pack_id),
        marketplace_log_value(error),
    );
}

const MARKETPLACE_INSTALL_IN_PROGRESS: &str =
    "marketplace_install_in_progress: another style pack installation is already running";

static MARKETPLACE_INSTALL_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

fn try_acquire_marketplace_install_lock() -> Result<tokio::sync::MutexGuard<'static, ()>, String> {
    MARKETPLACE_INSTALL_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .try_lock()
        .map_err(|_| MARKETPLACE_INSTALL_IN_PROGRESS.to_string())
}

fn require_valid_marketplace_auth_with(
    status: reqwest::StatusCode,
    clear_credential: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if status.is_redirection() {
        return Err(MARKETPLACE_REDIRECT_REJECTED.to_string());
    }
    let Some(message) = marketplace_auth_error_for_status(status) else {
        return Ok(());
    };
    if let Err(error) = clear_credential() {
        log::warn!("[marketplace] failed to clear rejected credential: {error}");
    }
    Err(message.to_string())
}

fn clear_marketplace_authentication_with(
    remove_credential: impl FnOnce() -> Result<(), String>,
    clear_display_login: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let credential_result = remove_credential();
    // Always clear display state even when durable vault deletion fails. The
    // process tombstone has already made the token unusable.
    let display_result = clear_display_login();
    credential_result.and(display_result)
}

pub(crate) fn clear_marketplace_authentication(coord: &Coordinator) -> Result<(), String> {
    clear_marketplace_authentication_with(
        || {
            CredentialsVault::remove_marketplace_github_token()
                .map_err(|error| format!("clear Marketplace credential failed: {error}"))
        },
        || {
            let mut prefs = coord.prefs().get();
            prefs.marketplace_dev_login.clear();
            coord
                .prefs()
                .set(prefs)
                .map_err(|error| format!("clear Marketplace display login failed: {error}"))
        },
    )
}

async fn execute_authenticated_marketplace_with(
    base: &str,
    endpoint: MarketplaceAuthenticatedEndpoint,
    token_provider: impl FnOnce() -> Result<String, String>,
    clear_rejected_credential: impl FnOnce() -> Result<(), String>,
) -> Result<reqwest::Response, String> {
    // Resolve authentication before constructing/sending a request. The
    // process tombstone therefore guarantees a rejected token never leaves the
    // process again, even if durable vault deletion failed.
    let token = token_provider()?;
    let operation = endpoint.operation();
    let response = net::send_with_retry(|| endpoint.request(base, &token))
        .await
        .map_err(|_| format!("{operation} request failed"))?;
    let status = response.status();
    require_valid_marketplace_auth_with(status, clear_rejected_credential)?;
    if !status.is_success() {
        // Never echo authenticated response bodies or request details into IPC
        // errors. Both may contain server diagnostics or credential material.
        return Err(format!("{operation} HTTP {status}"));
    }
    Ok(response)
}

async fn execute_authenticated_marketplace(
    base: &str,
    endpoint: MarketplaceAuthenticatedEndpoint,
    coord: &Coordinator,
) -> Result<reqwest::Response, String> {
    execute_authenticated_marketplace_with(base, endpoint, marketplace_access_token, || {
        clear_marketplace_authentication(coord)
    })
    .await
}

async fn execute_public_marketplace_with(
    base: &str,
    endpoint: MarketplacePublicEndpoint,
) -> Result<reqwest::Response, String> {
    let url = endpoint.url(base)?;
    let timeout = endpoint.timeout();
    let operation = endpoint.operation();
    let response = net::send_with_retry(|| {
        // Public browse/detail/download intentionally use the anonymous client
        // and never inherit Marketplace bearer state.
        net::anonymous_no_redirect_http()
            .get(url.clone())
            .timeout(timeout)
    })
    .await
    .map_err(|_| format!("{operation} request failed"))?;
    if response.status().is_redirection() {
        return Err(MARKETPLACE_PUBLIC_REDIRECT_REJECTED.to_string());
    }
    if !response.status().is_success() {
        return Err(format!("{operation} HTTP {}", response.status()));
    }
    Ok(response)
}

#[tauri::command]
pub async fn marketplace_list(
    coord: CoordinatorState<'_>,
    query: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<MarketplaceListItem>, String> {
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let resp = execute_public_marketplace_with(
        &base,
        MarketplacePublicEndpoint::List { query, sort, limit },
    )
    .await?;
    let items: Vec<MarketplaceListItem> = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;
    Ok(items)
}

#[tauri::command]
pub async fn marketplace_detail(
    coord: CoordinatorState<'_>,
    pack_id: String,
) -> Result<MarketplaceDetail, String> {
    if !is_valid_session_id(&pack_id) {
        return Err("invalid pack id".into());
    }
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let resp =
        execute_public_marketplace_with(&base, MarketplacePublicEndpoint::Detail { pack_id })
            .await?;
    resp.json::<MarketplaceDetail>()
        .await
        .map_err(|e| format!("parse failed: {e}"))
}

#[tauri::command]
pub async fn marketplace_install(
    coord: CoordinatorState<'_>,
    pack_id: String,
) -> Result<StylePack, String> {
    log::info!(
        "[marketplace-install] stage=start pack_id={}",
        marketplace_log_value(&pack_id)
    );
    // 安全校验：pack_id 来自远端 backend，可能含路径遍历 segment。
    // 用跟 read_audio_recording 同样的 UUID-v4 白名单挡住 ../ / 绝对路径等。
    // backend 当前用 Uuid::new_v4 生成所有 id，合法 id 必然匹配。
    if !is_valid_session_id(&pack_id) {
        log_marketplace_install_failure("validate", &pack_id, "invalid pack id");
        return Err("invalid pack id".into());
    }
    let _install_guard = match try_acquire_marketplace_install_lock() {
        Ok(guard) => guard,
        Err(error) => {
            log_marketplace_install_failure("lock", &pack_id, &error);
            return Err(error);
        }
    };
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);

    // 先拉 detail 拿 authorLogin —— 装好后本地写 originAuthorLogin，
    // 后续编辑+发布时 backend 据此判 supersede（原作者）vs derivative（他人 fork）。
    let detail_response = match execute_public_marketplace_with(
        &base,
        MarketplacePublicEndpoint::Detail {
            pack_id: pack_id.clone(),
        },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            log_marketplace_install_failure("detail", &pack_id, &error);
            return Err(error);
        }
    };
    let detail: serde_json::Value = match detail_response.json().await {
        Ok(detail) => detail,
        Err(error) => {
            let error = format!("parse detail failed: {error}");
            log_marketplace_install_failure("detail", &pack_id, &error);
            return Err(error);
        }
    };
    log::info!("[marketplace-install] stage=detail-ok pack_id={pack_id}");
    let origin_author_login = detail
        .get("authorLogin")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let response = match execute_public_marketplace_with(
        &base,
        MarketplacePublicEndpoint::Download {
            pack_id: pack_id.clone(),
        },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            log_marketplace_install_failure("download", &pack_id, &error);
            return Err(error);
        }
    };
    let bytes = match read_marketplace_archive_response(response).await {
        Ok(bytes) => bytes,
        Err(error) => {
            log_marketplace_install_failure("download", &pack_id, &error);
            return Err(error);
        }
    };
    log::info!(
        "[marketplace-install] stage=download-ok pack_id={pack_id} bytes={}",
        bytes.len()
    );

    // 每次安装使用 create_new 的唯一临时路径；Drop 覆盖导入成功和所有错误分支。
    let temp_root = match marketplace_temp_root() {
        Ok(root) => root,
        Err(error) => {
            log_marketplace_install_failure("temp-root", &pack_id, &error);
            return Err(error);
        }
    };
    log::info!(
        "[marketplace-install] stage=temp-root-ok pack_id={pack_id} path={}",
        marketplace_log_value(&temp_root.display().to_string())
    );
    let tmp = match MarketplaceTempArchive::create_empty_in(&temp_root, &pack_id) {
        Ok(tmp) => tmp,
        Err(error) => {
            log_marketplace_install_failure("temp-create", &pack_id, &error);
            return Err(error);
        }
    };
    log::info!("[marketplace-install] stage=temp-create-ok pack_id={pack_id}");
    if let Err(error) = tmp.write_bytes(&bytes) {
        log_marketplace_install_failure("temp-write", &pack_id, &error);
        return Err(error);
    }
    log::info!("[marketplace-install] stage=temp-write-ok pack_id={pack_id}");
    let imported = match coord.style_packs().import_from_zip(tmp.path()) {
        Ok(imported) => imported,
        Err(error) => {
            let error = error.to_string();
            log_marketplace_install_failure("import", &pack_id, &error);
            return Err(error);
        }
    };
    log::info!(
        "[marketplace-install] stage=import-ok pack_id={pack_id} local_pack_id={}",
        marketplace_log_value(&imported.id)
    );

    // 绑定 origin —— 后续编辑+发布走 derivative / supersede 分支。
    match coord
        .style_packs()
        .set_origin(&imported.id, Some(pack_id.clone()), origin_author_login)
    {
        Ok(pack) => {
            log::info!("[marketplace-install] stage=origin-ok pack_id={pack_id}");
            log::info!("[marketplace-install] stage=done pack_id={pack_id}");
            Ok(pack)
        }
        Err(error) => {
            let error = format!("set origin failed: {error}");
            log_marketplace_install_failure("origin", &pack_id, &error);
            Err(error)
        }
    }
}

fn validate_marketplace_archive_content_length(content_length: Option<u64>) -> Result<(), String> {
    if content_length.is_some_and(|length| {
        length > crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64
    }) {
        return Err(format!(
            "marketplace archive compressed size exceeds {} bytes",
            crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES
        ));
    }
    Ok(())
}

fn append_marketplace_archive_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), String> {
    if body.len().saturating_add(chunk.len())
        > crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES
    {
        return Err(format!(
            "marketplace archive streamed compressed size exceeds {} bytes",
            crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES
        ));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

async fn read_marketplace_archive_response(response: reqwest::Response) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;

    validate_marketplace_archive_content_length(response.content_length())?;
    let initial_capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES);
    let mut body = Vec::with_capacity(initial_capacity);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read marketplace archive body failed: {e}"))?;
        append_marketplace_archive_chunk(&mut body, &chunk)?;
    }
    Ok(body)
}

struct MarketplaceTempArchive {
    path: std::path::PathBuf,
}

impl MarketplaceTempArchive {
    fn create_empty_in(root: &std::path::Path, pack_id: &str) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|error| {
            format!("create marketplace temporary archive directory failed: {error}")
        })?;
        let path = root.join(format!(
            "openless-marketplace-{pack_id}-{}.zip",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("create marketplace temporary archive failed: {error}"))?;
        Ok(Self { path })
    }

    fn create_empty(pack_id: &str) -> Result<Self, String> {
        let root = marketplace_temp_root()?;
        Self::create_empty_in(&root, pack_id)
    }

    fn write_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        self.write_bytes_with(bytes, |file, bytes| file.write_all(bytes))
    }

    fn write_bytes_with(
        &self,
        bytes: &[u8],
        write: impl FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), String> {
        let write_result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&self.path)?;
            write(&mut file, bytes)?;
            file.sync_all()
        })();
        write_result.map_err(|error| format!("write marketplace temporary archive failed: {error}"))
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

fn marketplace_temp_root() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "android")]
    let root = {
        let cache_dir = crate::android::jni::android::app_cache_dir()?;
        marketplace_temp_root_from_cache_dir(std::path::Path::new(&cache_dir))
    };
    #[cfg(not(target_os = "android"))]
    let root = std::env::temp_dir();
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("marketplace temp root create failed: {error}"))?;
    Ok(root)
}

fn marketplace_temp_root_from_cache_dir(cache_dir: &std::path::Path) -> std::path::PathBuf {
    cache_dir.join("openless-marketplace")
}

impl Drop for MarketplaceTempArchive {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tauri::command]
pub async fn marketplace_upload(
    coord: CoordinatorState<'_>,
    pack_id: String,
    origin_pack_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // 本地 pack id 形态：`builtin.light` / 用户 slug / Uuid。用 local 白名单挡 `..` / `/` / `\`。
    if !is_valid_local_pack_id(&pack_id) {
        return Err("invalid pack id".into());
    }
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);

    // 拉本地 pack 拿 origin_pack_id —— 装过的 pack 这里有值，
    // backend 据此判同作者就 supersede 原行（新版本），他人就 derivative（独立新 row）。
    let local_pack = coord
        .style_packs()
        .get(&pack_id)
        .map_err(|e| format!("local pack not found: {e}"))?;
    let origin_pack_id = origin_pack_id
        .filter(|id| is_valid_session_id(id))
        .or_else(|| local_pack.origin_pack_id.clone());

    // 先 export 本地 pack → 临时 ZIP
    let tmp = MarketplaceTempArchive::create_empty(&pack_id)?;
    coord
        .style_packs()
        .export_to_zip(&pack_id, tmp.path())
        .map_err(|e| format!("export local pack failed: {e}"))?;
    let bytes = std::fs::read(tmp.path()).map_err(|e| format!("read exported zip: {e}"))?;

    let resp = execute_authenticated_marketplace(
        &base,
        MarketplaceAuthenticatedEndpoint::Upload {
            pack_id: pack_id.clone(),
            origin_pack_id: origin_pack_id.clone(),
            bytes,
        },
        &coord,
    )
    .await?;
    let body = resp
        .text()
        .await
        .map_err(|_| "read upload response failed".to_string())?;
    let parsed = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|e| format!("parse upload response failed: {e}"))?;

    // 本地从未绑定 origin（首次上传一个本地原创 pack）→ 把 backend 分配的 pack id 写回本地，
    // 让用户在同设备上后续编辑能继续走「同作者 supersede」分支，更新自己原创的包。
    if origin_pack_id.is_none() {
        if let Some(remote_id) = parsed.get("id").and_then(|v| v.as_str()) {
            let prefs2 = coord.prefs().get();
            let dev_user2 = marketplace_dev_user(&prefs2);
            let _ = coord.style_packs().set_origin(
                &pack_id,
                Some(remote_id.to_string()),
                Some(dev_user2),
            );
        }
    }

    Ok(parsed)
}

#[tauri::command]
pub async fn marketplace_like(
    coord: CoordinatorState<'_>,
    pack_id: String,
) -> Result<serde_json::Value, String> {
    if !is_valid_session_id(&pack_id) {
        return Err("invalid pack id".into());
    }
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let resp = execute_authenticated_marketplace(
        &base,
        MarketplaceAuthenticatedEndpoint::Like { pack_id },
        &coord,
    )
    .await?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse failed: {e}"))
}

#[cfg(test)]
mod archive_download_tests {
    use super::{
        append_marketplace_archive_chunk, marketplace_log_value,
        marketplace_temp_root_from_cache_dir, try_acquire_marketplace_install_lock,
        validate_marketplace_archive_content_length, MarketplaceTempArchive,
        MARKETPLACE_INSTALL_IN_PROGRESS,
    };
    use crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES;
    use std::io::Write;
    use std::path::PathBuf;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "openless-marketplace-test-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn marketplace_archive_rejects_oversized_declared_content_length() {
        let error = validate_marketplace_archive_content_length(Some(
            STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64 + 1,
        ))
        .expect_err("oversized content length must fail");

        assert!(error.contains("compressed size"));
    }

    #[test]
    fn marketplace_archive_rejects_streamed_chunk_crossing_limit() {
        let mut body = vec![0; STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES];
        let error = append_marketplace_archive_chunk(&mut body, b"x")
            .expect_err("streamed overflow must fail");

        assert!(error.contains("compressed size"));
        assert_eq!(body.len(), STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES);
    }

    #[test]
    fn marketplace_archive_accepts_exact_streamed_limit() {
        let mut body = vec![0; STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES - 1];
        append_marketplace_archive_chunk(&mut body, b"x").expect("exact limit is valid");

        assert_eq!(body.len(), STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES);
    }

    #[test]
    fn temporary_archives_are_unique_and_drop_cleans_them() {
        let root = test_root("unique");
        let first = MarketplaceTempArchive::create_empty_in(&root, "pack").unwrap();
        first.write_bytes(b"first").unwrap();
        let second = MarketplaceTempArchive::create_empty_in(&root, "pack").unwrap();
        second.write_bytes(b"second").unwrap();
        assert_ne!(first.path(), second.path());
        assert_eq!(std::fs::read(first.path()).unwrap(), b"first");
        assert_eq!(std::fs::read(second.path()).unwrap(), b"second");

        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();
        drop(first);
        drop(second);
        assert!(!first_path.exists());
        assert!(!second_path.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_archive_can_be_overwritten_for_upload() {
        let root = test_root("upload");
        let archive = MarketplaceTempArchive::create_empty_in(&root, "pack").unwrap();
        archive.write_bytes(b"exported zip bytes").unwrap();
        assert_eq!(
            std::fs::read(archive.path()).unwrap(),
            b"exported zip bytes"
        );
        drop(archive);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_write_leaves_no_archive_file() {
        let root = test_root("write-failure");
        let archive = MarketplaceTempArchive::create_empty_in(&root, "pack").unwrap();
        let path = archive.path().to_path_buf();
        let error = archive
            .write_bytes_with(b"must not persist", |file, bytes| {
                file.write_all(&bytes[..4])?;
                Err(std::io::Error::other("injected write failure"))
            })
            .expect_err("injected write failure must fail");
        assert!(error.contains("injected write failure"));
        drop(archive);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn android_cache_root_uses_private_cache_subdirectory() {
        let root = marketplace_temp_root_from_cache_dir(std::path::Path::new(
            "/data/user/0/com.openless.app/cache",
        ));
        assert_eq!(
            root,
            PathBuf::from("/data/user/0/com.openless.app/cache/openless-marketplace")
        );
        assert!(!root.starts_with("/data/local/tmp"));
    }

    #[test]
    fn marketplace_log_value_removes_controls_and_preserves_utf8_boundaries() {
        let sanitized = marketplace_log_value("first\nsecond\r\tthird");
        assert_eq!(sanitized, "first second  third");

        let truncated = marketplace_log_value(&"界".repeat(600));
        assert!(truncated.ends_with("…(truncated)"));
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(!truncated.chars().any(char::is_control));
    }

    #[test]
    fn marketplace_install_lock_rejects_concurrent_install() {
        let first = try_acquire_marketplace_install_lock().expect("first install lock");
        let error = match try_acquire_marketplace_install_lock() {
            Ok(_) => panic!("second install must be rejected while first is active"),
            Err(error) => error,
        };
        assert_eq!(error, MARKETPLACE_INSTALL_IN_PROGRESS);
        drop(first);
        assert!(try_acquire_marketplace_install_lock().is_ok());
    }
}

/// 撤回自己发布的 pack（后端软删 state='withdrawn'，前端列表不再可见）。
/// pack_id 来自远端，必须是 UUID-v4。
#[tauri::command]
pub async fn marketplace_delete(
    coord: CoordinatorState<'_>,
    pack_id: String,
) -> Result<(), String> {
    if !is_valid_session_id(&pack_id) {
        return Err("invalid pack id".into());
    }
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    execute_authenticated_marketplace(
        &base,
        MarketplaceAuthenticatedEndpoint::Delete { pack_id },
        &coord,
    )
    .await?;
    Ok(())
}

/// 拉当前用户赞过的所有 pack id，用于客户端市场页面渲染红心 + 「我赞过的」过滤。
#[tauri::command]
pub async fn marketplace_my_likes(coord: CoordinatorState<'_>) -> Result<Vec<String>, String> {
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let resp =
        execute_authenticated_marketplace(&base, MarketplaceAuthenticatedEndpoint::MyLikes, &coord)
            .await?;
    resp.json::<Vec<String>>()
        .await
        .map_err(|e| format!("parse my-likes failed: {e}"))
}

/// 拉当前用户发布过的 pack（含审核中/已通过/已拒绝/已撤回），用于「我的发布」页面。
#[tauri::command]
pub async fn marketplace_my_packs(
    coord: CoordinatorState<'_>,
) -> Result<Vec<MarketplaceMyPackItem>, String> {
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let resp =
        execute_authenticated_marketplace(&base, MarketplaceAuthenticatedEndpoint::MyPacks, &coord)
            .await?;
    resp.json::<Vec<MarketplaceMyPackItem>>()
        .await
        .map_err(|e| format!("parse my-packs failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::github_oauth::marketplace_auth_status;
    use anyhow::anyhow;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut expected = None;
        loop {
            let mut chunk = [0u8; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if expected.is_none() {
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    expected = Some(header_end + 4 + content_length);
                }
            }
            if expected.is_some_and(|expected| request.len() >= expected) {
                break;
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    fn status_reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            302 => "Found",
            401 => "Unauthorized",
            403 => "Forbidden",
            500 => "Internal Server Error",
            _ => "Test",
        }
    }

    async fn spawn_mock_response(
        status: u16,
        body: String,
    ) -> (String, tokio::task::JoinHandle<(String, bool)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let location = if status == 302 {
                "Location: /must-not-follow\r\n"
            } else {
                ""
            };
            let response = format!(
                "HTTP/1.1 {status} {}\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                status_reason(status),
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let followed_redirect =
                tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                    .await
                    .is_ok();
            (request, followed_redirect)
        });
        (base, task)
    }

    async fn spawn_mock_redirect_response(
        body: String,
    ) -> (
        String,
        tokio::task::JoinHandle<(String, usize, bool, String)>,
    ) {
        let redirect_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!(
            "http://{}/location-gho_public_redirect_secret",
            redirect_target.local_addr().unwrap()
        );
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", source.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();

            let target_accessed = match tokio::time::timeout(
                std::time::Duration::from_millis(300),
                redirect_target.accept(),
            )
            .await
            {
                Ok(Ok((mut target_stream, _))) => {
                    let mut request = [0u8; 2048];
                    let _ = target_stream.read(&mut request).await;
                    target_stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                        )
                        .await
                        .unwrap();
                    true
                }
                _ => false,
            };
            let source_connections =
                if tokio::time::timeout(std::time::Duration::from_millis(150), source.accept())
                    .await
                    .is_ok()
                {
                    2
                } else {
                    1
                };
            (request, source_connections, target_accessed, target_url)
        });
        (base, task)
    }

    fn authenticated_cases() -> Vec<(MarketplaceAuthenticatedEndpoint, reqwest::Method, String)> {
        vec![
            (
                MarketplaceAuthenticatedEndpoint::Upload {
                    pack_id: "local-pack".to_string(),
                    origin_pack_id: Some("origin-id".to_string()),
                    bytes: b"zip-fixture".to_vec(),
                },
                reqwest::Method::POST,
                "/packs".to_string(),
            ),
            (
                MarketplaceAuthenticatedEndpoint::Like {
                    pack_id: "remote-id".to_string(),
                },
                reqwest::Method::POST,
                "/packs/remote-id/like".to_string(),
            ),
            (
                MarketplaceAuthenticatedEndpoint::Delete {
                    pack_id: "remote-id".to_string(),
                },
                reqwest::Method::DELETE,
                "/packs/remote-id".to_string(),
            ),
            (
                MarketplaceAuthenticatedEndpoint::MyLikes,
                reqwest::Method::GET,
                "/me/likes".to_string(),
            ),
            (
                MarketplaceAuthenticatedEndpoint::MyPacks,
                reqwest::Method::GET,
                "/me/packs".to_string(),
            ),
        ]
    }

    fn assert_authenticated_request(request: &str, method: &reqwest::Method, path: &str) {
        let headers = request
            .split("\r\n\r\n")
            .next()
            .unwrap()
            .to_ascii_lowercase();
        assert!(
            headers.starts_with(&format!(
                "{} {} http/1.1",
                method.as_str().to_lowercase(),
                path
            )),
            "unexpected request: {request}"
        );
        assert_eq!(
            headers
                .lines()
                .filter(|line| *line == "authorization: bearer gho_http_secret")
                .count(),
            1,
            "must send exactly one bearer header"
        );
        assert!(!headers.contains("x-dev-user:"));
        assert!(!headers.contains("x-admin:"));
    }

    #[tokio::test]
    async fn every_authenticated_endpoint_executes_exact_request_and_like_toggles() {
        for (endpoint, method, path) in authenticated_cases() {
            let body = if matches!(endpoint, MarketplaceAuthenticatedEndpoint::Like { .. }) {
                r#"{"alreadyLiked":true,"likeCount":2}"#.to_string()
            } else {
                "{}".to_string()
            };
            let (base, server) = spawn_mock_response(200, body).await;
            let response = execute_authenticated_marketplace_with(
                &base,
                endpoint.clone(),
                || Ok("gho_http_secret".to_string()),
                || Ok(()),
            )
            .await
            .unwrap();
            let json: serde_json::Value = response.json().await.unwrap();
            if matches!(endpoint, MarketplaceAuthenticatedEndpoint::Like { .. }) {
                assert_eq!(json["alreadyLiked"], true);
            }
            let (request, followed_redirect) = server.await.unwrap();
            assert_authenticated_request(&request, &method, &path);
            assert!(!followed_redirect);
        }

        let endpoint = MarketplaceAuthenticatedEndpoint::Like {
            pack_id: "remote-id".to_string(),
        };
        let (base, server) =
            spawn_mock_response(200, r#"{"alreadyLiked":false,"likeCount":1}"#.to_string()).await;
        let response = execute_authenticated_marketplace_with(
            &base,
            endpoint,
            || Ok("gho_http_secret".to_string()),
            || Ok(()),
        )
        .await
        .unwrap();
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["alreadyLiked"], false, "same endpoint covers unlike");
        let (request, followed_redirect) = server.await.unwrap();
        assert_authenticated_request(&request, &reqwest::Method::POST, "/packs/remote-id/like");
        assert!(!followed_redirect);
    }

    #[tokio::test]
    async fn every_authenticated_endpoint_rejects_redirect_and_sanitizes_all_errors() {
        for (endpoint, method, path) in authenticated_cases() {
            for status in [302, 401, 403, 500] {
                let response_secret =
                    format!("backend-diagnostic-gho_http_secret-raw-device-secret-{status}");
                let (base, server) = spawn_mock_response(status, response_secret.clone()).await;
                let cleared = Arc::new(AtomicUsize::new(0));
                let clear_count = Arc::clone(&cleared);
                let error = execute_authenticated_marketplace_with(
                    &base,
                    endpoint.clone(),
                    || Ok("gho_http_secret".to_string()),
                    move || {
                        clear_count.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
                .unwrap_err();

                if status == 302 {
                    assert_eq!(error, MARKETPLACE_REDIRECT_REJECTED);
                } else if status == 401 {
                    assert_eq!(error, MARKETPLACE_REAUTH_REQUIRED);
                } else {
                    assert_eq!(
                        error,
                        format!(
                            "{} HTTP {} {}",
                            endpoint.operation(),
                            status,
                            status_reason(status)
                        )
                    );
                }
                assert!(!error.contains("gho_http_secret"));
                assert!(!error.contains("raw-device-secret"));
                assert!(!error.contains(&response_secret));
                assert_eq!(
                    cleared.load(Ordering::SeqCst),
                    if status == 401 { 1 } else { 0 }
                );

                let (request, followed_redirect) = server.await.unwrap();
                assert_authenticated_request(&request, &method, &path);
                assert!(!followed_redirect, "credential client followed redirect");
            }
        }
    }

    #[tokio::test]
    async fn public_list_detail_download_execute_anonymously() {
        let cases = [
            (
                MarketplacePublicEndpoint::List {
                    query: Some("hello".to_string()),
                    sort: Some("likes".to_string()),
                    limit: Some(25),
                },
                "/packs?q=hello&sort=likes&limit=25",
            ),
            (
                MarketplacePublicEndpoint::Detail {
                    pack_id: "remote-id".to_string(),
                },
                "/packs/remote-id",
            ),
            (
                MarketplacePublicEndpoint::Download {
                    pack_id: "remote-id".to_string(),
                },
                "/packs/remote-id/download",
            ),
        ];

        for (endpoint, path) in cases {
            let (base, server) = spawn_mock_response(200, "{}".to_string()).await;
            execute_public_marketplace_with(&base, endpoint)
                .await
                .unwrap();
            let (request, followed_redirect) = server.await.unwrap();
            let headers = request
                .split("\r\n\r\n")
                .next()
                .unwrap()
                .to_ascii_lowercase();
            assert!(headers.starts_with(&format!("get {path} http/1.1")));
            assert!(!headers.contains("authorization:"));
            assert!(!headers.contains("x-dev-user:"));
            assert!(!headers.contains("x-admin:"));
            assert!(!followed_redirect, "anonymous client sent a second request");
        }
    }

    #[tokio::test]
    async fn every_public_endpoint_rejects_redirect_without_contacting_target_or_leaking() {
        let cases = [
            MarketplacePublicEndpoint::List {
                query: None,
                sort: None,
                limit: None,
            },
            MarketplacePublicEndpoint::Detail {
                pack_id: "remote-id".to_string(),
            },
            MarketplacePublicEndpoint::Download {
                pack_id: "remote-id".to_string(),
            },
        ];

        for endpoint in cases {
            let response_secret = format!(
                "public-redirect-body-gho_secret-{}",
                endpoint.operation().replace(' ', "-")
            );
            let (base, server) = spawn_mock_redirect_response(response_secret.clone()).await;
            let result = execute_public_marketplace_with(&base, endpoint).await;
            let (request, source_connections, target_accessed, location) = server.await.unwrap();
            let headers = request
                .split("\r\n\r\n")
                .next()
                .unwrap()
                .to_ascii_lowercase();

            assert!(headers.starts_with("get "));
            assert!(!headers.contains("authorization:"));
            assert!(!headers.contains("x-dev-user:"));
            assert!(!headers.contains("x-admin:"));
            assert_eq!(source_connections, 1, "redirect source was requested again");
            assert!(!target_accessed, "redirect target was contacted");

            let error = result.expect_err("public Marketplace redirect must fail closed");
            assert_eq!(error, "marketplace_public_redirect_rejected");
            assert!(!error.contains(&response_secret));
            assert!(!error.contains(&location));
        }
    }

    #[tokio::test]
    async fn tombstoned_token_reports_signed_out_and_blocks_every_request_after_delete_failure() {
        CredentialsVault::seed_marketplace_github_token_for_tests("gho_rejected_http");
        let display_cleared = Arc::new(AtomicUsize::new(0));
        let display_count = Arc::clone(&display_cleared);
        let result = clear_marketplace_authentication_with(
            || {
                CredentialsVault::reject_marketplace_github_token_for_tests(|| {
                    Err(anyhow!("injected keyring delete failure"))
                })
                .map_err(|error| error.to_string())
            },
            move || {
                display_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(display_cleared.load(Ordering::SeqCst), 1);
        assert!(!marketplace_auth_status().unwrap().signed_in);

        for (endpoint, _, _) in authenticated_cases() {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let error = execute_authenticated_marketplace_with(
                &base,
                endpoint,
                marketplace_access_token,
                || Ok(()),
            )
            .await
            .unwrap_err();
            assert_eq!(error, MARKETPLACE_REAUTH_REQUIRED);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                    .await
                    .is_err(),
                "tombstoned request reached the network"
            );
        }

        let mut retried = false;
        CredentialsVault::reject_marketplace_github_token_for_tests(|| {
            retried = true;
            Ok(())
        })
        .unwrap();
        assert!(retried, "logout must retry durable deletion");
        assert!(!marketplace_auth_status().unwrap().signed_in);
        CredentialsVault::reset_marketplace_github_token_for_tests();
    }
}
