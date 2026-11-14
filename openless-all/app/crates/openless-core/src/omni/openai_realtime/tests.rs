use super::*;
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
const TEST_KEY: &str = "synthetic-realtime-key";

fn config(endpoint: &str) -> OmniConfig {
    OmniConfig {
        provider_id: PROVIDER_ID.into(),
        base_url: endpoint.into(),
        api_key: TEST_KEY.into(),
        model: DEFAULT_MODEL.into(),
        extra_headers: HashMap::new(),
        temperature: None,
        thinking_enabled: false,
    }
}

#[derive(Default)]
struct Capture(Mutex<Vec<TextStreamChunk>>);
impl TextStreamSink for Capture {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), crate::BackendError> {
        self.0.lock().push(chunk);
        Ok(())
    }
}

async fn serve<F, Fut>(handler: F) -> (String, JoinHandle<()>)
where
    F: FnOnce(ServerSocket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (stream, _) = timeout(DEADLINE, listener.accept()).await.unwrap().unwrap();
        let socket = tokio_tungstenite::accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.uri().path(), "/v1/realtime");
                assert_eq!(request.uri().query(), Some("model=gpt-realtime-2.1"));
                assert_eq!(
                    request.headers()["authorization"],
                    format!("Bearer {TEST_KEY}")
                );
                assert!(request.headers().get("openai-beta").is_none()); // GA, not Qwen/preview.
                Ok(response)
            },
        )
        .await
        .unwrap();
        timeout(DEADLINE, handler(socket))
            .await
            .expect("fake Realtime server stalled");
    });
    (endpoint, task)
}

async fn receive(socket: &mut ServerSocket) -> Value {
    let message = timeout(DEADLINE, socket.next())
        .await
        .unwrap()
        .expect("client closed early")
        .unwrap();
    match message {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected JSON, got {other:?}"),
    }
}

async fn send(socket: &mut ServerSocket, value: Value) {
    socket.send(Message::text(value.to_string())).await.unwrap();
}

async fn ready(socket: &mut ServerSocket) -> Value {
    send(
        socket,
        json!({"type":"session.created", "session":{"id":"synthetic-session"}}),
    )
    .await;
    let setup = receive(socket).await;
    assert_eq!(setup["type"], "session.update");
    assert_eq!(setup["session"]["type"], "realtime");
    assert_eq!(setup["session"]["output_modalities"], json!(["text"]));
    assert_eq!(
        setup["session"]["audio"]["input"]["format"],
        json!({"type":"audio/pcm", "rate":24000})
    );
    assert!(setup["session"]["audio"]["input"]["turn_detection"].is_null());
    assert_eq!(setup["session"]["reasoning"]["effort"], "low");
    assert!(setup["session"].get("temperature").is_none());
    send(socket, json!({"type":"session.updated"})).await;
    setup
}

async fn closed(socket: &mut ServerSocket) {
    match timeout(DEADLINE, socket.next()).await.unwrap() {
        None | Some(Ok(Message::Close(_))) | Some(Err(_)) => {}
        other => panic!("unexpected traffic after termination: {other:?}"),
    }
}

fn audio(value: i16, samples: usize) -> Vec<u8> {
    (0..samples).flat_map(|_| value.to_le_bytes()).collect()
}

