//! Production engine coverage for Gemini Live previews, finalization, and cancellation.

use std::future::{pending, Future};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{future::BoxFuture, SinkExt, StreamExt};
use openless_core::asr::gemini_live::MODEL;
use openless_core::credentials::{
    CredentialNamespace, ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT,
    OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_MODEL_ACCOUNT,
};
use openless_core::shared_types::PipelineMode;
use openless_core::testing::{FixtureAudioRecorder, FixtureTextInserter};
use openless_core::{
    ActiveRecording, AudioConsumer, AudioRecorder, BackendConfig, BackendDependencies,
    BackendError, BackendErrorCode, BackendEventKind, CredentialKey, CredentialStore,
    DictationAudioSource, DictationContext, DictationEngine, DictationInsertStatus, DictationPhase,
    EngineProgress, EngineProgressSink, InMemoryCredentialStore, InsertOutcome,
    NoopSettingsRuntime, OpenLessBackend, ProviderInvocation, RecordingProgressSink, SecretValue,
    SessionId, SettingsUpdateOptions, SharedCloudTranscriptionEngine, SharedOmniDictationEngine,
    TextStreamChunk, TextStreamSink, TranscriptDelta, TranscriptionEngine,
};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

const API_KEY: &str = "gemini-engine-test-key";
const ASR_CHANNEL: &str = "gemini-live-test-channel";
const DRAFT_ONE: &str = "Meet on Tuesday";
const DRAFT_TWO: &str = "Meet on Tuesday, sorry, Wednesday";
const VERBATIM: &str = "Meet on Tuesday, sorry, Wednesday.";
const SMART: &str = "Meet on Wednesday.";
type ServerSocket = WebSocketStream<TcpStream>;
type ProgressReceiver = mpsc::UnboundedReceiver<(SessionId, EngineProgress)>;

/// Bound asynchronous checks so a broken engine fails rather than hanging the suite.
async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("Gemini Live engine test stalled")
}

/// A complete 100 ms frame ensures the engine sends PCM before recording stops.
fn pcm() -> Vec<u8> {
    vec![42; 3_200]
}

/// Use channel-scoped credentials for Traditional and the independent Omni namespace.
async fn credentials(endpoint: &str, smart: bool) -> Arc<InMemoryCredentialStore> {
    let store = Arc::new(InMemoryCredentialStore::default());
    let (namespace, provider, accounts) = if smart {
        (
            CredentialNamespace::Omni,
            "gemini",
            [
                OMNI_API_KEY_ACCOUNT,
                OMNI_ENDPOINT_ACCOUNT,
                OMNI_MODEL_ACCOUNT,
            ],
        )
    } else {
        (
            CredentialNamespace::Asr,
            ASR_CHANNEL,
            [ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT],
        )
    };
    for (account, value) in accounts.into_iter().zip([API_KEY, endpoint, MODEL]) {
        store
            .write(
                CredentialKey::new(namespace, Some(provider.to_owned()), account).unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    store
}

/// Include structured hints and a style prompt that dedicated transcription must ignore.
fn context(smart: bool) -> Arc<DictationContext> {
    let mut context = DictationContext::default();
    context.audio_source = DictationAudioSource::External;
    context.asr = ProviderInvocation::new(ASR_CHANNEL, "gemini");
    context.asr.language = Some("en".to_owned());
    context.omni = ProviderInvocation::for_provider("gemini");
    context.polish.working_languages = vec!["en".to_owned()];
    context.polish.hotwords = vec!["OpenLess".to_owned()];
    context.polish.style_system_prompt = "A style prompt must not reach transcription".to_owned();
    if smart {
        context.pipeline_mode = PipelineMode::Multimodal;
    }
    Arc::new(context)
}

/// Bind a local endpoint; tests never require credentials or an external service.
async fn endpoint() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1beta", listener.local_addr().unwrap());
    (listener, endpoint)
}

/// Validate credentials using the handshake callback's fixed response signature.
struct CheckCredentials;

impl Callback for CheckCredentials {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert_eq!(request.headers()["x-goog-api-key"], API_KEY);
        assert!(request.uri().query().is_none());
        Ok(response)
    }
}

