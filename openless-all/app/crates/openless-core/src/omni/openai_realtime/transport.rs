//! Proxy-aware, no-redirect WebSocket upgrade with explicit header authentication.

use reqwest::header::{
    HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION,
    UPGRADE,
};
use tokio_tungstenite::tungstenite::{
    handshake::{client::generate_key, derive_accept_key},
    protocol::{Role, WebSocketConfig},
};
use tokio_tungstenite::WebSocketStream;

use super::super::OmniConfig;
use super::wire::Profile;
use crate::polish::LLMError;

pub(super) type Socket = WebSocketStream<reqwest::Upgraded>;

/// Normalize the shared HTTP API root for a Realtime session or model listing.
/// Retain gateway prefixes and routing queries, but never forward query credentials.
pub(crate) fn api_root(base: &str) -> Result<url::Url, LLMError> {
    let invalid = || LLMError::ParseError("Invalid Realtime endpoint".into());
    let raw = base.trim();
    if raw.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#', '\\'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    }) {
        return Err(invalid());
    }
    let mut url = url::Url::parse(if base.trim().is_empty() {
        "https://api.openai.com/v1"
    } else {
        base.trim()
    })
    .map_err(|_| invalid())?;
    let scheme = match url.scheme() {
        "https" | "wss" => "https",
        "http" | "ws" => "http",
        _ => return Err(invalid()),
    };
    url.set_scheme(scheme).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    url.set_fragment(None);
    crate::endpoint_security::validate_http_endpoint(url.as_str()).map_err(|_| invalid())?;
    let path = url.path().trim_end_matches('/');
    let root = if let Some(root) = path.strip_suffix("/realtime") {
        root.to_owned()
    } else {
        let root = path.strip_suffix("/chat/completions").unwrap_or(path);
        let root = if root.is_empty() { "/v1" } else { root };
        root.to_owned()
    };
    url.set_path(&root);
    let query = url
        .query_pairs()
        .filter(|(name, _)| !credential_or_model_query(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

/// reqwest performs the HTTP(S) upgrade, so WS(S) is normalized by api_root first.
pub(crate) fn endpoint(base: &str, model: &str) -> Result<url::Url, LLMError> {
    let mut url = api_root(base)?;
    let explicit = url::Url::parse(base.trim()).ok().filter(|source| {
        matches!(source.scheme(), "ws" | "wss") && source.path().trim_matches('/').len() > 0
    });
    if let Some(source) = explicit {
        url.set_path(source.path().trim_end_matches('/'));
    } else {
        let root = url.path().trim_end_matches('/');
        url.set_path(&format!("{root}/realtime"));
    }
    url.query_pairs_mut().append_pair("model", model.trim());
    Ok(url)
}

pub(crate) fn configured_endpoint(config: &OmniConfig) -> Result<url::Url, LLMError> {
    Profile::from_provider(&config.provider_id)?.endpoint(config)
}

pub(super) async fn connect(config: &OmniConfig) -> Result<Socket, LLMError> {
    let headers = config
        .realtime_options
        .authenticated_headers(&config.api_key, &config.extra_headers)?;
    let url = configured_endpoint(config)?;
    let nonce = generate_key();
    let mut request = crate::net::credential_http_for_url(url.as_str())
        .get(url)
        .version(reqwest::Version::HTTP_11);
    for (name, value) in &headers {
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| LLMError::ParseError("Invalid Realtime header value".into()))?;
        value.set_sensitive(true);
        request = request.header(name.as_str(), value);
    }
    let response = request
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, &nonce)
        .send()
        .await
        .map_err(|_| LLMError::Network("Realtime connection failed".into()))?;
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        return Err(LLMError::InvalidResponse {
            status: response.status().as_u16(),
            body: "Realtime WebSocket handshake failed; check endpoint, API key and model access"
                .into(),
        });
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
                .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
        });
    if !upgraded || !accepted || !connection {
        return Err(LLMError::Network(
            "Invalid Realtime WebSocket upgrade".into(),
        ));
    }
    let stream = response
        .upgrade()
        .await
        .map_err(|_| LLMError::Network("Realtime upgrade failed".into()))?;
    let limits = WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    Ok(WebSocketStream::from_raw_socket(stream, Role::Client, Some(limits)).await)
}

/// Query names are decoded by `query_pairs` before comparison. Normalize common
/// header/camelCase/snake_case spellings so they cannot leak a pasted API key.
pub(super) fn credential_or_model_query(name: &str) -> bool {
    let canonical = name
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect::<String>();
    matches!(
        canonical.as_str(),
        "model"
            | "key"
            | "apikey"
            | "xapikey"
            | "xdashscopeapikey"
            | "accesskey"
            | "accesskeyid"
            | "secretaccesskey"
            | "xapiaccesskey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "authtoken"
            | "securitytoken"
            | "authorization"
            | "auth"
            | "bearer"
            | "secret"
            | "clientsecret"
            | "clientkey"
            | "privatekey"
            | "password"
            | "passwd"
            | "signature"
            | "sig"
            | "credential"
            | "credentials"
            | "xamzcredential"
            | "xamzsignature"
            | "xamzsecuritytoken"
    )
}
