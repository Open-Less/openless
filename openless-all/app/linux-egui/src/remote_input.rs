use std::path::PathBuf;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, CredentialKey, CredentialNamespace, CredentialStore,
    DictationStartOptions, RemoteInputRuntimeAdapter, RemoteInputServerBinding,
    RemoteInputServerConfig, SecretValue, SessionId,
};

use crate::qa::LinuxBackendSlot;

const PIN_ACCOUNT: &str = "remote_input_pin";

/// Map the process locale to one of the language bundles shipped by the
/// Remote Input page. Linux commonly reports values such as `en_US.UTF-8` or
/// `zh_HK`, while the Core contract deliberately accepts only canonical
/// bundle identifiers. Keeping this conversion in the Host prevents ambient
/// OS formatting from leaking into Core policy or making backend startup fail.
pub(crate) fn remote_input_locale(locale: &str) -> &'static str {
    let locale = locale
        .split(['.', '@'])
        .next()
        .unwrap_or(locale)
        .replace('_', "-")
        .to_ascii_lowercase();
    if locale.starts_with("zh-tw") || locale.starts_with("zh-hk") || locale.starts_with("zh-mo") {
        "zh-TW"
    } else if locale.starts_with("zh") {
        "zh-CN"
    } else if locale.starts_with("ja") {
        "ja"
    } else if locale.starts_with("ko") {
        "ko"
    } else {
        // English is the shipped fallback for unsupported or absent locales;
        // it is preferable to refusing to start the whole Linux backend.
        "en"
    }
}

pub(crate) struct LinuxRemoteInputRuntime {
    // See LinuxQaRuntime: the Weak slot avoids Backend -> Service -> Adapter ->
    // Backend retention while exposing only Core's external-audio use-case.
    backend: LinuxBackendSlot,
    credentials: Arc<dyn CredentialStore>,
    data_dir: PathBuf,
    // This is transport ownership only. RemoteInputService serializes start,
    // stop, authentication, connection and stream state before invoking us.
    server: Arc<tokio::sync::Mutex<Option<LinuxRemoteServerHandle>>>,
}

impl LinuxRemoteInputRuntime {
    pub(crate) fn new(
        backend: LinuxBackendSlot,
        credentials: Arc<dyn CredentialStore>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            backend,
            credentials,
            data_dir,
            server: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    fn backend(&self) -> Result<Arc<openless_core::OpenLessBackend>, BackendError> {
        self.backend
            .lock()
            .expect("Linux backend slot lock poisoned")
            .upgrade()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Linux Core backend is not bound yet",
                )
            })
    }

    fn pin_key() -> CredentialKey {
        CredentialKey::new(CredentialNamespace::Application, None, PIN_ACCOUNT)
            .expect("built-in remote PIN credential key is valid")
    }
}

impl RemoteInputRuntimeAdapter for LinuxRemoteInputRuntime {
    fn load_pairing_pin(&self) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        self.credentials.read(Self::pin_key())
    }

    fn persist_pairing_pin(
        &self,
        pin: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.credentials.write(Self::pin_key(), pin)
    }

    fn start_server(
        &self,
        config: RemoteInputServerConfig,
    ) -> BoxFuture<'static, Result<RemoteInputServerBinding, BackendError>> {
        let backend = self.backend();
        let data_dir = self.data_dir.clone();
        let server = Arc::clone(&self.server);
        Box::pin(async move {
            let handle = start_server(config.port, data_dir, backend?).await?;
            let binding = RemoteInputServerBinding {
                port: handle.bound_port,
                urls: access_urls(handle.bound_port),
                urls_stale: false,
            };
            *server.lock().await = Some(handle);
            Ok(binding)
        })
    }

    fn stop_server(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let server = Arc::clone(&self.server);
        Box::pin(async move {
            if let Some(handle) = server.lock().await.take() {
                handle.shutdown().await;
            }
            Ok(())
        })
    }

    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        Box::pin(async { Ok(local_lan_ipv4s()) })
    }

    fn start_audio_session(
        &self,
        insert_text: bool,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        let backend = self.backend();
        Box::pin(async move {
            backend?
                .start_external_dictation_with_options(DictationStartOptions {
                    insert_text,
                    ..DictationStartOptions::default()
                })
                .await
        })
    }

    fn feed_audio(
        &self,
        session_id: SessionId,
        pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move { backend?.feed_external_pcm(session_id, &pcm_s16le) })
    }

    fn stop_audio_session(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move {
            backend?
                .stop_dictation_session(session_id)
                .await
                .map(|_| ())
        })
    }

    fn cancel_audio_session(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move { backend?.cancel_dictation(Some(session_id)).await })
    }
}

