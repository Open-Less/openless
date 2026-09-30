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
use std::time::{Duration, Instant};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
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

/// Server keepalive: send a WS Ping every KEEPALIVE_PING_SECS (browsers answer Pong
/// automatically); if no upstream frame (including Pong) arrives for IDLE_TIMEOUT_SECS, treat
/// the link as a half-open dead connection and disconnect. Phones with the screen off or after
/// Wi-Fi drift often never send a TCP FIN, and without liveness probes recv() hangs forever:
/// the connection task, event subscription, and any in-flight remote session all hang.
const KEEPALIVE_PING_SECS: u64 = 30;
const IDLE_TIMEOUT_SECS: u64 = 90;
const AUDIO_IDLE_TIMEOUT_SECS: u64 = 15;

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

struct WsState {
    backend: Arc<openless_core::OpenLessBackend>,
    /// Public CA certificate offered for phone installation; never the server leaf or key.
    cert_der: Vec<u8>,
    /// Receiver of the server-shutdown broadcast; each WS connection clones one and watches it
    /// in its main loop's select.
    conn_shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

/// Peer IP injected by the accept loop (axum Extension). hyper talking to the TLS stream
/// directly cannot get ConnectInfo, so an Extension carrying the peer is attached to each
/// connection's service and read by the handler.
#[derive(Clone, Copy)]
struct PeerIp(IpAddr);

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

// ───────────────────────── WebSocket ─────────────────────────

async fn ws_upgrade(
    State(state): State<Arc<WsState>>,
    axum::Extension(PeerIp(peer_ip)): axum::Extension<PeerIp>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Getting here means the wss TLS handshake succeeded (the certificate was accepted by the
    // phone). When debugging "cannot connect", look for this line: absent = stuck at
    // TLS/certificate (handshake failed); present = handshake OK, the problem is in
    // authentication or later logic.
    log::info!("[remote-input] WS 已升级：手机已通过 wss 接入（TLS/证书 OK）");
    ws.on_upgrade(move |socket| handle_ws(socket, state, peer_ip))
}

fn send_json<T: Serialize>(value: &T) -> Message {
    Message::Text(serde_json::to_string(value).unwrap_or_else(|_| "{}".into()))
}

/// Phones only receive the Core session opened by their own connection. Global capsule /
/// plain-text broadcasts have no owner and would send local PC dictation or another phone's
/// content to all paired connections; they must not be a network egress.
fn backend_event_to_phone(
    event: &openless_core::BackendEvent,
    remote_session_id: &mut Option<openless_core::SessionId>,
) -> Vec<String> {
    use openless_core::{BackendEventKind, DictationPhase};
    if remote_session_id.is_none() || event.session_id != *remote_session_id {
        return Vec::new();
    }
    match &event.kind {
        BackendEventKind::DictationCompleted(result) => {
            // Completed state is published before the result, so the downstream owner can only
            // be released after the result arrives. The stop future's completion does not clear
            // the owner either, so select ordering cannot lose the final result.
            *remote_session_id = None;
            vec![
                serde_json::json!({"type":"status", "kind":"done",
                    "insertedChars":result.polished_text.chars().count(), "message":null})
                .to_string(),
                serde_json::json!({"type":"result", "text":result.polished_text}).to_string(),
            ]
        }
        BackendEventKind::DictationStateChanged(snapshot) => {
            let kind = match snapshot.phase {
                DictationPhase::Starting | DictationPhase::Recording => "recording",
                DictationPhase::Transcribing => "transcribing",
                DictationPhase::Polishing | DictationPhase::Inserting => "polishing",
                DictationPhase::Failed => "error",
                DictationPhase::Cancelled => "done",
                DictationPhase::Idle | DictationPhase::Completed => return Vec::new(),
            };
            if matches!(
                snapshot.phase,
                DictationPhase::Failed | DictationPhase::Cancelled
            ) {
                *remote_session_id = None;
            }
            let mut messages = vec![serde_json::json!({
                "type":"status", "kind":kind, "insertedChars":null,
                "message":snapshot.message,
            })
            .to_string()];
            if snapshot.phase == DictationPhase::Recording {
                messages
                    .push(serde_json::json!({"type":"level", "value":snapshot.level}).to_string());
            }
            messages
        }
        _ => Vec::new(),
    }
}

// stop spans ASR/polish/insertion and can run for tens of seconds. The socket select holds and
// polls the future, still receiving cancel/Close/shutdown meanwhile; an unexpected disconnect
// continues finalization, and only permission revocation cancels it.
type PendingRemoteStop =
    futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>>;

async fn handle_ws(mut socket: WebSocket, state: Arc<WsState>, peer_ip: IpAddr) {
    // 1) Handshake: wait for the first hello + PIN frame.
    let connection_id = openless_core::SessionId::new();
    let authed = match tokio::time::timeout(Duration::from_secs(15), socket.recv()).await {
        Ok(Some(Ok(Message::Text(txt)))) => {
            let candidate = parse_hello_pin(&txt);
            match state
                .backend
                .services()
                .remote_input
                .authenticate(connection_id, peer_ip.to_string(), candidate)
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    log::warn!("[remote-input] authentication failed: {error}");
                    return;
                }
            }
        }
        _ => return, // timeout / non-text first frame / disconnected
    };
    match authed {
        openless_core::RemoteAuthResult::Ok => {
            log::info!("[remote-input] 配对成功，进入录音会话");
            let _ = socket
                .send(send_json(&serde_json::json!({"type":"auth","ok":true})))
                .await;
        }
        openless_core::RemoteAuthResult::BadPin => {
            log::warn!("[remote-input] 配对码错误，已拒绝");
            let _ = socket
                .send(send_json(
                    &serde_json::json!({"type":"auth","ok":false,"reason":"bad-pin"}),
                ))
                .await;
            return;
        }
        openless_core::RemoteAuthResult::Locked => {
            log::warn!("[remote-input] 配对已锁定（连续错误过多），已拒绝");
            let _ = socket
                .send(send_json(
                    &serde_json::json!({"type":"auth","ok":false,"reason":"locked"}),
                ))
                .await;
            return;
        }
    }

