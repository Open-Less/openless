//! Selection-voice edit session (issue #987 desktop MVP, Windows-first).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use chrono::Utc;
use serde::Serialize;
use tauri::Emitter;
use uuid::Uuid;

use super::{
    answer_qa_question_text, capture_external_focus_target, close_qa_panel, emit_capsule,
    open_qa_panel, polish_text, qa_event_target, qa_session, restore_focus_target_if_possible,
    schedule_capsule_idle, translate_text, CapsuleFeedback, Coordinator, Inner, QaPhase,
};
use crate::coordinator_state::{initial_session_id, new_session_id, SessionId};
use crate::edit_plan::{apply_edit_plan, parse_edit_plan, EditOperation, EditPlan};
use crate::selection::{SelectionContext, SelectionInsertionTarget};
use crate::selection_voice_intent::{
    parse_intent_classification_json, resolve_selection_voice_intent, SelectionVoiceIntent,
};
use crate::types::{
    CapsuleState, HistorySource, HotkeyMode, InsertStatus, OutputLanguagePreference, PolishMode,
    SelectionPolishOutputMode, SelectionVoiceIntentMode, UserPreferences,
};

static SELECTION_VOICE_BUSY: AtomicBool = AtomicBool::new(false);

/// 与听写 Auto 模式一致：短于该阈值视为点按（切换式锁存），否则视为按住说话。
const AUTO_HOLD_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(350);

