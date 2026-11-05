//! HTTPS + WebSocket server for remote input (LAN phone recording).
//!
//! A phone on the same LAN opens `https://<PC-IP>:<port>` in a browser and gets a recording
//! page (index.html / app.js / style.css under assets/, embedded via compile-time include_str!).
//! The phone streams 16k/mono/16-bit LE PCM back to the PC in real time over WebSocket and
//! enters the same dictation pipeline through the external-audio seam of the shared
//! [`openless_core::OpenLessBackend`].
//!
//! Browser microphone access requires a secure context, so this server uses HTTPS.
//! A persistent per-installation CA signs a separate LAN server certificate.
//! Phones install and trust the CA once. TLS uses ring
//! (matching reqwest/tungstenite and avoiding an additional aws-lc-sys dependency).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

mod assets {
    pub const INDEX_HTML: &str = include_str!("../../../assets/remote-input/index.html");
    pub const APP_JS: &str = include_str!("../../../assets/remote-input/app.js");
    pub const STYLE_CSS: &str = include_str!("../../../assets/remote-input/style.css");
    pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/remote-input/icon.png");
    pub const MIC_PNG: &[u8] = include_bytes!("../../../assets/remote-input/mic.png");
    pub const DONE_PNG: &[u8] = include_bytes!("../../../assets/remote-input/done.png");
}

const HEADER_HTML: &str = "text/html; charset=utf-8";
const HEADER_JS: &str = "application/javascript; charset=utf-8";
const HEADER_CSS: &str = "text/css; charset=utf-8";

// ───────────────────────── Public types ─────────────────────────

pub struct RemoteServerConfig {
    pub port: u16,
    pub backend: Arc<openless_core::OpenLessBackend>,
    pub app: AppHandle,
}

/// Handle for the running server. drop / shutdown triggers graceful shutdown.
pub struct RemoteServerHandle {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Shutdown signal broadcast to all established WS connections. Stopping the accept loop
    /// alone is not enough: connection tasks are spawned independently, and without notifying
    /// them a paired phone session survives intact after the user turns off remote input (or
    /// resets the PIN, triggering a restart) — still able to record and type at the PC cursor,
    /// which breaks the revoke semantics.
    conn_shutdown_tx: tokio::sync::watch::Sender<bool>,
    join: tauri::async_runtime::JoinHandle<()>,
    pub bound_port: u16,
    pub ca_fingerprint_sha256: String,
}

impl RemoteServerHandle {
    /// Tell the accept loop and all existing WS connections to exit, and wait for the accept loop to finish.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.conn_shutdown_tx.send(true);
        let _ = self.join.await;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputStatus {
    pub running: bool,
    pub starting: bool,
    pub port: u16,
    pub pin: String,
    pub urls: Vec<String>,
    pub urls_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_fingerprint_sha256: Option<String>,
}

// ───────────────────────── Utility functions ─────────────────────────

/// Generate a 6-digit numeric pairing code. Uses rejection sampling to remove modulo bias
/// (u32::MAX is not a multiple of 1_000_000; plain modulo gives low-numbered PINs about 0.03%
/// extra probability).
pub fn generate_pin() -> String {
    // u32::MAX + 1 = 2^32; discard the high remainder range that would bias the result.
    // Bias cutoff: u32::MAX - (u32::MAX % 1_000_000) + 1 (rounded down to a multiple of 1_000_000)
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let b = uuid::Uuid::new_v4().into_bytes();
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if n < LIMIT {
            return format!("{:06}", n % 1_000_000);
        }
        // Extremely rare (about 2^32 mod 10^6 / 2^32 ≈ 0.007% chance); just resample.
    }
}

fn pin_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("remote-input-pin.txt"))
}

mod pin_persistence;

/// Read the persisted pairing code; generate and persist a new one if missing/invalid. Keeps the pairing code stable across restarts.
pub fn load_or_create_pin(app: &AppHandle) -> std::io::Result<String> {
    let path = pin_path(app).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "OpenLess config directory is unavailable",
        )
    })?;
    pin_persistence::load_or_create_pin_at_path(&path, generate_pin)
}

/// Write the pairing code to disk atomically; callers commit in-memory state only after success.
pub fn save_pin(app: &AppHandle, pin: &str) -> std::io::Result<()> {
    let path = pin_path(app).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "OpenLess config directory is unavailable",
        )
    })?;
    pin_persistence::persist_pin_atomically(&path, pin)
}

fn is_private_lan(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    !ip.is_loopback()
        && !ip.is_link_local()
        && ((o[0] == 192 && o[1] == 168)
            || o[0] == 10
            || (o[0] == 172 && (16..=31).contains(&o[1])))
}

