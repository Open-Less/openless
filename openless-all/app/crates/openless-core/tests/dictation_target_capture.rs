use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::app_style_routing::{AppStyleAssignment, InputApp};
use openless_core::testing::{
    FixtureAudioRecorder, FixtureDictationEngine, FixtureTranscriptionEngine, RecordingHostActions,
};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, CapturedTextTarget,
    CliDispatchOutcome, CliIntent, CredentialKey, CredentialNamespace, CredentialStore,
    CredentialsStatus, DictationContext, DictationHotkeyEdge, DictationInsertStatus,
    DictationStartOptions, HostContextAdapter, HostContextCapture, InMemoryCredentialStore,
    InsertOutcome, InsertWriteResult, OpenLessBackend, PipelineDictationEngine, ProviderSlot,
    SecretValue, SessionId, SharedCloudTextPolisher, StylePack, TextInserter, TextInsertionSession,
    UserPreferences,
};
use tokio::sync::Notify;


#[derive(Default)]
struct DelayedCredentials {
    store: InMemoryCredentialStore,
    waited: AtomicBool,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl CredentialStore for DelayedCredentials {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        self.store.status(preferences)
    }

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        self.store.read(key)
    }

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.store.write(key, value)
    }

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.store.remove(key)
    }

    fn active_provider(
        &self,
        _slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let wait = !self.waited.swap(true, Ordering::AcqRel);
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(async move {
            if wait {
                entered.notify_one();
                release.notified().await;
            }
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "fixture fallback",
            ))
        })
    }
}


struct Focus {
    current: &'static str,
    captured: Vec<&'static str>,
    context_captured: Vec<&'static str>,
    prepared: Vec<&'static str>,
    delivered: Vec<(&'static str, String)>,
    delivered_positions: Vec<(u64, u64)>,
    position: Option<(u64, u64)>,
}

struct FocusContextAdapter {
    focus: Arc<Mutex<Focus>>,
    target: &'static str,
}

impl HostContextAdapter for FocusContextAdapter {
    fn input_app(&self) -> Option<InputApp> {
        let bundle = match self.target {
            "VS Code" => "com.microsoft.VSCode",
            "WeChat" => "com.tencent.xinWeChat",
            "Mail" => "com.apple.mail",
            "Notes" => "com.apple.Notes",
            "ChatGPT" => "com.openai.codex",
            "Orca" => "com.stablyai.orca",
            "Custom" => "org.example.unknown",
            _ => return None,
        };
        InputApp::macos(bundle, Some(self.target))
    }

    fn input_location(&self) -> Option<String> {
        // A host-provided control label, tied to the captured target rather than current focus.
        (self.target == "VS Code").then(|| "代码编辑区（可信控件标签）".into())
    }

    fn capture(
        &self,
        _include_cursor: bool,
    ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
        self.focus
            .lock()
            .unwrap()
            .context_captured
            .push(self.target);
        let front_app = self.target.to_string();
        Box::pin(async move {
            Ok(HostContextCapture {
                front_app: Some(front_app),
                cursor_context: None,
            })
        })
    }
}

struct FocusInserter { focus: Arc<Mutex<Focus>> }
impl TextInserter for FocusInserter {
    fn defer_preparation_until_stop(&self) -> bool { true }
    fn capture_target(&self) -> Option<CapturedTextTarget> {
        let mut focus = self.focus.lock().unwrap();
        let target = focus.current;
        focus.captured.push(target);
        Some(CapturedTextTarget {
            inserter: Arc::new(Self { focus: self.focus.clone() }),
            host_context: Arc::new(FocusContextAdapter { focus: self.focus.clone(), target }),
        })
    }
    fn begin(&self, _: SessionId, context: Arc<DictationContext>)
        -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
        assert!(!context.insertion.streaming);
        assert!(!context.insertion.composition_input);
        let mut focus = self.focus.lock().unwrap();
        let current = focus.current;
        focus.prepared.push(current);
        let session: Arc<dyn TextInsertionSession> = Arc::new(FocusSession { focus: self.focus.clone() });
        Box::pin(async move { Ok(session) })
    }
}
struct FocusSession { focus: Arc<Mutex<Focus>> }
impl TextInsertionSession for FocusSession {
    fn write(&self, _: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        panic!("ordinary dictation must not stream into the target")
    }
    fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
        panic!("successful current-focus input must not use fallback")
    }
    fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let mut focus = self.focus.lock().unwrap();
        if let Some(position) = focus.position {
            focus.delivered_positions.push(position);
            focus.position = Some((position.0 + text.encode_utf16().count() as u64, 0));
        }
        let current = focus.current;
        focus.delivered.push((current, text));
        Box::pin(async { Ok(InsertOutcome::Inserted) })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> { Box::pin(async { Ok(()) }) }
}

