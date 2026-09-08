//! Shared Marketplace and GitHub device-flow orchestration.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use futures_util::StreamExt;

use crate::credentials::{CredentialKey, CredentialNamespace, CredentialStore, SecretValue};
use crate::domains::{
    MarketplaceApi, MarketplaceAuthStatus, MarketplaceDetail, MarketplaceLikeResult,
    MarketplaceListItem, MarketplaceMyPackItem, MarketplaceQuery, MarketplaceUploadResult,
    OAuthDeviceFlow, OAuthPollResult,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::types::StylePackChange;
use crate::{PreferencesStore, StylePack, StylePackStore};

pub const MARKETPLACE_GITHUB_TOKEN_ACCOUNT: &str = "github.oauth_token";
pub const MARKETPLACE_BASE_URL: &str = "https://apic.openless.top";

#[derive(Debug, Clone)]
pub struct MarketplaceConfig {
    pub base_url: reqwest::Url,
    pub github_client_id: String,
    pub github_device_code_url: reqwest::Url,
    pub github_access_token_url: reqwest::Url,
    pub github_user_url: reqwest::Url,
}

impl MarketplaceConfig {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, BackendError> {
        let parse = |value: &str, name: &str| {
            reqwest::Url::parse(value).map_err(|_| {
                BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    format!("invalid {name} URL"),
                )
            })
        };
        let github_client_id = std::env::var("GITHUB_OAUTH_CLIENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Ov23liyv3nEucG7oMHNE".into());
        Ok(Self {
            base_url: parse(base_url.as_ref(), "marketplace")?,
            github_client_id,
            github_device_code_url: parse(
                "https://github.com/login/device/code",
                "GitHub device code",
            )?,
            github_access_token_url: parse(
                "https://github.com/login/oauth/access_token",
                "GitHub access token",
            )?,
            github_user_url: parse("https://api.github.com/user", "GitHub user")?,
        })
    }

    pub fn production() -> Self {
        Self::new(MARKETPLACE_BASE_URL).expect("built-in Marketplace URLs are valid")
    }
}

#[derive(Clone)]
struct SecretDeviceCode(String);

impl std::fmt::Debug for SecretDeviceCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretDeviceCode([REDACTED])")
    }
}

#[derive(Clone)]
struct ActiveDeviceFlow {
    flow_id: String,
    generation: u64,
    device_code: SecretDeviceCode,
    expires_at: Instant,
    interval: Duration,
    last_poll_at: Option<Instant>,
}

#[derive(Clone)]
struct DevicePollLease {
    flow_id: String,
    generation: u64,
    device_code: SecretDeviceCode,
}