/// All LAN IPv4 addresses of this machine (filtering loopback / link-local / non-private ranges from virtual adapters).
pub fn local_lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if is_private_lan(&v4) {
                    out.push(v4);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Access URLs shown to the frontend.
pub fn access_urls(port: u16) -> Vec<String> {
    local_lan_ipv4s()
        .iter()
        .map(|ip| format!("https://{ip}:{port}"))
        .collect()
}

// ───────────────────────── TLS ─────────────────────────

mod tls_identity;

// ───────────────────────── Startup ─────────────────────────

fn build_router(state: Arc<WsState>) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, HEADER_JS)],
                    assets::APP_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, HEADER_CSS)],
                    assets::STYLE_CSS,
                )
            }),
        )
        .route(
            "/icon.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::ICON_PNG,
                )
            }),
        )
        .route(
            "/mic.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::MIC_PNG,
                )
            }),
        )
        .route(
            "/done.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::DONE_PNG,
                )
            }),
        )
        // Certificate download: the phone opens it in a browser to download and install trust
        // (iOS Safari's wss does not reuse page-level certificate exceptions; wss is only
        // stable after full trust in the system settings).
        .route(
            "/cert.cer",
            get(|State(state): State<Arc<WsState>>| async move {
                (
                    [
                        (
                            axum::http::header::CONTENT_TYPE,
                            "application/x-x509-ca-cert",
                        ),
                        (axum::http::header::CACHE_CONTROL, "no-store"),
                    ],
                    state.cert_der.clone(),
                )
            }),
        )
        .route("/cert.mobileconfig", get(mobileconfig_handler))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

/// The index page injects language and the recording default mode from current PC preferences,
/// reading fresh values on every refresh. The H5 page keeps the mode explicitly saved on the
/// phone; PC defaults apply only on first visit or for invalid local values.
async fn index_handler(State(state): State<Arc<WsState>>) -> impl IntoResponse {
    let lang = state
        .backend
        .services()
        .remote_input
        .status()
        .map(|status| status.locale)
        .unwrap_or_else(|_| "zh-CN".to_string());
    // Preferences come from persisted data; never concatenate arbitrary strings into inline
    // script — only fixed literals are injected.
    let default_mode = if state.backend.get_preferences().remote_input_default_mode == "hold" {
        "hold"
    } else {
        "toggle"
    };
    Html(
        assets::INDEX_HTML
            .replace("%%OL_LANG%%", &lang)
            .replace("%%OL_DEFAULT_MODE%%", default_mode),
    )
}

// iOS still requires the user to install the profile and enable full trust.
async fn mobileconfig_handler(State(state): State<Arc<WsState>>) -> impl IntoResponse {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/x-apple-aspen-config",
            ),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        tls_identity::mobileconfig(&state.cert_der),
    )
}

pub async fn start(cfg: RemoteServerConfig) -> Result<RemoteServerHandle, String> {
    let _ = HEADER_HTML; // index uses axum Html(), which sets its own content-type
    let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for ip in local_lan_ipv4s() {
        sans.push(ip.to_string());
    }
    // Trust must survive restarts and upgrades; refuse ephemeral identities.
    let cert_dir = cfg
        .app
        .path()
        .app_config_dir()
        .map_err(|error| format!("remote TLS config directory: {error}"))?;
    let identity = tls_identity::load_or_create(&cert_dir, &sans)?;
    let ca_fingerprint_sha256 = identity.ca_fingerprint_sha256;
    let cert_der = identity.trust_cert;
    let acceptor = TlsAcceptor::from(identity.server_config);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            "port-in-use".to_string()
        } else {
            format!("bind: {e}")
        }
    })?;
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(cfg.port);

    let (conn_shutdown_tx, conn_shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(WsState {
        backend: cfg.backend,
        cert_der,
        conn_shutdown_rx,
    });
    let router = build_router(state);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let join = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    log::info!("[remote-input] accept loop shutting down");
                    break;
                }
                accepted = listener.accept() => {
                    let (tcp, peer) = match accepted {
                        Ok(x) => x,
                        Err(e) => {
                            log::warn!("[remote-input] accept error: {e}");
                            continue;
                        }
                    };
                    // Lowest-level diagnostics: log the source IP of every TCP connection
                    // reaching port 8443. As soon as a phone connects, this shows whether it
                    // actually reached this machine and from which subnet (debugging "did it
                    // connect to some other device").
                    log::info!("[remote-input] 收到 TCP 连接，来自 {peer}");
                    let acceptor = acceptor.clone();
                    // Attach the peer IP to the router as an Extension per connection, for PIN
                    // locking by IP.
                    let router = router.clone().layer(axum::Extension(PeerIp(peer.ip())));
                    tokio::spawn(async move {
                        let tls = match acceptor.accept(tcp).await {
                            Ok(t) => t,
                            Err(e) => {
                                log::warn!("[remote-input] 来自 {peer} 的 TLS 握手失败（证书没被接受）：{e}");
                                return;
                            }
                        };
                        let io = TokioIo::new(tls);
                        let svc = hyper_util::service::TowerToHyperService::new(router);
                        let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection_with_upgrades(io, svc)
                            .await;
                    });
                }
            }
        }
    });

    Ok(RemoteServerHandle {
        shutdown_tx: Some(shutdown_tx),
        conn_shutdown_tx,
        join,
        bound_port,
        ca_fingerprint_sha256,
    })
}

mod websocket;
use websocket::{ws_upgrade, PeerIp, WsState};