    // Subscribe before the start control frame so even a fast session's initial events are
    // forwarded once the owner is established.
    let mut events = state.backend.subscribe();
    let mut last_event_sequence = 0;

    // 3) Main loop: phone upstream (control / PCM) + backend status downstream + keepalive
    // probes + shutdown broadcast.
    let mut conn_shutdown_rx = state.conn_shutdown_rx.clone();
    let mut keepalive = tokio::time::interval(Duration::from_secs(KEEPALIVE_PING_SECS));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_rx = Instant::now();
    let mut remote_session_id = None;
    let mut pending_stop: Option<PendingRemoteStop> = None;
    let mut preserve_audio = true;
    let mut receiving_audio = false;
    let mut last_audio = Instant::now();
    let mut audio_watchdog = tokio::time::interval(Duration::from_secs(2));
    audio_watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    'connection: loop {
        tokio::select! {
            incoming = socket.recv() => {
                last_rx = Instant::now();
                match incoming {
                    Some(Ok(Message::Binary(pcm))) => {
                        if !receiving_audio { continue; }
                        match parse_audio_frame(&pcm) {
                            Ok((session_id, sequence, pcm)) => {
                                if let Err(error) = state
                                    .backend
                                    .services()
                                    .remote_input
                                    .feed_pcm(connection_id, session_id, sequence, pcm)
                                    .await
                                {
                                    log::warn!("[remote-input] PCM frame rejected: {error}");
                                } else {
                                    last_audio = Instant::now();
                                }
                            }
                            Err(error) => {
                                log::warn!("[remote-input] invalid binary frame: {error}")
                            }
                        }
                    }
                    Some(Ok(Message::Text(txt))) => {
                        let previous_session = remote_session_id;
                        if !handle_control(
                            &txt,
                            &state,
                            connection_id,
                            &mut socket,
                            &mut remote_session_id,
                            &mut pending_stop,
                        )
                        .await
                        {
                            break;
                        }
                        if remote_session_id != previous_session {
                            last_audio = Instant::now();
                            receiving_audio = remote_session_id.is_some();
                        }
                        if pending_stop.is_some() { receiving_audio = false; }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            received = events.recv() => {
                let received = match received {
                    Ok(event) => vec![event],
                    Err(openless_core::EventRecvError::Lagged(_)) => {
                        state.backend.replay_events_after(last_event_sequence).events
                    }
                    Err(openless_core::EventRecvError::Closed) => { preserve_audio = false; break; },
                    Err(openless_core::EventRecvError::Empty) => continue,
                };
                for event in received {
                    if event.sequence <= last_event_sequence {
                        continue;
                    }
                    last_event_sequence = event.sequence;
                    for msg in backend_event_to_phone(&event, &mut remote_session_id) {
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break 'connection;
                        }
                    }
                }
            }
            stopped = async { pending_stop.as_mut().expect("guarded stop future").await }, if pending_stop.is_some() => {
                pending_stop = None;
                if let Err(error) = stopped {
                    log::warn!("[remote-input] stop stream failed: {error}");
                }
            }
            _ = keepalive.tick() => {
                // Half-open liveness: browsers answer Ping with Pong automatically (recv above
                // refreshes last_rx when it arrives). No upstream at all past the timeout ->
                // dead link; break to the unified finalization below (revoking the Core lease),
                // avoiding a dangling remote session and flags when a phone drops mid-recording.
                if last_rx.elapsed() > Duration::from_secs(IDLE_TIMEOUT_SECS) {
                    log::info!("[remote-input] 连接 {}s 无上行（含 Pong），按半开死链断开", IDLE_TIMEOUT_SECS);
                    break;
                }
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
            _ = audio_watchdog.tick(), if receiving_audio && pending_stop.is_none() && remote_session_id.is_some() => {
                // A browser may pause only the microphone while still answering Pongs, so TCP
                // liveness alone is not enough.
                if last_audio.elapsed() >= Duration::from_secs(AUDIO_IDLE_TIMEOUT_SECS) {
                    receiving_audio = false;
                    pending_stop = Some(state.backend.services().remote_input
                        .stop_stream(connection_id, remote_session_id.unwrap()));
                }
            }
            changed = conn_shutdown_rx.changed() => {
                // Server shutting down (user turned off remote input / reset the PIN / changed
                // the port, triggering a restart): actively disconnect existing connections and
                // revoke paired phones' sessions and typing ability.
                if changed.is_err() || *conn_shutdown_rx.borrow() {
                    preserve_audio = false;
                    log::info!("[remote-input] 服务关停，断开存量手机连接");
                    break;
                }
            }
        }
    }

    // 4) Disconnect ends only capture; recognition and history persistence continue and do not
    // depend on the phone staying connected.
    log::info!("[remote-input] WS 连接已关闭");
    drop(socket);
    if preserve_audio && !*conn_shutdown_rx.borrow() {
        let finishing = openless_core::finish_remote_input_connection(
            state.backend.services().remote_input.as_ref(),
            connection_id,
            remote_session_id,
            pending_stop.take(),
        );
        tokio::pin!(finishing);
        tokio::select! {
            result = &mut finishing => {
                if let Err(error) = result { log::warn!("[remote-input] 断线录音收尾：{error}"); }
            }
            _ = conn_shutdown_rx.changed() => {
                let _ = state.backend.services().remote_input.disconnect(connection_id).await;
            }
        }
        return;
    }
    let _ = state
        .backend
        .services()
        .remote_input
        .disconnect(connection_id)
        .await;
    drop(pending_stop);
}

/// Returns false when the connection should be dropped.
async fn handle_control(
    txt: &str,
    state: &Arc<WsState>,
    connection_id: openless_core::SessionId,
    socket: &mut WebSocket,
    remote_session_id: &mut Option<openless_core::SessionId>,
    pending_stop: &mut Option<PendingRemoteStop>,
) -> bool {
    if let Some(reply) = apply_remote_control(
        txt,
        state.backend.services().remote_input.as_ref(),
        connection_id,
        remote_session_id,
        pending_stop,
    )
    .await
    {
        if socket.send(send_json(&reply)).await.is_err() {
            return false;
        }
    }
    true
}

async fn apply_remote_control(
    txt: &str,
    remote_input: &dyn openless_core::RemoteInputApi,
    connection_id: openless_core::SessionId,
    remote_session_id: &mut Option<openless_core::SessionId>,
    pending_stop: &mut Option<PendingRemoteStop>,
) -> Option<serde_json::Value> {
    let v: serde_json::Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(_) => return None,
    };
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "start" => {
            log::info!("[remote-input] 收到「开始录音」");
            match remote_input.start_stream(connection_id).await {
                Ok(session_id) => {
                    *remote_session_id = Some(session_id);
                    return Some(serde_json::json!({
                        "type": "started",
                        "sessionId": session_id.to_string(),
                        "recoveryKey": remote_input.recovery_key(connection_id, session_id).ok().map(|key| key.into_exposed()),
                    }));
                }
                Err(error) => {
                    if error.code == openless_core::BackendErrorCode::Cancelled {
                        *remote_session_id = None;
                    }
                    let reason = error.to_string();
                    log::warn!("[remote-input] 开始录音被拒：{reason}");
                    return Some(serde_json::json!({"type":"busy","reason":reason}));
                }
            }
        }
        "stop" => {
            log::info!("[remote-input] 收到「结束录音」");
            if pending_stop.is_none() {
                if let Some(session_id) = *remote_session_id {
                    *pending_stop = Some(remote_input.stop_stream(connection_id, session_id));
                }
            }
        }
        "cancel" => {
            if let Some(session_id) = remote_session_id.take() {
                let _ = remote_input.cancel_stream(connection_id, session_id).await;
                *pending_stop = None;
            }
            // A result event may arrive before Core stop's final cleanup steps. The owner is
            // already released then; a late cancel must not drop the still-running stop future,
            // or a Core finishing lease would be left behind. Let select poll it to completion
            // normally.
        }
        "recover" => {
            let session_id = v
                .get("sessionId")
                .and_then(|value| value.as_str())
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
                .map(openless_core::SessionId::from_uuid);
            let recovery = match session_id {
                Some(session_id) => remote_input
                    .recover_stream(
                        connection_id,
                        session_id,
                        openless_core::SecretValue::new(
                            v.get("recoveryKey")
                                .and_then(|value| value.as_str())
                                .unwrap_or_default(),
                        ),
                    )
                    .await
                    .unwrap_or(openless_core::RemoteInputRecovery::Unavailable),
                None => openless_core::RemoteInputRecovery::Unavailable,
            };
            return Some(
                serde_json::json!({"type":"recovery", "sessionId":session_id, "recovery":recovery}),
            );
        }
        "set_insert" => {
            // Phone-side "type on PC" switch: value=true means insert. no_insert = !value.
            let insert = v.get("value").and_then(|b| b.as_bool()).unwrap_or(true);
            log::info!("[remote-input] 电脑落字开关 = {insert}");
            if let Err(error) = remote_input.set_insert(connection_id, insert).await {
                return Some(serde_json::json!({"type":"busy","reason":error.to_string()}));
            }
        }
        _ => {}
    }
    None
}