enum PollPermit {
    Ready(DevicePollLease),
    TooSoon,
    Invalid(&'static str),
}

#[derive(Default)]
struct DeviceFlowRegistry {
    generation: u64,
    active: Option<ActiveDeviceFlow>,
}

const OAUTH_FLOW_CANCELLED: &str = "OAuth 登录已取消，请重新发起登录";
const OAUTH_FLOW_EXPIRED: &str = "OAuth 设备码已过期，请重新发起登录";

impl DeviceFlowRegistry {
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
    ) -> Result<(), BackendError> {
        if generation != self.generation {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                OAUTH_FLOW_CANCELLED,
            ));
        }
        self.active = Some(ActiveDeviceFlow {
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
        let matches = match (flow_id, self.active.as_ref()) {
            (Some(expected), Some(active)) => active.flow_id == expected,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if matches {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
        }
    }

    fn poll_permit(&mut self, flow_id: &str, now: Instant) -> PollPermit {
        let Some(active) = self.active.as_mut() else {
            return PollPermit::Invalid(OAUTH_FLOW_CANCELLED);
        };
        if active.flow_id != flow_id {
            return PollPermit::Invalid(OAUTH_FLOW_CANCELLED);
        }
        if now >= active.expires_at {
            self.generation = self.generation.wrapping_add(1);
            self.active = None;
            return PollPermit::Invalid(OAUTH_FLOW_EXPIRED);
        }
        if active
            .last_poll_at
            .is_some_and(|last| now.saturating_duration_since(last) < active.interval)
        {
            return PollPermit::TooSoon;
        }
        active.last_poll_at = Some(now);
        PollPermit::Ready(DevicePollLease {
            flow_id: active.flow_id.clone(),
            generation: active.generation,
            device_code: active.device_code.clone(),
        })
    }

    fn lease_is_active(&mut self, lease: &DevicePollLease, now: Instant) -> bool {
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

    fn apply_slow_down(&mut self, lease: &DevicePollLease, now: Instant) -> bool {
        if !self.lease_is_active(lease, now) {
            return false;
        }
        if let Some(active) = self.active.as_mut() {
            active.interval = active.interval.saturating_add(Duration::from_secs(5));
        }
        true
    }

    fn consume(&mut self, lease: &DevicePollLease) {
        debug_assert!(self.active.as_ref().is_some_and(|active| {
            active.flow_id == lease.flow_id && active.generation == lease.generation
        }));
        self.generation = self.generation.wrapping_add(1);
        self.active = None;
    }
}

#[derive(Clone)]
pub(crate) struct MarketplaceService {
    config: MarketplaceConfig,
    #[allow(dead_code)]
    credential_store: Arc<dyn CredentialStore>,
    #[allow(dead_code)]
    preferences: Arc<PreferencesStore>,
    #[allow(dead_code)]
    style_packs: Arc<StylePackStore>,
    #[allow(dead_code)]
    events: BackendEventPublisher,
    #[allow(dead_code)]
    style_pack_revision: Arc<AtomicU64>,
    #[allow(dead_code)]
    auth_tombstoned: Arc<AtomicBool>,
    #[allow(dead_code)]
    install_lock: Arc<tokio::sync::Mutex<()>>,
    device_flows: Arc<tokio::sync::Mutex<DeviceFlowRegistry>>,
}

impl MarketplaceService {
    pub(crate) fn new(
        config: MarketplaceConfig,
        credential_store: Arc<dyn CredentialStore>,
        preferences: Arc<PreferencesStore>,
        style_packs: Arc<StylePackStore>,
        events: BackendEventPublisher,
        style_pack_revision: Arc<AtomicU64>,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            config,
            credential_store,
            preferences,
            style_packs,
            events,
            style_pack_revision,
            auth_tombstoned: Arc::new(AtomicBool::new(false)),
            install_lock: Arc::new(tokio::sync::Mutex::new(())),
            device_flows: Arc::new(tokio::sync::Mutex::new(DeviceFlowRegistry::default())),
        })
    }

    fn public_url(&self, path: &str) -> Result<reqwest::Url, BackendError> {
        self.config.base_url.join(path).map_err(|_| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "invalid Marketplace request URL",
            )
        })
    }

    async fn send_with_retry<F>(&self, make: F) -> Result<reqwest::Response, BackendError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        const MAX_ATTEMPTS: u32 = 10;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match make().send().await {
                Ok(response) => return Ok(response),
                Err(error) if error.is_connect() && attempt < MAX_ATTEMPTS => {
                    let backoff = (150u64 * 2u64.pow((attempt - 1).min(3))).min(900);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
                Err(_) => {
                    return Err(BackendError::new(
                        BackendErrorCode::Provider,
                        "Marketplace request failed",
                    ))
                }
            }
        }
    }

    async fn public_get(
        &self,
        url: reqwest::Url,
        timeout: Duration,
    ) -> Result<reqwest::Response, BackendError> {
        // Resolve the cached client for every request so a saved or live proxy
        // opt-out takes effect. Anonymous requests add no auth to this client;
        // the shared no-redirect policy also protects OAuth/bearer requests.
        let response = self
            .send_with_retry(|| {
                crate::net::credential_http_for_url(url.as_str())
                    .get(url.clone())
                    .timeout(timeout)
            })
            .await?;
        if response.status().is_redirection() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "marketplace_public_redirect_rejected",
            ));
        }
        if !response.status().is_success() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!("Marketplace request HTTP {}", response.status()),
            ));
        }
        Ok(response)
    }

    async fn download_archive_impl(&self, pack_id: &str) -> Result<Vec<u8>, BackendError> {
        if !is_remote_pack_id(pack_id) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "invalid Marketplace pack id",
            ));
        }
        let url = self.public_url(&format!("packs/{pack_id}/download"))?;
        let response = self.public_get(url, Duration::from_secs(30)).await?;
        let limit = crate::style_pack_archive::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES;
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!("Marketplace archive exceeds {limit} compressed bytes"),
            ));
        }
        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(limit);
        let mut bytes = Vec::with_capacity(capacity);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "read Marketplace archive failed",
                )
            })?;
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    format!("Marketplace archive exceeds {limit} streamed compressed bytes"),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        crate::style_pack_archive::validate_style_pack_archive_bytes(&bytes).map_err(|_| {
            BackendError::new(
                BackendErrorCode::Provider,
                "Marketplace returned an invalid style pack archive",
            )
        })?;
        Ok(bytes)
    }

    async fn install_impl(&self, pack_id: String) -> Result<StylePack, BackendError> {
        if !is_remote_pack_id(&pack_id) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "invalid Marketplace pack id",
            ));
        }
        let _guard = self.install_lock.try_lock().map_err(|_| {
            BackendError::new(
                BackendErrorCode::Busy,
                "another Marketplace installation is already running",
            )
        })?;
        let detail = MarketplaceApi::detail(self, pack_id.clone()).await?;
        let archive = self.download_archive_impl(&pack_id).await?;
        let pack = self.style_packs.import_from_zip_bytes_with_origin(
            &archive,
            pack_id,
            Some(detail.summary.author_login),
        )?;
        let revision = self.style_pack_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.events.publish(
            None,
            BackendEventKind::StylePacksChanged(StylePackChange { revision }),
        );
        Ok(pack)
    }

    fn token_key() -> Result<CredentialKey, BackendError> {
        CredentialKey::new(
            CredentialNamespace::Marketplace,
            None,
            MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
        )
    }

    fn authentication_required() -> BackendError {
        BackendError::new(
            BackendErrorCode::PermissionDenied,
            "marketplace_auth_required: GitHub sign-in expired or is missing; sign in again",
        )
    }

    async fn read_access_token(&self) -> Result<SecretValue, BackendError> {
        if self.auth_tombstoned.load(Ordering::Acquire) {
            return Err(Self::authentication_required());
        }
        self.credential_store
            .read(Self::token_key()?)
            .await?
            .ok_or_else(Self::authentication_required)
    }

    async fn clear_authentication(&self) -> Result<(), BackendError> {
        self.auth_tombstoned.store(true, Ordering::Release);
        let remove_result = self.credential_store.remove(Self::token_key()?).await;
        let mut preferences = self.preferences.get();
        preferences.marketplace_dev_login.clear();
        let preferences_result = self.preferences.set(preferences);
        remove_result.and(preferences_result)
    }

    async fn authenticated_response(
        &self,
        method: reqwest::Method,
        path: &str,
        timeout: Duration,
    ) -> Result<reqwest::Response, BackendError> {
        let token = self.read_access_token().await?;
        let url = self.public_url(path)?;
        let response = self
            .send_with_retry(|| {
                crate::net::credential_http_for_url(url.as_str())
                    .request(method.clone(), url.clone())
                    .bearer_auth(token.expose_secret())
                    .timeout(timeout)
            })
            .await?;
        self.validate_authenticated_response(response).await
    }

    async fn validate_authenticated_response(
        &self,
        response: reqwest::Response,
    ) -> Result<reqwest::Response, BackendError> {
        let status = response.status();
        if status.is_redirection() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "marketplace_authenticated_redirect_rejected",
            ));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let _ = self.clear_authentication().await;
            return Err(Self::authentication_required());
        }
        if !status.is_success() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!("authenticated Marketplace request HTTP {status}"),
            ));
        }
        Ok(response)
    }

    async fn upload_impl(
        &self,
        pack_id: String,
        requested_origin: Option<String>,
    ) -> Result<MarketplaceUploadResult, BackendError> {
        if !is_local_pack_id(&pack_id) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "invalid local style pack id",
            ));
        }
        if requested_origin
            .as_deref()
            .is_some_and(|origin| !is_remote_pack_id(origin))
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "invalid Marketplace origin pack id",
            ));
        }
        let local_pack = self.style_packs.get(&pack_id)?;
        let origin_pack_id = requested_origin.or_else(|| local_pack.origin_pack_id.clone());
        let archive = self.style_packs.export_zip_bytes(&pack_id)?;
        let token = self.read_access_token().await?;
        let url = self.public_url("packs")?;
        let upload_pack_id = pack_id.clone();
        let upload_origin = origin_pack_id.clone();
        let response = self
            .send_with_retry(|| {
                let part = reqwest::multipart::Part::bytes(archive.clone())
                    .file_name(format!("{upload_pack_id}.zip"))
                    .mime_str("application/zip")
                    .expect("static ZIP MIME type is valid");
                let mut form = reqwest::multipart::Form::new().part("file", part);
                if let Some(origin) = &upload_origin {
                    form = form.text("origin_pack_id", origin.clone());
                }
                crate::net::credential_http_for_url(url.as_str())
                    .post(url.clone())
                    .bearer_auth(token.expose_secret())
                    .timeout(Duration::from_secs(30))
                    .multipart(form)
            })
            .await?;
        let response = self.validate_authenticated_response(response).await?;
        let result: MarketplaceUploadResult = response.json().await.map_err(|_| {
            BackendError::new(
                BackendErrorCode::Provider,
                "parse Marketplace upload result failed",
            )
        })?;
        if !is_remote_pack_id(&result.id) {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "Marketplace upload returned an invalid pack id",
            ));
        }
        if origin_pack_id.is_none() {
            let author_login = self.preferences.get().marketplace_dev_login;
            if self
                .style_packs
                .set_origin(&pack_id, Some(result.id.clone()), Some(author_login))
                .is_ok()
            {
                let revision = self.style_pack_revision.fetch_add(1, Ordering::AcqRel) + 1;
                self.events.publish(
                    None,
                    BackendEventKind::StylePacksChanged(StylePackChange { revision }),
                );
            }
        }
        Ok(result)
    }

    async fn start_device_flow_impl(&self) -> Result<OAuthDeviceFlow, BackendError> {
        let generation = self.device_flows.lock().await.begin_start();
        let result = async {
            let response = self
                .send_with_retry(|| {
                    crate::net::credential_http_for_url(self.config.github_device_code_url.as_str())
                        .post(self.config.github_device_code_url.clone())
                        .header("Accept", "application/json")
                        .timeout(Duration::from_secs(15))
                        .form(&[
                            ("client_id", self.config.github_client_id.as_str()),
                            ("scope", "read:user"),
                        ])
                })
                .await?;
            let status = response.status();
            if status.is_redirection() {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "GitHub device flow rejected redirect",
                ));
            }
            if !status.is_success() {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    format!("GitHub device flow HTTP {status}"),
                ));
            }
            let body: serde_json::Value = response.json().await.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "GitHub device flow returned a malformed response",
                )
            })?;
            let required = |name: &str| {
                body[name]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Provider,
                            format!("GitHub device flow response is missing {name}"),
                        )
                    })
            };
            let device_code = SecretDeviceCode(required("device_code")?);
            let user_code = required("user_code")?;
            let verification_uri = required("verification_uri")?;
            let verification_url = reqwest::Url::parse(&verification_uri).map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "GitHub device flow returned an invalid verification URI",
                )
            })?;
            if verification_url.scheme() != "https" {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "GitHub device flow requires an HTTPS verification URI",
                ));
            }
            let interval_secs = body["interval"].as_u64().unwrap_or(5);
            let expires_in_secs = body["expires_in"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::Provider,
                        "GitHub device flow returned an invalid expiry",
                    )
                })?;
            if interval_secs == 0 {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "GitHub device flow returned an invalid poll interval",
                ));
            }
            let flow_id = uuid::Uuid::new_v4().to_string();
            self.device_flows.lock().await.activate(
                generation,
                flow_id.clone(),
                device_code,
                Instant::now() + Duration::from_secs(expires_in_secs),
                Duration::from_secs(interval_secs),
            )?;
            Ok(OAuthDeviceFlow {
                flow_id,
                user_code,
                verification_uri,
                expires_in_secs,
                interval_secs,
            })
        }
        .await;
        if result.is_err() {
            self.device_flows
                .lock()
                .await
                .invalidate_generation(generation);
        }
        result
    }

    async fn poll_device_flow_impl(
        &self,
        flow_id: String,
    ) -> Result<OAuthPollResult, BackendError> {
        let lease = match self
            .device_flows
            .lock()
            .await
            .poll_permit(&flow_id, Instant::now())
        {
            PollPermit::Ready(lease) => lease,
            PollPermit::TooSoon => return Ok(OAuthPollResult::Pending),
            PollPermit::Invalid(message) => {
                return Ok(OAuthPollResult::Error {
                    message: message.into(),
                })
            }
        };
        let token_response = self
            .send_with_retry(|| {
                crate::net::credential_http_for_url(self.config.github_access_token_url.as_str())
                    .post(self.config.github_access_token_url.clone())
                    .header("Accept", "application/json")
                    .timeout(Duration::from_secs(15))
                    .form(&[
                        ("client_id", self.config.github_client_id.as_str()),
                        ("device_code", lease.device_code.0.as_str()),
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ])
            })
            .await?;
        let token_status = token_response.status();
        if token_status.is_redirection() {
            self.device_flows.lock().await.cancel(Some(&flow_id));
            return Ok(OAuthPollResult::Error {
                message: "GitHub token exchange rejected redirect".into(),
            });
        }
        if !token_status.is_success() {
            self.device_flows.lock().await.cancel(Some(&flow_id));
            return Ok(OAuthPollResult::Error {
                message: format!("GitHub token exchange HTTP {token_status}"),
            });
        }
        let body: serde_json::Value = match token_response.json().await {
            Ok(body) => body,
            Err(_) => {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                return Ok(OAuthPollResult::Error {
                    message: "GitHub token exchange returned a malformed response".into(),
                });
            }
        };
        if !self
            .device_flows
            .lock()
            .await
            .lease_is_active(&lease, Instant::now())
        {
            return Ok(OAuthPollResult::Error {
                message: OAUTH_FLOW_CANCELLED.into(),
            });
        }

        if let Some(token) = body["access_token"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let access_token = SecretValue::new(token);
            let user_response = self
                .send_with_retry(|| {
                    crate::net::credential_http_for_url(self.config.github_user_url.as_str())
                        .get(self.config.github_user_url.clone())
                        .header("Accept", "application/vnd.github+json")
                        .timeout(Duration::from_secs(15))
                        .bearer_auth(access_token.expose_secret())
                })
                .await?;
            let user_status = user_response.status();
            if user_status.is_redirection() {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                return Ok(OAuthPollResult::Error {
                    message: "GitHub user verification rejected redirect".into(),
                });
            }
            if !user_status.is_success() {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                return Ok(OAuthPollResult::Error {
                    message: format!("GitHub user verification HTTP {user_status}"),
                });
            }
            let user_body: serde_json::Value = match user_response.json().await {
                Ok(body) => body,
                Err(_) => {
                    self.device_flows.lock().await.cancel(Some(&flow_id));
                    return Ok(OAuthPollResult::Error {
                        message: "GitHub user verification returned a malformed response".into(),
                    });
                }
            };
            let login = user_body["login"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let Some(login) = login else {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                return Ok(OAuthPollResult::Error {
                    message: "GitHub user verification returned no login".into(),
                });
            };

            let mut flows = self.device_flows.lock().await;
            if !flows.lease_is_active(&lease, Instant::now()) {
                return Ok(OAuthPollResult::Error {
                    message: OAUTH_FLOW_CANCELLED.into(),
                });
            }
            if self
                .credential_store
                .write(Self::token_key()?, access_token)
                .await
                .is_err()
            {
                return Ok(OAuthPollResult::Error {
                    message: "save Marketplace credential failed".into(),
                });
            }
            self.auth_tombstoned.store(false, Ordering::Release);
            let mut preferences = self.preferences.get();
            preferences.marketplace_dev_login = login.clone();
            let _ = self.preferences.set(preferences);
            flows.consume(&lease);
            return Ok(OAuthPollResult::Authorized { login });
        }

        match body["error"].as_str().unwrap_or("") {
            "authorization_pending" => Ok(OAuthPollResult::Pending),
            "slow_down" => {
                if self
                    .device_flows
                    .lock()
                    .await
                    .apply_slow_down(&lease, Instant::now())
                {
                    Ok(OAuthPollResult::SlowDown)
                } else {
                    Ok(OAuthPollResult::Error {
                        message: OAUTH_FLOW_CANCELLED.into(),
                    })
                }
            }
            "expired_token" => {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                Ok(OAuthPollResult::Error {
                    message: OAUTH_FLOW_EXPIRED.into(),
                })
            }
            "access_denied" => {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                Ok(OAuthPollResult::Error {
                    message: "GitHub authorization was denied".into(),
                })
            }
            _ => {
                self.device_flows.lock().await.cancel(Some(&flow_id));
                Ok(OAuthPollResult::Error {
                    message: "GitHub token exchange returned a malformed response".into(),
                })
            }
        }
    }
}

