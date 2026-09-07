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
            // 取消必须先同步撤销Starting owner。它不能排队等待capture
            // 安装：Core已使token失效，迟到的原生句柄只会关闭，不会attach。
            Self::apply(&inner, session_id, action);
            return Ok(());
        }
        // 与 flush 串行化：不能在 flush 读空队列以后才把启动事件排入。
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
        "selectionVoiceNoSelection" => "请先选中文字".into(),
        "selectionVoiceTargetUnavailable" => "无法定位选区，请重试".into(),
        "selectionVoiceBusy" => "选区语音会话进行中".into(),
        other => other.into(),
    }
}

fn emit_selection_voice_begin_error(inner: &Arc<Inner>, error: &str) {
    emit_capsule(
        inner,
        CapsuleState::Error,
        0.0,
        0,
        Some(selection_voice_user_message(error)),
        None,
    );
    // 无选区等 begin 失败时胶囊会停在 Error；与听写 Done/Error 同口径，2s 后自动收回。
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
    let (selection_opt, insertion_target, capture_diag) =
        crate::selection::resolve_selection_workspace_capture_with_diag();
    log::info!(
        "[selection-voice] begin capture diag={}",
        capture_diag.summary()
    );
    let selection = match selection_opt {
        Some(selection) => selection,
        None => {
            log::warn!(
                "[selection-voice] begin failed: selectionVoiceNoSelection ({})",
                capture_diag.summary()
            );
            return Err("selectionVoiceNoSelection".into());
        }
    };
    if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
        log::warn!(
            "[selection-voice] begin failed: selectionVoiceTargetUnavailable ({})",
            capture_diag.summary()
        );
        return Err("selectionVoiceTargetUnavailable".into());
    }

    let session_id = inner
        .backend
        .services()
        .selection_voice
        .begin(SelectionCapture {
            text: selection.text,
            source_app: selection.source_app,
        })
        .await
        .map_err(core_error)?;
    {
        let mut host = inner.selection_voice_host.lock();
        host.target_session_id = Some(session_id);
        host.insertion_target = insertion_target;
    }

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
                // 启动中的取消会先撤销 target owner。检查 owner 与安装
                // capture 共用这段锁，迟到的设备启动不能覆盖下一轮句柄。
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
                // 用户已取消或开始新一轮，不再把迟到的旧启动显示为错误。
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
    // 延迟静音事件携带旧 generation；Core 的 mark_processing 会在同一
    // 状态锁内再次验证，保证检查后发生的取消/换轮也不会被越过。
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
    // 结束录音后熄灭胶囊；预览模式才打开华词面板，直接覆盖则静默处理。
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    schedule_capsule_idle(inner, 0);
    let workflow: Result<EndWorkflowOutcome, String> = async {
        let capture = inner
            .selection_voice_capture
            .lock()
            .as_ref()
            .filter(|capture| capture.session_id() == session_id)
            .cloned()
            .ok_or_else(|| "selectionVoiceAsrUnavailable".to_string())?;
        // ASR finish 期间仍保留注册表中的 Arc，让取消能中止相同 provider。
        // finish 返回后仅移除自己的句柄，旧任务不能清掉下一轮 capture。
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
            schedule_capsule_idle(inner, 2000);
            return Ok(EndWorkflowOutcome::Finished);
        }

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
                // 取消后的旧 provider 结果只清理自己的 owner；不再触发
                // 错误胶囊，否则可能把下一轮正在录音的 UI 覆盖掉。
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
            inner.host.show_selection_voice_intent_prompt();
            Ok(EndWorkflowOutcome::AwaitingIntent)
        }
        SelectionVoiceRoute::QuestionCompleted { session_id } => {
            clear_host_session(inner, session_id);
            Ok(EndWorkflowOutcome::Finished)
        }
        SelectionVoiceRoute::EditConversationOpened { .. } => Ok(EndWorkflowOutcome::Finished),
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
        // 先撤销 owner，阻止仍在 await 的启动任务安装资源；之后只取走
        // 对应 generation 的录音，旧取消回调不能中止新一轮。
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
            emit_capsule(&self.inner, CapsuleState::Idle, 0.0, 0, None, None);
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
        let validation = crate::selection::validate_selection_insertion_target(
            &insertion_target,
            &ticket.source_text,
        );
        if let Some(code) = validation.error_code() {
            return Err(code.to_string());
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
        emit_capsule(&self.inner, CapsuleState::Idle, 0.0, 0, None, None);
        schedule_capsule_idle(&self.inner, 0);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use openless_core::RecordingControlSink;

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
