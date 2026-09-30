//! Shared HTTP clients + retrying request sender.
//!
//! Background: each network command used to create its own `reqwest::Client::new()`,
//! with no connection-pool reuse — a successful TLS connection was discarded after
//! one use and the next command paid a fresh handshake. On networks with unstable
//! handshakes (proxy routing etc.) the first handshake was often reset, forcing
//! repeated user retries.
//!
//! Two things live here:
//! - `http()`: process-wide shared client. Connections from a successful handshake
//!   enter the pool and later commands reuse them without paying the handshake again.
//! - `send_with_retry`: exponential-backoff retry for **connection-layer failures**
//!   only (`is_connect()` — handshake reset / connection refused etc.). These happen
//!   before the request reaches the server and are usually transient, so retrying is
//!   both idempotent-safe and useful. **Timeouts and other request-layer errors are
//!   not retried**: a timeout may occur after the server already received the request
//!   (retrying POST / DELETE would repeat it); `is_request()` errors are mostly
//!   deterministic (e.g. a misconfigured endpoint) and retrying only adds seconds of
//!   delay. HTTP 4xx/5xx are likewise not retried — the server answered; the status
//!   code is the caller's to judge.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Whether the user allows the app to use the system proxy (issue #869). Default
/// true = follow the system proxy, matching historical behavior; when off, all
/// reqwest clients are built with `.no_proxy()` for direct connections.
/// Initialized from persisted settings at startup by the coordinator and kept in
/// sync on `set_settings` changes.
static USE_SYSTEM_PROXY: AtomicBool = AtomicBool::new(true);

/// Build cache for the shared / provider clients. key = `(discriminator, no_proxy
/// decision)`. Cleared entirely when the proxy toggle changes so "saved = effective".
static CACHE: Lazy<Mutex<HashMap<(u64, bool), reqwest::Client>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Whether the system proxy is in use (false = all requests go direct).
pub fn use_system_proxy() -> bool {
    USE_SYSTEM_PROXY.load(Ordering::Relaxed)
}

/// Updates the system-proxy toggle and clears the client cache so later requests
/// rebuild their connection pools under the new policy immediately. Called at startup
/// initialization and from `set_settings` on value changes.
pub fn set_use_system_proxy(enabled: bool) {
    USE_SYSTEM_PROXY.store(enabled, Ordering::Relaxed);
    CACHE.lock().clear();
}

/// Whether a base_url should bypass the system proxy: loopback always bypasses
/// (proxying localhost is pointless and can self-loop); with the global system-proxy
/// toggle off, everything bypasses (issue #869).
pub fn should_bypass_proxy(base_url: &str, use_system_proxy: bool) -> bool {
    !use_system_proxy || is_loopback_url(base_url)
}

fn is_loopback_url(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    // The url crate returns IPv6 hosts bracketed ("[::1]"); strip before parsing.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Base builder for shared clients: bounded handshake + connection pool + UA;
/// disables the system proxy on demand.
fn base_client_builder(no_proxy: bool) -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        // Separate handshake timeout: stuck handshakes should fail fast so
        // send_with_retry can retry immediately.
        .connect_timeout(Duration::from_secs(8))
        // Connection pool: a successfully handshaked connection stays 90s for later
        // commands to reuse.
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(concat!("OpenLess/", env!("CARGO_PKG_VERSION")));
    if no_proxy {
        builder = builder.no_proxy();
    }
    builder
}

/// Process-wide shared HTTP client. Connection-pooled — connections from a successful
/// handshake are reused by later requests; a proxy-toggle change clears CACHE and the
/// client rebuilds under the new policy automatically.
pub fn http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((0, no_proxy), || {
        base_client_builder(no_proxy)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// HTTP client for requests carrying OAuth device credentials or bearer tokens.
/// Redirects are disabled so secrets are never replayed to a different origin.
pub fn credential_http() -> reqwest::Client {
    credential_http_for_url("")
}

/// No-redirect client selected for the current request, not backend startup.
/// A loopback OAuth endpoint must remain direct even when other endpoints in
/// the same service are public. This client caches no credentials; bearer
/// tokens and device codes belong only to the individual request builder.
pub fn credential_http_for_url(base_url: &str) -> reqwest::Client {
    let no_proxy = should_bypass_proxy(base_url, use_system_proxy());
    cached_client((1, no_proxy), || {
        base_client_builder(no_proxy)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect credential HTTP client")
    })
}

/// Anonymous HTTP client for public endpoints that must fail closed on redirects.
pub fn anonymous_no_redirect_http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((2, no_proxy), || {
        base_client_builder(no_proxy)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build anonymous no-redirect HTTP client")
    })
}

/// Shared client for public model metadata and ranged downloads. Model hosts
/// legitimately redirect objects to a CDN, but the redirect chain stays
/// bounded and follows the same live proxy policy as every other Core client.
pub fn model_http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((3, no_proxy), || {
        base_client_builder(no_proxy)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("build model HTTP client")
    })
}

