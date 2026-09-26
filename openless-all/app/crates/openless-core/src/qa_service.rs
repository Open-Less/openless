use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;

use crate::domains::{
    QaApi, QaInput, QaMessage, QaPhase, QaProgress, QaProgressSink, QaRuntimeAdapter, QaSnapshot,
    QaTurnRequest, SelectionVoiceApi,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{
    BackendEventKind, BackendEventPublisher, QaRecordingLevel, QaStateEvent, QaStateKind,
};
use crate::ports::{HostAction, HostActions};
use crate::types::{
    DictationSession, HistoryChange, HistoryInsertStatus, HistorySource, PolishMode, SessionId,
};
use crate::{Clock, HistoryStore, PreferencesStore};

#[derive(Default)]
struct QaState {
    snapshot: QaSnapshot,
}

enum QaSubmission {
    Text(String),
    ScopedText { text: String, expected_session: Option<SessionId> },
    Captured(QaInput),
    SelectionEdit {
        selection_voice_session_id: SessionId,
        capture: crate::domains::SelectionCapture,
        instruction: String,
    },
}

#[derive(Clone)]
pub struct QaService {
    runtime: Arc<dyn QaRuntimeAdapter>,
    host_actions: Arc<dyn HostActions>,
    events: Arc<Mutex<Option<BackendEventPublisher>>>,
    state: Arc<Mutex<QaState>>,
    // Serialize synchronous open/close transitions across native/UI threads.
    // Hosts may inspect snapshot() during a window action, so this is distinct
    // from the state mutex and is always released before native async cleanup.
    presentation: Arc<Mutex<()>>,
    persistence: Option<Arc<QaPersistence>>,
    selection_voice: Option<Arc<dyn SelectionVoiceApi>>,
    voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    runtime_work: Arc<crate::voice_session::RuntimeActivityGate>,
}

pub(crate) struct QaPersistence {
    preferences: Arc<PreferencesStore>,
    history: Arc<HistoryStore>,
    history_revision: Arc<AtomicU64>,
    clock: Arc<dyn Clock>,
}

impl QaPersistence {
    pub(crate) fn new(
        preferences: Arc<PreferencesStore>,
        history: Arc<HistoryStore>,
        history_revision: Arc<AtomicU64>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            preferences,
            history,
            history_revision,
            clock,
        }
    }
}

