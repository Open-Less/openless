//! Shared validation and DNS pinning preparation for configurable HTTP endpoints.

use std::net::{IpAddr, SocketAddr};

use crate::{BackendError, BackendErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub host: String,
    pub addrs: Vec<SocketAddr>,
}

/// Validate endpoint syntax without restricting user-selected network ranges.
pub fn validate_http_endpoint(raw: &str) -> Result<(), BackendError> {
    let url = url::Url::parse(raw).map_err(|error| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("endpoint 不是合法 URL：{error}"),
        )
    })?;
    url.host_str().ok_or_else(|| {
        BackendError::new(BackendErrorCode::InvalidArgument, "endpoint 缺少主机名")
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "endpoint 必须使用 http 或 https",
        ));
    }
    Ok(())
}

/// Resolve a hostname once so the request adapter can pin the exact addresses
/// and avoid a second DNS decision between validation and connection.
pub async fn resolve_http_endpoint(raw: &str) -> Result<Option<ResolvedEndpoint>, BackendError> {
    validate_http_endpoint(raw)?;
    let url = url::Url::parse(raw).map_err(|error| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("endpoint 不是合法 URL：{error}"),
        )
    })?;
    let host = url.host_str().ok_or_else(|| {
        BackendError::new(BackendErrorCode::InvalidArgument, "endpoint 缺少主机名")
    })?;
    if host.parse::<IpAddr>().is_ok() {
        return Ok(None);
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| BackendError::new(BackendErrorCode::InvalidArgument, "endpoint 缺少端口"))?;
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| {
            BackendError::new(BackendErrorCode::Provider, "endpoint 主机名无法解析").retryable(true)
        })?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(
            BackendError::new(BackendErrorCode::Provider, "endpoint 主机名无法解析")
                .retryable(true),
        );
    }
    Ok(Some(ResolvedEndpoint {
        host: host.to_string(),
        addrs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_user_selected_http_and_https_networks() {
        for endpoint in [
            "http://example.com:12345/",
            "http://1.2.3.4/v1",
            "http://192.168.1.50:9000/v1",
            "http://localhost:9000/v1",
            "http://169.254.169.254/v1",
            "https://example.com:12345/",
        ] {
            validate_http_endpoint(endpoint).unwrap();
        }
    }

    #[test]
    fn rejects_malformed_or_non_http_urls_with_stable_code() {
        for endpoint in ["not a url", "ftp://example.com/", "wss://example.com/"] {
            let error = validate_http_endpoint(endpoint).unwrap_err();
            assert_eq!(error.code, BackendErrorCode::InvalidArgument);
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn literal_ip_needs_no_dns_pin() {
        assert_eq!(
            resolve_http_endpoint("https://127.0.0.1:8443/v1")
                .await
                .unwrap(),
            None
        );
    }
}
