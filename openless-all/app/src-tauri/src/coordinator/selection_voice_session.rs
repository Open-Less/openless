//! Selection-voice edit session (issue #987 desktop MVP, Windows-first).

use std::sync::{Arc, Weak};

use super::{emit_capsule, schedule_capsule_idle, Coordinator, Inner, CAPSULE_AUTO_HIDE_DELAY_MS};
use crate::coordinator_state::SessionId;
use crate::selection::SelectionInsertionTarget;
use crate::types::{CapsuleState, InsertStatus};
use openless_core::{
    BackendError, BackendErrorCode, SelectionCapture, SelectionVoiceApplyOutcome,
    SelectionVoiceDisposition, SelectionVoiceHotkeyAction, SelectionVoiceHotkeyEdge,
    SelectionVoicePhase, SelectionVoiceRoute, SessionId as CoreSessionId,
};

/// Platform half of automatic recording control. Core owns the silence/fault
/// decision; this object only reaches the capture handle kept by the Tauri
/// coordinator and performs the requested stop/cancel effect.
struct SelectionVoiceRecordingControl {
    inner: Weak<Inner>,
    pending: parking_lot::Mutex<Vec<(CoreSessionId, openless_core::RecordingControlAction)>>,
}

impl SelectionVoiceRecordingControl {
    fn new(inner: &Arc<Inner>) -> Self {
        Self {
            inner: Arc::downgrade(inner),
            pending: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn apply(
        inner: &Arc<Inner>,
        session_id: CoreSessionId,
        action: openless_core::RecordingControlAction,
    ) {
        match action {
            openless_core::RecordingControlAction::Stop => {
                let task_inner = Arc::clone(inner);
                inner.host.spawn(async move {
                    if let Err(error) =
                        end_selection_voice_session(&task_inner, Some(session_id)).await
                    {
                        log::warn!("[selection-voice] automatic stop failed: {error}");
                    }
                });
            }
            openless_core::RecordingControlAction::Cancel => {
                Coordinator {
                    inner: Arc::clone(inner),
                }
                .finish_cancelled_selection_voice_host(session_id);
            }
        }
    }

    fn flush(&self, session_id: CoreSessionId) {
        let requests = {
            let mut pending = self.pending.lock();
            let mut requests = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].0 == session_id {
                    requests.push(pending.remove(index));
                } else {
                    index += 1;
                }
            }
            requests
        };
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        for (_, action) in requests {
            Self::apply(&inner, session_id, action);
        }
    }
}

impl openless_core::RecordingControlSink for SelectionVoiceRecordingControl {
    fn request(
        &self,
        session_id: CoreSessionId,
        action: openless_core::RecordingControlAction,
    ) -> Result<(), BackendError> {
        let inner = self.inner.upgrade().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Cancelled,
                "selection voice host session is no longer available",
            )
        })?;
        if action == openless_core::RecordingControlAction::Cancel {
            // Cancellation must synchronously revoke the Starting owner first.
            // It must not queue behind capture installation: Core has already
            // invalidated the token, so a late native handle is only closed,
            // never attached.
            Self::apply(&inner, session_id, action);
            return Ok(());
        }
        // Serialize against flush: a start event must not be enqueued after
        // flush has already read an empty queue.
        let mut pending = self.pending.lock();
        let ready = inner
            .selection_voice_capture
            .lock()
            .as_ref()
            .is_some_and(|capture| capture.session_id() == session_id);
        if ready {
            drop(pending);
            Self::apply(&inner, session_id, action);
        } else {
            // cpal may report a device fault immediately after starting its
            // stream, before the returned capture reaches the Host slot.
            // Preserve that Core decision and replay it after installation.
            pending.push((session_id, action));
        }
        Ok(())
    }
}

fn selection_voice_user_message(error: &str) -> String {
    match error {
        "dictationActive" => "正在听写，请先结束录音".into(),
        "selectionVoiceNoSelection" => "请先选中文字，或将光标放在可输入的文本框中".into(),
        "selectionVoiceTargetUnavailable" => "无法定位输入目标，请先点击文本框后再试".into(),
        "selectionVoiceBusy" => "选区语音会话进行中".into(),
        other => other.into(),
    }
}