impl QaService {
    pub fn new(runtime: Arc<dyn QaRuntimeAdapter>, host_actions: Arc<dyn HostActions>) -> Self {
        Self {
            runtime,
            host_actions,
            events: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(QaState::default())),
            runtime_work: Arc::new(crate::voice_session::RuntimeActivityGate::default()),
            presentation: Arc::new(Mutex::new(())),
            persistence: None,
            selection_voice: None,
            voice_sessions: Arc::new(crate::voice_session::VoiceSessionGate::default()),
        }
    }

    pub(crate) fn new_with_persistence(
        runtime: Arc<dyn QaRuntimeAdapter>,
        host_actions: Arc<dyn HostActions>,
        persistence: QaPersistence,
        selection_voice: Arc<dyn SelectionVoiceApi>,
        voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    ) -> Self {
        Self {
            runtime,
            host_actions,
            events: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(QaState::default())),
            runtime_work: Arc::new(crate::voice_session::RuntimeActivityGate::default()),
            presentation: Arc::new(Mutex::new(())),
            persistence: Some(Arc::new(persistence)),
            selection_voice: Some(selection_voice),
            voice_sessions,
        }
    }

    fn begin_runtime_work(
        &self,
    ) -> Result<crate::voice_session::RuntimeActivityHold, BackendError> {
        let state = self.state.lock().expect("QA state lock poisoned");
        if matches!(
            state.snapshot.phase,
            QaPhase::Recording | QaPhase::Thinking | QaPhase::AwaitingApproval
        ) {
            Ok(self.runtime_work.existing_work())
        } else {
            self.runtime_work.acquire()
        }
    }

    fn progress_sink(&self) -> Arc<dyn QaProgressSink> {
        Arc::new(QaServiceProgress {
            state: Arc::clone(&self.state),
            events: self.event_publisher(),
        })
    }

    fn event_publisher(&self) -> BackendEventPublisher {
        self.events
            .lock()
            .expect("QA event publisher lock poisoned")
            .clone()
            .expect("QA service must be attached to an OpenLessBackend before use")
    }

    fn publish_snapshot(&self, kind: QaStateKind, expected: Option<(SessionId, QaPhase)>) {
        let state = self.state.lock().expect("QA state lock poisoned");
        if let Some((session_id, phase)) = expected {
            if state.snapshot.session_id != Some(session_id) || state.snapshot.phase != phase {
                return;
            }
        }
        // Keep the owner check and event publication together. Reading B after
        // an old A transition must not label B's snapshot with A's event kind.
        // Unscoped show/dismiss calls hold the presentation guard instead.
        publish_qa_snapshot(&self.event_publisher(), &state.snapshot, kind, None, None);
    }

    fn fail_if_current(&self, session_id: SessionId, error: &BackendError) {
        let message = public_qa_error(error);
        {
            let mut state = self.state.lock().expect("QA state lock poisoned");
            if state.snapshot.session_id != Some(session_id)
                || !matches!(
                    state.snapshot.phase,
                    QaPhase::Recording | QaPhase::Thinking | QaPhase::AwaitingApproval
                )
            {
                return;
            }
            state.snapshot.phase = QaPhase::Failed;
            state.snapshot.pending_approval_token = None;
            state.snapshot.last_error = Some(message.clone());
            state.snapshot.conversation_id = None;
            if state
                .snapshot
                .messages
                .last()
                .is_some_and(|message| message.role == "user")
            {
                state.snapshot.messages.pop();
            }
            publish_qa_snapshot(
                &self.event_publisher(),
                &state.snapshot,
                QaStateKind::Error,
                None,
                Some(message),
            );
        }
    }

    async fn begin_recording(&self) -> Result<(), BackendError> {
        let _runtime = self.begin_runtime_work()?;
        let session_id = SessionId::new();
        {
            let _presentation = self
                .presentation
                .lock()
                .expect("QA presentation lock poisoned");
            let previous = {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                ensure_qa_idle(&state.snapshot)?;
                self.voice_sessions
                    .acquire(session_id, crate::voice_session::VoiceSessionKind::Qa)?;
                let previous = state.snapshot.clone();
                let conversation_id = state.snapshot.conversation_id.unwrap_or(session_id);
                state.snapshot.phase = QaPhase::Recording;
                state.snapshot.session_id = Some(session_id);
                state.snapshot.conversation_id = Some(conversation_id);
                state.snapshot.pending_approval_token = None;
                state.snapshot.last_error = None;
                state.snapshot.selection_preview = None;
                state.snapshot.edit_apply_available = false;
                state.snapshot.edit_revert_available = false;
                previous
            };
            if let Err(error) = self.host_actions.request(HostAction::ShowQa) {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                if state.snapshot.session_id == Some(session_id)
                    && state.snapshot.phase == QaPhase::Recording
                {
                    state.snapshot = previous;
                }
                self.voice_sessions.release(session_id);
                return Err(error);
            }
            self.publish_snapshot(
                QaStateKind::Recording,
                Some((session_id, QaPhase::Recording)),
            );
        }
        if let Err(error) = self
            .runtime
            .start_recording(session_id, self.progress_sink())
            .await
        {
            self.fail_if_current(session_id, &error);
            self.cancel_runtime_best_effort(session_id).await;
            self.voice_sessions.release(session_id);
            return Err(public_qa_backend_error(&error));
        }
        Ok(())
    }

    async fn finish_recording(&self, session_id: SessionId) -> Result<(), BackendError> {
        let _runtime = {
            let mut state = self.state.lock().expect("QA state lock poisoned");
            // Validate the callback's generation and claim the finish under
            // one lock. A delayed silence event cannot toggle a completed turn
            // back on, stop its successor, or compete with a manual stop.
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
            state.snapshot.phase = QaPhase::Thinking;
            self.runtime_work.existing_work()
        };
        self.publish_snapshot(QaStateKind::Loading, Some((session_id, QaPhase::Thinking)));

        let input = match self.runtime.finish_recording(session_id).await {
            Ok(input) => input,
            Err(error) => {
                self.fail_if_current(session_id, &error);
                self.cancel_runtime_best_effort(session_id).await;
                self.voice_sessions.release(session_id);
                return Err(public_qa_backend_error(&error));
            }
        };
        let result = self.answer_input(session_id, input).await;
        self.voice_sessions.release(session_id);
        result
    }

    async fn submit_text_inner(&self, text: String) -> Result<(), BackendError> {
        self.submit_inner(QaSubmission::Text(text)).await
    }

    async fn submit_selection_edit_inner(
        &self,
        selection_voice_session_id: SessionId,
        capture: crate::domains::SelectionCapture,
        instruction: String,
    ) -> Result<(), BackendError> {
        self.submit_inner(QaSubmission::SelectionEdit {
            selection_voice_session_id,
            capture,
            instruction,
        })
        .await
    }

    async fn submit_inner(&self, submission: QaSubmission) -> Result<(), BackendError> {
        let _runtime = self.begin_runtime_work()?;
        let expected_session = match &submission {
            QaSubmission::ScopedText { expected_session, .. } => Some(*expected_session),
            _ => None,
        };
        let text = match &submission {
            QaSubmission::Text(text) => text,
            QaSubmission::ScopedText { text, .. } => text,
            QaSubmission::Captured(input) => &input.text,
            QaSubmission::SelectionEdit { instruction, .. } => instruction,
        }
        .trim()
        .to_string();
        if text.is_empty() {
            return Ok(());
        }
        let session_id = SessionId::new();
        {
            let _presentation = self
                .presentation
                .lock()
                .expect("QA presentation lock poisoned");
            let previous = {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                ensure_qa_idle(&state.snapshot)?;
                if expected_session.is_some_and(|expected| expected != state.snapshot.session_id) {
                    return Err(BackendError::new(BackendErrorCode::InvalidState, "qa_context_changed"));
                }
                let previous = state.snapshot.clone();
                let conversation_id = state.snapshot.conversation_id.unwrap_or(session_id);
                state.snapshot.phase = QaPhase::Thinking;
                state.snapshot.session_id = Some(session_id);
                state.snapshot.conversation_id = Some(conversation_id);
                state.snapshot.pending_approval_token = None;
                state.snapshot.last_error = None;
                state.snapshot.selection_preview = None;
                state.snapshot.edit_apply_available = false;
                state.snapshot.edit_revert_available = false;
                previous
            };
            if let Err(error) = self.host_actions.request(HostAction::ShowQa) {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                if state.snapshot.session_id == Some(session_id)
                    && state.snapshot.phase == QaPhase::Thinking
                {
                    state.snapshot = previous;
                }
                return Err(error);
            }
            self.publish_snapshot(QaStateKind::Loading, Some((session_id, QaPhase::Thinking)));
        }

        let prepared = match submission {
            QaSubmission::Text(_) | QaSubmission::ScopedText { .. } => self.runtime.prepare_text(session_id, text).await,
            QaSubmission::Captured(mut input) => {
                input.text = text;
                self.runtime.prepare_captured_text(session_id, input).await
            }
            QaSubmission::SelectionEdit {
                selection_voice_session_id,
                capture,
                ..
            } => {
                self.runtime
                    .prepare_selection_edit(session_id, selection_voice_session_id, capture, text)
                    .await
            }
        };
        let input = match prepared {
            Ok(input) => input,
            Err(error) => {
                self.fail_if_current(session_id, &error);
                self.cancel_runtime_best_effort(session_id).await;
                return Err(public_qa_backend_error(&error));
            }
        };
        self.answer_input(session_id, input).await
    }

    async fn answer_input(
        &self,
        session_id: SessionId,
        mut input: QaInput,
    ) -> Result<(), BackendError> {
        // A platform context/capture may finish preparing after cancellation.
        // Reject the stale generation and explicitly sweep the runtime again;
        // adapters make release idempotent, including a late resource install.
        let current = {
            let state = self.state.lock().expect("QA state lock poisoned");
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Thinking)
        };
        if let Err(error) = current {
            self.cancel_runtime_best_effort(session_id).await;
            return Err(error);
        }
        input.text = input.text.trim().to_string();
        if input.text.is_empty() {
            if let Err(error) = self.runtime.complete(session_id).await {
                log::warn!("failed to release empty QA runtime session: {error}");
                self.cancel_runtime_best_effort(session_id).await;
            }
            let mut state = self.state.lock().expect("QA state lock poisoned");
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Thinking)?;
            state.snapshot.phase = QaPhase::Completed;
            state.snapshot.selection_preview = None;
            drop(state);
            self.publish_snapshot(QaStateKind::Idle, Some((session_id, QaPhase::Completed)));
            return Ok(());
        }

        let (request, edit_instruction_mode) = {
            let mut state = self.state.lock().expect("QA state lock poisoned");
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Thinking)?;
            let conversation_id = state.snapshot.conversation_id.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "QA conversation owner is unavailable",
                )
            })?;
            let user_content = compose_qa_user_content(
                input.selection_text.as_deref().unwrap_or_default(),
                &input.text,
            );
            state.snapshot.selection_preview = input.selection_text.clone();
            state.snapshot.messages.push(QaMessage {
                id: SessionId::new().to_string(),
                role: "user".to_string(),
                content: user_content,
                selection_text: input.selection_text.clone(),
            });
            (
                QaTurnRequest {
                    session_id,
                    conversation_id,
                    input,
                    messages: state.snapshot.messages.clone(),
                },
                state.snapshot.edit_instruction_mode,
            )
        };
        self.publish_snapshot(QaStateKind::Thinking, Some((session_id, QaPhase::Thinking)));

        let history_input = request.input.clone();
        let turn_result = if edit_instruction_mode {
            let selection_text = request
                .input
                .selection_text
                .clone()
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "no selection is available for editing",
                    )
                });
            match (self.selection_voice.as_ref(), selection_text) {
                (Some(selection_voice), Ok(selection_text)) => selection_voice
                    .edit_preview(crate::domains::SelectionVoiceEditRequest {
                        owner_session_id: request.conversation_id,
                        capture: crate::domains::SelectionCapture {
                            text: selection_text,
                            source_app: request.input.selection_source_app.clone(),
                        },
                        instruction: request.input.text.clone(),
                    })
                    .await
                    .and_then(|result| {
                        self.runtime
                            .bind_selection_voice_target(session_id, result.preview.session_id)?;
                        Ok((result.answer_text(), true, result.replaced_existing))
                    }),
                (None, _) => Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "selection voice editing is unavailable",
                )),
                (_, Err(error)) => Err(error),
            }
        } else {
            self.runtime
                .answer(request.clone(), self.progress_sink())
                .await
                .map(|result| (result.answer, false, false))
        };
        let (answer, edit_apply_available, edit_revert_available) = match turn_result {
            Ok(result) => result,
            Err(error) => {
                self.fail_if_current(session_id, &error);
                self.cancel_runtime_best_effort(session_id).await;
                if edit_instruction_mode {
                    self.clear_edit_preview_best_effort(Some(request.conversation_id))
                        .await;
                }
                return Err(public_qa_backend_error(&error));
            }
        };
        let completion = match self.runtime.complete(session_id).await {
            Ok(completion) => completion,
            Err(error) => {
                log::warn!("failed to finalize QA runtime metadata: {error}");
                self.cancel_runtime_best_effort(session_id).await;
                Default::default()
            }
        };

        {
            let mut state = self.state.lock().expect("QA state lock poisoned");
            if state.snapshot.session_id != Some(session_id)
                || !matches!(
                    state.snapshot.phase,
                    QaPhase::Thinking | QaPhase::AwaitingApproval
                )
            {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "QA session is no longer active",
                ));
            }
            state.snapshot.messages.push(QaMessage {
                id: SessionId::new().to_string(),
                role: "assistant".to_string(),
                content: answer.clone(),
                selection_text: None,
            });
            state.snapshot.phase = QaPhase::Completed;
            state.snapshot.selection_preview = None;
            state.snapshot.pending_approval_token = None;
            state.snapshot.last_error = None;
            state.snapshot.edit_apply_available = edit_apply_available;
            state.snapshot.edit_revert_available = edit_revert_available;
            publish_qa_snapshot(
                &self.event_publisher(),
                &state.snapshot,
                QaStateKind::Answer,
                None,
                None,
            );
        }
        self.persist_history(history_input.text, &answer, completion);
        Ok(())
    }

    fn persist_history(
        &self,
        question: String,
        answer: &str,
        completion: crate::domains::QaRuntimeCompletion,
    ) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let preferences = persistence.preferences.get();
        if !preferences.qa_save_history {
            return;
        }
        let front = crate::shared_types::split_front_app_opt(completion.front_app.as_deref());
        let session = DictationSession {
            // One panel conversation can contain several history entries; each
            // entry therefore needs its own identifier even though the edit
            // preview owner remains stable across successful turns.
            id: SessionId::new().to_string(),
            created_at: persistence.clock.now_utc().to_rfc3339(),
            source: HistorySource::Voice,
            raw_transcript: completion.raw_transcript_override.unwrap_or(question),
            asr_transcript: None,
            final_text: answer.to_string(),
            mode: PolishMode::Raw,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: front.bundle_id,
            app_name: front.name,
            insert_status: HistoryInsertStatus::CopiedFallback,
            error_code: Some("qaSession".to_string()),
            duration_ms: completion.duration_ms,
            dictionary_entry_count: None,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: None,
            llm_model: None,
            pipeline_mode: None,
            asr_ms: None,
            polish_ms: None,
        };
        match persistence.history.append_with_retention(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            Ok(()) => {
                let revision = persistence.history_revision.fetch_add(1, Ordering::AcqRel) + 1;
                self.event_publisher().publish(
                    None,
                    BackendEventKind::HistoryChanged(HistoryChange { revision }),
                );
            }
            Err(error) => log::warn!("failed to persist QA history: {error}"),
        }
    }

    async fn cancel_inner(
        &self,
        requested_session_id: Option<SessionId>,
        clear: bool,
    ) -> Result<(), BackendError> {
        let service = self.clone();
        self.runtime_work
            .cleanup(Box::pin(async move {
                service
                    .cancel_inner_owned(requested_session_id, clear)
                    .await
            }))
            .await
    }

    async fn cancel_inner_owned(
        &self,
        requested_session_id: Option<SessionId>,
        clear: bool,
    ) -> Result<(), BackendError> {
        let (runtime_session_id, conversation_id, host_result) = {
            let _presentation = self
                .presentation
                .lock()
                .expect("QA presentation lock poisoned");
            let (runtime_session_id, conversation_id, publish_cancelled) = {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                let active_session_id = state.snapshot.session_id;
                if let Some(requested) = requested_session_id {
                    if Some(requested) != active_session_id {
                        return Err(BackendError::new(
                            BackendErrorCode::Cancelled,
                            "QA session is no longer active",
                        ));
                    }
                }
                if !clear
                    && matches!(state.snapshot.phase, QaPhase::Idle | QaPhase::Cancelled)
                    && active_session_id.is_none()
                {
                    return Ok(());
                }
                let publish_cancelled =
                    !matches!(state.snapshot.phase, QaPhase::Idle | QaPhase::Cancelled);
                let runtime_session_id = matches!(
                    state.snapshot.phase,
                    QaPhase::Recording | QaPhase::Thinking | QaPhase::AwaitingApproval
                )
                .then_some(active_session_id)
                .flatten();
                if publish_cancelled {
                    state.snapshot.phase = QaPhase::Cancelled;
                }
                state.snapshot.pending_approval_token = None;
                state.snapshot.last_error = None;
                let conversation_id = state.snapshot.conversation_id.take();
                (runtime_session_id, conversation_id, publish_cancelled)
            };
            if let Some(session_id) = requested_session_id.or(runtime_session_id) {
                self.voice_sessions.release(session_id);
            }
            if publish_cancelled {
                self.publish_snapshot(QaStateKind::Cancelled, None);
            }
            // Closing state, events and Hide form one synchronous presentation
            // transition. "Before await" alone is insufficient: another OS thread
            // could otherwise open B between these locks and have A erase/hide it.
            // Host actions remain outside the state lock so hosts can inspect QA.
            let host_result = if clear {
                {
                    let mut state = self.state.lock().expect("QA state lock poisoned");
                    state.snapshot = QaSnapshot::default();
                }
                self.publish_snapshot(QaStateKind::Idle, None);
                self.host_actions.request(HostAction::HideQa)
            } else {
                Ok(())
            };
            (runtime_session_id, conversation_id, host_result)
        };
        let runtime_result = if let Some(session_id) = runtime_session_id {
            self.runtime.cancel(session_id).await
        } else {
            Ok(())
        };
        if clear {
            // Only the captured conversation owner may lose its preview.
            self.clear_edit_preview_best_effort(conversation_id).await;
        }
        host_result?;
        runtime_result
    }

    async fn cancel_runtime_best_effort(&self, session_id: SessionId) {
        if let Err(error) = self
            .runtime_work
            .cleanup(self.runtime.cancel(session_id))
            .await
        {
            log::warn!("failed to release QA runtime session after an error: {error}");
        }
    }

    async fn clear_edit_preview_best_effort(&self, conversation_id: Option<SessionId>) {
        let (Some(selection_voice), Some(conversation_id)) =
            (&self.selection_voice, conversation_id)
        else {
            return;
        };
        let preview = match selection_voice.preview(Some(conversation_id)).await {
            Ok(preview) => preview,
            Err(error) if error.code == BackendErrorCode::Unsupported => return,
            Err(error) => {
                log::warn!("failed to query QA edit preview while dismissing: {error}");
                return;
            }
        };
        if let Some(preview) = preview {
            if let Err(error) = selection_voice.cancel(Some(preview.session_id)).await {
                log::warn!("failed to clear QA edit preview while dismissing: {error}");
            }
        }
    }
}

