use super::{protocol, transport, GeminiLiveASR, GeminiLiveConfig, MODEL};
use crate::asr::{gemini::TranscriptionMode, TranscriptCapture};
use crate::ports::{AudioConsumer, TextStreamChunk, TextStreamSink};
use crate::TokioTaskSpawner;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

mod buffered_audio;
mod finalization;
mod lifecycle;
mod protocol_contract;
mod transport_contract;

type ServerSocket = WebSocketStream<TcpStream>;
const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const PENDING_WINDOW: Duration = Duration::from_millis(100);
const API_KEY: &str = "test-gemini-secret";

/// Fake-server credentials stay on loopback and never contact the Gemini service.
fn config(endpoint: &str) -> GeminiLiveConfig {
    GeminiLiveConfig {
        api_key: API_KEY.to_owned(),
        base_url: endpoint.to_owned(),
        languages: Vec::new(),
        hotwords: Vec::new(),
        mode: TranscriptionMode::Verbatim,
        extra_headers: HashMap::new(),
    }
}

/// Bound every server wait so a broken lifecycle fails instead of hanging the suite.
async fn serve<F, Fut>(handler: F) -> (String, JoinHandle<()>)
where
    F: FnOnce(ServerSocket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    serve_with_timeout(TEST_TIMEOUT, handler).await
}

/// Larger replay probes still have a fixed deadline independent of production timeouts.
async fn serve_with_timeout<F, Fut>(deadline: Duration, handler: F) -> (String, JoinHandle<()>)
where
    F: FnOnce(ServerSocket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1beta", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (stream, _) = timeout(TEST_TIMEOUT, listener.accept())
            .await
            .expect("client did not connect")
            .unwrap();
        let socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        timeout(deadline, handler(socket))
            .await
            .expect("fake Gemini session stalled");
    });
    (endpoint, task)
}

/// Start the public client through the same injected executor used by the hosts.
async fn connect(endpoint: &str, capture: Arc<dyn TextStreamSink>) -> Arc<GeminiLiveASR> {
    timeout(
        TEST_TIMEOUT,
        GeminiLiveASR::connect(config(endpoint), capture, Arc::new(TokioTaskSpawner)),
    )
    .await
    .expect("client setup stalled")
    .unwrap()
}

/// Read client JSON without using the production decoder to validate its wire contract.
async fn receive(socket: &mut ServerSocket) -> Value {
    let message = socket.next().await.expect("client closed early").unwrap();
    match message {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        Message::Binary(bytes) => serde_json::from_slice(&bytes).unwrap(),
        other => panic!("expected JSON, received {other:?}"),
    }
}

/// Google emits binary JSON, while some gateways emit text JSON.
async fn send(socket: &mut ServerSocket, value: Value, binary: bool) {
    let message = if binary {
        Message::binary(serde_json::to_vec(&value).unwrap())
    } else {
        Message::text(value.to_string())
    };
    socket.send(message).await.unwrap();
}

/// Complete setup before allowing the client to delimit the recording's activity.
async fn ready(socket: &mut ServerSocket) {
    let setup = receive(socket).await;
    assert_eq!(setup["setup"]["model"], format!("models/{MODEL}"));
    send(socket, json!({"setupComplete": {}}), true).await;
    assert_eq!(
        receive(socket).await,
        json!({"realtimeInput": {"activityStart": {}}})
    );
}

/// Return only raw PCM observed before the manual stop marker.
async fn receive_audio_until_end(socket: &mut ServerSocket) -> Vec<u8> {
    let mut audio = Vec::new();
    loop {
        let message = receive(socket).await;
        let input = &message["realtimeInput"];
        assert!(input.get("audioStreamEnd").is_none());
        if input.get("activityEnd").is_some() {
            return audio;
        }
        assert_eq!(input["audio"]["mimeType"], "audio/pcm;rate=16000");
        audio.extend(
            base64::engine::general_purpose::STANDARD
                .decode(input["audio"]["data"].as_str().unwrap())
                .unwrap(),
        );
    }
}

/// Accept a close handshake or released transport, but never further application traffic.
async fn assert_closed(socket: &mut ServerSocket) {
    match socket.next().await {
        None | Some(Ok(Message::Close(_))) | Some(Err(_)) => {}
        other => panic!("session remained active after termination: {other:?}"),
    }
}

/// A notification follows each captured snapshot, avoiding sleeps for protocol ordering.
struct NotifyingCapture {
    snapshots: TranscriptCapture,
    published: mpsc::UnboundedSender<()>,
}

impl TextStreamSink for NotifyingCapture {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), crate::BackendError> {
        self.snapshots.publish(chunk)?;
        let _ = self.published.send(());
        Ok(())
    }
}

fn notifying_capture() -> (Arc<NotifyingCapture>, mpsc::UnboundedReceiver<()>) {
    let (published, received) = mpsc::unbounded_channel();
    (
        Arc::new(NotifyingCapture {
            snapshots: TranscriptCapture::default(),
            published,
        }),
        received,
    )
}

async fn wait_for_snapshots(received: &mut mpsc::UnboundedReceiver<()>, count: usize) {
    for _ in 0..count {
        timeout(TEST_TIMEOUT, received.recv())
            .await
            .expect("transcript preview was not published")
            .expect("transcript publisher closed");
    }
}