fn emit_selection_voice_begin_error(inner: &Arc<Inner>, error: &str) {
    release_selection_voice_capsule_claim(inner);
    emit_capsule(
        inner,
        CapsuleState::Error,
        0.0,
        0,
        Some(selection_voice_user_message(error)),
        None,
    );
    // On begin failures like "no selection" the capsule stops at Error; same
    // convention as dictation Done/Error, auto-hidden after 2s.
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
    log::info!(
        "[selection-voice] begin error capsule shown error={error} auto_hide_ms={CAPSULE_AUTO_HIDE_DELAY_MS}"
    );
}

fn emit_selection_voice_end_error(inner: &Arc<Inner>, error: &str) {
    log::warn!("[selection-voice] workflow failed: {error}");
    let message = selection_voice_end_message(error);
    emit_capsule(inner, CapsuleState::Error, 0.0, 0, Some(message), None);
    schedule_capsule_idle(inner, 2500);
}

fn selection_voice_end_message(error: &str) -> String {
    if error.contains("invalid EditPlan XML") || error.contains("invalid EditPlan JSON") {
        return "编辑方案解析失败，请重试".into();
    }
    if error.contains("edit plan has no operations") {
        return "未能生成有效编辑方案，请重试".into();
    }
    if error.contains("edit plan has too many operations") {
        return "编辑方案过于复杂，请缩短指令".into();
    }
    if error.contains("edit operation exceeds size limit") {
        return "编辑内容过长，请缩短选区或拆步操作".into();
    }
    if error.contains("global timeout") || error.contains("bailian global timeout") {
        return "语音识别超时，请重试".into();
    }
    if error.contains("selectionVoiceAsrUnavailable") {
        return "语音识别不可用，请重试".into();
    }
    if error.contains("translation unchanged") {
        return "翻译结果与原文相同，请重试或调整指令".into();
    }
    selection_voice_user_message(error)
}