fn is_remote_pack_id(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn is_local_pack_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

impl MarketplaceApi for MarketplaceService {
    fn list(
        &self,
        query: MarketplaceQuery,
    ) -> BoxFuture<'static, Result<Vec<MarketplaceListItem>, BackendError>> {
        let mut url = match self.public_url("packs") {
            Ok(url) => url,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(query) = query
                .query
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                pairs.append_pair("q", query);
            }
            if let Some(sort) = query
                .sort
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                pairs.append_pair("sort", sort);
            }
            if let Some(limit) = query.limit {
                pairs.append_pair("limit", &limit.to_string());
            }
        }
        Box::pin(async move {
            let response = crate::net::credential_http_for_url(url.as_str())
                .get(url)
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|_| {
                    BackendError::new(
                        BackendErrorCode::Provider,
                        "Marketplace list request failed",
                    )
                })?;
            if response.status().is_redirection() {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "marketplace_public_redirect_rejected",
                ));
            }
            if !response.status().is_success() {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    format!("Marketplace list HTTP {}", response.status()),
                ));
            }
            response.json().await.map_err(|_| {
                BackendError::new(BackendErrorCode::Provider, "parse Marketplace list failed")
            })
        })
    }

    fn detail(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<MarketplaceDetail, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            if !is_remote_pack_id(&pack_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "invalid Marketplace pack id",
                ));
            }
            let url = this.public_url(&format!("packs/{pack_id}"))?;
            let response = this.public_get(url, Duration::from_secs(15)).await?;
            response.json().await.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "parse Marketplace detail failed",
                )
            })
        })
    }

    fn install(&self, pack_id: String) -> BoxFuture<'static, Result<StylePack, BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.install_impl(pack_id).await })
    }

    fn download_archive(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.download_archive_impl(&pack_id).await })
    }

    fn upload(
        &self,
        pack_id: String,
        origin_pack_id: Option<String>,
    ) -> BoxFuture<'static, Result<MarketplaceUploadResult, BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.upload_impl(pack_id, origin_pack_id).await })
    }

    fn toggle_like(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<MarketplaceLikeResult, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            if !is_remote_pack_id(&pack_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "invalid Marketplace pack id",
                ));
            }
            let response = this
                .authenticated_response(
                    reqwest::Method::POST,
                    &format!("packs/{pack_id}/like"),
                    Duration::from_secs(10),
                )
                .await?;
            response.json().await.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "parse Marketplace like result failed",
                )
            })
        })
    }

    fn delete(&self, pack_id: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            if !is_remote_pack_id(&pack_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "invalid Marketplace pack id",
                ));
            }
            this.authenticated_response(
                reqwest::Method::DELETE,
                &format!("packs/{pack_id}"),
                Duration::from_secs(15),
            )
            .await?;
            Ok(())
        })
    }

    fn my_likes(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            let response = this
                .authenticated_response(reqwest::Method::GET, "me/likes", Duration::from_secs(10))
                .await?;
            response.json().await.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "parse Marketplace liked packs failed",
                )
            })
        })
    }

    fn my_packs(&self) -> BoxFuture<'static, Result<Vec<MarketplaceMyPackItem>, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            let response = this
                .authenticated_response(reqwest::Method::GET, "me/packs", Duration::from_secs(10))
                .await?;
            response.json().await.map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Provider,
                    "parse Marketplace published packs failed",
                )
            })
        })
    }

    fn auth_status(&self) -> BoxFuture<'static, Result<MarketplaceAuthStatus, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            if this.auth_tombstoned.load(Ordering::Acquire) {
                return Ok(MarketplaceAuthStatus { signed_in: false });
            }
            let signed_in = this
                .credential_store
                .read(Self::token_key()?)
                .await?
                .is_some();
            Ok(MarketplaceAuthStatus { signed_in })
        })
    }

    fn start_device_flow(&self) -> BoxFuture<'static, Result<OAuthDeviceFlow, BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.start_device_flow_impl().await })
    }

    fn poll_device_flow(
        &self,
        flow_id: String,
    ) -> BoxFuture<'static, Result<OAuthPollResult, BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.poll_device_flow_impl(flow_id).await })
    }

    fn cancel_device_flow(
        &self,
        flow_id: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            this.device_flows.lock().await.cancel(flow_id.as_deref());
            Ok(())
        })
    }

    fn logout(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let this = self.clone();
        Box::pin(async move { this.clear_authentication().await })
    }
}