#[cfg(target_os = "linux")]
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
#[cfg(target_os = "linux")]
use hyper_util::rt::{TokioExecutor, TokioIo};
#[cfg(target_os = "linux")]
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use tokio::net::TcpListener;
#[cfg(target_os = "linux")]
use tokio_rustls::TlsAcceptor;

#[cfg(target_os = "linux")]
mod assets {
    pub const INDEX_HTML: &str =
        include_str!("../../src-tauri/src/remote_server/assets/index.html");
    pub const APP_JS: &str = include_str!("../../src-tauri/src/remote_server/assets/app.js");
    pub const STYLE_CSS: &str = include_str!("../../src-tauri/src/remote_server/assets/style.css");
    pub const ICON_PNG: &[u8] = include_bytes!("../../src-tauri/src/remote_server/assets/icon.png");
    pub const MIC_PNG: &[u8] = include_bytes!("../../src-tauri/src/remote_server/assets/mic.png");
    pub const DONE_PNG: &[u8] = include_bytes!("../../src-tauri/src/remote_server/assets/done.png");
}

#[cfg(target_os = "linux")]
const KEEPALIVE_PING_SECS: u64 = 30;
#[cfg(target_os = "linux")]
const IDLE_TIMEOUT_SECS: u64 = 90;

#[cfg(target_os = "linux")]
struct LinuxRemoteServerHandle {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    // Accepted WebSocket tasks outlive the accept loop, so they receive a
    // separate broadcast and release their Core connection/session leases.
    connections_shutdown: tokio::sync::watch::Sender<bool>,
    join: tokio::task::JoinHandle<()>,
    bound_port: u16,
}

#[cfg(target_os = "linux")]
impl LinuxRemoteServerHandle {
    async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.connections_shutdown.send(true);
        let _ = self.join.await;
    }
}

#[cfg(not(target_os = "linux"))]
struct LinuxRemoteServerHandle {
    bound_port: u16,
}

#[cfg(not(target_os = "linux"))]
impl LinuxRemoteServerHandle {
    async fn shutdown(self) {}
}