fn selection_voice_apply_outcome(
    status: InsertStatus,
) -> Result<SelectionVoiceApplyOutcome, String> {
    match status {
        InsertStatus::Inserted => Ok(SelectionVoiceApplyOutcome::Inserted),
        InsertStatus::PasteSent => Ok(SelectionVoiceApplyOutcome::PasteSent),
        InsertStatus::CopiedFallback => Ok(SelectionVoiceApplyOutcome::CopiedFallback),
        InsertStatus::Failed | InsertStatus::NotRequested => {
            Err("selectionVoiceInsertFailed".to_string())
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct SelectionVoiceHostState {
    /// Opaque native target only. Selection text, instruction, intent and
    /// preview remain exclusively owned by `openless-core`.
    target_session_id: Option<CoreSessionId>,
    insertion_target: SelectionInsertionTarget,
    /// Claim the shared capsule before clipboard capture finishes so a late
    /// dictation Done/Idle cannot paint 「录音已完成」over the new session.
    owns_capsule: bool,
    /// Live mic levels only while Recording. Cleared on stop → Transcribing so
    /// late `SelectionVoiceLevel` cannot re-emit Recording and bump the capsule
    /// epoch (which would make hide_core_capsule_if_current no-op).
    accepts_level: bool,
}

fn claim_selection_voice_capsule(inner: &Arc<Inner>) {
    let mut host = inner.selection_voice_host.lock();
    host.owns_capsule = true;
    host.accepts_level = true;
}

fn release_selection_voice_capsule_claim(inner: &Arc<Inner>) {
    let mut host = inner.selection_voice_host.lock();
    host.owns_capsule = false;
    host.accepts_level = false;
}

fn stop_selection_voice_level_meter(inner: &Arc<Inner>) {
    inner.selection_voice_host.lock().accepts_level = false;
}

pub(super) fn selection_voice_owns_capsule(inner: &Arc<Inner>) -> bool {
    inner.selection_voice_host.lock().owns_capsule
}

/// Accept live meter frames only while Recording for this claimed session.
pub(super) fn selection_voice_accepts_level(inner: &Arc<Inner>, session_id: &str) -> bool {
    let host = inner.selection_voice_host.lock();
    host.owns_capsule
        && host.accepts_level
        && host
            .target_session_id
            .as_ref()
            .is_some_and(|id| id.to_string() == session_id)
}

/// Terminal dismiss: Done (visible end anim) then Idle after the same dwell as
/// dictation — never jump straight to Idle (instant window.hide kills Siri exit).
fn finish_selection_voice_capsule(inner: &Arc<Inner>, message: Option<String>) {
    emit_capsule(inner, CapsuleState::Done, 0.0, 0, message, None);
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
}

fn core_error(error: BackendError) -> String {
    match error.code {
        BackendErrorCode::Busy => "selectionVoiceBusy".to_string(),
        BackendErrorCode::Cancelled => "selectionVoicePreviewUnavailable".to_string(),
        BackendErrorCode::InvalidArgument if error.message.contains("intent") => error
            .message
            .rsplit_once(':')
            .map(|(_, intent)| format!("selectionVoiceInvalidIntent:{}", intent.trim()))
            .unwrap_or_else(|| "selectionVoiceInvalidIntent".to_string()),
        BackendErrorCode::InvalidState if error.message.contains("intent prompt") => {
            "selectionVoiceIntentPromptUnavailable".to_string()
        }
        BackendErrorCode::InvalidState if error.message.contains("preview") => {
            "selectionVoicePreviewUnavailable".to_string()
        }
        _ => error.message,
    }
}

fn owner_session_id(session_id: SessionId) -> CoreSessionId {
    CoreSessionId::from_uuid(session_id)
}

fn target_for_session(
    inner: &Arc<Inner>,
    session_id: CoreSessionId,
) -> Result<SelectionInsertionTarget, String> {
    let host = inner.selection_voice_host.lock();
    if host.target_session_id != Some(session_id) {
        return Err("selectionVoiceTargetUnavailable".to_string());
    }
    Ok(host.insertion_target.clone())
}

fn clear_host_session(inner: &Arc<Inner>, session_id: CoreSessionId) -> bool {
    let mut host = inner.selection_voice_host.lock();
    if host.target_session_id == Some(session_id) {
        *host = SelectionVoiceHostState::default();
        return true;
    }
    false
}

pub(super) fn bind_selection_voice_target_state(
    host_slot: &Arc<parking_lot::Mutex<SelectionVoiceHostState>>,
    session_id: CoreSessionId,
    insertion_target: SelectionInsertionTarget,
) -> Result<(), String> {
    if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
        return Err("selectionVoiceTargetUnavailable".to_string());
    }
    let mut host = host_slot.lock();
    host.target_session_id = Some(session_id);
    host.insertion_target = insertion_target;
    Ok(())
}

pub(super) async fn handle_selection_voice_pressed(inner: &Arc<Inner>) {
    let action = match inner
        .backend
        .services()
        .selection_voice
        .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed {
            at: std::time::Instant::now(),
        }) {
        Ok(action) => action,
        Err(error) => {
            log::warn!("[selection-voice] hotkey dispatch failed: {error}");
            return;
        }
    };
    let result = match action {
        SelectionVoiceHotkeyAction::Start => begin_selection_voice_session(inner).await,
        SelectionVoiceHotkeyAction::Finish => end_selection_voice_session(inner, None).await,
        SelectionVoiceHotkeyAction::Noop => return,
    };
    if let Err(error) = result {
        log::warn!("[selection-voice] hotkey action failed: {error}");
        emit_selection_voice_begin_error(inner, &error);
    }
}

pub(super) async fn handle_selection_voice_released(inner: &Arc<Inner>) {
    let action = match inner
        .backend
        .services()
        .selection_voice
        .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Released {
            at: std::time::Instant::now(),
        }) {
        Ok(action) => action,
        Err(error) => {
            log::warn!("[selection-voice] hotkey dispatch failed: {error}");
            return;
        }
    };
    if action == SelectionVoiceHotkeyAction::Finish {
        if let Err(error) = end_selection_voice_session(inner, None).await {
            log::warn!("[selection-voice] end on hotkey release failed: {error}");
        }
    }
}

