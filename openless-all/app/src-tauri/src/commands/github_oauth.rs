use super::*;

// ───────────────────────── GitHub OAuth Device Flow ─────────────────────────
//
// 客户端直连 GitHub 拿 access_token + login。Rust 在 `/user` 验证成功后把 token
// 写入 CredentialsVault；前端只收到 login 用作展示缓存，永远拿不到 token。
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

fn github_login_from_verified_response(
    status: reqwest::StatusCode,
    body: &serde_json::Value,
) -> Result<String, String> {
    if !status.is_success() {
        return Err(format!("GitHub /user HTTP {status}"));
    }
    let login = body["login"].as_str().unwrap_or("").trim();
    if login.is_empty() {
        return Err("GitHub /user 返回空 login".to_string());
    }
    Ok(login.to_string())
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

    if let Some(token) = body["access_token"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
    {
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
        let user_status = user_resp.status();
        let user_body: serde_json::Value = user_resp
            .json()
            .await
            .map_err(|e| format!("解析 /user 响应失败：{e}"))?;
        let login = match github_login_from_verified_response(user_status, &user_body) {
            Ok(login) => login,
            Err(message) => return Ok(GithubDevicePollResult::Error { message }),
        };
        CredentialsVault::set_marketplace_github_token(token)
            .map_err(|e| format!("保存 Marketplace 登录凭据失败：{e}"))?;
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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAuthStatus {
    pub signed_in: bool,
}

/// Exposes only credential presence. The token remains inside Rust.
#[tauri::command]
pub fn marketplace_auth_status() -> Result<MarketplaceAuthStatus, String> {
    let signed_in = CredentialsVault::get_marketplace_github_token()
        .map_err(|e| format!("读取 Marketplace 登录状态失败：{e}"))?
        .is_some();
    Ok(MarketplaceAuthStatus { signed_in })
}

#[tauri::command]
pub fn marketplace_logout() -> Result<(), String> {
    CredentialsVault::remove_marketplace_github_token()
        .map_err(|e| format!("清除 Marketplace 登录凭据失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::{github_login_from_verified_response, GithubDevicePollResult};
    use reqwest::StatusCode;

    #[test]
    fn github_user_must_be_successful_and_have_a_login_before_token_persistence() {
        assert_eq!(
            github_login_from_verified_response(
                StatusCode::OK,
                &serde_json::json!({ "login": "octocat" }),
            )
            .as_deref(),
            Ok("octocat")
        );
        assert!(github_login_from_verified_response(
            StatusCode::UNAUTHORIZED,
            &serde_json::json!({ "login": "forged" }),
        )
        .is_err());
        assert!(
            github_login_from_verified_response(StatusCode::OK, &serde_json::json!({})).is_err()
        );
    }

    #[test]
    fn authorized_poll_result_exposes_login_but_never_the_token() {
        let serialized = serde_json::to_string(&GithubDevicePollResult::Authorized {
            login: "octocat".to_string(),
        })
        .expect("poll result should serialize");

        assert!(serialized.contains("octocat"));
        assert!(!serialized.contains("access_token"));
        assert!(!serialized.contains("gho_"));
    }
}