struct Fixture {
    backend: Arc<OpenLessBackend>,
    credentials: Arc<DelayedCredentials>,
    focus: Arc<Mutex<Focus>>,
    directory: std::path::PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_engine(|_| Arc::new(FixtureDictationEngine::successful("spoken", "spoken")))
            .await
    }

    async fn with_engine(
        build_engine: impl FnOnce(Arc<DelayedCredentials>) -> Arc<dyn openless_core::DictationEngine>,
    ) -> Self {
        Self::with_position_engine(build_engine, false).await
    }

    async fn with_position_engine(
        build_engine: impl FnOnce(Arc<DelayedCredentials>) -> Arc<dyn openless_core::DictationEngine>,
        _enforce_position: bool,
    ) -> Self {
        let directory = std::env::temp_dir().join(format!("openless-target-{}", SessionId::new()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("input-app-catalog-cache.json"),
            include_str!("fixtures/app-catalog.json"),
        )
        .unwrap();
        let credentials = Arc::new(DelayedCredentials::default());
        let focus = Arc::new(Mutex::new(Focus {
            current: "A",
            captured: Vec::new(),
            context_captured: Vec::new(),
            prepared: Vec::new(),
            delivered: Vec::new(),
            delivered_positions: Vec::new(),
            position: Some((4, 0)),
        }));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        dependencies.host_actions = Arc::new(RecordingHostActions::default());
        dependencies.dictation_engine = build_engine(credentials.clone());
        dependencies.text_inserter = Arc::new(FocusInserter {
            focus: focus.clone(),
        });
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: directory.clone(),
                    ..BackendConfig::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        backend.start().await.unwrap();
        Self {
            backend,
            credentials,
            focus,
            directory,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn start(backend: Arc<OpenLessBackend>, route: u8) -> Result<SessionId, BackendError> {
    if route == 0 {
        return backend.start_dictation().await;
    }
    let outcome = if route == 1 {
        backend
            .dispatch_cli_intent(CliIntent::ToggleDictation)
            .await?
    } else {
        backend
            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 1,
                at: std::time::Instant::now(),
            })
            .await?
    };
    match outcome {
        CliDispatchOutcome::DictationStarted(session_id) => Ok(session_id),
        other => panic!("expected a started session, got {other:?}"),
    }
}

#[tokio::test]
async fn every_dictation_entry_freezes_prompt_app_but_delivers_to_current_focus() {
    for route in 0..3 {
        let fixture = Fixture::new().await;
        let starting = tokio::spawn(start(fixture.backend.clone(), route));
        fixture.credentials.entered.notified().await;
        fixture.focus.lock().unwrap().current = "B";
        fixture.credentials.release.notify_one();
        starting.await.unwrap().unwrap();
        fixture.backend.stop_dictation().await.unwrap();
        let focus = fixture.focus.lock().unwrap();
        assert_eq!(
            focus.delivered,
            vec![("B", "spoken".to_string())],
            "route {route}"
        );
        assert_eq!(focus.captured, vec!["A"]);
        assert_eq!(focus.context_captured, vec!["A"]);
        assert_eq!(focus.prepared, vec!["B"]);
    }
}

#[tokio::test]
async fn cancelled_context_capture_never_prepares_or_reuses_its_target() {
    let fixture = Fixture::new().await;
    let starting = tokio::spawn(start(fixture.backend.clone(), 0));
    fixture.credentials.entered.notified().await;
    fixture.backend.cancel_dictation(None).await.unwrap();
    fixture.focus.lock().unwrap().current = "B";
    fixture.credentials.release.notify_one();
    assert_eq!(
        starting.await.unwrap().unwrap_err().code,
        BackendErrorCode::Cancelled
    );
    assert!(fixture.focus.lock().unwrap().prepared.is_empty());
    fixture.backend.start_dictation().await.unwrap();
    fixture.backend.stop_dictation().await.unwrap();
    let focus = fixture.focus.lock().unwrap();
    assert_eq!(focus.captured, vec!["A", "B"]);
    assert_eq!(focus.context_captured, vec!["A", "B"]);
    assert_eq!(focus.prepared, vec!["B"]);
    assert_eq!(focus.delivered, vec![("B", "spoken".to_string())]);
}

#[tokio::test]
async fn no_insertion_skips_target_capture_and_native_preparation() {
    let fixture = Fixture::new().await;
    fixture.credentials.release.notify_one();
    fixture
        .backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            ..DictationStartOptions::default()
        })
        .await
        .unwrap();
    let result = fixture.backend.stop_dictation().await.unwrap();
    assert_eq!(result.inserted, DictationInsertStatus::NotRequested);
    let focus = fixture.focus.lock().unwrap();
    assert!(focus.captured.is_empty());
    assert!(focus.prepared.is_empty());
    assert!(focus.delivered.is_empty());
}