async fn begin_selection_voice_session(inner: &Arc<Inner>) -> Result<(), String> {
    // Claim + show Recording before Ctrl+C capture (~0.5–1s). Otherwise the
    // previous dictation Done / streaming text stays visible as 「录音已完成」.
    claim_selection_voice_capsule(inner);
    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);

    let (selection_opt, insertion_target, capture_diag) =
        crate::selection::resolve_selection_workspace_capture_with_diag();
    log::info!(
        "[selection-voice] begin capture diag={}",
        capture_diag.summary()
    );
    if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
        log::warn!(
            "[selection-voice] begin failed: selectionVoiceTargetUnavailable ({})",
            capture_diag.summary()
        );
        release_selection_voice_capsule_claim(inner);
        return Err("selectionVoiceTargetUnavailable".into());
    }
    // Empty selection is allowed when the insertion target is valid (Help me write / QA).
    let selection = match selection_opt {
        Some(selection) => selection,
        None => {
            log::info!(
                "[selection-voice] begin with empty selection (compose/qa path) ({})",
                capture_diag.summary()
            );
            crate::selection::SelectionContext {
                text: String::new(),
                source_app: capture_diag.front_app.clone(),
            }
        }
    };

    let session_id = match inner
        .backend
        .services()
        .selection_voice
        .begin(SelectionCapture {
            text: selection.text,
            source_app: selection.source_app,
        })
        .await
    {
        Ok(session_id) => session_id,
        Err(error) => {
            release_selection_voice_capsule_claim(inner);
            return Err(core_error(error));
        }
    };
    {
        let mut host = inner.selection_voice_host.lock();
        host.target_session_id = Some(session_id);
        host.insertion_target = insertion_target;
        host.owns_capsule = true;
        host.accepts_level = true;
    }

    // Re-assert Recording after core begin so a racing dictation event cannot
    // leave the capsule on Done while the mic is open.
    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);
    let recording_control = Arc::new(SelectionVoiceRecordingControl::new(inner));
    match inner
        .backend
        .start_selection_voice_capture(
            session_id,
            Arc::clone(&recording_control) as Arc<dyn openless_core::RecordingControlSink>,
        )
        .await
    {
        Ok(capture) => {
            let capture = Arc::new(capture);
            let installed = {
                // A cancel during startup revokes the target owner first.
                // Owner check and capture installation share this lock section,
                // so a late device start cannot overwrite the next round's
                // handle.
                let host = inner.selection_voice_host.lock();
                if host.target_session_id == Some(session_id) {
                    *inner.selection_voice_capture.lock() = Some(Arc::clone(&capture));
                    true
                } else {
                    false
                }
            };
            if !installed {
                let _ = capture.cancel().await;
                // The user already cancelled or started a new round; do not
                // surface the stale late start as an error.
                return Ok(());
            }
            recording_control.flush(session_id);
        }
        Err(error) => {
            let snapshot = inner
                .backend
                .services()
                .selection_voice
                .snapshot()
                .await
                .map_err(core_error)?;
            if snapshot.session_id != Some(session_id)
                || snapshot.phase == SelectionVoicePhase::Cancelled
            {
                clear_host_session(inner, session_id);
                return Ok(());
            }
            let _ = inner
                .backend
                .services()
                .selection_voice
                .cancel(Some(session_id))
                .await;
            clear_host_session(inner, session_id);
            return Err(core_error(error));
        }
    }
    Ok(())
}

