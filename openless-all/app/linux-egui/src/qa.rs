use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, CredentialStore, DictationContext, DictationStartOptions,
    OpenLessBackend, QaInput, QaProgress, QaProgressSink, QaRuntimeAdapter, QaRuntimeCompletion,
    QaTurnRequest, QaTurnResult, RecordingProgressSink, SessionId,
};

pub(crate) type LinuxBackendSlot = Arc<Mutex<Weak<OpenLessBackend>>>;

pub(crate) fn backend_slot() -> LinuxBackendSlot {
    Arc::new(Mutex::new(Weak::new()))
}

pub(crate) fn bind_backend(slot: &LinuxBackendSlot, backend: &Arc<OpenLessBackend>) {
    *slot.lock().expect("Linux backend slot lock poisoned") = Arc::downgrade(backend);
}

#[derive(Clone)]
pub struct LinuxQaRuntime {
    // The backend owns this adapter through QaService. A Weak slot breaks that
    // ownership cycle while still letting host effects reuse Core's canonical
    // context/audio entry points after construction has completed.
    backend: LinuxBackendSlot,
    credentials: Arc<dyn CredentialStore>,
    // Core owns the QA phase, Busy rule and terminal event. This table owns
    // only opaque Host resources that must survive across async adapter calls.
    sessions: Arc<Mutex<HashMap<SessionId, Arc<LinuxQaSession>>>>,
}

struct LinuxQaSession {
    context: Mutex<Option<Arc<DictationContext>>>,
    // Finish borrows this shared handle; cancel retains provider access until
    // the entire turn completes, including an in-flight final ASR response.
    voice_capture: Mutex<Option<Arc<openless_core::QaVoiceCaptureSession>>>,
    audio_wav: Mutex<Option<Vec<u8>>>,
    selection_text: Option<String>,
    duration_ms: AtomicU64,
    voice_turn: bool,
    cancelled: Arc<AtomicBool>,
}

impl LinuxQaSession {
    fn context(&self) -> Result<Arc<DictationContext>, BackendError> {
        self.context
            .lock()
            .expect("Linux QA context lock poisoned")
            .clone()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Linux QA session context is unavailable",
                )
            })
    }
}

struct LinuxQaRecordingProgress {
    session_id: SessionId,
    progress: Arc<dyn QaProgressSink>,
}

impl RecordingProgressSink for LinuxQaRecordingProgress {
    fn publish_level(&self, _elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        self.progress.publish(
            self.session_id,
            QaProgress::RecordingLevel(level.clamp(0.0, 1.0)),
        )
    }
}