#[cfg(target_os = "linux")]
struct WebState {
    backend: Arc<openless_core::OpenLessBackend>,
    cert_der: Vec<u8>,
    connections_shutdown: tokio::sync::watch::Receiver<bool>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct PeerIp(IpAddr);

#[cfg(target_os = "linux")]
fn local_lan_ipv4s() -> Vec<String> {
    let mut addresses = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, address)| match address {
            IpAddr::V4(address) if is_private_lan(address) => Some(address.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    addresses
}

#[cfg(not(target_os = "linux"))]
fn local_lan_ipv4s() -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn is_private_lan(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    !address.is_loopback()
        && !address.is_link_local()
        && ((octets[0] == 10)
            || (octets[0] == 172 && (16..=31).contains(&octets[1]))
            || (octets[0] == 192 && octets[1] == 168))
}

fn access_urls(port: u16) -> Vec<String> {
    local_lan_ipv4s()
        .into_iter()
        .map(|address| format!("https://{address}:{port}"))
        .collect()
}

#[cfg(target_os = "linux")]
fn load_or_generate_certificate(
    directory: &std::path::Path,
    sans: &[String],
) -> Result<(Vec<u8>, rustls::pki_types::PrivateKeyDer<'static>), BackendError> {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    let cert_path = directory.join("remote-cert-v4.der");
    let key_path = directory.join("remote-key-v4.der");
    let sans_path = directory.join("remote-cert-sans-v4.txt");
    if let (Ok(cert), Ok(key), Ok(saved)) = (
        std::fs::read(&cert_path),
        std::fs::read(&key_path),
        std::fs::read_to_string(&sans_path),
    ) {
        let saved = saved.lines().collect::<std::collections::HashSet<_>>();
        if sans.iter().all(|value| saved.contains(value.as_str())) {
            return Ok((cert, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))));
        }
    }
    let mut params = rcgen::CertificateParams::new(sans.to_vec())
        .map_err(|error| remote_platform_error(format!("invalid TLS names: {error}")))?;
    let mut name = rcgen::DistinguishedName::new();
    name.push(rcgen::DnType::CommonName, "OpenLess Remote Input");
    params.distinguished_name = name;
    params
        .extended_key_usages
        .push(rcgen::ExtendedKeyUsagePurpose::ServerAuth);
    let key = rcgen::KeyPair::generate()
        .map_err(|error| remote_platform_error(format!("TLS key generation failed: {error}")))?;
    let cert = params
        .self_signed(&key)
        .map_err(|error| remote_platform_error(format!("TLS certificate failed: {error}")))?;
    let cert_der = cert.der().as_ref().to_vec();
    let key_der = key.serialize_der();
    std::fs::create_dir_all(directory)
        .map_err(|error| remote_platform_error(format!("TLS directory failed: {error}")))?;
    std::fs::write(&cert_path, &cert_der)
        .map_err(|error| remote_platform_error(format!("TLS certificate save failed: {error}")))?;
    std::fs::write(&key_path, &key_der)
        .map_err(|error| remote_platform_error(format!("TLS key save failed: {error}")))?;
    std::fs::write(&sans_path, sans.join("\n"))
        .map_err(|error| remote_platform_error(format!("TLS names save failed: {error}")))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| remote_platform_error(format!("TLS key permissions failed: {error}")))?;
    Ok((
        cert_der,
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
    ))
}

#[cfg(target_os = "linux")]
fn tls_config(
    cert: Vec<u8>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<Arc<rustls::ServerConfig>, BackendError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| remote_platform_error(format!("TLS protocol failed: {error}")))?
        .with_no_client_auth()
        .with_single_cert(vec![rustls::pki_types::CertificateDer::from(cert)], key)
        .map(Arc::new)
        .map_err(|error| remote_platform_error(format!("TLS certificate failed: {error}")))
}

#[cfg(target_os = "linux")]
fn router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    assets::APP_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    assets::STYLE_CSS,
                )
            }),
        )
        .route("/icon.png", get(|| async { image(assets::ICON_PNG) }))
        .route("/mic.png", get(|| async { image(assets::MIC_PNG) }))
        .route("/done.png", get(|| async { image(assets::DONE_PNG) }))
        .route(
            "/cert.cer",
            get(|State(state): State<Arc<WebState>>| async move {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "application/x-x509-ca-cert",
                    )],
                    state.cert_der.clone(),
                )
            }),
        )
        .route("/ws", get(websocket_upgrade))
        .with_state(state)
}

#[cfg(target_os = "linux")]
fn image(bytes: &'static [u8]) -> ([(axum::http::HeaderName, &'static str); 1], &'static [u8]) {
    ([(axum::http::header::CONTENT_TYPE, "image/png")], bytes)
}

