//! Native recording ownership for Core's selection-voice workflow. The Core
//! service owns intent routing, preview state and the cancellation generation.
use openless_core::{
    BackendError, OpenLessBackend, RecordingControlAction, RecordingControlSink, SessionId,
    VoiceTranscriptionSession,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Capture {
    applied: Option<SessionId>,
    expected: Option<SessionId>,
    session: Option<Arc<VoiceTranscriptionSession>>,
    finishing: bool,
    pending: Option<RecordingControlAction>,
}
#[derive(Clone)]
pub struct LinuxSelectionVoice {
    backend: Arc<OpenLessBackend>,
    capture: Arc<Mutex<Capture>>,
    runtime: Arc<Mutex<Option<tokio::runtime::Handle>>>,
}
impl LinuxSelectionVoice {
    pub fn new(backend: Arc<OpenLessBackend>) -> Self {
        Self {
            backend,
            capture: Arc::default(),
            runtime: Arc::default(),
        }
    }

    pub async fn edge(&self, pressed: bool, at: std::time::Instant) -> Result<bool, BackendError> {
        *self.runtime.lock().unwrap() = Some(tokio::runtime::Handle::current());
        use openless_core::{
            SelectionVoiceHotkeyAction as Action, SelectionVoiceHotkeyEdge as Edge,
            SelectionVoicePhase as Phase,
        };
        let api = &self.backend.services().selection_voice;
        let snapshot = api.snapshot().await?;
        let active = matches!(
            snapshot.phase,
            Phase::Recording
                | Phase::Processing
                | Phase::AwaitingIntent
                | Phase::Preview
                | Phase::Applying
        );
        if !self.backend.get_preferences().selection_voice_enabled && !active {
            return Ok(false);
        }
        if !pressed && !active {
            return Ok(false);
        }
        let action = api.dispatch_hotkey_edge(if pressed {
            Edge::Pressed { at }
        } else {
            Edge::Released { at }
        })?;
        match action {
            Action::Start => {
                self.dismiss_applied();
                let ticket = SessionId::new();
                let ticket_string = ticket.to_string();
                let source = tokio::task::spawn_blocking(move || {
                    crate::fcitx5::capture_selection_target(&ticket_string)
                })
                .await
                .map_err(crate::context::platform)?;
                let source = match source {
                    Ok(text) if !text.is_empty() => text,
                    _ => {
                        let _ = crate::fcitx5::cancel_selection_target(&ticket.to_string());
                        return Ok(false);
                    }
                };
                let source_app = {
                    use crate::context::ContextReader;
                    let id = ticket.to_string();
                    tokio::task::spawn_blocking(move || {
                        crate::context::NativeContextReader
                            .read(Some(&id), false)
                            .ok()
                            .map(|s| s.application)
                            .filter(|s| !s.is_empty())
                    })
                    .await
                    .ok()
                    .flatten()
                };
                let session_id = match api
                    .begin(openless_core::SelectionCapture {
                        text: source,
                        source_app,
                    })
                    .await
                {
                    Ok(id) => id,
                    Err(error) => {
                        let _ = crate::fcitx5::cancel_selection_target(&ticket.to_string());
                        return Err(error);
                    }
                };
                if let Err(error) = crate::fcitx5::rekey_selection_target(
                    &ticket.to_string(),
                    &session_id.to_string(),
                ) {
                    let _ = api.cancel(Some(session_id)).await;
                    let _ = crate::fcitx5::cancel_selection_target(&ticket.to_string());
                    return Err(error);
                }
                {
                    let mut state = self.capture.lock().unwrap();
                    state.expected = Some(session_id);
                    state.pending = None;
                    state.finishing = false;
                }
                let session = self
                    .backend
                    .start_selection_voice_capture(session_id, Arc::new(self.clone()))
                    .await;
                let session = match session {
                    Ok(session) => session,
                    Err(error) => {
                        self.cancel(session_id).await?;
                        return Err(error);
                    }
                };
                let mut late = Some(Arc::new(session));
                let pending = {
                    let mut state = self.capture.lock().unwrap();
                    if state.expected == Some(session_id) {
                        state.session = late.take();
                        state.pending.take()
                    } else {
                        None
                    }
                };
                if let Some(session) = late {
                    session.cancel().await?;
                    return Ok(true);
                }
                if let Some(action) = pending {
                    self.request(session_id, action)?;
                }
            }
            Action::Finish => {
                if let Some(id) = snapshot.session_id {
                    self.request(id, RecordingControlAction::Stop)?;
                }
            }
            Action::Noop => return Ok(active),
        }
        Ok(true)
    }

    pub async fn cancel(&self, id: SessionId) -> Result<(), BackendError> {
        let capture = {
            let mut state = self.capture.lock().unwrap();
            if state.expected == Some(id) {
                state.expected = None;
                state.pending = None;
                state.session.take()
            } else {
                None
            }
        };
        if let Some(capture) = capture {
            let _ = capture.cancel().await;
        }
        self.backend
            .services()
            .selection_voice
            .cancel(Some(id))
            .await?;
        let ticket = id.to_string();
        tokio::task::spawn_blocking(move || crate::fcitx5::cancel_selection_target(&ticket))
            .await
            .map_err(crate::context::platform)?
    }

    pub async fn confirm_intent(&self, id: SessionId, intent: String) -> Result<(), BackendError> {
        let api = &self.backend.services().selection_voice;
        let disposition = api.confirm_intent(id, intent).await?;
        self.route(disposition).await?;
        Ok(())
    }

    async fn route(
        &self,
        disposition: openless_core::SelectionVoiceDisposition,
    ) -> Result<(), BackendError> {
        if let openless_core::SelectionVoiceRoute::ReadyToApply { preview } = self
            .backend
            .services()
            .selection_voice
            .route_disposition(disposition)
            .await?
        {
            self.apply(preview.text, preview.owner_session_id).await?;
        }
        Ok(())
    }

    pub async fn apply(&self, text: String, owner: Option<SessionId>) -> Result<(), BackendError> {
        let api = &self.backend.services().selection_voice;
        let ticket = api.begin_preview_apply(owner, text.clone())?;
        let source = ticket.source_text.clone();
        let id = ticket.session_id;
        let result = tokio::task::spawn_blocking(move || {
            crate::fcitx5::apply_selection_target(&id.to_string(), &source, &text)
        })
        .await
        .map_err(crate::context::platform)?;
        api.finish_preview_apply(
            ticket.ticket_id,
            if result.is_ok() {
                openless_core::SelectionVoiceApplyOutcome::Inserted
            } else {
                openless_core::SelectionVoiceApplyOutcome::Failed
            },
        )
        .await?;
        if result.is_ok() {
            self.capture.lock().unwrap().applied = Some(id);
        }
        result
    }

    pub fn applied_target(&self) -> Option<SessionId> {
        self.capture.lock().unwrap().applied
    }
    pub fn dismiss_applied(&self) {
        let id = self.capture.lock().unwrap().applied.take();
        if let (Some(id), Some(runtime)) = (id, self.runtime.lock().unwrap().clone()) {
            runtime.spawn_blocking(move || crate::fcitx5::cancel_selection_target(&id.to_string()));
        }
    }
    pub async fn revert_applied(&self, id: SessionId) -> Result<(), BackendError> {
        if self.applied_target() != Some(id) {
            return Err(crate::context::platform("applied selection expired"));
        }
        tokio::task::spawn_blocking(move || {
            crate::fcitx5::revert_selection_target(&id.to_string())
        })
        .await
        .map_err(crate::context::platform)??;
        let mut capture = self.capture.lock().unwrap();
        if capture.applied == Some(id) {
            capture.applied = None;
        }
        Ok(())
    }
}
impl RecordingControlSink for LinuxSelectionVoice {
    fn request(&self, id: SessionId, action: RecordingControlAction) -> Result<(), BackendError> {
        let capture = {
            let mut state = self.capture.lock().unwrap();
            if state.expected != Some(id) {
                return Ok(());
            }
            if action == RecordingControlAction::Cancel {
                state.expected = None;
                state.pending = None;
                state.session.take()
            } else if state.finishing {
                return Ok(());
            } else if let Some(session) = state.session.clone() {
                state.finishing = true;
                Some(session)
            } else {
                state.pending = Some(action);
                return Ok(());
            }
        };
        let this = self.clone();
        let runtime = self.runtime.lock().unwrap().clone().ok_or_else(|| {
            crate::context::platform("selection voice runtime is not initialized")
        })?;
        runtime.spawn(async move {
            let result: Result<(), BackendError> = async {
                if action == RecordingControlAction::Cancel {
                    if let Some(capture) = capture {
                        capture.cancel().await?;
                    }
                    return this.cancel(id).await;
                }
                let Some(capture) = capture else {
                    return Ok(());
                };
                let api = &this.backend.services().selection_voice;
                api.mark_processing(id).await?;
                let transcript = capture.finish().await?;
                let disposition = api.process_transcript(id, transcript).await?;
                this.route(disposition).await?;
                {
                    let mut state = this.capture.lock().unwrap();
                    if state.expected == Some(id) {
                        state.expected = None;
                        state.session = None;
                        state.finishing = false;
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                log::warn!("selection voice failed: {error}");
                let _ = this.cancel(id).await;
            }
        });
        Ok(())
    }
}
