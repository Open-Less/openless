use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::*;
use tokio::sync::Semaphore;

struct Host;
impl HostActions for Host {
    fn request(&self, _: HostAction) -> Result<(), BackendError> {
        Ok(())
    }
}
struct Control;
impl RecordingControlSink for Control {
    fn request(&self, _: SessionId, _: RecordingControlAction) -> Result<(), BackendError> {
        Ok(())
    }
}
struct StopFailure;
impl AudioRecorder for StopFailure {
    fn start(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        _: Arc<dyn AudioConsumer>,
        _: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        Box::pin(async { Ok(Box::new(StopFailure) as Box<dyn ActiveRecording>) })
    }
}
impl ActiveRecording for StopFailure {
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Platform,
                "native stop failed: secret-api-key=https://private.example/token",
            ))
        })
    }
}
struct Progress;
impl RecordingProgressSink for Progress {
    fn publish_level(&self, _: u64, _: f32) -> Result<(), BackendError> {
        Ok(())
    }
}

// The Host owns its slot/target; the opaque Core capture owns native cleanup.
// Model the same synchronous slot transfer as Tauri's recording-control adapter.
#[derive(Default)]
struct SelectionHostControl {
    owner: Mutex<Option<SessionId>>,
    capture: Mutex<Option<Arc<VoiceTranscriptionSession>>>,
    cancellations: AtomicUsize,
}
impl RecordingControlSink for SelectionHostControl {
    fn request(&self, id: SessionId, action: RecordingControlAction) -> Result<(), BackendError> {
        if action != RecordingControlAction::Cancel {
            return Ok(());
        }
        let mut owner = self.owner.lock().unwrap();
        if *owner != Some(id) {
            return Ok(());
        }
        *owner = None;
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        let capture = self.capture.lock().unwrap().take();
        if let Some(capture) = capture {
            tokio::spawn(capture.cancel());
        }
        Ok(())
    }
}

struct Recorder {
    starts: Arc<AtomicUsize>,
    stopped: Arc<Semaphore>,
    stop_gate: Arc<Semaphore>,
    archive: Option<Arc<Archive>>,
}
#[derive(Default)]
struct Archive(Arc<AtomicUsize>);
impl RecordingArchive for Archive {
    fn is_available(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 0
    }
    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        Box::pin(async { Ok(vec![0, 1]) })
    }
    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
struct Recording {
    stopped: Arc<Semaphore>,
    gate: Arc<Semaphore>,
    archive: Option<Arc<Archive>>,
}
impl ActiveRecording for Recording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.archive
            .clone()
            .map(|archive| archive as Arc<dyn RecordingArchive>)
    }
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            self.stopped.add_permits(1);
            self.gate.acquire().await.unwrap().forget();
            Ok(())
        })
    }
}
impl AudioRecorder for Recorder {
    fn start(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        _: Arc<dyn AudioConsumer>,
        _: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        let recording = Recording {
            stopped: self.stopped.clone(),
            gate: self.stop_gate.clone(),
            archive: self.archive.clone(),
        };
        Box::pin(async move { Ok(Box::new(recording) as Box<dyn ActiveRecording>) })
    }
}
struct SlowAsr {
    entered: Arc<Semaphore>,
    gate: Arc<Semaphore>,
    inner: testing::FixtureTranscriptionEngine,
}

// Model only the native archive boundary: once an archive was requested, a
// filesystem sharing violation would make its final deletion fail. PCM remains
// available in memory and must not depend on this optional disk side effect.
struct ArchivePolicyRecorder {
    inner: testing::FixtureAudioRecorder,
    plans: Arc<Mutex<Vec<RecordingPlan>>>,
}
struct ArchivePolicyRecording {
    inner: Box<dyn ActiveRecording>,
    archive_enabled: bool,
}
struct LockedArchive;
impl RecordingArchive for LockedArchive {
    fn is_available(&self) -> bool {
        true
    }
    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Persistence,
                "archive is locked by another process",
            ))
        })
    }
}
impl ActiveRecording for ArchivePolicyRecording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.archive_enabled
            .then(|| Arc::new(LockedArchive) as Arc<dyn RecordingArchive>)
    }
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        self.inner.stop()
    }
}
impl AudioRecorder for ArchivePolicyRecorder {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        self.plans.lock().unwrap().push(context.recording.clone());
        let archive_enabled = context.recording.archive_enabled;
        let starting = self.inner.start(id, context, consumer, progress);
        Box::pin(async move {
            Ok(Box::new(ArchivePolicyRecording {
                inner: starting.await?,
                archive_enabled,
            }) as Box<dyn ActiveRecording>)
        })
    }
}
struct SlowRecorder {
    inner: Recorder,
    entered: Arc<Semaphore>,
    gate: Arc<Semaphore>,
    progress: Arc<Mutex<Option<Arc<dyn RecordingProgressSink>>>>,
}
impl AudioRecorder for SlowRecorder {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        *self.progress.lock().unwrap() = Some(progress.clone());
        let started = self.inner.start(id, context, consumer, progress);
        let entered = self.entered.clone();
        let gate = self.gate.clone();
        Box::pin(async move {
            entered.add_permits(1);
            gate.acquire().await.unwrap().forget();
            started.await
        })
    }
}
impl TranscriptionEngine for SlowAsr {
    fn start(
        &self,
        id: SessionId,
        context: Arc<DictationContext>,
        sink: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let entered = self.entered.clone();
        let gate = self.gate.clone();
        let result = self.inner.start(id, context, sink);
        Box::pin(async move {
            entered.add_permits(1);
            gate.acquire().await.unwrap().forget();
            result.await
        })
    }
}