/// 选区语音会话占用麦克风时，禁止再开听写/追问录音。
pub(super) fn selection_voice_blocks_other_recording(inner: &Arc<Inner>) -> bool {
    matches!(
        inner.selection_voice_state.lock().phase,
        SelectionVoicePhase::Recording
            | SelectionVoicePhase::Processing
            | SelectionVoicePhase::AwaitingIntent
    )
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

fn selection_voice_preview_mode(prefs: &UserPreferences) -> bool {
    prefs.selection_polish_output_mode != SelectionPolishOutputMode::DirectReplace
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
}

fn emit_selection_voice_end_error(inner: &Arc<Inner>, error: &str) {
    log::warn!("[selection-voice] workflow failed: {error}");
    let message = selection_voice_end_message(error);
    let preview_mode = selection_voice_preview_mode(&inner.prefs.get());
    let qa_visible = inner.qa_state.lock().panel_visible;
    if preview_mode && qa_visible {
        let mut qa = inner.qa_state.lock();
        qa.phase = QaPhase::Idle;
        let messages = qa.messages.clone();
        let session_id = qa.session_id;
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "error",
                    "session_id": session_id,
                    "error": message,
                    "messages": messages,
                    "edit_apply_available": false,
                    "edit_revert_available": false,
                }),
            );
        }
        emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
        schedule_capsule_idle(inner, 0);
    } else {
        emit_capsule(
            inner,
            CapsuleState::Error,
            0.0,
            0,
            Some(message),
            None,
        );
        schedule_capsule_idle(inner, 2500);
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionVoicePhase {
    Idle,
    Recording,
    Processing,
    AwaitingIntent,
}

#[derive(Debug, Clone)]
pub(super) struct SelectionVoiceSessionState {
    pub(super) phase: SelectionVoicePhase,
    pub(super) session_id: SessionId,
    pub(super) selection: Option<SelectionContext>,
    pub(super) insertion_target: SelectionInsertionTarget,
    pub(super) instruction_raw: Option<String>,
    pub(super) instruction_polished: Option<String>,
    /// Auto 模式判定短按/长按的按下时刻。
    pub(super) auto_press_at: Option<std::time::Instant>,
}

impl Default for SelectionVoiceSessionState {
    fn default() -> Self {
        Self {
            phase: SelectionVoicePhase::Idle,
            session_id: initial_session_id(),
            selection: None,
            insertion_target: SelectionInsertionTarget::default(),
            instruction_raw: None,
            instruction_polished: None,
            auto_press_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionVoicePreviewPayload {
    pub text: String,
    pub source_text: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionVoiceIntentPromptPayload {
    pub instruction: String,
    pub source_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSelectionVoiceIntentPrompt {
    session_id: SessionId,
    selection: SelectionContext,
    insertion_target: SelectionInsertionTarget,
    instruction_polished: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSelectionVoicePreview {
    qa_session_id: Option<SessionId>,
    insertion_target: SelectionInsertionTarget,
    source_text: String,
    preview_text: String,
    previous_preview_text: Option<String>,
    summary: Option<String>,
    source_app: Option<String>,
}

fn use_existing_qa_preview(
    preview_slot: &mut Option<PendingSelectionVoicePreview>,
    qa_session_id: SessionId,
) -> bool {
    match preview_slot.as_ref() {
        Some(preview) if preview.qa_session_id == Some(qa_session_id) => true,
        Some(_) => {
            preview_slot.take();
            false
        }
        None => false,
    }
}

fn clear_qa_bound_preview(preview_slot: &mut Option<PendingSelectionVoicePreview>) {
    if preview_slot
        .as_ref()
        .is_some_and(|preview| preview.qa_session_id.is_some())
    {
        preview_slot.take();
    }
}

pub(super) fn clear_qa_bound_selection_voice_preview(inner: &Arc<Inner>) {
    clear_qa_bound_preview(&mut inner.selection_voice_preview.lock());
}

fn parse_confirmed_selection_voice_intent(intent: &str) -> Result<SelectionVoiceIntent, String> {
    match intent {
        "question" => Ok(SelectionVoiceIntent::Question),
        "edit" => Ok(SelectionVoiceIntent::Edit),
        other => Err(format!("selectionVoiceInvalidIntent:{other}")),
    }
}

fn take_confirmed_selection_voice_intent_prompt(
    prompt_slot: &mut Option<PendingSelectionVoiceIntentPrompt>,
    intent: &str,
) -> Result<(PendingSelectionVoiceIntentPrompt, SelectionVoiceIntent), String> {
    let resolved = parse_confirmed_selection_voice_intent(intent)?;
    let prompt = prompt_slot
        .take()
        .ok_or_else(|| "selectionVoiceIntentPromptUnavailable".to_string())?;
    Ok((prompt, resolved))
}

fn apply_selection_voice_preview_transaction<F>(
    preview_slot: &mut Option<PendingSelectionVoicePreview>,
    owner: Option<SessionId>,
    apply: F,
) -> Result<(PendingSelectionVoicePreview, InsertStatus), String>
where
    F: FnOnce(&PendingSelectionVoicePreview) -> Result<InsertStatus, String>,
{
    let preview = preview_slot
        .as_ref()
        .filter(|preview| preview.qa_session_id == owner)
        .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())?;
    let status = apply(preview)?;
    if status == InsertStatus::Failed {
        return Err("selectionVoiceInsertFailed".into());
    }
    let preview = preview_slot
        .take()
        .expect("validated selection voice preview must remain present");
    Ok((preview, status))
}

fn selection_voice_session_active(state: &SelectionVoiceSessionState, session_id: SessionId) -> bool {
    state.session_id == session_id && state.phase != SelectionVoicePhase::Idle
}

fn selection_voice_recording_active(
    state: &SelectionVoiceSessionState,
    session_id: SessionId,
) -> bool {
    selection_voice_session_active(state, session_id) && state.phase == SelectionVoicePhase::Recording
}

pub(super) async fn handle_selection_voice_pressed(inner: &Arc<Inner>) {
    if !inner.prefs.get().selection_voice_enabled {
        return;
    }

    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.selection_voice_state.lock().phase;

    // 切换式 / Auto 锁存态的「再按一次停止」不能被子 busy 挡住。
    match (mode, phase) {
        (HotkeyMode::Toggle, SelectionVoicePhase::Recording)
        | (HotkeyMode::Auto, SelectionVoicePhase::Recording) => {
            if let Err(error) = end_selection_voice_session(inner).await {
                log::warn!("[selection-voice] end on stop press failed: {error}");
            }
            SELECTION_VOICE_BUSY.store(false, Ordering::Release);
            {
                let mut state = inner.selection_voice_state.lock();
                state.auto_press_at = None;
            }
            return;
        }
        _ => {}
    }

    if SELECTION_VOICE_BUSY.swap(true, Ordering::AcqRel) {
        return;
    }

    let begin_result = match (mode, phase) {
        (HotkeyMode::Toggle, SelectionVoicePhase::Idle) => {
            begin_selection_voice_session(inner).await
        }
        (HotkeyMode::Hold, SelectionVoicePhase::Idle) => {
            begin_selection_voice_session(inner).await
        }
        (HotkeyMode::Auto, SelectionVoicePhase::Idle) => {
            {
                let mut state = inner.selection_voice_state.lock();
                state.auto_press_at = Some(std::time::Instant::now());
            }
            begin_selection_voice_session(inner).await
        }
        _ => {
            SELECTION_VOICE_BUSY.store(false, Ordering::Release);
            return;
        }
    };

    if let Err(error) = begin_result {
        log::warn!("[selection-voice] begin failed: {error}");
        emit_selection_voice_begin_error(inner, &error);
        {
            let mut state = inner.selection_voice_state.lock();
            state.auto_press_at = None;
        }
    }
    SELECTION_VOICE_BUSY.store(false, Ordering::Release);
}

pub(super) async fn handle_selection_voice_released(inner: &Arc<Inner>) {
    if !inner.prefs.get().selection_voice_enabled {
        return;
    }
    let mode = inner.prefs.get().hotkey.mode;
    if mode == HotkeyMode::Toggle {
        return;
    }
    let phase = inner.selection_voice_state.lock().phase;
    if phase != SelectionVoicePhase::Recording {
        SELECTION_VOICE_BUSY.store(false, Ordering::Release);
        return;
    }
    if mode == HotkeyMode::Hold {
        if let Err(error) = end_selection_voice_session(inner).await {
            log::warn!("[selection-voice] end on hold release failed: {error}");
        }
        SELECTION_VOICE_BUSY.store(false, Ordering::Release);
        return;
    }
    if mode == HotkeyMode::Auto {
        let released_at = std::time::Instant::now();
        let held_long = {
            let mut state = inner.selection_voice_state.lock();
            state
                .auto_press_at
                .take()
                .map(|pressed_at| {
                    released_at.saturating_duration_since(pressed_at) >= AUTO_HOLD_THRESHOLD
                })
                .unwrap_or(false)
        };
        if held_long {
            if let Err(error) = end_selection_voice_session(inner).await {
                log::warn!("[selection-voice] end on auto hold release failed: {error}");
            }
        } else {
            log::info!("[selection-voice] auto short-tap latched; next press stops");
        }
        SELECTION_VOICE_BUSY.store(false, Ordering::Release);
    }
}

async fn begin_selection_voice_session(inner: &Arc<Inner>) -> Result<(), String> {
    if !matches!(inner.state.lock().phase, crate::coordinator_state::SessionPhase::Idle) {
        return Err("dictationActive".into());
    }
    if selection_voice_blocks_other_recording(inner) {
        return Err("selectionVoiceBusy".into());
    }

    let (selection_opt, insertion_target) = crate::selection::resolve_selection_workspace_capture();
    let selection = selection_opt.ok_or_else(|| "selectionVoiceNoSelection".to_string())?;
    if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
        return Err("selectionVoiceTargetUnavailable".into());
    }

    let session_id = new_session_id();
    {
        let mut state = inner.selection_voice_state.lock();
        state.phase = SelectionVoicePhase::Recording;
        state.session_id = session_id;
        state.selection = Some(selection);
        state.insertion_target = insertion_target;
        state.instruction_raw = None;
        state.instruction_polished = None;
    }

    emit_capsule(inner, CapsuleState::Recording, 0.0, 0, None, None);
    qa_session::start_selection_voice_recorder(inner, session_id).await?;
    Ok(())
}

async fn end_selection_voice_session(inner: &Arc<Inner>) -> Result<(), String> {
    let session_id = {
        let state = inner.selection_voice_state.lock();
        if state.phase != SelectionVoicePhase::Recording {
            return Ok(());
        }
        state.session_id
    };
    {
        let mut state = inner.selection_voice_state.lock();
        state.phase = SelectionVoicePhase::Processing;
    }
    // 结束录音后熄灭胶囊；预览模式才打开华词面板，直接覆盖则静默处理。
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    schedule_capsule_idle(inner, 0);
    let preview_mode = selection_voice_preview_mode(&inner.prefs.get());
    let early_qa_session = if preview_mode {
        open_qa_panel(inner);
        let mut qa = inner.qa_state.lock();
        qa.session_id = new_session_id();
        qa.phase = QaPhase::Processing;
        qa.panel_visible = true;
        let session_id = qa.session_id;
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "thinking",
                    "session_id": session_id,
                    "messages": [],
                }),
            );
        }
        Some(session_id)
    } else {
        None
    };

    let workflow: Result<EndWorkflowOutcome, String> = async {
        let transcript = qa_session::finish_selection_voice_transcript(inner, session_id).await?;
        if transcript.trim().is_empty() {
            reset_selection_voice_session(inner);
            if let Some(qa_session) = early_qa_session {
                if let Some(app) = inner.app.lock().clone() {
                    let _ = app.emit_to(
                        qa_event_target(),
                        "qa:state",
                        serde_json::json!({
                            "kind": "error",
                            "session_id": qa_session,
                            "error": "未识别到指令",
                            "messages": [],
                        }),
                    );
                }
                let mut qa = inner.qa_state.lock();
                if qa.session_id == qa_session {
                    qa.phase = QaPhase::Idle;
                }
            } else {
                emit_capsule(
                    inner,
                    CapsuleState::Cancelled,
                    0.0,
                    0,
                    Some("未识别到指令".into()),
                    None,
                );
                schedule_capsule_idle(inner, 2000);
            }
            return Ok(EndWorkflowOutcome::Finished);
        }

        let (selection, insertion_target) = {
            let state = inner.selection_voice_state.lock();
            (
                state.selection.clone(),
                state.insertion_target.clone(),
            )
        };
        let selection = selection.ok_or_else(|| "selectionVoiceNoSelection".to_string())?;
        let rules = inner.correction_rules.list().map_err(|e| e.to_string())?;
        let instruction_raw = crate::correction::apply_correction_rules(&transcript, &rules);

        let instruction_polished = polish_selection_voice_instruction(inner, &instruction_raw).await?;
        {
            let mut state = inner.selection_voice_state.lock();
            state.instruction_raw = Some(instruction_raw);
            state.instruction_polished = Some(instruction_polished.clone());
        }

        let prefs = inner.prefs.get();
        if prefs.selection_voice_intent_mode == SelectionVoiceIntentMode::Prompt {
            *inner.selection_voice_intent_prompt.lock() = Some(PendingSelectionVoiceIntentPrompt {
                session_id,
                selection: selection.clone(),
                insertion_target: insertion_target.clone(),
                instruction_polished: instruction_polished.clone(),
            });
            {
                let mut state = inner.selection_voice_state.lock();
                state.phase = SelectionVoicePhase::AwaitingIntent;
            }
            if let Some(qa_session) = early_qa_session {
                let mut qa = inner.qa_state.lock();
                if qa.session_id == qa_session {
                    qa.phase = QaPhase::Idle;
                }
            }
            if let Some(app) = inner.app.lock().clone() {
                crate::show_selection_voice_intent_prompt(&app);
            }
            return Ok(EndWorkflowOutcome::AwaitingIntent);
        }

        let intent = resolve_intent_with_optional_llm(inner, &instruction_polished).await;
        if preview_mode {
            let edit_mode = intent == SelectionVoiceIntent::Edit;
            let mut qa = inner.qa_state.lock();
            qa.edit_instruction_mode = edit_mode;
            let session_id = qa.session_id;
            if let Some(app) = inner.app.lock().clone() {
                let _ = app.emit_to(
                    qa_event_target(),
                    "qa:state",
                    serde_json::json!({
                        "kind": "thinking",
                        "session_id": session_id,
                        "messages": qa.messages.clone(),
                        "edit_instruction_mode": edit_mode,
                    }),
                );
            }
        }
        continue_selection_voice_with_intent(
            inner,
            session_id,
            &selection,
            &insertion_target,
            &instruction_polished,
            intent,
        )
        .await?;
        Ok(EndWorkflowOutcome::Finished)
    }
    .await;

    match workflow {
        Ok(EndWorkflowOutcome::AwaitingIntent) => Ok(()),
        Ok(EndWorkflowOutcome::Finished) => {
            reset_selection_voice_session(inner);
            Ok(())
        }
        Err(error) => {
            reset_selection_voice_session(inner);
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

async fn continue_selection_voice_with_intent(
    inner: &Arc<Inner>,
    session_id: SessionId,
    selection: &SelectionContext,
    insertion_target: &SelectionInsertionTarget,
    instruction_polished: &str,
    intent: SelectionVoiceIntent,
) -> Result<(), String> {
    match intent {
        SelectionVoiceIntent::Question => {
            run_selection_voice_question(inner, session_id, selection, instruction_polished)
                .await?;
        }
        SelectionVoiceIntent::Edit => {
            run_selection_voice_edit(
                inner,
                selection,
                insertion_target,
                instruction_polished,
            )
            .await?;
        }
    }
    Ok(())
}

fn reset_selection_voice_session(inner: &Arc<Inner>) {
    let mut state = inner.selection_voice_state.lock();
    *state = SelectionVoiceSessionState::default();
}

async fn polish_selection_voice_instruction(
    inner: &Arc<Inner>,
    instruction_raw: &str,
) -> Result<String, String> {
    qa_session::polish_voice_instruction(inner, instruction_raw).await
}

async fn resolve_intent_with_optional_llm(
    inner: &Arc<Inner>,
    instruction_polished: &str,
) -> SelectionVoiceIntent {
    let prefs = inner.prefs.get();
    let heuristic = resolve_selection_voice_intent(&prefs, instruction_polished);
    if prefs.selection_voice_intent_mode != SelectionVoiceIntentMode::Auto {
        log::info!(
            "[selection-voice] intent={:?} source={}",
            heuristic.intent,
            heuristic.source
        );
        return heuristic.intent;
    }

    // Auto：默认走服务配置的 LLM 判问句 vs 编辑；启发式仅作 LLM 失败时的兜底。
    let mut classification = heuristic;
    let system = crate::polish::prompts::selection_voice_intent_classification_prompt();
    let mut llm_call = None;
    let mut polish_ms = None;
    match polish_text(
        instruction_polished,
        PolishMode::Light,
        &[],
        &system,
        &prefs.working_languages,
        prefs.chinese_script_preference,
        prefs.output_language_preference,
        prefs.llm_thinking_enabled,
        None,
        None,
        &[],
        &mut llm_call,
        &mut polish_ms,
        false,
    )
    .await
    {
        Ok(raw) => {
            if let Some(intent) = parse_intent_classification_json(&raw) {
                classification.intent = intent;
                classification.source = "auto_llm";
            } else {
                log::warn!(
                    "[selection-voice] intent LLM unparsable; fallback to heuristic {:?} preview={}",
                    classification.intent,
                    raw.chars().take(120).collect::<String>()
                );
                classification.source = "auto_heuristic_fallback";
            }
        }
        Err(error) => {
            log::warn!(
                "[selection-voice] intent LLM failed: {error}; fallback to heuristic {:?}",
                classification.intent
            );
            classification.source = "auto_heuristic_fallback";
        }
    }
    log::info!(
        "[selection-voice] intent={:?} source={} instruction_len={}",
        classification.intent,
        classification.source,
        instruction_polished.chars().count()
    );
    classification.intent
}

async fn run_selection_voice_question(
    inner: &Arc<Inner>,
    _session_id: SessionId,
    selection: &SelectionContext,
    instruction_polished: &str,
) -> Result<(), String> {
    let need_open = !inner.qa_state.lock().panel_visible;
    if need_open {
        open_qa_panel(inner);
    }
    let qa_session_id = {
        let mut qa = inner.qa_state.lock();
        qa.selection = Some(selection.clone());
        qa.edit_instruction_mode = false;
        if need_open {
            qa.session_id = new_session_id();
            qa.messages.clear();
        }
        qa.phase = QaPhase::Processing;
        qa.panel_visible = true;
        qa.session_id
    };
    answer_qa_question_text(
        inner,
        instruction_polished.to_string(),
        0,
        qa_session_id,
        None,
        CapsuleFeedback::Hide,
    )
    .await
}

async fn run_selection_voice_edit(
    inner: &Arc<Inner>,
    selection: &SelectionContext,
    insertion_target: &SelectionInsertionTarget,
    instruction_polished: &str,
) -> Result<(), String> {
    let prefs = inner.prefs.get();
    let preview_mode = selection_voice_preview_mode(&prefs);
    let qa_session_id = if preview_mode {
        let need_open = !inner.qa_state.lock().panel_visible;
        if need_open {
            open_qa_panel(inner);
        }
        let mut qa = inner.qa_state.lock();
        qa.selection = Some(selection.clone());
        qa.edit_instruction_mode = true;
        if need_open {
            qa.session_id = new_session_id();
            qa.messages.clear();
            qa.edit_instruction_mode = true;
        }
        qa.phase = QaPhase::Processing;
        qa.panel_visible = true;
        let session_id = qa.session_id;
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "thinking",
                    "session_id": session_id,
                    "selection_preview": selection.text.chars().take(60).collect::<String>(),
                    "messages": qa.messages.clone(),
                    "edit_instruction_mode": true,
                }),
            );
        }
        session_id
    } else {
        emit_capsule(
            inner,
            CapsuleState::Polishing,
            0.0,
            0,
            Some("正在生成编辑…".into()),
            None,
        );
        new_session_id()
    };

    let plan = generate_edit_plan(inner, &selection.text, instruction_polished).await?;
    let preview = apply_edit_plan(&selection.text, &plan).map_err(|error| error.to_string())?;
    if preview == selection.text {
        log::warn!(
            "[selection-voice] edit result identical to source (chars={})",
            preview.chars().count()
        );
    }

    let direct = !preview_mode;

    if preview_mode {
        let user_content = format!("# 编辑指令\n{instruction_polished}");
        let summary_line = plan
            .summary
            .as_deref()
            .map(|s| format!("（{s}）\n\n"))
            .unwrap_or_default();
        let assistant_content = format!("{summary_line}{preview}");

        let mut qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return Ok(());
        }
        *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
            qa_session_id: Some(qa_session_id),
            insertion_target: insertion_target.clone(),
            source_text: selection.text.clone(),
            preview_text: preview.clone(),
            previous_preview_text: None,
            summary: plan.summary.clone(),
            source_app: selection.source_app.clone(),
        });
        qa.messages = vec![
            crate::types::QaChatMessage {
                role: "user".into(),
                content: user_content,
                selection_text: Some(selection.text.clone()),
            },
            crate::types::QaChatMessage {
                role: "assistant".into(),
                content: assistant_content,
                selection_text: None,
            },
        ];
        qa.phase = QaPhase::Idle;
        qa.edit_instruction_mode = true;
        let messages = qa.messages.clone();
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "answer",
                    "session_id": qa_session_id,
                    "messages": messages,
                    "edit_apply_available": true,
                    "edit_revert_available": false,
                    "edit_instruction_mode": true,
                }),
            );
        }
    }

    if direct {
        *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
            qa_session_id: None,
            insertion_target: insertion_target.clone(),
            source_text: selection.text.clone(),
            preview_text: preview.clone(),
            previous_preview_text: None,
            summary: plan.summary.clone(),
            source_app: selection.source_app.clone(),
        });
        let coord = Coordinator {
            inner: Arc::clone(inner),
        };
        coord.confirm_selection_voice_preview(preview, None)?;
    }

    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    schedule_capsule_idle(inner, 0);
    Ok(())
}