/// Check the engine-selected mode and credentials before granting setup completion.
async fn accept_setup(listener: &TcpListener, mode: &str) -> ServerSocket {
    let (stream, _) = within(listener.accept()).await.unwrap();
    let mut socket = within(tokio_tungstenite::accept_hdr_async(
        stream,
        CheckCredentials,
    ))
    .await
    .unwrap();
    let setup = receive(&mut socket).await;
    assert_eq!(setup["setup"]["model"], format!("models/{MODEL}"));
    let transcription = &setup["setup"]["inputAudioTranscription"];
    assert_eq!(transcription["mode"], mode);
    assert_eq!(transcription["languageCodes"], json!(["en"]));
    assert_eq!(transcription["customVocabulary"], json!(["OpenLess"]));
    assert!(setup["setup"].get("systemInstruction").is_none());
    socket
}

/// The real service uses binary JSON for setup acknowledgement.
async fn activate(socket: &mut ServerSocket) {
    socket
        .send(Message::binary(
            serde_json::to_vec(&json!({"setupComplete": {}})).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        receive(socket).await,
        json!({"realtimeInput": {"activityStart": {}}})
    );
}

/// Read a client protocol frame while preserving a bounded failure time.
async fn receive(socket: &mut ServerSocket) -> Value {
    let message = within(socket.next()).await.unwrap().unwrap();
    match message {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        Message::Binary(bytes) => serde_json::from_slice(&bytes).unwrap(),
        other => panic!("expected Gemini JSON, received {other:?}"),
    }
}

/// Assert the engine delivers canonical PCM, then revise a draft before stop is requested.
async fn preview(socket: &mut ServerSocket) {
    let audio = receive(socket).await;
    assert_eq!(
        audio["realtimeInput"]["audio"]["mimeType"],
        "audio/pcm;rate=16000"
    );
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(audio["realtimeInput"]["audio"]["data"].as_str().unwrap())
            .unwrap(),
        pcm()
    );
    for text in [DRAFT_ONE, DRAFT_TWO] {
        socket
            .send(Message::text(
                json!({"serverContent": {"interimInputTranscription": {"text": text}}}).to_string(),
            ))
            .await
            .unwrap();
    }
}

/// Authoritative output is withheld until the engine sends its stop boundary.
async fn finalize(socket: &mut ServerSocket, text: &str) {
    assert_eq!(
        receive(socket).await,
        json!({"realtimeInput": {"activityEnd": {}}})
    );
    socket
        .send(Message::binary(
            serde_json::to_vec(&json!({"serverContent": {
                "inputTranscription": {"text": text}, "generationComplete": true
            }}))
            .unwrap(),
        ))
        .await
        .unwrap();
}

/// Completion and cancellation must release the connection without another request.
async fn assert_closed(socket: &mut ServerSocket, listener: &TcpListener) {
    match within(socket.next()).await {
        None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {}
        other => panic!("engine retained or reused its Live connection: {other:?}"),
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "the engine opened a second connection instead of using the Live final result"
    );
}

/// Observe Traditional previews through the public transcription port.
struct TextEvents(mpsc::UnboundedSender<TextStreamChunk>);

impl TextStreamSink for TextEvents {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        self.0.send(chunk).unwrap();
        Ok(())
    }
}

/// Observe Multimodal progress through the same port consumed by host UIs.
struct ProgressEvents(mpsc::UnboundedSender<(SessionId, EngineProgress)>);

impl EngineProgressSink for ProgressEvents {
    fn publish(&self, session: SessionId, event: EngineProgress) -> Result<(), BackendError> {
        self.0.send((session, event)).unwrap();
        Ok(())
    }
}

/// Simulate a host disappearing at the transition from recording to finalization.
struct RejectFinalization(ProgressEvents);

impl EngineProgressSink for RejectFinalization {
    fn publish(&self, session: SessionId, event: EngineProgress) -> Result<(), BackendError> {
        if matches!(event, EngineProgress::Stage(_)) {
            return Err(BackendError::new(
                BackendErrorCode::Internal,
                "test progress delivery failed",
            ));
        }
        self.0.publish(session, event)
    }
}

/// Feed microphone PCM immediately, then hold stop before Live finalization can begin.
struct BlockingStopRecorder {
    stop_entered: Arc<Notify>,
}

impl AudioRecorder for BlockingStopRecorder {
    fn start(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        _: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        let stop_entered = Arc::clone(&self.stop_entered);
        Box::pin(async move {
            consumer.consume_pcm_chunk(&pcm());
            Ok(Box::new(BlockingStopRecording { stop_entered }) as Box<dyn ActiveRecording>)
        })
    }
}

/// The test drops this pending stop future while the outer Omni session still owns Live.
struct BlockingStopRecording {
    stop_entered: Arc<Notify>,
}

impl ActiveRecording for BlockingStopRecording {
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            self.stop_entered.notify_one();
            pending().await
        })
    }
}

