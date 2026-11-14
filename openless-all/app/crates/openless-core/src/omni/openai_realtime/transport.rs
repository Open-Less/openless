//! Proxy-aware, no-redirect WebSocket upgrade with bearer credentials in headers only.

use reqwest::header::{
    HeaderValue, AUTHORIZATION, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY,
    SEC_WEBSOCKET_VERSION, UPGRADE,
};
use tokio_tungstenite::tungstenite::{
    handshake::{client::generate_key, derive_accept_key},
    protocol::{Role, WebSocketConfig},
};
use tokio_tungstenite::WebSocketStream;

use super::super::OmniConfig;
use crate::polish::LLMError;

pub(super) type Socket = WebSocketStream<reqwest::Upgraded>;

/// Accept an API root or an explicit Realtime URL, retaining compatible gateway prefixes.
/// reqwest performs the HTTP(S) upgrade, so WS(S) is normalized before security validation.
pub(crate) fn endpoint(base: &str, model: &str) -> Result<url::Url, LLMError> {
    let invalid = || LLMError::ParseError("Invalid OpenAI Realtime endpoint".into());
    let mut url = url::Url::parse(if base.trim().is_empty() { "https://api.openai.com/v1" } else { base.trim() })
        .map_err(|_| invalid())?;
    let scheme = match url.scheme() {
        "https" | "wss" => "https",
        "http" | "ws" => "http",
        _ => return Err(invalid()),
    };
    url.set_scheme(scheme).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() { return Err(invalid()); }
    url.set_fragment(None);
    crate::endpoint_security::validate_http_endpoint(url.as_str()).map_err(|_| invalid())?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/realtime") {
        let root = path.strip_suffix("/chat/completions").unwrap_or(path);
        let root = if root.is_empty() { "/v1" } else { root };
        url.set_path(&format!("{root}/realtime"));
    }
    let query = url.query_pairs().filter(|(name, _)| {
        !matches!(name.to_ascii_lowercase().as_str(),
            "model" | "key" | "api_key" | "api-key" | "access_token" | "authorization")
    }).map(|(name, value)| (name.into_owned(), value.into_owned())).collect::<Vec<_>>();
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(query).append_pair("model", model.trim());
    Ok(url)
}

pub(super) async fn connect(config: &OmniConfig) -> Result<Socket, LLMError> {
    if config.api_key.trim().is_empty() { return Err(LLMError::MissingCredentials); }
    let url = endpoint(&config.base_url, &config.model)?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {}", config.api_key.trim()))
        .map_err(|_| LLMError::ParseError("Invalid OpenAI Realtime API key".into()))?;
    auth.set_sensitive(true);
    let nonce = generate_key();
    let mut request = crate::net::credential_http_for_url(url.as_str())
        .get(url).version(reqwest::Version::HTTP_11);
    for (name, value) in &config.extra_headers {
        if !matches!(name.to_ascii_lowercase().as_str(),
            "authorization" | "connection" | "upgrade" | "host" | "content-length"
            | "sec-websocket-key" | "sec-websocket-version" | "sec-websocket-protocol"
            | "sec-websocket-extensions") {
            request = request.header(name, value);
        }
    }
    let response = request.header(AUTHORIZATION, auth)
        .header(CONNECTION, "Upgrade").header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13").header(SEC_WEBSOCKET_KEY, &nonce)
        .send().await.map_err(|_| LLMError::Network("OpenAI Realtime connection failed".into()))?;
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        return Err(LLMError::InvalidResponse {
            status: response.status().as_u16(),
            body: "OpenAI Realtime WebSocket handshake failed; check endpoint, API key and model access".into(),
        });
    }
    let expected = derive_accept_key(nonce.as_bytes());
    let headers = response.headers();
    let upgraded = headers.get(UPGRADE).and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let accepted = headers.get(SEC_WEBSOCKET_ACCEPT).and_then(|value| value.to_str().ok()) == Some(expected.as_str());
    let connection = headers.get(CONNECTION).and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|part| part.trim().eq_ignore_ascii_case("upgrade")));
    if !upgraded || !accepted || !connection {
        return Err(LLMError::Network("Invalid OpenAI Realtime WebSocket upgrade".into()));
    }
    let stream = response.upgrade().await.map_err(|_| LLMError::Network("OpenAI Realtime upgrade failed".into()))?;
    let limits = WebSocketConfig::default().max_message_size(Some(4 * 1024 * 1024)).max_frame_size(Some(4 * 1024 * 1024));
    Ok(WebSocketStream::from_raw_socket(stream, Role::Client, Some(limits)).await)
}
