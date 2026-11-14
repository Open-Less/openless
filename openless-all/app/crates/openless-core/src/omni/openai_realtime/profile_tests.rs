//! Offline integration contracts for the real shared Realtime socket worker.

use super::*;
use crate::omni::realtime_options::{RealtimeAuthMode, RealtimeOptions, RealtimeReasoningEffort};
use crate::ports::{TextStreamChunk, TextStreamSink};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

type ServerSocket = WebSocketStream<TcpStream>;
const DEADLINE: Duration = Duration::from_secs(5);
const KEY: &str = "synthetic-profile-main-key";
const QWEN_MODEL: &str = "synthetic-qwen-omni-alias";
const CURRENT: &str = "dashscope-realtime";
const LEGACY: &str = "dashscope-realtime-legacy";

#[derive(Default)]
struct Capture(Mutex<Vec<TextStreamChunk>>);

impl TextStreamSink for Capture {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), crate::BackendError> {
        self.0.lock().push(chunk);
        Ok(())
    }
}

fn configuration(endpoint: &str, provider: &str, auth_mode: RealtimeAuthMode) -> OmniConfig {
    let mut extra_headers = HashMap::from([("X-Tenant".into(), "synthetic-tenant".into())]);
    if auth_mode == RealtimeAuthMode::Headers {
        extra_headers.insert("X-Gateway-Key".into(), "synthetic-header-key".into());
    }
    OmniConfig {
        provider_id: provider.into(),
        base_url: endpoint.into(),
        api_key: if auth_mode == RealtimeAuthMode::Headers {
            ""
        } else {
            KEY
        }
        .into(),
        model: if provider == PROVIDER_ID {
            DEFAULT_MODEL
        } else {
            QWEN_MODEL
        }
        .into(),
        extra_headers,
        temperature: None,
        thinking_enabled: false,
        realtime_options: RealtimeOptions {
            auth_mode,
            ..RealtimeOptions::default()
        },
    }
}

async fn serve<F, Fut>(
    path: &'static str,
    model: &'static str,
    auth: RealtimeAuthMode,
    handler: F,
) -> (String, JoinHandle<()>)
where
    F: FnOnce(ServerSocket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}{path}?route=fixture&model=old&api_key=URL_SECRET",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        let (stream, _) = timeout(DEADLINE, listener.accept()).await.unwrap().unwrap();
        let socket = timeout(
            DEADLINE,
            tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    assert_eq!(
                        request.uri().path(),
                        path,
                        "explicit WS gateway paths must be retained"
                    );
                    let query =
                        url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                            .map(|(key, value)| (key.into_owned(), value.into_owned()))
                            .collect::<HashMap<_, _>>();
                    assert_eq!(
                        query,
                        HashMap::from([
                            ("route".into(), "fixture".into()),
                            ("model".into(), model.into())
                        ])
                    );
                    let headers = request.headers();
                    assert_eq!(headers["x-tenant"], "synthetic-tenant");
                    match auth {
                        RealtimeAuthMode::Bearer => {
                            assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
                            assert!(headers.get("api-key").is_none());
                        }
                        RealtimeAuthMode::ApiKey => {
                            assert_eq!(headers["api-key"], KEY);
                            assert!(headers.get("authorization").is_none());
                        }
                        RealtimeAuthMode::Headers => {
                            assert_eq!(headers["x-gateway-key"], "synthetic-header-key");
                            assert!(headers.get("authorization").is_none());
                            assert!(headers.get("api-key").is_none());
                        }
                    }
                    assert_eq!(headers["sec-websocket-version"], "13");
                    assert_ne!(headers["sec-websocket-key"], "synthetic-override");
                    assert_eq!(headers["upgrade"], "websocket");
                    Ok(response)
                },
            ),
        )
        .await
        .expect("fake profile handshake stalled")
        .unwrap();
        timeout(DEADLINE, handler(socket))
            .await
            .expect("fake profile server stalled");
    });
    (endpoint, server)
}

async fn receive(socket: &mut ServerSocket) -> Value {
    let message = timeout(DEADLINE, socket.next())
        .await
        .unwrap()
        .expect("client closed early")
        .unwrap();
    match message {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected a JSON event, got {other:?}"),
    }
}

async fn send(socket: &mut ServerSocket, event: Value) {
    socket.send(Message::text(event.to_string())).await.unwrap();
}