/// Recording levels may accompany previews, but polished output must wait for stop.
async fn next_preview(events: &mut ProgressReceiver, session: SessionId) -> TranscriptDelta {
    within(async {
        loop {
            let (actual_session, event) = events.recv().await.unwrap();
            assert_eq!(actual_session, session);
            match event {
                EngineProgress::TranscriptDelta(delta) => return delta,
                EngineProgress::PolishDelta(_) => panic!("polish output arrived before stop"),
                _ => {}
            }
        }
    })
    .await
}

#[tokio::test]
async fn traditional_live_streams_replaceable_previews_then_returns_verbatim_final() {
    let (listener, endpoint) = endpoint().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "VERBATIM").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        finalize(&mut socket, VERBATIM).await;
        assert_closed(&mut socket, &listener).await;
    });
    let engine = SharedCloudTranscriptionEngine::new(credentials(&endpoint, false).await);
    let (sender, mut events) = mpsc::unbounded_channel();
    let session = within(engine.start(
        SessionId::new(),
        context(false),
        Arc::new(TextEvents(sender)),
    ))
    .await
    .unwrap();
    session.consume_pcm_chunk(&pcm());
    for text in [DRAFT_ONE, DRAFT_TWO] {
        assert_eq!(
            within(events.recv()).await.unwrap(),
            TextStreamChunk {
                text: text.to_owned(),
                offset: 0
            }
        );
    }
    let output = within(session.finish()).await.unwrap();
    assert_eq!(output.text, VERBATIM);
    assert_eq!(output.duration_ms, 100);
    let label = session.asr_call_label().unwrap();
    assert_eq!(label.provider, "gemini");
    assert_eq!(label.model.as_deref(), Some(MODEL));
    within(server).await.unwrap();
}

#[tokio::test]
async fn multimodal_live_previews_before_stop_and_uses_smart_final_without_a_second_call() {
    let (listener, endpoint) = endpoint().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        finalize(&mut socket, SMART).await;
        assert_closed(&mut socket, &listener).await;
    });
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(FixtureAudioRecorder::default()),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(ProgressEvents(sender));
    let session = SessionId::new();
    within(engine.start(session, context(true), progress.clone()))
        .await
        .unwrap();
    engine.feed_audio(session, &pcm()).unwrap();
    for text in [DRAFT_ONE, DRAFT_TWO] {
        let delta = next_preview(&mut events, session).await;
        assert_eq!(delta.text, text);
        assert_eq!(delta.offset, 0);
        assert!(!delta.is_final);
    }
    let result = within(engine.finish(session, progress)).await.unwrap();
    assert_eq!(result.polished_text, SMART);
    assert_eq!(result.raw_text, SMART);
    assert_eq!(result.duration_ms, 100);
    assert!(!result.polish_failed);
    let mut final_polishes = Vec::new();
    while let Ok((actual_session, event)) = events.try_recv() {
        assert_eq!(actual_session, session);
        if let EngineProgress::PolishDelta(delta) = event {
            final_polishes.push(delta);
        }
    }
    assert_eq!(final_polishes.len(), 1);
    assert_eq!(final_polishes[0].text, SMART);
    assert_eq!(final_polishes[0].offset, 0);
    assert!(final_polishes[0].is_final);
    within(server).await.unwrap();
}

