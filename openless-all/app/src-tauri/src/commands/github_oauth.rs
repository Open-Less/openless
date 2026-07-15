use super::*;

use parking_lot::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const GITHUB_OAUTH_CLIENT_ID: &str = "Ov23liyv3nEucG7oMHNE";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const FLOW_CANCELLED: &str = "OAuth 登录已取消，请重新发起登录";
const FLOW_EXPIRED: &str = "OAuth 设备码已过期，请重新发起登录";

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
    Err("GitHub OAuth 未配置".to_string())
}

#[derive(Clone)]
struct SecretDeviceCode(String);

impl std::fmt::Debug for SecretDeviceCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceStartResponse {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u32,
    pub expires_in: u32,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum GithubDevicePollResult {
    Authorized { login: String },
    Pending,
    SlowDown,
    Error { message: String },
}

#[derive(Clone)]
struct ActiveGithubDeviceFlow {
    flow_id: String,
    generation: u64,
    device_code: SecretDeviceCode,
    expires_at: Instant,
    interval: Duration,
    last_poll_at: Option<Instant>,
}

#[derive(Clone)]
struct GithubDevicePollLease {
    flow_id: String,
    generation: u64,
    device_code: SecretDeviceCode,
}

enum PollPermit {
    Ready(GithubDevicePollLease),
    TooSoon,
    Invalid(&'static str),
}

#[derive(Default)]
struct GithubDeviceFlowRegistry {
    generation: u64,
    active: Option<ActiveGithubDeviceFlow>,
}

impl GithubDeviceFlowRegistry {
    fn begin_start(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.active = None;
        self.generation
    }

    fn activate(
        &mut self,
        generation: u64,
        flow_id: String,
        device_code: SecretDeviceCode,
        expires_at: Instant,
        interval: Duration,
    ) -> Result<(), String> {
        if generation != self.generation {
            return Err(FLOW_CANCELLED.to_string());
        }
        self.active = Some(ActiveGithubDeviceFlow {
            flow_id,
            generation,
            device_code,
            expires_at,
            interval,
            last_poll_at: None,
        });
        Ok(())
    }

    fn invalidate_generation(&mut self, generation: u64) {
        if self.generation == generation {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
        }
    }

    fn cancel(&mut self, flow_id: Option<&str>) {
        let should_cancel = match (flow_id, self.active.as_ref()) {
            (Some(expected), Some(active)) => active.flow_id == expected,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if should_cancel {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
        }
    }

    fn poll_permit(&mut self, flow_id: &str, now: Instant) -> PollPermit {
        let Some(active) = self.active.as_mut() else {
            return PollPermit::Invalid(FLOW_CANCELLED);
        };
        if active.flow_id != flow_id {
            return PollPermit::Invalid(FLOW_CANCELLED);
        }
        if now >= active.expires_at {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
            return PollPermit::Invalid(FLOW_EXPIRED);
        }
        if active
            .last_poll_at
            .is_some_and(|last| now.saturating_duration_since(last) < active.interval)
        {
            return PollPermit::TooSoon;
        }
        active.last_poll_at = Some(now);
        PollPermit::Ready(GithubDevicePollLease {
            flow_id: active.flow_id.clone(),
            generation: active.generation,
            device_code: active.device_code.clone(),
        })
    }

    fn lease_is_active(&mut self, lease: &GithubDevicePollLease, now: Instant) -> bool {
        let Some(active) = self.active.as_ref() else {
            return false;
        };
        if now >= active.expires_at {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
            return false;
        }
        active.generation == lease.generation && active.flow_id == lease.flow_id
    }

    fn apply_slow_down(&mut self, lease: &GithubDevicePollLease, now: Instant) -> bool {
        if !self.lease_is_active(lease, now) {
            return false;
        }
        if let Some(active) = self.active.as_mut() {
            active.interval = active.interval.saturating_add(Duration::from_secs(5));
        }
        true
    }

    fn consume_if_active(
        &mut self,
        lease: &GithubDevicePollLease,
        now: Instant,
        save: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.lease_is_active(lease, now) {
            return Err(FLOW_CANCELLED.to_string());
        }
        // Save while holding the flow lock. Cancellation and successful
        // consumption therefore have one atomic ordering point.
        save()?;
        self.generation = self.generation.wrapping_add(1);
        self.active = None;
        Ok(())
    }
}

fn github_device_flows() -> &'static Mutex<GithubDeviceFlowRegistry> {
    static FLOWS: OnceLock<Mutex<GithubDeviceFlowRegistry>> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(GithubDeviceFlowRegistry::default()))
}

#[derive(Clone, Copy)]
struct GithubOAuthEndpoints<'a> {
    device_code: &'a str,
    access_token: &'a str,
    user: &'a str,
}