/// Caches and reuses `reqwest::Client` by `(timeout_secs, no_proxy)`.
///
/// LLM / ASR providers used to build a fresh `reqwest::Client` per request; a new
/// client's pool is empty, so every utterance paid a fresh TLS handshake
/// (~100-300ms). Built clients are now cached by their config: later providers with
/// the same config `clone()` the same pool (`reqwest::Client` is an `Arc` inside —
/// clone shares pool and config), so the handshake is paid once.
///
/// `build` runs only on the first miss and must produce a client consistent with the
/// `key` semantics.
pub fn cached_client<F>(key: (u64, bool), build: F) -> reqwest::Client
where
    F: FnOnce() -> reqwest::Client,
{
    CACHE.lock().entry(key).or_insert_with(build).clone()
}

/// Render a user-configured URL for logs without credentials or secret-bearing components.
pub fn sanitized_url_for_logs(raw_url: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw_url.trim()) else {
        return "<invalid-url>".to_string();
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.set_username("").is_err()
        || url.set_password(None).is_err()
    {
        return "<invalid-url>".to_string();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

/// Stable diagnostic category for a reqwest failure. Unlike `Display`, this never embeds its URL.
pub fn request_error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection"
    } else if error.is_body() || error.is_decode() {
        "response-body"
    } else {
        "request"
    }
}

/// Max attempts per request. Individual failures are fast (handshake reset ~0.5s), so
/// even 10 attempts stay within a controllable total.
const MAX_ATTEMPTS: u32 = 10;

/// Sends a request with exponential-backoff retries for connection-layer failures
/// only (`is_connect()`: handshake reset / connection refused etc.).
///
/// `make` rebuilds the `RequestBuilder` on each attempt (`send()` consumes it). Only
/// `is_connect()` is retried — the connection was never established, the request
/// never reached the server, and such failures are usually transient, so retrying is
/// idempotent-safe and worthwhile. Timeouts (the server may already be processing)
/// and other `is_request()` errors (mostly deterministic, e.g. a misconfigured
/// endpoint) are not retried. Any HTTP response (including 4xx/5xx) is returned
/// as-is; the caller judges the status code.
pub async fn send_with_retry<F>(make: F) -> reqwest::Result<reqwest::Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match make().send().await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                let retryable = err.is_connect();
                if !retryable || attempt >= MAX_ATTEMPTS {
                    return Err(err);
                }
                // Backoff: 150 / 300 / 600 / 900 / 900 … ms.
                let backoff = (150u64 * 2u64.pow((attempt - 1).min(3))).min(900);
                let failure = request_error_kind(&err);
                log::warn!(
                    "[net] transient {failure} failure (attempt {attempt}/{MAX_ATTEMPTS}), retry in {backoff}ms"
                );
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{credential_http, sanitized_url_for_logs};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn proxy_bypass_decision_is_pure() {
        use super::should_bypass_proxy;
        // Loopback addresses bypass regardless of the system-proxy toggle.
        for url in [
            "http://localhost:9000/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            assert!(
                should_bypass_proxy(url, true),
                "{url} should bypass when system proxy is on"
            );
            assert!(
                should_bypass_proxy(url, false),
                "{url} should bypass when system proxy is off"
            );
        }
        // Public hosts: follow the proxy when the system proxy is on, direct when off.
        assert!(!should_bypass_proxy("https://api.example.com/v1", true));
        assert!(should_bypass_proxy("https://api.example.com/v1", false));
        // Unparseable URLs: no bypass when the toggle is on, always bypass when the
        // global toggle is off.
        assert!(!should_bypass_proxy("not a url", true));
        assert!(should_bypass_proxy("not a url", false));
    }

    #[test]
    fn system_proxy_toggle_updates_flag_and_rebuilds_shared_client() {
        use super::{http, model_http, set_use_system_proxy, use_system_proxy, CACHE};
        set_use_system_proxy(true);
        CACHE.lock().clear();
        let _ = http();
        let _ = model_http();
        assert!(!CACHE.lock().is_empty());
        set_use_system_proxy(false);
        assert!(!use_system_proxy());
        // The next http() rebuilds under the "direct" decision (the key's bool is
        // no_proxy).
        let _ = http();
        let _ = model_http();
        assert!(CACHE.lock().contains_key(&(0, true)));
        assert!(CACHE.lock().contains_key(&(3, true)));
        set_use_system_proxy(true);
        assert!(use_system_proxy());
    }

    #[tokio::test]
    async fn credential_client_never_follows_redirects_or_forwards_bearer() {
        let redirect_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}", redirect_target.local_addr().unwrap());
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source_url = format!("http://{}", source.local_addr().unwrap());
        let source_task = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..read])
                .to_ascii_lowercase()
                .contains("authorization: bearer gho_redirect_test"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let response = credential_http()
            .get(source_url)
            .bearer_auth("gho_redirect_test")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), redirect_target.accept())
                .await
                .is_err()
        );
        source_task.await.unwrap();
    }

    #[test]
    fn log_url_removes_userinfo_query_and_fragment() {
        let rendered = sanitized_url_for_logs(
            "https://alice:password@example.com:8443/v1/models?token=secret#private",
        );
        assert_eq!(rendered, "https://example.com:8443/v1/models");
        for secret in ["alice", "password", "token", "secret", "private"] {
            assert!(!rendered.contains(secret), "log URL leaked {secret}");
        }
    }

    #[test]
    fn log_url_never_echoes_malformed_input() {
        assert_eq!(
            sanitized_url_for_logs("not a URL?token=secret#private"),
            "<invalid-url>"
        );
    }
}