async fn generate_edit_plan(
    inner: &Arc<Inner>,
    draft: &str,
    instruction_polished: &str,
) -> Result<EditPlan, String> {
    let prefs = inner.prefs.get();
    if selection_voice_instruction_looks_like_translation(instruction_polished) {
        let target = infer_selection_voice_translation_target(instruction_polished, &prefs);
        if !target.is_empty() {
            log::info!(
                "[selection-voice] translation edit path target={target} instruction={instruction_polished}"
            );
            return generate_translation_edit_plan(inner, draft, &target).await;
        }
    }

    let safe_draft =
        crate::polish::prompts::sanitize_for_xml_envelope(draft, "draft");
    let safe_instruction = crate::polish::prompts::sanitize_for_xml_envelope(
        instruction_polished,
        "instruction",
    );
    let user_prompt = format!(
        "<field_context></field_context>\n<draft>\n{safe_draft}\n</draft>\n\n<instruction>\n{safe_instruction}\n</instruction>"
    );
    let system = crate::polish::prompts::voice_edit_system_prompt();
    let mut llm_call = None;
    let mut polish_ms = None;
    let raw = polish_text(
        &user_prompt,
        PolishMode::Light,
        &[],
        &system,
        &prefs.working_languages,
        prefs.chinese_script_preference,
        prefs.output_language_preference,
        prefs.llm_thinking_enabled,
        None,
        None,
        &[],
        &mut llm_call,
        &mut polish_ms,
        false,
    )
    .await
    .map_err(|error| error.to_string())?;
    match parse_edit_plan(&raw) {
        Ok(plan) => {
            if plan.operations.is_empty() {
                log::warn!("[selection-voice] edit plan parsed with zero operations");
                if selection_voice_instruction_looks_like_translation(instruction_polished) {
                    let target = infer_selection_voice_translation_target(
                        instruction_polished,
                        &prefs,
                    );
                    if !target.is_empty() {
                        return generate_translation_edit_plan(inner, draft, &target).await;
                    }
                }
                return Err("edit plan has no operations".into());
            }
            Ok(plan)
        }
        Err(error) => {
            log::warn!(
                "[selection-voice] edit plan parse failed: {error}; preview={}",
                raw.chars().take(240).collect::<String>()
            );
            if selection_voice_instruction_looks_like_translation(instruction_polished) {
                let target = infer_selection_voice_translation_target(
                    instruction_polished,
                    &prefs,
                );
                if !target.is_empty() {
                    log::info!(
                        "[selection-voice] falling back to translation edit path target={target}"
                    );
                    return generate_translation_edit_plan(inner, draft, &target).await;
                }
            }
            Err(error)
        }
    }
}

