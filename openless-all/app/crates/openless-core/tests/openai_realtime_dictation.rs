//! Native coordinator coverage: the recorder and Stop do not wait on socket setup,
//! and prepared text uses the existing script conversion, insertion and history path.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use openless_core::credentials::{
    CredentialNamespace, OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_MODEL_ACCOUNT,
};
use openless_core::shared_types::{ChineseScriptPreference, PipelineMode};
use openless_core::testing::{FixtureAudioRecorder, FixtureTextInserter};
use openless_core::{
    BackendConfig, BackendDependencies, BackendEventKind, CredentialKey, CredentialStore,
    DictationInsertStatus, DictationStartOptions, InMemoryCredentialStore, InsertOutcome,
    NoopSettingsRuntime, OpenLessBackend, SecretValue, SessionId, SettingsUpdateOptions,
    SharedOmniDictationEngine,
};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

const PROVIDER: &str = "openai-realtime";
const KEY: &str = "synthetic-native-realtime-key";
const MODEL: &str = "gpt-realtime-2.1";
const SPOKEN_RESULT: &str = "確認。\n\n1. 網絡 API 保持不變。\n2. 不要啟用同步。";
const SIMPLIFIED: &str = "确认。\n\n1. 网络 API 保持不变。\n2. 不要启用同步。";
type Socket = WebSocketStream<TcpStream>;

async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(8), future)
        .await
        .expect("native Realtime contract stalled")
}

async fn receive(socket: &mut Socket) -> Value {
    let message = within(socket.next())
        .await
        .expect("socket closed early")
        .unwrap();
    match message {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected JSON, got {other:?}"),
    }
}

async fn send(socket: &mut Socket, value: Value) {
    socket.send(Message::text(value.to_string())).await.unwrap();
}

async fn credentials(endpoint: &str) -> Arc<InMemoryCredentialStore> {
    let store = Arc::new(InMemoryCredentialStore::default());
    for (account, value) in [
        OMNI_API_KEY_ACCOUNT,
        OMNI_ENDPOINT_ACCOUNT,
        OMNI_MODEL_ACCOUNT,
    ]
    .into_iter()
    .zip([KEY, endpoint, MODEL])
    {
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Omni,
                    Some(PROVIDER.to_owned()),
                    account,
                )
                .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    store
}

#[tokio::test]
async fn native_openai_realtime_records_before_setup_and_reuses_shared_insertion_and_script_policy()
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (setup_sent, setup_seen) = oneshot::channel();
    let (allow_setup, setup_allowed) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = within(listener.accept()).await.unwrap();
        let mut socket = tokio_tungstenite::accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.headers()["authorization"], format!("Bearer {KEY}"));
                assert_eq!(request.uri().path(), "/v1/realtime");
                Ok(response)
            },
        )
        .await
        .unwrap();
        send(&mut socket, json!({"type":"session.created"})).await;
        let setup = receive(&mut socket).await;
        let prompt = setup["session"]["instructions"].as_str().unwrap();
        assert!(prompt.contains("native cursor fixture"));
        assert!(prompt.contains("<cursor_context>"));
        assert!(prompt.contains("OpenLess"));
        assert!(prompt.contains("简体"));
        assert!(prompt.contains("# Realtime audio dictation"));
        assert_eq!(setup["session"]["reasoning"]["effort"], "low");
        setup_sent.send(()).unwrap();
        setup_allowed.await.unwrap();
        send(&mut socket, json!({"type":"session.updated"})).await;
        let mut audio = Vec::new();
        let mut commits = 0;
        loop {
            let event = receive(&mut socket).await;
            match event["type"].as_str() {
                Some("input_audio_buffer.append") => audio.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(event["audio"].as_str().unwrap())
                        .unwrap(),
                ),
                Some("input_audio_buffer.commit") => commits += 1,
                Some("session.update") => assert!(event["session"]["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("native cursor fixture")),
                Some("response.create") => break,
                other => panic!("unexpected client event {other:?}"),
            }
        }
        assert_eq!(commits, 1);
        assert_eq!(audio, vec![42; 48_000]); // One complete second, converted 16 -> 24 kHz.
        send(
            &mut socket,
            json!({"type":"response.output_text.delta", "delta":SPOKEN_RESULT}),
        )
        .await;
        send(
            &mut socket,
            json!({"type":"response.done", "response":{"status":"completed", "output":[{
                "type":"message", "content":[{"type":"output_text", "text":SPOKEN_RESULT}]
            }]}}),
        )
        .await;
        match within(socket.next()).await {
            None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {}
            other => panic!("connection retained: {other:?}"),
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "a second polish call must not be made"
        );
    });
    let store = credentials(&endpoint).await;
    let data_dir =
        std::env::temp_dir().join(format!("openless-native-realtime-{}", SessionId::new()));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                dictation_engine: Arc::new(SharedOmniDictationEngine::new(
                    store.clone(),
                    Arc::new(FixtureAudioRecorder::new(
                        vec![vec![42; 32_000]],
                        Vec::new(),
                    )),
                )),
                credential_store: store,
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap(),
    );
    let mut prefs = backend.get_preferences();
    prefs.multimodal_pipeline_enabled = true;
    prefs.pipeline_mode = PipelineMode::Multimodal;
    prefs.active_omni_provider = PROVIDER.to_owned();
    prefs.chinese_script_preference = ChineseScriptPreference::Simplified;
    prefs.stable_transcription_enabled = false;
    prefs.cursor_context_enabled = true;
    backend
        .update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)
        .unwrap();
    backend.add_vocabulary("OpenLess".to_owned(), None).unwrap();
    backend.start().await.unwrap();
    let mut events = backend.subscribe();
    let session = within(backend.start_dictation_with_options(DictationStartOptions {
        front_app: Some("Fixture editor".into()),
        cursor_context: Some(openless_core::prompts::cursor_context_input(
            "native cursor fixture",
            "",
        )),
        ..DictationStartOptions::default()
    }))
    .await
    .unwrap();
    within(setup_seen).await.unwrap();
    let stopping_backend = backend.clone();
    let stopping =
        tokio::spawn(async move { stopping_backend.stop_dictation_session(session).await });
    assert!(!stopping.is_finished());
    allow_setup.send(()).unwrap();
    let result = within(stopping).await.unwrap().unwrap();
    assert_eq!(result.raw_text, SPOKEN_RESULT);
    assert_eq!(result.polished_text, SIMPLIFIED);
    assert_eq!(result.inserted, DictationInsertStatus::Inserted);
    assert_eq!(result.duration_ms, 1_000);
    let history = backend.list_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].final_text, SIMPLIFIED);
    assert!(history[0].error_code.is_none());
    let mut final_count = 0;
    while let Ok(event) = events.try_recv() {
        match event.kind {
            BackendEventKind::PolishDelta(delta) if delta.is_final => final_count += 1,
            BackendEventKind::TranscriptDelta(_) => {
                panic!("prepared text must not masquerade as ASR")
            }
            _ => {}
        }
    }
    assert_eq!(final_count, 1);
    within(server).await.unwrap();
    backend.shutdown().await.unwrap();
    // The test creates this unique temporary directory; no user profile is involved.
    std::fs::remove_dir_all(data_dir).unwrap();
}
