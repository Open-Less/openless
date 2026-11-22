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
                consumer.consume_pcm_chunk(&[index as u8, 1]);
                consumer.consume_pcm_chunk(&[index as u8, 2]);
                Ok(Box::new(Recording {
                    owner,
                    consumer,
                    index,
                }) as Box<dyn ActiveRecording>)
            })
        }
    }
    struct Transcriber {
        release: Arc<tokio::sync::Semaphore>,
        entered: Arc<tokio::sync::Notify>,
        sessions: Arc<Mutex<Vec<(SessionId, Arc<Transcript>)>>>,
        fail: bool,
    }
    struct Transcript {
        pcm: Mutex<Vec<u8>>,
        first: bool,
        fail: bool,
        release: Arc<tokio::sync::Semaphore>,
        entered: Arc<tokio::sync::Notify>,
    }
    impl AudioConsumer for Transcript {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().extend_from_slice(pcm);
        }
    }
    impl TranscriptionSession for Transcript {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let (first, fail, release, entered) = (
                self.first,
                self.fail,
                self.release.clone(),
                self.entered.clone(),
            );
            Box::pin(async move {
                if first {
                    entered.notify_one();
                    release.acquire().await.unwrap().forget();
                    if fail {
                        return Err(BackendError::new(
                            BackendErrorCode::Provider,
                            "first ASR failed",
                        ));
                    }
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
            let session = Arc::new(Transcript {
                pcm: Mutex::new(Vec::new()),
                first: sessions.is_empty(),
                fail: self.fail,
                release: self.release.clone(),
                entered: self.entered.clone(),
            });
            sessions.push((id, session.clone()));
            Box::pin(async { Ok(session as Arc<dyn TranscriptionSession>) })
        }
    }
    for outcome in ["success", "failure", "cancel"] {
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
            let mut prefs = backend.get_preferences();
            let hotkey = outcome == "success" && !queue_second;
            prefs.hotkey.mode = crate::shared_types::HotkeyMode::Toggle;
            prefs.dictation_hotkey = crate::shared_types::ShortcutBinding {
                primary: "F11".into(),
                modifiers: vec![],
            };
            let profile = backend.expression_profile.lock().unwrap().as_ref().unwrap().clone();
            for index in 0..10 {
                profile.record(uuid::Uuid::new_v4(), 100 + index % 4,
                    crate::expression_observation::ExpressionSignal { line_breaks: 1, exclamations: 0, terminal_punctuation: 0 }).unwrap();
            }
            prefs.expression_learning_enabled = true;
            prefs.cursor_context_enabled = true;
            prefs.stable_transcription_enabled = true;
            prefs.translation_target_language = "English".into();
            backend.set_preferences(prefs).unwrap();
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
            // Turn translation on and off in the same uninterrupted capture.
            assert!(!backend.snapshot().dictation.translation_active);
            backend.toggle_dictation_translation_for_session(first).unwrap();
            assert!(backend.snapshot().dictation.translation_active);
            backend.toggle_dictation_translation_for_session(first).unwrap();
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
                backend.start_dictation_with_options(options).await.unwrap()
            };
            assert!(!backend.snapshot().dictation.translation_active);
            assert!(backend.toggle_dictation_translation_for_session(first).is_err());
            backend.toggle_dictation_translation_for_session(second).unwrap();
            assert!(backend.snapshot().dictation.translation_active);
            assert!(!backend.submitted_dictation_context(first).unwrap().polish.translation_active);
            let second_frozen = backend
                .state
                .read()
                .unwrap()
                .dictation_context
                .clone()
                .unwrap();
            assert_eq!(owner.load(Ordering::SeqCst), 2);
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
            prefs.translation_target_language = "Japanese".into();
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
                backend.stop_dictation_session(second).await.unwrap()
            };
            assert_eq!(result.raw_text, "second");
            assert_eq!(owner.load(Ordering::SeqCst), 0);
            let sessions = sessions.lock().unwrap();
            assert_eq!(
                sessions.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                [first, second]
            );
            assert_eq!(*sessions[0].1.pcm.lock().unwrap(), [1, 1, 1, 2, 1, 3]);
            assert_eq!(*sessions[1].1.pcm.lock().unwrap(), [2, 1, 2, 2, 2, 3]);
            let history = backend.list_history().unwrap();
            assert!(history
                .iter()
                .any(|entry| entry.id == second.to_string() && entry.raw_transcript == "second" && entry.translation_active));
            assert_eq!(
                history.iter().any(|entry| entry.id == first.to_string()),
                outcome != "cancel"
            );
            if let Some(entry) = history.iter().find(|entry| entry.id == first.to_string()) {
                assert!(!entry.translation_active);
            }
            assert!(backend.snapshot().processing_dictations.is_empty());
            assert!(backend.voice_sessions.runtime_restore_idle());
            drop(sessions);
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
                assert_eq!(system.contains("用户经多次编辑"), !context.polish.translation_active);
                assert!(!serialized.contains("Japanese"));
            }
        }
    }
}