fn selection_voice_instruction_looks_like_translation(instruction: &str) -> bool {
    let lower = instruction.to_lowercase();
    lower.contains("翻译")
        || lower.contains("译成")
        || lower.contains("译为")
        || lower.contains("translate")
        || lower.contains("translation")
}

fn language_label_from_fragment(fragment: &str) -> Option<String> {
    let token = fragment
        .trim()
        .split(|c: char| {
            c == '，' || c == ',' || c == '。' || c == '.' || c == ' ' || c == '；' || c == ';'
        })
        .next()
        .unwrap_or(fragment)
        .trim()
        .to_lowercase();
    if token.is_empty() {
        return None;
    }
    if token.contains("英文") || token.contains("英语") || token.contains("english") {
        return Some("English".into());
    }
    if token.contains("繁体") || token.contains("繁體") {
        return Some("繁體中文".into());
    }
    if token.contains("简体") || token.contains("簡體") || token.contains("中文") {
        return Some("简体中文".into());
    }
    if token.contains("日文") || token.contains("日语") || token.contains("japanese") {
        return Some("日本語".into());
    }
    if token.contains("韩文") || token.contains("韩语") || token.contains("korean") {
        return Some("한국어".into());
    }
    None
}

fn extract_translation_target_after_cue(instruction: &str) -> Option<String> {
    let lower = instruction.to_lowercase();
    let cues = [
        "翻译成",
        "译成",
        "译为",
        "翻译为",
        "翻譯成",
        "譯成",
        "translate to",
        "translate into",
        "translated to",
    ];
    for cue in cues {
        if let Some(idx) = lower.find(cue) {
            let after = instruction[idx + cue.len()..].trim();
            if let Some(lang) = language_label_from_fragment(after) {
                return Some(lang);
            }
        }
    }
    None
}

