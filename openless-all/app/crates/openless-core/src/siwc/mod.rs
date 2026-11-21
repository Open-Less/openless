//! Official Sign in with ChatGPT. OAuth secrets never enter the provider credential map.
pub(crate) mod inference;
mod store;
pub use store::{locked_head, SiwcVault};
#[cfg(test)]
mod tests;

use crate::{BackendError, BackendErrorCode, SecretValue};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PROVIDER_ID: &str = "chatgpt_siwc";
pub const API_BASE: &str = "https://api.openai.com/v1";
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
const ISSUER: &str = "https://auth.openai.com";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const DIRECT: &str = "chatgpt.tokens.use.direct";
const AUTH_TIMEOUT: Duration = Duration::from_secs(600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SiwcError {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_shape: Option<String>,
}
pub fn error(code: &str) -> SiwcError {
    SiwcError {
        code: code.into(),
        http_status: None,
        param: None,
        request_id: None,
        body_shape: None,
    }
}
impl std::fmt::Display for SiwcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "siwc:{}",
            serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
        )
    }
}
impl std::error::Error for SiwcError {}
impl SiwcError {
    fn with_http_context(mut self, status: u16, request_id: Option<&str>) -> Self {
        self.http_status = Some(status);
        self.request_id = safe_identifier(request_id);
        self
    }

    /// Keep this conversion explicit so unrelated host Result inference remains unchanged.
    pub fn into_backend(self) -> BackendError {
        let code = if self.code == "cancelled" {
            BackendErrorCode::Cancelled
        } else {
            BackendErrorCode::Provider
        };
        let message = match self.code.as_str() {
            "subscription_sharing_usage_limit_exceeded" | "rate_limited" => "ChatGPT plan or app usage limit reached. Open Manage usage in the ChatGPT Plan channel.",
            "subscription_sharing_user_not_eligible" | "forbidden" => "This ChatGPT account, workspace or region is not eligible for plan usage.",
            "usage_disabled" => "Enable ChatGPT plan usage in this channel's settings.",
            "sign_in_required" | "reauthorization_required" | "refresh_interrupted" | "session_changed" => "Sign in to ChatGPT again in the ChatGPT Plan channel.",
            "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" | "unavailable" => "ChatGPT plan usage is temporarily unavailable. Retry later.",
            "subscription_sharing_unsupported_capability" => "This ChatGPT model or capability is unavailable. Refresh the model list.",
            "response_incomplete" => "No completed response was received from ChatGPT.",
            "cancelled" => "ChatGPT request cancelled.",
            _ => "ChatGPT Plan request failed. Check this channel's connection status.",
        };
        let mut result = BackendError::new(code, message);
        result.details = Some(json!({"siwc": self}));
        result
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn random() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    URL_SAFE_NO_PAD.encode(bytes)
}
fn safe_identifier(value: Option<&str>) -> Option<String> {
    value
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 160
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-[]".contains(&b))
        })
        .map(str::to_owned)
}
pub(crate) fn response_error(status: u16, body: &Value, request_id: Option<&str>) -> SiwcError {
    let detail = body.get("error").unwrap_or(body);
    let mut failure = error(
        &safe_identifier(detail["code"].as_str()).unwrap_or_else(|| {
            match status {
                401 => "unauthorized",
                403 => "forbidden",
                429 => "rate_limited",
                503 => "unavailable",
                _ => "request_failed",
            }
            .into()
        }),
    );
    failure.http_status = Some(status);
    failure.param = safe_identifier(detail["param"].as_str());
    failure.request_id = safe_identifier(request_id);
    failure.body_shape = Some(
        if body.get("error").is_some() {
            "error"
        } else if body.get("detail").is_some() {
            "detail"
        } else {
            "other"
        }
        .into(),
    );
    failure
}