#[derive(Default)]
struct QaRuntime {
    capture: Mutex<Option<Arc<QaVoiceCaptureSession>>>,
    fail: AtomicBool,
}
impl QaRuntimeAdapter for QaRuntime {
    fn prepare_text(
        &self,
        _: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        Box::pin(async move {
            Ok(QaInput {
                text,
                selection_text: None,
                selection_source_app: None,
            })
        })
    }
    fn start_recording(
        &self,
        _: SessionId,
        _: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
    fn finish_recording(&self, _: SessionId) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        self.prepare_text(SessionId::new(), "question".into())
    }
    fn answer(
        &self,
        _: QaTurnRequest,
        _: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
        let fail = self.fail.load(Ordering::SeqCst);
        Box::pin(async move {
            if fail {
                Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture failure",
                ))
            } else {
                Ok(QaTurnResult {
                    answer: "answer".into(),
                })
            }
        })
    }
    fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let capture = self.capture.lock().unwrap().take();
        Box::pin(async move {
            if let Some(capture) = capture {
                capture.cancel().await
            } else {
                Ok(())
            }
        })
    }
}

fn backend(
    recorder: Arc<dyn AudioRecorder>,
    asr: Arc<dyn TranscriptionEngine>,
    qa: Arc<QaRuntime>,
) -> (Arc<OpenLessBackend>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("openless-aux-voice-{}", uuid::Uuid::new_v4()));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.host_actions = Arc::new(Host);
    dependencies.qa_runtime = Some(qa);
    dependencies.dictation_engine = Arc::new(PipelineDictationEngine::new(
        recorder,
        asr,
        Arc::new(testing::FixtureTextPolisher::successful("unused")),
    ));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: path.clone(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap(),
    );
    let mut prefs = backend.get_preferences();
    prefs.coding_agent_enabled = true;
    backend
        .update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)
        .unwrap();
    (backend, path)
}