fn infer_selection_voice_translation_target(
    instruction: &str,
    prefs: &UserPreferences,
) -> String {
    if let Some(target) = extract_translation_target_after_cue(instruction) {
        return target;
    }
    let lower = instruction.to_lowercase();
    // 无「译成/translate to」时，才用指令里出现的语言词作兜底（可能指源语言，慎用）。
    if lower.contains("日文") || lower.contains("日语") || lower.contains("japanese") {
        return "日本語".into();
    }
    if lower.contains("韩文") || lower.contains("韩语") || lower.contains("korean") {
        return "한국어".into();
    }
    if lower.contains("繁体") || lower.contains("繁體") {
        return "繁體中文".into();
    }
    if lower.contains("简体") || lower.contains("簡體") || lower.contains("中文") {
        return "简体中文".into();
    }
    if lower.contains("英文") || lower.contains("英语") || lower.contains("english") {
        return "English".into();
    }
    let from_prefs = prefs.translation_target_language.trim();
    if !from_prefs.is_empty() {
        return from_prefs.to_string();
    }
    match prefs.output_language_preference {
        OutputLanguagePreference::En => "English".into(),
        OutputLanguagePreference::Ja => "日本語".into(),
        OutputLanguagePreference::Ko => "한국어".into(),
        OutputLanguagePreference::ZhCn => "简体中文".into(),
        OutputLanguagePreference::ZhTw => "繁體中文".into(),
        OutputLanguagePreference::Auto => String::new(),
    }
}

async fn generate_translation_edit_plan(
    inner: &Arc<Inner>,
    draft: &str,
    target_language: &str,
) -> Result<EditPlan, String> {
    let prefs = inner.prefs.get();
    let mut llm_call = None;
    let mut polish_ms = None;
    let translated_raw = translate_text(
        draft,
        target_language,
        &prefs.working_languages,
        prefs.chinese_script_preference,
        prefs.output_language_preference,
        prefs.llm_thinking_enabled,
        None,
        &mut llm_call,
        &mut polish_ms,
    )
    .await
    .map_err(|error| error.to_string())?;
    let translated = clean_translation_edit_output(&translated_raw);
    if translated.trim().is_empty() {
        return Err("translation produced empty text".into());
    }
    if translated == draft {
        return Err(format!(
            "translation unchanged for target={target_language}"
        ));
    }
    Ok(EditPlan {
        operations: vec![EditOperation::FullRewrite {
            text: translated,
        }],
        summary: Some(format!("翻译为{target_language}")),
    })
}

fn clean_translation_edit_output(raw: &str) -> String {
    let mut text = crate::polish::clean_json_llm_output(raw);
    // Models sometimes wrap translations in markdown headings / fences.
    loop {
        let trimmed = text.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            if let Some((_, after)) = rest.split_once('\n') {
                text = after.to_string();
                continue;
            }
            if rest.starts_with("Processing") || rest.starts_with("处理") {
                text = String::new();
                break;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            if let Some((_, after)) = rest.split_once('\n') {
                text = after.to_string();
                continue;
            }
        }
        break;
    }
    text.trim().to_string()
}