/// Exercise the production phase checks, insertion, and history through both audio entrypoints.
async fn backend_finalizes_live_segments(
    segments: &'static [&'static str],
    completion_first: bool,
) {
    for microphone in [false, true] {
        let (listener, endpoint) = endpoint().await;
        let server = tokio::spawn(async move {
            let mut socket = accept_setup(&listener, "SMART").await;
            activate(&mut socket).await;
            preview(&mut socket).await;
            assert_eq!(
                receive(&mut socket).await,
                json!({"realtimeInput": {"activityEnd": {}}})
            );
            // A late hypothesis and changed final must remain valid while stop drains Live.
            if segments.len() > 1 {
                socket
                    .send(Message::text(
                        json!({"serverContent": {
                            "interimInputTranscription": {"text": "Can we meet on Tuesday"}
                        }})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
            if completion_first {
                socket
                    .send(Message::text(
                        json!({"serverContent": {
                            "generationComplete": true, "turnComplete": true
                        }})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
            for (index, text) in segments.iter().enumerate() {
                socket
                    .send(Message::text(
                        json!({"serverContent": {
                            "inputTranscription": {"text": text},
                            "generationComplete": !completion_first && index + 1 == segments.len()
                        }})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
            assert_closed(&mut socket, &listener).await;
        });
        let store = credentials(&endpoint, true).await;
        let data_dir =
            std::env::temp_dir().join(format!("openless-live-stop-{}", SessionId::new()));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                dictation_engine: Arc::new(SharedOmniDictationEngine::new(
                    store.clone(),
                    Arc::new(FixtureAudioRecorder::new(vec![pcm()], Vec::new())),
                )),
                credential_store: store,
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        let mut prefs = backend.get_preferences();
        prefs.multimodal_pipeline_enabled = true;
        prefs.pipeline_mode = PipelineMode::Multimodal;
        prefs.active_omni_provider = "gemini".to_owned();
        prefs.working_languages = vec!["en".to_owned()];
        prefs.stable_transcription_enabled = false;
        backend
            .update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)
            .unwrap();
        backend.add_vocabulary("OpenLess".to_owned(), None).unwrap();
        backend.start().await.unwrap();
        let mut events = backend.subscribe();
        let session = within(async {
            if microphone {
                backend.start_dictation().await
            } else {
                backend.start_external_dictation().await
            }
        })
        .await
        .unwrap();
        if !microphone {
            backend.feed_external_pcm(session, &pcm()).unwrap();
        }
        within(async {
            loop {
                if let BackendEventKind::TranscriptDelta(delta) = events.recv().await.unwrap().kind
                {
                    if delta.text == DRAFT_TWO {
                        break;
                    }
                }
            }
        })
        .await;
        let result = within(backend.stop_dictation_session(session))
            .await
            .unwrap();
        let expected = segments.join(" ");
        assert_eq!(result.raw_text, expected);
        assert_eq!(result.polished_text, expected);
        assert_eq!(result.inserted, DictationInsertStatus::Inserted);
        let mut polishing = false;
        let mut last_transcript = String::new();
        let mut finals = 0;
        let mut completed = false;
        while let Ok(event) = events.try_recv() {
            match event.kind {
                BackendEventKind::TranscriptDelta(delta) => {
                    assert!(!polishing, "Live previews must finish before Polishing");
                    last_transcript = delta.text;
                }
                BackendEventKind::DictationStateChanged(state)
                    if state.phase == DictationPhase::Polishing =>
                {
                    polishing = true;
                }
                BackendEventKind::PolishDelta(delta) => {
                    assert!(polishing);
                    assert!(delta.is_final);
                    assert_eq!(delta.text, expected);
                    finals += 1;
                }
                BackendEventKind::DictationCompleted(_) => completed = true,
                _ => {}
            }
        }
        assert_eq!(last_transcript, expected);
        assert_eq!(finals, 1);
        assert!(completed);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].raw_transcript, expected);
        assert_eq!(history[0].final_text, expected);
        assert!(history[0].error_code.is_none());
        within(server).await.unwrap();
        backend.shutdown().await.unwrap();
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn multimodal_backend_accepts_revised_final_after_stop() {
    backend_finalizes_live_segments(&[SMART], false).await;
}

#[tokio::test]
async fn multimodal_backend_preserves_multiple_final_segments_after_stop() {
    backend_finalizes_live_segments(
        &["Can we meet on Tuesday?", "Wednesday works better."],
        false,
    )
    .await;
}

#[tokio::test]
async fn multimodal_stable_mode_connects_after_stop_and_replays_pcm_for_smart_transcription() {
    let (listener, endpoint) = endpoint().await;
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(FixtureAudioRecorder::default()),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(ProgressEvents(sender));
    let session = SessionId::new();
    let mut stable_context = context(true);
    Arc::make_mut(&mut stable_context)
        .recording
        .transcribe_after_stop = true;
    within(engine.start(session, stable_context, progress.clone()))
        .await
        .unwrap();
    engine.feed_audio(session, &pcm()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "stable transcription must keep audio local until recording stops"
    );
    while let Ok((actual_session, event)) = events.try_recv() {
        assert_eq!(actual_session, session);
        assert!(matches!(event, EngineProgress::RecordingLevel { .. }));
    }
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        finalize(&mut socket, SMART).await;
        assert_closed(&mut socket, &listener).await;
    });
    let result = within(engine.finish(session, progress)).await.unwrap();
    assert_eq!(result.polished_text, SMART);
    assert_eq!(result.duration_ms, 100);
    while let Ok((_, event)) = events.try_recv() {
        assert!(!matches!(event, EngineProgress::TranscriptDelta(_)));
    }
    within(server).await.unwrap();
}

#[tokio::test]
async fn multimodal_cancel_interrupts_delayed_setup_and_removes_the_session() {
    let (listener, endpoint) = endpoint().await;
    let (setup_seen, setup_received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        setup_seen.send(()).unwrap();
        assert_closed(&mut socket, &listener).await;
    });
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(FixtureAudioRecorder::default()),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(ProgressEvents(sender));
    let session = SessionId::new();
    let starting = tokio::spawn(engine.start(session, context(true), progress.clone()));
    within(setup_received).await.unwrap();
    within(engine.cancel(session)).await.unwrap();
    let error = within(starting).await.unwrap().unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Cancelled);
    assert_eq!(
        engine.feed_audio(session, &pcm()).unwrap_err().code,
        BackendErrorCode::InvalidState
    );
    assert_eq!(
        within(engine.finish(session, progress))
            .await
            .unwrap_err()
            .error
            .code,
        BackendErrorCode::InvalidState
    );
    assert!(events.try_recv().is_err());
    within(server).await.unwrap();
}

#[tokio::test]
async fn multimodal_cancel_closes_an_active_stream_without_promoting_the_draft() {
    let (listener, endpoint) = endpoint().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        assert_closed(&mut socket, &listener).await;
    });
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(FixtureAudioRecorder::default()),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(ProgressEvents(sender));
    let session = SessionId::new();
    within(engine.start(session, context(true), progress.clone()))
        .await
        .unwrap();
    engine.feed_audio(session, &pcm()).unwrap();
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_ONE);
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_TWO);
    within(engine.cancel(session)).await.unwrap();
    assert_eq!(
        engine.feed_audio(session, &pcm()).unwrap_err().code,
        BackendErrorCode::InvalidState
    );
    assert_eq!(
        within(engine.finish(session, progress))
            .await
            .unwrap_err()
            .error
            .code,
        BackendErrorCode::InvalidState
    );
    within(server).await.unwrap();
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn multimodal_progress_failure_closes_live_connection_and_removes_the_session() {
    let (listener, endpoint) = endpoint().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        assert_closed(&mut socket, &listener).await;
    });
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(FixtureAudioRecorder::default()),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(RejectFinalization(ProgressEvents(sender)));
    let session = SessionId::new();
    within(engine.start(session, context(true), progress.clone()))
        .await
        .unwrap();
    engine.feed_audio(session, &pcm()).unwrap();
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_ONE);
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_TWO);
    let failure = within(engine.finish(session, progress)).await.unwrap_err();
    assert_eq!(failure.error.code, BackendErrorCode::Internal);
    assert_eq!(failure.error.message, "test progress delivery failed");
    assert_eq!(
        engine.feed_audio(session, &pcm()).unwrap_err().code,
        BackendErrorCode::InvalidState
    );
    within(server).await.unwrap();
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn dropping_multimodal_finish_during_microphone_stop_closes_live_and_removes_the_session() {
    let (listener, endpoint) = endpoint().await;
    let server = tokio::spawn(async move {
        let mut socket = accept_setup(&listener, "SMART").await;
        activate(&mut socket).await;
        preview(&mut socket).await;
        assert_closed(&mut socket, &listener).await;
    });
    let stop_entered = Arc::new(Notify::new());
    let engine = SharedOmniDictationEngine::new(
        credentials(&endpoint, true).await,
        Arc::new(BlockingStopRecorder {
            stop_entered: Arc::clone(&stop_entered),
        }),
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let progress = Arc::new(ProgressEvents(sender));
    let session = SessionId::new();
    let mut microphone_context = context(true);
    Arc::make_mut(&mut microphone_context).audio_source = DictationAudioSource::Microphone;
    within(engine.start(session, microphone_context, progress.clone()))
        .await
        .unwrap();
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_ONE);
    assert_eq!(next_preview(&mut events, session).await.text, DRAFT_TWO);

    let mut finishing = engine.finish(session, progress.clone());
    within(async {
        tokio::select! {
            _ = stop_entered.notified() => {}
            result = &mut finishing => panic!("finish bypassed the blocked microphone stop: {result:?}"),
        }
    })
    .await;
    drop(finishing);

    let failure = within(engine.finish(session, progress)).await.unwrap_err();
    assert_eq!(failure.error.code, BackendErrorCode::InvalidState);
    within(server).await.unwrap();
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn multimodal_backend_preserves_segments_when_completion_arrives_first() {
    backend_finalizes_live_segments(&["First.", "Second."], true).await;
}