fn parse_hello_pin(txt: &str) -> openless_core::SecretValue {
    let pin = serde_json::from_str::<serde_json::Value>(txt)
        .ok()
        .filter(|value| value.get("type").and_then(serde_json::Value::as_str) == Some("hello"))
        .and_then(|value| {
            value
                .get("pin")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    openless_core::SecretValue::new(pin)
}

fn parse_audio_frame(
    frame: &[u8],
) -> Result<(openless_core::SessionId, u64, Vec<u8>), openless_core::BackendError> {
    openless_core::RemoteFrameCodec::decode(frame)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use openless_core::{
        BackendConfig, BackendDependencies, BackendErrorCode, OpenLessBackend, RemoteAuthResult,
        RemoteInputConfig, RemoteInputService, SessionId,
    };

    use super::{apply_remote_control, backend_event_to_phone, parse_audio_frame, parse_hello_pin};

    fn backend() -> (
        OpenLessBackend,
        Arc<openless_core::testing::RecordingRemoteInputRuntime>,
        std::path::PathBuf,
    ) {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-remote-ws-contract-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let runtime = Arc::new(openless_core::testing::RecordingRemoteInputRuntime::default());
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.services.remote_input = Arc::new(
            RemoteInputService::new(runtime.clone(), 8443, "zh-CN")
                .expect("fixture remote config is valid"),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .expect("fixture backend is valid");
        (backend, runtime, data_dir)
    }

    #[tokio::test]
    async fn websocket_control_owns_one_stream_and_drops_a_stale_restart_lease() {
        let (backend, runtime, data_dir) = backend();
        let remote = &backend.services().remote_input;
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        let connection_id = SessionId::new();
        let pin = remote.read_pairing_pin().await.unwrap();
        assert_eq!(
            remote
                .authenticate(connection_id, "127.0.0.1".to_string(), pin)
                .await
                .unwrap(),
            RemoteAuthResult::Ok
        );
        let mut session_id = None;
        let mut pending_stop = None;

        let started = apply_remote_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection_id,
            &mut session_id,
            &mut pending_stop,
        )
        .await
        .expect("start must return its typed session identity");
        assert_eq!(started["type"], "started");
        let first_session = session_id.expect("start must establish a session lease");
        let duplicate = apply_remote_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection_id,
            &mut session_id,
            &mut pending_stop,
        )
        .await
        .expect("duplicate start must return a busy response");
        assert_eq!(duplicate["type"], "busy");
        assert_eq!(session_id, Some(first_session));
        assert_eq!(runtime.audio_start_count(), 1);

        assert!(apply_remote_control(
            r#"{"type":"stop"}"#,
            remote.as_ref(),
            connection_id,
            &mut session_id,
            &mut pending_stop,
        )
        .await
        .is_none());
        assert_eq!(
            session_id,
            Some(first_session),
            "stop 必须保留可取消的会话，直到终态"
        );
        pending_stop.take().unwrap().await.unwrap();
        assert_eq!(runtime.audio_stop_count(), 1);

        apply_remote_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection_id,
            &mut session_id,
            &mut pending_stop,
        )
        .await;
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 9443,
            })
            .await
            .unwrap();
        let stale = apply_remote_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection_id,
            &mut session_id,
            &mut pending_stop,
        )
        .await
        .expect("stale connection must be rejected");
        assert_eq!(stale["type"], "busy");
        assert_eq!(session_id, None, "cancelled core lease must be forgotten");
        assert_eq!(runtime.audio_cancel_count(), 1);
        assert_eq!(
            remote.start_stream(connection_id).await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn websocket_stop_returns_to_the_control_loop_and_remains_cancellable() {
        let (backend, runtime, data_dir) = backend();
        let remote = &backend.services().remote_input;
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        let connection = SessionId::new();
        remote
            .authenticate(
                connection,
                "127.0.0.1".into(),
                remote.read_pairing_pin().await.unwrap(),
            )
            .await
            .unwrap();
        let mut owner = None;
        let mut pending_stop = None;
        apply_remote_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        let session = owner.unwrap();
        apply_remote_control(
            r#"{"type":"stop"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        assert_eq!(owner, Some(session));
        assert!(pending_stop.is_some(), "ASR stop 交由 socket select 轮询");
        apply_remote_control(
            r#"{"type":"cancel"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        assert_eq!(owner, None);
        assert!(pending_stop.is_none());
        assert_eq!(runtime.audio_cancel_count(), 1);

        // The downstream result already cleared the owner, but while stop is still doing final
        // cleanup, a late cancel must not destroy this cleanup future; otherwise Core's
        // finishing lease would hang forever.
        pending_stop = Some(Box::pin(async { Ok(()) }));
        apply_remote_control(
            r#"{"type":"cancel"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        pending_stop
            .take()
            .expect("已完成会话的清理仍须被轮询")
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn websocket_events_only_reveal_the_owned_session_and_keep_its_final_result() {
        use openless_core::{
            BackendEvent, BackendEventKind, DictationInsertStatus, DictationResult,
        };
        let session = SessionId::new();
        let mut owner = Some(session);
        let result = |id| BackendEvent {
            sequence: 1,
            session_id: Some(id),
            kind: BackendEventKind::DictationCompleted(DictationResult {
                session_id: id,
                raw_text: "private".into(),
                polished_text: "自己的结果".into(),
                polish_source: None,
                duration_ms: 1,
                inserted: DictationInsertStatus::NotRequested,
            }),
        };
        assert!(
            backend_event_to_phone(&result(SessionId::new()), &mut owner).is_empty(),
            "其他手机/本机结果不可转发"
        );
        assert_eq!(owner, Some(session));
        let messages = backend_event_to_phone(&result(session), &mut owner);
        assert_eq!(messages.len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&messages[1]).unwrap(),
            serde_json::json!({"type":"result", "text":"自己的结果"})
        );
        assert_eq!(owner, None);
        assert!(
            backend_event_to_phone(&result(session), &mut owner).is_empty(),
            "取消/终态之后的迟到结果不可转发"
        );
    }

    #[test]
    fn websocket_wire_parser_requires_contract_2_frames() {
        let session = SessionId::new();
        let mut frame = Vec::from(*b"OL20");
        frame.extend_from_slice(session.as_uuid().as_bytes());
        frame.extend_from_slice(&7_u64.to_be_bytes());
        frame.extend_from_slice(&[1, 0, 2, 0]);

        let parsed = parse_audio_frame(&frame).unwrap();
        assert_eq!(parsed.0, session);
        assert_eq!(parsed.1, 7);
        assert_eq!(parsed.2, vec![1, 0, 2, 0]);

        frame[0] = b'X';
        assert_eq!(
            parse_audio_frame(&frame).unwrap_err().code,
            BackendErrorCode::InvalidArgument
        );
        assert!(parse_hello_pin(r#"{"type":"other","pin":"123456"}"#)
            .expose_secret()
            .is_empty());
    }
}