pub(super) async fn submit_selection_voice_follow_up_edit(
    inner: &Arc<Inner>,
    instruction: String,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let instruction = instruction.trim().to_string();
    if instruction.is_empty() {
        return Ok(());
    }

    let pending = {
        let mut qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return Err("QA is busy".to_string());
        }
        // Idle：文字提交入口；Processing：麦克风 end_qa_session 已进入处理中。
        if qa.phase != QaPhase::Idle && qa.phase != QaPhase::Processing {
            return Err("QA is busy".to_string());
        }
        let pending = {
            let mut preview_slot = inner.selection_voice_preview.lock();
            if !use_existing_qa_preview(&mut preview_slot, qa_session_id) {
                return Err("selectionVoicePreviewUnavailable".to_string());
            }
            preview_slot
                .as_ref()
                .cloned()
                .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())?
        };
        qa.phase = QaPhase::Processing;
        qa.messages.push(crate::types::QaChatMessage {
            role: "user".into(),
            content: format!("# 编辑指令\n{instruction}"),
            selection_text: None,
        });
        let messages = qa.messages.clone();
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "thinking",
                    "session_id": qa_session_id,
                    "messages": messages,
                }),
            );
        }
        pending
    };

    let plan =
        generate_edit_plan(inner, &pending.preview_text, &instruction).await?;
    let new_preview =
        apply_edit_plan(&pending.preview_text, &plan).map_err(|error| error.to_string())?;

    let summary_line = plan
        .summary
        .as_deref()
        .map(|s| format!("（{s}）\n\n"))
        .unwrap_or_default();
    let assistant_content = format!("{summary_line}{new_preview}");

    let mut qa = inner.qa_state.lock();
    if qa.session_id != qa_session_id || !qa.panel_visible {
        return Ok(());
    }
    *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
        qa_session_id: Some(qa_session_id),
        insertion_target: pending.insertion_target,
        source_text: pending.source_text,
        preview_text: new_preview.clone(),
        previous_preview_text: Some(pending.preview_text),
        summary: plan.summary.clone(),
        source_app: pending.source_app,
    });
    qa.messages.push(crate::types::QaChatMessage {
        role: "assistant".into(),
        content: assistant_content,
        selection_text: None,
    });
    qa.phase = QaPhase::Idle;
    qa.edit_instruction_mode = true;
    let messages = qa.messages.clone();
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "answer",
                "session_id": qa_session_id,
                "messages": messages,
                "edit_apply_available": true,
                "edit_revert_available": true,
                "edit_instruction_mode": true,
            }),
        );
    }
    Ok(())
}

/// 划词提问面板勾选「编辑指令」且尚无 preview：对当前选区跑一轮编辑写入预览。
pub(super) async fn submit_selection_voice_edit_from_qa_selection(
    inner: &Arc<Inner>,
    instruction: String,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let instruction = instruction.trim().to_string();
    if instruction.is_empty() {
        return Ok(());
    }

    let selection = {
        let qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return Err("QA is busy".to_string());
        }
        qa.selection.clone()
    };
    let selection = match selection.filter(|s| !s.text.trim().is_empty()) {
        Some(selection) => selection,
        None => {
            #[cfg(target_os = "windows")]
            {
                let saved_target = {
                    let mut state = inner.qa_state.lock();
                    if let Some(current_external) = capture_external_focus_target() {
                        state.qa_focus_target = Some(current_external);
                    }
                    state.qa_focus_target
                };
                let _ = restore_focus_target_if_possible(saved_target);
            }
            let captured = crate::selection::capture_selection_with_status().selection;
            #[cfg(target_os = "windows")]
            if let Some(app) = inner.app.lock().clone() {
                crate::refocus_qa_window(&app);
            }
            let Some(selection) = captured.filter(|s| !s.text.trim().is_empty()) else {
                return Err("无选区可编辑".to_string());
            };
            {
                let mut qa = inner.qa_state.lock();
                if qa.session_id != qa_session_id {
                    return Ok(());
                }
                qa.selection = Some(selection.clone());
            }
            selection
        }
    };

    let insertion_target = {
        #[cfg(target_os = "windows")]
        {
            let saved_target = {
                let mut state = inner.qa_state.lock();
                if let Some(current_external) = capture_external_focus_target() {
                    state.qa_focus_target = Some(current_external);
                }
                state.qa_focus_target
            };
            let _ = restore_focus_target_if_possible(saved_target);
        }
        let target = crate::selection::capture_selection_insertion_target();
        #[cfg(target_os = "windows")]
        if let Some(app) = inner.app.lock().clone() {
            crate::refocus_qa_window(&app);
        }
        target
    };

    {
        let mut qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return Err("QA is busy".to_string());
        }
        if qa.phase != QaPhase::Idle && qa.phase != QaPhase::Processing {
            return Err("QA is busy".to_string());
        }
        qa.phase = QaPhase::Processing;
        qa.messages.push(crate::types::QaChatMessage {
            role: "user".into(),
            content: format!("# 编辑指令\n{instruction}"),
            selection_text: Some(selection.text.clone()),
        });
        let messages = qa.messages.clone();
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "thinking",
                    "session_id": qa_session_id,
                    "selection_preview": selection.text.chars().take(60).collect::<String>(),
                    "messages": messages,
                    "edit_instruction_mode": true,
                }),
            );
        }
    }

    let plan = generate_edit_plan(inner, &selection.text, &instruction).await?;
    let preview = apply_edit_plan(&selection.text, &plan).map_err(|error| error.to_string())?;

    let summary_line = plan
        .summary
        .as_deref()
        .map(|s| format!("（{s}）\n\n"))
        .unwrap_or_default();
    let assistant_content = format!("{summary_line}{preview}");

    let mut qa = inner.qa_state.lock();
    if qa.session_id != qa_session_id || !qa.panel_visible {
        return Ok(());
    }
    *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
        qa_session_id: Some(qa_session_id),
        insertion_target,
        source_text: selection.text.clone(),
        preview_text: preview.clone(),
        previous_preview_text: None,
        summary: plan.summary.clone(),
        source_app: selection.source_app.clone(),
    });
    qa.messages.push(crate::types::QaChatMessage {
        role: "assistant".into(),
        content: assistant_content,
        selection_text: None,
    });
    qa.phase = QaPhase::Idle;
    qa.edit_instruction_mode = true;
    let messages = qa.messages.clone();
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "answer",
                "session_id": qa_session_id,
                "messages": messages,
                "edit_apply_available": true,
                "edit_revert_available": false,
                "edit_instruction_mode": true,
            }),
        );
    }
    Ok(())
}

/// 划词提问面板「编辑指令」统一入口：有 preview 则 follow-up，否则对选区首轮编辑。
pub(super) async fn apply_qa_panel_edit_instruction(
    inner: &Arc<Inner>,
    instruction: String,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let has_preview = {
        let qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return Err("QA is busy".to_string());
        }
        use_existing_qa_preview(
            &mut inner.selection_voice_preview.lock(),
            qa_session_id,
        )
    };
    if has_preview {
        return submit_selection_voice_follow_up_edit(inner, instruction, qa_session_id).await;
    }
    submit_selection_voice_edit_from_qa_selection(inner, instruction, qa_session_id).await
}

