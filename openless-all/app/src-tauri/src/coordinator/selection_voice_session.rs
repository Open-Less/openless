//! Selection-voice edit session (issue #987 desktop MVP, Windows-first).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::{
    answer_qa_question_text, emit_capsule, open_qa_panel, polish_text, qa_session,
    CapsuleFeedback, Coordinator, Inner, QaPhase,
};
use crate::coordinator_state::{initial_session_id, new_session_id, SessionId};
use crate::edit_plan::{apply_edit_plan, parse_edit_plan_json, EditPlan};
use crate::selection::{SelectionContext, SelectionInsertionTarget};
use crate::selection_voice_intent::{
    parse_intent_classification_json, resolve_selection_voice_intent, SelectionVoiceIntent,
};
use crate::types::{
    CapsuleState, HistorySource, HotkeyMode, InsertStatus, PolishMode, SelectionVoiceIntentMode,
};

static SELECTION_VOICE_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionVoicePhase {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug, Clone)]
pub(super) struct SelectionVoiceSessionState {
    pub(super) phase: SelectionVoicePhase,
    pub(super) session_id: SessionId,
    pub(super) selection: Option<SelectionContext>,
    pub(super) insertion_target: SelectionInsertionTarget,
    pub(super) instruction_raw: Option<String>,
    pub(super) instruction_polished: Option<String>,
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

#[derive(Debug, Clone)]
pub(crate) struct PendingSelectionVoicePreview {
    insertion_target: SelectionInsertionTarget,
    source_text: String,
    preview_text: String,
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
        return;
    }
    if SELECTION_VOICE_BUSY.swap(true, Ordering::AcqRel) {
        return;
    }

    let mode = inner.prefs.get().hotkey.mode;
    let phase = inner.selection_voice_state.lock().phase;
    match (mode, phase) {
        (HotkeyMode::Toggle, SelectionVoicePhase::Idle) => {
            if let Err(error) = begin_selection_voice_session(inner).await {
                log::warn!("[selection-voice] begin failed: {error}");
                SELECTION_VOICE_BUSY.store(false, Ordering::Release);
            }
        }
        (HotkeyMode::Toggle, SelectionVoicePhase::Recording) => {
            let _ = end_selection_voice_session(inner).await;
            SELECTION_VOICE_BUSY.store(false, Ordering::Release);
        }
        (HotkeyMode::Hold | HotkeyMode::Auto, SelectionVoicePhase::Idle) => {
            if let Err(error) = begin_selection_voice_session(inner).await {
                log::warn!("[selection-voice] begin failed: {error}");
                SELECTION_VOICE_BUSY.store(false, Ordering::Release);
            }
        }
        _ => {
            SELECTION_VOICE_BUSY.store(false, Ordering::Release);
        }
    }
}

pub(super) async fn handle_selection_voice_released(inner: &Arc<Inner>) {
    if !inner.prefs.get().selection_voice_enabled {
        return;
    }
    let mode = inner.prefs.get().hotkey.mode;
    if !matches!(mode, HotkeyMode::Hold | HotkeyMode::Auto) {
        return;
    }
    if inner.selection_voice_state.lock().phase != SelectionVoicePhase::Recording {
        SELECTION_VOICE_BUSY.store(false, Ordering::Release);
        return;
    }
    let _ = end_selection_voice_session(inner).await;
    SELECTION_VOICE_BUSY.store(false, Ordering::Release);
}

async fn begin_selection_voice_session(inner: &Arc<Inner>) -> Result<(), String> {
    if !matches!(inner.state.lock().phase, crate::coordinator_state::SessionPhase::Idle) {
        return Err("dictationActive".into());
    }

    let insertion_target = crate::selection::capture_selection_insertion_target();
    let capture = crate::selection::capture_selection_with_status();
    let selection = capture.selection.ok_or_else(|| "selectionVoiceNoSelection".to_string())?;
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
    emit_capsule(inner, CapsuleState::Transcribing, 0.0, 0, None, None);

    let transcript = qa_session::finish_selection_voice_transcript(inner, session_id).await?;
    if transcript.trim().is_empty() {
        reset_selection_voice_session(inner);
        emit_capsule(inner, CapsuleState::Cancelled, 0.0, 0, Some("未识别到指令".into()), None);
        return Ok(());
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

    emit_capsule(inner, CapsuleState::Polishing, 0.0, 0, None, None);
    let instruction_polished = polish_selection_voice_instruction(inner, &instruction_raw).await?;
    {
        let mut state = inner.selection_voice_state.lock();
        state.instruction_raw = Some(instruction_raw);
        state.instruction_polished = Some(instruction_polished.clone());
    }

    let intent = resolve_intent_with_optional_llm(inner, &instruction_polished).await;
    match intent {
        SelectionVoiceIntent::Question => {
            run_selection_voice_question(inner, session_id, &selection, &instruction_polished)
                .await?;
        }
        SelectionVoiceIntent::Edit => {
            run_selection_voice_edit(
                inner,
                &selection,
                &insertion_target,
                &instruction_polished,
            )
            .await?;
        }
    }
    reset_selection_voice_session(inner);
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
        return classification.intent;
    }
    let system = crate::polish::prompts::selection_voice_intent_classification_prompt();
    let mut llm_call = None;
    let mut polish_ms = None;
    if let Ok(raw) = polish_text(
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
        if let Some(intent) = parse_intent_classification_json(&raw) {
            classification.intent = intent;
            classification.source = "auto_llm";
        }
    }
    log::info!(
        "[selection-voice] intent={:?} source={}",
        classification.intent,
        classification.source
    );
    classification.intent
}

async fn run_selection_voice_question(
    inner: &Arc<Inner>,
    session_id: SessionId,
    selection: &SelectionContext,
    instruction_polished: &str,
) -> Result<(), String> {
    open_qa_panel(inner);
    {
        let mut qa = inner.qa_state.lock();
        qa.selection = Some(selection.clone());
        qa.session_id = new_session_id();
        qa.phase = QaPhase::Processing;
        qa.messages.clear();
        qa.panel_visible = true;
    }
    let qa_session_id = inner.qa_state.lock().session_id;
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
    let plan = generate_edit_plan(inner, &selection.text, instruction_polished).await?;
    let preview = apply_edit_plan(&selection.text, &plan).map_err(|error| error.to_string())?;
    *inner.selection_voice_preview.lock() = Some(PendingSelectionVoicePreview {
        insertion_target: insertion_target.clone(),
        source_text: selection.text.clone(),
        preview_text: preview,
        summary: plan.summary.clone(),
        source_app: selection.source_app.clone(),
    });
    if let Some(app) = inner.app.lock().clone() {
        crate::show_selection_voice_preview(&app);
    }
    emit_capsule(
        inner,
        CapsuleState::Done,
        0.0,
        0,
        Some("已打开预览，等待确认".into()),
        None,
    );
    Ok(())
}

async fn generate_edit_plan(
    inner: &Arc<Inner>,
    draft: &str,
    instruction_polished: &str,
) -> Result<EditPlan, String> {
    let prefs = inner.prefs.get();
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
    parse_edit_plan_json(&raw)
}

impl Coordinator {
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_voice_session_active_checks_phase() {
        let state = SelectionVoiceSessionState {
            phase: SelectionVoicePhase::Recording,
            session_id: 7,
            ..SelectionVoiceSessionState::default()
        };
        assert!(selection_voice_recording_active(&state, 7));
        assert!(!selection_voice_recording_active(&state, 8));
    }
}
