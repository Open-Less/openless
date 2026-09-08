//! Injectable transport used by the provider management service.
//!
//! Provider selection and response parsing belong to Core, but the concrete
//! HTTP client is a replaceable boundary.  Keeping that boundary explicit
//! makes timeout, cancellation, redirect and response-size behaviour testable
//! without contacting a real provider or putting secrets in test output.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;

/// Request issued by [`crate::ProviderService`] for a model-list endpoint.
///
/// Header values can contain credentials.  The custom `Debug` implementation
/// therefore reports names only and never renders values.
#[derive(Clone)]
pub struct ProviderTransportRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_response_bytes: usize,
}

impl fmt::Debug for ProviderTransportRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names = self
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        formatter
            .debug_struct("ProviderTransportRequest")
            .field("url", &crate::net::sanitized_url_for_logs(&self.url))
            .field("header_names", &header_names)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

/// Bounded response returned by a [`ProviderTransport`].
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderTransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for ProviderTransportResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderTransportResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Transport failures which are safe for Core to classify without exposing a
/// URL, request body or credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTransportError {
    Timeout,
    Connection,
    Cancelled,
    Request,
    ResponseTooLarge,
}

/// Cooperative cancellation token for a model-list request.
///
/// The token is intentionally tiny and runtime-neutral.  A transport checks
/// it before dispatch and between response chunks; the bounded request
/// timeout guarantees that a request currently waiting in the network cannot
/// remain unobserved indefinitely.
#[derive(Clone, Default)]
pub struct ProviderCancellation(Arc<AtomicBool>);

impl ProviderCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl fmt::Debug for ProviderCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Injectable model-list transport.  Hosts may provide a deterministic fake;
/// production uses [`ReqwestProviderTransport`].
pub trait ProviderTransport: Send + Sync {
    fn execute(
        &self,
        request: ProviderTransportRequest,
        cancellation: ProviderCancellation,
    ) -> BoxFuture<'static, Result<ProviderTransportResponse, ProviderTransportError>>;
}

/// Production transport for provider management requests.
#[derive(Debug, Clone, Default)]
pub struct ReqwestProviderTransport;

impl ReqwestProviderTransport {
    pub fn new() -> Self {
        Self
    }
}

impl ProviderTransport for ReqwestProviderTransport {
    fn execute(
        &self,
        request: ProviderTransportRequest,
        cancellation: ProviderCancellation,
    ) -> BoxFuture<'static, Result<ProviderTransportResponse, ProviderTransportError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(ProviderTransportError::Cancelled);
            }
            let client = crate::net::credential_http();
            let mut builder = client.get(&request.url).timeout(request.timeout);
            for (name, value) in request.headers {
                builder = builder.header(name, value);
            }
            let response = builder.send().await.map_err(map_reqwest_error)?;
            let status = response.status().as_u16();
            if response
                .content_length()
                .is_some_and(|length| length as usize > request.max_response_bytes)
            {
                return Err(ProviderTransportError::ResponseTooLarge);
            }

            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
                if cancellation.is_cancelled() {
                    return Err(ProviderTransportError::Cancelled);
                }
                let chunk = chunk.map_err(map_reqwest_error)?;
                if body.len().saturating_add(chunk.len()) > request.max_response_bytes {
                    return Err(ProviderTransportError::ResponseTooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(ProviderTransportResponse { status, body })
        })
    }
}

fn map_reqwest_error(error: reqwest::Error) -> ProviderTransportError {
    if error.is_timeout() {
        ProviderTransportError::Timeout
    } else if error.is_connect() {
        ProviderTransportError::Connection
    } else {
        ProviderTransportError::Request
    }
}
