//! Selection-only voice editing. Capture occurs on the fcitx5 hotkey, before showing Qt.
use openless_core::domains::{SelectionRuntimeAdapter, VoiceEditPlanRequest};
use openless_core::{
    DictationOutputTarget, DictationStartOptions, OpenLessBackend, SessionId, TextSelection,
    VoiceEditPhase, VoiceEditSession, VoiceEditSnapshot,
};
use openless_linux_host::LinuxSelectionRuntime;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct VoiceEdit {
    armed: Mutex<Option<Instant>>,
    state: tokio::sync::Mutex<Option<Active>>,
    selection: LinuxSelectionRuntime,
}

struct Active {
    session: VoiceEditSession,
    ticket: SessionId,
    source: String,
    recording: Option<SessionId>,
}

impl Default for VoiceEdit {
    fn default() -> Self {
        Self {
            armed: Mutex::new(None),
            state: tokio::sync::Mutex::new(None),
            selection: LinuxSelectionRuntime::focused_only(),
        }
    }
}

impl VoiceEdit {
    pub fn arm(&self) {
        *self.armed.lock().unwrap() = Some(Instant::now() + Duration::from_secs(30));
    }

    pub fn take_arm(&self) -> bool {
        self.armed
            .lock()
            .unwrap()
            .take()
            .is_some_and(|deadline| Instant::now() <= deadline)
    }

    pub async fn capture(&self) -> Result<(), String> {
        let mut state = self.state.try_lock().map_err(|_| "Voice edit is busy")?;
        if state
            .as_ref()
            .is_some_and(|active| !terminal(active.session.snapshot().phase))
        {
            return Err("A voice edit session is already active".into());
        }
        if let Some(previous) = state.take() {
            let _ = self.selection.cancel(previous.ticket).await;
        }
        let ticket = SessionId::new();
        let source = self
            .selection
            .capture(ticket, None)
            .await
            .map_err(text)?
            .text;
        let result = (|| {
            let end = u32::try_from(source.chars().count()).map_err(text)?;
            let mut session =
                VoiceEditSession::start(source.clone(), Some(TextSelection { start: 0, end }))
                    .map_err(text)?;
            // Seed the editable draft with the selected text, without inserting it.
            session.finish_dictation(source.clone()).map_err(text)?;
            Ok(Active {
                session,
                ticket,
                source,
                recording: None,
            })
        })();
        match result {
            Ok(active) => {
                *state = Some(active);
                Ok(())
            }
            Err(error) => {
                let _ = self.selection.cancel(ticket).await;
                Err(error)
            }
        }
    }

    pub async fn snapshot(&self) -> Option<VoiceEditSnapshot> {
        self.state
            .lock()
            .await
            .as_ref()
            .map(|active| active.session.snapshot())
    }

    pub async fn action(
        &self,
        backend: &OpenLessBackend,
        command: &str,
        id: &str,
    ) -> Result<VoiceEditSnapshot, String> {
        let mut state = self.state.try_lock().map_err(|_| "Voice edit is busy")?;
        let active = state
            .as_mut()
            .filter(|active| active.session.session_id().to_string() == id)
            .ok_or("Voice edit session is stale")?;
        match command {
            "start_voice_edit_instruction" => {
                if !backend.get_preferences().voice_edit_enabled {
                    return Err("Voice edit is disabled".into());
                }
                active.session.enter_editing().map_err(text)?;
                match backend
                    .start_dictation_with_options(DictationStartOptions {
                        insert_text: false,
                        output_target: DictationOutputTarget::VoiceEdit,
                        ..Default::default()
                    })
                    .await
                {
                    Ok(recording) => active.recording = Some(recording),
                    Err(error) => {
                        let _ = active.session.recover_instruction();
                        return Err(text(error));
                    }
                }
            }
            "stop_voice_edit_instruction" | "finalize_voice_edit_instruction" => {
                let recording = active
                    .recording
                    .take()
                    .ok_or("No voice instruction is recording")?;
                active.session.begin_applying().map_err(text)?;
                if let Err(error) = apply_instruction(active, backend, recording).await {
                    if backend.snapshot().dictation.session_id == Some(recording) {
                        let _ = backend.cancel_dictation(Some(recording)).await;
                    }
                    let _ = active.session.recover_instruction();
                    return Err(error);
                }
            }
            "commit_voice_edit" | "commit_voice_edit_session" => {
                let commit = active.session.begin_commit().map_err(text)?;
                if commit.text != active.source {
                    if let Err(error) = self
                        .selection
                        .apply(active.ticket, active.source.clone(), commit.text)
                        .await
                    {
                        let _ = active.session.recover_commit();
                        return Err(text(error));
                    }
                }
                active.session.complete_commit().map_err(text)?;
                let _ = self.selection.cancel(active.ticket).await;
            }
            "cancel_voice_edit_session" => {
                if let Some(recording) = active.recording {
                    backend
                        .cancel_dictation(Some(recording))
                        .await
                        .map_err(text)?;
                    active.recording = None;
                }
                if !terminal(active.session.snapshot().phase) {
                    active.session.cancel().map_err(text)?;
                }
                let _ = self.selection.cancel(active.ticket).await;
            }
            _ => return Err("Unsupported voice edit command".into()),
        }
        Ok(active.session.snapshot())
    }
}

async fn apply_instruction(
    active: &mut Active,
    backend: &OpenLessBackend,
    recording: SessionId,
) -> Result<(), String> {
    let result = backend
        .stop_dictation_session(recording)
        .await
        .map_err(text)?;
    let raw = result.raw_text;
    if raw.trim().is_empty() {
        return Err("The voice instruction is empty".into());
    }
    let polished = if result.polished_text.trim().is_empty() {
        raw.clone()
    } else {
        result.polished_text
    };
    let snapshot = active.session.snapshot();
    let context = snapshot.context.ok_or("Voice edit draft is missing")?;
    let plan = backend
        .services()
        .selection_voice
        .voice_edit_plan(VoiceEditPlanRequest {
            session_id: snapshot.session_id,
            field_context: context.field_text,
            draft: context.preview,
            instruction_raw: raw.clone(),
            instruction_polished: polished,
        })
        .await
        .map_err(text)?;
    active
        .session
        .apply_instruction(raw, plan.instruction_polished, plan.plan)
        .map_err(text)
}

pub fn terminal(phase: VoiceEditPhase) -> bool {
    matches!(phase, VoiceEditPhase::Completed | VoiceEditPhase::Cancelled)
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_is_one_shot_and_expired_arm_does_not_consume_polish() {
        let edit = VoiceEdit::default();
        assert!(!edit.take_arm());
        edit.arm();
        assert!(edit.take_arm());
        assert!(!edit.take_arm());
        *edit.armed.lock().unwrap() = Some(Instant::now() - Duration::from_secs(1));
        assert!(!edit.take_arm());
        assert!(edit.armed.lock().unwrap().is_none());
    }
}
