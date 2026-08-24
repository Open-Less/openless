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
    answer_qa_question_text, close_qa_panel, emit_capsule, open_qa_panel, polish_text,
    qa_event_target, qa_session, schedule_capsule_idle, translate_text, CapsuleFeedback,
    Coordinator, Inner, QaPhase,
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

pub(super) fn selection_voice_busy_for_debug() -> bool {
    SELECTION_VOICE_BUSY.load(Ordering::SeqCst)
}

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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H7",
        "selection_voice_session.rs:end",
        "selection voice end workflow failed",
        serde_json::json!({
            "error": error,
            "previewMode": preview_mode,
            "qaVisible": qa_visible,
        }),
    );
    // #endregion
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
    insertion_target: SelectionInsertionTarget,
    source_text: String,
    preview_text: String,
    previous_preview_text: Option<String>,
    summary: Option<String>,
    source_app: Option<String>,
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
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H2",
            "selection_voice_session.rs:pressed",
            "ignored: selection voice disabled",
            serde_json::json!({}),
        );
        // #endregion
        return;
    }

    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.selection_voice_state.lock().phase;
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H3",
        "selection_voice_session.rs:pressed",
        "handling press",
        serde_json::json!({ "hotkeyMode": format!("{:?}", mode), "phase": format!("{:?}", phase) }),
    );
    // #endregion

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
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H2",
            "selection_voice_session.rs:pressed",
            "ignored: selection voice busy",
            serde_json::json!({}),
        );
        // #endregion
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
            // #region agent log
            crate::agent_debug::agent_debug_log(
                "H3",
                "selection_voice_session.rs:pressed",
                "ignored: unexpected phase/mode",
                serde_json::json!({ "hotkeyMode": format!("{:?}", mode), "phase": format!("{:?}", phase) }),
            );
            // #endregion
            SELECTION_VOICE_BUSY.store(false, Ordering::Release);
            return;
        }
    };

    if let Err(error) = begin_result {
        log::warn!("[selection-voice] begin failed: {error}");
        emit_selection_voice_begin_error(inner, &error);
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H3",
            "selection_voice_session.rs:pressed",
            "begin failed",
            serde_json::json!({ "error": error }),
        );
        // #endregion
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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H5",
        "selection_voice_session.rs:released",
        "handling release",
        serde_json::json!({ "hotkeyMode": format!("{:?}", mode) }),
    );
    // #endregion
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
    let selection_text_len = selection.text.len();
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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H3",
        "selection_voice_session.rs:begin",
        "selection voice session started",
        serde_json::json!({ "sessionId": session_id.to_string(), "selectionLen": selection_text_len }),
    );
    // #endregion
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
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H6",
            "selection_voice_session.rs:end",
            "transcript ready",
            serde_json::json!({ "chars": transcript.chars().count() }),
        );
        // #endregion
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
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H6",
            "selection_voice_session.rs:end",
            "intent resolved",
            serde_json::json!({ "intent": format!("{:?}", intent) }),
        );
        // #endregion
        continue_selection_voice_with_intent(
            inner,
            session_id,
            &selection,
            &insertion_target,
            &instruction_polished,
            intent,
        )
        .await?;
        // #region agent log
        crate::agent_debug::agent_debug_log(
            "H6",
            "selection_voice_session.rs:end",
            "selection voice workflow completed",
            serde_json::json!({}),
        );
        // #endregion
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
    let prefs = inner.prefs.get();
    let mut llm_call = None;
    let mut polish_ms = None;
    let prompt = crate::polish::prompts::selection_voice_instruction_polish_prompt();
    polish_text(
        instruction_raw,
        PolishMode::Light,
        &[],
        &prompt,
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
    .map_err(|error| error.to_string())
}

async fn resolve_intent_with_optional_llm(
    inner: &Arc<Inner>,
    instruction_polished: &str,
) -> SelectionVoiceIntent {
    let prefs = inner.prefs.get();
    let mut classification = resolve_selection_voice_intent(&prefs, instruction_polished);
    if prefs.selection_voice_intent_mode != SelectionVoiceIntentMode::Auto {
        log::info!(
            "[selection-voice] intent={:?} source={}",
            classification.intent,
            classification.source
        );
        return classification.intent;
    }
    // Clear edit cues already win — skip flaky LLM classification.
    if classification.intent == SelectionVoiceIntent::Edit {
        log::info!(
            "[selection-voice] intent={:?} source={}",
            classification.intent,
            classification.source
        );
        return classification.intent;
    }
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
            } else if crate::selection_voice_intent::looks_like_edit_instruction(instruction_polished)
            {
                log::warn!(
                    "[selection-voice] intent LLM unparsable; heuristic edit fallback preview={}",
                    raw.chars().take(120).collect::<String>()
                );
                classification.intent = SelectionVoiceIntent::Edit;
                classification.source = "auto_heuristic_fallback";
            } else {
                log::warn!(
                    "[selection-voice] intent LLM unparsable; keep {:?} preview={}",
                    classification.intent,
                    raw.chars().take(120).collect::<String>()
                );
            }
        }
        Err(error) => {
            log::warn!("[selection-voice] intent LLM failed: {error}");
            if crate::selection_voice_intent::looks_like_edit_instruction(instruction_polished) {
                classification.intent = SelectionVoiceIntent::Edit;
                classification.source = "auto_heuristic_fallback";
            }
        }
    }
    log::info!(
        "[selection-voice] intent={:?} source={}",
        classification.intent,
        classification.source
    );
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H1",
        "selection_voice_session.rs:intent",
        "intent resolved",
        serde_json::json!({
            "intent": format!("{:?}", classification.intent),
            "source": classification.source,
        }),
    );
    // #endregion
    classification.intent
}