const GITHUB_ENDPOINTS: GithubOAuthEndpoints<'static> = GithubOAuthEndpoints {
    device_code: GITHUB_DEVICE_CODE_URL,
    access_token: GITHUB_ACCESS_TOKEN_URL,
    user: GITHUB_USER_URL,
};

fn reject_authenticated_redirect(
    status: reqwest::StatusCode,
    operation: &str,
) -> Result<(), String> {
    if status.is_redirection() {
        return Err(format!("{operation} rejected redirect"));
    }
    Ok(())
}

fn parse_device_start_response(
    status: reqwest::StatusCode,
    body: &serde_json::Value,
) -> Result<(SecretDeviceCode, String, String, u32, u32), String> {
    reject_authenticated_redirect(status, "GitHub device flow")?;
    if !status.is_success() {
        return Err(format!("GitHub device flow HTTP {status}"));
    }
    let required = |name: &str| {
        body[name]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("GitHub device flow malformed response: missing {name}"))
    };
    let device_code = SecretDeviceCode(required("device_code")?);
    let user_code = required("user_code")?;
    let verification_uri = required("verification_uri")?;
    let verification_url = reqwest::Url::parse(&verification_uri)
        .map_err(|_| "GitHub device flow malformed verification URI".to_string())?;
    if verification_url.scheme() != "https" {
        return Err("GitHub device flow requires an HTTPS verification URI".to_string());
    }
    let interval = body["interval"]
        .as_u64()
        .unwrap_or(5)
        .try_into()
        .map_err(|_| "GitHub device flow invalid interval".to_string())?;
    let expires_in = body["expires_in"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "GitHub device flow invalid expiry".to_string())?;
    if interval == 0 {
        return Err("GitHub device flow invalid interval".to_string());
    }
    Ok((
        device_code,
        user_code,
        verification_uri,
        interval,
        expires_in,
    ))
}

async fn github_device_flow_start_with(
    endpoints: GithubOAuthEndpoints<'_>,
    flows: &Mutex<GithubDeviceFlowRegistry>,
) -> Result<GithubDeviceStartResponse, String> {
    let generation = flows.lock().begin_start();
    let result = async {
        let client_id = get_github_oauth_client_id()?;
        let resp = net::send_with_retry(|| {
            net::credential_http()
                .post(endpoints.device_code)
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(15))
                .form(&[("client_id", client_id.as_str()), ("scope", "read:user")])
        })
        .await
        .map_err(|_| "GitHub device flow request failed".to_string())?;
        let status = resp.status();
        reject_authenticated_redirect(status, "GitHub device flow")?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "GitHub device flow malformed response".to_string())?;
        let (device_code, user_code, verification_uri, interval, expires_in) =
            parse_device_start_response(status, &body)?;
        let flow_id = uuid::Uuid::new_v4().to_string();
        flows.lock().activate(
            generation,
            flow_id.clone(),
            device_code,
            Instant::now() + Duration::from_secs(expires_in.into()),
            Duration::from_secs(interval.into()),
        )?;
        Ok(GithubDeviceStartResponse {
            flow_id,
            user_code,
            verification_uri,
            interval,
            expires_in,
        })
    }
    .await;
    if result.is_err() {
        flows.lock().invalidate_generation(generation);
    }
    result
}

#[tauri::command]
pub async fn github_device_flow_start() -> Result<GithubDeviceStartResponse, String> {
    github_device_flow_start_with(GITHUB_ENDPOINTS, github_device_flows()).await
}

fn github_login_from_verified_response(
    status: reqwest::StatusCode,
    body: &serde_json::Value,
) -> Result<String, String> {
    reject_authenticated_redirect(status, "GitHub user verification")?;
    if !status.is_success() {
        return Err(format!("GitHub user verification HTTP {status}"));
    }
    let login = body["login"].as_str().unwrap_or("").trim();
    if login.is_empty() {
        return Err("GitHub user verification returned no login".to_string());
    }
    Ok(login.to_string())
}