async fn end_selection_voice_session(
    inner: &Arc<Inner>,
    expected_session: Option<CoreSessionId>,
) -> Result<(), String> {
    let snapshot = inner
        .backend
        .services()
        .selection_voice
        .snapshot()
        .await
        .map_err(core_error)?;
    if snapshot.phase != SelectionVoicePhase::Recording {
        return Ok(());
    }
    let session_id = snapshot
        .session_id
        .ok_or_else(|| "selectionVoiceSessionUnavailable".to_string())?;
    // A delayed mute event carries the old generation; Core's mark_processing
    // re-validates under the same state lock, so a cancel / new round that
    // happens after the check cannot slip through either.
    if expected_session.is_some_and(|expected| expected != session_id) {
        return Ok(());
    }
    inner
        .backend
        .services()
        .selection_voice
        .mark_processing(session_id)
        .await
        .map_err(core_error)?;
    // Stop meter before Transcribing so late levels cannot clobber processing
    // or bump capsule_event_epoch past the hide/Done gate.
    stop_selection_voice_level_meter(inner);
    // Keep processing visible until recognition and insertion finish.
    let _ = emit_capsule(inner, CapsuleState::Transcribing, 0.0, 0, None, None);
    let workflow: Result<EndWorkflowOutcome, String> = async {
        let capture = inner
            .selection_voice_capture
            .lock()
            .as_ref()
            .filter(|capture| capture.session_id() == session_id)
            .cloned()
            .ok_or_else(|| "selectionVoiceAsrUnavailable".to_string())?;
        // Keep the Arc in the registry during ASR finish so cancellation can
        // still abort the same provider. After finish returns, remove only this
        // handle; an old task must not clear the next round's capture.
        let result = capture.finish().await;
        {
            let mut current = inner.selection_voice_capture.lock();
            if current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &capture))
            {
                current.take();
            }
        }
        let transcript = result.map_err(core_error)?;
        if transcript.trim().is_empty() {
            inner
                .backend
                .services()
                .selection_voice
                .cancel(Some(session_id))
                .await
                .map_err(core_error)?;
            clear_host_session(inner, session_id);
            emit_capsule(
                inner,
                CapsuleState::Cancelled,
                0.0,
                0,
                Some("未识别到指令".into()),
                None,
            );
            schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
            return Ok(EndWorkflowOutcome::Finished);
        }

        // Intent classify + compose/edit LLM — match dictation Polishing orb.
        emit_capsule(inner, CapsuleState::Polishing, 0.0, 0, None, None);
        let disposition = inner
            .backend
            .services()
            .selection_voice
            .process_transcript(session_id, transcript)
            .await
            .map_err(core_error)?;
        continue_selection_voice_disposition(inner, disposition).await
    }
    .await;

    match workflow {
        Ok(EndWorkflowOutcome::AwaitingIntent) => Ok(()),
        Ok(EndWorkflowOutcome::Finished) => Ok(()),
        Err(error) => {
            let snapshot = inner
                .backend
                .services()
                .selection_voice
                .snapshot()
                .await
                .map_err(core_error)?;
            if snapshot.session_id != Some(session_id)
                || snapshot.phase == SelectionVoicePhase::Cancelled
            {
                // A stale provider result after cancellation only cleans up its
                // own owner; no error capsule is emitted, otherwise it could
                // overwrite the UI of the next round's recording in flight.
                clear_host_session(inner, session_id);
                return Ok(());
            }
            let _ = inner
                .backend
                .services()
                .selection_voice
                .cancel(Some(session_id))
                .await;
            clear_host_session(inner, session_id);
            emit_selection_voice_end_error(inner, &error);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndWorkflowOutcome {
    Finished,
    AwaitingIntent,
}

async fn continue_selection_voice_disposition(
    inner: &Arc<Inner>,
    disposition: SelectionVoiceDisposition,
) -> Result<EndWorkflowOutcome, String> {
    let route = inner
        .backend
        .services()
        .selection_voice
        .route_disposition(disposition)
        .await
        .map_err(core_error)?;
    match route {
        SelectionVoiceRoute::AwaitingIntent { .. } => {
            // Keep insertion target for later confirm; only drop meter + capsule claim
            // for Done dwell. Re-claim in continue_confirmed_selection_voice_intent.
            stop_selection_voice_level_meter(inner);
            release_selection_voice_capsule_claim(inner);
            finish_selection_voice_capsule(inner, Some("请选择意图".into()));
            inner.host.show_selection_voice_intent_prompt();
            Ok(EndWorkflowOutcome::AwaitingIntent)
        }
        SelectionVoiceRoute::QuestionCompleted { session_id } => {
            clear_host_session(inner, session_id);
            finish_selection_voice_capsule(inner, Some("已提交问题".into()));
            Ok(EndWorkflowOutcome::Finished)
        }
        SelectionVoiceRoute::EditConversationOpened { .. } => {
            // Keep opaque insertion_target for QA apply (prebound session still
            // resolves via target_for_session). Only drop meter + claim so Done
            // can dismiss and dictation is not blocked forever.
            stop_selection_voice_level_meter(inner);
            release_selection_voice_capsule_claim(inner);
            finish_selection_voice_capsule(inner, Some("已打开润色".into()));
            Ok(EndWorkflowOutcome::Finished)
        }
        SelectionVoiceRoute::ReadyToApply { preview } => {
            let coordinator = Coordinator {
                inner: Arc::clone(inner),
            };
            coordinator
                .confirm_selection_voice_preview(preview.text, None)
                .await?;
            Ok(EndWorkflowOutcome::Finished)
        }
    }
}

impl Coordinator {
    pub(crate) async fn continue_confirmed_selection_voice_intent(
        &self,
        session_id: CoreSessionId,
        disposition: SelectionVoiceDisposition,
    ) -> Result<(), String> {
        self.inner.host.hide_selection_voice_intent_prompt();
        // Re-claim so concurrent dictation cannot overwrite Polishing/Done while
        // post-intent LLM / apply still runs (AwaitingIntent released the claim).
        {
            let mut host = self.inner.selection_voice_host.lock();
            if host.target_session_id == Some(session_id) {
                host.owns_capsule = true;
                host.accepts_level = false;
            }
        }
        emit_capsule(&self.inner, CapsuleState::Polishing, 0.0, 0, None, None);
        let result = continue_selection_voice_disposition(&self.inner, disposition)
            .await
            .map(|_| ());
        if let Err(error) = &result {
            let _ = self
                .inner
                .backend
                .services()
                .selection_voice
                .cancel(Some(session_id))
                .await;
            clear_host_session(&self.inner, session_id);
            emit_selection_voice_end_error(&self.inner, error);
        }
        result
    }

    pub(crate) fn finish_cancelled_selection_voice_host(&self, session_id: CoreSessionId) {
        // Revoke the owner first so a start task still awaiting cannot install
        // resources; then take only the recording of the matching generation,
        // so an old cancel callback cannot abort a new round.
        let was_current = clear_host_session(&self.inner, session_id);
        let capture = {
            let mut current = self.inner.selection_voice_capture.lock();
            if current
                .as_ref()
                .is_some_and(|capture| capture.session_id() == session_id)
            {
                current.take()
            } else {
                None
            }
        };
        let spawner = self.inner.host.clone();
        spawner.spawn(async move {
            if let Some(capture) = capture {
                let _ = capture.cancel().await;
            }
        });
        if was_current {
            self.inner.host.hide_selection_voice_intent_prompt();
            emit_capsule(&self.inner, CapsuleState::Cancelled, 0.0, 0, None, None);
            schedule_capsule_idle(&self.inner, 0);
        }
    }

    pub(crate) fn bind_selection_voice_target(
        &self,
        session_id: CoreSessionId,
        insertion_target: SelectionInsertionTarget,
    ) -> Result<(), String> {
        bind_selection_voice_target_state(
            &self.inner.selection_voice_host,
            session_id,
            insertion_target,
        )
    }

    pub(crate) async fn confirm_selection_voice_preview(
        &self,
        text: String,
        qa_session_id: Option<SessionId>,
    ) -> Result<SelectionVoiceApplyOutcome, String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("selectionVoiceEmptyOutput".into());
        }

        if qa_session_id.is_some() {
            if !self.inner.qa_context.is_panel_visible() {
                return Err("selectionVoicePreviewUnavailable".into());
            }
        }
        let owner = qa_session_id.map(owner_session_id);
        let ticket = self
            .inner
            .backend
            .services()
            .selection_voice
            .begin_preview_apply(owner, text.clone())
            .map_err(core_error)?;
        let outcome = match self.apply_selection_voice_preview_ticket(&ticket) {
            Ok(outcome) => {
                self.inner
                    .backend
                    .services()
                    .selection_voice
                    .finish_preview_apply(ticket.ticket_id, outcome)
                    .await
                    .map_err(core_error)?;
                outcome
            }
            Err(error) => {
                let _ = self
                    .inner
                    .backend
                    .services()
                    .selection_voice
                    .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::Failed)
                    .await;
                return Err(error);
            }
        };

        self.finish_selection_voice_preview_host(ticket.session_id);
        Ok(outcome)
    }

    pub(crate) fn apply_selection_voice_preview_ticket(
        &self,
        ticket: &openless_core::SelectionVoiceApplyTicket,
    ) -> Result<SelectionVoiceApplyOutcome, String> {
        let prefs = self.inner.backend.get_preferences();
        let insertion_target = target_for_session(&self.inner, ticket.session_id)?;
        if !crate::selection::reactivate_selection_insertion_target(&insertion_target) {
            return Err("selectionVoiceTargetUnavailable".to_string());
        }
        // Empty source_text = caret insert (Help me write). Skip Ctrl+C content
        // re-validation that assumes a non-empty selection (#1014).
        if !ticket.source_text.trim().is_empty() {
            let validation = crate::selection::validate_selection_insertion_target(
                &insertion_target,
                &ticket.source_text,
            );
            if let Some(code) = validation.error_code() {
                return Err(code.to_string());
            }
        }
        let status = self.inner.inserter.insert(
            &ticket.replacement_text,
            prefs.restore_clipboard_after_paste,
            prefs.paste_shortcut,
        );
        selection_voice_apply_outcome(status)
    }

    pub(crate) fn finish_selection_voice_preview_host(&self, session_id: CoreSessionId) {
        clear_host_session(&self.inner, session_id);
        finish_selection_voice_capsule(&self.inner, Some("已完成".into()));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use openless_core::RecordingControlSink;

    #[test]
    fn selection_voice_accepts_level_only_for_claimed_owner_session() {
        let (coordinator, _, data_dir) =
            super::super::hotkey_loops::windows_less_computer_tests::fixture_coordinator(
                crate::types::HotkeyMode::Toggle,
                std::time::Duration::ZERO,
            );
        let id = CoreSessionId::new();
        let other = CoreSessionId::new();
        assert!(!selection_voice_accepts_level(
            &coordinator.inner,
            &id.to_string()
        ));
        {
            let mut host = coordinator.inner.selection_voice_host.lock();
            host.owns_capsule = true;
            host.accepts_level = true;
            host.target_session_id = Some(id);
        }
        assert!(selection_voice_accepts_level(
            &coordinator.inner,
            &id.to_string()
        ));
        assert!(!selection_voice_accepts_level(
            &coordinator.inner,
            &other.to_string()
        ));
        // Meter off while claim remains: processing must reject late levels.
        stop_selection_voice_level_meter(&coordinator.inner);
        assert!(!selection_voice_accepts_level(
            &coordinator.inner,
            &id.to_string()
        ));
        {
            let mut host = coordinator.inner.selection_voice_host.lock();
            host.accepts_level = true;
        }
        assert!(selection_voice_accepts_level(
            &coordinator.inner,
            &id.to_string()
        ));
        release_selection_voice_capsule_claim(&coordinator.inner);
        assert!(!selection_voice_accepts_level(
            &coordinator.inner,
            &id.to_string()
        ));
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn paste_dispatch_is_a_terminal_receipt_without_claiming_inserted() {
        for (status, wire) in [
            (InsertStatus::Inserted, "inserted"),
            (InsertStatus::PasteSent, "paste_sent"),
            (InsertStatus::CopiedFallback, "copied_fallback"),
        ] {
            let outcome = selection_voice_apply_outcome(status).unwrap();
            assert_eq!(serde_json::to_value(outcome).unwrap(), wire);
            assert!(outcome.may_have_applied());
        }
        for status in [InsertStatus::Failed, InsertStatus::NotRequested] {
            assert!(selection_voice_apply_outcome(status).is_err());
        }
    }

    #[tokio::test]
    async fn selection_cancel_revokes_a_starting_host_target_without_waiting_for_attach() {
        let (coordinator, _, data_dir) =
            super::super::hotkey_loops::windows_less_computer_tests::fixture_coordinator(
                crate::types::HotkeyMode::Toggle,
                std::time::Duration::ZERO,
            );
        let id = CoreSessionId::new();
        coordinator
            .inner
            .selection_voice_host
            .lock()
            .target_session_id = Some(id);
        let control = SelectionVoiceRecordingControl::new(&coordinator.inner);
        control
            .request(id, openless_core::RecordingControlAction::Cancel)
            .unwrap();
        assert_eq!(
            coordinator
                .inner
                .selection_voice_host
                .lock()
                .target_session_id,
            None
        );
        assert!(
            control.pending.lock().is_empty(),
            "取消不能等候永远不会发生的attach"
        );
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn selection_esc_clears_the_real_host_slot_and_stops_its_capture() {
        let (coordinator, recorder, data_dir) =
            super::super::hotkey_loops::windows_less_computer_tests::fixture_coordinator(
                crate::types::HotkeyMode::Toggle,
                std::time::Duration::ZERO,
            );
        let inner = &coordinator.inner;
        let id = inner
            .backend
            .services()
            .selection_voice
            .begin(SelectionCapture {
                text: "selected text".into(),
                source_app: None,
            })
            .await
            .unwrap();
        inner.selection_voice_host.lock().target_session_id = Some(id);
        let capture = inner
            .backend
            .start_selection_voice_capture(id, Arc::new(SelectionVoiceRecordingControl::new(inner)))
            .await
            .unwrap();
        *inner.selection_voice_capture.lock() = Some(Arc::new(capture));
        assert!(super::super::dictation::cancel_active_session(inner).await);
        assert_eq!(inner.selection_voice_host.lock().target_session_id, None);
        assert!(inner.selection_voice_capture.lock().is_none());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while recorder.stop_count() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}
