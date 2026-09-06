//! Tauri-owned resources for the framework-independent QA service.
//!
//! Session phase, cancellation semantics and the message log belong to
//! `openless-core::QaService`. This module only captures host context, owns the
//! selection/focus handles and translates Core results.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, DictationContext, DictationStartOptions, QaInput, QaProgress,
    QaProgressSink, QaRuntimeAdapter, QaRuntimeCompletion, QaTurnRequest, QaTurnResult,
    RecordingProgressSink, SelectionCapture, SessionId,
};
use parking_lot::Mutex;

use crate::core_adapters::{AppHandleSlot, BackendSlot};

pub(crate) type SelectionVoiceTargetBinder = Arc<
    dyn Fn(
            openless_core::SessionId,
            crate::selection::SelectionInsertionTarget,
        ) -> Result<(), String>
        + Send
        + Sync,
>;

#[derive(Default)]
pub(crate) struct TauriQaHostContext {
    focus_target: Mutex<Option<usize>>,
    front_app: Mutex<Option<String>>,
    panel_visible: AtomicBool,
    selection_voice_target_binder: Mutex<Option<SelectionVoiceTargetBinder>>,
}

impl TauriQaHostContext {
    pub(crate) fn is_panel_visible(&self) -> bool {
        self.panel_visible.load(Ordering::Acquire)
    }

    pub(crate) fn prepare_show(&self) {
        let was_visible = self.panel_visible.swap(true, Ordering::AcqRel);
        if let Some(target) = crate::coordinator::capture_external_focus_target() {
            *self.focus_target.lock() = Some(target);
        } else if !was_visible {
            *self.focus_target.lock() = crate::coordinator::capture_focus_target();
        }
        if let Some(front_app) = crate::coordinator::capture_frontmost_app() {
            *self.front_app.lock() = Some(front_app);
        }
    }

    pub(crate) fn clear(&self) {
        self.panel_visible.store(false, Ordering::Release);
        *self.focus_target.lock() = None;
        *self.front_app.lock() = None;
    }

    /// Bind the narrow host operation needed to attach an opaque selection
    /// insertion target to a Core-owned preview. The QA adapter must not look
    /// up `Coordinator` through Tauri managed state; the coordinator installs
    /// this host-scoped callback over shared opaque-target state during setup.
    pub(crate) fn set_selection_voice_target_binder(&self, binder: SelectionVoiceTargetBinder) {
        *self.selection_voice_target_binder.lock() = Some(binder);
    }

    fn bind_selection_voice_target(
        &self,
        session_id: openless_core::SessionId,
        insertion_target: crate::selection::SelectionInsertionTarget,
    ) -> Result<(), BackendError> {
        let binder = self
            .selection_voice_target_binder
            .lock()
            .clone()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Unsupported,
                    "selection voice target binding is unavailable",
                )
            })?;
        binder(session_id, insertion_target)
            .map_err(|message| BackendError::new(BackendErrorCode::Platform, message))
    }

    fn capture_turn(&self, app: &AppHandleSlot) -> TauriQaHostCapture {
        if let Some(target) = crate::coordinator::capture_external_focus_target() {
            *self.focus_target.lock() = Some(target);
        }
        let _ = crate::coordinator::restore_focus_target_if_possible(*self.focus_target.lock());
        let (selection, selection_target) = crate::selection::resolve_selection_workspace_capture();
        if let Some(app) = app.lock().clone() {
            crate::refocus_qa_window(&app);
        }
        let selection_text = selection.as_ref().map(|selection| selection.text.clone());
        let front_app = selection
            .and_then(|selection| selection.source_app)
            .or_else(|| self.front_app.lock().clone());
        TauriQaHostCapture {
            selection_text,
            selection_target,
            front_app,
        }
    }
}

struct TauriQaHostCapture {
    selection_text: Option<String>,
    selection_target: crate::selection::SelectionInsertionTarget,
    front_app: Option<String>,
}

pub(crate) struct TauriQaRuntimeAdapter {
    app: AppHandleSlot,
    backend: BackendSlot,
    credentials: Arc<dyn openless_core::CredentialStore>,
    host_context: Arc<TauriQaHostContext>,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<TauriQaRuntimeSession>>>>,
}

