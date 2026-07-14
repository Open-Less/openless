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

fn marketplace_auth_error_for_status(status: reqwest::StatusCode) -> Option<&'static str> {
    (status == reqwest::StatusCode::UNAUTHORIZED).then_some(MARKETPLACE_REAUTH_REQUIRED)
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

pub(crate) fn clear_marketplace_authentication(coord: &Coordinator) -> Result<(), String> {
    let credential_result = CredentialsVault::remove_marketplace_github_token()
        .map_err(|error| format!("clear Marketplace credential failed: {error}"));
    let mut prefs = coord.prefs().get();
    prefs.marketplace_dev_login.clear();
    let prefs_result = coord
        .prefs()
        .set(prefs)
        .map_err(|error| format!("clear Marketplace display login failed: {error}"));
    credential_result.and(prefs_result)
}

fn require_valid_marketplace_auth_for(
    status: reqwest::StatusCode,
    coord: &Coordinator,
) -> Result<(), String> {
    require_valid_marketplace_auth_with(status, || clear_marketplace_authentication(coord))
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
    let mut url = reqwest::Url::parse(&format!("{base}/packs"))
        .map_err(|e| format!("invalid marketplace url: {e}"))?;
    if let Some(q) = query.as_deref() {
        if !q.trim().is_empty() {
            url.query_pairs_mut().append_pair("q", q.trim());
        }
    }
    if let Some(s) = sort.as_deref() {
        if !s.trim().is_empty() {
            url.query_pairs_mut().append_pair("sort", s.trim());
        }
    }
    if let Some(n) = limit {
        url.query_pairs_mut().append_pair("limit", &n.to_string());
    }
    let resp = net::send_with_retry(|| {
        net::http()
            .get(url.clone())
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("marketplace request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("marketplace HTTP {status}: {body}"));
    }
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
    let url = format!("{base}/packs/{pack_id}");
    let resp = net::send_with_retry(|| {
        net::http()
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("marketplace request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        return Err(format!("marketplace HTTP {status}"));
    }
    resp.json::<MarketplaceDetail>()
        .await
        .map_err(|e| format!("parse failed: {e}"))
}

#[tauri::command]
pub async fn marketplace_install(
    coord: CoordinatorState<'_>,
    pack_id: String,
) -> Result<StylePack, String> {
    // 安全校验：pack_id 来自远端 backend，可能含路径遍历 segment。
    // 用跟 read_audio_recording 同样的 UUID-v4 白名单挡住 ../ / 绝对路径等。
    // backend 当前用 Uuid::new_v4 生成所有 id，合法 id 必然匹配。
    if !is_valid_session_id(&pack_id) {
        return Err("invalid pack id".into());
    }
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);

    // 先拉 detail 拿 authorLogin —— 装好后本地写 originAuthorLogin，
    // 后续编辑+发布时 backend 据此判 supersede（原作者）vs derivative（他人 fork）。
    let detail_url = format!("{base}/packs/{pack_id}");
    let detail: serde_json::Value = net::send_with_retry(|| {
        net::http()
            .get(&detail_url)
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|e| format!("marketplace detail failed: {e}"))?
    .error_for_status()
    .map_err(|e| format!("marketplace detail HTTP error: {e}"))?
    .json()
    .await
    .map_err(|e| format!("parse detail failed: {e}"))?;
    let origin_author_login = detail
        .get("authorLogin")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let download_url = format!("{base}/packs/{pack_id}/download");
    let response = net::send_with_retry(|| {
        net::http()
            .get(&download_url)
            .timeout(std::time::Duration::from_secs(30))
    })
    .await
    .map_err(|e| format!("marketplace download failed: {e}"))?
    .error_for_status()
    .map_err(|e| format!("marketplace HTTP error: {e}"))?;
    let bytes = read_marketplace_archive_response(response).await?;

    // 每次安装使用 create_new 的唯一临时路径；Drop 覆盖导入成功和所有错误分支。
    let tmp = MarketplaceTempArchive::create(&pack_id, &bytes)?;
    let imported = coord
        .style_packs()
        .import_from_zip(tmp.path())
        .map_err(|e| e.to_string())?;

    // 绑定 origin —— 后续编辑+发布走 derivative / supersede 分支。
    coord
        .style_packs()
        .set_origin(&imported.id, Some(pack_id), origin_author_login)
        .map_err(|e| format!("set origin failed: {e}"))
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
    fn create(pack_id: &str, bytes: &[u8]) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "openless-marketplace-{pack_id}-{}.zip",
            uuid::Uuid::new_v4().simple()
        ));
        let write_result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(bytes)?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&path);
            return Err(format!(
                "write marketplace temporary archive failed: {error}"
            ));
        }
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
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
    let access_token = marketplace_access_token()?;

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
    let tmp = std::env::temp_dir().join(format!("openless-marketplace-upload-{pack_id}.zip"));
    coord
        .style_packs()
        .export_to_zip(&pack_id, &tmp)
        .map_err(|e| format!("export local pack failed: {e}"))?;
    let bytes = std::fs::read(&tmp).map_err(|e| format!("read exported zip: {e}"))?;
    let _ = std::fs::remove_file(&tmp);

    // multipart 上传：表单是流式 body，不走 send_with_retry 的闭包重试；改用共享
    // 客户端 —— 之前 list/detail 命令若已打开过连接，这里直接复用连接池里的连接。
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(format!("{pack_id}.zip"))
        .mime_str("application/zip")
        .map_err(|e| format!("multipart build failed: {e}"))?;
    let mut form = reqwest::multipart::Form::new().part("file", part);
    if let Some(ref oid) = origin_pack_id {
        form = form.text("origin_pack_id", oid.clone());
    }
    let upload_url = format!("{base}/packs");
    let resp = marketplace_bearer_request(reqwest::Method::POST, &upload_url, &access_token)
        .timeout(std::time::Duration::from_secs(30))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("upload request failed: {e}"))?;
    let status = resp.status();
    require_valid_marketplace_auth_for(status, &coord)?;
    if !status.is_success() {
        return Err(format!("upload HTTP {status}"));
    }
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
    let access_token = marketplace_access_token()?;
    let like_url = format!("{base}/packs/{pack_id}/like");
    let resp = net::send_with_retry(|| {
        marketplace_bearer_request(reqwest::Method::POST, &like_url, &access_token)
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("like request failed: {e}"))?;
    require_valid_marketplace_auth_for(resp.status(), &coord)?;
    if !resp.status().is_success() {
        return Err(format!("like HTTP {}", resp.status()));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse failed: {e}"))
}

#[cfg(test)]
mod archive_download_tests {
    use super::{append_marketplace_archive_chunk, validate_marketplace_archive_content_length};
    use crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES;

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
    let access_token = marketplace_access_token()?;
    let delete_url = format!("{base}/packs/{pack_id}");
    let resp = net::send_with_retry(|| {
        marketplace_bearer_request(reqwest::Method::DELETE, &delete_url, &access_token)
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|e| format!("delete request failed: {e}"))?;
    let status = resp.status();
    require_valid_marketplace_auth_for(status, &coord)?;
    if !status.is_success() {
        return Err(format!("delete HTTP {status}"));
    }
    Ok(())
}

/// 拉当前用户赞过的所有 pack id，用于客户端市场页面渲染红心 + 「我赞过的」过滤。
#[tauri::command]
pub async fn marketplace_my_likes(coord: CoordinatorState<'_>) -> Result<Vec<String>, String> {
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let access_token = marketplace_access_token()?;
    let likes_url = format!("{base}/me/likes");
    let resp = net::send_with_retry(|| {
        marketplace_bearer_request(reqwest::Method::GET, &likes_url, &access_token)
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("my-likes request failed: {e}"))?;
    require_valid_marketplace_auth_for(resp.status(), &coord)?;
    if !resp.status().is_success() {
        return Err(format!("my-likes HTTP {}", resp.status()));
    }
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
    let access_token = marketplace_access_token()?;
    let packs_url = format!("{base}/me/packs");
    let resp = net::send_with_retry(|| {
        marketplace_bearer_request(reqwest::Method::GET, &packs_url, &access_token)
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("my-packs request failed: {e}"))?;
    require_valid_marketplace_auth_for(resp.status(), &coord)?;
    if !resp.status().is_success() {
        return Err(format!("my-packs HTTP {}", resp.status()));
    }
    resp.json::<Vec<MarketplaceMyPackItem>>()
        .await
        .map_err(|e| format!("parse my-packs failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{
        marketplace_auth_error_for_status, marketplace_bearer_request,
        require_valid_marketplace_auth_with, MARKETPLACE_REAUTH_REQUIRED,
        MARKETPLACE_REDIRECT_REJECTED,
    };
    use reqwest::StatusCode;

    #[test]
    fn authenticated_request_uses_bearer_and_never_dev_identity_headers() {
        let request = marketplace_bearer_request(
            reqwest::Method::POST,
            "https://example.invalid/packs",
            "gho_header_test",
        )
        .build()
        .expect("request should build");

        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer gho_header_test")
        );
        assert!(request.headers().get("X-Dev-User").is_none());
        assert!(request.headers().get("X-Admin").is_none());
    }

    #[test]
    fn unauthorized_response_maps_to_clear_reauthentication_without_echoing_token() {
        let message = marketplace_auth_error_for_status(StatusCode::UNAUTHORIZED)
            .expect("401 should require reauthentication");

        assert_eq!(message, MARKETPLACE_REAUTH_REQUIRED);
        assert!(!message.contains("gho_header_test"));
        assert_eq!(
            marketplace_auth_error_for_status(StatusCode::FORBIDDEN),
            None
        );
    }

    #[test]
    fn rejected_token_is_cleared_and_requires_reauthentication() {
        let mut cleared = false;
        let result = require_valid_marketplace_auth_with(StatusCode::UNAUTHORIZED, || {
            cleared = true;
            Ok(())
        });

        assert_eq!(result, Err(MARKETPLACE_REAUTH_REQUIRED.to_string()));
        assert!(cleared);
    }

    #[test]
    fn every_authenticated_marketplace_entry_uses_bearer_without_dev_headers() {
        let entries = [
            (reqwest::Method::POST, "/packs"),
            (reqwest::Method::POST, "/packs/id/like"),
            (reqwest::Method::DELETE, "/packs/id"),
            (reqwest::Method::GET, "/me/likes"),
            (reqwest::Method::GET, "/me/packs"),
        ];
        for (method, path) in entries {
            let request = marketplace_bearer_request(
                method.clone(),
                &format!("https://example.invalid{path}"),
                "gho_entry_test",
            )
            .build()
            .unwrap();
            assert_eq!(request.method(), method);
            assert_eq!(request.url().path(), path);
            assert_eq!(
                request
                    .headers()
                    .get(reqwest::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer gho_entry_test")
            );
            assert!(request.headers().get("X-Dev-User").is_none());
            assert!(request.headers().get("X-Admin").is_none());
        }
    }

    #[test]
    fn redirects_fail_closed_without_clearing_valid_token() {
        let mut cleared = false;
        let result = require_valid_marketplace_auth_with(StatusCode::FOUND, || {
            cleared = true;
            Ok(())
        });

        assert_eq!(result, Err(MARKETPLACE_REDIRECT_REJECTED.to_string()));
        assert!(!cleared);
    }
}