impl QaApi for QaService {
    fn bind_runtime_restore_guard(
        &self,
        guard: crate::domains::RuntimeRestoreGuard,
        spawner: Arc<dyn crate::config::TaskSpawner>,
    ) -> Result<(), BackendError> {
        self.runtime_work.bind(guard, spawner)
    }

    fn runtime_restore_idle(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            matches!(
                state.snapshot.phase,
                QaPhase::Idle | QaPhase::Completed | QaPhase::Cancelled | QaPhase::Failed
            ) && self.runtime_work.runtime_restore_idle()
        })
    }

    fn bind_event_publisher(&self, publisher: BackendEventPublisher) {
        *self
            .events
            .lock()
            .expect("QA event publisher lock poisoned") = Some(publisher);
    }

    fn show(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let _presentation = service
                .presentation
                .lock()
                .expect("QA presentation lock poisoned");
            service.host_actions.request(HostAction::ShowQa)?;
            service.publish_snapshot(QaStateKind::Idle, None);
            Ok(())
        })
    }

    fn snapshot(&self) -> BoxFuture<'static, Result<QaSnapshot, BackendError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            Ok(state
                .lock()
                .expect("QA state lock poisoned")
                .snapshot
                .clone())
        })
    }

    fn toggle_recording(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let snapshot = service
                .state
                .lock()
                .expect("QA state lock poisoned")
                .snapshot
                .clone();
            match (snapshot.phase, snapshot.session_id) {
                (QaPhase::Recording, Some(session_id)) => {
                    service.finish_recording(session_id).await
                }
                (QaPhase::Idle | QaPhase::Completed | QaPhase::Cancelled | QaPhase::Failed, _) => {
                    service.begin_recording().await
                }
                _ => Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "QA session is busy",
                )),
            }
        })
    }

    fn recording_fault(
        &self,
        session_id: SessionId,
        error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        self.runtime_work.cleanup(Box::pin(async move {
            {
                let state = service.state.lock().expect("QA state lock poisoned");
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
            }
            service.fail_if_current(session_id, &error);
            service.voice_sessions.release(session_id);
            service.runtime.cancel(session_id).await
        }))
    }

    fn stop_recording(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.finish_recording(session_id).await })
    }

    fn submit_text(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.submit_text_inner(text).await })
    }

    fn submit_text_in_context(&self, text: String, expected_session: Option<SessionId>) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.submit_inner(QaSubmission::ScopedText { text, expected_session }).await })
    }

    fn submit_captured_text(&self, input: QaInput) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.submit_inner(QaSubmission::Captured(input)).await })
    }

    fn submit_selection_edit(
        &self,
        selection_voice_session_id: SessionId,
        capture: crate::domains::SelectionCapture,
        instruction: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .submit_selection_edit_inner(selection_voice_session_id, capture, instruction)
                .await
        })
    }

    fn set_edit_instruction_mode(
        &self,
        enabled: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let _presentation = service
                .presentation
                .lock()
                .expect("QA presentation lock poisoned");
            let mut state = service.state.lock().expect("QA state lock poisoned");
            if matches!(
                state.snapshot.phase,
                QaPhase::Recording | QaPhase::Thinking | QaPhase::AwaitingApproval
            ) {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "QA mode cannot change during an active turn",
                ));
            }
            state.snapshot.edit_instruction_mode = enabled;
            // An edit-apply/revert completion can replace the last answer from
            // another thread. Do not emit a previously cloned messages array
            // after that newer answer event.
            publish_qa_snapshot_impl(
                &service.event_publisher(),
                &state.snapshot,
                QaStateKind::Answer,
                None,
                None,
                true,
            );
            Ok(())
        })
    }

    fn revert_edit_preview(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            {
                let mut state = service.state.lock().expect("QA state lock poisoned");
                // Lock order is QA state -> Selection Voice state. Selection
                // routing releases its own state before calling QA; the local
                // revert below performs no await or native callback.
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Completed)?;
                let owner = state.snapshot.conversation_id.ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "QA conversation owner is unavailable",
                    )
                })?;
                let message = state
                    .snapshot
                    .messages
                    .iter_mut()
                    .rev()
                    .find(|message| message.role == "assistant")
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidState,
                            "QA assistant answer is unavailable",
                        )
                    })?;
                let preview = service
                    .selection_voice
                    .as_ref()
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Unsupported,
                            "QA selection edit is unavailable",
                        )
                    })?
                    .revert_preview(Some(owner))?;
                message.content = preview.text;
                state.snapshot.edit_apply_available = true;
                state.snapshot.edit_revert_available = false;
                publish_qa_snapshot(
                    &service.event_publisher(),
                    &state.snapshot,
                    QaStateKind::Answer,
                    None,
                    None,
                );
            }
            Ok(())
        })
    }

    fn begin_edit_preview_apply(
        &self,
        session_id: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<crate::domains::SelectionVoiceApplyTicket, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let state = service.state.lock().expect("QA state lock poisoned");
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Completed)?;
            let owner = state.snapshot.conversation_id.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "QA conversation owner is unavailable",
                )
            })?;
            service
                .selection_voice
                .as_ref()
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::Unsupported,
                        "QA selection edit is unavailable",
                    )
                })?
                .begin_preview_apply(Some(owner), text)
        })
    }

    fn cancel(
        &self,
        session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.cancel_inner(session_id, false).await })
    }

    fn dismiss(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.cancel_inner(None, true).await })
    }

    fn dismiss_session(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.cancel_inner(Some(session_id), true).await })
    }
}

