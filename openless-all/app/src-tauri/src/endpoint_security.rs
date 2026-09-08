//! Tauri compatibility adapter for shared HTTP endpoint validation.

pub(crate) use openless_core::endpoint_security::ResolvedEndpoint;

pub(crate) fn validate_http_endpoint(raw: &str) -> anyhow::Result<()> {
    openless_core::endpoint_security::validate_http_endpoint(raw).map_err(anyhow::Error::new)
}

pub(crate) async fn resolve_http_endpoint(raw: &str) -> anyhow::Result<Option<ResolvedEndpoint>> {
    openless_core::endpoint_security::resolve_http_endpoint(raw)
        .await
        .map_err(anyhow::Error::new)
}