struct TauriQaRuntimeSession {
    context: Mutex<Option<Arc<DictationContext>>>,
    // Keep the cancellation handle reachable while finish awaits the provider.
    voice_capture: Mutex<Option<Arc<openless_core::QaVoiceCaptureSession>>>,
    audio_wav: Mutex<Option<Vec<u8>>>,
    selection_text: Option<String>,
    selection_target: Mutex<Option<crate::selection::SelectionInsertionTarget>>,
    /// Selection Voice already bound this turn's opaque target before opening
    /// QA. In that path QA must preserve the target instead of recapturing the
    /// now-focused panel.
    prebound_selection_voice_session_id: Option<SessionId>,
    front_app: Option<String>,
    duration_ms: AtomicU64,
    voice_turn: bool,
    cancelled: Arc<AtomicBool>,
}

impl TauriQaRuntimeSession {
    fn context(&self) -> Result<Arc<DictationContext>, BackendError> {
        self.context.lock().clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "QA session context is not ready",
            )
        })
    }
}

struct TauriQaRecordingProgress {
    session_id: SessionId,
    progress: Arc<dyn QaProgressSink>,
}

impl RecordingProgressSink for TauriQaRecordingProgress {
    fn publish_level(&self, _elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        self.progress.publish(
            self.session_id,
            QaProgress::RecordingLevel(level.clamp(0.0, 1.0)),
        )
    }
}

