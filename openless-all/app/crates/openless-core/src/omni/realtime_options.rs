//! Channel-scoped Realtime configuration, separate from transport headers.

use std::collections::HashMap;

use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::polish::LLMError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RealtimeAuthMode {
    #[default]
    Bearer,
    ApiKey,
    Headers,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RealtimeReasoningEffort {
    #[default]
    Auto,
    Omit,
    /// Accepted by targeted API trials, but not in the public Realtime enum.
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

impl RealtimeReasoningEffort {
    pub(crate) fn explicit_value(self) -> Option<&'static str> {
        match self {
            Self::Auto | Self::Omit => None,
            Self::None => Some("none"),
            Self::Minimal => Some("minimal"),
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::Xhigh => Some("xhigh"),
        }
    }
}

fn fallback_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RealtimeOptions {
    #[serde(default)]
    pub auth_mode: RealtimeAuthMode,
    #[serde(default)]
    pub reasoning_effort: RealtimeReasoningEffort,
    #[serde(default = "fallback_default")]
    pub reasoning_fallback: bool,
}

impl Default for RealtimeOptions {
    fn default() -> Self {
        Self {
            auth_mode: RealtimeAuthMode::Bearer,
            reasoning_effort: RealtimeReasoningEffort::Auto,
            reasoning_fallback: true,
        }
    }
}

impl RealtimeOptions {
    pub fn parse(value: &str) -> Result<Self, LLMError> {
        if value.len() > 8_192 {
            return Err(invalid_options());
        }
        if value.trim().is_empty() {
            return Ok(Self::default());
        }
        // Do not echo invalid JSON or unknown values: configuration may contain a
        // mistakenly pasted credential, and errors reach the application UI.
        serde_json::from_str(value).map_err(|_| invalid_options())
    }

    pub(crate) fn authenticated_headers(
        &self,
        key: &str,
        extra: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>, LLMError> {
        let mut headers = validate_headers(extra.clone())?;
        if self.auth_mode != RealtimeAuthMode::Headers {
            headers.remove("authorization");
            headers.remove("api-key");
            if key.trim().is_empty() {
                return Err(LLMError::MissingCredentials);
            }
            let (name, value) = match self.auth_mode {
                RealtimeAuthMode::Bearer => ("authorization", format!("Bearer {}", key.trim())),
                RealtimeAuthMode::ApiKey => ("api-key", key.trim().to_owned()),
                RealtimeAuthMode::Headers => unreachable!(),
            };
            HeaderValue::from_str(&value).map_err(|_| invalid_headers())?;
            headers.insert(name.to_owned(), value);
        }
        Ok(headers)
    }
}

/// Explicit Realtime profiles may use custom header authentication. Existing
/// HTTP/LLM header policy is not relaxed by this parser.
pub fn parse_headers(value: &str) -> Result<HashMap<String, String>, LLMError> {
    if value.len() > 65_536 {
        return Err(invalid_headers());
    }
    if value.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let headers: HashMap<String, String> =
        serde_json::from_str(value).map_err(|_| invalid_headers())?;
    validate_headers(headers)
}

fn validate_headers(headers: HashMap<String, String>) -> Result<HashMap<String, String>, LLMError> {
    let mut output = HashMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| invalid_headers())?;
        let name = name.as_str();
        if name.starts_with("sec-websocket-")
            || matches!(
                name,
                "host"
                    | "connection"
                    | "upgrade"
                    | "content-length"
                    | "transfer-encoding"
                    | "proxy-authorization"
            )
        {
            return Err(invalid_headers());
        }
        HeaderValue::from_str(&value).map_err(|_| invalid_headers())?;
        if value.contains(['\r', '\n']) || output.insert(name.to_owned(), value).is_some() {
            return Err(invalid_headers());
        }
    }
    Ok(output)
}

fn invalid_options() -> LLMError {
    LLMError::ParseError(
        "Invalid Realtime options; check authentication and reasoning settings".into(),
    )
}

fn invalid_headers() -> LLMError {
    LLMError::ParseError(
        "Invalid Realtime extra headers; handshake headers cannot be overridden".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_typed_bounded_and_backward_compatible() {
        assert_eq!(
            RealtimeOptions::parse("").unwrap(),
            RealtimeOptions::default()
        );
        assert_eq!(
            RealtimeOptions::parse("{}").unwrap(),
            RealtimeOptions::default()
        );
        let options = RealtimeOptions::parse(
            r#"{"authMode":"api-key","reasoningEffort":"none","reasoningFallback":false}"#,
        )
        .unwrap();
        assert_eq!(options.auth_mode, RealtimeAuthMode::ApiKey);
        assert_eq!(options.reasoning_effort, RealtimeReasoningEffort::None);
        assert!(!options.reasoning_fallback);
        for input in [
            r#"{"reasoningEffort":"synthetic-secret"}"#,
            r#"{"reasoningFallback":"true"}"#,
            r#"{"unknown":"synthetic-secret"}"#,
        ] {
            let error = RealtimeOptions::parse(input).unwrap_err().to_string();
            assert!(!error.contains("synthetic-secret"));
        }
        assert!(RealtimeOptions::parse(&" ".repeat(8_193)).is_err());
        assert!(RealtimeOptions::parse(&format!("{{{}}}", " ".repeat(8_193))).is_err());
    }

    #[test]
    fn auth_modes_are_explicit_and_protected_headers_cannot_override_them() {
        let extra = HashMap::from([
            ("Authorization".into(), "Basic synthetic".into()),
            ("api-key".into(), "other-key".into()),
            ("X-Tenant".into(), "tenant".into()),
        ]);
        let mut options = RealtimeOptions::default();
        let headers = options.authenticated_headers("main-key", &extra).unwrap();
        assert_eq!(headers["authorization"], "Bearer main-key");
        assert!(!headers.contains_key("api-key"));
        assert_eq!(headers["x-tenant"], "tenant");
        options.auth_mode = RealtimeAuthMode::ApiKey;
        let headers = options.authenticated_headers("main-key", &extra).unwrap();
        assert_eq!(headers["api-key"], "main-key");
        assert!(!headers.contains_key("authorization"));
        options.auth_mode = RealtimeAuthMode::Headers;
        let headers = options.authenticated_headers("", &extra).unwrap();
        assert_eq!(headers["authorization"], "Basic synthetic");
        assert_eq!(headers["api-key"], "other-key");
        assert!(RealtimeOptions::default()
            .authenticated_headers("", &extra)
            .is_err());
    }

    #[test]
    fn header_validation_blocks_framing_and_does_not_echo_secrets() {
        for header in [
            "Host",
            "Upgrade",
            "Sec-WebSocket-Key",
            "Connection",
            "Content-Length",
        ] {
            let value = serde_json::json!({header: "synthetic-secret"}).to_string();
            assert!(!parse_headers(&value)
                .unwrap_err()
                .to_string()
                .contains("synthetic-secret"));
        }
        assert!(
            parse_headers(r#"{"Authorization":"Bearer a","authorization":"Bearer b"}"#).is_err()
        );
        assert!(parse_headers(r#"{"X-Key":"a\r\nb"}"#).is_err());
        assert!(parse_headers(r#"{"X-Key":10}"#).is_err());
        assert_eq!(
            parse_headers(r#"{"X-Key":"value"}"#).unwrap()["x-key"],
            "value"
        );
    }
}