async fn completed(socket: &mut ServerSocket, text: &str) {
    send(
        socket,
        json!({"type":"response.done", "response":{"status":"completed", "output":[{
            "type":"message", "content":[{"type":"output_text", "text":text}]
        }]}}),
    )
    .await;
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

#[test]
fn endpoint_uses_api_root_and_preserves_gateway_prefix_without_query_credentials() {
    for (root, expected) in [
        (
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/realtime?model=gpt-realtime-2.1",
        ),
        (
            "wss://example.com/gateway/v1/realtime?model=old&key=secret&routing=one",
            "https://example.com/gateway/v1/realtime?routing=one&model=gpt-realtime-2.1",
        ),
        (
            "https://example.com/gateway/v1/chat/completions",
            "https://example.com/gateway/v1/realtime?model=gpt-realtime-2.1",
        ),
    ] {
        assert_eq!(
            transport::endpoint(root, DEFAULT_MODEL).unwrap().as_str(),
            expected
        );
    }
    assert!(transport::endpoint("https://user:secret@example.com/v1", DEFAULT_MODEL).is_err());
    assert!(transport::endpoint("file:///tmp/realtime", DEFAULT_MODEL).is_err());
}

#[test]
fn resampler_preserves_pcm_across_callback_boundaries_and_flushes_exact_tail() {
    let input = (0..37i16)
        .flat_map(|value| (value * 300).to_le_bytes())
        .collect::<Vec<_>>();
    let mut whole = pcm::Resampler::default();
    let mut expected = whole.push(&input);
    expected.extend(whole.finish());
    assert_eq!(expected.len(), (37usize * 3).div_ceil(2) * 2);
    for size in [2, 6, 10, 34] {
        let mut state = pcm::Resampler::default();
        let mut actual = input
            .chunks(size)
            .flat_map(|chunk| state.push(chunk))
            .collect::<Vec<_>>();
        actual.extend(state.finish());
        assert_eq!(actual, expected);
    }
    let mut constant = pcm::Resampler::default();
    let mut output = constant.push(&audio(i16::MIN, 8));
    output.extend(constant.finish());
    assert_eq!(output, audio(i16::MIN, 12));
}

#[test]
fn setup_and_finalization_keep_ga_text_contract_and_hide_upstream_error_payloads() {
    let cfg = config("https://api.openai.com/v1");
    let setup = protocol::decode(protocol::setup(&cfg, "reference-only"))
        .unwrap()
        .unwrap();
    assert_eq!(setup["session"]["reasoning"]["effort"], "low");
    for model in ["gpt-realtime-2.1", "gpt-realtime-2.1-mini"] {
        let mut cfg = cfg.clone();
        cfg.model = model.into();
        cfg.thinking_enabled = true;
        let setup = protocol::decode(protocol::setup(&cfg, "context"))
            .unwrap()
            .unwrap();
        assert_eq!(setup["session"]["reasoning"]["effort"], "low");
    }
    assert!(protocol::final_text(&json!({"status":"incomplete"}), "partial").is_err());
    assert!(protocol::final_text(&json!({"status":"completed"}), "").is_err());
    let limited = protocol::final_text(
        &json!({"status":"incomplete", "status_details":{"reason":"max_output_tokens"}}),
        "partial",
    )
    .unwrap_err()
    .to_string();
    assert!(limited.contains("output limit"));
    assert!(protocol::final_text(&json!({"status":"completed", "output":[{"content":[{"type":"output_text","text":"different"}]}]}), "partial").is_err());
    let error =
        protocol::server_error(&json!({"code":"rate_limit_exceeded", "message":"PRIVATE_PAYLOAD"}))
            .to_string();
    assert!(!error.contains("PRIVATE_PAYLOAD"));
    assert!(error.contains("rate limit"));
}

#[tokio::test]
async fn validation_only_checks_the_real_session_and_does_not_create_response_or_audio() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        closed(&mut socket).await;
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    provider.validate_connection().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn pcm_is_sent_before_stop_and_finish_commits_tail_then_generates_exactly_once() {
    let (sent, observed) = oneshot::channel();
    let (endpoint, server) = serve(|mut socket| async move {
        let setup = ready(&mut socket).await;
        assert_eq!(setup["session"]["instructions"], "frozen cursor + terms");
        let first = receive(&mut socket).await;
        assert_eq!(first["type"], "input_audio_buffer.append");
        let mut wire = base64::engine::general_purpose::STANDARD
            .decode(first["audio"].as_str().unwrap())
            .unwrap();
        assert!(wire.len() > 3_200); // 24 kHz, not a mislabeled 16 kHz payload.
        sent.send(()).unwrap();
        let events = until_response(&mut socket).await;
        for event in &events {
            if event["type"] == "input_audio_buffer.append" {
                wire.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(event["audio"].as_str().unwrap())
                        .unwrap(),
                );
            }
        }
        assert_eq!(wire.len(), 4_800); // Exactly 100 ms at 24 kHz including the held tail.
        let end = &events[events.len() - 3..];
        assert_eq!(end[0]["type"], "session.update");
        assert_eq!(
            end[0]["session"]["instructions"],
            "final style + same cursor"
        );
        assert_eq!(end[1]["type"], "input_audio_buffer.commit");
        assert_eq!(end[2]["type"], "response.create");
        assert_eq!(end[2]["response"]["output_modalities"], json!(["text"]));
        send(
            &mut socket,
            json!({"type":"response.output_text.delta", "delta":"简体"}),
        )
        .await;
        send(
            &mut socket,
            json!({"type":"response.output_text.delta", "delta":" English 🙂"}),
        )
        .await;
        completed(&mut socket, "简体 English 🙂").await;
        closed(&mut socket).await;
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let capture = Arc::new(Capture::default());
    let active = provider
        .connect("frozen cursor + terms", capture.clone())
        .await
        .unwrap();
    active.consume_pcm_chunk(&audio(1234, 1_600));
    timeout(DEADLINE, observed).await.unwrap().unwrap(); // The server received audio before finish exists.
    let result = active.finish("final style + same cursor").await.unwrap();
    assert_eq!(result.text, "简体 English 🙂");
    assert_eq!(result.duration_ms, 100);
    assert_eq!(
        *capture.0.lock(),
        vec![
            TextStreamChunk {
                text: "简体".into(),
                offset: 0
            },
            TextStreamChunk {
                text: " English 🙂".into(),
                offset: 2
            }
        ]
    );
    server.await.unwrap();
}

#[tokio::test]
async fn incomplete_and_closed_responses_never_promote_partial_text_to_success() {
    for status in ["incomplete", "closed"] {
        let (endpoint, server) = serve(move |mut socket| async move {
            ready(&mut socket).await;
            until_response(&mut socket).await;
            send(
                &mut socket,
                json!({"type":"response.output_text.delta", "delta":"partial"}),
            )
            .await;
            if status == "closed" {
                socket.close(None).await.unwrap();
            } else {
                send(
                    &mut socket,
                    json!({"type":"response.done", "response":{"status":"incomplete"}}),
                )
                .await;
                closed(&mut socket).await;
            }
        })
        .await;
        let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
        let active = provider
            .connect("prompt", Arc::new(Capture::default()))
            .await
            .unwrap();
        active.consume_pcm_chunk(&audio(1, 1_600));
        assert!(active.finish("prompt").await.is_err());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_and_dropping_the_last_session_close_the_worker_socket() {
    for cancel in [true, false] {
        let (sent, observed) = oneshot::channel();
        let (endpoint, server) = serve(|mut socket| async move {
            ready(&mut socket).await;
            sent.send(()).unwrap();
            closed(&mut socket).await;
        })
        .await;
        let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
        let active = provider
            .connect("prompt", Arc::new(Capture::default()))
            .await
            .unwrap();
        timeout(DEADLINE, observed).await.unwrap().unwrap();
        if cancel {
            active.cancel();
        }
        drop(active);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn cancelled_finish_future_closes_worker_even_if_the_public_session_is_retained() {
    let (sent, observed) = oneshot::channel();
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        until_response(&mut socket).await;
        sent.send(()).unwrap();
        closed(&mut socket).await;
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("prompt", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&audio(1, 1_600));
    let retained = active.clone();
    let pending = tokio::spawn(async move { retained.finish("prompt").await });
    timeout(DEADLINE, observed).await.unwrap().unwrap();
    pending.abort();
    let _ = pending.await;
    server.await.unwrap();
    drop(active);
}

#[tokio::test]
async fn too_short_silent_oversized_and_odd_pcm_fail_without_response_generation() {
    for pcm in [
        vec![1],
        audio(1, 10),
        audio(0, 1_600),
        vec![1; MAX_AUDIO_BYTES + 2],
    ] {
        let (sent, observed) = oneshot::channel();
        let (endpoint, server) = serve(|mut socket| async move {
            ready(&mut socket).await;
            sent.send(()).unwrap();
            // Audio might already have been sent, but failure cannot create a response.
            loop {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => assert_eq!(
                        serde_json::from_str::<Value>(&text).unwrap()["type"],
                        "input_audio_buffer.append"
                    ),
                    None | Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                    _ => {}
                }
            }
        })
        .await;
        let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
        let active = provider
            .connect("prompt", Arc::new(Capture::default()))
            .await
            .unwrap();
        timeout(DEADLINE, observed).await.unwrap().unwrap();
        active.consume_pcm_chunk(&pcm);
        assert!(active.finish("prompt").await.is_err());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn one_second_recording_and_stop_are_buffered_before_a_slow_handshake_finishes() {
    let (setup_sent, setup_observed) = oneshot::channel();
    let (release, allowed) = oneshot::channel();
    let (endpoint, server) = serve(|mut socket| async move {
        send(&mut socket, json!({"type":"session.created"})).await;
        let setup = receive(&mut socket).await;
        assert_eq!(setup["type"], "session.update");
        setup_sent.send(()).unwrap();
        allowed.await.unwrap(); // Recording and Stop already happened before this ACK.
        send(&mut socket, json!({"type":"session.updated"})).await;
        let events = until_response(&mut socket).await;
        let wire = events
            .iter()
            .filter(|event| event["type"] == "input_audio_buffer.append")
            .flat_map(|event| {
                base64::engine::general_purpose::STANDARD
                    .decode(event["audio"].as_str().unwrap())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(wire, audio(1234, 24_000));
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "input_audio_buffer.commit")
                .count(),
            1
        );
        completed(&mut socket, "可以。").await;
        closed(&mut socket).await;
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let active = timeout(
        Duration::from_millis(100),
        provider.connect("frozen context", Arc::new(Capture::default())),
    )
    .await
    .expect("local audio queue must not wait for a network handshake")
    .unwrap();
    active.consume_pcm_chunk(&audio(1234, 16_000));
    let pending = tokio::spawn(async move { active.finish("frozen context").await });
    timeout(DEADLINE, setup_observed).await.unwrap().unwrap();
    assert!(!pending.is_finished());
    release.send(()).unwrap();
    let result = timeout(DEADLINE, pending).await.unwrap().unwrap().unwrap();
    assert_eq!(result.text, "可以。");
    assert_eq!(result.duration_ms, 1_000);
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_a_pending_session_setup_and_releases_queued_pcm() {
    let (sent, observed) = oneshot::channel();
    let (endpoint, server) = serve(|mut socket| async move {
        send(&mut socket, json!({"type":"session.created"})).await;
        assert_eq!(receive(&mut socket).await["type"], "session.update");
        sent.send(()).unwrap();
        closed(&mut socket).await;
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("prompt", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&audio(1, 16_000));
    timeout(DEADLINE, observed).await.unwrap().unwrap();
    active.cancel();
    assert!(active.finish("prompt").await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn setup_rejection_is_an_error_even_if_audio_and_stop_were_already_queued() {
    let (endpoint, server) = serve(|mut socket| async move {
        send(&mut socket, json!({"type":"session.created"})).await;
        assert_eq!(receive(&mut socket).await["type"], "session.update");
        send(&mut socket, json!({"type":"error", "error":{"code":"invalid_api_key", "message":"PRIVATE_CREDENTIAL_OR_TEXT"}})).await;
        closed(&mut socket).await;
    }).await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let active = provider
        .connect("prompt", Arc::new(Capture::default()))
        .await
        .unwrap();
    active.consume_pcm_chunk(&audio(1, 16_000));
    let error = active.finish("prompt").await.unwrap_err().to_string();
    assert!(error.contains("authentication"));
    assert!(!error.contains("PRIVATE_CREDENTIAL_OR_TEXT"));
    server.await.unwrap();
}

#[tokio::test]
async fn recorded_retry_preserves_user_text_beside_audio_and_text_only_requests_skip_commit() {
    for with_audio in [true, false] {
        let (endpoint, server) = serve(move |mut socket| async move {
            let setup = ready(&mut socket).await;
            assert_eq!(setup["session"]["instructions"], "prompt");
            assert!(!setup["session"]["instructions"]
                .as_str()
                .unwrap()
                .contains("# Application dictation state"));
            let item = receive(&mut socket).await;
            assert_eq!(item["type"], "conversation.item.create");
            assert_eq!(
                item["item"]["content"][0],
                json!({"type":"input_text", "text":"selected text or question"})
            );
            let events = until_response(&mut socket).await;
            for update in events
                .iter()
                .filter(|event| event["type"] == "session.update")
            {
                assert_eq!(update["session"]["instructions"], "prompt");
            }
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event["type"] == "input_audio_buffer.commit")
                    .count(),
                usize::from(with_audio)
            );
            assert_eq!(
                events
                    .iter()
                    .any(|event| event["type"] == "input_audio_buffer.append"),
                with_audio
            );
            completed(&mut socket, "prepared text").await;
            closed(&mut socket).await;
        })
        .await;
        let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
        let wav = crate::asr::wav::encode_wav_16k_mono(&vec![1i16; 1_600]);
        let result = provider
            .complete_streaming(
                "prompt",
                "selected text or question",
                with_audio.then_some(wav.as_slice()),
                |_| {},
                || false,
            )
            .await
            .unwrap();
        assert_eq!(result, "prepared text");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn continuous_deltas_do_not_starve_caller_cancellation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        assert_eq!(
            receive(&mut socket).await["type"],
            "conversation.item.create"
        );
        until_response(&mut socket).await;
        let (mut writer, mut reader) = socket.split();
        let delta =
            Message::text(json!({"type":"response.output_text.delta", "delta":"x"}).to_string());
        loop {
            tokio::select! {
                incoming = reader.next() => match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    _ => {},
                },
                result = writer.send(delta.clone()) => if result.is_err() { break; },
            }
        }
    })
    .await;
    let provider = OpenAIRealtime::new(config(&endpoint), Arc::new(crate::TokioTaskSpawner));
    let cancelled = AtomicBool::new(false);
    let result = timeout(
        DEADLINE,
        provider.complete_streaming(
            "prompt",
            "question",
            None,
            |_| {
                cancelled.store(true, Ordering::Release);
            },
            || cancelled.load(Ordering::Acquire),
        ),
    )
    .await
    .unwrap();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    server.await.unwrap();
}