struct QaServiceProgress {
    state: Arc<Mutex<QaState>>,
    events: BackendEventPublisher,
}

impl QaProgressSink for QaServiceProgress {
    fn publish(&self, session_id: SessionId, progress: QaProgress) -> Result<(), BackendError> {
        // Linearize native progress with state changes, but do not take the
        // presentation mutex: a Host window action must not block audio levels.
        match progress {
            QaProgress::RecordingLevel(level) => {
                let state = self.state.lock().expect("QA state lock poisoned");
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
                let level = if level.is_finite() {
                    level.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::QaLevel(QaRecordingLevel {
                        session_id: session_id.to_string(),
                        level,
                    }),
                );
            }
            QaProgress::SelectionCaptured(selection) => {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
                state.snapshot.selection_preview = selection;
                publish_qa_snapshot(
                    &self.events,
                    &state.snapshot,
                    QaStateKind::Recording,
                    None,
                    None,
                );
            }
            QaProgress::AnswerDelta(chunk) => {
                let state = self.state.lock().expect("QA state lock poisoned");
                let snapshot = &state.snapshot;
                if snapshot.session_id != Some(session_id)
                    || !matches!(
                        snapshot.phase,
                        QaPhase::Thinking | QaPhase::AwaitingApproval
                    )
                {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "QA session is no longer active",
                    ));
                }
                publish_qa_snapshot(
                    &self.events,
                    snapshot,
                    QaStateKind::AnswerDelta,
                    Some(chunk),
                    None,
                );
            }
            QaProgress::AwaitingApproval { token } => {
                let mut state = self.state.lock().expect("QA state lock poisoned");
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Thinking)?;
                state.snapshot.phase = QaPhase::AwaitingApproval;
                state.snapshot.pending_approval_token = Some(token);
                publish_qa_snapshot(
                    &self.events,
                    &state.snapshot,
                    QaStateKind::AwaitingApproval,
                    None,
                    None,
                );
            }
        }
        Ok(())
    }
}