#[tokio::test]
async fn qa_and_selection_voice_never_request_disk_archives() {
    for entry in ["qa", "qa-omni", "selection", "dictation", "less"] {
        let plans = Arc::new(Mutex::new(Vec::new()));
        let recorder = testing::FixtureAudioRecorder::new(vec![vec![0; 320]], Vec::new());
        let (backend, path) = backend(
            Arc::new(ArchivePolicyRecorder {
                inner: recorder.clone(),
                plans: plans.clone(),
            }),
            Arc::new(testing::FixtureTranscriptionEngine::successful(
                "instruction",
                10,
            )),
            Arc::new(QaRuntime::default()),
        );
        // The main-path debug switch must not opt private QA/Selection audio
        // into disk retention. Traditional and Omni QA share this boundary.
        let mut prefs = backend.get_preferences();
        prefs.record_audio_for_debug = true;
        if entry == "qa-omni" {
            prefs.multimodal_pipeline_enabled = true;
            prefs.pipeline_mode = shared_types::PipelineMode::Multimodal;
        }
        backend
            .update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)
            .unwrap();
        backend.start().await.unwrap();
        match entry {
            "qa" | "qa-omni" => {
                backend.services().qa.toggle_recording().await.unwrap();
                let id = backend
                    .services()
                    .qa
                    .snapshot()
                    .await
                    .unwrap()
                    .session_id
                    .unwrap();
                let capture = backend
                    .start_qa_voice_capture(
                        id,
                        DictationStartOptions::default(),
                        Arc::new(Progress),
                    )
                    .await
                    .unwrap();
                let output = capture
                    .finish()
                    .await
                    .expect("private QA must not create a fallible archive");
                if entry == "qa" {
                    assert_eq!(output.transcript.as_deref(), Some("instruction"));
                } else {
                    assert!(output.audio_wav.is_some());
                }
                backend.services().qa.dismiss().await.unwrap();
            }
            "selection" => {
                let id = backend
                    .services()
                    .selection_voice
                    .begin(SelectionCapture {
                        text: "selection".into(),
                        source_app: None,
                    })
                    .await
                    .unwrap();
                let capture = backend
                    .start_selection_voice_capture(id, Arc::new(Control))
                    .await
                    .unwrap();
                assert_eq!(
                    capture
                        .finish()
                        .await
                        .expect("private Selection audio must stay in memory"),
                    "instruction"
                );
                backend
                    .services()
                    .selection_voice
                    .cancel(Some(id))
                    .await
                    .unwrap();
            }
            "dictation" => {
                backend
                    .start_dictation_with_options(DictationStartOptions {
                        insert_text: false,
                        ..DictationStartOptions::default()
                    })
                    .await
                    .unwrap();
                let _ = backend.cancel_dictation(None).await;
            }
            "less" => {
                let capture = backend
                    .start_less_computer_voice(SessionId::new(), Arc::new(Control))
                    .await
                    .unwrap();
                let _ = capture.cancel().await;
            }
            _ => unreachable!(),
        }
        assert_eq!(
            plans.lock().unwrap()[0].archive_enabled,
            matches!(entry, "dictation" | "less"),
            "{entry}"
        );
        assert_eq!(recorder.stop_count(), 1, "{entry}");
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn cli_cancel_covers_less_capture_without_expanding_to_qa() {
    let recorder = Arc::new(testing::FixtureAudioRecorder::default());
    let (backend, path) = backend(
        recorder.clone(),
        Arc::new(testing::FixtureTranscriptionEngine::successful(
            "instruction",
            10,
        )),
        Arc::new(QaRuntime::default()),
    );
    let capture = backend
        .start_less_computer_voice(SessionId::new(), Arc::new(Control))
        .await
        .unwrap();
    assert_eq!(
        backend
            .dispatch_cli_intent(CliIntent::CancelDictation)
            .await
            .unwrap(),
        CliDispatchOutcome::DictationCancelled
    );
    assert_eq!(recorder.stop_count(), 1);
    let next = backend
        .start_less_computer_voice(SessionId::new(), Arc::new(Control))
        .await
        .unwrap();
    next.cancel().await.unwrap();
    drop(capture);
    backend.services().qa.toggle_recording().await.unwrap();
    let qa = backend.services().qa.snapshot().await.unwrap();
    assert_eq!(
        backend
            .dispatch_cli_intent(CliIntent::CancelDictation)
            .await
            .unwrap(),
        CliDispatchOutcome::Noop
    );
    assert_eq!(
        backend.services().qa.snapshot().await.unwrap(),
        qa,
        "legacy CLI cancel did not target the separate QA session"
    );
    backend.services().qa.dismiss().await.unwrap();
    drop(backend);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn cancelled_auxiliary_capture_keeps_gate_until_native_stop_finishes() {
    for less in [true, false] {
        let stopped = Arc::new(Semaphore::new(0));
        let stop_gate = Arc::new(Semaphore::new(0));
        let qa = Arc::new(QaRuntime::default());
        let (backend, path) = backend(
            Arc::new(Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: stopped.clone(),
                stop_gate: stop_gate.clone(),
                archive: None,
            }),
            Arc::new(testing::FixtureTranscriptionEngine::successful(
                "instruction",
                10,
            )),
            qa.clone(),
        );
        backend.start().await.unwrap();
        let cancel = if less {
            let capture = backend
                .start_less_computer_voice(SessionId::new(), Arc::new(Control))
                .await
                .unwrap();
            tokio::spawn(async move { capture.cancel().await })
        } else {
            backend.services().qa.toggle_recording().await.unwrap();
            let id = backend
                .services()
                .qa
                .snapshot()
                .await
                .unwrap()
                .session_id
                .unwrap();
            *qa.capture.lock().unwrap() = Some(Arc::new(
                backend
                    .start_qa_voice_capture(
                        id,
                        DictationStartOptions::default(),
                        Arc::new(Progress),
                    )
                    .await
                    .unwrap(),
            ));
            let backend = backend.clone();
            tokio::spawn(async move { backend.services().qa.cancel(Some(id)).await })
        };
        stopped.acquire().await.unwrap().forget();
        let next = backend.begin_less_computer_capture(SessionId::new());
        stop_gate.add_permits(1);
        cancel.await.unwrap().unwrap();
        assert_eq!(
            next.unwrap_err().code,
            BackendErrorCode::Busy,
            "less={less}: native stop still owns the microphone"
        );
        let next = SessionId::new();
        backend.begin_less_computer_capture(next).unwrap();
        backend.abort_less_computer_capture(next).unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn cancellation_during_cold_asr_never_starts_the_microphone() {
    for less in [true, false] {
        let starts = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Semaphore::new(0));
        let gate = Arc::new(Semaphore::new(0));
        let (backend, path) = backend(
            Arc::new(Recorder {
                starts: starts.clone(),
                stopped: Arc::new(Semaphore::new(0)),
                stop_gate: Arc::new(Semaphore::new(1)),
                archive: None,
            }),
            Arc::new(SlowAsr {
                entered: entered.clone(),
                gate: gate.clone(),
                inner: testing::FixtureTranscriptionEngine::successful("late", 10),
            }),
            Arc::new(QaRuntime::default()),
        );
        backend.start().await.unwrap();
        let id = if less {
            SessionId::new()
        } else {
            backend.services().qa.toggle_recording().await.unwrap();
            backend
                .services()
                .qa
                .snapshot()
                .await
                .unwrap()
                .session_id
                .unwrap()
        };
        let mut events = backend.subscribe();
        let starting = tokio::spawn({
            let backend = backend.clone();
            async move {
                if less {
                    backend
                        .start_less_computer_voice(id, Arc::new(Control))
                        .await
                        .map(|_| ())
                } else {
                    backend
                        .start_qa_voice_capture(
                            id,
                            DictationStartOptions::default(),
                            Arc::new(Progress),
                        )
                        .await
                        .map(|_| ())
                }
            }
        });
        entered.acquire().await.unwrap().forget();
        if less {
            backend.cancel_less_computer(Some(id)).await.unwrap();
        } else {
            backend.services().qa.cancel(Some(id)).await.unwrap();
        }
        gate.add_permits(1);
        assert!(starting.await.unwrap().is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        if less {
            let phases = std::iter::from_fn(|| events.try_recv().ok())
                .filter_map(|event| match event.kind {
                    BackendEventKind::LessComputerEvent(LessComputerEvent {
                        kind: LessComputerEventKind::VoiceState { phase, .. },
                        ..
                    }) => Some(phase),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                phases,
                [
                    LessComputerVoicePhase::Starting,
                    LessComputerVoicePhase::Idle
                ]
            );
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn failed_native_start_closes_less_voice_feedback_and_reports_error() {
    struct FailedRecorder;
    impl AudioRecorder for FailedRecorder {
        fn start(
            &self,
            _: SessionId,
            _: Arc<DictationContext>,
            _: Arc<dyn AudioConsumer>,
            _: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::PermissionDenied,
                    "fixture microphone denied",
                ))
            })
        }
    }
    let (backend, path) = backend(
        Arc::new(FailedRecorder),
        Arc::new(testing::FixtureTranscriptionEngine::successful("unused", 0)),
        Arc::new(QaRuntime::default()),
    );
    backend.start().await.unwrap();
    let mut events = backend.subscribe();
    assert!(backend
        .start_less_computer_voice(SessionId::new(), Arc::new(Control))
        .await
        .is_err());
    let mut phases = Vec::new();
    let mut errors = 0;
    while let Ok(event) = events.try_recv() {
        match event.kind {
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                kind: LessComputerEventKind::VoiceState { phase, .. },
                ..
            }) => phases.push(phase),
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                kind: LessComputerEventKind::Error { .. },
                ..
            }) => errors += 1,
            _ => {}
        }
    }
    assert_eq!(
        phases,
        [
            LessComputerVoicePhase::Starting,
            LessComputerVoicePhase::Idle
        ]
    );
    assert_eq!(errors, 1);
    let next = SessionId::new();
    backend.begin_less_computer_capture(next).unwrap();
    backend.abort_less_computer_capture(next).unwrap();
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn less_successful_asr_discards_only_non_debug_audio() {
    for (debug, fail, expected_discards) in [(false, false, 1), (true, false, 0), (false, true, 0)]
    {
        let archive = Arc::new(Archive::default());
        let transcription = if fail {
            testing::FixtureTranscriptionEngine::failing(BackendError::new(
                BackendErrorCode::Provider,
                "fixture ASR failure",
            ))
        } else {
            testing::FixtureTranscriptionEngine::successful("instruction", 10)
        };
        let (backend, path) = backend(
            Arc::new(Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: Arc::new(Semaphore::new(0)),
                stop_gate: Arc::new(Semaphore::new(1)),
                archive: Some(archive.clone()),
            }),
            Arc::new(transcription),
            Arc::new(QaRuntime::default()),
        );
        let mut prefs = backend.get_preferences();
        prefs.record_audio_for_debug = debug;
        backend
            .update_settings(prefs, SettingsUpdateOptions::STRICT, &NoopSettingsRuntime)
            .unwrap();
        backend.start().await.unwrap();
        let capture = backend
            .start_less_computer_voice(SessionId::new(), Arc::new(Control))
            .await
            .unwrap();
        // Archive retention depends on successful ASR, before any Agent outcome.
        let _ = capture.finish().await;
        assert_eq!(
            archive.0.load(Ordering::SeqCst),
            expected_discards,
            "debug={debug}, fail={fail}"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn completed_failed_and_cancelled_qa_allow_less_voice() {
    for phase in [QaPhase::Completed, QaPhase::Failed, QaPhase::Cancelled] {
        let qa = Arc::new(QaRuntime::default());
        let (backend, path) = backend(
            Arc::new(testing::FixtureAudioRecorder::new(vec![], vec![])),
            Arc::new(testing::FixtureTranscriptionEngine::successful(
                "instruction",
                10,
            )),
            qa.clone(),
        );
        backend.start().await.unwrap();
        qa.fail.store(phase == QaPhase::Failed, Ordering::SeqCst);
        let _ = backend.services().qa.submit_text("question".into()).await;
        if phase == QaPhase::Cancelled {
            backend.services().qa.cancel(None).await.unwrap();
        }
        assert_eq!(backend.services().qa.snapshot().await.unwrap().phase, phase);
        let capture = backend
            .start_less_computer_voice(SessionId::new(), Arc::new(Control))
            .await;
        assert!(
            capture.is_ok(),
            "QA terminal {phase:?} must not occupy voice capture"
        );
        capture.unwrap().cancel().await.unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn abandoning_a_cancel_reply_does_not_abandon_native_cleanup() {
    for less in [true, false] {
        let stopped = Arc::new(Semaphore::new(0));
        let stop_gate = Arc::new(Semaphore::new(0));
        let qa = Arc::new(QaRuntime::default());
        let (backend, path) = backend(
            Arc::new(Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: stopped.clone(),
                stop_gate: stop_gate.clone(),
                archive: None,
            }),
            Arc::new(testing::FixtureTranscriptionEngine::successful(
                "instruction",
                10,
            )),
            qa.clone(),
        );
        backend.start().await.unwrap();
        let mut retained_less_capture = None;
        let id = if less {
            let id = SessionId::new();
            retained_less_capture = Some(
                backend
                    .start_less_computer_voice(id, Arc::new(Control))
                    .await
                    .unwrap(),
            );
            id
        } else {
            backend.services().qa.toggle_recording().await.unwrap();
            let id = backend
                .services()
                .qa
                .snapshot()
                .await
                .unwrap()
                .session_id
                .unwrap();
            *qa.capture.lock().unwrap() = Some(Arc::new(
                backend
                    .start_qa_voice_capture(
                        id,
                        DictationStartOptions::default(),
                        Arc::new(Progress),
                    )
                    .await
                    .unwrap(),
            ));
            id
        };
        let task = tokio::spawn({
            let backend = backend.clone();
            async move {
                if less {
                    backend.cancel_less_computer(Some(id)).await
                } else {
                    backend.services().qa.cancel(Some(id)).await
                }
            }
        });
        stopped.acquire().await.unwrap().forget();
        task.abort();
        let _ = task.await;
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        stop_gate.add_permits(1);
        let next = SessionId::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if backend.begin_less_computer_capture(next).is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if let Some(capture) = retained_less_capture {
            capture.cancel().await.unwrap();
            assert_eq!(
                backend.less_computer_active_session(),
                Some(next),
                "old Host handle must not cancel its successor"
            );
        }
        backend.abort_less_computer_capture(next).unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn abandoned_start_closes_a_late_recorder_before_releasing_the_gate() {
    for (entry, abandon_first) in [
        ("less", false),
        ("qa", false),
        ("dictation", false),
        ("dictation", true),
    ] {
        let less = entry == "less";
        let dictation = entry == "dictation";
        let entered = Arc::new(Semaphore::new(0));
        let start_gate = Arc::new(Semaphore::new(0));
        let stopped = Arc::new(Semaphore::new(0));
        let stop_gate = Arc::new(Semaphore::new(0));
        let qa = Arc::new(QaRuntime::default());
        let (backend, path) = backend(
            Arc::new(SlowRecorder {
                inner: Recorder {
                    starts: Arc::new(AtomicUsize::new(0)),
                    stopped: stopped.clone(),
                    stop_gate: stop_gate.clone(),
                    archive: None,
                },
                entered: entered.clone(),
                gate: start_gate.clone(),
                progress: Arc::new(Mutex::new(None)),
            }),
            Arc::new(testing::FixtureTranscriptionEngine::successful("late", 10)),
            qa,
        );
        backend.start().await.unwrap();
        let id = if less || dictation {
            SessionId::new()
        } else {
            backend.services().qa.toggle_recording().await.unwrap();
            backend
                .services()
                .qa
                .snapshot()
                .await
                .unwrap()
                .session_id
                .unwrap()
        };
        let starting = tokio::spawn({
            let backend = backend.clone();
            async move {
                if dictation {
                    backend
                        .start_dictation_with_options(DictationStartOptions {
                            insert_text: false,
                            ..DictationStartOptions::default()
                        })
                        .await
                        .map(|_| ())
                } else if less {
                    backend
                        .start_less_computer_voice(id, Arc::new(Control))
                        .await
                        .map(|_| ())
                } else {
                    backend
                        .start_qa_voice_capture(
                            id,
                            DictationStartOptions::default(),
                            Arc::new(Progress),
                        )
                        .await
                        .map(|_| ())
                }
            }
        });
        entered.acquire().await.unwrap().forget();
        let mut starting = Some(starting);
        if abandon_first {
            let starting = starting.take().unwrap();
            starting.abort();
            let _ = starting.await;
        }
        if dictation {
            backend.cancel_dictation(None).await.unwrap();
        } else if less {
            backend.cancel_less_computer(Some(id)).await.unwrap();
        } else {
            backend.services().qa.cancel(Some(id)).await.unwrap();
        }
        if let Some(starting) = starting {
            starting.abort();
            let _ = starting.await;
        }
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        start_gate.add_permits(1);
        stopped.acquire().await.unwrap().forget();
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        stop_gate.add_permits(1);
        let next = SessionId::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if backend.begin_less_computer_capture(next).is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        backend.abort_less_computer_capture(next).unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn less_voice_feedback_preserves_phases_and_rejects_late_levels() {
    let progress = Arc::new(Mutex::new(None));
    let (backend, path) = backend(
        Arc::new(SlowRecorder {
            inner: Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: Arc::new(Semaphore::new(0)),
                stop_gate: Arc::new(Semaphore::new(1)),
                archive: None,
            },
            entered: Arc::new(Semaphore::new(0)),
            gate: Arc::new(Semaphore::new(1)),
            progress: progress.clone(),
        }),
        Arc::new(testing::FixtureTranscriptionEngine::successful(
            "instruction",
            10,
        )),
        Arc::new(QaRuntime::default()),
    );
    backend.start().await.unwrap();
    let mut events = backend.subscribe();
    let id = SessionId::new();
    let capture = backend
        .start_less_computer_voice(id, Arc::new(Control))
        .await
        .unwrap();
    let sink = progress.lock().unwrap().clone().unwrap();
    assert!(matches!(
        backend.event_publisher().latest_less_computer_voice_state(),
        Some(LessComputerEvent {
            kind: LessComputerEventKind::VoiceState {
                phase: LessComputerVoicePhase::Starting,
                ..
            },
            ..
        })
    ));
    sink.publish_level(0, 0.0).unwrap();
    sink.publish_level(80, 0.7).unwrap();
    let _ = capture.finish().await;
    sink.publish_level(100, 0.8).unwrap();
    let phases = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| {
            if let BackendEventKind::LessComputerEvent(LessComputerEvent {
                kind:
                    LessComputerEventKind::VoiceState {
                        session_id,
                        phase,
                        level,
                        ..
                    },
                ..
            }) = event.kind
            {
                assert_eq!(session_id, id);
                Some((phase, level))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        vec![
            (LessComputerVoicePhase::Starting, 0.0),
            (LessComputerVoicePhase::Recording, 0.0),
            (LessComputerVoicePhase::Recording, 0.7),
            (LessComputerVoicePhase::Transcribing, 0.0),
            (LessComputerVoicePhase::Idle, 0.0)
        ]
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn non_inserting_dictation_keeps_gate_while_native_stop_is_in_flight() {
    let stopped = Arc::new(Semaphore::new(0));
    let stop_gate = Arc::new(Semaphore::new(0));
    let (backend, path) = backend(
        Arc::new(Recorder {
            starts: Arc::new(AtomicUsize::new(0)),
            stopped: stopped.clone(),
            stop_gate: stop_gate.clone(),
            archive: None,
        }),
        Arc::new(testing::FixtureTranscriptionEngine::successful(
            "dictation",
            10,
        )),
        Arc::new(QaRuntime::default()),
    );
    backend.start().await.unwrap();
    let id = backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            ..DictationStartOptions::default()
        })
        .await
        .unwrap();
    let finishing = tokio::spawn({
        let backend = backend.clone();
        async move { backend.stop_dictation_session(id).await }
    });
    stopped.acquire().await.unwrap().forget();
    backend.cancel_dictation(Some(id)).await.unwrap();
    let next = SessionId::new();
    let while_stopping = backend.begin_less_computer_capture(next);
    finishing.abort();
    let _ = finishing.await;
    stop_gate.add_permits(1);
    assert_eq!(while_stopping.unwrap_err().code, BackendErrorCode::Busy);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if backend.begin_less_computer_capture(next).is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    backend.abort_less_computer_capture(next).unwrap();
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn selection_terminal_entries_close_host_capture_and_retain_native_cleanup_hold() {
    for entry in ["cancel", "general", "fault", "shutdown"] {
        let stopped = Arc::new(Semaphore::new(0));
        let stop_gate = Arc::new(Semaphore::new(0));
        let asr = Arc::new(testing::FixtureTranscriptionEngine::successful(
            "instruction",
            10,
        ));
        let (backend, path) = backend(
            Arc::new(Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: stopped.clone(),
                stop_gate: stop_gate.clone(),
                archive: None,
            }),
            asr.clone(),
            Arc::new(QaRuntime::default()),
        );
        backend.start().await.unwrap();
        let id = backend
            .services()
            .selection_voice
            .begin(SelectionCapture {
                text: "selected text".into(),
                source_app: None,
            })
            .await
            .unwrap();
        let host = Arc::new(SelectionHostControl::default());
        *host.owner.lock().unwrap() = Some(id);
        *host.capture.lock().unwrap() = Some(Arc::new(
            backend
                .start_selection_voice_capture(id, host.clone())
                .await
                .unwrap(),
        ));
        match entry {
            "cancel" => backend
                .services()
                .selection_voice
                .cancel(Some(id))
                .await
                .unwrap(),
            "general" => backend.cancel_active_voice_session(None).await.unwrap(),
            "fault" => backend
                .services()
                .selection_voice
                .recording_fault(
                    id,
                    BackendError::new(BackendErrorCode::Platform, "device disconnected"),
                )
                .await
                .unwrap(),
            _ => backend.shutdown().await.unwrap(),
        }
        assert_eq!(
            *host.owner.lock().unwrap(),
            None,
            "{entry} must revoke the Host target"
        );
        assert!(
            host.capture.lock().unwrap().is_none(),
            "{entry} must transfer the native handle"
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), stopped.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy,
            "{entry} must keep the hold through native stop"
        );
        stop_gate.add_permits(1);
        let next = SessionId::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if backend.begin_less_computer_capture(next).is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(asr.cancel_count(), 1);
        backend
            .services()
            .selection_voice
            .cancel(Some(id))
            .await
            .unwrap();
        assert_eq!(host.cancellations.load(Ordering::SeqCst), 1);
        assert_eq!(backend.less_computer_active_session(), Some(next));
        backend.abort_less_computer_capture(next).unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn selection_cold_start_cancel_revokes_host_before_late_native_cleanup() {
    let entered = Arc::new(Semaphore::new(0));
    let start_gate = Arc::new(Semaphore::new(0));
    let stopped = Arc::new(Semaphore::new(0));
    let stop_gate = Arc::new(Semaphore::new(0));
    let (backend, path) = backend(
        Arc::new(SlowRecorder {
            inner: Recorder {
                starts: Arc::new(AtomicUsize::new(0)),
                stopped: stopped.clone(),
                stop_gate: stop_gate.clone(),
                archive: None,
            },
            entered: entered.clone(),
            gate: start_gate.clone(),
            progress: Arc::new(Mutex::new(None)),
        }),
        Arc::new(testing::FixtureTranscriptionEngine::successful("late", 10)),
        Arc::new(QaRuntime::default()),
    );
    backend.start().await.unwrap();
    let id = backend
        .services()
        .selection_voice
        .begin(SelectionCapture {
            text: "selected text".into(),
            source_app: None,
        })
        .await
        .unwrap();
    let host = Arc::new(SelectionHostControl::default());
    *host.owner.lock().unwrap() = Some(id);
    let starting = tokio::spawn({
        let backend = backend.clone();
        let host = host.clone();
        async move { backend.start_selection_voice_capture(id, host).await }
    });
    entered.acquire().await.unwrap().forget();
    backend.cancel_active_voice_session(Some(id)).await.unwrap();
    assert_eq!(*host.owner.lock().unwrap(), None);
    starting.abort();
    let _ = starting.await;
    start_gate.add_permits(1);
    stopped.acquire().await.unwrap().forget();
    assert_eq!(
        backend
            .begin_less_computer_capture(SessionId::new())
            .unwrap_err()
            .code,
        BackendErrorCode::Busy
    );
    stop_gate.add_permits(1);
    let next = SessionId::new();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if backend.begin_less_computer_capture(next).is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    backend.abort_less_computer_capture(next).unwrap();
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn qa_terminal_states_do_not_intercept_selection_voice_cancellation() {
    for phase in [QaPhase::Completed, QaPhase::Failed, QaPhase::Cancelled] {
        let qa = Arc::new(QaRuntime::default());
        let (backend, path) = backend(
            Arc::new(testing::FixtureAudioRecorder::default()),
            Arc::new(testing::FixtureTranscriptionEngine::successful(
                "unused", 10,
            )),
            qa.clone(),
        );
        backend.start().await.unwrap();
        qa.fail.store(phase == QaPhase::Failed, Ordering::SeqCst);
        let _ = backend.services().qa.submit_text("question".into()).await;
        if phase == QaPhase::Cancelled {
            backend.services().qa.cancel(None).await.unwrap();
        }
        let before = backend.services().qa.snapshot().await.unwrap();
        assert_eq!(before.phase, phase);
        let id = backend
            .services()
            .selection_voice
            .begin(SelectionCapture {
                text: "selected text".into(),
                source_app: None,
            })
            .await
            .unwrap();
        backend.cancel_active_voice_session(None).await.unwrap();
        assert_eq!(
            backend
                .services()
                .selection_voice
                .snapshot()
                .await
                .unwrap()
                .phase,
            SelectionVoicePhase::Cancelled,
            "QA {phase:?} must not intercept cancel"
        );
        assert_eq!(backend.services().qa.snapshot().await.unwrap(), before);
        assert_eq!(
            backend
                .services()
                .selection_voice
                .snapshot()
                .await
                .unwrap()
                .session_id,
            Some(id)
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn less_voice_finish_errors_publish_one_safe_error_but_cancellation_does_not() {
    for (asr, cancelled, stop_fails) in [
        (
            testing::FixtureTranscriptionEngine::failing(BackendError::new(
                BackendErrorCode::Provider,
                "HTTP 401 secret-api-key=https://private.example/token",
            )),
            false,
            false,
        ),
        (
            testing::FixtureTranscriptionEngine::successful("  ", 10),
            false,
            false,
        ),
        (
            testing::FixtureTranscriptionEngine::successful("unused", 10),
            false,
            true,
        ),
        (
            testing::FixtureTranscriptionEngine::failing(BackendError::new(
                BackendErrorCode::Cancelled,
                "user cancelled",
            )),
            true,
            false,
        ),
    ] {
        let recorder: Arc<dyn AudioRecorder> = if stop_fails {
            Arc::new(StopFailure)
        } else {
            Arc::new(testing::FixtureAudioRecorder::default())
        };
        let (backend, path) = backend(recorder, Arc::new(asr), Arc::new(QaRuntime::default()));
        let mut events = backend.subscribe();
        let id = SessionId::new();
        let capture = backend
            .start_less_computer_voice(id, Arc::new(Control))
            .await
            .unwrap();
        let error = capture.finish().await.unwrap_err();
        assert!(!error.message.contains("secret-api-key"));
        assert!(!error.message.contains("private.example"));
        let errors = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event.kind {
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    kind: LessComputerEventKind::Error { message },
                    ..
                }) => {
                    assert_eq!(event.session_id, Some(id));
                    Some(message)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), usize::from(!cancelled));
        for error in errors {
            assert!(!error.contains("secret-api-key"));
            assert!(!error.contains("private.example"));
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}