async fn run_selection_voice_question(
    inner: &Arc<Inner>,
    _session_id: SessionId,
    selection: &SelectionContext,
    instruction_polished: &str,
) -> Result<(), String> {
    let qa_session_id = {
        let mut qa = inner.qa_state.lock();
        qa.selection = Some(selection.clone());
        if !qa.panel_visible {
            open_qa_panel(inner);
            let mut qa = inner.qa_state.lock();
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
        let mut qa = inner.qa_state.lock();
        qa.selection = Some(selection.clone());
        if !qa.panel_visible {
            open_qa_panel(inner);
            let mut qa = inner.qa_state.lock();
            qa.session_id = new_session_id();
            qa.messages.clear();
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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H6",
        "selection_voice_session.rs:edit",
        "edit preview ready",
        serde_json::json!({
            "sourceChars": selection.text.chars().count(),
            "previewChars": preview.chars().count(),
            "sameAsSource": preview == selection.text,
            "summary": plan.summary,
        }),
    );
    // #endregion
    if preview == selection.text {
        log::warn!(
            "[selection-voice] edit result identical to source (chars={})",
            preview.chars().count()
        );
    }

    let direct = !preview_mode;

    *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
        insertion_target: insertion_target.clone(),
        source_text: selection.text.clone(),
        preview_text: preview.clone(),
        previous_preview_text: None,
        summary: plan.summary.clone(),
        source_app: selection.source_app.clone(),
    });

    if preview_mode {
        let user_content = format!("# 编辑指令\n{instruction_polished}");
        let summary_line = plan
            .summary
            .as_deref()
            .map(|s| format!("（{s}）\n\n"))
            .unwrap_or_default();
        let assistant_content = format!("{summary_line}{preview}");

        let mut qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id {
            return Ok(());
        }
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
                }),
            );
        }
    }

    if direct {
        let coord = Coordinator {
            inner: Arc::clone(inner),
        };
        coord.confirm_selection_voice_preview(preview)?;
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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H6",
        "selection_voice_session.rs:edit_plan",
        "edit plan llm response received",
        serde_json::json!({ "rawChars": raw.chars().count() }),
    );
    // #endregion
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
            // #region agent log
            crate::agent_debug::agent_debug_log(
                "H6",
                "selection_voice_session.rs:edit_plan",
                "edit plan parsed",
                serde_json::json!({ "operations": plan.operations.len() }),
            );
            // #endregion
            Ok(plan)
        }
        Err(error) => {
            log::warn!(
                "[selection-voice] edit plan parse failed: {error}; preview={}",
                raw.chars().take(240).collect::<String>()
            );
            // #region agent log
            crate::agent_debug::agent_debug_log(
                "H7",
                "selection_voice_session.rs:edit_plan",
                "edit plan parse failed",
                serde_json::json!({
                    "error": error,
                    "preview": raw.chars().take(240).collect::<String>(),
                }),
            );
            // #endregion
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
    if lower.contains("英文")
        || lower.contains("英语")
        || lower.contains("english")
    {
        return "English".into();
    }
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
    // #region agent log
    crate::agent_debug::agent_debug_log(
        "H6",
        "selection_voice_session.rs:translate",
        "translation edit result",
        serde_json::json!({
            "target": target_language,
            "draftChars": draft.chars().count(),
            "rawChars": translated_raw.chars().count(),
            "cleanChars": translated.chars().count(),
            "sameAsDraft": translated == draft,
            "preview": translated.chars().take(80).collect::<String>(),
        }),
    );
    // #endregion
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

    let pending = inner
        .selection_voice_preview
        .lock()
        .clone()
        .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())?;

    {
        let mut qa = inner.qa_state.lock();
        if qa.session_id != qa_session_id || qa.phase != QaPhase::Idle || !qa.panel_visible {
            return Err("QA is busy".to_string());
        }
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
    }

    let plan =
        generate_edit_plan(inner, &pending.preview_text, &instruction).await?;
    let new_preview =
        apply_edit_plan(&pending.preview_text, &plan).map_err(|error| error.to_string())?;

    *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
        insertion_target: pending.insertion_target,
        source_text: pending.source_text,
        preview_text: new_preview.clone(),
        previous_preview_text: Some(pending.preview_text),
        summary: plan.summary.clone(),
        source_app: pending.source_app,
    });

    let summary_line = plan
        .summary
        .as_deref()
        .map(|s| format!("（{s}）\n\n"))
        .unwrap_or_default();
    let assistant_content = format!("{summary_line}{new_preview}");

    let mut qa = inner.qa_state.lock();
    if qa.session_id != qa_session_id {
        return Ok(());
    }
    qa.messages.push(crate::types::QaChatMessage {
        role: "assistant".into(),
        content: assistant_content,
        selection_text: None,
    });
    qa.phase = QaPhase::Idle;
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
            }),
        );
    }
    Ok(())
}