async fn created(socket: &mut ServerSocket) -> Value {
    send(socket, json!({"type":"session.created"})).await;
    let setup = receive(socket).await;
    assert_eq!(setup["type"], "session.update");
    setup
}

async fn closed(socket: &mut ServerSocket) {
    match timeout(DEADLINE, socket.next()).await.unwrap() {
        None | Some(Ok(Message::Close(_))) | Some(Err(_)) => {}
        other => panic!("unexpected traffic after terminal state: {other:?}"),
    }
    let _ = socket.flush().await;
}

async fn until_response(socket: &mut ServerSocket) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let event = receive(socket).await;
        let done = event["type"] == "response.create";
        events.push(event);
        if done {
            return events;
        }
    }
}

fn audio_bytes(events: &[Value]) -> Vec<u8> {
    events
        .iter()
        .filter(|event| event["type"] == "input_audio_buffer.append")
        .flat_map(|event| {
            base64::engine::general_purpose::STANDARD
                .decode(event["audio"].as_str().unwrap())
                .unwrap()
        })
        .collect()
}

fn pcm(value: i16, samples: usize) -> Vec<u8> {
    (0..samples).flat_map(|_| value.to_le_bytes()).collect()
}

async fn completed(socket: &mut ServerSocket, text: &str) {
    send(
        socket,
        json!({"type":"response.done", "response":{"status":"completed", "output":[{
            "type":"message", "content":[{"type":"text", "text":text}]
        }]}}),
    )
    .await;
}

fn assert_qwen_setup(setup: &Value, legacy: bool) {
    let session = &setup["session"];
    assert_eq!(session["modalities"], json!(["text"]));
    assert_eq!(session.get("turn_detection"), Some(&Value::Null));
    for key in ["type", "output_modalities", "reasoning"] {
        assert!(
            session.get(key).is_none(),
            "Qwen setup must not inherit GA field {key}"
        );
    }
    if legacy {
        assert_eq!(session["input_audio_format"], "pcm");
        assert!(session.get("audio").is_none());
    } else {
        assert_eq!(
            session["audio"]["input"]["format"],
            json!({"type":"pcm", "sample_rate":16000})
        );
        assert!(session.get("input_audio_format").is_none());
    }
}

