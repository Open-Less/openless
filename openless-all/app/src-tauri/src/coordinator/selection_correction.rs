//! 最近一次听写落字范围内的「划词纠错」工作流。
//!
//! 与 Selection Polish 的语义不同：LiteralReplace 从不调用 LLM；Review 把用户的
//! 批注当作编辑指令，但结果只能进入确认预览，不能自动覆盖原选区。

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{
    dictation, emit_capsule, enabled_phrases, pipeline_multimodal_enabled, polish_text,
    schedule_capsule_idle, Coordinator, Inner, CAPSULE_AUTO_HIDE_DELAY_MS,
};
use crate::{
    host_document::{EditPair, LearnedRule, SelectionObservation},
    selection::SelectionInsertionTarget,
    types::{CapsuleState, DictationSession, InsertStatus, PolishMode},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionCorrectionAction {
    LiteralReplace,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionCorrectionBubbleState {
    Actions,
    Recording,
    Processing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCorrectionBubblePayload {
    pub selected_text: String,
    pub state: SelectionCorrectionBubbleState,
    pub action: Option<SelectionCorrectionAction>,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
struct SelectionCorrectionCandidate {
    selected_text: String,
    document_text: String,
    selection_start_utf16: usize,
    selection_length_utf16: usize,
    whole_field_selected: bool,
    source_app: Option<String>,
}

#[derive(Debug, Clone)]
struct ActiveSelectionCorrection {
    action: SelectionCorrectionAction,
    candidate: SelectionCorrectionCandidate,
    insertion_target: SelectionInsertionTarget,
    session_id: Option<crate::coordinator_state::SessionId>,
    started_at: std::time::Instant,
}

#[derive(Default)]
pub(super) struct SelectionCorrectionRuntime {
    candidate: Option<SelectionCorrectionCandidate>,
    active: Option<ActiveSelectionCorrection>,
    payload: Option<SelectionCorrectionBubblePayload>,
}

#[derive(Debug, Clone)]
pub(super) struct SelectionCorrectionHistoryMetadata {
    pub asr_provider: Option<String>,
    pub asr_model: Option<String>,
    pub asr_ms: Option<u64>,
    pub asr_dictionary_delivery: Option<crate::types::DictionaryDeliveryReport>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub pipeline_mode: Option<String>,
    pub polish_ms: Option<u64>,
    pub has_audio_recording: Option<bool>,
}

fn payload_for_candidate(
    candidate: &SelectionCorrectionCandidate,
) -> SelectionCorrectionBubblePayload {
    SelectionCorrectionBubblePayload {
        selected_text: candidate.selected_text.clone(),
        state: SelectionCorrectionBubbleState::Actions,
        action: None,
        message: None,
    }
}

pub(super) fn handle_selection_observation(
    inner: &Arc<Inner>,
    observation: Option<SelectionObservation>,
    source_app: Option<String>,
) {
    let app = inner.app.lock().clone();
    let mut runtime = inner.selection_correction.lock();
    if runtime.active.is_some() {
        return;
    }
    let Some(observation) = observation else {
        runtime.candidate = None;
        runtime.payload = None;
        drop(runtime);
        if let Some(app) = app {
            crate::hide_selection_correction_bubble(&app);
        }
        return;
    };
    // 选区两端带空白时直接替换会把原排版一并吞掉；让用户重新精确选词更安全。
    if observation.selected_text.trim() != observation.selected_text {
        runtime.candidate = None;
        runtime.payload = None;
        drop(runtime);
        if let Some(app) = app {
            crate::hide_selection_correction_bubble(&app);
        }
        return;
    }
    let candidate = SelectionCorrectionCandidate {
        selected_text: observation.selected_text,
        document_text: observation.document_text,
        selection_start_utf16: observation.selection_start_utf16,
        selection_length_utf16: observation.selection_length_utf16,
        whole_field_selected: observation.whole_field_selected,
        source_app,
    };
    let payload = payload_for_candidate(&candidate);
    runtime.candidate = Some(candidate);
    runtime.payload = Some(payload.clone());
    drop(runtime);
    if let Some(app) = app {
        crate::show_selection_correction_bubble(&app, &payload);
    }
}

fn set_bubble_state(
    inner: &Arc<Inner>,
    state: SelectionCorrectionBubbleState,
    action: Option<SelectionCorrectionAction>,
    message: Option<String>,
) {
    let app = inner.app.lock().clone();
    let payload = {
        let mut runtime = inner.selection_correction.lock();
        let selected_text = runtime
            .active
            .as_ref()
            .map(|active| active.candidate.selected_text.clone())
            .or_else(|| {
                runtime
                    .candidate
                    .as_ref()
                    .map(|candidate| candidate.selected_text.clone())
            })
            .or_else(|| {
                runtime
                    .payload
                    .as_ref()
                    .map(|payload| payload.selected_text.clone())
            })
            .unwrap_or_default();
        let payload = SelectionCorrectionBubblePayload {
            selected_text,
            state,
            action,
            message,
        };
        runtime.payload = Some(payload.clone());
        payload
    };
    if let Some(app) = app {
        crate::show_selection_correction_bubble(&app, &payload);
    }
}

fn clear_runtime(inner: &Arc<Inner>, hide: bool) {
    let app = inner.app.lock().clone();
    {
        let mut runtime = inner.selection_correction.lock();
        runtime.candidate = None;
        runtime.active = None;
        runtime.payload = None;
    }
    if hide {
        if let Some(app) = app {
            crate::hide_selection_correction_bubble(&app);
        }
    }
}

pub(super) fn bind_active_session(
    inner: &Arc<Inner>,
    session_id: crate::coordinator_state::SessionId,
) -> Result<(), String> {
    let mut runtime = inner.selection_correction.lock();
    let active = runtime
        .active
        .as_mut()
        .ok_or_else(|| "selectionCorrectionUnavailable".to_string())?;
    active.session_id = Some(session_id);
    Ok(())
}

pub(super) fn is_active_session(
    inner: &Arc<Inner>,
    session_id: crate::coordinator_state::SessionId,
) -> bool {
    inner
        .selection_correction
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.session_id == Some(session_id))
}

pub(super) fn active_action(
    inner: &Arc<Inner>,
    session_id: crate::coordinator_state::SessionId,
) -> Option<SelectionCorrectionAction> {
    inner
        .selection_correction
        .lock()
        .active
        .as_ref()
        .filter(|active| active.session_id == Some(session_id))
        .map(|active| active.action)
}

pub(super) fn cancel_active(inner: &Arc<Inner>) {
    if inner.selection_correction.lock().active.is_some() {
        clear_runtime(inner, true);
    }
}

pub(super) fn dismiss_candidate(inner: &Arc<Inner>) {
    if inner.selection_correction.lock().active.is_none() {
        clear_runtime(inner, true);
    }
}

fn literal_replacement_vocab_rule(
    source: &str,
    replacement: &str,
    whole_field_selected: bool,
) -> Option<LearnedRule> {
    if whole_field_selected || source == replacement {
        return None;
    }
    let edit = EditPair {
        source: source.to_string(),
        target: replacement.to_string(),
        before: String::new(),
        after: String::new(),
    };
    crate::host_document::is_vocab_worthy(&edit).then(|| LearnedRule {
        pattern: source.trim().to_string(),
        replacement: replacement.trim().to_string(),
    })
}

fn review_prompt(annotation: &str) -> String {
    let annotation = crate::polish::prompts::sanitize_for_xml_envelope(annotation, "review_note");
    format!(
        "# 选区批注修改\n\
         你只负责修改用户当前选中的文本。`<review_note>` 是用户本次明确给出的修改要求，\
         按它调整选中文字；文档上下文只用于理解指代、语气与术语。\n\
         不要改写选区之外的内容，不要解释理由，不要加引号、前缀或 Markdown，\
         最终只输出可直接替换选区的文本。\n\n\
         <review_note>\n{annotation}\n</review_note>"
    )
}

fn selected_context(candidate: &SelectionCorrectionCandidate) -> String {
    let start = crate::host_document::utf16_offset_to_char_offset(
        &candidate.document_text,
        candidate.selection_start_utf16,
    );
    let end = crate::host_document::utf16_offset_to_char_offset(
        &candidate.document_text,
        candidate
            .selection_start_utf16
            .saturating_add(candidate.selection_length_utf16),
    );
    let before: String = candidate.document_text.chars().take(start).collect();
    let after: String = candidate.document_text.chars().skip(end).collect();
    crate::polish::prompts::cursor_context_input(&before, &after)
}

fn finish_session_ui(inner: &Arc<Inner>, state: CapsuleState, message: &str) {
    emit_capsule(inner, state, 0.0, 0, Some(message.to_string()), None);
    schedule_capsule_idle(inner, CAPSULE_AUTO_HIDE_DELAY_MS);
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_selection_correction_bubble(&app);
    }
}

fn set_session_idle(inner: &Arc<Inner>) {
    inner.state.lock().phase = crate::coordinator_state::SessionPhase::Idle;
    let now = std::time::Instant::now();
    *inner.session_cooldown_until.lock() =
        Some(now + std::time::Duration::from_millis(super::POST_SESSION_COOLDOWN_MS));
}

pub(super) async fn finish_transcript(
    inner: &Arc<Inner>,
    session_id: crate::coordinator_state::SessionId,
    transcript: String,
    duration_ms: u64,
    metadata: SelectionCorrectionHistoryMetadata,
) -> Result<(), String> {
    let active = {
        let mut runtime = inner.selection_correction.lock();
        let matches = runtime
            .active
            .as_ref()
            .is_some_and(|active| active.session_id == Some(session_id));
        if !matches {
            return Err("selectionCorrectionSessionChanged".into());
        }
        runtime.candidate = None;
        runtime.active.take().expect("active session checked")
    };
    let replacement_or_note = transcript.trim().to_string();
    if replacement_or_note.is_empty() {
        clear_runtime(inner, true);
        set_session_idle(inner);
        finish_session_ui(inner, CapsuleState::Error, "没有识别到语音");
        return Err("selectionCorrectionEmptyTranscript".into());
    }
    set_bubble_state(
        inner,
        SelectionCorrectionBubbleState::Processing,
        Some(active.action),
        None,
    );

    match active.action {
        SelectionCorrectionAction::LiteralReplace => {
            let validation = crate::selection::validate_selection_insertion_target(
                &active.insertion_target,
                &active.candidate.selected_text,
            );
            if let Some(code) = validation.error_code() {
                clear_runtime(inner, true);
                set_session_idle(inner);
                finish_session_ui(inner, CapsuleState::Cancelled, "选区已变化，未替换");
                return Err(code.to_string());
            }
            {
                let mut state = inner.state.lock();
                if state.cancelled {
                    drop(state);
                    clear_runtime(inner, true);
                    set_session_idle(inner);
                    finish_session_ui(inner, CapsuleState::Cancelled, "已取消");
                    return Ok(());
                }
                state.phase = crate::coordinator_state::SessionPhase::Inserting;
            }
            let prefs = inner.prefs.get();
            let status = inner.inserter.insert(
                &replacement_or_note,
                prefs.restore_clipboard_after_paste,
                prefs.paste_shortcut,
            );
            set_session_idle(inner);

            let front = crate::types::split_front_app_opt(active.candidate.source_app.as_deref());
            let session = DictationSession {
                id: session_id.to_string(),
                created_at: Utc::now().to_rfc3339(),
                source: crate::types::HistorySource::SelectionCorrection,
                raw_transcript: replacement_or_note.clone(),
                asr_transcript: None,
                final_text: replacement_or_note.clone(),
                mode: PolishMode::Raw,
                style_pack_id: None,
                translation_active: false,
                polish_source: None,
                app_bundle_id: front.bundle_id,
                app_name: front.name,
                insert_status: status,
                error_code: (status == InsertStatus::Failed)
                    .then_some("selectionCorrectionInsertFailed".into()),
                duration_ms: Some(duration_ms),
                dictionary_entry_count: Some(0),
                has_audio_recording: metadata.has_audio_recording,
                asr_provider: metadata.asr_provider,
                asr_model: metadata.asr_model,
                llm_provider: metadata.llm_provider,
                llm_model: metadata.llm_model,
                pipeline_mode: metadata.pipeline_mode,
                asr_ms: metadata.asr_ms,
                polish_ms: metadata.polish_ms,
                asr_dictionary_delivery: metadata.asr_dictionary_delivery,
                llm_dictionary_sent_count: None,
                llm_dictionary_delivery: None,
            };
            if let Err(error) = inner.history.append_with_retention(
                session,
                prefs.history_retention_days,
                prefs.history_max_entries,
            ) {
                log::error!("[selection-correction] history append failed: {error}");
            }
            if status == InsertStatus::Failed {
                clear_runtime(inner, true);
                finish_session_ui(inner, CapsuleState::Error, "替换失败，请重试");
                return Err("selectionCorrectionInsertFailed".into());
            }
            if let Some(rule) = literal_replacement_vocab_rule(
                &active.candidate.selected_text,
                &replacement_or_note,
                active.candidate.whole_field_selected,
            ) {
                let already_enabled = enabled_phrases(inner)
                    .iter()
                    .any(|phrase| phrase == &rule.replacement);
                if !already_enabled {
                    dictation::queue_explicit_correction_suggestion(
                        inner,
                        &rule,
                        active.candidate.source_app.clone(),
                    );
                }
            }
            clear_runtime(inner, true);
            finish_session_ui(inner, CapsuleState::Done, "已替换");
            Ok(())
        }
        SelectionCorrectionAction::Review => {
            emit_capsule(inner, CapsuleState::Polishing, 0.0, 0, None, None);
            let prefs = inner.prefs.get();
            let hotwords = enabled_phrases(inner);
            let prompt = review_prompt(&replacement_or_note);
            let cursor_context = selected_context(&active.candidate);
            let mut llm_call = None;
            let mut polish_ms = None;
            let result = polish_text(
                &active.candidate.selected_text,
                PolishMode::Light,
                &hotwords,
                &prompt,
                &prefs.working_languages,
                prefs.chinese_script_preference,
                prefs.output_language_preference,
                prefs.llm_thinking_enabled,
                active.candidate.source_app.as_deref(),
                Some(&cursor_context),
                &[],
                &mut llm_call,
                &mut polish_ms,
                pipeline_multimodal_enabled(&prefs),
            )
            .await
            .map_err(|error| error.to_string());
            let proposed = match result {
                Ok(text) if !text.trim().is_empty() => text.trim().to_string(),
                Ok(_) => {
                    clear_runtime(inner, true);
                    set_session_idle(inner);
                    finish_session_ui(inner, CapsuleState::Error, "未生成可替换文本");
                    return Err("selectionCorrectionEmptyOutput".into());
                }
                Err(error) => {
                    log::warn!("[selection-correction] review provider failed: {error}");
                    clear_runtime(inner, true);
                    set_session_idle(inner);
                    finish_session_ui(inner, CapsuleState::Error, "生成修改建议失败");
                    return Err(error);
                }
            };
            let (llm_provider, llm_model) = llm_call
                .map(|label| (Some(label.provider), Some(label.model)))
                .unwrap_or((None, None));
            *inner.selection_polish_preview.lock() =
                Some(super::selection_polish::PendingSelectionPolishPreview {
                    insertion_target: active.insertion_target,
                    source_text: active.candidate.selected_text,
                    polished_text: proposed,
                    source_app: active.candidate.source_app,
                    mode: PolishMode::Light,
                    style_pack_id: String::new(),
                    llm_provider,
                    llm_model,
                    llm_dictionary_sent_count: hotwords.len().min(u32::MAX as usize) as u32,
                    polish_ms,
                    started_at: active.started_at,
                    expected_document_text: Some(active.candidate.document_text),
                    review_annotation: Some(replacement_or_note.clone()),
                    history_raw_transcript: Some(replacement_or_note),
                    history_source: crate::types::HistorySource::SelectionCorrection,
                    asr_provider: metadata.asr_provider,
                    asr_model: metadata.asr_model,
                    asr_ms: metadata.asr_ms,
                    asr_dictionary_delivery: metadata.asr_dictionary_delivery,
                    has_audio_recording: metadata.has_audio_recording,
                    pipeline_mode: metadata.pipeline_mode,
                });
            set_session_idle(inner);
            clear_runtime(inner, true);
            if let Some(app) = inner.app.lock().clone() {
                crate::show_selection_polish_preview(&app);
            }
            finish_session_ui(inner, CapsuleState::Done, "已生成修改建议");
            Ok(())
        }
    }
}

impl Coordinator {
    pub async fn start_selection_correction(
        &self,
        action: SelectionCorrectionAction,
    ) -> Result<(), String> {
        if self.inner.state.lock().phase != crate::coordinator_state::SessionPhase::Idle {
            return Err("selectionCorrectionBusy".into());
        }
        let candidate = self
            .inner
            .selection_correction
            .lock()
            .candidate
            .clone()
            .ok_or_else(|| "selectionCorrectionUnavailable".to_string())?;
        let insertion_target = crate::selection::capture_selection_insertion_target();
        if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
            return Err("selectionPolishTargetUnavailable".into());
        }
        let validation = crate::selection::validate_selection_insertion_target(
            &insertion_target,
            &candidate.selected_text,
        );
        if let Some(code) = validation.error_code() {
            return Err(code.to_string());
        }
        {
            let mut runtime = self.inner.selection_correction.lock();
            runtime.active = Some(ActiveSelectionCorrection {
                action,
                candidate,
                insertion_target,
                session_id: None,
                started_at: std::time::Instant::now(),
            });
        }
        super::disarm_edit_watch(&self.inner);
        set_bubble_state(
            &self.inner,
            SelectionCorrectionBubbleState::Recording,
            Some(action),
            None,
        );
        if let Err(error) = dictation::begin_selection_correction_session(&self.inner).await {
            // 启动失败时仍保留原选词，错误气泡才有足够上下文；关闭气泡会正常清理。
            let mut runtime = self.inner.selection_correction.lock();
            if let Some(active) = runtime.active.take() {
                runtime.candidate = Some(active.candidate);
            }
            drop(runtime);
            set_bubble_state(
                &self.inner,
                SelectionCorrectionBubbleState::Error,
                Some(action),
                Some(error.clone()),
            );
            return Err(error);
        }
        let began = self
            .inner
            .selection_correction
            .lock()
            .active
            .as_ref()
            .is_some_and(|active| active.session_id.is_some());
        if !began {
            clear_runtime(&self.inner, true);
            return Err("selectionCorrectionBusy".into());
        }
        Ok(())
    }

    pub async fn stop_selection_correction(&self) -> Result<(), String> {
        let action = self
            .inner
            .selection_correction
            .lock()
            .active
            .as_ref()
            .map(|active| active.action)
            .ok_or_else(|| "selectionCorrectionUnavailable".to_string())?;
        set_bubble_state(
            &self.inner,
            SelectionCorrectionBubbleState::Processing,
            Some(action),
            None,
        );
        if self.inner.state.lock().phase == crate::coordinator_state::SessionPhase::Starting {
            dictation::request_stop_during_starting(&self.inner, "selection correction stop");
            return Ok(());
        }
        dictation::end_session(&self.inner).await
    }

    pub fn cancel_selection_correction(&self) {
        dictation::cancel_session(&self.inner);
        clear_runtime(&self.inner, true);
    }

    pub fn dismiss_selection_correction(&self) {
        if self.inner.selection_correction.lock().active.is_none() {
            clear_runtime(&self.inner, true);
        }
    }

    pub fn selection_correction_payload(&self) -> Option<SelectionCorrectionBubblePayload> {
        self.inner.selection_correction.lock().payload.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_replace_only_suggests_short_word_like_replacements() {
        assert!(literal_replacement_vocab_rule("扣德克斯", "Codex", false).is_some());
        assert!(literal_replacement_vocab_rule(
            "整段话",
            "这是一整段完全重写且超过十二个字符的话",
            false
        )
        .is_none());
        assert!(literal_replacement_vocab_rule("全部", "新词", true).is_none());
        assert!(literal_replacement_vocab_rule("Codex", "Codex", false).is_none());
    }

    #[test]
    fn review_prompt_requires_replacement_only() {
        let prompt = review_prompt("改得更简洁");
        assert!(prompt.contains("只输出可直接替换选区的文本"));
        assert!(prompt.contains("<review_note>"));
    }
}