pub(super) fn revert_selection_voice_preview_state(
    inner: &Arc<Inner>,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let mut qa = inner.qa_state.lock();
    if qa.session_id != qa_session_id || !qa.panel_visible {
        return Err("selectionVoicePreviewUnavailable".into());
    }
    let mut preview_slot = inner.selection_voice_preview.lock();
    let Some(pending) = preview_slot.as_mut() else {
        return Err("selectionVoicePreviewUnavailable".into());
    };
    if pending.qa_session_id != Some(qa_session_id) {
        return Err("selectionVoicePreviewUnavailable".into());
    }
    let previous = pending
        .previous_preview_text
        .clone()
        .ok_or_else(|| "selectionVoiceRevertUnavailable".to_string())?;
    pending.preview_text = previous;
    pending.previous_preview_text = None;
    pending.summary = None;

    if let Some(last) = qa.messages.last_mut() {
        if last.role == "assistant" {
            last.content = pending.preview_text.clone();
        }
    }
    qa.phase = QaPhase::Idle;
    let messages = qa.messages.clone();
    let edit_mode = qa.edit_instruction_mode;
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit_to(
            qa_event_target(),
            "qa:state",
            serde_json::json!({
                "kind": "answer",
                "session_id": qa_session_id,
                "messages": messages,
                "edit_apply_available": true,
                "edit_revert_available": false,
                "edit_instruction_mode": edit_mode,
            }),
        );
    }
    Ok(())
}

impl Coordinator {
    pub(crate) fn selection_voice_intent_prompt(
        &self,
    ) -> Option<SelectionVoiceIntentPromptPayload> {
        self.inner
            .selection_voice_intent_prompt
            .lock()
            .as_ref()
            .map(|prompt| SelectionVoiceIntentPromptPayload {
                instruction: prompt.instruction_polished.clone(),
                source_text: prompt.selection.text.clone(),
            })
    }