fn ensure_qa_idle(snapshot: &QaSnapshot) -> Result<(), BackendError> {
    if matches!(
        snapshot.phase,
        QaPhase::Recording | QaPhase::Thinking | QaPhase::AwaitingApproval
    ) {
        return Err(BackendError::new(
            BackendErrorCode::Busy,
            "QA session is busy",
        ));
    }
    Ok(())
}

fn ensure_current_phase(
    snapshot: &QaSnapshot,
    session_id: SessionId,
    phase: QaPhase,
) -> Result<(), BackendError> {
    if snapshot.session_id != Some(session_id) || snapshot.phase != phase {
        return Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "QA session is no longer active",
        ));
    }
    Ok(())
}

fn compose_qa_user_content(selection_text: &str, question: &str) -> String {
    if selection_text.trim().is_empty() {
        return question.to_string();
    }
    let safe_selection =
        crate::prompts::sanitize_for_xml_envelope(selection_text.trim(), "selected_text");
    format!("<selected_text>\n{safe_selection}\n</selected_text>\n\n# 我的问题\n{question}")
}

fn public_qa_error(error: &BackendError) -> String {
    let message = error.message.as_str();
    if message.contains("---model_output---") || message.contains("invalid EditPlan") {
        if message.starts_with("编辑方案解析失败") {
            return message.to_string();
        }
        return format!("编辑方案解析失败\n\n{message}");
    }
    match error.code {
        BackendErrorCode::PermissionDenied => "QA permission denied".to_string(),
        BackendErrorCode::Unsupported => "QA is unsupported by this host".to_string(),
        BackendErrorCode::Cancelled => "QA request was cancelled".to_string(),
        _ => "QA request failed".to_string(),
    }
}