#[tokio::test]
async fn qwen_profiles_buffer_before_ack_preserve_exact_16k_tail_and_manual_stop_order() {
    for provider_id in [CURRENT, LEGACY] {
        let legacy = provider_id == LEGACY;
        let expected = (0..1607_i16)
            .flat_map(|sample| (sample * 7 - 3000).to_le_bytes())
            .collect::<Vec<_>>();
        let expected_wire = expected.clone();
        let (setup_sent, setup_observed) = oneshot::channel();
        let (release, allowed) = oneshot::channel();
        let (endpoint, server) = serve("/gateway/qwen-socket/", QWEN_MODEL, RealtimeAuthMode::Bearer,
            move |mut socket| async move {
                let setup = created(&mut socket).await;
                assert_qwen_setup(&setup, legacy);
                assert_eq!(setup["session"]["instructions"], "frozen synthetic context");
                setup_sent.send(()).unwrap();
                assert!(timeout(Duration::from_millis(40), socket.next()).await.is_err(),
                    "queued audio must wait for the session ACK");
                allowed.await.unwrap();
                send(&mut socket, json!({"type":"session.updated"})).await;
                let events = until_response(&mut socket).await;
                assert_eq!(audio_bytes(&events), expected_wire);
                let ending = &events[events.len() - 3..];
                assert_eq!(ending, &[
                    json!({"type":"session.update", "session":{"instructions":"final synthetic context"}}),
                    json!({"type":"input_audio_buffer.commit"}),
                    json!({"type":"response.create"}),
                ]);
                assert_eq!(events.iter().filter(|event| event["type"] == "response.create").count(), 1);
                completed(&mut socket, "简体 English 尾帧。").await;
                closed(&mut socket).await;
            }).await;
        let provider = OpenAIRealtime::new(
            configuration(&endpoint, provider_id, RealtimeAuthMode::Bearer),
            Arc::new(crate::TokioTaskSpawner),
        );
        assert_eq!(provider.label().provider, provider_id);
        let active = timeout(
            Duration::from_millis(250),
            provider.connect("frozen synthetic context", Arc::new(Capture::default())),
        )
        .await
        .expect("local recording must not wait for the handshake")
        .unwrap();
        for chunk in expected.chunks(514) {
            active.consume_pcm_chunk(chunk);
        }
        let pending = tokio::spawn(async move { active.finish("final synthetic context").await });
        timeout(DEADLINE, setup_observed).await.unwrap().unwrap();
        assert!(!pending.is_finished());
        release.send(()).unwrap();
        let result = timeout(DEADLINE, pending).await.unwrap().unwrap().unwrap();
        assert_eq!(result.text, "简体 English 尾帧。");
        assert_eq!(result.duration_ms, (expected.len() as u64 * 1000) / 32_000);
        timeout(DEADLINE, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn qwen_text_done_waits_for_authoritative_completed_response_done() {
    let (snapshot_sent, snapshot_seen) = oneshot::channel();
    let (release, allowed) = oneshot::channel();
    let (endpoint, server) = serve(
        "/qwen/current",
        QWEN_MODEL,
        RealtimeAuthMode::Bearer,
        move |mut socket| async move {
            assert_qwen_setup(&created(&mut socket).await, false);
            send(&mut socket, json!({"type":"session.updated"})).await;
            until_response(&mut socket).await;
            send(
                &mut socket,
                json!({"type":"response.text.done", "text":"只在最终完成后输出。"}),
            )
            .await;
            snapshot_sent.send(()).unwrap();
            allowed.await.unwrap();
            completed(&mut socket, "只在最终完成后输出。").await;
            closed(&mut socket).await;
        },
    )
    .await;
    let provider = OpenAIRealtime::new(
        configuration(&endpoint, CURRENT, RealtimeAuthMode::Bearer),
        Arc::new(crate::TokioTaskSpawner),
    );
    let active = provider
        .connect("fixture", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&pcm(1234, 1600));
    let mut pending = tokio::spawn(async move { active.finish("fixture").await });
    timeout(DEADLINE, snapshot_seen).await.unwrap().unwrap();
    assert!(
        timeout(Duration::from_millis(100), &mut pending)
            .await
            .is_err(),
        "text.done must not release a successful final result"
    );
    release.send(()).unwrap();
    assert_eq!(
        timeout(DEADLINE, pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .text,
        "只在最终完成后输出。"
    );
    timeout(DEADLINE, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn qwen_incomplete_audio_only_and_audio_deltas_never_promote_a_text_snapshot() {
    for (provider_id, case) in [
        (CURRENT, "incomplete"),
        (LEGACY, "audio-only"),
        (CURRENT, "audio-delta"),
    ] {
        let (endpoint, server) = serve("/qwen/failure", QWEN_MODEL, RealtimeAuthMode::Bearer,
            move |mut socket| async move {
                created(&mut socket).await;
                send(&mut socket, json!({"type":"session.updated"})).await;
                until_response(&mut socket).await;
                send(&mut socket, json!({"type":"response.text.done", "text":"untrusted partial snapshot"})).await;
                let event = match case {
                    "incomplete" => json!({"type":"response.done", "response":{"status":"incomplete",
                        "output":[{"content":[{"type":"text","text":"untrusted partial snapshot"}]}]}}),
                    "audio-only" => json!({"type":"response.done", "response":{"status":"completed",
                        "output":[{"type":"message","content":[{"type":"audio","transcript":"untrusted partial snapshot"}]}]}}),
                    _ => json!({"type":"response.audio.delta", "delta":"AAAA"}),
                };
                send(&mut socket, event).await;
                closed(&mut socket).await;
            }).await;
        let provider = OpenAIRealtime::new(
            configuration(&endpoint, provider_id, RealtimeAuthMode::Bearer),
            Arc::new(crate::TokioTaskSpawner),
        );
        let active = provider
            .connect("fixture", Arc::new(Capture::default()))
            .await
            .unwrap();
        active.consume_pcm_chunk(&pcm(1000, 1600));
        assert!(
            timeout(DEADLINE, active.finish("fixture"))
                .await
                .unwrap()
                .is_err(),
            "{case}"
        );
        timeout(DEADLINE, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn all_auth_modes_reach_the_socket_and_header_only_allows_an_empty_main_key() {
    for (provider_id, auth) in [
        (PROVIDER_ID, RealtimeAuthMode::Bearer),
        (CURRENT, RealtimeAuthMode::ApiKey),
        (LEGACY, RealtimeAuthMode::Headers),
    ] {
        let model = if provider_id == PROVIDER_ID {
            DEFAULT_MODEL
        } else {
            QWEN_MODEL
        };
        let (endpoint, server) = serve(
            "/custom/auth-socket",
            model,
            auth,
            |mut socket| async move {
                created(&mut socket).await;
                send(&mut socket, json!({"type":"session.updated"})).await;
                closed(&mut socket).await;
            },
        )
        .await;
        let mut cfg = configuration(&endpoint, provider_id, auth);
        if auth != RealtimeAuthMode::Headers {
            cfg.extra_headers
                .insert("Authorization".into(), "Basic synthetic-ignored".into());
            cfg.extra_headers
                .insert("api-key".into(), "synthetic-ignored".into());
        } else {
            assert!(cfg.api_key.is_empty());
        }
        OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner))
            .validate_connection()
            .await
            .unwrap();
        timeout(DEADLINE, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn framing_and_handshake_headers_are_rejected_before_any_connection() {
    for header in [
        "Host",
        "Connection",
        "Upgrade",
        "Sec-WebSocket-Key",
        "Sec-WebSocket-Protocol",
        "Content-Length",
        "Transfer-Encoding",
        "Proxy-Authorization",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/custom/auth", listener.local_addr().unwrap());
        let mut cfg = configuration(&endpoint, CURRENT, RealtimeAuthMode::Headers);
        cfg.extra_headers
            .insert(header.into(), "synthetic-override".into());
        let provider = OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner));
        let error = timeout(DEADLINE, provider.validate_connection())
            .await
            .expect("reserved headers must fail before network setup")
            .unwrap_err();
        assert!(!error.to_string().contains("synthetic-override"));
        assert!(
            timeout(Duration::from_millis(40), listener.accept())
                .await
                .is_err(),
            "{header}"
        );
    }
}

#[tokio::test]
async fn openai_none_schema_rejection_retries_low_once_before_audio_and_generates_once() {
    let (endpoint, server) = serve("/openai/custom", DEFAULT_MODEL, RealtimeAuthMode::Bearer,
        |mut socket| async move {
            let first = created(&mut socket).await;
            assert_eq!(first["session"]["reasoning"]["effort"], "none");
            send(&mut socket, json!({"type":"error", "error":{"code":"invalid_value",
                "type":"invalid_request_error", "param":"session.reasoning.effort", "message":"PRIVATE_ERROR_PAYLOAD"}})).await;
            let retry = receive(&mut socket).await;
            assert_eq!(retry["type"], "session.update");
            assert_eq!(retry["session"]["reasoning"]["effort"], "low");
            assert_eq!(retry["session"]["instructions"], first["session"]["instructions"]);
            assert!(timeout(Duration::from_millis(40), socket.next()).await.is_err());
            send(&mut socket, json!({"type":"session.updated"})).await;
            let events = until_response(&mut socket).await;
            assert_eq!(audio_bytes(&events), pcm(1234, (1607_usize * 3).div_ceil(2)));
            assert_eq!(events.iter().filter(|event| event["type"] == "input_audio_buffer.commit").count(), 1);
            assert_eq!(events.iter().filter(|event| event["type"] == "response.create").count(), 1);
            completed(&mut socket, "一次完整生成。").await;
            closed(&mut socket).await;
        }).await;
    let mut cfg = configuration(&endpoint, PROVIDER_ID, RealtimeAuthMode::Bearer);
    cfg.realtime_options.reasoning_effort = RealtimeReasoningEffort::None;
    let provider = OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("fixture", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&pcm(1234, 1607));
    assert_eq!(
        timeout(DEADLINE, active.finish("fixture"))
            .await
            .unwrap()
            .unwrap()
            .text,
        "一次完整生成。"
    );
    timeout(DEADLINE, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn nonmatching_setup_errors_profiles_and_disabled_fallback_never_retry() {
    let cases = [
        (
            PROVIDER_ID,
            true,
            json!({"code":"invalid_api_key", "param":"reasoning.effort"}),
        ),
        (
            PROVIDER_ID,
            true,
            json!({"code":"insufficient_quota", "param":"reasoning.effort"}),
        ),
        (
            PROVIDER_ID,
            true,
            json!({"code":"invalid_value", "param":"session.audio.input.format"}),
        ),
        (
            PROVIDER_ID,
            true,
            json!({"code":"invalid_value", "param":"reasoning.effort", "type":"authentication_error"}),
        ),
        (
            PROVIDER_ID,
            false,
            json!({"code":"invalid_value", "param":"reasoning.effort"}),
        ),
        (
            CURRENT,
            true,
            json!({"code":"invalid_value", "param":"reasoning.effort"}),
        ),
        (PROVIDER_ID, true, Value::Null), // An actual socket close is not a schema rejection.
    ];
    for (provider_id, enabled, mut error) in cases {
        let disconnect = error.is_null();
        if !disconnect {
            error["message"] = json!("PRIVATE_ERROR_PAYLOAD");
        }
        let model = if provider_id == PROVIDER_ID {
            DEFAULT_MODEL
        } else {
            QWEN_MODEL
        };
        let (endpoint, server) = serve(
            "/custom/rejected",
            model,
            RealtimeAuthMode::Bearer,
            move |mut socket| async move {
                created(&mut socket).await;
                if disconnect {
                    socket.close(None).await.unwrap();
                } else {
                    send(&mut socket, json!({"type":"error", "error":error})).await;
                }
                closed(&mut socket).await;
            },
        )
        .await;
        let mut cfg = configuration(&endpoint, provider_id, RealtimeAuthMode::Bearer);
        cfg.realtime_options.reasoning_effort = RealtimeReasoningEffort::None;
        cfg.realtime_options.reasoning_fallback = enabled;
        let provider = OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner));
        let active = provider
            .connect("fixture", Arc::new(Capture::default()))
            .await
            .unwrap();
        active.consume_pcm_chunk(&pcm(1000, 1600));
        let error = timeout(DEADLINE, active.finish("fixture"))
            .await
            .unwrap()
            .unwrap_err();
        assert!(!error.to_string().contains("PRIVATE_ERROR_PAYLOAD"));
        timeout(DEADLINE, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn a_second_none_effort_rejection_fails_instead_of_retrying_or_uploading_audio() {
    let (endpoint, server) = serve("/openai/reject-twice", DEFAULT_MODEL, RealtimeAuthMode::Bearer,
        |mut socket| async move {
            let setup = created(&mut socket).await;
            assert_eq!(setup["session"]["reasoning"]["effort"], "none");
            let error = json!({"type":"error", "error":{"code":"unsupported_value", "param":"reasoning.effort"}});
            send(&mut socket, error.clone()).await;
            assert_eq!(receive(&mut socket).await["session"]["reasoning"]["effort"], "low");
            send(&mut socket, error).await;
            closed(&mut socket).await;
        }).await;
    let mut cfg = configuration(&endpoint, PROVIDER_ID, RealtimeAuthMode::Bearer);
    cfg.realtime_options.reasoning_effort = RealtimeReasoningEffort::None;
    let provider = OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("fixture", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&pcm(1000, 1600));
    assert!(timeout(DEADLINE, active.finish("fixture"))
        .await
        .unwrap()
        .is_err());
    timeout(DEADLINE, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn cancellation_during_none_retry_setup_drops_queued_pcm_and_closes_socket() {
    let (retry_sent, retry_seen) = oneshot::channel();
    let (endpoint, server) = serve("/openai/cancel-retry", DEFAULT_MODEL, RealtimeAuthMode::Bearer,
        move |mut socket| async move {
            created(&mut socket).await;
            send(&mut socket, json!({"type":"error", "error":{"code":"invalid_parameter", "param":"reasoning.effort"}})).await;
            assert_eq!(receive(&mut socket).await["session"]["reasoning"]["effort"], "low");
            retry_sent.send(()).unwrap();
            closed(&mut socket).await;
        }).await;
    let mut cfg = configuration(&endpoint, PROVIDER_ID, RealtimeAuthMode::Bearer);
    cfg.realtime_options.reasoning_effort = RealtimeReasoningEffort::None;
    let provider = OpenAIRealtime::new(cfg, Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("fixture", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&pcm(1000, 1600));
    timeout(DEADLINE, retry_seen).await.unwrap().unwrap();
    active.cancel();
    assert!(timeout(DEADLINE, active.finish("fixture"))
        .await
        .unwrap()
        .is_err());
    timeout(DEADLINE, server).await.unwrap().unwrap();
}