#[cfg(target_os = "linux")]
async fn index(State(state): State<Arc<WebState>>) -> Html<String> {
    let locale = state
        .backend
        .services()
        .remote_input
        .status()
        .map(|status| status.locale)
        .unwrap_or_else(|_| "zh-CN".to_string());
    // Read the current PC default on each page load. The page retains an explicit
    // phone choice; only fixed literals may enter its inline script, never raw prefs.
    let default_mode = if state.backend.get_preferences().remote_input_default_mode == "hold" {
        "hold"
    } else {
        "toggle"
    };
    Html(
        assets::INDEX_HTML
            .replace("%%OL_LANG%%", &locale)
            .replace("%%OL_DEFAULT_MODE%%", default_mode),
    )
}

#[cfg(target_os = "linux")]
async fn websocket_upgrade(
    State(state): State<Arc<WebState>>,
    axum::Extension(PeerIp(peer)): axum::Extension<PeerIp>,
    websocket: WebSocketUpgrade,
) -> impl IntoResponse {
    websocket.on_upgrade(move |socket| websocket_session(socket, state, peer))
}

#[cfg(target_os = "linux")]
async fn start_server(
    port: u16,
    data_dir: PathBuf,
    backend: Arc<openless_core::OpenLessBackend>,
) -> Result<LinuxRemoteServerHandle, BackendError> {
    let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    sans.extend(local_lan_ipv4s());
    let (cert_der, key) = load_or_generate_certificate(&data_dir.join("remote-input"), &sans)?;
    let acceptor = TlsAcceptor::from(tls_config(cert_der.clone(), key)?);
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .map_err(|error| remote_platform_error(format!("remote input bind failed: {error}")))?;
    let bound_port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(port);
    let (connections_shutdown, receiver) = tokio::sync::watch::channel(false);
    let state = Arc::new(WebState {
        backend,
        cert_der,
        connections_shutdown: receiver,
    });
    let app = router(state);
    let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    let Ok((tcp, peer)) = accepted else { continue };
                    let acceptor = acceptor.clone();
                    let service = app.clone().layer(axum::Extension(PeerIp(peer.ip())));
                    tokio::spawn(async move {
                        let Ok(tls) = acceptor.accept(tcp).await else { return };
                        let io = TokioIo::new(tls);
                        let service = hyper_util::service::TowerToHyperService::new(service);
                        let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection_with_upgrades(io, service)
                            .await;
                    });
                }
            }
        }
    });
    Ok(LinuxRemoteServerHandle {
        shutdown: Some(shutdown),
        connections_shutdown,
        join,
        bound_port,
    })
}

#[cfg(not(target_os = "linux"))]
async fn start_server(
    _port: u16,
    _data_dir: PathBuf,
    _backend: Arc<openless_core::OpenLessBackend>,
) -> Result<LinuxRemoteServerHandle, BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "Linux remote input transport is only available on Linux",
    ))
}