impl LinuxQaRuntime {
    pub(crate) fn new(backend: LinuxBackendSlot, credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            backend,
            credentials,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn backend(&self) -> Result<Arc<OpenLessBackend>, BackendError> {
        self.backend
            .lock()
            .expect("Linux backend slot lock poisoned")
            .upgrade()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Linux Core backend is not bound yet",
                )
            })
    }

    fn selection_text(session_id: SessionId) -> Option<String> {
        crate::fcitx5::capture_selection_target(&session_id.to_string())
            .ok()
            .filter(|text| !text.trim().is_empty())
    }

    fn insert_session(
        &self,
        session_id: SessionId,
        session: Arc<LinuxQaSession>,
    ) -> Result<(), BackendError> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("Linux QA session lock poisoned");
        if sessions.contains_key(&session_id) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "Linux QA runtime session already exists",
            ));
        }
        sessions.insert(session_id, session);
        Ok(())
    }

    fn session(&self, session_id: SessionId) -> Result<Arc<LinuxQaSession>, BackendError> {
        self.sessions
            .lock()
            .expect("Linux QA session lock poisoned")
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Linux QA runtime session is no longer active",
                )
            })
    }

    fn remove(&self, session_id: SessionId) -> Option<Arc<LinuxQaSession>> {
        self.sessions
            .lock()
            .expect("Linux QA session lock poisoned")
            .remove(&session_id)
    }

    async fn capture_text_session(
        &self,
        session_id: SessionId,
    ) -> Result<Arc<LinuxQaSession>, BackendError> {
        let session = Arc::new(LinuxQaSession {
            context: Mutex::new(None),
            voice_capture: Mutex::new(None),
            audio_wav: Mutex::new(None),
            selection_text: Self::selection_text(session_id),
            duration_ms: AtomicU64::new(0),
            voice_turn: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        // Register before the first await so cancel owns the selection target
        // and can revoke preparation while host context capture is pending.
        self.insert_session(session_id, Arc::clone(&session))?;
        let result = async {
            let context = self
                .backend()?
                .capture_host_dictation_context(DictationStartOptions::default())
                .await?;
            let sessions = self
                .sessions
                .lock()
                .expect("Linux QA session lock poisoned");
            if session.cancelled.load(Ordering::Acquire)
                || !sessions
                    .get(&session_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                return Err(Self::cancelled_error());
            }
            *session
                .context
                .lock()
                .expect("Linux QA context lock poisoned") = Some(context);
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let _ = self.cancel(session_id).await;
            return Err(error);
        }
        Ok(session)
    }

    fn cancelled_error() -> BackendError {
        BackendError::new(
            BackendErrorCode::Cancelled,
            "Linux QA runtime session was cancelled",
        )
    }
}

impl QaRuntimeAdapter for LinuxQaRuntime {
    fn prepare_text(
        &self,
        session_id: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        let runtime = self.clone();
        Box::pin(async move {
            let session = runtime.capture_text_session(session_id).await?;
            Ok(QaInput {
                text,
                selection_text: session.selection_text.clone(),
                selection_source_app: None,
            })
        })
    }

    fn start_recording(
        &self,
        session_id: SessionId,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let runtime = self.clone();
        Box::pin(async move {
            let selection_text = Self::selection_text(session_id);
            let session = Arc::new(LinuxQaSession {
                context: Mutex::new(None),
                voice_capture: Mutex::new(None),
                audio_wav: Mutex::new(None),
                selection_text: selection_text.clone(),
                duration_ms: AtomicU64::new(0),
                voice_turn: true,
                cancelled: Arc::new(AtomicBool::new(false)),
            });
            runtime.insert_session(session_id, Arc::clone(&session))?;
            let capture = match async {
                progress.publish(session_id, QaProgress::SelectionCaptured(selection_text))?;
                runtime
                    .backend()?
                    .start_qa_voice_capture(
                        session_id,
                        DictationStartOptions::default(),
                        Arc::new(LinuxQaRecordingProgress {
                            session_id,
                            progress,
                        }),
                    )
                    .await
            }
            .await
            {
                Ok(capture) => capture,
                Err(error) => {
                    let _ = runtime.cancel(session_id).await;
                    return Err(error);
                }
            };
            let capture = Arc::new(capture);
            let installed = {
                let sessions = runtime
                    .sessions
                    .lock()
                    .expect("Linux QA session lock poisoned");
                if session.cancelled.load(Ordering::Acquire)
                    || !sessions
                        .get(&session_id)
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    false
                } else {
                    *session
                        .context
                        .lock()
                        .expect("Linux QA context lock poisoned") = Some(capture.context());
                    *session
                        .voice_capture
                        .lock()
                        .expect("Linux QA voice capture lock poisoned") =
                        Some(Arc::clone(&capture));
                    true
                }
            };
            if !installed {
                // Startup may return after cancel removed the owner. Release
                // this late capture directly instead of reviving the turn.
                let _ = capture.cancel().await;
                return Err(Self::cancelled_error());
            }
            // Audio startup may already have queued silence or a Fatal event.
            // Release it only now, when stop/cancel can reach the capture.
            capture.arm_recording_progress();
            if session.cancelled.load(Ordering::Acquire) {
                let _ = capture.cancel().await;
                return Err(Self::cancelled_error());
            }
            Ok(())
        })
    }

    fn finish_recording(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        let session = self.session(session_id);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let session = session?;
            // Keep the shared handle registered throughout provider finish.
            // Core claims finish once; cancel can still abort the same ASR.
            let capture = session
                .voice_capture
                .lock()
                .expect("Linux QA voice capture lock poisoned")
                .clone()
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "Linux QA recording is not ready",
                    )
                })?;
            let result = capture.finish().await?;
            let sessions = sessions.lock().expect("Linux QA session lock poisoned");
            if session.cancelled.load(Ordering::Acquire)
                || !sessions
                    .get(&session_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                return Err(Self::cancelled_error());
            }
            session
                .duration_ms
                .store(result.duration_ms, Ordering::Release);
            *session
                .audio_wav
                .lock()
                .expect("Linux QA audio lock poisoned") = result.audio_wav;
            Ok(QaInput {
                text: result
                    .transcript
                    .unwrap_or_else(|| "（语音问题）".to_string()),
                selection_text: session.selection_text.clone(),
                selection_source_app: None,
            })
        })
    }

    fn answer(
        &self,
        request: QaTurnRequest,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
        let session = self.session(request.session_id);
        let credentials = Arc::clone(&self.credentials);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let session = session?;
            if session.cancelled.load(Ordering::Acquire) {
                return Err(Self::cancelled_error());
            }
            let audio_wav = session
                .audio_wav
                .lock()
                .expect("Linux QA audio lock poisoned")
                .take();
            let answer = openless_core::answer_qa_with_context(
                credentials,
                session.context()?,
                request.messages,
                audio_wav,
                request.session_id,
                progress,
                Arc::clone(&session.cancelled),
            )
            .await?;
            let sessions = sessions.lock().expect("Linux QA session lock poisoned");
            if session.cancelled.load(Ordering::Acquire)
                || !sessions
                    .get(&request.session_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                return Err(Self::cancelled_error());
            }
            Ok(QaTurnResult { answer })
        })
    }

    fn bind_selection_voice_target(
        &self,
        qa_session_id: SessionId,
        selection_voice_session_id: SessionId,
    ) -> Result<(), BackendError> {
        crate::fcitx5::rekey_selection_target(
            &qa_session_id.to_string(),
            &selection_voice_session_id.to_string(),
        )
    }

    fn complete(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaRuntimeCompletion, BackendError>> {
        let session = self.remove(session_id);
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Linux QA runtime session is no longer active",
                )
            })?;
            let _ = crate::fcitx5::cancel_selection_target(&session_id.to_string());
            let context = session.context()?;
            Ok(QaRuntimeCompletion {
                duration_ms: session
                    .voice_turn
                    .then(|| session.duration_ms.load(Ordering::Acquire)),
                raw_transcript_override: (session.voice_turn
                    && context.pipeline_mode
                        == openless_core::shared_types::PipelineMode::Multimodal)
                    .then(String::new),
                ..QaRuntimeCompletion::default()
            })
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = {
            let mut sessions = self
                .sessions
                .lock()
                .expect("Linux QA session lock poisoned");
            let session = sessions.remove(&session_id);
            if let Some(session) = &session {
                session.cancelled.store(true, Ordering::Release);
            }
            session
        };
        Box::pin(async move {
            let Some(session) = session else {
                return Ok(());
            };
            let _ = crate::fcitx5::cancel_selection_target(&session_id.to_string());
            session
                .context
                .lock()
                .expect("Linux QA context lock poisoned")
                .take();
            session
                .audio_wav
                .lock()
                .expect("Linux QA audio lock poisoned")
                .take();
            let capture = session
                .voice_capture
                .lock()
                .expect("Linux QA voice capture lock poisoned")
                .take();
            match capture {
                Some(capture) => capture.cancel().await,
                None => Ok(()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::{AudioRecorder, BackendConfig, BackendDependencies, QaPhase};

    struct PendingRecorder {
        entered: Arc<tokio::sync::Semaphore>,
        gate: Arc<tokio::sync::Semaphore>,
        recorder: openless_core::testing::FixtureAudioRecorder,
        fatal: bool,
    }

    impl AudioRecorder for PendingRecorder {
        fn start(
            &self,
            session_id: SessionId,
            context: Arc<DictationContext>,
            consumer: Arc<dyn openless_core::AudioConsumer>,
            progress: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn openless_core::ActiveRecording>, BackendError>>
        {
            let entered = self.entered.clone();
            let gate = self.gate.clone();
            let recorder = self.recorder.clone();
            let fatal = self.fatal;
            Box::pin(async move {
                entered.add_permits(1);
                gate.acquire().await.unwrap().forget();
                if fatal {
                    progress.publish(openless_core::RecordingEvent::Fatal(BackendError::new(
                        BackendErrorCode::Platform,
                        "fixture microphone disconnected",
                    )))?;
                }
                recorder
                    .start(session_id, context, consumer, progress)
                    .await
            })
        }
    }

    fn voice_backend(
        recorder: Arc<PendingRecorder>,
    ) -> (
        Arc<OpenLessBackend>,
        Arc<LinuxQaRuntime>,
        std::path::PathBuf,
    ) {
        let runtime = Arc::new(LinuxQaRuntime::new(
            backend_slot(),
            Arc::new(openless_core::UnsupportedCredentialStore),
        ));
        let data_dir =
            std::env::temp_dir().join(format!("openless-linux-qa-lifecycle-{}", SessionId::new()));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.qa_runtime = Some(runtime.clone());
        dependencies.dictation_engine = Arc::new(openless_core::PipelineDictationEngine::new(
            recorder,
            Arc::new(
                openless_core::testing::FixtureTranscriptionEngine::successful("question", 100),
            ),
            Arc::new(openless_core::testing::FixtureTextPolisher::successful(
                "unused",
            )),
        ));
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.clone(),
                    ..Default::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        bind_backend(&runtime.backend, &backend);
        (backend, runtime, data_dir)
    }

    #[tokio::test]
    async fn cancelled_startup_closes_the_late_native_capture_once() {
        let recorder = Arc::new(PendingRecorder {
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
            recorder: openless_core::testing::FixtureAudioRecorder::default(),
            fatal: false,
        });
        let (backend, runtime, data_dir) = voice_backend(recorder.clone());
        let qa = backend.services().qa.clone();
        let starting = tokio::spawn(qa.toggle_recording());
        recorder.entered.acquire().await.unwrap().forget();
        let session_id = qa.snapshot().await.unwrap().session_id.unwrap();
        qa.cancel(Some(session_id)).await.unwrap();
        recorder.gate.add_permits(1);
        assert_eq!(
            starting.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert_eq!(recorder.recorder.stop_count(), 1);
        assert!(runtime.sessions.lock().unwrap().is_empty());
        assert_eq!(qa.snapshot().await.unwrap().phase, QaPhase::Cancelled);
        drop(backend);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn startup_silence_and_fatal_events_are_armed_after_capture_installation() {
        for fatal in [false, true] {
            let recorder = Arc::new(PendingRecorder {
                entered: Arc::new(tokio::sync::Semaphore::new(0)),
                gate: Arc::new(tokio::sync::Semaphore::new(1)),
                recorder: openless_core::testing::FixtureAudioRecorder::new(
                    vec![],
                    if fatal { vec![] } else { vec![(10_000, 0.0)] },
                ),
                fatal,
            });
            let (backend, runtime, data_dir) = voice_backend(recorder.clone());
            let mut preferences = backend.get_preferences();
            preferences.silence_auto_stop_enabled = true;
            preferences.hotkey.mode = openless_core::shared_types::HotkeyMode::Toggle;
            backend
                .update_settings(
                    preferences,
                    openless_core::SettingsUpdateOptions::STRICT,
                    &openless_core::NoopSettingsRuntime,
                )
                .unwrap();
            let qa = &backend.services().qa;
            qa.toggle_recording().await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    if qa.snapshot().await.unwrap().phase != QaPhase::Recording {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("queued startup terminal must be dispatched");
            assert_eq!(
                qa.snapshot().await.unwrap().phase,
                if fatal {
                    QaPhase::Failed
                } else {
                    QaPhase::Cancelled
                }
            );
            assert_eq!(recorder.recorder.stop_count(), 1);
            assert!(runtime.sessions.lock().unwrap().is_empty());
            drop(backend);
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }

    #[tokio::test]
    async fn cancelled_text_preparation_does_not_reinstall_host_context() {
        struct PendingContext(Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>);
        impl openless_core::HostContextAdapter for PendingContext {
            fn capture(
                &self,
                _include_cursor: bool,
            ) -> BoxFuture<'static, Result<openless_core::HostContextCapture, BackendError>>
            {
                let entered = self.0.clone();
                let gate = self.1.clone();
                Box::pin(async move {
                    entered.add_permits(1);
                    gate.acquire().await.unwrap().forget();
                    Ok(openless_core::HostContextCapture::default())
                })
            }
        }
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let runtime = Arc::new(LinuxQaRuntime::new(
            backend_slot(),
            Arc::new(openless_core::UnsupportedCredentialStore),
        ));
        let data_dir =
            std::env::temp_dir().join(format!("openless-linux-qa-context-{}", SessionId::new()));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.services.host_context =
            Arc::new(PendingContext(entered.clone(), gate.clone()));
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.clone(),
                    ..Default::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        bind_backend(&runtime.backend, &backend);
        let session_id = SessionId::new();
        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = true;
        backend
            .update_settings(
                preferences,
                openless_core::SettingsUpdateOptions::STRICT,
                &openless_core::NoopSettingsRuntime,
            )
            .unwrap();
        let preparing = tokio::spawn(runtime.prepare_text(session_id, "question".into()));
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.acquire())
            .await
            .expect("host context capture must begin")
            .unwrap()
            .forget();
        assert!(runtime.sessions.lock().unwrap().contains_key(&session_id));
        runtime.cancel(session_id).await.unwrap();
        gate.add_permits(1);
        assert_eq!(
            preparing.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(runtime.sessions.lock().unwrap().is_empty());
        drop(backend);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
