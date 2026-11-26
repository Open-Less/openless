// Included in api::tests to exercise the public facade with the real pipeline.
#[tokio::test]
async fn submitted_segments_release_capture_and_isolate_success_failure_and_cancel() {
    use crate::ports::{
        ActiveRecording, AudioConsumer, AudioRecorder, RecordingProgressSink, TextStreamSink,
        TranscriptOutput, TranscriptionEngine, TranscriptionSession,
    };
    use std::sync::atomic::AtomicUsize;

    struct Recorder {
        owner: Arc<AtomicUsize>,
        index: AtomicUsize,
    }
    struct Recording {
        owner: Arc<AtomicUsize>,
        consumer: Arc<dyn AudioConsumer>,
        index: usize,
    }
    impl ActiveRecording for Recording {
        fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async move {
                // The last frame must join the same segment before release.
                self.consumer.consume_pcm_chunk(&[self.index as u8, 3]);
                assert_eq!(self.owner.swap(0, Ordering::SeqCst), self.index);
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
            let index = self.index.fetch_add(1, Ordering::SeqCst) + 1;
            assert_eq!(
                self.owner.swap(index, Ordering::SeqCst),
                0,
                "two microphone owners"
            );
            let owner = self.owner.clone();
            Box::pin(async move {
                consumer.consume_pcm_chunk(&[index as u8, 1].repeat(1_600));
                consumer.consume_pcm_chunk(&[index as u8, 2]);
                Ok(Box::new(Recording {
                    owner,
                    consumer,
                    index,
                }) as Box<dyn ActiveRecording>)
            })
        }
    }
    type TranscriptSessions = Arc<Mutex<Vec<(SessionId, Arc<Transcript>)>>>;
    struct Transcriber {
        release: Arc<tokio::sync::Semaphore>,
        entered: Arc<tokio::sync::Notify>,
        sessions: TranscriptSessions,
        fail: bool,
        native_gate: Arc<tokio::sync::Semaphore>,
    }
    enum Decode {
        Pcm(Vec<u8>),
        Finish(tokio::sync::oneshot::Sender<Result<TranscriptOutput, BackendError>>),
    }
    struct Transcript {
        pcm: Arc<Mutex<Vec<u8>>>,
        commands: tokio::sync::mpsc::UnboundedSender<Decode>,
        cancelled: Arc<tokio::sync::Notify>,
    }
    impl AudioConsumer for Transcript {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.commands.send(Decode::Pcm(pcm.to_vec())).unwrap();
        }
    }
    impl TranscriptionSession for Transcript {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.commands.send(Decode::Finish(tx)).unwrap();
            Box::pin(async move {
                rx.await.unwrap_or_else(|_| {
                    Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "controlled decode cancelled",
                    ))
                })
            })
        }
        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancelled.notify_one();
            Box::pin(async { Ok(()) })
        }
    }
    impl TranscriptionEngine for Transcriber {
        fn start(
            &self,
            id: SessionId,
            _: Arc<DictationContext>,
            partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let mut sessions = self.sessions.lock().unwrap();
            let first = sessions.is_empty();
            let (commands, mut input) = tokio::sync::mpsc::unbounded_channel();
            let session = Arc::new(Transcript {
                pcm: Arc::new(Mutex::new(Vec::new())),
                commands,
                cancelled: Arc::new(tokio::sync::Notify::new()),
            });
            sessions.push((id, session.clone()));
            let pcm = session.pcm.clone();
            let cancelled = session.cancelled.clone();
            let native_gate = self.native_gate.clone();
            let release = self.release.clone();
            let entered = self.entered.clone();
            let fail = self.fail;
            // Like Sherpa online: PCM enters a per-session worker queue; only
            // native operations share the model executor, never session lifetime.
            tokio::spawn(async move {
                loop {
                    let command = tokio::select! {
                        _ = cancelled.notified() => break,
                        command = input.recv() => match command {
                            Some(command) => command,
                            None => break,
                        },
                    };
                    let _native = tokio::select! {
                        _ = cancelled.notified() => break,
                        permit = native_gate.acquire() => permit.unwrap(),
                    };
                    match command {
                        Decode::Pcm(chunk) => {
                            pcm.lock().unwrap().extend_from_slice(&chunk);
                            partials
                                .publish(crate::ports::TextStreamChunk {
                                    text: if first { "first live" } else { "second live" }.into(),
                                    offset: 0,
                                })
                                .unwrap();
                        }
                        Decode::Finish(reply) => {
                            if first {
                                entered.notify_one();
                                tokio::select! {
                                    _ = cancelled.notified() => break,
                                    permit = release.acquire() => permit.unwrap().forget(),
                                }
                            }
                            let result = if first && fail {
                                Err(BackendError::new(
                                    BackendErrorCode::Provider,
                                    "first ASR failed",
                                ))
                            } else {
                                Ok(TranscriptOutput {
                                    text: if first { "first" } else { "second" }.into(),
                                    duration_ms: 3,
                                })
                            };
                            let _ = reply.send(result);
                            break;
                        }
                    }
                }
            });
            Box::pin(async { Ok(session as Arc<dyn TranscriptionSession>) })
        }
    }
    for (stable, outcome) in [
        (false, "success"),
        (false, "failure"),
        (false, "cancel"),
        (true, "success"),
        (true, "failure"),
        (true, "cancel"),
    ] {
        for queue_second in [false, true] {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            );
            let requests = tokio::spawn(async move {
                let mut bodies = Vec::new();
                for _ in 0..if outcome == "success" { 2 } else { 1 } {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 4096];
                    loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&buffer[..n]);
                        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&request[..end]);
                            let size = headers
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(key, _)| {
                                            key.eq_ignore_ascii_case("content-length")
                                        })
                                        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                                })
                                .unwrap();
                            if request.len() >= end + 4 + size {
                                bodies.push(
                                    serde_json::from_slice::<serde_json::Value>(
                                        &request[end + 4..end + 4 + size],
                                    )
                                    .unwrap(),
                                );
                                break;
                            }
                        }
                    }
                    let body = r#"{"choices":[{"message":{"content":"polished"}}]}"#;
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
                }
                bodies
            });
            let owner = Arc::new(AtomicUsize::new(0));
            let release = Arc::new(tokio::sync::Semaphore::new(0));
            let entered = Arc::new(tokio::sync::Notify::new());
            let sessions = Arc::new(Mutex::new(Vec::new()));
            let data = TestDataDir::new("segment-overlap");
            let mut deps = BackendDependencies::unsupported();
            deps.task_spawner = Arc::new(TokioTaskSpawner);
            deps.credential_store = llm_configured_credentials();
            deps.dictation_engine = Arc::new(crate::DictationEngineRouter::new(Arc::new(
                crate::PipelineDictationEngine::new(
                    Arc::new(Recorder {
                        owner: owner.clone(),
                        index: AtomicUsize::new(0),
                    }),
                    Arc::new(Transcriber {
                        release: release.clone(),
                        entered: entered.clone(),
                        sessions: sessions.clone(),
                        fail: outcome == "failure",
                        native_gate: Arc::new(tokio::sync::Semaphore::new(1)),
                    }),
                    Arc::new(
                        crate::providers::OpenAiChatPolisher::new(
                            crate::providers::OpenAiChatPolisherConfig::new(
                                endpoint.parse().unwrap(),
                                "fixture-model",
                                None,
                                "fixture-system",
                            ),
                        )
                        .unwrap(),
                    ),
                )
                .with_local_startup_spawner(Arc::new(TokioTaskSpawner)),
            )));
            let backend = Arc::new(
                OpenLessBackend::new(
                    BackendConfig {
                        data_dir: data.path().into(),
                        ..Default::default()
                    },
                    deps,
                )
                .unwrap(),
            );
            backend.start().await.unwrap();
            let mut prefs = backend.get_preferences();
            let hotkey = outcome == "success" && !queue_second;
            prefs.hotkey.mode = crate::shared_types::HotkeyMode::Toggle;
            prefs.dictation_hotkey = crate::shared_types::ShortcutBinding {
                primary: "F11".into(),
                modifiers: vec![],
            };
            let profile = backend
                .expression_profile
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .clone();
            for index in 0..10 {
                profile
                    .record(
                        uuid::Uuid::new_v4(),
                        100 + index % 4,
                        crate::expression_observation::ExpressionSignal {
                            line_breaks: 1,
                            exclamations: 0,
                            terminal_punctuation: 0,
                        },
                    )
                    .unwrap();
            }
            prefs.expression_learning_enabled = true;
            prefs.cursor_context_enabled = true;
            prefs.stable_transcription_enabled = stable;
            prefs.active_asr_provider = "sherpa-onnx-local".into();
            prefs.translation_target_language = "English".into();
            backend.set_preferences(prefs).unwrap();
            let mut events = backend.subscribe();
            let options = DictationStartOptions {
                insert_text: false,
                translation_requested: false,
                cursor_context: Some(crate::prompts::cursor_context_input("captured-context", "")),
                front_app: Some("captured-app".into()),
                ..Default::default()
            };
            let key_epoch = std::time::Instant::now();
            let press = |press_id| DictationHotkeyEdge::Pressed {
                press_id,
                at: key_epoch + std::time::Duration::from_secs(press_id),
            };
            let release_key = |press_id| DictationHotkeyEdge::Released {
                press_id,
                at: key_epoch + std::time::Duration::from_millis(press_id * 1000 + 300),
            };
            let first = if hotkey {
                let result = backend
                    .dispatch_dictation_hotkey_edge_with_options(press(1), options.clone())
                    .await
                    .unwrap();
                backend
                    .dispatch_dictation_hotkey_edge(release_key(1))
                    .await
                    .unwrap();
                match result {
                    CliDispatchOutcome::DictationStarted(id) => id,
                    _ => panic!("hotkey did not start capture"),
                }
            } else {
                backend
                    .start_dictation_with_options(options.clone())
                    .await
                    .unwrap()
            };
            assert_eq!(owner.load(Ordering::SeqCst), 1);
            if !stable {
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        let event = events.recv().await.unwrap();
                        if let crate::BackendEventKind::TranscriptDelta(delta) = event.kind {
                            assert_eq!(event.session_id, Some(first));
                            assert_eq!(delta.text, "first live");
                            assert!(!delta.is_final);
                            break;
                        }
                    }
                })
                .await
                .expect("live local ASR must reach the production partial sink before stop");
            } else {
                while let Ok(event) = events.try_recv() {
                    assert!(!matches!(
                        event.kind,
                        crate::BackendEventKind::TranscriptDelta(_)
                    ));
                }
                assert!(sessions.lock().unwrap().is_empty());
            }
            // Turn translation on and off in the same uninterrupted capture.
            assert!(!backend.snapshot().dictation.translation_active);
            backend
                .toggle_dictation_translation_for_session(first)
                .unwrap();
            assert!(backend.snapshot().dictation.translation_active);
            backend
                .toggle_dictation_translation_for_session(first)
                .unwrap();
            assert!(!backend.snapshot().dictation.translation_active);
            assert_eq!(owner.load(Ordering::SeqCst), 1);

            let stopping = tokio::spawn({
                let backend = backend.clone();
                async move {
                    if outcome == "success" && queue_second {
                        return backend
                            .submit_dictation_session(first, Default::default())
                            .await
                            .map(|_| ());
                    }
                    if hotkey {
                        match backend
                            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                                press_id: 2,
                                at: key_epoch + std::time::Duration::from_secs(2),
                            })
                            .await?
                        {
                            CliDispatchOutcome::DictationCompleted(_) => Ok(()),
                            _ => panic!("hotkey did not stop capture"),
                        }
                    } else {
                        backend.stop_dictation_session(first).await.map(|_| ())
                    }
                }
            });
            entered.notified().await;
            assert_eq!(owner.load(Ordering::SeqCst), 0);
            let frozen = backend.submitted_dictation_context(first).unwrap();
            let second = if hotkey {
                backend
                    .dispatch_dictation_hotkey_edge(release_key(2))
                    .await
                    .unwrap();
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    backend.dispatch_dictation_hotkey_edge_with_options(press(3), options),
                )
                .await
                .unwrap()
                .unwrap()
                {
                    CliDispatchOutcome::DictationStarted(id) => id,
                    _ => panic!("processing blocked the next hotkey recording"),
                }
            } else {
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    backend.start_dictation_with_options(options),
                )
                .await
                .expect("prior native finish must not block capture")
                .unwrap()
            };
            assert!(!backend.snapshot().dictation.translation_active);
            assert!(backend
                .toggle_dictation_translation_for_session(first)
                .is_err());
            backend
                .toggle_dictation_translation_for_session(second)
                .unwrap();
            assert!(backend.snapshot().dictation.translation_active);
            assert!(
                !backend
                    .submitted_dictation_context(first)
                    .unwrap()
                    .polish
                    .translation_active
            );
            let second_frozen = backend
                .state
                .read()
                .unwrap()
                .dictation_context
                .clone()
                .unwrap();
            assert_eq!(owner.load(Ordering::SeqCst), 2);
            if !stable {
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    while sessions.lock().unwrap().len() < 2 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert!(
                    sessions.lock().unwrap()[1].1.pcm.lock().unwrap().is_empty(),
                    "second live decode must wait while the first native operation is in flight"
                );
            }
            assert!(backend
                .engine_progress_sink()
                .publish(
                    first,
                    EngineProgress::RecordingFault(BackendError::new(
                        BackendErrorCode::Platform,
                        "late first recorder fault"
                    ))
                )
                .is_err());
            assert_eq!(backend.snapshot().dictation.session_id, Some(second));
            assert_eq!(
                backend.start_dictation().await.unwrap_err().code,
                BackendErrorCode::Busy
            );
            let mut prefs = backend.get_preferences();
            // Clearing the current global target is valid on desktop and the
            // isolated mobile Core, and must not change either frozen segment.
            prefs.translation_target_language.clear();
            backend.set_preferences(prefs).unwrap();
            assert_eq!(
                backend
                    .submitted_dictation_context(first)
                    .unwrap()
                    .polish
                    .translation_target_language,
                frozen.polish.translation_target_language
            );
            let second_stop = if queue_second {
                let task = tokio::spawn({
                    let backend = backend.clone();
                    async move { backend.stop_dictation_session(second).await }
                });
                backend.wait_for_dictation_submission(second).await;
                assert_eq!(owner.load(Ordering::SeqCst), 0);
                assert_eq!(backend.snapshot().processing_dictations.len(), 2);
                Some(task)
            } else {
                None
            };
            if outcome == "cancel" {
                backend.cancel_dictation(Some(first)).await.unwrap();
            }
            let mut observed_transcripts = Vec::new();
            release.add_permits(1);
            let result = stopping.await.unwrap();
            assert_eq!(result.is_ok(), outcome == "success");
            let result = if let Some(second_stop) = second_stop {
                second_stop.await.unwrap().unwrap()
            } else {
                assert_eq!(backend.snapshot().dictation.session_id, Some(second));
                assert_eq!(
                    owner.load(Ordering::SeqCst),
                    2,
                    "old terminal stopped the new recorder"
                );
                if !stable {
                    tokio::time::timeout(std::time::Duration::from_secs(2), async {
                        loop {
                            let event = events.recv().await.unwrap();
                            if let crate::BackendEventKind::TranscriptDelta(delta) = event.kind {
                                let is_second_live = event.session_id == Some(second)
                                    && !delta.is_final
                                    && delta.text == "second live";
                                observed_transcripts.push((event.session_id, delta));
                                if is_second_live {
                                    break;
                                }
                            }
                        }
                    })
                    .await
                    .expect("old terminal must leave the new live partial stream intact");
                }
                backend.stop_dictation_session(second).await.unwrap()
            };
            assert_eq!(result.raw_text, "second");
            while let Ok(event) = events.try_recv() {
                if let crate::BackendEventKind::TranscriptDelta(delta) = event.kind {
                    observed_transcripts.push((event.session_id, delta));
                }
            }
            let finals: Vec<_> = observed_transcripts
                .iter()
                .filter(|(_, delta)| delta.is_final)
                .map(|(id, delta)| (*id, delta.text.as_str()))
                .collect();
            let expected_finals = if outcome == "success" {
                vec![(Some(first), "first"), (Some(second), "second")]
            } else {
                vec![(Some(second), "second")]
            };
            assert_eq!(finals, expected_finals);
            for (id, delta) in &observed_transcripts {
                assert!(matches!(*id, Some(id) if id == first || id == second));
                if !delta.is_final {
                    assert!(!stable);
                    assert_eq!(
                        delta.text,
                        if *id == Some(first) {
                            "first live"
                        } else {
                            "second live"
                        }
                    );
                }
            }
            assert_eq!(owner.load(Ordering::SeqCst), 0);
            {
                let sessions = sessions.lock().unwrap();
                assert_eq!(
                    sessions.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                    [first, second]
                );
                let mut expected_first = [1u8, 1].repeat(1_600);
                expected_first.extend_from_slice(&[1, 2, 1, 3]);
                let mut expected_second = [2u8, 1].repeat(1_600);
                expected_second.extend_from_slice(&[2, 2, 2, 3]);
                assert_eq!(*sessions[0].1.pcm.lock().unwrap(), expected_first);
                assert_eq!(*sessions[1].1.pcm.lock().unwrap(), expected_second);
                let history = backend.list_history().unwrap();
                assert!(history.iter().any(|entry| entry.id == second.to_string()
                    && entry.raw_transcript == "second"
                    && entry.translation_active));
                assert_eq!(
                    history.iter().any(|entry| entry.id == first.to_string()),
                    outcome != "cancel"
                );
                if let Some(entry) = history.iter().find(|entry| entry.id == first.to_string()) {
                    assert!(!entry.translation_active);
                }
                assert!(backend.snapshot().processing_dictations.is_empty());
                assert!(backend.voice_sessions.runtime_restore_idle());
            }
            let bodies = tokio::time::timeout(std::time::Duration::from_secs(3), requests)
                .await
                .unwrap()
                .unwrap();
            let expected = if outcome == "success" {
                vec![(frozen, "first"), (second_frozen, "second")]
            } else {
                vec![(second_frozen, "second")]
            };
            assert_eq!(bodies.len(), expected.len());
            for (request, (context, raw)) in bodies.into_iter().zip(expected) {
                let serialized = request.to_string();
                let (system, user) = context.effective_polish_prompts(raw);
                assert_eq!(request["messages"][0]["content"], system);
                assert_eq!(request["messages"][1]["content"], user);
                assert!(serialized.contains("captured-context"));
                assert_eq!(
                    system.contains("用户经多次编辑"),
                    !context.polish.translation_active
                );
                assert_eq!(context.polish.translation_target_language, "English");
            }
        }
    }
}

