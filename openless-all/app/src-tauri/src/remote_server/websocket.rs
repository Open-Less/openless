//! WebSocket admission, audio transport and result ownership, independent of the desktop shell.
//! Kept separate so the production handler can be exercised without native ASR/UI DLLs.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use serde::Serialize;

/// Server keepalive: send a WS Ping every KEEPALIVE_PING_SECS (browsers answer Pong
/// automatically); if no upstream frame (including Pong) arrives for IDLE_TIMEOUT_SECS, treat
/// the link as a half-open dead connection and disconnect. Phones with the screen off or after
/// Wi-Fi drift often never send a TCP FIN, and without liveness probes recv() hangs forever:
/// the connection task, event subscription, and any in-flight remote session all hang.
const KEEPALIVE_PING_SECS: u64 = 30;
const IDLE_TIMEOUT_SECS: u64 = 90;
const AUDIO_IDLE_TIMEOUT_SECS: u64 = 15;

pub(super) struct WsState {
    pub(super) backend: Arc<openless_core::OpenLessBackend>,
    /// Public CA certificate offered for phone installation; never the server leaf or key.
    pub(super) cert_der: Vec<u8>,
    /// Receiver of the server-shutdown broadcast; each WS connection clones one and watches it
    /// in its main loop's select.
    pub(super) conn_shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

/// Peer IP injected by the accept loop (axum Extension). hyper talking to the TLS stream
/// directly cannot get ConnectInfo, so an Extension carrying the peer is attached to each
/// connection's service and read by the handler.
#[derive(Clone, Copy)]
pub(super) struct PeerIp(pub(super) IpAddr);

// ───────────────────────── WebSocket ─────────────────────────

pub(super) async fn ws_upgrade(
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
///
/// `wants_partials`: the client asked for live text in its hello frame. The owned session's
/// streaming transcript (`TranscriptDelta`) is then forwarded as `partial` frames; it is the
/// same session-scoped text the desktop capsule shows, and never another session's.
fn backend_event_to_phone(
    event: &openless_core::BackendEvent,
    remote_session_id: &mut Option<openless_core::SessionId>,
    wants_partials: bool,
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
        BackendEventKind::TranscriptDelta(delta) if wants_partials => {
            // Same contract as TranscriptAccumulator::apply: replace from `offset` (chars).
            vec![serde_json::json!({
                "type":"partial", "text":delta.text, "offset":delta.offset, "final":delta.is_final,
            })
            .to_string()]
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
    let wants_partials;
    let authentication = match tokio::time::timeout(Duration::from_secs(15), socket.recv()).await {
        Ok(Some(Ok(Message::Text(txt)))) => {
            wants_partials = parse_hello_partials(&txt);
            openless_core::authenticate_remote_input_hello(
                state.backend.services().remote_input.as_ref(),
                connection_id,
                peer_ip.to_string(),
                &txt,
            )
            .await
        }
        _ => return,
    };
    let reply = match authentication {
        Ok(reply) => reply,
        Err(error) => {
            log::warn!("[remote-input] authentication failed: {error}");
            return;
        }
    };
    let accepted = reply["ok"] == true;
    if socket.send(send_json(&reply)).await.is_err() {
        // Admission may have succeeded before the phone disappeared.
        let _ = state
            .backend
            .services()
            .remote_input
            .disconnect(connection_id)
            .await;
        return;
    }
    if !accepted {
        return;
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
                    let messages =
                        backend_event_to_phone(&event, &mut remote_session_id, wants_partials);
                    for msg in messages {
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

/// Optional `"partials": true` in the hello frame opts this connection into live text.
/// Clients that do not send it (the bundled phone page) keep the previous message set.
fn parse_hello_partials(txt: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(txt)
        .ok()
        .filter(|value| value.get("type").and_then(serde_json::Value::as_str) == Some("hello"))
        .and_then(|value| value.get("partials").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
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

    use super::{
        apply_remote_control, backend_event_to_phone, parse_audio_frame, parse_hello_partials,
    };

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
    async fn websocket_rotation_keeps_two_phones_audio_and_final_results_connected() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as PhoneMessage;
        type Phone = tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >;
        async fn receive(phone: &mut Phone, kind: &str) -> serde_json::Value {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    match phone.next().await.expect("phone stays connected").unwrap() {
                        PhoneMessage::Text(text) => {
                            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                            if value["type"] == kind {
                                return value;
                            }
                        }
                        PhoneMessage::Close(_) => panic!("PIN rotation closed a phone connection"),
                        _ => {}
                    }
                }
            })
            .await
            .expect("phone receives its reply")
        }
        let (backend, runtime, data_dir) = backend();
        let backend = Arc::new(backend);
        let remote = &backend.services().remote_input;
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        let (connection_shutdown, shutdown_receiver) = tokio::sync::watch::channel(false);
        let state = Arc::new(super::WsState {
            backend: Arc::clone(&backend),
            cert_der: Vec::new(),
            conn_shutdown_rx: shutdown_receiver,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route("/ws", axum::routing::get(super::ws_upgrade))
            .with_state(state)
            .layer(axum::Extension(super::PeerIp("127.0.0.1".parse().unwrap())));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let url = format!("ws://{address}/ws");
        let (mut first, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let pin = remote.read_pairing_pin().await.unwrap();
        first
            .send(PhoneMessage::Text(
                serde_json::json!({
                    "type": "hello", "pin": pin.expose_secret(),
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(receive(&mut first, "auth").await["ok"], true);
        first
            .send(PhoneMessage::Text(r#"{"type":"start"}"#.into()))
            .await
            .unwrap();
        let started = receive(&mut first, "started").await;
        let session = SessionId::from_uuid(
            uuid::Uuid::parse_str(started["sessionId"].as_str().unwrap()).unwrap(),
        );
        first
            .send(PhoneMessage::Binary(
                openless_core::RemoteFrameCodec::encode(session, 0, &[1, 0]).unwrap(),
            ))
            .await
            .unwrap();

        // A second phone pairs while the first records; this consumes and rotates the PIN too.
        let (mut second, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let pin = remote.read_pairing_pin().await.unwrap();
        second
            .send(PhoneMessage::Text(
                serde_json::json!({
                    "type": "hello", "pin": pin.expose_secret(),
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(receive(&mut second, "auth").await["ok"], true);
        remote.rotate_pairing_pin().await.unwrap();
        first
            .send(PhoneMessage::Binary(
                openless_core::RemoteFrameCodec::encode(session, 1, &[2, 0]).unwrap(),
            ))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while runtime.frames().len() < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            runtime.frames(),
            vec![(session, vec![1, 0]), (session, vec![2, 0])]
        );
        assert_eq!(remote.status().unwrap().connection_count, 2);
        first
            .send(PhoneMessage::Text(r#"{"type":"stop"}"#.into()))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while runtime.audio_stop_count() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        remote.rotate_pairing_pin().await.unwrap();
        backend.event_publisher().publish(
            Some(session),
            openless_core::BackendEventKind::DictationCompleted(openless_core::DictationResult {
                session_id: session,
                raw_text: "recorded".into(),
                polished_text: "result after rotation".into(),
                polish_source: None,
                duration_ms: 1,
                inserted: openless_core::DictationInsertStatus::NotRequested,
            }),
        );
        assert_eq!(
            receive(&mut first, "result").await["text"],
            "result after rotation"
        );
        for phone in [&mut first, &mut second] {
            phone
                .send(PhoneMessage::Text(r#"{"type":"start"}"#.into()))
                .await
                .unwrap();
            assert!(receive(phone, "started").await["sessionId"].is_string());
        }
        assert_eq!(runtime.server_start_count(), 1);
        assert_eq!(runtime.server_stop_count(), 0);
        assert_eq!(runtime.audio_cancel_count(), 0);
        let _ = connection_shutdown.send(true);
        drop(first);
        drop(second);
        let _ = stop.send(());
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
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
            backend_event_to_phone(&result(SessionId::new()), &mut owner, false).is_empty(),
            "其他手机/本机结果不可转发"
        );
        assert_eq!(owner, Some(session));
        let messages = backend_event_to_phone(&result(session), &mut owner, false);
        assert_eq!(messages.len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&messages[1]).unwrap(),
            serde_json::json!({"type":"result", "text":"自己的结果"})
        );
        assert_eq!(owner, None);
        assert!(
            backend_event_to_phone(&result(session), &mut owner, false).is_empty(),
            "取消/终态之后的迟到结果不可转发"
        );
    }

    #[test]
    fn websocket_forwards_live_transcript_only_when_asked_and_only_for_the_owned_session() {
        use openless_core::{BackendEvent, BackendEventKind, TranscriptDelta};
        let session = SessionId::new();
        let mut owner = Some(session);
        let delta = |id, text: &str, offset| BackendEvent {
            sequence: 1,
            session_id: Some(id),
            kind: BackendEventKind::TranscriptDelta(TranscriptDelta {
                text: text.into(),
                offset,
                is_final: false,
            }),
        };
        assert!(
            backend_event_to_phone(&delta(session, "你好", 0), &mut owner, false).is_empty(),
            "未在 hello 中请求实时文字的客户端保持原有消息集"
        );
        assert!(
            backend_event_to_phone(&delta(SessionId::new(), "别人的", 0), &mut owner, true)
                .is_empty(),
            "其他会话的实时文字不可转发"
        );
        let messages = backend_event_to_phone(&delta(session, "您好", 1), &mut owner, true);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&messages[0]).unwrap(),
            serde_json::json!({"type":"partial", "text":"您好", "offset":1, "final":false})
        );
        assert_eq!(owner, Some(session), "实时文字不结束会话所有权");

        assert!(parse_hello_partials(
            r#"{"type":"hello","pin":"123456","partials":true}"#
        ));
        assert!(!parse_hello_partials(r#"{"type":"hello","pin":"123456"}"#));
        assert!(!parse_hello_partials(r#"{"type":"other","partials":true}"#));
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
    }
}