    pub(crate) fn cancel_selection_voice_intent_prompt(&self) {
        self.inner.selection_voice_intent_prompt.lock().take();
        reset_selection_voice_session(&self.inner);
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_voice_intent_prompt(&app);
        }
    }

    pub(crate) async fn confirm_selection_voice_intent_prompt(
        &self,
        intent: String,
    ) -> Result<(), String> {
        let (prompt, resolved) = take_confirmed_selection_voice_intent_prompt(
            &mut self.inner.selection_voice_intent_prompt.lock(),
            &intent,
        )?;
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_voice_intent_prompt(&app);
        }
        let result = continue_selection_voice_with_intent(
            &self.inner,
            prompt.session_id,
            &prompt.selection,
            &prompt.insertion_target,
            &prompt.instruction_polished,
            resolved,
        )
        .await;
        reset_selection_voice_session(&self.inner);
        if let Err(error) = &result {
            emit_selection_voice_end_error(&self.inner, error);
        }
        result
    }

    pub(crate) fn selection_voice_preview(
        &self,
        qa_session_id: SessionId,
    ) -> Option<SelectionVoicePreviewPayload> {
        let qa = self.inner.qa_state.lock();
        if qa.session_id != qa_session_id || !qa.panel_visible {
            return None;
        }
        self.inner
            .selection_voice_preview
            .lock()
            .as_ref()
            .filter(|preview| preview.qa_session_id == Some(qa_session_id))
            .map(|preview| SelectionVoicePreviewPayload {
                text: preview.preview_text.clone(),
                source_text: preview.source_text.clone(),
                summary: preview.summary.clone(),
            })
    }

    pub(crate) fn confirm_selection_voice_preview(
        &self,
        text: String,
        qa_session_id: Option<SessionId>,
    ) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("selectionVoiceEmptyOutput".into());
        }

        let qa = if let Some(qa_session_id) = qa_session_id {
            let qa = self.inner.qa_state.lock();
            if qa.session_id != qa_session_id || !qa.panel_visible {
                return Err("selectionVoicePreviewUnavailable".into());
            }
            Some(qa)
        } else {
            None
        };
        let prefs = self.inner.prefs.get();
        let mut preview_slot = self.inner.selection_voice_preview.lock();
        let (preview, status) = apply_selection_voice_preview_transaction(
            &mut preview_slot,
            qa_session_id,
            |preview| {
                if !crate::selection::reactivate_selection_insertion_target(
                    &preview.insertion_target,
                ) {
                    return Err("selectionVoiceTargetUnavailable".into());
                }
                let validation = crate::selection::validate_selection_insertion_target(
                    &preview.insertion_target,
                    &preview.source_text,
                );
                if let Some(code) = validation.error_code() {
                    return Err(code.to_string());
                }
                Ok(self.inner.inserter.insert(
                    &text,
                    prefs.restore_clipboard_after_paste,
                    prefs.paste_shortcut,
                ))
            },
        )?;
        drop(preview_slot);
        drop(qa);

        let dictionary_entry_count = self
            .inner
            .vocab
            .record_hits(&text)
            .ok()
            .map(|hits| hits.min(u32::MAX as u64) as u32);
        let front = crate::types::split_front_app_opt(preview.source_app.as_deref());
        let session = crate::types::DictationSession {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            source: HistorySource::SelectionVoiceEdit,
            raw_transcript: preview.source_text,
            asr_transcript: None,
            final_text: text.clone(),
            mode: PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: preview.summary.clone(),
            app_bundle_id: front.bundle_id,
            app_name: front.name,
            insert_status: status,
            error_code: None,
            duration_ms: None,
            dictionary_entry_count,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: None,
            llm_model: None,
            pipeline_mode: None,
            asr_ms: None,
            polish_ms: None,
        };
        if let Err(error) = self.inner.history.append_with_retention(
            session,
            prefs.history_retention_days,
            prefs.history_max_entries,
        ) {
            log::warn!("[selection-voice] history append failed: {error}");
        }
        if qa_session_id.is_some() {
            close_qa_panel(&self.inner);
        }
        emit_capsule(&self.inner, CapsuleState::Idle, 0.0, 0, None, None);
        schedule_capsule_idle(&self.inner, 0);
        Ok(())
    }

    pub(crate) fn revert_selection_voice_preview(
        &self,
        qa_session_id: SessionId,
    ) -> Result<(), String> {
        revert_selection_voice_preview_state(&self.inner, qa_session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_preview(qa_session_id: Option<SessionId>) -> PendingSelectionVoicePreview {
        PendingSelectionVoicePreview {
            qa_session_id,
            insertion_target: SelectionInsertionTarget::default(),
            source_text: "source".into(),
            preview_text: "preview".into(),
            previous_preview_text: None,
            summary: None,
            source_app: None,
        }
    }

    fn pending_intent_prompt() -> PendingSelectionVoiceIntentPrompt {
        PendingSelectionVoiceIntentPrompt {
            session_id: new_session_id(),
            selection: SelectionContext {
                text: "source".into(),
                source_app: None,
            },
            insertion_target: SelectionInsertionTarget::default(),
            instruction_polished: "instruction".into(),
        }
    }

    #[test]
    fn qa_edit_reuses_only_preview_owned_by_current_session() {
        let current = new_session_id();
        let mut matching = Some(pending_preview(Some(current)));
        assert!(use_existing_qa_preview(&mut matching, current));
        assert!(matching.is_some());

        let mut stale = Some(pending_preview(Some(new_session_id())));
        assert!(!use_existing_qa_preview(&mut stale, current));
        assert!(stale.is_none());

        let mut direct_replace = Some(pending_preview(None));
        assert!(!use_existing_qa_preview(&mut direct_replace, current));
        assert!(direct_replace.is_none());
    }

    #[test]
    fn qa_close_clears_only_qa_owned_preview_state() {
        let mut qa_preview = Some(pending_preview(Some(new_session_id())));
        clear_qa_bound_preview(&mut qa_preview);
        assert!(qa_preview.is_none());

        let mut direct_replace = Some(pending_preview(None));
        clear_qa_bound_preview(&mut direct_replace);
        assert!(direct_replace.is_some());
    }

    #[test]
    fn closing_qa_rotates_session_and_preserves_direct_replace_preview() {
        let coordinator = Coordinator::new();
        let closed_session_id = new_session_id();
        {
            let mut qa = coordinator.inner.qa_state.lock();
            qa.panel_visible = true;
            qa.session_id = closed_session_id;
        }
        *coordinator.inner.selection_voice_preview.lock() =
            Some(pending_preview(Some(closed_session_id)));

        close_qa_panel(&coordinator.inner);

        let qa = coordinator.inner.qa_state.lock();
        assert!(!qa.panel_visible);
        assert_ne!(qa.session_id, closed_session_id);
        drop(qa);
        assert!(coordinator.inner.selection_voice_preview.lock().is_none());

        *coordinator.inner.selection_voice_preview.lock() = Some(pending_preview(None));
        close_qa_panel(&coordinator.inner);
        assert_eq!(
            coordinator
                .inner
                .selection_voice_preview
                .lock()
                .as_ref()
                .and_then(|preview| preview.qa_session_id),
            None
        );
    }

    #[test]
    fn stale_preview_requests_do_not_clear_current_session_preview() {
        let coordinator = Coordinator::new();
        let current_session_id = new_session_id();
        let stale_session_id = new_session_id();
        {
            let mut qa = coordinator.inner.qa_state.lock();
            qa.panel_visible = true;
            qa.session_id = current_session_id;
        }
        *coordinator.inner.selection_voice_preview.lock() =
            Some(pending_preview(Some(current_session_id)));

        assert!(coordinator
            .selection_voice_preview(stale_session_id)
            .is_none());
        assert_eq!(
            coordinator
                .confirm_selection_voice_preview("replacement".into(), Some(stale_session_id))
                .unwrap_err(),
            "selectionVoicePreviewUnavailable"
        );
        assert_eq!(
            coordinator
                .revert_selection_voice_preview(stale_session_id)
                .unwrap_err(),
            "selectionVoicePreviewUnavailable"
        );
        assert_eq!(
            coordinator
                .inner
                .selection_voice_preview
                .lock()
                .as_ref()
                .and_then(|preview| preview.qa_session_id),
            Some(current_session_id)
        );
    }

    #[test]
    fn invalid_confirmed_intent_does_not_consume_pending_prompt() {
        let mut prompt = Some(pending_intent_prompt());
        assert_eq!(
            take_confirmed_selection_voice_intent_prompt(&mut prompt, "unknown").unwrap_err(),
            "selectionVoiceInvalidIntent:unknown"
        );
        assert!(prompt.is_some());

        let (_, intent) =
            take_confirmed_selection_voice_intent_prompt(&mut prompt, "question").unwrap();
        assert_eq!(intent, SelectionVoiceIntent::Question);
        assert!(prompt.is_none());
    }

    #[test]
    fn preview_apply_consumes_state_only_after_successful_insert() {
        let qa_session_id = new_session_id();
        let owner = Some(qa_session_id);

        let mut target_failure = Some(pending_preview(owner));
        let error = apply_selection_voice_preview_transaction(
            &mut target_failure,
            owner,
            |_| Err("selectionVoiceTargetUnavailable".into()),
        )
        .unwrap_err();
        assert_eq!(error, "selectionVoiceTargetUnavailable");
        assert!(target_failure.is_some());

        let mut insert_failure = Some(pending_preview(owner));
        let error = apply_selection_voice_preview_transaction(
            &mut insert_failure,
            owner,
            |_| Ok(InsertStatus::Failed),
        )
        .unwrap_err();
        assert_eq!(error, "selectionVoiceInsertFailed");
        assert!(insert_failure.is_some());

        let current_owner = Some(new_session_id());
        let mut current_preview = Some(pending_preview(current_owner));
        let error = apply_selection_voice_preview_transaction(
            &mut current_preview,
            owner,
            |_| panic!("stale session must not attempt insertion"),
        )
        .unwrap_err();
        assert_eq!(error, "selectionVoicePreviewUnavailable");
        assert_eq!(
            current_preview
                .as_ref()
                .and_then(|preview| preview.qa_session_id),
            current_owner
        );

        let mut success = Some(pending_preview(owner));
        let (_, status) = apply_selection_voice_preview_transaction(
            &mut success,
            owner,
            |_| Ok(InsertStatus::Inserted),
        )
        .unwrap();
        assert_eq!(status, InsertStatus::Inserted);
        assert!(success.is_none());
        assert_eq!(
            apply_selection_voice_preview_transaction(&mut success, owner, |_| {
                Ok(InsertStatus::Inserted)
            })
            .unwrap_err(),
            "selectionVoicePreviewUnavailable"
        );
    }

    #[test]
    fn infers_translation_target_after_cue_not_source_language() {
        let prefs = UserPreferences::default();
        let target = infer_selection_voice_translation_target(
            "把上面的英文翻译成中文。",
            &prefs,
        );
        assert_eq!(target, "简体中文");
        let target = infer_selection_voice_translation_target(
            "将上面的中文翻译成英文。",
            &prefs,
        );
        assert_eq!(target, "English");
    }

    #[test]
    fn selection_voice_session_active_checks_phase() {
        let session_id = new_session_id();
        let state = SelectionVoiceSessionState {
            phase: SelectionVoicePhase::Recording,
            session_id,
            ..SelectionVoiceSessionState::default()
        };
        assert!(selection_voice_recording_active(&state, session_id));
        assert!(!selection_voice_recording_active(&state, new_session_id()));
    }
}