#[tokio::test]
async fn rapid_three_submissions_keep_pcm_and_join_final_input_before_the_next() {
    use crate::ports::{
        ActiveRecording, AudioConsumer, AudioRecorder, RecordingProgressSink, TextStreamSink,
        TranscriptOutput, TranscriptionEngine, TranscriptionSession,
    };
    use std::sync::atomic::AtomicUsize;
    struct Recorder(AtomicUsize);
    struct Recording(usize, Arc<dyn AudioConsumer>);
    impl AudioRecorder for Recorder {
        fn start(
            &self,
            _: SessionId,
            _: Arc<DictationContext>,
            consumer: Arc<dyn AudioConsumer>,
            _: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            consumer.consume_pcm_chunk(&[n as u8, 1].repeat(1_600));
            consumer.consume_pcm_chunk(&[n as u8, 2]);
            Box::pin(
                async move { Ok(Box::new(Recording(n, consumer)) as Box<dyn ActiveRecording>) },
            )
        }
    }
    impl ActiveRecording for Recording {
        fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
            self.1.consume_pcm_chunk(&[self.0 as u8, 3]);
            Box::pin(async { Ok(()) })
        }
    }
    type Audio = Arc<Mutex<Vec<Arc<Transcript>>>>;
    struct Transcriber {
        audio: Audio,
        release: Arc<tokio::sync::Semaphore>,
        outcome: &'static str,
    }
    struct Transcript {
        n: usize,
        pcm: Mutex<Vec<u8>>,
        release: Arc<tokio::sync::Semaphore>,
        outcome: &'static str,
    }
    impl AudioConsumer for Transcript {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().extend_from_slice(pcm);
        }
    }
    impl TranscriptionSession for Transcript {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let (n, release, outcome) = (self.n, self.release.clone(), self.outcome);
            Box::pin(async move {
                if n == 1 {
                    release.acquire().await.unwrap().forget();
                }
                if n == 1 && outcome == "failure" {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "controlled first ASR failure",
                    ));
                }
                Ok(TranscriptOutput {
                    text: format!("segment{n}"),
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
            _: SessionId,
            _: Arc<DictationContext>,
            _: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let mut audio = self.audio.lock().unwrap();
            let item = Arc::new(Transcript {
                n: audio.len() + 1,
                pcm: Mutex::new(Vec::new()),
                release: self.release.clone(),
                outcome: self.outcome,
            });
            audio.push(item.clone());
            Box::pin(async { Ok(item as Arc<dyn TranscriptionSession>) })
        }
    }
    struct Inserter {
        calls: Arc<Mutex<Vec<String>>>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    }
    impl crate::ports::TextInserter for Inserter {
        fn defer_preparation_until_stop(&self) -> bool {
            true
        }
        fn begin(
            &self,
            _: SessionId,
            context: Arc<DictationContext>,
        ) -> BoxFuture<'static, Result<Arc<dyn crate::ports::TextInsertionSession>, BackendError>>
        {
            // Deferred preparation preserves stable input preferences; Raw still
            // performs only final delivery, whose ordering is asserted below.
            let preferences = crate::shared_types::UserPreferences::default();
            assert_eq!(context.insertion.streaming, preferences.streaming_insert);
            assert_eq!(
                context.insertion.composition_input,
                cfg!(target_os = "windows")
                    && preferences.windows_composition_input
                    && preferences.windows_insertion_mode
                        == crate::shared_types::WindowsInsertionMode::Tsf
            );
            let session = Arc::new(Insertion {
                calls: self.calls.clone(),
                entered: self.entered.clone(),
                release: self.release.clone(),
            });
            Box::pin(async { Ok(session as Arc<dyn crate::ports::TextInsertionSession>) })
        }
    }
    struct Insertion {
        calls: Arc<Mutex<Vec<String>>>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    }
    impl crate::ports::TextInsertionSession for Insertion {
        fn write(
            &self,
            _: String,
        ) -> BoxFuture<'static, Result<crate::ports::InsertWriteResult, BackendError>> {
            panic!("no live target writes")
        }
        fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
            panic!("no fallback expected")
        }
        fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
            let (calls, entered, release) = (
                self.calls.clone(),
                self.entered.clone(),
                self.release.clone(),
            );
            Box::pin(async move {
                calls.lock().unwrap().push(format!("start:{text}"));
                if text == "segment1" {
                    entered.notify_one();
                    release.acquire().await.unwrap().forget();
                }
                calls.lock().unwrap().push(format!("end:{text}"));
                Ok(InsertOutcome::Inserted)
            })
        }
        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }
    for outcome in ["success", "failure", "cancel"] {
        let data = TestDataDir::new("rapid-three");
        let audio = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let input_release = Arc::new(tokio::sync::Semaphore::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut deps = BackendDependencies::unsupported();
        deps.task_spawner = Arc::new(TokioTaskSpawner);
        deps.host_actions = Arc::new(FakeHost::default());
        deps.credential_store = llm_configured_credentials();
        deps.text_inserter = Arc::new(Inserter {
            calls: calls.clone(),
            entered: entered.clone(),
            release: input_release.clone(),
        });
        deps.dictation_engine = Arc::new(crate::DictationEngineRouter::new(Arc::new(
            crate::PipelineDictationEngine::new(
                Arc::new(Recorder(AtomicUsize::new(0))),
                Arc::new(Transcriber {
                    audio: audio.clone(),
                    release: release.clone(),
                    outcome,
                }),
                Arc::new(crate::testing::FixtureTextPolisher::successful("unused")),
            ),
        )));
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data.path().into(),
                    ..Default::default()
                },
                deps,
            )
            .unwrap(),
        );
        backend.start().await.unwrap();
        backend
            .activate_style_pack(crate::style_packs::BUILTIN_STYLE_PACK_RAW_ID)
            .unwrap();
        let mut ids = Vec::new();
        let mut stops = Vec::new();
        for _ in 0..3 {
            let id = backend.start_dictation().await.unwrap();
            ids.push(id);
            let owner = backend.clone();
            stops.push(tokio::spawn(async move {
                owner.stop_dictation_session(id).await
            }));
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                backend.wait_for_dictation_submission(id),
            )
            .await
            .unwrap();
        }
        assert!(calls.lock().unwrap().is_empty());
        let cancel = if outcome == "cancel" {
            let owner = backend.clone();
            let first = ids[0];
            let task = tokio::spawn(async move { owner.cancel_dictation(Some(first)).await });
            tokio::task::yield_now().await;
            Some(task)
        } else {
            None
        };
        release.add_permits(1);
        if outcome == "success" {
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            assert_eq!(*calls.lock().unwrap(), ["start:segment1"]);
            input_release.add_permits(1);
        }
        if let Some(cancel) = cancel {
            cancel.await.unwrap().unwrap();
        }
        for (index, stop) in stops.into_iter().enumerate() {
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), stop)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.is_ok(), index > 0 || outcome == "success");
        }
        let expected: Vec<_> = (if outcome == "success" { 1 } else { 2 }..=3)
            .flat_map(|n| [format!("start:segment{n}"), format!("end:segment{n}")])
            .collect();
        assert_eq!(*calls.lock().unwrap(), expected);
        for (index, item) in audio.lock().unwrap().iter().enumerate() {
            let n = index as u8 + 1;
            let mut expected = [n, 1].repeat(1_600);
            expected.extend_from_slice(&[n, 2, n, 3]);
            assert_eq!(*item.pcm.lock().unwrap(), expected);
        }
        let history = backend.list_history().unwrap();
        for (index, id) in ids.iter().enumerate().skip(1) {
            assert!(history
                .iter()
                .any(|row| row.id == id.to_string()
                    && row.final_text == format!("segment{}", index + 1)));
        }
        backend.shutdown().await.unwrap();
    }
}