pub(super) fn revert_selection_voice_preview_state(
    inner: &Arc<Inner>,
    qa_session_id: SessionId,
) -> Result<(), String> {
    let mut preview_slot = inner.selection_voice_preview.lock();
    let Some(pending) = preview_slot.as_mut() else {
        return Err("selectionVoicePreviewUnavailable".into());
    };
    let previous = pending
        .previous_preview_text
        .clone()
        .ok_or_else(|| "selectionVoiceRevertUnavailable".to_string())?;
    pending.preview_text = previous;
    pending.previous_preview_text = None;
    pending.summary = None;

    let mut qa = inner.qa_state.lock();
    if qa.session_id != qa_session_id || !qa.panel_visible {
        return Ok(());
    }
    if let Some(last) = qa.messages.last_mut() {
        if last.role == "assistant" {
            last.content = pending.preview_text.clone();
        }
    }
    qa.phase = QaPhase::Idle;
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

    pub(crate) fn confirm_selection_voice_intent_prompt(
        &self,
        intent: String,
    ) -> Result<(), String> {
        let prompt = self
            .inner
            .selection_voice_intent_prompt
            .lock()
            .take()
            .ok_or_else(|| "selectionVoiceIntentPromptUnavailable".to_string())?;
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_voice_intent_prompt(&app);
        }
        let resolved = match intent.as_str() {
            "question" => SelectionVoiceIntent::Question,
            "edit" => SelectionVoiceIntent::Edit,
            other => return Err(format!("selectionVoiceInvalidIntent:{other}")),
        };
        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::block_on(async move {
            continue_selection_voice_with_intent(
                &inner,
                prompt.session_id,
                &prompt.selection,
                &prompt.insertion_target,
                &prompt.instruction_polished,
                resolved,
            )
            .await?;
            reset_selection_voice_session(&inner);
            Ok(())
        })
    }

    pub(crate) fn selection_voice_preview(&self) -> Option<SelectionVoicePreviewPayload> {
        self.inner.selection_voice_preview.lock().as_ref().map(|preview| {
            SelectionVoicePreviewPayload {
                text: preview.preview_text.clone(),
                source_text: preview.source_text.clone(),
                summary: preview.summary.clone(),
            }
        })
    }

    pub(crate) fn cancel_selection_voice_preview(&self) {
        self.inner.selection_voice_preview.lock().take();
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_voice_preview(&app);
        }
    }

    pub(crate) fn confirm_selection_voice_preview(&self, text: String) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("selectionVoiceEmptyOutput".into());
        }
        let preview = self
            .inner
            .selection_voice_preview
            .lock()
            .take()
            .ok_or_else(|| "selectionVoicePreviewUnavailable".to_string())?;

        if !crate::selection::reactivate_selection_insertion_target(&preview.insertion_target) {
            return Err("selectionVoiceTargetUnavailable".into());
        }
        let validation = crate::selection::validate_selection_insertion_target(
            &preview.insertion_target,
            &preview.source_text,
        );
        if let Some(code) = validation.error_code() {
            return Err(code.to_string());
        }

        let prefs = self.inner.prefs.get();
        let status = self.inner.inserter.insert(
            &text,
            prefs.restore_clipboard_after_paste,
            prefs.paste_shortcut,
        );
        if status == InsertStatus::Failed {
            return Err("selectionVoiceInsertFailed".into());
        }

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
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_voice_preview(&app);
        }
        close_qa_panel(&self.inner);
        emit_capsule(&self.inner, CapsuleState::Idle, 0.0, 0, None, None);
        schedule_capsule_idle(&self.inner, 0);
        Ok(())
    }

    pub(crate) fn revert_selection_voice_preview(&self) -> Result<(), String> {
        let qa_session_id = self.inner.qa_state.lock().session_id;
        revert_selection_voice_preview_state(&self.inner, qa_session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
