//! WebSocket upgrade using the shared proxy-aware credential HTTP client.

use anyhow::{bail, Result};
use reqwest::header::{
    HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION,
    UPGRADE,
};
use tokio_tungstenite::tungstenite::{
    handshake::{client::generate_key, derive_accept_key},
    protocol::{Role, WebSocketConfig},
};
use tokio_tungstenite::WebSocketStream;

pub(super) type Socket = WebSocketStream<reqwest::Upgraded>;
const LIVE_PATH: &str =
    "ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// Accept the existing Gemini API root or an explicit Live URL, preserving gateway prefixes.
pub(crate) fn endpoint(base: &str) -> Result<url::Url> {
    let base = if base.trim().is_empty() {
        crate::asr::gemini::DEFAULT_ENDPOINT
    } else {
        base.trim()
    };
    let mut url =
        url::Url::parse(base).map_err(|_| anyhow::anyhow!("invalid Gemini Live endpoint"))?;
    let scheme = match url.scheme() {
        "https" | "wss" => "https",
        "http" | "ws" => "http",
        _ => bail!("Gemini Live endpoint must use HTTP(S) or WS(S)"),
    };
    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("invalid Gemini Live endpoint"))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Gemini Live endpoint must not contain credentials");
    }
    url.set_fragment(None);
    crate::endpoint_security::validate_http_endpoint(url.as_str())
        .map_err(|_| anyhow::anyhow!("invalid Gemini Live endpoint"))?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with(".BidiGenerateContent") {
        let root = path.strip_suffix("/interactions").unwrap_or(path);
        let root = root
            .strip_suffix("/v1beta")
            .or_else(|| root.strip_suffix("/v1alpha"))
            .unwrap_or(root);
        url.set_path(&format!("{root}/{LIVE_PATH}"));
    }
    let query = url
        .query_pairs()
        .filter(|(name, _)| name != "key" && name != "access_token")
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

/// Preserve proxy settings, loopback bypass, TLS, and the no-redirect credential policy.
pub(super) async fn connect(config: &super::GeminiLiveConfig) -> Result<Socket> {
    let url = endpoint(&config.base_url)?;
    let mut auth = HeaderValue::from_str(config.api_key.trim())
        .map_err(|_| anyhow::anyhow!("invalid Gemini API key"))?;
    auth.set_sensitive(true);
    let nonce = generate_key();
    let mut request = crate::net::credential_http_for_url(url.as_str())
        .get(url)
        .version(reqwest::Version::HTTP_11);
    for (name, value) in &config.extra_headers {
        if !matches!(
            name.to_ascii_lowercase().as_str(),
            "x-goog-api-key"
                | "connection"
                | "upgrade"
                | "host"
                | "sec-websocket-key"
                | "sec-websocket-version"
                | "sec-websocket-protocol"
                | "sec-websocket-extensions"
        ) {
            request = request.header(name, value);
        }
    }
    let response = request
        .header("x-goog-api-key", auth)
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, &nonce)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Gemini Live connection failed"))?;
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        bail!(
            "Gemini Live handshake failed (HTTP {})",
            response.status().as_u16()
        );
    }
    let expected = derive_accept_key(nonce.as_bytes());
    let headers = response.headers();
    let upgraded = headers
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let accepted = headers
        .get(SEC_WEBSOCKET_ACCEPT)
        .and_then(|value| value.to_str().ok())
        == Some(expected.as_str());
    let connection = headers
        .get(CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
    if !upgraded || !accepted || !connection {
        bail!("invalid Gemini Live WebSocket handshake");
    }
    let stream = response
        .upgrade()
        .await
        .map_err(|_| anyhow::anyhow!("Gemini Live upgrade failed"))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    Ok(WebSocketStream::from_raw_socket(stream, Role::Client, Some(config)).await)
}