#[tokio::test]
async fn ordinary_delayed_result_follows_current_focus_and_caret() {
    use openless_core::{DictationEngine, EngineFailure, EngineProgressSink, EngineResult};

    struct DelayedResult {
        inner: FixtureDictationEngine,
        finishing: AtomicBool,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    impl DictationEngine for DelayedResult {
        fn start(&self, id: SessionId, context: Arc<DictationContext>, progress: Arc<dyn EngineProgressSink>)
            -> BoxFuture<'static, Result<(), BackendError>> {
            self.inner.start(id, context, progress)
        }
        fn stop_recording(&self, _: SessionId) -> BoxFuture<'static, Result<bool, BackendError>> {
            Box::pin(async { Ok(true) })
        }
        fn finish(&self, id: SessionId, progress: Arc<dyn EngineProgressSink>)
            -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            let first = !self.finishing.swap(true, Ordering::AcqRel);
            let entered = self.entered.clone();
            let release = self.release.clone();
            let finishing = self.inner.finish(id, progress);
            Box::pin(async move {
                if first { entered.notify_one(); release.notified().await; }
                finishing.await
            })
        }
        fn cancel(&self, id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> { self.inner.cancel(id) }
    }

    for (initial, moved, next_capture) in [
        (Some((4, 0)), Some((12, 0)), false),
        (Some((4, 0)), Some((12, 0)), true),
        (Some((4, 0)), Some((4, 0)), true),
        (None, None, false),
    ] {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let engine = Arc::new(DelayedResult {
            inner: FixtureDictationEngine::successful("spoken", "retained result"),
            finishing: AtomicBool::new(false), entered: entered.clone(), release: release.clone(),
        });
        let fixture = Fixture::with_position_engine(|_| engine, true).await;
        fixture.credentials.waited.store(true, Ordering::Release);
        fixture.focus.lock().unwrap().position = initial;
        let first = fixture.backend.start_dictation().await.unwrap();
        let stopping = tokio::spawn({ let backend = fixture.backend.clone(); async move { backend.stop_dictation_session(first).await } });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
        fixture.focus.lock().unwrap().position = moved;
        let second = if next_capture {
            Some(fixture.backend.start_dictation().await.unwrap())
        } else {
            None
        };
        release.notify_one();
        let result = stopping.await.unwrap().unwrap();
        assert_eq!(result.session_id, first);
        assert_eq!(fixture.focus.lock().unwrap().delivered.len(), 1);
        if let Some(second) = second {
            assert_eq!(fixture.backend.snapshot().dictation.session_id, Some(second));
            fixture.backend.stop_dictation_session(second).await.unwrap();
            assert_eq!(fixture.focus.lock().unwrap().delivered.len(), 2);
        }
        assert!(fixture.backend.list_history().unwrap().iter().all(|row| row.error_code.is_none()));
    }
}