impl TauriQaRuntimeAdapter {
    pub(crate) fn new(
        app: AppHandleSlot,
        backend: BackendSlot,
        credentials: Arc<dyn openless_core::CredentialStore>,
        host_context: Arc<TauriQaHostContext>,
    ) -> Self {
        Self {
            app,
            backend,
            credentials,
            host_context,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn backend(&self) -> Result<Arc<openless_core::OpenLessBackend>, BackendError> {
        self.backend
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "core backend state is unavailable",
                )
            })
    }

    fn insert_session(
        sessions: &Arc<Mutex<HashMap<SessionId, Arc<TauriQaRuntimeSession>>>>,
        session_id: SessionId,
        session: Arc<TauriQaRuntimeSession>,
    ) -> Result<(), BackendError> {
        let mut sessions = sessions.lock();
        if sessions.contains_key(&session_id) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "QA runtime session already exists",
            ));
        }
        sessions.insert(session_id, session);
        Ok(())
    }

    fn remove_if_current(
        sessions: &Arc<Mutex<HashMap<SessionId, Arc<TauriQaRuntimeSession>>>>,
        session_id: SessionId,
        expected: &Arc<TauriQaRuntimeSession>,
    ) {
        let mut sessions = sessions.lock();
        if sessions
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            sessions.remove(&session_id);
        }
    }

    async fn capture_session(
        &self,
        session_id: SessionId,
    ) -> Result<Arc<TauriQaRuntimeSession>, BackendError> {
        let capture = self.host_context.capture_turn(&self.app);
        let session = Arc::new(TauriQaRuntimeSession {
            context: Mutex::new(None),
            voice_capture: Mutex::new(None),
            audio_wav: Mutex::new(None),
            selection_text: capture.selection_text,
            selection_target: Mutex::new(Some(capture.selection_target)),
            prebound_selection_voice_session_id: None,
            front_app: capture.front_app,
            duration_ms: AtomicU64::new(0),
            voice_turn: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        // Publish the owner before context capture can yield. Cancellation
        // must be able to revoke this turn even before any resource is ready.
        Self::insert_session(&self.sessions, session_id, Arc::clone(&session))?;
        self.prepare_context(session_id, &session).await?;
        Ok(session)
    }

    async fn prepare_context(
        &self,
        session_id: SessionId,
        session: &Arc<TauriQaRuntimeSession>,
    ) -> Result<(), BackendError> {
        let result = async {
            let context = self
                .backend()?
                .capture_host_dictation_context(DictationStartOptions {
                    front_app: session.front_app.clone(),
                    ..DictationStartOptions::default()
                })
                .await?;
            // Checking identity and installing under the same registry lock
            // prevents a late await from resurrecting a removed/replaced turn.
            let sessions = self.sessions.lock();
            if session.cancelled.load(Ordering::Acquire)
                || !sessions
                    .get(&session_id)
                    .is_some_and(|current| Arc::ptr_eq(current, session))
            {
                return Err(Self::cancelled_error());
            }
            *session.context.lock() = Some(context);
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = self.cancel(session_id).await;
        }
        result
    }

    fn cancelled_error() -> BackendError {
        BackendError::new(
            BackendErrorCode::Cancelled,
            "QA runtime session was cancelled",
        )
    }
}

impl QaRuntimeAdapter for TauriQaRuntimeAdapter {
    fn prepare_text(
        &self,
        session_id: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        let adapter = self.clone();
        Box::pin(async move {
            let session = adapter.capture_session(session_id).await?;
            Ok(QaInput {
                text,
                selection_text: session.selection_text.clone(),
                selection_source_app: session.front_app.clone(),
            })
        })
    }

    fn prepare_selection_edit(
        &self,
        session_id: SessionId,
        selection_voice_session_id: SessionId,
        capture: SelectionCapture,
        instruction: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        let adapter = self.clone();
        Box::pin(async move {
            // Do not call `capture_turn`: opening QA has already changed focus,
            // while Selection Voice still owns the original opaque insertion
            // target in the coordinator's host state.
            let session = Arc::new(TauriQaRuntimeSession {
                context: Mutex::new(None),
                voice_capture: Mutex::new(None),
                audio_wav: Mutex::new(None),
                selection_text: Some(capture.text.clone()),
                selection_target: Mutex::new(None),
                prebound_selection_voice_session_id: Some(selection_voice_session_id),
                front_app: capture.source_app.clone(),
                duration_ms: AtomicU64::new(0),
                voice_turn: false,
                cancelled: Arc::new(AtomicBool::new(false)),
            });
            Self::insert_session(&adapter.sessions, session_id, Arc::clone(&session))?;
            adapter.prepare_context(session_id, &session).await?;
            Ok(QaInput {
                text: instruction,
                selection_text: Some(capture.text),
                selection_source_app: capture.source_app,
            })
        })
    }

    fn start_recording(
        &self,
        session_id: SessionId,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let adapter = self.clone();
        Box::pin(async move {
            let capture = adapter.host_context.capture_turn(&adapter.app);
            let session = Arc::new(TauriQaRuntimeSession {
                context: Mutex::new(None),
                voice_capture: Mutex::new(None),
                audio_wav: Mutex::new(None),
                selection_text: capture.selection_text,
                selection_target: Mutex::new(Some(capture.selection_target)),
                prebound_selection_voice_session_id: None,
                front_app: capture.front_app.clone(),
                duration_ms: AtomicU64::new(0),
                voice_turn: true,
                cancelled: Arc::new(AtomicBool::new(false)),
            });
            Self::insert_session(&adapter.sessions, session_id, Arc::clone(&session))?;
            if let Err(error) = progress.publish(
                session_id,
                QaProgress::SelectionCaptured(session.selection_text.clone()),
            ) {
                Self::remove_if_current(&adapter.sessions, session_id, &session);
                return Err(error);
            }
            let backend = match adapter.backend() {
                Ok(backend) => backend,
                Err(error) => {
                    Self::remove_if_current(&adapter.sessions, session_id, &session);
                    return Err(error);
                }
            };
            let voice_capture = match backend
                .start_qa_voice_capture(
                    session_id,
                    DictationStartOptions {
                        front_app: capture.front_app,
                        ..DictationStartOptions::default()
                    },
                    Arc::new(TauriQaRecordingProgress {
                        session_id,
                        progress,
                    }),
                )
                .await
            {
                Ok(capture) => capture,
                Err(error) => {
                    Self::remove_if_current(&adapter.sessions, session_id, &session);
                    return Err(error);
                }
            };
            let voice_capture = Arc::new(voice_capture);
            let installed = {
                let sessions = adapter.sessions.lock();
                if session.cancelled.load(Ordering::Acquire)
                    || !sessions
                        .get(&session_id)
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    false
                } else {
                    *session.context.lock() = Some(voice_capture.context());
                    *session.voice_capture.lock() = Some(Arc::clone(&voice_capture));
                    true
                }
            };
            if !installed {
                // Cancel can win while cpal/provider startup is awaiting. The
                // returned capture was never installed, so this path owns it.
                let _ = voice_capture.cancel().await;
                return Err(Self::cancelled_error());
            }
            // Core may have queued an immediate device fault while cpal was
            // starting. Arm only after the capture is reachable by cancel or
            // finish, otherwise the terminal action could race this install.
            voice_capture.arm_recording_progress();
            if session.cancelled.load(Ordering::Acquire) {
                let _ = voice_capture.cancel().await;
                return Err(Self::cancelled_error());
            }
            Ok(())
        })
    }

    fn finish_recording(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        let session = self.sessions.lock().get(&session_id).cloned();
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "QA runtime session is no longer active",
                )
            })?;
            // Retain an Arc in the registry until complete/cancel; finishing
            // the microphone is not the end of the provider request's lease.
            let voice_capture = session.voice_capture.lock().clone().ok_or_else(|| {
                BackendError::new(BackendErrorCode::InvalidState, "QA recording is not ready")
            })?;
            let result = voice_capture.finish().await?;
            let sessions = sessions.lock();
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
            *session.audio_wav.lock() = result.audio_wav;
            Ok(QaInput {
                text: result
                    .transcript
                    .unwrap_or_else(|| "（语音问题）".to_string()),
                selection_text: session.selection_text.clone(),
                selection_source_app: session.front_app.clone(),
            })
        })
    }

    fn answer(
        &self,
        request: QaTurnRequest,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
        let session = self.sessions.lock().get(&request.session_id).cloned();
        let credentials = Arc::clone(&self.credentials);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "QA runtime session is no longer active",
                )
            })?;
            if session.cancelled.load(Ordering::Acquire) {
                return Err(Self::cancelled_error());
            }
            let context = session.context()?;
            let audio_wav = session.audio_wav.lock().take();
            let answer = openless_core::answer_qa_with_context(
                credentials,
                context,
                request.messages,
                audio_wav,
                request.session_id,
                progress,
                Arc::clone(&session.cancelled),
            )
            .await?;
            let sessions = sessions.lock();
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
        let session = self
            .sessions
            .lock()
            .get(&qa_session_id)
            .cloned()
            .ok_or_else(Self::cancelled_error)?;
        if let Some(expected) = session.prebound_selection_voice_session_id {
            return if expected == selection_voice_session_id {
                Ok(())
            } else {
                Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "selection edit target belongs to a stale Selection Voice session",
                ))
            };
        }
        let target = session.selection_target.lock().take().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Platform,
                "selection edit target is unavailable",
            )
        })?;
        self.host_context
            .bind_selection_voice_target(selection_voice_session_id, target)
    }

    fn complete(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaRuntimeCompletion, BackendError>> {
        let session = self.sessions.lock().remove(&session_id);
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "QA runtime session is no longer active",
                )
            })?;
            let context = session.context()?;
            Ok(QaRuntimeCompletion {
                duration_ms: session
                    .voice_turn
                    .then(|| session.duration_ms.load(Ordering::Acquire)),
                front_app: session.front_app.clone(),
                raw_transcript_override: (session.voice_turn
                    && context.pipeline_mode
                        == openless_core::shared_types::PipelineMode::Multimodal)
                    .then(String::new),
            })
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = {
            let mut sessions = self.sessions.lock();
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
            // Clear host-only data as soon as cancellation wins, even when a
            // prepare/finish future still retains this session Arc.
            session.context.lock().take();
            session.audio_wav.lock().take();
            session.selection_target.lock().take();
            let voice_capture = session.voice_capture.lock().take();
            match voice_capture {
                Some(voice_capture) => voice_capture.cancel().await,
                None => Ok(()),
            }
        })
    }
}

