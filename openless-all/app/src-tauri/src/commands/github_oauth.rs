use super::*;

// ─────────────────────── GitHub OAuth Device Flow (Phase 1) ───────────────────────
//
// 客户端直连 GitHub 拿 access_token + login，前端自动把 login 写进
// prefs.marketplaceDevLogin。marketplace backend 完全不动（依然 X-Dev-User）。
// Phase 2 才会让 backend 验证 GitHub identity（JWT 签发 + 防伪造）。
//
// 配置 client_id 的两种方式（OAuth App client_id 非敏感，可硬编码）：
//   1. 在下方 GITHUB_OAUTH_CLIENT_ID 常量填值（生产推荐 — 直接 bake 进二进制）
//   2. 启动前设置环境变量 GITHUB_OAUTH_CLIENT_ID=<your_client_id>（dev 方便）
//
// 注册 OAuth App：
//   https://github.com/settings/applications/new
//   - Application name: OpenLess (or your fork)
//   - Homepage URL: https://openless.top (or任意)
//   - Authorization callback URL: https://openless.top (Device Flow 不真用，但表单要求填)
//   - 创建后在 General 页面勾选 "Enable Device Flow"
//   - 抄 client_id 填到本常量

const GITHUB_OAUTH_CLIENT_ID: &str = "Ov23liyv3nEucG7oMHNE";

fn get_github_oauth_client_id() -> Result<String, String> {
    if let Ok(env_id) = std::env::var("GITHUB_OAUTH_CLIENT_ID") {
        let trimmed = env_id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if !GITHUB_OAUTH_CLIENT_ID.is_empty() {
        return Ok(GITHUB_OAUTH_CLIENT_ID.to_string());
    }
    Err(
        "GitHub OAuth 未配置。请去 https://github.com/settings/applications/new 注册一个 OAuth App\
        （必须勾 Enable Device Flow），把 client_id 填到 \
        openless-all/app/src-tauri/src/commands.rs 的 GITHUB_OAUTH_CLIENT_ID 常量，\
        或在启动前设置环境变量 GITHUB_OAUTH_CLIENT_ID=<your_client_id>。"
            .to_string(),
    )
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceStartResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u32,
    pub expires_in: u32,
}

#[tauri::command]
pub async fn github_device_flow_start() -> Result<GithubDeviceStartResponse, String> {
    let client_id = get_github_oauth_client_id()?;
    let resp = net::send_with_retry(|| {
        net::http()
            .post("https://github.com/login/device/code")
            .header("Accept", "application/json")
            .header("User-Agent", "OpenLess")
            .timeout(std::time::Duration::from_secs(15))
            .form(&[("client_id", client_id.as_str()), ("scope", "read:user")])
    })
    .await
    .map_err(|e| format!("调用 GitHub /login/device/code 失败：{e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 device/code 响应失败：{e}"))?;
    if !status.is_success() {
        let err = body["error"].as_str().unwrap_or("unknown_error");
        let desc = body["error_description"].as_str().unwrap_or("");
        return Err(format!("GitHub device/code {status} {err}: {desc}"));
    }
    Ok(GithubDeviceStartResponse {
        device_code: body["device_code"].as_str().unwrap_or("").to_string(),
        user_code: body["user_code"].as_str().unwrap_or("").to_string(),
        verification_uri: body["verification_uri"]
            .as_str()
            .unwrap_or("https://github.com/login/device")
            .to_string(),
        interval: body["interval"].as_u64().unwrap_or(5) as u32,
        expires_in: body["expires_in"].as_u64().unwrap_or(900) as u32,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum GithubDevicePollResult {
    Authorized { login: String },
    Pending,
    SlowDown,
    Error { message: String },
}

#[tauri::command]
pub async fn github_device_flow_poll(
    device_code: String,
) -> Result<GithubDevicePollResult, String> {
    let client_id = get_github_oauth_client_id()?;
    let token_resp = net::send_with_retry(|| {
        net::http()
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .header("User-Agent", "OpenLess")
            .timeout(std::time::Duration::from_secs(15))
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
    })
    .await
    .map_err(|e| format!("调用 GitHub /login/oauth/access_token 失败：{e}"))?;
    let body: serde_json::Value = token_resp
        .json()
        .await
        .map_err(|e| format!("解析 access_token 响应失败：{e}"))?;

    if let Some(token) = body["access_token"].as_str() {
        let user_resp = net::send_with_retry(|| {
            net::http()
                .get("https://api.github.com/user")
                .header("User-Agent", "OpenLess")
                .header("Accept", "application/vnd.github+json")
                .timeout(std::time::Duration::from_secs(15))
                .bearer_auth(token)
        })
        .await
        .map_err(|e| format!("调用 GitHub /user 失败：{e}"))?;
        let user_body: serde_json::Value = user_resp
            .json()
            .await
            .map_err(|e| format!("解析 /user 响应失败：{e}"))?;
        let login = user_body["login"].as_str().unwrap_or("").to_string();
        if login.is_empty() {
            return Ok(GithubDevicePollResult::Error {
                message: "GitHub /user 返回空 login".to_string(),
            });
        }
        return Ok(GithubDevicePollResult::Authorized { login });
    }

    let err = body["error"].as_str().unwrap_or("");
    let msg = match err {
        "authorization_pending" => return Ok(GithubDevicePollResult::Pending),
        "slow_down" => return Ok(GithubDevicePollResult::SlowDown),
        "expired_token" => "OAuth 设备码已过期，请重新发起登录".to_string(),
        "access_denied" => "你在 GitHub 上拒绝了授权".to_string(),
        other if !other.is_empty() => format!("OAuth 错误：{other}"),
        _ => "未知 OAuth 错误（access_token 缺失）".to_string(),
    };
    Ok(GithubDevicePollResult::Error { message: msg })
}