fn public_qa_backend_error(error: &BackendError) -> BackendError {
    BackendError::new(error.code, public_qa_error(error)).retryable(error.retryable)
}

fn publish_qa_snapshot(
    events: &BackendEventPublisher,
    snapshot: &QaSnapshot,
    kind: QaStateKind,
    chunk: Option<String>,
    error: Option<String>,
) {
    publish_qa_snapshot_impl(events, snapshot, kind, chunk, error, false);
}

fn publish_qa_snapshot_impl(
    events: &BackendEventPublisher,
    snapshot: &QaSnapshot,
    kind: QaStateKind,
    chunk: Option<String>,
    error: Option<String>,
    force_edit_fields: bool,
) {
    events.publish(
        snapshot.session_id,
        BackendEventKind::QaState(QaStateEvent::from_snapshot_transition(
            snapshot,
            kind,
            chunk,
            error,
            force_edit_fields,
        )),
    );
}

#[cfg(test)]
mod runtime_restore_tests {
    use super::*;
    use crate::domains::QaTurnResult;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    #[derive(Clone, Default)]
    struct Runtime {
        calls: Arc<AtomicUsize>,
        block_prepare: Arc<AtomicBool>,
        fail_prepare: Arc<AtomicBool>,
        prepare_entered: Arc<tokio::sync::Notify>,
        prepare_release: Arc<tokio::sync::Notify>,
        block_cancel: Arc<AtomicBool>,
        cancel_entered: Arc<tokio::sync::Notify>,
        cancel_release: Arc<tokio::sync::Notify>,
        cancel_done: Arc<tokio::sync::Notify>,
    }
    impl QaRuntimeAdapter for Runtime {
        fn prepare_text(
            &self,
            _: SessionId,
            text: String,
        ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
            let this = self.clone();
            Box::pin(async move {
                this.calls.fetch_add(1, Ordering::SeqCst);
                if this.fail_prepare.load(Ordering::SeqCst) {
                    return Err(BackendError::new(
                        BackendErrorCode::Provider,
                        "fixture prepare failure",
                    ));
                }
                if this.block_prepare.load(Ordering::SeqCst) {
                    this.prepare_entered.notify_one();
                    this.prepare_release.notified().await;
                }
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
        fn finish_recording(
            &self,
            _: SessionId,
        ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
            Box::pin(async {
                Ok(QaInput {
                    text: "question".into(),
                    selection_text: None,
                    selection_source_app: None,
                })
            })
        }
        fn answer(
            &self,
            _: QaTurnRequest,
            _: Arc<dyn QaProgressSink>,
        ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
            Box::pin(async {
                Ok(QaTurnResult {
                    answer: "answer".into(),
                })
            })
        }
        fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            let this = self.clone();
            Box::pin(async move {
                if this.block_cancel.load(Ordering::SeqCst) {
                    this.cancel_entered.notify_one();
                    this.cancel_release.notified().await;
                }
                this.cancel_done.notify_one();
                Ok(())
            })
        }
    }
    fn service(runtime: Runtime, restoring: Arc<AtomicBool>) -> QaService {
        let service = QaService::new(Arc::new(runtime), Arc::new(crate::ports::NoopHostActions));
        service.bind_event_publisher(BackendEventPublisher::new(Arc::new(
            crate::events::EventBus::new(32),
        )));
        service
            .bind_runtime_restore_guard(
                Arc::new(move || {
                    if restoring.load(Ordering::Acquire) {
                        Err(BackendError::new(BackendErrorCode::Busy, "restoring"))
                    } else {
                        Ok(())
                    }
                }),
                Arc::new(crate::config::TokioTaskSpawner),
            )
            .unwrap();
        service
    }

    #[tokio::test]
    async fn runtime_restore_rejects_qa_text_and_voice_before_host_work_but_keeps_snapshot_readable(
    ) {
        let runtime = Runtime::default();
        let flag = Arc::new(AtomicBool::new(true));
        let service = service(runtime.clone(), flag.clone());
        assert_eq!(
            service
                .submit_text("question".into())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            service.toggle_recording().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 0);
        assert_eq!(service.snapshot().await.unwrap().phase, QaPhase::Idle);
        assert!(service.runtime_restore_idle());
        flag.store(false, Ordering::Release);
        service.submit_text("question".into()).await.unwrap();
        assert_eq!(service.snapshot().await.unwrap().phase, QaPhase::Completed);
        assert!(service.runtime_restore_idle());
    }

    #[tokio::test]
    async fn runtime_restore_qa_text_lease_does_not_take_the_exclusive_voice_slot() {
        let runtime = Runtime::default();
        runtime.block_prepare.store(true, Ordering::SeqCst);
        let flag = Arc::new(AtomicBool::new(false));
        let service = service(runtime.clone(), flag.clone());
        let text = tokio::spawn(service.submit_text("question".into()));
        runtime.prepare_entered.notified().await;
        assert!(!service.runtime_restore_idle());
        let voice = SessionId::new();
        service
            .voice_sessions
            .acquire(voice, crate::voice_session::VoiceSessionKind::Dictation)
            .unwrap();
        service.voice_sessions.release(voice);
        flag.store(true, Ordering::Release);
        assert_eq!(
            service
                .submit_text("another".into())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        runtime.prepare_release.notify_one();
        text.await.unwrap().unwrap();
        assert!(
            service.runtime_restore_idle(),
            "an attempted restore must not break the accepted turn"
        );
    }

    #[tokio::test]
    async fn runtime_restore_qa_cancel_waiter_drop_keeps_cleanup_busy_until_drained() {
        let runtime = Runtime::default();
        runtime.block_cancel.store(true, Ordering::SeqCst);
        let service = service(runtime.clone(), Arc::new(AtomicBool::new(false)));
        service.toggle_recording().await.unwrap();
        let cancel = tokio::spawn(service.cancel(None));
        runtime.cancel_entered.notified().await;
        assert_eq!(service.snapshot().await.unwrap().phase, QaPhase::Cancelled);
        assert!(!service.runtime_restore_idle());
        cancel.abort();
        assert!(cancel.await.unwrap_err().is_cancelled());
        assert!(!service.runtime_restore_idle());
        runtime.cancel_release.notify_one();
        runtime.cancel_done.notified().await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(service.runtime_restore_idle());
    }

    #[test]
    fn runtime_restore_qa_probe_cannot_pass_between_check_and_claim() {
        let flag = Arc::new(AtomicBool::new(false));
        let service = QaService::new(
            Arc::new(Runtime::default()),
            Arc::new(crate::ports::NoopHostActions),
        );
        let (checked, receive_checked) = std::sync::mpsc::channel();
        let (resume, receive_resume) = std::sync::mpsc::channel();
        let receive_resume = Mutex::new(receive_resume);
        let check_flag = flag.clone();
        service.bind_event_publisher(BackendEventPublisher::new(Arc::new(
            crate::events::EventBus::new(32),
        )));
        service
            .bind_runtime_restore_guard(
                Arc::new(move || {
                    assert!(!check_flag.load(Ordering::Acquire));
                    checked.send(()).unwrap();
                    receive_resume.lock().unwrap().recv().unwrap();
                    Ok(())
                }),
                Arc::new(crate::config::TokioTaskSpawner),
            )
            .unwrap();
        let start = std::thread::spawn({
            let service = service.clone();
            move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(service.toggle_recording())
            }
        });
        receive_checked
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        flag.store(true, Ordering::Release);
        let probe = std::thread::spawn({
            let service = service.clone();
            move || service.runtime_restore_idle()
        });
        resume.send(()).unwrap();
        start.join().unwrap().unwrap();
        assert!(!probe.join().unwrap());
    }
    #[tokio::test]
    async fn runtime_restore_error_cleanup_survives_dropping_the_failed_submit_waiter() {
        let runtime = Runtime::default();
        runtime.fail_prepare.store(true, Ordering::SeqCst);
        runtime.block_cancel.store(true, Ordering::SeqCst);
        let service = service(runtime.clone(), Arc::new(AtomicBool::new(false)));
        let submitting = tokio::spawn(service.submit_text("fixture".into()));
        runtime.cancel_entered.notified().await;
        assert_eq!(service.snapshot().await.unwrap().phase, QaPhase::Failed);
        submitting.abort();
        assert!(submitting.await.unwrap_err().is_cancelled());
        assert!(
            !service.runtime_restore_idle(),
            "error cleanup still owns native runtime work"
        );
        runtime.cancel_release.notify_one();
        runtime.cancel_done.notified().await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(service.runtime_restore_idle());
    }
    #[tokio::test]
    async fn runtime_restore_cancel_return_does_not_release_an_inflight_text_reader() {
        let runtime = Runtime::default();
        runtime.block_prepare.store(true, Ordering::SeqCst);
        let service = service(runtime.clone(), Arc::new(AtomicBool::new(false)));
        let question = tokio::spawn(service.submit_text("fixture".into()));
        runtime.prepare_entered.notified().await;
        service.cancel(None).await.unwrap();
        assert_eq!(service.snapshot().await.unwrap().phase, QaPhase::Cancelled);
        assert!(
            !service.runtime_restore_idle(),
            "old context preparation has not drained"
        );
        runtime.prepare_release.notify_one();
        assert_eq!(
            question.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(service.runtime_restore_idle());
    }
}