#[cfg(target_os = "linux")]
async fn websocket_session(mut socket: WebSocket, state: Arc<WebState>, peer: IpAddr) {
    let connection_id = SessionId::new();
    let auth = match tokio::time::timeout(Duration::from_secs(15), socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            state
                .backend
                .services()
                .remote_input
                .authenticate(connection_id, peer.to_string(), hello_pin(&text))
                .await
        }
        _ => return,
    };
    let Ok(auth) = auth else { return };
    let (ok, reason) = match auth {
        openless_core::RemoteAuthResult::Ok => (true, None),
        openless_core::RemoteAuthResult::BadPin => (false, Some("bad-pin")),
        openless_core::RemoteAuthResult::Locked => (false, Some("locked")),
    };
    let _ = socket
        .send(json_message(serde_json::json!({
            "type": "auth",
            "ok": ok,
            "reason": reason,
        })))
        .await;
    if !ok {
        return;
    }

    let mut events = state.backend.subscribe();
    let mut remote_session = None;
    // Poll finalization beside socket input. Awaiting it in the receive arm
    // would prevent the phone from cancelling a slow provider request.
    let mut pending_stop: Option<(SessionId, BoxFuture<'static, Result<(), BackendError>>)> = None;
    let mut shutdown = state.connections_shutdown.clone();
    let mut keepalive = tokio::time::interval(Duration::from_secs(KEEPALIVE_PING_SECS));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_received = Instant::now();
    'connection: loop {
        tokio::select! {
            incoming = socket.recv() => {
                last_received = Instant::now();
                match incoming {
                    Some(Ok(Message::Binary(frame))) => {
                        if let Ok((session_id, sequence, pcm)) = openless_core::RemoteFrameCodec::decode(&frame) {
                            let _ = state.backend.services().remote_input
                                .feed_pcm(connection_id, session_id, sequence, pcm).await;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Some(reply) = apply_control(
                            &text,
                            state.backend.services().remote_input.as_ref(),
                            connection_id,
                            &mut remote_session,
                            &mut pending_stop,
                        ).await {
                            if socket.send(json_message(reply)).await.is_err() { break; }
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            event = events.recv() => {
                let Ok(event) = event else { continue };
                if remote_session.is_none() || event.session_id != remote_session { continue; }
                let terminal = matches!(&event.kind,
                    openless_core::BackendEventKind::DictationCompleted(_)
                    | openless_core::BackendEventKind::DictationStateChanged(openless_core::DictationStateSnapshot {
                        phase: openless_core::DictationPhase::Cancelled | openless_core::DictationPhase::Failed, ..
                    }));
                for reply in remote_event(event.kind) {
                    // A failed outbound send means this socket no longer owns
                    // a usable transport. Leave the outer loop immediately so
                    // Core disconnect cancels any active external-audio lease.
                    if socket.send(json_message(reply)).await.is_err() {
                        break 'connection;
                    }
                }
                if terminal { remote_session = None; }
            }
            result = async { pending_stop.as_mut().expect("guarded pending stop").1.as_mut().await }, if pending_stop.is_some() => {
                let (session_id, _) = pending_stop.take().expect("completed pending stop");
                // Successful finalization queues done/result events. Keep their
                // output owner until they are delivered, not merely until the
                // future returns. Late errors after cancel must stay invisible.
                if let Err(error) = result {
                    if remote_session == Some(session_id) {
                        remote_session = None;
                        if socket.send(json_message(serde_json::json!({"type":"status", "kind":"error", "message":error.to_string()}))).await.is_err() { break; }
                    }
                }
            }
            _ = keepalive.tick() => {
                if last_received.elapsed() > Duration::from_secs(IDLE_TIMEOUT_SECS)
                    || socket.send(Message::Ping(Vec::new())).await.is_err()
                {
                    break;
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
        }
    }
    let _ = state
        .backend
        .services()
        .remote_input
        .disconnect(connection_id)
        .await;
}

#[cfg(target_os = "linux")]
fn json_message(value: serde_json::Value) -> Message {
    Message::Text(value.to_string())
}

#[cfg(target_os = "linux")]
fn hello_pin(text: &str) -> SecretValue {
    let pin = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .filter(|value| value.get("type").and_then(serde_json::Value::as_str) == Some("hello"))
        .and_then(|value| {
            value
                .get("pin")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    SecretValue::new(pin)
}

#[cfg(target_os = "linux")]
async fn apply_control(
    text: &str,
    remote: &dyn openless_core::RemoteInputApi,
    connection_id: SessionId,
    remote_session: &mut Option<SessionId>,
    pending_stop: &mut Option<(SessionId, BoxFuture<'static, Result<(), BackendError>>)>,
) -> Option<serde_json::Value> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    match value.get("type").and_then(serde_json::Value::as_str)? {
        "start" if pending_stop.is_some() => {
            Some(serde_json::json!({"type":"busy", "reason":"previous stream is still finishing"}))
        }
        "start" => match remote.start_stream(connection_id).await {
            Ok(session_id) => {
                *remote_session = Some(session_id);
                Some(serde_json::json!({"type":"started", "sessionId":session_id.to_string()}))
            }
            Err(error) => Some(serde_json::json!({"type":"busy", "reason":error.to_string()})),
        },
        "stop" => {
            if pending_stop.is_none() {
                if let Some(session_id) = *remote_session {
                    *pending_stop =
                        Some((session_id, remote.stop_stream(connection_id, session_id)));
                }
            }
            None
        }
        "cancel" => {
            if let Some(session_id) = remote_session.take() {
                let _ = remote.cancel_stream(connection_id, session_id).await;
                // Core has now revoked the audio lease. A provider may leave
                // its in-flight HTTP request pending until timeout; dropping
                // that abandoned stop future lets the phone start immediately.
                // If a terminal already cleared the owner, leave pending_stop
                // alone so its remaining Core cleanup still gets polled.
                *pending_stop = None;
                return Some(serde_json::json!({"type":"status", "kind":"done"}));
            }
            None
        }
        "set_insert" => {
            let insert = value
                .get("value")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            remote
                .set_insert(connection_id, insert)
                .await
                .err()
                .map(|error| serde_json::json!({"type":"busy", "reason":error.to_string()}))
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn remote_event(kind: openless_core::BackendEventKind) -> Vec<serde_json::Value> {
    match kind {
        openless_core::BackendEventKind::DictationStateChanged(state) => {
            let kind = match state.phase {
                openless_core::DictationPhase::Starting
                | openless_core::DictationPhase::Recording => "recording",
                openless_core::DictationPhase::Transcribing => "transcribing",
                openless_core::DictationPhase::Polishing
                | openless_core::DictationPhase::Inserting => "polishing",
                openless_core::DictationPhase::Cancelled => "done",
                openless_core::DictationPhase::Failed => "error",
                _ => return Vec::new(),
            };
            vec![serde_json::json!({
                "type":"status",
                "kind":kind,
                "level":state.level,
                "message":state.message,
            })]
        }
        openless_core::BackendEventKind::DictationCompleted(result) => vec![
            serde_json::json!({
                "type":"status",
                "kind":"done",
                "insertedChars":result.polished_text.chars().count(),
            }),
            serde_json::json!({"type":"result", "text":result.polished_text}),
        ],
        _ => Vec::new(),
    }
}

#[cfg(target_os = "linux")]
fn remote_platform_error(message: String) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_urls_include_only_private_lan_addresses() {
        assert!(access_urls(8443)
            .iter()
            .all(|url| url.starts_with("https://") && url.ends_with(":8443")));
    }

    #[test]
    fn system_locales_map_to_shipped_remote_input_bundles() {
        assert_eq!(remote_input_locale("en_US.UTF-8"), "en");
        assert_eq!(remote_input_locale("zh_HK.UTF-8"), "zh-TW");
        assert_eq!(remote_input_locale("zh_CN.UTF-8"), "zh-CN");
        assert_eq!(remote_input_locale("ja_JP"), "ja");
        assert_eq!(remote_input_locale("ko_KR"), "ko");
        assert_eq!(remote_input_locale("fr_FR.UTF-8"), "en");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hello_requires_the_typed_handshake() {
        assert_eq!(
            hello_pin(r#"{"type":"hello","pin":"123456"}"#).expose_secret(),
            "123456"
        );
        assert!(hello_pin(r#"{"type":"other","pin":"123456"}"#)
            .expose_secret()
            .is_empty());
    }

    #[cfg(target_os = "linux")]
    async fn connected_remote_fixture(
        runtime: Arc<dyn RemoteInputRuntimeAdapter>,
    ) -> (openless_core::OpenLessBackend, SessionId, PathBuf) {
        use openless_core::{
            BackendConfig, BackendDependencies, RemoteInputConfig, RemoteInputService,
        };
        let data_dir =
            std::env::temp_dir().join(format!("openless-remote-wire-{}", uuid::Uuid::new_v4()));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.services.remote_input =
            Arc::new(RemoteInputService::new(runtime, 8443, "en").unwrap());
        let backend = openless_core::OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..Default::default()
            },
            dependencies,
        )
        .unwrap();
        let remote = Arc::clone(&backend.services().remote_input);
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
        (backend, connection, data_dir)
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn stop_keeps_the_session_owner_for_terminal_websocket_events() {
        let (backend, connection, data_dir) = connected_remote_fixture(Arc::new(
            openless_core::testing::RecordingRemoteInputRuntime::default(),
        ))
        .await;
        let remote = &backend.services().remote_input;
        let mut owner = None;
        let mut pending_stop = None;
        apply_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await
        .unwrap();
        let started = owner.unwrap();
        apply_control(
            r#"{"type":"stop"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        pending_stop.take().unwrap().1.await.unwrap();
        assert_eq!(
            owner,
            Some(started),
            "queued done/result must retain their outbound owner after stop"
        );
        remote.disconnect(connection).await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct NeverFinishingRuntime(openless_core::testing::RecordingRemoteInputRuntime);

    // Replace only the external provider wait. Authentication, session ownership,
    // finalization state and cancellation still execute the real Core service.
    #[cfg(target_os = "linux")]
    impl RemoteInputRuntimeAdapter for NeverFinishingRuntime {
        fn load_pairing_pin(
            &self,
        ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
            self.0.load_pairing_pin()
        }
        fn persist_pairing_pin(
            &self,
            pin: SecretValue,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.persist_pairing_pin(pin)
        }
        fn start_server(
            &self,
            config: RemoteInputServerConfig,
        ) -> BoxFuture<'static, Result<RemoteInputServerBinding, BackendError>> {
            self.0.start_server(config)
        }
        fn stop_server(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.stop_server()
        }
        fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
            self.0.list_local_ips()
        }
        fn start_audio_session(
            &self,
            insert: bool,
        ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
            self.0.start_audio_session(insert)
        }
        fn feed_audio(
            &self,
            session: SessionId,
            pcm: Vec<u8>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.feed_audio(session, pcm)
        }
        fn stop_audio_session(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(std::future::pending())
        }
        fn cancel_audio_session(
            &self,
            session: SessionId,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.cancel_audio_session(session)
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn cancel_releases_a_never_finishing_stop_before_the_next_start() {
        let runtime = Arc::new(NeverFinishingRuntime::default());
        let (backend, connection, data_dir) = connected_remote_fixture(runtime.clone()).await;
        let remote = &backend.services().remote_input;
        let mut owner = None;
        let mut pending_stop = None;
        apply_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        let first = owner.unwrap();
        apply_control(
            r#"{"type":"stop"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        // Enter the actual Core finishing state and reach the never-ready Host
        // wait, rather than merely testing a not-yet-polled stop future.
        assert!(futures_util::poll!(pending_stop.as_mut().unwrap().1.as_mut()).is_pending());
        assert_eq!(remote.status().unwrap().active_session_id, Some(first));
        apply_control(
            r#"{"type":"cancel"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        assert_eq!(runtime.0.audio_cancel_count(), 1);
        let response = apply_control(
            r#"{"type":"start"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await
        .unwrap();
        assert_eq!(
            response["type"], "started",
            "cancel must permit a new recording before the old HTTP timeout"
        );
        assert_ne!(owner, Some(first));

        remote.disconnect(connection).await.unwrap();
        owner = None;
        pending_stop = Some((first, Box::pin(async { Ok(()) })));
        apply_control(
            r#"{"type":"cancel"}"#,
            remote.as_ref(),
            connection,
            &mut owner,
            &mut pending_stop,
        )
        .await;
        pending_stop
            .take()
            .expect("a terminal already removed the owner; its cleanup must finish")
            .1
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