async fn github_device_flow_poll_with(
    flow_id: String,
    endpoints: GithubOAuthEndpoints<'_>,
    flows: &Mutex<GithubDeviceFlowRegistry>,
    save_token: impl FnOnce(&str) -> Result<(), String>,
) -> Result<GithubDevicePollResult, String> {
    let lease = match flows.lock().poll_permit(&flow_id, Instant::now()) {
        PollPermit::Ready(lease) => lease,
        PollPermit::TooSoon => return Ok(GithubDevicePollResult::Pending),
        PollPermit::Invalid(message) => {
            return Ok(GithubDevicePollResult::Error {
                message: message.to_string(),
            })
        }
    };
    let client_id = get_github_oauth_client_id()?;
    let token_resp = net::send_with_retry(|| {
        net::credential_http()
            .post(endpoints.access_token)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", lease.device_code.0.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
    })
    .await
    .map_err(|_| "GitHub token request failed".to_string())?;
    let token_status = token_resp.status();
    if let Err(message) = reject_authenticated_redirect(token_status, "GitHub token exchange") {
        flows.lock().cancel(Some(&flow_id));
        return Ok(GithubDevicePollResult::Error { message });
    }
    let body: serde_json::Value = match token_resp.json().await {
        Ok(body) => body,
        Err(_) => {
            flows.lock().cancel(Some(&flow_id));
            return Ok(GithubDevicePollResult::Error {
                message: "GitHub token exchange malformed response".to_string(),
            });
        }
    };
    if !token_status.is_success() {
        flows.lock().cancel(Some(&flow_id));
        return Ok(GithubDevicePollResult::Error {
            message: format!("GitHub token exchange HTTP {token_status}"),
        });
    }
    if !flows.lock().lease_is_active(&lease, Instant::now()) {
        return Ok(GithubDevicePollResult::Error {
            message: FLOW_CANCELLED.to_string(),
        });
    }

    if let Some(token) = body["access_token"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
    {
        if !flows.lock().lease_is_active(&lease, Instant::now()) {
            return Ok(GithubDevicePollResult::Error {
                message: FLOW_CANCELLED.to_string(),
            });
        }
        let user_resp = net::send_with_retry(|| {
            net::credential_http()
                .get(endpoints.user)
                .header("Accept", "application/vnd.github+json")
                .timeout(Duration::from_secs(15))
                .bearer_auth(token)
        })
        .await
        .map_err(|_| "GitHub user verification request failed".to_string())?;
        let user_status = user_resp.status();
        if let Err(message) = reject_authenticated_redirect(user_status, "GitHub user verification")
        {
            flows.lock().cancel(Some(&flow_id));
            return Ok(GithubDevicePollResult::Error { message });
        }
        let user_body: serde_json::Value = match user_resp.json().await {
            Ok(body) => body,
            Err(_) => {
                flows.lock().cancel(Some(&flow_id));
                return Ok(GithubDevicePollResult::Error {
                    message: "GitHub user verification malformed response".to_string(),
                });
            }
        };
        if !flows.lock().lease_is_active(&lease, Instant::now()) {
            return Ok(GithubDevicePollResult::Error {
                message: FLOW_CANCELLED.to_string(),
            });
        }
        let login = match github_login_from_verified_response(user_status, &user_body) {
            Ok(login) => login,
            Err(message) => {
                flows.lock().cancel(Some(&flow_id));
                return Ok(GithubDevicePollResult::Error { message });
            }
        };
        let persisted = flows
            .lock()
            .consume_if_active(&lease, Instant::now(), || save_token(token));
        if let Err(message) = persisted {
            return Ok(GithubDevicePollResult::Error { message });
        }
        return Ok(GithubDevicePollResult::Authorized { login });
    }

    match body["error"].as_str().unwrap_or("") {
        "authorization_pending" => Ok(GithubDevicePollResult::Pending),
        "slow_down" => {
            if flows.lock().apply_slow_down(&lease, Instant::now()) {
                Ok(GithubDevicePollResult::SlowDown)
            } else {
                Ok(GithubDevicePollResult::Error {
                    message: FLOW_CANCELLED.to_string(),
                })
            }
        }
        "expired_token" => {
            flows.lock().cancel(Some(&flow_id));
            Ok(GithubDevicePollResult::Error {
                message: FLOW_EXPIRED.to_string(),
            })
        }
        "access_denied" => {
            flows.lock().cancel(Some(&flow_id));
            Ok(GithubDevicePollResult::Error {
                message: "GitHub authorization was denied".to_string(),
            })
        }
        _ => {
            flows.lock().cancel(Some(&flow_id));
            Ok(GithubDevicePollResult::Error {
                message: "GitHub token exchange malformed response".to_string(),
            })
        }
    }
}

#[tauri::command]
pub async fn github_device_flow_poll(flow_id: String) -> Result<GithubDevicePollResult, String> {
    github_device_flow_poll_with(flow_id, GITHUB_ENDPOINTS, github_device_flows(), |token| {
        CredentialsVault::set_marketplace_github_token(token)
            .map_err(|error| format!("save Marketplace credential failed: {error}"))
    })
    .await
}

#[tauri::command]
pub fn github_device_flow_cancel(flow_id: Option<String>) {
    github_device_flows().lock().cancel(flow_id.as_deref());
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAuthStatus {
    pub signed_in: bool,
}

#[tauri::command]
pub fn marketplace_auth_status() -> Result<MarketplaceAuthStatus, String> {
    let signed_in = CredentialsVault::get_marketplace_github_token()
        .map_err(|error| format!("read Marketplace sign-in status failed: {error}"))?
        .is_some();
    Ok(MarketplaceAuthStatus { signed_in })
}

#[tauri::command]
pub fn marketplace_logout(coord: CoordinatorState<'_>) -> Result<(), String> {
    super::marketplace::clear_marketplace_authentication(&coord)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    fn activate_test_flow(
        flows: &Mutex<GithubDeviceFlowRegistry>,
        now: Instant,
        interval: Duration,
        lifetime: Duration,
    ) -> String {
        let mut flows = flows.lock();
        let generation = flows.begin_start();
        let flow_id = "opaque-flow-id".to_string();
        flows
            .activate(
                generation,
                flow_id.clone(),
                SecretDeviceCode("raw-device-secret".to_string()),
                now + lifetime,
                interval,
            )
            .unwrap();
        flow_id
    }

    #[test]
    fn start_response_and_secret_debug_never_expose_device_code() {
        let response = GithubDeviceStartResponse {
            flow_id: "opaque".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            interval: 7,
            expires_in: 600,
        };
        let serialized = serde_json::to_string(&response).unwrap();

        assert!(!serialized.contains("deviceCode"));
        assert!(!serialized.contains("raw-device-secret"));
        assert_eq!(
            format!("{:?}", SecretDeviceCode("secret".into())),
            "[REDACTED]"
        );
    }

    #[test]
    fn registry_enforces_interval_slow_down_expiry_and_single_consumption() {
        let flows = Mutex::new(GithubDeviceFlowRegistry::default());
        let now = Instant::now();
        let flow_id =
            activate_test_flow(&flows, now, Duration::from_secs(7), Duration::from_secs(30));
        let lease = match flows.lock().poll_permit(&flow_id, now) {
            PollPermit::Ready(lease) => lease,
            _ => panic!("first poll should run"),
        };
        assert!(matches!(
            flows
                .lock()
                .poll_permit(&flow_id, now + Duration::from_secs(6)),
            PollPermit::TooSoon
        ));
        assert!(flows
            .lock()
            .apply_slow_down(&lease, now + Duration::from_secs(6)));
        assert!(matches!(
            flows
                .lock()
                .poll_permit(&flow_id, now + Duration::from_secs(11)),
            PollPermit::TooSoon
        ));
        assert!(matches!(
            flows
                .lock()
                .poll_permit(&flow_id, now + Duration::from_secs(12)),
            PollPermit::Ready(_)
        ));
        assert!(matches!(
            flows
                .lock()
                .poll_permit(&flow_id, now + Duration::from_secs(31)),
            PollPermit::Invalid(FLOW_EXPIRED)
        ));
    }

    #[test]
    fn cancel_after_user_response_prevents_token_save() {
        let flows = Mutex::new(GithubDeviceFlowRegistry::default());
        let now = Instant::now();
        let flow_id =
            activate_test_flow(&flows, now, Duration::from_secs(5), Duration::from_secs(60));
        let lease = match flows.lock().poll_permit(&flow_id, now) {
            PollPermit::Ready(lease) => lease,
            _ => panic!("poll should run"),
        };
        flows.lock().cancel(Some(&flow_id));
        let mut saved = false;
        let result = flows.lock().consume_if_active(&lease, now, || {
            saved = true;
            Ok(())
        });

        assert!(result.is_err());
        assert!(!saved);
    }

    #[test]
    fn malformed_and_redirect_start_responses_fail_closed() {
        assert!(parse_device_start_response(
            reqwest::StatusCode::FOUND,
            &serde_json::json!({"device_code":"secret"}),
        )
        .is_err());
        assert!(parse_device_start_response(
            reqwest::StatusCode::OK,
            &serde_json::json!({
                "device_code":"secret",
                "user_code":"CODE",
                "verification_uri":"https://github.com/login/device",
                "interval":0,
                "expires_in":900
            }),
        )
        .is_err());
        assert!(github_login_from_verified_response(
            reqwest::StatusCode::FOUND,
            &serde_json::json!({"login":"forged"}),
        )
        .is_err());
    }

    async fn write_json_response(
        stream: &mut tokio::net::TcpStream,
        body: &str,
    ) -> std::io::Result<()> {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        stream.write_all(response.as_bytes()).await
    }

    #[tokio::test]
    async fn real_start_http_keeps_raw_device_code_inside_rust() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            write_json_response(
                &mut stream,
                r#"{"device_code":"raw-http-device-secret","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","interval":9,"expires_in":600}"#,
            )
            .await
            .unwrap();
        });
        let endpoint = format!("http://{address}");
        let endpoints = GithubOAuthEndpoints {
            device_code: &endpoint,
            access_token: &endpoint,
            user: &endpoint,
        };
        let flows = Mutex::new(GithubDeviceFlowRegistry::default());

        let response = github_device_flow_start_with(endpoints, &flows)
            .await
            .unwrap();
        server.await.unwrap();
        let serialized = serde_json::to_string(&response).unwrap();

        assert_eq!(response.interval, 9);
        assert_eq!(response.expires_in, 600);
        assert!(!serialized.contains("raw-http-device-secret"));
        assert!(!serialized.contains("deviceCode"));
        assert_eq!(
            flows
                .lock()
                .active
                .as_ref()
                .map(|active| active.device_code.0.as_str()),
            Some("raw-http-device-secret")
        );
    }

    #[tokio::test]
    async fn in_flight_user_request_cancelled_before_resume_never_saves_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (user_seen_tx, user_seen_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut token_stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let _ = token_stream.read(&mut request).await.unwrap();
            write_json_response(&mut token_stream, r#"{"access_token":"gho_mock_secret"}"#)
                .await
                .unwrap();

            let (mut user_stream, _) = listener.accept().await.unwrap();
            let _ = user_stream.read(&mut request).await.unwrap();
            user_seen_tx.send(()).unwrap();
            resume_rx.await.unwrap();
            write_json_response(&mut user_stream, r#"{"login":"octocat"}"#)
                .await
                .unwrap();
        });
        let base = format!("http://{address}");
        let endpoints = GithubOAuthEndpoints {
            device_code: &base,
            access_token: &base,
            user: &base,
        };
        let flows = Mutex::new(GithubDeviceFlowRegistry::default());
        let flow_id = activate_test_flow(
            &flows,
            Instant::now(),
            Duration::from_millis(1),
            Duration::from_secs(60),
        );
        let saved = AtomicBool::new(false);
        let poll = github_device_flow_poll_with(flow_id.clone(), endpoints, &flows, |_| {
            saved.store(true, Ordering::SeqCst);
            Ok(())
        });
        let cancel = async {
            user_seen_rx.await.unwrap();
            flows.lock().cancel(Some(&flow_id));
            resume_tx.send(()).unwrap();
        };
        let (result, ()) = tokio::join!(poll, cancel);
        server.await.unwrap();

        assert!(matches!(
            result.unwrap(),
            GithubDevicePollResult::Error { .. }
        ));
        assert!(!saved.load(Ordering::SeqCst));
    }
}