impl Clone for TauriQaRuntimeAdapter {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            backend: Arc::clone(&self.backend),
            credentials: Arc::clone(&self.credentials),
            host_context: Arc::clone(&self.host_context),
            sessions: Arc::clone(&self.sessions),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_during_selection_context_capture_cannot_reinstall_the_turn() {
        struct PendingContext {
            entered: Arc<tokio::sync::Semaphore>,
            gate: Arc<tokio::sync::Semaphore>,
        }
        impl openless_core::HostContextAdapter for PendingContext {
            fn capture(
                &self,
                _include_cursor: bool,
            ) -> BoxFuture<'static, Result<openless_core::HostContextCapture, BackendError>>
            {
                let entered = self.entered.clone();
                let gate = self.gate.clone();
                Box::pin(async move {
                    entered.add_permits(1);
                    gate.acquire().await.unwrap().forget();
                    Ok(openless_core::HostContextCapture::default())
                })
            }
        }
        let pending = Arc::new(PendingContext {
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
        });
        let data_dir =
            std::env::temp_dir().join(format!("openless-tauri-qa-prepare-{}", SessionId::new()));
        let mut dependencies = openless_core::BackendDependencies::unsupported();
        dependencies.services.host_context = pending.clone();
        let backend = Arc::new(
            openless_core::OpenLessBackend::new(
                openless_core::BackendConfig {
                    data_dir: data_dir.clone(),
                    ..Default::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = true;
        backend
            .update_settings(
                preferences,
                openless_core::SettingsUpdateOptions::STRICT,
                &openless_core::NoopSettingsRuntime,
            )
            .unwrap();
        let adapter = TauriQaRuntimeAdapter::new(
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(Some(Arc::downgrade(&backend)))),
            Arc::new(openless_core::UnsupportedCredentialStore),
            Arc::new(TauriQaHostContext::default()),
        );
        let session_id = SessionId::new();
        let preparing = tokio::spawn(adapter.prepare_selection_edit(
            session_id,
            SessionId::new(),
            SelectionCapture {
                text: "original selection".into(),
                source_app: Some("Editor".into()),
            },
            "shorten".into(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), pending.entered.acquire())
            .await
            .expect("host context capture must begin")
            .unwrap()
            .forget();
        assert!(adapter.sessions.lock().contains_key(&session_id));
        adapter.cancel(session_id).await.unwrap();
        pending.gate.add_permits(1);
        assert_eq!(
            preparing.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(adapter.sessions.lock().is_empty());
        drop(backend);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    fn runtime_session() -> Arc<TauriQaRuntimeSession> {
        Arc::new(TauriQaRuntimeSession {
            context: Mutex::new(Some(Arc::new(DictationContext::default()))),
            voice_capture: Mutex::new(None),
            audio_wav: Mutex::new(None),
            selection_text: None,
            selection_target: Mutex::new(Some(Default::default())),
            prebound_selection_voice_session_id: None,
            front_app: None,
            duration_ms: AtomicU64::new(0),
            voice_turn: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    #[test]
    fn duplicate_session_is_rejected_without_replacing_the_original_owner() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let session_id = SessionId::new();
        let original = runtime_session();
        let replacement = runtime_session();

        TauriQaRuntimeAdapter::insert_session(&sessions, session_id, Arc::clone(&original))
            .unwrap();
        let error =
            TauriQaRuntimeAdapter::insert_session(&sessions, session_id, replacement).unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Busy);
        assert!(Arc::ptr_eq(
            sessions.lock().get(&session_id).unwrap(),
            &original
        ));
    }

    #[test]
    fn host_visibility_follows_show_and_clear() {
        let context = TauriQaHostContext::default();
        assert!(!context.is_panel_visible());

        context.prepare_show();
        assert!(context.is_panel_visible());

        context.clear();
        assert!(!context.is_panel_visible());
    }

    #[test]
    fn selection_target_binding_uses_the_narrow_host_callback() {
        let context = TauriQaHostContext::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        context.set_selection_voice_target_binder(Arc::new(move |session_id, _target| {
            observed.lock().push(session_id);
            Ok(())
        }));

        let session_id = openless_core::SessionId::new();
        context
            .bind_selection_voice_target(session_id, Default::default())
            .unwrap();

        assert_eq!(*calls.lock(), vec![session_id]);
    }

    #[test]
    fn prebound_selection_voice_target_is_preserved_and_stale_ids_are_rejected() {
        let host_context = Arc::new(TauriQaHostContext::default());
        let adapter = TauriQaRuntimeAdapter::new(
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(openless_core::UnsupportedCredentialStore),
            host_context,
        );
        let qa_session_id = SessionId::new();
        let selection_voice_session_id = SessionId::new();
        let session = Arc::new(TauriQaRuntimeSession {
            context: Mutex::new(Some(Arc::new(DictationContext::default()))),
            voice_capture: Mutex::new(None),
            audio_wav: Mutex::new(None),
            selection_text: Some("original selection".to_string()),
            prebound_selection_voice_session_id: Some(selection_voice_session_id),
            selection_target: Mutex::new(None),
            front_app: Some("Editor".to_string()),
            duration_ms: AtomicU64::new(0),
            voice_turn: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        adapter.sessions.lock().insert(qa_session_id, session);

        adapter
            .bind_selection_voice_target(qa_session_id, selection_voice_session_id)
            .unwrap();
        let error = adapter
            .bind_selection_voice_target(qa_session_id, SessionId::new())
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
    }
}
