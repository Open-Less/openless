use super::*;

// ─────────────────────────── marketplace (Phase A) ───────────────────────────
//
// 客户端跟 marketplace backend 的 HTTP 客户端封装。Backend URL 走 prefs
// `marketplace_base_url`（默认 http://127.0.0.1:8090 开发；生产用户填 https://api.<domain>）。
// dev-mode auth：用户在 Settings 填 `marketplace_dev_login`（GitHub 风格 username），
// 后续 OAuth 接入时换成 token 字段。
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
    let bytes = net::send_with_retry(|| {
        net::http()
            .get(&download_url)
            .timeout(std::time::Duration::from_secs(30))
    })
    .await
    .map_err(|e| format!("marketplace download failed: {e}"))?
    .error_for_status()
    .map_err(|e| format!("marketplace HTTP error: {e}"))?
    .bytes()
    .await
    .map_err(|e| format!("read body failed: {e}"))?;

    // pack_id 已经过 UUID 白名单，拼临时文件路径安全。
    let tmp = std::env::temp_dir().join(format!("openless-marketplace-{pack_id}.zip"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write tmp zip: {e}"))?;
    let imported_result = coord
        .style_packs()
        .import_from_zip(&tmp)
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    let imported = imported_result?;

    // 绑定 origin —— 后续编辑+发布走 derivative / supersede 分支。
    coord
        .style_packs()
        .set_origin(&imported.id, Some(pack_id), origin_author_login)
        .map_err(|e| format!("set origin failed: {e}"))
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
    let dev_user = marketplace_dev_user(&prefs);
    if dev_user.is_empty() {
        return Err("未登录：先在 Settings 填发布者名字".into());
    }

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
    let resp = net::http()
        .post(format!("{base}/packs"))
        .header("X-Dev-User", dev_user)
        .timeout(std::time::Duration::from_secs(30))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("upload request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .unwrap_or_else(|e| format!("read body failed: {e}"))
        .clone();
    if !status.is_success() {
        return Err(format!("upload HTTP {status}: {body}"));
    }
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
    let dev_user = marketplace_dev_user(&prefs);
    if dev_user.is_empty() {
        return Err("未登录：先在 Settings 填发布者名字".into());
    }
    let like_url = format!("{base}/packs/{pack_id}/like");
    let resp = net::send_with_retry(|| {
        net::http()
            .post(&like_url)
            .header("X-Dev-User", dev_user.as_str())
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("like request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("like HTTP {}", resp.status()));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse failed: {e}"))
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
    let dev_user = marketplace_dev_user(&prefs);
    if dev_user.is_empty() {
        return Err("未登录：先在 Settings 填发布者名字".into());
    }
    let delete_url = format!("{base}/packs/{pack_id}");
    let resp = net::send_with_retry(|| {
        net::http()
            .delete(&delete_url)
            .header("X-Dev-User", dev_user.as_str())
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|e| format!("delete request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("delete HTTP {status}: {body}"));
    }
    Ok(())
}

/// 拉当前用户赞过的所有 pack id，用于客户端市场页面渲染红心 + 「我赞过的」过滤。
#[tauri::command]
pub async fn marketplace_my_likes(coord: CoordinatorState<'_>) -> Result<Vec<String>, String> {
    let prefs = coord.prefs().get();
    let base = marketplace_url_from_prefs(&prefs);
    let dev_user = marketplace_dev_user(&prefs);
    if dev_user.is_empty() {
        return Ok(Vec::new()); // 未登录就空集合，UI 渲染无红心
    }
    let likes_url = format!("{base}/me/likes");
    let resp = net::send_with_retry(|| {
        net::http()
            .get(&likes_url)
            .header("X-Dev-User", dev_user.as_str())
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("my-likes request failed: {e}"))?;
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
    let dev_user = marketplace_dev_user(&prefs);
    if dev_user.is_empty() {
        return Ok(Vec::new());
    }
    let packs_url = format!("{base}/me/packs");
    let resp = net::send_with_retry(|| {
        net::http()
            .get(&packs_url)
            .header("X-Dev-User", dev_user.as_str())
            .timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("my-packs request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("my-packs HTTP {}", resp.status()));
    }
    resp.json::<Vec<MarketplaceMyPackItem>>()
        .await
        .map_err(|e| format!("parse my-packs failed: {e}"))
}