#[derive(Clone, Serialize, Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
struct Tokens {
    access: String,
    refresh: String,
    id: String,
    nonce: String,
    expires_at: u64,
    scopes: Vec<String>,
}
impl Tokens {
    fn plan(&self) -> bool {
        self.scopes.iter().any(|s| s == DIRECT)
            && self.scopes.iter().any(|s| s == "resource.invoke")
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Registration {
    id: String,
    issuer: String,
    subject: String,
    client_id: String,
    email: Option<String>,
    epoch: u64,
    tokens: Option<Tokens>,
    refreshing: Option<(String, u64)>,
    failure: Option<SiwcError>,
    welcomed: bool,
    #[serde(default)]
    revocation_confirmed: Option<bool>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Binding {
    registration: Option<String>,
    attempt: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Data {
    host_id: String,
    registrations: BTreeMap<String, Registration>,
    channels: BTreeMap<String, Binding>,
    #[serde(default)]
    pending_clients: BTreeMap<String, String>,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            registrations: BTreeMap::new(),
            channels: BTreeMap::new(),
            pending_clients: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiwcAccount {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
}
impl Registration {
    fn account(&self) -> SiwcAccount {
        let suffix = &self.client_id[self.client_id.len().saturating_sub(6)..];
        SiwcAccount {
            id: self.id.clone(),
            email: self.email.clone(),
            label: format!(
                "{} · {}",
                self.email.as_deref().unwrap_or("ChatGPT"),
                suffix
            ),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiwcStatus {
    pub state: String,
    pub account: Option<SiwcAccount>,
    pub accounts: Vec<SiwcAccount>,
    pub attempt_id: Option<String>,
    pub error: Option<SiwcError>,
    pub welcome_pending: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SiwcModel {
    pub slug: String,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiwcDisconnect {
    pub remote_revoked: bool,
}

/// Host-only launch data: never serialize, log, or send the authorization URL to WebView.
pub struct SiwcLaunch {
    pub attempt_id: String,
    url: SecretValue,
}
impl SiwcLaunch {
    pub fn authorization_url(&self) -> &str {
        self.url.expose_secret()
    }
}
struct Pending {
    id: String,
    cancellation: crate::provider_transport::ProviderCancellation,
}
#[derive(Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    revocation_endpoint: String,
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    exp: u64,
    iat: u64,
    nonce: Option<String>,
    email: Option<String>,
}
#[derive(Clone)]
struct Endpoints {
    issuer: String,
    discovery: String,
    api: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            issuer: ISSUER.into(),
            discovery: format!("{ISSUER}/.well-known/openid-configuration"),
            api: API_BASE.into(),
        }
    }
}
pub struct SiwcService {
    store: Arc<store::Store>,
    endpoints: Endpoints,
    pending: parking_lot::Mutex<BTreeMap<String, Pending>>,
    errors: parking_lot::Mutex<BTreeMap<String, SiwcError>>,
    jwks: tokio::sync::Mutex<Option<(u64, jsonwebtoken::jwk::JwkSet)>>,
}
impl SiwcService {
    pub fn new(vault: Arc<dyn SiwcVault>) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(store::Store(vault)),
            endpoints: Endpoints::default(),
            pending: Default::default(),
            errors: Default::default(),
            jwks: Default::default(),
        })
    }
    async fn load(&self) -> Result<store::Snapshot, SiwcError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.load())
            .await
            .map_err(|_| error("storage_failed"))?
    }
    async fn change<T: Send + 'static>(
        &self,
        mut f: impl FnMut(&mut Data) -> Result<T, SiwcError> + Send + 'static,
    ) -> Result<T, SiwcError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            for _ in 0..16 {
                let before = store.load()?;
                let mut next = before.data.clone();
                let result = f(&mut next)?;
                if store.commit(&before, &next)? {
                    return Ok(result);
                }
            }
            Err(error("busy"))
        })
        .await
        .map_err(|_| error("storage_failed"))?
    }
    pub async fn status(&self, channel: &str) -> Result<SiwcStatus, SiwcError> {
        let data = self.load().await?.data;
        let binding = data.channels.get(channel);
        let registration = binding
            .and_then(|b| b.registration.as_ref())
            .and_then(|id| data.registrations.get(id));
        let attempt_id = self.pending.lock().get(channel).map(|p| p.id.clone());
        let mut state = "signed_out";
        let mut failure = self.errors.lock().get(channel).cloned();
        if let Some(r) = registration {
            failure = failure.or_else(|| r.failure.clone());
            if r.tokens.is_none() && r.revocation_confirmed == Some(false) {
                failure = Some(error("remote_revocation_unconfirmed"));
            }
            state = match &r.tokens {
                None => "reauthorization_required",
                Some(t) if !t.plan() => "usage_disabled",
                Some(_)
                    if r.failure
                        .as_ref()
                        .is_some_and(|e| e.code == "subscription_sharing_usage_limit_exceeded") =>
                {
                    "usage_limited"
                }
                Some(_)
                    if r.refreshing
                        .as_ref()
                        .is_some_and(|(_, since)| now().saturating_sub(*since) >= 120) =>
                {
                    "reauthorization_required"
                }
                Some(_) => "connected",
            };
        }
        if attempt_id.is_some() {
            state = "authorizing";
        }
        Ok(SiwcStatus {
            state: state.into(),
            account: registration.map(Registration::account),
            accounts: data
                .registrations
                .values()
                .map(Registration::account)
                .collect(),
            attempt_id,
            error: failure,
            welcome_pending: registration
                .is_some_and(|r| !r.welcomed && r.tokens.as_ref().is_some_and(Tokens::plan)),
        })
    }
    pub fn authorizing(&self, channel: &str, attempt: &str) -> bool {
        self.pending
            .lock()
            .get(channel)
            .is_some_and(|p| p.id == attempt)
    }
    pub async fn has_binding(&self, channel: &str) -> Result<bool, SiwcError> {
        Ok(self
            .load()
            .await?
            .data
            .channels
            .get(channel)
            .is_some_and(|b| b.registration.is_some() || b.attempt.is_some()))
    }
    pub async fn select_account(
        &self,
        channel: String,
        registration: String,
    ) -> Result<(), SiwcError> {
        self.cancel_sign_in(&channel).await?;
        self.errors.lock().remove(&channel);
        self.change(move |data| {
            if !data.registrations.contains_key(&registration) {
                return Err(error("account_missing"));
            }
            data.channels.insert(
                channel.clone(),
                Binding {
                    registration: Some(registration.clone()),
                    attempt: None,
                },
            );
            Ok(())
        })
        .await
    }
    pub async fn acknowledge_welcome(&self, channel: String) -> Result<(), SiwcError> {
        self.change(move |data| {
            selected_mut(data, &channel)?.welcomed = true;
            Ok(())
        })
        .await
    }
    pub async fn retry_usage(&self, channel: String) -> Result<(), SiwcError> {
        self.change(move |data| {
            selected_mut(data, &channel)?.failure = None;
            Ok(())
        })
        .await
    }
    pub async fn cancel_sign_in(&self, channel: &str) -> Result<(), SiwcError> {
        let local = self
            .pending
            .lock()
            .get(channel)
            .map(|pending| pending.id.clone());
        let attempt = match local {
            Some(id) => Some(id),
            None => self
                .load()
                .await?
                .data
                .channels
                .get(channel)
                .and_then(|b| b.attempt.clone()),
        };
        if let Some(attempt) = attempt {
            self.cancel_attempt(channel, &attempt).await?;
        }
        Ok(())
    }
    pub(crate) async fn cancel_attempt(
        &self,
        channel: &str,
        attempt: &str,
    ) -> Result<(), SiwcError> {
        {
            let mut pending = self.pending.lock();
            if pending.get(channel).is_some_and(|p| p.id == attempt) {
                if let Some(pending) = pending.remove(channel) {
                    pending.cancellation.cancel();
                }
            }
        }
        let snapshot = self.load().await?;
        if snapshot
            .data
            .channels
            .get(channel)
            .and_then(|b| b.attempt.as_deref())
            != Some(attempt)
        {
            return Ok(());
        }
        let channel = channel.to_owned();
        let attempt = attempt.to_owned();
        self.change(move |data| {
            if let Some(binding) = data.channels.get_mut(&channel) {
                if binding.attempt.as_deref() == Some(&attempt) {
                    binding.attempt = None;
                }
            }
            Ok(())
        })
        .await
    }
    pub async fn detach(&self, channel: String) -> Result<(), SiwcError> {
        self.cancel_sign_in(&channel).await?;
        let snapshot = self.load().await?;
        if !snapshot.data.channels.contains_key(&channel)
            && !snapshot.data.pending_clients.contains_key(&channel)
        {
            return Ok(());
        }
        self.change(move |data| {
            data.channels.remove(&channel);
            data.pending_clients.remove(&channel);
            Ok(())
        })
        .await
    }
    async fn discovery(&self) -> Result<Discovery, SiwcError> {
        let value = json_response(
            self.client()
                .get(&self.endpoints.discovery)
                .timeout(HTTP_TIMEOUT)
                .send()
                .await,
        )
        .await?;
        let discovery: Discovery =
            serde_json::from_value(value).map_err(|_| error("discovery_invalid"))?;
        if discovery.issuer != self.endpoints.issuer {
            return Err(error("issuer_mismatch"));
        }
        let issuer =
            url::Url::parse(&self.endpoints.issuer).map_err(|_| error("discovery_invalid"))?;
        for endpoint in [
            &discovery.authorization_endpoint,
            &discovery.token_endpoint,
            &discovery.jwks_uri,
            &discovery.revocation_endpoint,
        ] {
            let parsed = url::Url::parse(endpoint).map_err(|_| error("discovery_invalid"))?;
            if parsed.origin() != issuer.origin()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err(error("discovery_invalid"));
            }
        }
        Ok(discovery)
    }
    fn client(&self) -> reqwest::Client {
        crate::net::credential_http_for_url(&self.endpoints.issuer)
    }
    pub async fn begin_sign_in(
        self: &Arc<Self>,
        channel: String,
        new_account: bool,
        enable_usage: bool,
    ) -> Result<SiwcLaunch, SiwcError> {
        self.cancel_sign_in(&channel).await?;
        self.errors.lock().remove(&channel);
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| error("callback_unavailable"))?;
        let redirect = format!(
            "http://127.0.0.1:{}/auth/callback",
            listener
                .local_addr()
                .map_err(|_| error("callback_unavailable"))?
                .port()
        );
        let id = uuid::Uuid::new_v4().to_string();
        let attempt = id.clone();
        let key = channel.clone();
        let (host_id, account, pending_client) = self
            .change(move |data| {
                let binding = data.channels.entry(key.clone()).or_default();
                binding.attempt = Some(attempt.clone());
                let account = if new_account {
                    None
                } else {
                    binding
                        .registration
                        .as_ref()
                        .and_then(|id| data.registrations.get(id))
                        .cloned()
                };
                if new_account {
                    data.pending_clients.remove(&key);
                }
                Ok((
                    data.host_id.clone(),
                    account,
                    data.pending_clients.get(&key).cloned(),
                ))
            })
            .await?;
        let client_id = account
            .as_ref()
            .map(|r| r.client_id.clone())
            .or(pending_client);
        let state = random();
        let nonce = random();
        let verifier = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = url::Url::parse(&format!("{}/api/accounts/authorize", self.endpoints.issuer))
            .map_err(|_| error("discovery_invalid"))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair(
                    "client_id",
                    client_id.as_deref().unwrap_or("dynamic_agent_client"),
                )
                .append_pair("ext_agent_host_id", &host_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &redirect)
                .append_pair("scope", SCOPES)
                .append_pair("resource", API_BASE)
                .append_pair("state", &state)
                .append_pair("nonce", &nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &challenge);
            if let Some(r) = &account {
                if let Some(t) = &r.tokens {
                    if !t.id.is_empty() {
                        query.append_pair("id_token_hint", &t.id);
                    }
                }
                if let Some(email) = &r.email {
                    query.append_pair("login_hint", email);
                }
            }
            if client_id.is_none() {
                query.append_pair("agent_name_hint", "OpenLess");
            }
            if enable_usage {
                query.append_pair("prompt", "consent");
            }
        }
        let cancellation = crate::provider_transport::ProviderCancellation::new();
        self.pending.lock().insert(
            channel.clone(),
            Pending {
                id: id.clone(),
                cancellation: cancellation.clone(),
            },
        );
        let service = self.clone();
        let task_id = id.clone();
        tokio::spawn(async move {
            let task = service.finish_sign_in(
                &channel, &task_id, listener, account, client_id, redirect, state, nonce, verifier,
            );
            let result = tokio::select! {
                _ = wait_cancel(&cancellation) => Err(error("cancelled")),
                result = tokio::time::timeout(AUTH_TIMEOUT, task) => result.unwrap_or_else(|_| Err(error("authorization_timeout"))),
            };
            let current = {
                let mut pending = service.pending.lock();
                let current = pending.get(&channel).is_some_and(|p| p.id == task_id);
                if current {
                    pending.remove(&channel);
                    if let Err(e) = result {
                        service.errors.lock().insert(channel.clone(), e);
                    }
                }
                current
            };
            if current {
                let _ = service
                    .change(move |data| {
                        if let Some(b) = data.channels.get_mut(&channel) {
                            if b.attempt.as_deref() == Some(&task_id) {
                                b.attempt = None;
                            }
                        }
                        Ok(())
                    })
                    .await;
            }
        });
        Ok(SiwcLaunch {
            attempt_id: id,
            url: SecretValue::new(url.to_string()),
        })
    }
    #[allow(clippy::too_many_arguments)]
    async fn finish_sign_in(
        &self,
        channel: &str,
        attempt: &str,
        listener: tokio::net::TcpListener,
        account: Option<Registration>,
        expected_client: Option<String>,
        redirect: String,
        state: String,
        nonce: String,
        verifier: String,
    ) -> Result<(), SiwcError> {
        let (mut socket, params) = callback(&listener, &state).await?;
        let result = async {
            if params.contains_key("error") {
                return Err(error("access_denied"));
            }
            let code = params
                .get("code")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| error("callback_invalid"))?;
            let client = match expected_client {
                Some(client) => {
                    if params.get("client_id").is_some_and(|c| c != &client) {
                        return Err(error("client_mismatch"));
                    }
                    client
                }
                None => params
                    .get("client_id")
                    .filter(|s| {
                        s.as_str() != "dynamic_agent_client" && safe_identifier(Some(s)).is_some()
                    })
                    .cloned()
                    .ok_or_else(|| error("registration_incomplete"))?,
            };
            let pending_channel = channel.to_owned();
            let pending_attempt = attempt.to_owned();
            let pending_client = client.clone();
            self.change(move |data| {
                if data
                    .channels
                    .get(&pending_channel)
                    .and_then(|b| b.attempt.as_deref())
                    != Some(&pending_attempt)
                {
                    return Err(error("cancelled"));
                }
                data.pending_clients
                    .insert(pending_channel.clone(), pending_client.clone());
                Ok(())
            })
            .await?;
            let discovery = self.discovery().await?;
            let value = json_response(
                self.client()
                    .post(&discovery.token_endpoint)
                    .timeout(HTTP_TIMEOUT)
                    .form(&[
                        ("grant_type", "authorization_code"),
                        ("client_id", &client),
                        ("code", code),
                        ("code_verifier", &verifier),
                        ("redirect_uri", &redirect),
                        ("resource", API_BASE),
                    ])
                    .send()
                    .await,
            )
            .await?;
            let id_token = value["id_token"]
                .as_str()
                .ok_or_else(|| error("id_token_missing"))?;
            let claims = self
                .verify_identity(id_token, &client, Some(&nonce), &discovery)
                .await?;
            if account
                .as_ref()
                .is_some_and(|r| r.subject != claims.sub || r.issuer != claims.iss)
            {
                return Err(error("identity_mismatch"));
            }
            let tokens = parse_tokens(&value, None, &nonce)?;
            let channel = channel.to_string();
            let attempt = attempt.to_string();
            self.change(move |data| {
                if data
                    .channels
                    .get(&channel)
                    .and_then(|b| b.attempt.as_deref())
                    != Some(&attempt)
                {
                    return Err(error("cancelled"));
                }
                if let Some(old) = &account {
                    if data
                        .registrations
                        .get(&old.id)
                        .is_none_or(|r| r.epoch != old.epoch)
                    {
                        return Err(error("cancelled"));
                    }
                }
                let existing = data
                    .registrations
                    .values()
                    .find(|r| {
                        r.client_id == client && r.subject == claims.sub && r.issuer == claims.iss
                    })
                    .cloned();
                let id = existing
                    .as_ref()
                    .map(|r| r.id.clone())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                data.registrations.insert(
                    id.clone(),
                    Registration {
                        id: id.clone(),
                        issuer: claims.iss.clone(),
                        subject: claims.sub.clone(),
                        client_id: client.clone(),
                        email: claims.email.clone(),
                        epoch: existing.as_ref().map_or(1, |r| r.epoch + 1),
                        tokens: Some(tokens.clone()),
                        refreshing: None,
                        failure: None,
                        welcomed: existing.is_some_and(|r| r.welcomed),
                        revocation_confirmed: None,
                    },
                );
                data.channels.insert(
                    channel.clone(),
                    Binding {
                        registration: Some(id),
                        attempt: None,
                    },
                );
                data.pending_clients.remove(&channel);
                Ok(())
            })
            .await
        }
        .await;
        let text = if result.is_ok() {
            "ChatGPT authorization complete. You can return to OpenLess."
        } else {
            "Authorization was not completed. Return to OpenLess to retry."
        };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", text.len(), text);
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            socket.write_all(response.as_bytes()),
        )
        .await;
        result
    }
    async fn verify_identity(
        &self,
        token: &str,
        client: &str,
        nonce: Option<&str>,
        discovery: &Discovery,
    ) -> Result<Claims, SiwcError> {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation};
        let header = jsonwebtoken::decode_header(token).map_err(|_| error("id_token_invalid"))?;
        if header.alg != Algorithm::RS256 {
            return Err(error("id_token_invalid"));
        }
        let kid = header.kid.ok_or_else(|| error("id_token_invalid"))?;
        let mut cache = self.jwks.lock().await;
        if cache
            .as_ref()
            .is_none_or(|(at, set)| now().saturating_sub(*at) > 3600 || set.find(&kid).is_none())
        {
            let value = json_response(
                self.client()
                    .get(&discovery.jwks_uri)
                    .timeout(HTTP_TIMEOUT)
                    .send()
                    .await,
            )
            .await?;
            *cache = Some((
                now(),
                serde_json::from_value(value).map_err(|_| error("jwks_invalid"))?,
            ));
        }
        let jwk = cache
            .as_ref()
            .and_then(|(_, set)| set.find(&kid))
            .ok_or_else(|| error("id_token_invalid"))?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| error("id_token_invalid"))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&discovery.issuer]);
        validation.set_audience(&[client]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp", "iat"]);
        validation.leeway = 5;
        validation.validate_nbf = true;
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|_| error("id_token_invalid"))?
            .claims;
        if claims.sub.is_empty()
            || claims.exp == 0
            || claims.iat > now() + 5
            || nonce.is_some_and(|n| claims.nonce.as_deref() != Some(n))
        {
            return Err(error("id_token_invalid"));
        }
        Ok(claims)
    }
    pub(crate) async fn access(self: &Arc<Self>, channel: &str) -> Result<SiwcAccess, SiwcError> {
        let data = self.load().await?.data;
        let r = selected(&data, channel)?;
        Ok(SiwcAccess {
            service: self.clone(),
            registration: r.id.clone(),
            epoch: r.epoch,
        })
    }
    async fn token(self: &Arc<Self>, id: &str, epoch: u64) -> Result<SecretValue, SiwcError> {
        let mut waited = false;
        loop {
            let data = self.load().await?.data;
            let r = data
                .registrations
                .get(id)
                .filter(|r| r.epoch == epoch)
                .ok_or_else(|| error("session_changed"))?;
            let tokens = r
                .tokens
                .as_ref()
                .ok_or_else(|| error("reauthorization_required"))?;
            if !tokens.plan() {
                return Err(error("usage_disabled"));
            }
            if let Some(failure) = r.failure.as_ref().filter(|e| {
                waited
                    || matches!(
                        e.code.as_str(),
                        "subscription_sharing_usage_limit_exceeded"
                            | "subscription_sharing_user_not_eligible"
                    )
            }) {
                return Err(failure.clone());
            }
            if let Some((_, since)) = &r.refreshing {
                if now().saturating_sub(*since) >= 120 {
                    return Err(error("refresh_interrupted"));
                }
                waited = true;
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            if tokens.expires_at > now() + if waited { 0 } else { 60 } {
                return Ok(SecretValue::new(tokens.access.clone()));
            }
            if tokens.refresh.is_empty() {
                return Err(error("reauthorization_required"));
            }
            // A cancelled inference must not abandon a rotating-token transaction.
            let service = self.clone();
            let id = id.to_owned();
            tokio::spawn(async move { service.perform_refresh(id, epoch).await })
                .await
                .map_err(|_| error("refresh_interrupted"))??;
            waited = true;
        }
    }
    async fn perform_refresh(&self, id: String, epoch: u64) -> Result<(), SiwcError> {
        // Discovery cannot rotate tokens; complete it before claiming the refresh transaction.
        let discovery = self.discovery().await?;
        let operation = uuid::Uuid::new_v4().to_string();
        let key = id.clone();
        let op = operation.clone();
        let original = self
            .change(move |data| {
                let r = data
                    .registrations
                    .get_mut(&key)
                    .filter(|r| r.epoch == epoch)
                    .ok_or_else(|| error("session_changed"))?;
                if r.refreshing.is_some()
                    || r.tokens.as_ref().is_some_and(|t| t.expires_at > now() + 60)
                {
                    return Ok(None);
                }
                r.refreshing = Some((op.clone(), now()));
                r.failure = None;
                Ok(Some(r.clone()))
            })
            .await?;
        let Some(original) = original else {
            return Ok(());
        };
        match self.refresh_tokens(&original, &discovery).await {
            Ok(tokens) => {
                self.change(move |data| {
                    let r = data
                        .registrations
                        .get_mut(&id)
                        .filter(|r| {
                            r.epoch == epoch
                                && r.refreshing
                                    .as_ref()
                                    .is_some_and(|(id, _)| id == &operation)
                        })
                        .ok_or_else(|| error("session_changed"))?;
                    r.tokens = Some(tokens.clone());
                    r.refreshing = None;
                    r.failure = None;
                    Ok(())
                })
                .await
            }
            Err(failure) => {
                let returned = failure.clone();
                self.change(move |data| {
                    if let Some(r) = data.registrations.get_mut(&id).filter(|r| {
                        r.epoch == epoch
                            && r.refreshing
                                .as_ref()
                                .is_some_and(|(id, _)| id == &operation)
                    }) {
                        if terminal_refresh(&failure.code) {
                            r.tokens = None;
                            r.epoch += 1;
                            r.refreshing = None;
                        } else if failure.http_status.is_some() {
                            r.refreshing = None;
                        }
                        // Ambiguous network failure can mean the token already rotated; never replay it.
                        r.failure = Some(failure.clone());
                    }
                    Ok(())
                })
                .await?;
                Err(returned)
            }
        }
    }
    async fn refresh_tokens(
        &self,
        registration: &Registration,
        discovery: &Discovery,
    ) -> Result<Tokens, SiwcError> {
        let old = registration
            .tokens
            .as_ref()
            .ok_or_else(|| error("reauthorization_required"))?;
        let value = json_response(
            self.client()
                .post(&discovery.token_endpoint)
                .timeout(HTTP_TIMEOUT)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", &registration.client_id),
                    ("refresh_token", &old.refresh),
                    ("resource", API_BASE),
                ])
                .send()
                .await,
        )
        .await?;
        if let Some(token) = value["id_token"].as_str() {
            let claims = self
                .verify_identity(token, &registration.client_id, None, discovery)
                .await?;
            if claims.sub != registration.subject
                || claims.iss != registration.issuer
                || claims
                    .nonce
                    .as_ref()
                    .is_some_and(|nonce| nonce != &old.nonce)
            {
                return Err(error("identity_mismatch"));
            }
        }
        parse_tokens(&value, Some(old), &old.nonce)
    }
    pub async fn list_models(self: &Arc<Self>, channel: &str) -> Result<Vec<SiwcModel>, SiwcError> {
        let access = self.access(channel).await?;
        let token = access.token().await?;
        let response = crate::net::credential_http_for_url(&self.endpoints.api)
            .get(format!("{}/models", self.endpoints.api))
            .bearer_auth(token.expose_secret())
            .timeout(HTTP_TIMEOUT)
            .send()
            .await;
        let result = json_response(response).await;
        if let Err(e) = &result {
            access.record_error(e.clone()).await?;
        }
        let value = result?;
        access.check().await?;
        let models = value["models"]
            .as_array()
            .ok_or_else(|| error("models_invalid"))?;
        let mut seen = std::collections::HashSet::new();
        Ok(models
            .iter()
            .filter(|m| m["visibility"] == "list")
            .filter_map(|m| {
                let slug = m["slug"].as_str().filter(|s| !s.is_empty())?;
                if !seen.insert(slug.to_string()) {
                    return None;
                }
                Some(SiwcModel {
                    slug: slug.into(),
                    display_name: m["display_name"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(slug)
                        .into(),
                })
            })
            .collect())
    }
    pub async fn disconnect(&self, channel: &str) -> Result<SiwcDisconnect, SiwcError> {
        self.cancel_sign_in(channel).await?;
        let channel = channel.to_string();
        let (registration, channels) = self
            .change(move |data| {
                let r = selected_mut(data, &channel)?;
                let old = r.clone();
                r.epoch += 1;
                r.tokens = None;
                r.refreshing = None;
                r.failure = None;
                r.revocation_confirmed = Some(false);
                let id = r.id.clone();
                let channels: Vec<String> = data
                    .channels
                    .iter_mut()
                    .filter(|(_, b)| b.registration.as_deref() == Some(&id))
                    .map(|(channel, b)| {
                        b.attempt = None;
                        channel.clone()
                    })
                    .collect();
                Ok((old, channels))
            })
            .await?;
        for channel in channels {
            if let Some(pending) = self.pending.lock().remove(&channel) {
                pending.cancellation.cancel();
            }
        }
        let remote_revoked = if let Some(tokens) = &registration.tokens {
            if tokens.refresh.is_empty() {
                true
            } else if let Ok(discovery) = self.discovery().await {
                let mut success = false;
                for retry in 0..2 {
                    let response = self
                        .client()
                        .post(&discovery.revocation_endpoint)
                        .timeout(Duration::from_secs(10))
                        .form(&[
                            ("token", tokens.refresh.as_str()),
                            ("token_type_hint", "refresh_token"),
                            ("client_id", &registration.client_id),
                        ])
                        .send()
                        .await;
                    match response {
                        Ok(r) if r.status().as_u16() == 200 => {
                            success = true;
                            break;
                        }
                        Ok(r) if !r.status().is_server_error() => break,
                        _ => {}
                    }
                    if retry == 0 {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }
                }
                success
            } else {
                false
            }
        } else {
            registration.revocation_confirmed.unwrap_or(false)
        };
        // A racing rotation can supersede the revoked token; an HTTP 200 is then insufficient proof.
        let remote_revoked = remote_revoked && registration.refreshing.is_none();
        let id = registration.id.clone();
        let signed_out_epoch = registration.epoch + 1;
        self.change(move |data| {
            if let Some(current) = data
                .registrations
                .get_mut(&id)
                .filter(|r| r.epoch == signed_out_epoch && r.tokens.is_none())
            {
                current.revocation_confirmed = Some(remote_revoked);
            }
            Ok(())
        })
        .await?;
        Ok(SiwcDisconnect { remote_revoked })
    }
}
fn selected<'a>(data: &'a Data, channel: &str) -> Result<&'a Registration, SiwcError> {
    data.channels
        .get(channel)
        .and_then(|b| b.registration.as_ref())
        .and_then(|id| data.registrations.get(id))
        .ok_or_else(|| error("sign_in_required"))
}
fn selected_mut<'a>(data: &'a mut Data, channel: &str) -> Result<&'a mut Registration, SiwcError> {
    let id = data
        .channels
        .get(channel)
        .and_then(|b| b.registration.clone())
        .ok_or_else(|| error("sign_in_required"))?;
    data.registrations
        .get_mut(&id)
        .ok_or_else(|| error("sign_in_required"))
}
fn terminal_refresh(code: &str) -> bool {
    matches!(
        code,
        "invalid_grant"
            | "invalid_refresh_token"
            | "token_expired"
            | "refresh_token_expired"
            | "refresh_token_invalidated"
            | "refresh_token_reused"
            | "identity_mismatch"
            | "id_token_invalid"
    )
}
fn parse_tokens(value: &Value, old: Option<&Tokens>, nonce: &str) -> Result<Tokens, SiwcError> {
    let scopes = match value["scope"].as_str() {
        Some(s) => s.split_whitespace().map(str::to_owned).collect(),
        None => old.map(|t| t.scopes.clone()).unwrap_or_default(),
    };
    let tokens = Tokens {
        access: value["access_token"].as_str().unwrap_or_default().into(),
        refresh: value["refresh_token"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| old.map(|t| t.refresh.clone()))
            .unwrap_or_default(),
        id: value["id_token"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| old.map(|t| t.id.clone()))
            .unwrap_or_default(),
        nonce: nonce.into(),
        expires_at: now().saturating_add(value["expires_in"].as_u64().unwrap_or(0)),
        scopes,
    };
    if tokens.plan()
        && (tokens.access.is_empty()
            || tokens.refresh.is_empty()
            || tokens.expires_at <= now()
            || !value["token_type"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("bearer")))
    {
        return Err(error("token_response_invalid"));
    }
    Ok(tokens)
}
async fn json_response(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<Value, SiwcError> {
    let mut response = response.map_err(|_| error("network_failed"))?;
    let status = response.status().as_u16();
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("openai-request-id"))
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    while let Some(part) = response
        .chunk()
        .await
        .map_err(|_| error("network_failed"))?
    {
        if bytes.len() + part.len() > 2 * 1024 * 1024 {
            return Err(error("response_too_large"));
        }
        bytes.extend_from_slice(&part);
    }
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) if !(200..300).contains(&status) => Value::Null,
        Err(_) => return Err(error("response_invalid")),
    };
    if !(200..300).contains(&status) {
        let mut failure = response_error(status, &value, request_id.as_deref());
        if let Some(code) = value["error"]
            .as_str()
            .and_then(|s| safe_identifier(Some(s)))
        {
            failure.code = code;
        }
        return Err(failure);
    }
    Ok(value)
}
async fn wait_cancel(cancellation: &crate::provider_transport::ProviderCancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn callback(
    listener: &tokio::net::TcpListener,
    expected: &str,
) -> Result<(tokio::net::TcpStream, BTreeMap<String, String>), SiwcError> {
    loop {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|_| error("callback_unavailable"))?;
        let read = async {
            let mut bytes = Vec::new();
            loop {
                let mut part = [0u8; 1024];
                let count = socket
                    .read(&mut part)
                    .await
                    .map_err(|_| error("callback_invalid"))?;
                if count == 0 || bytes.len() + count > 16 * 1024 {
                    return Err(error("callback_invalid"));
                }
                bytes.extend_from_slice(&part[..count]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = std::str::from_utf8(&bytes).map_err(|_| error("callback_invalid"))?;
            let mut line = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace();
            if line.next() != Some("GET") {
                return Err(error("callback_invalid"));
            }
            let target = line.next().ok_or_else(|| error("callback_invalid"))?;
            let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
                .map_err(|_| error("callback_invalid"))?;
            if url.path() != "/auth/callback" {
                return Err(error("callback_invalid"));
            }
            let mut params = BTreeMap::new();
            for (key, value) in url.query_pairs() {
                if params
                    .insert(key.into_owned(), value.into_owned())
                    .is_some()
                {
                    return Err(error("callback_invalid"));
                }
            }
            if params.get("state").map(String::as_str) != Some(expected) {
                return Err(error("state_mismatch"));
            }
            Ok(params)
        };
        match tokio::time::timeout(Duration::from_secs(2), read).await {
            Ok(Ok(params)) => return Ok((socket, params)),
            _ => {
                let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;
            }
        }
    }
}

pub(crate) struct SiwcAccess {
    service: Arc<SiwcService>,
    registration: String,
    epoch: u64,
}
impl SiwcAccess {
    pub async fn token(&self) -> Result<SecretValue, SiwcError> {
        self.service.token(&self.registration, self.epoch).await
    }
    pub async fn check(&self) -> Result<(), SiwcError> {
        let data = self.service.load().await?.data;
        if data
            .registrations
            .get(&self.registration)
            .is_some_and(|r| r.epoch == self.epoch && r.tokens.is_some())
        {
            Ok(())
        } else {
            Err(error("session_changed"))
        }
    }
    pub async fn cancelled(&self) -> SiwcError {
        loop {
            if let Err(error) = self.check().await {
                return error;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    pub async fn record_error(&self, failure: SiwcError) -> Result<(), SiwcError> {
        let id = self.registration.clone();
        let epoch = self.epoch;
        self.service
            .change(move |data| {
                if let Some(r) = data.registrations.get_mut(&id).filter(|r| r.epoch == epoch) {
                    r.failure = Some(failure.clone());
                }
                Ok(())
            })
            .await
    }
    pub fn endpoint(&self) -> String {
        format!("{}/responses", self.service.endpoints.api)
    }
}