#[tokio::test]
async fn overlapping_pipeline_preserves_audio_modes_and_confirmed_target_progress() {
    use openless_core::{
        ActiveRecording, AudioConsumer, AudioRecorder, PolishOutput, RecordingProgressSink,
        TextPolisher, TextStreamSink, TranscriptOutput, TranscriptionEngine, TranscriptionSession,
    };
    use std::sync::atomic::AtomicUsize;

    struct Recorder {
        owner: Arc<AtomicUsize>,
        next: AtomicUsize,
    }
    struct Recording {
        owner: Arc<AtomicUsize>,
        consumer: Arc<dyn AudioConsumer>,
        number: usize,
    }
    impl ActiveRecording for Recording {
        fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async move {
                self.consumer.consume_pcm_chunk(&[self.number as u8, 3]);
                assert_eq!(self.owner.swap(0, Ordering::SeqCst), self.number);
                Ok(())
            })
        }
    }
    impl AudioRecorder for Recorder {
        fn start(
            &self,
            _: SessionId,
            _: Arc<DictationContext>,
            consumer: Arc<dyn AudioConsumer>,
            _: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            let number = self.next.fetch_add(1, Ordering::SeqCst) + 1;
            assert_eq!(self.owner.swap(number, Ordering::SeqCst), 0);
            let owner = self.owner.clone();
            Box::pin(async move {
                consumer.consume_pcm_chunk(&[number as u8, 1]);
                consumer.consume_pcm_chunk(&[number as u8, 2]);
                Ok(Box::new(Recording {
                    owner,
                    consumer,
                    number,
                }) as Box<dyn ActiveRecording>)
            })
        }
    }

    type TranscriptSessions = Arc<Mutex<Vec<(SessionId, Arc<Transcript>)>>>;
    struct Transcriber {
        sessions: TranscriptSessions,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    struct Transcript {
        pcm: Mutex<Vec<u8>>,
        first: bool,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    impl AudioConsumer for Transcript {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().extend_from_slice(pcm);
        }
    }
    impl TranscriptionSession for Transcript {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let (first, entered, release) =
                (self.first, self.entered.clone(), self.release.clone());
            Box::pin(async move {
                if first {
                    entered.notify_one();
                    release.notified().await;
                }
                Ok(TranscriptOutput {
                    text: if first { "first" } else { "second" }.into(),
                    duration_ms: 3,
                })
            })
        }
        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }
    impl TranscriptionEngine for Transcriber {
        fn start(
            &self,
            id: SessionId,
            _: Arc<DictationContext>,
            _: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let mut sessions = self.sessions.lock().unwrap();
            let transcript = Arc::new(Transcript {
                pcm: Mutex::new(Vec::new()),
                first: sessions.is_empty(),
                entered: self.entered.clone(),
                release: self.release.clone(),
            });
            sessions.push((id, transcript.clone()));
            Box::pin(async { Ok(transcript as Arc<dyn TranscriptionSession>) })
        }
    }
    struct Polisher {
        calls: Arc<Mutex<Vec<(SessionId, bool, String)>>>,
    }
    impl TextPolisher for Polisher {
        fn polish(
            &self,
            id: SessionId,
            context: Arc<DictationContext>,
            raw: String,
            _: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
            let (system, _) = context.effective_polish_prompts(&raw);
            assert!(system.starts_with("当前应用：VS Code"));
            assert_eq!(system.matches("当前应用：").count(), 1);
            assert!(
                system.find("本次输入场景：").unwrap()
                    < system.find("PIPELINE_STYLE_MARKER").unwrap()
            );
            self.calls
                .lock()
                .unwrap()
                .push((id, context.polish.translation_active, raw.clone()));
            let prefix = if context.polish.translation_active {
                "translated"
            } else {
                "polished"
            };
            Box::pin(async move { Ok(PolishOutput::text(format!("{prefix}-{raw}"))) })
        }
        fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }

    // These are controlled audio and target ports around the production Pipeline,
    // not physical microphone or AX/UIA acceptance. Every case traverses Core start/stop.
    for move_second in [false, true] {
        let owner = Arc::new(AtomicUsize::new(0));
        let sessions = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let fixture = Fixture::with_position_engine(
            |_| {
                Arc::new(openless_core::DictationEngineRouter::new(Arc::new(
                    PipelineDictationEngine::new(
                        Arc::new(Recorder {
                            owner: owner.clone(),
                            next: AtomicUsize::new(0),
                        }),
                        Arc::new(Transcriber {
                            sessions: sessions.clone(),
                            entered: entered.clone(),
                            release: release.clone(),
                        }),
                        Arc::new(Polisher {
                            calls: calls.clone(),
                        }),
                    ),
                )))
            },
            true,
        )
        .await;
        fixture.credentials.waited.store(true, Ordering::Release);
        fixture
            .credentials
            .write(
                CredentialKey::new(CredentialNamespace::Llm, Some("ark".into()), "ark.api_key")
                    .unwrap(),
                SecretValue::new("fixture-key"),
            )
            .await
            .unwrap();
        fixture.focus.lock().unwrap().current = "VS Code";
        let backend = fixture.backend.clone();
        let mut prefs = backend.get_preferences();
        prefs.multimodal_pipeline_enabled = false;
        prefs.active_llm_provider = "ark".into();
        prefs.streaming_insert = true;
        prefs.hotkey.mode = openless_core::shared_types::HotkeyMode::Auto;
        prefs.dictation_hotkey.primary = "Fn".into();
        prefs.polish_skip_max_chars = 0;
        prefs.cursor_context_enabled = true;
        prefs.translation_target_language = "English".into();
        prefs.working_languages = vec!["zh".into()];
        backend
            .update_settings(
                prefs,
                openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                &openless_core::NoopSettingsRuntime,
            )
            .unwrap();
        let style = backend
            .create_style_pack(StylePack {
                name: "PIPELINE-STYLE".into(),
                prompt: "PIPELINE_STYLE_MARKER".into(),
                ..StylePack::default()
            })
            .unwrap();
        let options = DictationStartOptions {
            style_pack_id: Some(style.id),
            insert_text: true,
            ..Default::default()
        };

        let epoch = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let first = match backend.dispatch_dictation_hotkey_edge_with_options(
            DictationHotkeyEdge::Pressed { press_id: 101, at: epoch }, options.clone()).await.unwrap() {
                CliDispatchOutcome::DictationStarted(id) => id, other => panic!("{other:?}"),
            };
        backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {
            press_id: 101, at: epoch + std::time::Duration::from_millis(40),
        }).await.unwrap();
        backend
            .toggle_dictation_translation_for_session(first)
            .unwrap();
        backend
            .toggle_dictation_translation_for_session(first)
            .unwrap();
        let stopping = tokio::spawn({
            let backend = backend.clone();
            async move { backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed { press_id: 102, at: epoch + std::time::Duration::from_secs(1) }).await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        assert_eq!(owner.load(Ordering::SeqCst), 0);
        backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released { press_id: 102, at: epoch + std::time::Duration::from_millis(1040) }).await.unwrap();
        let second = match backend.dispatch_dictation_hotkey_edge_with_options(
            DictationHotkeyEdge::Pressed { press_id: 103, at: epoch + std::time::Duration::from_millis(1100) }, options).await.unwrap() {
                CliDispatchOutcome::DictationStarted(id) => id, other => panic!("rapid successor was lost: {other:?}"),
            };
        backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released { press_id: 103, at: epoch + std::time::Duration::from_millis(1140) }).await.unwrap();
        backend
            .toggle_dictation_translation_for_session(second)
            .unwrap();
        assert!(backend
            .toggle_dictation_translation_for_session(first)
            .is_err());
        assert_eq!(owner.load(Ordering::SeqCst), 2);
        assert_eq!(
            *sessions.lock().unwrap()[1].1.pcm.lock().unwrap(),
            [2, 1, 2, 2]
        );
        release.notify_one();
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(3), stopping)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first_result, CliDispatchOutcome::DictationCompleted(ref result) if result.inserted == DictationInsertStatus::Inserted));
        assert_eq!(
            fixture.focus.lock().unwrap().delivered,
            [("VS Code", "polished-first".into())]
        );
        assert_eq!(fixture.focus.lock().unwrap().delivered_positions, [(4, 0)]);
        assert_eq!(backend.snapshot().dictation.session_id, Some(second));
        assert_eq!(
            backend.snapshot().dictation.phase,
            openless_core::DictationPhase::Recording
        );
        assert!(backend.snapshot().dictation.translation_active);
        assert_eq!(
            owner.load(Ordering::SeqCst),
            2,
            "finishing A must not stop B's microphone"
        );

        if move_second { fixture.focus.lock().unwrap().position = Some((88, 0)); }
        assert_eq!(backend.stop_dictation_session(second).await.unwrap().inserted, DictationInsertStatus::Inserted);
        assert_eq!(fixture.focus.lock().unwrap().delivered, [
            ("VS Code", "polished-first".into()), ("VS Code", "translated-second".into())
        ]);
        assert_eq!(owner.load(Ordering::SeqCst), 0);
        {
            let sessions = sessions.lock().unwrap();
            assert_eq!(
                sessions.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                [first, second]
            );
            assert_eq!(*sessions[0].1.pcm.lock().unwrap(), [1, 1, 1, 2, 1, 3]);
            assert_eq!(*sessions[1].1.pcm.lock().unwrap(), [2, 1, 2, 2, 2, 3]);
        }
        assert_eq!(
            *calls.lock().unwrap(),
            [
                (first, false, "first".into()),
                (second, true, "second".into())
            ]
        );
        let history = backend.list_history().unwrap();
        assert!(history.iter().any(|row| row.id == first.to_string()
            && row.final_text == "polished-first"
            && !row.translation_active));
        assert!(history.iter().any(|row| row.id == second.to_string()
            && row.raw_transcript == "second"
            && row.final_text == "translated-second"
            && row.translation_active));
        backend.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn routed_dictation_keeps_the_frozen_pack_and_delivers_to_current_focus() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sent, mut payloads) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut bytes = vec![0; length];
            socket.read_exact(&mut bytes).await.unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let system = payload["messages"][0]["content"].as_str().unwrap();
            let output = if system.contains("用户有任何输入后，输出“一一一”。") {
                "一一一"
            } else {
                "polished"
            };
            let output = if system.contains(openless_core::POLISH_TRANSLATE_TGT_MARKER) {
                format!("{}\n{output}\n{}\n{output}", openless_core::POLISH_TRANSLATE_SRC_MARKER, openless_core::POLISH_TRANSLATE_TGT_MARKER)
            } else { output.to_owned() };
            let (mime, response) = if payload["stream"] == true {
                ("text/event-stream", format!("data: {}\n\ndata: [DONE]\n\n", serde_json::json!({"choices":[{"delta":{"content":output},"finish_reason":null}]})))
            } else {
                (
                    "application/json",
                    serde_json::json!({"choices":[{"message":{"content":output}}]}).to_string(),
                )
            };
            sent.send(payload).unwrap();
            let reply = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n{response}", response.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    let fixture = Fixture::with_engine(|credentials| {
        Arc::new(PipelineDictationEngine::new(
            Arc::new(FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])),
            Arc::new(FixtureTranscriptionEngine::successful(
                "spoken test transcript",
                500,
            )),
            Arc::new(SharedCloudTextPolisher::new(credentials)),
        ))
    })
    .await;
    let backend = &fixture.backend;
    for (account, secret) in [("ark.api_key", "fixture-key"), ("ark.endpoint", &endpoint)] {
        fixture
            .credentials
            .write(
                CredentialKey::new(CredentialNamespace::Llm, Some("ark".into()), account).unwrap(),
                SecretValue::new(secret),
            )
            .await
            .unwrap();
    }
    let mut preferences = backend.get_preferences();
    preferences.multimodal_pipeline_enabled = false;
    preferences.active_llm_provider = "ark".into();
    preferences.polish_skip_max_chars = 0;
    preferences.streaming_insert = false;
    preferences.cursor_context_enabled = true;
    preferences.translation_target_language = "English".into();
    preferences.working_languages = vec!["zh".into()];
    backend
        .update_settings(
            preferences,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &openless_core::NoopSettingsRuntime,
        )
        .unwrap();

    let make_pack = |name: &str| {
        backend
            .create_style_pack(StylePack {
                name: name.into(),
                prompt: format!("Apply the unique {name} marker."),
                ..StylePack::default()
            })
            .unwrap()
    };
    let current = make_pack("MANUAL-STYLE");
    backend.activate_style_pack(&current.id).unwrap();
    let coding = make_pack("CODING-STYLE");
    let chat = make_pack("CHAT-STYLE");
    let mail = make_pack("MAIL-STYLE");
    for (category, pack) in [
        ("coding".to_owned(), &coding),
        ("chat".to_owned(), &chat),
        ("email".to_owned(), &mail),
    ] {
        backend
            .set_app_style_assignment(AppStyleAssignment::Category {
                category,
                style_pack_id: Some(pack.id.clone()),
            })
            .unwrap();
    }
    let expect_request = |payload: serde_json::Value, marker: &str| {
        let system = payload["messages"][0]["content"].as_str().unwrap();
        assert!(system.starts_with("当前应用："));
        assert_eq!(system.matches("当前应用：").count(), 1);
        assert!(!system.contains("# 输入场景参考"));
        assert!(system.find("本次输入场景：").unwrap() < system.find(marker).unwrap());
        assert!(payload["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains("spoken test transcript"));
        assert_eq!(system.matches(openless_core::DICTATION_REVISION_RULES).count(), 1);
        assert!(system.find(marker).unwrap() < system.find(openless_core::DICTATION_REVISION_RULES).unwrap());
        if marker == "CODING-STYLE" {
            assert!(payload["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("fixture coding context"));
        }
        assert!(
            payload["messages"][0]["content"]
                .as_str()
                .unwrap_or_default()
                .contains(marker),
            "{payload}"
        );
    };

    fixture.focus.lock().unwrap().current = "VS Code";
    let starting = tokio::spawn(start(backend.clone(), 2));
    fixture.credentials.entered.notified().await;
    fixture.focus.lock().unwrap().current = "WeChat";
    backend
        .set_app_style_assignment(AppStyleAssignment::Category {
            category: "coding".to_owned(),
            style_pack_id: None,
        })
        .unwrap();
    fixture.credentials.release.notify_one();
    starting.await.unwrap().unwrap();
    let result = backend.stop_dictation().await.unwrap();
    assert_eq!(result.polished_text, "polished");
    let first_payload = payloads.try_recv().unwrap();
    let first_system = first_payload["messages"][0]["content"].as_str().unwrap();
    assert!(first_system.starts_with("当前应用：VS Code\n应用标识：macos:com.microsoft.VSCode\n应用分类：编程"));
    assert!(first_system.contains("应用主要用途：fixture editor context"));
    assert!(first_system.contains("当前输入位置：代码编辑区（可信控件标签）"));
    expect_request(first_payload, "CODING-STYLE");
    assert_eq!(fixture.focus.lock().unwrap().delivered[0].0, "WeChat");
    assert_eq!(
        backend.app_style_routing().unwrap().apps[0].app.name,
        "VS Code"
    );

    let override_app = AppStyleAssignment::App {
        app_id: "macos:com.microsoft.VSCode".into(),
        style_pack_id: Some(chat.id.clone()),
    };
    backend.set_app_style_assignment(override_app).unwrap();
    backend
        .set_app_style_assignment(AppStyleAssignment::Category {
            category: "coding".to_owned(),
            style_pack_id: Some(coding.id.clone()),
        })
        .unwrap();

    // Explicit caller context must not activate the host app/category assignments.
    fixture.focus.lock().unwrap().current = "VS Code";
    for (style_pack_id, expected_pack, marker) in [
        (None, current.id.as_str(), "MANUAL-STYLE"),
        (Some(mail.id.clone()), mail.id.as_str(), "MAIL-STYLE"),
    ] {
        backend.start_dictation_with_options(DictationStartOptions {
            front_app: Some("Caller supplied context".into()),
            style_pack_id,
            ..Default::default()
        }).await.unwrap();
        let result = backend.stop_dictation().await.unwrap();
        let payload = payloads.try_recv().unwrap();
        let system = payload["messages"][0]["content"].as_str().unwrap();
        assert!(system.starts_with("当前应用：VS Code\n应用标识：macos:com.microsoft.VSCode\n应用分类：编程"));
        assert!(system.contains("当前输入位置：代码编辑区（可信控件标签）"));
        assert!(!system.contains("CHAT-STYLE"));
        assert!(!system.contains("CODING-STYLE"));
        expect_request(payload, marker);
        let history = backend.list_history().unwrap();
        let entry = history.iter().find(|entry| entry.id == result.session_id.to_string()).unwrap();
        assert_eq!(entry.style_pack_id.as_deref(), Some(expected_pack));
    }

    for (app, marker) in [
        ("WeChat", "CHAT-STYLE"),
        ("Mail", "MAIL-STYLE"),
        ("VS Code", "CHAT-STYLE"),
        ("Custom", "MANUAL-STYLE"),
        ("No stable identifier", "MANUAL-STYLE"),
    ] {
        fixture.focus.lock().unwrap().current = app;
        backend.start_dictation().await.unwrap();
        assert_eq!(
            backend.stop_dictation().await.unwrap().polished_text,
            "polished"
        );
        let payload = payloads.try_recv().unwrap();
        let system = payload["messages"][0]["content"].as_str().unwrap();
        if app == "Custom" {
            assert!(system.starts_with("当前应用：Custom\n应用标识：macos:org.example.unknown\n应用分类：未知\n应用主要用途：未知"));
        }
        if app == "No stable identifier" {
            assert!(system.starts_with("当前应用：未知\n应用标识：未知"));
        }
        if app != "VS Code" {
            assert!(system.contains("当前输入位置：应用内输入，无法正确确定"));
        }
        expect_request(payload, marker);
        assert_eq!(
            fixture.focus.lock().unwrap().delivered.last().unwrap().0,
            app
        );
    }

    // User membership changes affect the next capture, not an in-flight snapshot.
    fixture.focus.lock().unwrap().current = "VS Code";
    backend.set_app_style_assignment(AppStyleAssignment::Membership {
        member_id: "catalog:vscode".into(), category: Some("email".into()),
    }).unwrap();
    backend.start_dictation_with_options(DictationStartOptions {
        style_pack_id: Some(current.id.clone()), ..Default::default()
    }).await.unwrap();
    backend.set_app_style_assignment(AppStyleAssignment::Membership {
        member_id: "catalog:vscode".into(), category: Some("coding".into()),
    }).unwrap();
    backend.stop_dictation().await.unwrap();
    let payload = payloads.try_recv().unwrap();
    assert!(payload["messages"][0]["content"].as_str().unwrap().contains("应用分类：邮件"));
    expect_request(payload, "MANUAL-STYLE");

    let mut preferences = backend.get_preferences();
    preferences.cursor_context_enabled = false;
    backend.update_settings(preferences, openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT, &openless_core::NoopSettingsRuntime).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    let payload = payloads.try_recv().unwrap();
    let system = payload["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with("当前应用：VS Code"));
    assert!(system.contains("应用分类：编程"));
    assert!(system.contains("当前输入位置：应用内输入，无法正确确定"));
    expect_request(payload, "CHAT-STYLE");

    // A newly observed agent client inherits its category on its first input.
    for app in ["ChatGPT", "Orca"] {
        fixture.focus.lock().unwrap().current = app;
        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();
        expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");
        assert_eq!(
            backend
                .app_style_routing()
                .unwrap()
                .apps
                .iter()
                .find(|entry| entry.app.name == app)
                .unwrap()
                .category,
            Some("coding".to_owned())
        );
    }

    // Reproduce Notes with a different global selection, then rename/edit the
    // assigned pack. Assert the actual HTTP payload, delivered text, and history.
    fixture.focus.lock().unwrap().current = "Notes";
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "MANUAL-STYLE");
    let mut notes = make_pack("未命名");
    backend
        .set_app_style_assignment(AppStyleAssignment::App {
            app_id: "macos:com.apple.Notes".into(),
            style_pack_id: Some(notes.id.clone()),
        })
        .unwrap();
    notes.name = "一一一".into();
    notes.prompt = "用户有任何输入后，输出“一一一”。".into();
    backend.update_style_pack(notes.clone()).unwrap();
    let mut assigned_payload = serde_json::Value::Null;
    for _ in 0..3 {
        backend.start_dictation().await.unwrap();
        // Switching windows preserves the frozen style, while delivery follows current focus.
        fixture.focus.lock().unwrap().current = "WeChat";
        let result = backend.stop_dictation().await.unwrap();
        let payload = payloads.try_recv().unwrap();
        expect_request(payload.clone(), &notes.prompt);
        let system = payload["messages"][0]["content"].as_str().unwrap();
        assert!(!system.contains("MANUAL-STYLE"));
        assert!(system.contains("内容与形式由当前风格包定义"));
        assert!(!system.contains("请按这个应用的常见沟通风格调整语气"));
        assigned_payload = payload;
        assert_eq!(result.polished_text, "一一一");
        assert_eq!(
            fixture.focus.lock().unwrap().delivered.last().unwrap(),
            &("WeChat", "一一一".into())
        );
        let history = backend.list_history().unwrap();
        let entry = history
            .iter()
            .find(|entry| entry.id == result.session_id.to_string())
            .unwrap();
        assert_eq!(entry.style_pack_id.as_deref(), Some(notes.id.as_str()));
        assert_eq!(entry.app_bundle_id.as_deref(), Some("com.apple.Notes"));
        fixture.focus.lock().unwrap().current = "Notes";
    }
    backend
        .set_app_style_assignment(AppStyleAssignment::App {
            app_id: "macos:com.apple.Notes".into(),
            style_pack_id: None,
        })
        .unwrap();

    backend.activate_style_pack(&notes.id).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    let manual_payload = payloads.try_recv().unwrap();
    assert_eq!(assigned_payload["messages"], manual_payload["messages"]);
    backend.activate_style_pack(&current.id).unwrap();

    fixture.focus.lock().unwrap().current = "VS Code";
    backend
        .set_app_style_assignment(AppStyleAssignment::App {
            app_id: "macos:com.microsoft.VSCode".into(),
            style_pack_id: Some("builtin.raw".into()),
        })
        .unwrap();
    backend.start_dictation().await.unwrap();
    assert_eq!(
        backend.stop_dictation().await.unwrap().polished_text,
        "spoken test transcript"
    );
    assert!(
        payloads.try_recv().is_err(),
        "catalog context must preserve Raw passthrough"
    );
    backend
        .start_dictation_with_options(DictationStartOptions {
            translation_requested: true,
            ..Default::default()
        })
        .await
        .unwrap();
    backend.stop_dictation().await.unwrap();
    assert!(payloads.try_recv().unwrap()["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("fixture coding context"));
    while payloads.try_recv().is_ok() {}
    // A real opted-in profile must not turn canonical Raw into a polish request.
    let profile = backend.repositories().expression_profile.lock().unwrap().as_ref().unwrap().clone();
    for index in 0..10 {
        profile.record(uuid::Uuid::new_v4(), 100 + index % 4,
            openless_core::expression_observation::ExpressionSignal {
                line_breaks: 1, exclamations: 0, terminal_punctuation: 0,
            }).unwrap();
    }
    let frozen_expression = openless_core::expression_observation::prompt_instructions(
        &profile.stable_directions().unwrap()).unwrap();
    let mut preferences = backend.get_preferences();
    preferences.expression_learning_enabled = true;
    backend.update_settings(preferences, openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
        &openless_core::NoopSettingsRuntime).unwrap();
    backend.start_dictation_with_options(DictationStartOptions {
        translation_requested: true, ..Default::default()
    }).await.unwrap();
    backend.stop_dictation().await.unwrap();
    let translated = payloads.try_recv().unwrap();
    let translated_system = translated["messages"][0]["content"].as_str().unwrap();
    assert!(translated_system.starts_with("当前应用："));
    assert!(translated_system.contains(openless_core::POLISH_TRANSLATE_TGT_MARKER));
    assert!(!translated_system.contains(&frozen_expression));
    assert!(payloads.try_recv().is_err(), "translation must issue exactly one request");
    for live in [true, false] {
        backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: true,
                ..Default::default()
            })
            .await
            .unwrap();
        let result = if live {
            let id = backend.snapshot().dictation.session_id.unwrap();
            backend.toggle_dictation_translation_for_session(id).unwrap();
            backend.stop_dictation().await.unwrap()
        } else {
            backend
                .stop_dictation_with_options(openless_core::DictationStopOptions {
                    translation_requested: Some(false),
                    ..Default::default()
                })
                .await
                .unwrap()
        };
        assert_eq!(result.polished_text, "spoken test transcript");
        assert!(
            payloads.try_recv().is_err(),
            "unexpected request after cancellation, live={live}"
        );
        assert_eq!(
            fixture.focus.lock().unwrap().delivered.last().unwrap(),
            &("VS Code", "spoken test transcript".into())
        );
        let history = backend.list_history().unwrap();
        let entry = history.iter().find(|entry| entry.id == result.session_id.to_string()).unwrap();
        assert!(!entry.translation_active);
        assert_eq!(entry.style_pack_id.as_deref(), Some("builtin.raw"));
        assert_eq!(entry.final_text, "spoken test transcript");
    }
    backend.start_dictation().await.unwrap();
    assert_eq!(backend.stop_dictation().await.unwrap().polished_text, "spoken test transcript");
    assert!(payloads.try_recv().is_err(), "profile must not turn canonical Raw into a request");
    let custom_raw = backend
        .create_style_pack(StylePack {
            name: "CUSTOM-RAW".into(),
            base_mode: openless_core::PolishMode::Raw,
            prompt: "Apply CUSTOM-RAW.".into(),
            ..StylePack::default()
        })
        .unwrap();
    backend
        .set_app_style_assignment(AppStyleAssignment::App {
            app_id: "macos:com.microsoft.VSCode".into(),
            style_pack_id: Some(custom_raw.id),
        })
        .unwrap();
    let id = backend.start_dictation_with_options(DictationStartOptions {
        translation_requested: true, ..Default::default()
    }).await.unwrap();
    backend.toggle_dictation_translation_for_session(id).unwrap();
    assert_eq!(
        backend.stop_dictation().await.unwrap().polished_text,
        "polished"
    );
    let custom_payload = payloads.try_recv().unwrap();
    assert!(custom_payload["messages"][0]["content"].as_str().unwrap().contains(&frozen_expression));
    expect_request(custom_payload, "CUSTOM-RAW");
    assert!(payloads.try_recv().is_err(), "custom Raw must issue exactly one request");

    // Translation suppresses expression guidance; cancelling restores this capture's profile.
    let id = backend.start_dictation_with_options(DictationStartOptions {
        style_pack_id: Some(coding.id.clone()), translation_requested: true,
        ..Default::default()
    }).await.unwrap();
    backend.clear_expression_profile().unwrap();
    backend.toggle_dictation_translation_for_session(id).unwrap();
    backend.stop_dictation().await.unwrap();
    let restored = payloads.try_recv().unwrap();
    assert!(restored["messages"][0]["content"].as_str().unwrap().contains(&frozen_expression));
    assert_eq!(restored["messages"][0]["content"].as_str().unwrap().matches(&frozen_expression).count(), 1);
    expect_request(restored, "CODING-STYLE");
    assert!(payloads.try_recv().is_err(), "restored ordinary polish must issue exactly one request");

    backend
        .set_app_style_assignment(AppStyleAssignment::App {
            app_id: "macos:com.microsoft.VSCode".into(),
            style_pack_id: Some(chat.id.clone()),
        })
        .unwrap();
    backend.set_style_pack_enabled(&chat.id, false).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");
    backend.set_style_pack_enabled(&chat.id, true).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CHAT-STYLE");
    backend.remove_style_pack(&chat.id).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");

    let before_clear = backend.app_style_routing().unwrap();
    assert_eq!(before_clear.apps.len(), 7);
    assert_eq!(before_clear.categories[&"chat".to_owned()], chat.id);
    let cleared = backend.clear_input_app_history().unwrap();
    assert_eq!(cleared.apps.len(), 1);
    assert!(!cleared.apps[0].seen);
    assert_eq!(
        cleared.apps[0].style_pack_id.as_deref(),
        Some(chat.id.as_str())
    );
    assert_eq!(backend.get_preferences().active_style_pack_id, current.id);
    let local_file =
        std::fs::read_to_string(fixture.directory.join("app-style-routing.json")).unwrap();
    assert!(!local_file.contains("spoken test transcript"));
    assert!(!local_file.contains("polished"));
    // Exercise the production start -> ASR -> request serialization path.
    // Fixture outputs only prove routing/serialization, never model semantics.
    for id in ["builtin.light", "builtin.structured", "builtin.formal", openless_core::BUILTIN_INTENT_ID, &current.id] {
        for translation_requested in [false, true] {
            backend.start_dictation_with_options(DictationStartOptions {
                style_pack_id: Some(id.into()),
                translation_requested,
                ..Default::default()
            }).await.unwrap();
            backend.stop_dictation().await.unwrap();
            let payload = payloads.try_recv().unwrap();
            let system = payload["messages"][0]["content"].as_str().unwrap();
            assert!(system.starts_with("当前应用：VS Code"));
            let pack = backend.get_style_pack(id).unwrap();
            assert!(system.find("本次输入场景：").unwrap() < system.find(pack.prompt.split("{{HOTWORDS}}").next().unwrap().trim()).unwrap());
            assert_eq!(system.matches(openless_core::DICTATION_REVISION_RULES).count(), 1, "{id}, translation={translation_requested}");
            assert!(payload["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains("spoken test transcript"));
            if translation_requested {
                assert!(system.contains("[[OPENLESS_TRANSLATION]]"));
            }
        }
    }
    backend.shutdown().await.unwrap();
    server.abort();
}
