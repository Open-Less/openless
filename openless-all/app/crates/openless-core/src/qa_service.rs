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
            presentation: Arc::new(Mutex::new(())),
            persistence: Some(Arc::new(persistence)),
            selection_voice: Some(selection_voice),
            voice_sessions,
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
        {
            let mut state = self.state.lock().expect("QA state lock poisoned");
            // Validate the callback's generation and claim the finish under
            // one lock. A delayed silence event cannot toggle a completed turn
            // back on, stop its successor, or compete with a manual stop.
            ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
            state.snapshot.phase = QaPhase::Thinking;
        }
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
        let text = match &submission {
            QaSubmission::Text(text) => text,
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
            QaSubmission::Text(_) => self.runtime.prepare_text(session_id, text).await,
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
        if let Err(error) = self.runtime.cancel(session_id).await {
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
        Box::pin(async move {
            {
                let state = service.state.lock().expect("QA state lock poisoned");
                ensure_current_phase(&state.snapshot, session_id, QaPhase::Recording)?;
            }
            service.fail_if_current(session_id, &error);
            service.voice_sessions.release(session_id);
            service.runtime.cancel(session_id).await
        })
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
