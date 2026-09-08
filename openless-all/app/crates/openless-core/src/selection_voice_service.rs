use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use futures_util::future::BoxFuture;

use crate::config::Clock;
use crate::correction::apply_correction_rules;
use crate::credentials::{CredentialStore, ProviderSlot};
use crate::dictation_context::{
    DictationContext, DictationProviderInvocations, DictationStartOptions, ProviderInvocation,
};
use crate::domains::{
    QaApi, SelectionCapture, SelectionVoiceApi, SelectionVoiceApplyOutcome,
    SelectionVoiceApplyTicket, SelectionVoiceDisposition, SelectionVoiceEditAction,
    SelectionVoiceEditPreviewResult, SelectionVoiceEditRequest, SelectionVoiceHotkeyAction,
    SelectionVoiceHotkeyEdge, SelectionVoiceInstructionRequest, SelectionVoiceIntentPrompt,
    SelectionVoicePhase, SelectionVoicePreview, SelectionVoicePreviewUpdate, SelectionVoiceRoute,
    SelectionVoiceSnapshot,
};
use crate::edit_plan::{apply_edit_plan, parse_edit_plan, EditOperation, EditPlan};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::ports::{TextPolisher, TextStreamChunk, TextStreamSink};
use crate::selection_voice_intent::{
    classify_selection_voice_intent_with_provider_result, clean_selection_voice_translation_output,
    infer_selection_voice_translation_target, selection_voice_instruction_looks_like_translation,
    SelectionVoiceIntent,
};
use crate::shared_types::SelectionPolishOutputMode;
use crate::types::{
    DictationSession, HistoryChange, HistoryInsertStatus, HistorySource, PolishMode, SessionId,
    VocabularyChange,
};
use crate::{ActivityStore, CorrectionRuleStore, DictionaryStore, HistoryStore, PreferencesStore};

#[derive(Debug, Clone)]
struct StoredPreview {
    owner_session_id: Option<SessionId>,
    text: String,
    previous_text: Option<String>,
    summary: Option<String>,
}

#[derive(Default)]
struct SelectionVoiceState {
    phase: SelectionVoicePhase,
    session_id: Option<SessionId>,
    selection: Option<SelectionCapture>,
    instruction_raw: Option<String>,
    instruction_polished: Option<String>,
    resolved_intent: Option<SelectionVoiceIntent>,
    intent_prompt: Option<SelectionVoiceIntentPrompt>,
    preview: Option<StoredPreview>,
    applying_ticket: Option<SelectionVoiceApplyTicket>,
    apply_outcome: Option<SelectionVoiceApplyOutcome>,
    started_at: Option<std::time::Instant>,
    recording_control: Option<Arc<dyn crate::ports::RecordingControlSink>>,
}

impl SelectionVoiceState {
    fn snapshot(&self) -> SelectionVoiceSnapshot {
        SelectionVoiceSnapshot {
            phase: self.phase,
            session_id: self.session_id,
            source_text: self.selection.as_ref().map(|capture| capture.text.clone()),
            instruction_raw: self.instruction_raw.clone(),
            instruction_polished: self.instruction_polished.clone(),
            intent_prompt: self.intent_prompt.clone(),
            preview: self.preview(),
            apply_outcome: self.apply_outcome,
        }
    }

    fn preview(&self) -> Option<SelectionVoicePreview> {
        let session_id = self.session_id?;
        let selection = self.selection.as_ref()?;
        let preview = self.preview.as_ref()?;
        Some(SelectionVoicePreview {
            session_id,
            owner_session_id: preview.owner_session_id,
            source_text: selection.text.clone(),
            text: preview.text.clone(),
            summary: preview.summary.clone(),
            source_app: selection.source_app.clone(),
            can_revert: preview.previous_text.is_some(),
        })
    }

    fn ensure_session(&self, session_id: SessionId) -> Result<(), BackendError> {
        if self.session_id != Some(session_id) {
            return Err(cancelled("selection voice session is stale"));
        }
        Ok(())
    }

    fn disposition(
        &self,
        intent: SelectionVoiceIntent,
    ) -> Result<SelectionVoiceDisposition, BackendError> {
        let session_id = self
            .session_id
            .ok_or_else(|| invalid_state("selection voice is idle"))?;
        let selection = self
            .selection
            .clone()
            .ok_or_else(|| invalid_state("selection voice capture is unavailable"))?;
        let instruction = self
            .instruction_polished
            .clone()
            .ok_or_else(|| invalid_state("selection voice instruction is unavailable"))?;
        Ok(match intent {
            SelectionVoiceIntent::Question => SelectionVoiceDisposition::Question {
                session_id,
                selection,
                instruction,
            },
            SelectionVoiceIntent::Edit => SelectionVoiceDisposition::Edit {
                session_id,
                selection,
                instruction,
            },
        })
    }
}

#[derive(Clone)]
pub(crate) struct SelectionVoiceService {
    state: Arc<RwLock<SelectionVoiceState>>,
    events: BackendEventPublisher,
    persistence: Arc<SelectionVoicePersistence>,
    workflow: Arc<SelectionVoiceWorkflow>,
    voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    qa: Arc<RwLock<Option<Weak<dyn QaApi>>>>,
    auto_press_at: Arc<RwLock<Option<std::time::Instant>>>,
}

struct SelectionVoiceWorkflow {
    preferences: Arc<PreferencesStore>,
    correction_rules: Arc<CorrectionRuleStore>,
    credential_store: Arc<dyn CredentialStore>,
    polisher: Option<Arc<dyn TextPolisher>>,
}

struct SelectionVoicePersistence {
    preferences: Arc<PreferencesStore>,
    history: Arc<HistoryStore>,
    history_revision: Arc<AtomicU64>,
    clock: Arc<dyn Clock>,
    vocabulary: Arc<DictionaryStore>,
    vocabulary_revision: Arc<AtomicU64>,
    correction_rules: Arc<CorrectionRuleStore>,
    activity: Arc<ActivityStore>,
    events: BackendEventPublisher,
}

impl SelectionVoiceService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        events: BackendEventPublisher,
        preferences: Arc<PreferencesStore>,
        history: Arc<HistoryStore>,
        history_revision: Arc<AtomicU64>,
        clock: Arc<dyn Clock>,
        vocabulary: Arc<DictionaryStore>,
        vocabulary_revision: Arc<AtomicU64>,
        correction_rules: Arc<CorrectionRuleStore>,
        activity: Arc<ActivityStore>,
        credential_store: Arc<dyn CredentialStore>,
        polisher: Option<Arc<dyn TextPolisher>>,
        voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(SelectionVoiceState::default())),
            events: events.clone(),
            persistence: Arc::new(SelectionVoicePersistence {
                preferences: Arc::clone(&preferences),
                history,
                history_revision,
                clock,
                vocabulary,
                vocabulary_revision,
                correction_rules: Arc::clone(&correction_rules),
                activity,
                events,
            }),
            workflow: Arc::new(SelectionVoiceWorkflow {
                preferences,
                correction_rules,
                credential_store,
                polisher,
            }),
            voice_sessions,
            qa: Arc::new(RwLock::new(None)),
            auto_press_at: Arc::new(RwLock::new(None)),
        }
    }

    fn qa(&self) -> Result<Arc<dyn QaApi>, BackendError> {
        self.qa
            .read()
            .expect("selection voice QA binding lock poisoned")
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Unsupported,
                    "selection voice QA routing is unavailable",
                )
            })
    }

    fn begin_session(
        &self,
        capture: SelectionCapture,
        phase: SelectionVoicePhase,
    ) -> Result<SessionId, BackendError> {
        if capture.text.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "selected text must not be empty",
            ));
        }
        let mut state = self
            .state
            .write()
            .expect("selection voice state lock poisoned");
        if matches!(
            state.phase,
            SelectionVoicePhase::Recording
                | SelectionVoicePhase::Processing
                | SelectionVoicePhase::AwaitingIntent
                | SelectionVoicePhase::Preview
                | SelectionVoicePhase::Applying
        ) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "a selection voice session is already active",
            ));
        }
        let session_id = SessionId::new();
        // QA has already captured its input and owns any voice lease itself.
        // A pure edit starts in Processing without inventing a second recorder;
        // actual Selection Voice capture still reserves audio until Preview.
        if phase == SelectionVoicePhase::Recording {
            self.voice_sessions.acquire(
                session_id,
                crate::voice_session::VoiceSessionKind::SelectionVoice,
            )?;
        }
        *state = SelectionVoiceState {
            phase,
            session_id: Some(session_id),
            selection: Some(capture),
            started_at: Some(std::time::Instant::now()),
            ..SelectionVoiceState::default()
        };
        let snapshot = state.snapshot();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionVoiceStateChanged(snapshot),
        );
        Ok(session_id)
    }
}

struct DiscardTextStream;

impl TextStreamSink for DiscardTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

impl SelectionVoiceWorkflow {
    fn corrected_instruction(&self, transcript: &str) -> Result<String, BackendError> {
        let rules = self.correction_rules.list()?;
        Ok(apply_correction_rules(transcript, &rules))
    }

    async fn model_text(
        &self,
        session_id: SessionId,
        preferences: &crate::shared_types::UserPreferences,
        input: String,
        system_prompt: String,
        translation_target: Option<&str>,
    ) -> Result<String, BackendError> {
        let polisher = self.polisher.as_ref().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Unsupported,
                "selection voice model runtime is not configured",
            )
        })?;
        let llm = crate::provider_resolution::resolve_session_provider(
            &self.credential_store,
            ProviderSlot::Llm,
            &preferences.active_llm_provider,
        )
        .await?;
        let translation_only = translation_target.is_some();
        let style_pack = crate::style_packs::builtin_style_pack_for_mode(if translation_only {
            PolishMode::Raw
        } else {
            PolishMode::Light
        });
        let mut context = DictationContext::capture(
            preferences,
            &style_pack,
            DictationProviderInvocations::new(
                ProviderInvocation::for_provider("selection-voice-unused-asr"),
                llm,
                ProviderInvocation::for_provider("selection-voice-unused-omni"),
            ),
            Vec::new(),
            Vec::new(),
            &DictationStartOptions::default(),
        );
        context.asr.prompt = None;
        context.polish.mode = if translation_only {
            PolishMode::Raw
        } else {
            PolishMode::Light
        };
        context.polish.style_system_prompt = if translation_only {
            style_pack.prompt
        } else {
            system_prompt
        };
        context.polish.translation_active = translation_only;
        context.polish.translation_target_language = translation_target.unwrap_or_default().into();
        context.polish.hotwords.clear();
        context.polish.cursor_context = None;
        context.polish.context_window_minutes = 0;
        context.polish.prior_turns.clear();
        let output = polisher
            .polish(
                session_id,
                Arc::new(context),
                input,
                Arc::new(DiscardTextStream),
            )
            .await?;
        let text = output.text.trim().to_string();
        if text.is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "selection voice model returned empty text",
            ));
        }
        Ok(text)
    }

    async fn polish_instruction(
        &self,
        session_id: SessionId,
        preferences: &crate::shared_types::UserPreferences,
        instruction: String,
    ) -> Result<String, BackendError> {
        self.model_text(
            session_id,
            preferences,
            instruction,
            crate::prompts::selection_voice_instruction_polish_prompt(),
            None,
        )
        .await
    }

    async fn auto_classification(
        &self,
        session_id: SessionId,
        preferences: &crate::shared_types::UserPreferences,
        instruction: &str,
    ) -> Option<String> {
        if preferences.selection_voice_intent_mode != crate::types::SelectionVoiceIntentMode::Auto {
            return None;
        }
        match self
            .model_text(
                session_id,
                preferences,
                instruction.to_string(),
                crate::prompts::selection_voice_intent_classification_prompt(),
                None,
            )
            .await
        {
            Ok(classification) => Some(classification),
            Err(error) => {
                log::warn!(
                    "selection voice intent model failed: {error}; using the core heuristic"
                );
                None
            }
        }
    }

    async fn generate_edit_plan(
        &self,
        session_id: SessionId,
        draft: &str,
        instruction: &str,
    ) -> Result<EditPlan, BackendError> {
        let preferences = self.preferences.get();
        if selection_voice_instruction_looks_like_translation(instruction) {
            let target = infer_selection_voice_translation_target(instruction, &preferences);
            if !target.is_empty() {
                return self
                    .generate_translation_plan(session_id, draft, &target, &preferences)
                    .await;
            }
        }

        let safe_draft = crate::prompts::sanitize_for_xml_envelope(draft, "draft");
        let safe_instruction =
            crate::prompts::sanitize_for_xml_envelope(instruction, "instruction");
        let input = format!(
            "<field_context></field_context>\n<draft>\n{safe_draft}\n</draft>\n\n<instruction>\n{safe_instruction}\n</instruction>"
        );
        let raw = self
            .model_text(
                session_id,
                &preferences,
                input,
                crate::prompts::voice_edit_system_prompt(),
                None,
            )
            .await?;
        match parse_edit_plan(&raw) {
            Ok(plan) => Ok(plan),
            Err(error) => {
                log::warn!(
                    "selection voice EditPlan parse failed: {error}; preview={}",
                    raw.chars().take(240).collect::<String>()
                );
                if selection_voice_instruction_looks_like_translation(instruction) {
                    let target =
                        infer_selection_voice_translation_target(instruction, &preferences);
                    if !target.is_empty() {
                        return self
                            .generate_translation_plan(session_id, draft, &target, &preferences)
                            .await;
                    }
                }
                Err(BackendError::new(BackendErrorCode::Provider, error))
            }
        }
    }

    async fn generate_translation_plan(
        &self,
        session_id: SessionId,
        draft: &str,
        target_language: &str,
        preferences: &crate::shared_types::UserPreferences,
    ) -> Result<EditPlan, BackendError> {
        let translated_raw = self
            .model_text(
                session_id,
                preferences,
                draft.to_string(),
                crate::prompts::translate_system_prompt(target_language),
                Some(target_language),
            )
            .await?;
        let translated = clean_selection_voice_translation_output(&translated_raw);
        if translated.is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                "translation produced empty text",
            ));
        }
        if translated == draft {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!("translation unchanged for target={target_language}"),
            ));
        }
        Ok(EditPlan {
            operations: vec![EditOperation::FullRewrite { text: translated }],
            summary: Some(format!("翻译为{target_language}")),
        })
    }

    async fn generate_preview(
        &self,
        session_id: SessionId,
        draft: &str,
        instruction: &str,
    ) -> Result<(String, Option<String>), BackendError> {
        let plan = self
            .generate_edit_plan(session_id, draft, instruction)
            .await?;
        let preview = apply_edit_plan(draft, &plan)
            .map_err(|error| BackendError::new(BackendErrorCode::Provider, error.to_string()))?;
        Ok((preview, plan.summary))
    }
}

impl SelectionVoicePersistence {
    fn corrected_text(&self, text: String) -> String {
        match self.correction_rules.list() {
            Ok(rules) => apply_correction_rules(&text, &rules),
            Err(error) => {
                log::warn!(
                    "failed to load correction rules for selection voice apply: {error}; continuing without correction"
                );
                text
            }
        }
    }

    fn persist_completed(
        &self,
        ticket: SelectionVoiceApplyTicket,
        outcome: SelectionVoiceApplyOutcome,
        duration_ms: Option<u64>,
    ) {
        let preferences = self.preferences.get();
        let dictionary_entry_count = match self.vocabulary.record_hits(&ticket.replacement_text) {
            Ok(hits) => {
                if hits > 0 {
                    let revision = self.vocabulary_revision.fetch_add(1, Ordering::AcqRel) + 1;
                    self.events.publish(
                        None,
                        BackendEventKind::VocabularyChanged(VocabularyChange { revision }),
                    );
                }
                Some(hits.min(u64::from(u32::MAX)) as u32)
            }
            Err(error) => {
                log::warn!("failed to record selection voice vocabulary hits: {error}");
                None
            }
        };
        let front = crate::shared_types::split_front_app_opt(ticket.source_app.as_deref());
        let insert_status = match outcome {
            SelectionVoiceApplyOutcome::Inserted => HistoryInsertStatus::Inserted,
            SelectionVoiceApplyOutcome::PasteSent => HistoryInsertStatus::PasteSent,
            SelectionVoiceApplyOutcome::CopiedFallback => HistoryInsertStatus::CopiedFallback,
            SelectionVoiceApplyOutcome::Failed => return,
        };
        let final_chars = ticket.replacement_text.chars().count() as u64;
        let session = DictationSession {
            id: ticket.session_id.to_string(),
            created_at: self.clock.now_utc().to_rfc3339(),
            source: HistorySource::SelectionVoiceEdit,
            raw_transcript: ticket.source_text,
            asr_transcript: None,
            final_text: ticket.replacement_text,
            mode: PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: ticket.summary,
            app_bundle_id: front.bundle_id,
            app_name: front.name,
            insert_status,
            error_code: None,
            duration_ms,
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
        let mut changed = false;
        match self.history.append_with_retention(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            Ok(()) => changed = true,
            Err(error) => log::warn!("failed to persist selection voice history: {error}"),
        }
        if let Err(error) = self.activity.bump(
            &self.clock.today_local().format("%Y-%m-%d").to_string(),
            final_chars,
            duration_ms.unwrap_or_default(),
        ) {
            log::warn!("failed to persist selection voice activity: {error}");
        } else {
            changed = true;
        }
        if changed {
            let revision = self.history_revision.fetch_add(1, Ordering::AcqRel) + 1;
            self.events.publish(
                None,
                BackendEventKind::HistoryChanged(HistoryChange { revision }),
            );
        }
    }
}

impl SelectionVoiceApi for SelectionVoiceService {
    fn bind_qa(&self, qa: Weak<dyn QaApi>) {
        *self
            .qa
            .write()
            .expect("selection voice QA binding lock poisoned") = Some(qa);
    }

    fn bind_recording_control(
        &self,
        session_id: SessionId,
        control: Arc<dyn crate::ports::RecordingControlSink>,
    ) -> Result<(), BackendError> {
        let mut state = self
            .state
            .write()
            .expect("selection voice state lock poisoned");
        if state.session_id != Some(session_id) || state.phase != SelectionVoicePhase::Recording {
            // Cancellation may win between acquiring the resource hold and
            // registering its Host slot. Revoke that late owner as well.
            drop(state);
            cancel_recording_control(Some(control), session_id)?;
            return Err(cancelled(
                "selection voice was cancelled before capture registration",
            ));
        }
        if state.recording_control.is_some() {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "selection voice capture is already registered",
            ));
        }
        state.recording_control = Some(control);
        Ok(())
    }

    fn dispatch_hotkey_edge(
        &self,
        edge: SelectionVoiceHotkeyEdge,
    ) -> Result<SelectionVoiceHotkeyAction, BackendError> {
        use crate::shared_types::HotkeyMode;

        const AUTO_HOLD_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(350);

        let preferences = self.workflow.preferences.get();
        if !preferences.selection_voice_enabled {
            return Ok(SelectionVoiceHotkeyAction::Noop);
        }
        let phase = self
            .state
            .read()
            .expect("selection voice state lock poisoned")
            .phase;
        let active = phase == SelectionVoicePhase::Recording;
        let idle = matches!(
            phase,
            SelectionVoicePhase::Idle
                | SelectionVoicePhase::Completed
                | SelectionVoicePhase::Cancelled
                | SelectionVoicePhase::Failed
        );
        Ok(match edge {
            SelectionVoiceHotkeyEdge::Pressed { at } if idle => {
                *self
                    .auto_press_at
                    .write()
                    .expect("selection voice hotkey lock poisoned") =
                    (preferences.hotkey.mode == HotkeyMode::Auto).then_some(at);
                SelectionVoiceHotkeyAction::Start
            }
            SelectionVoiceHotkeyEdge::Pressed { .. } if active => match preferences.hotkey.mode {
                HotkeyMode::Toggle | HotkeyMode::Auto => {
                    *self
                        .auto_press_at
                        .write()
                        .expect("selection voice hotkey lock poisoned") = None;
                    SelectionVoiceHotkeyAction::Finish
                }
                HotkeyMode::Hold | HotkeyMode::DoubleClick => SelectionVoiceHotkeyAction::Noop,
            },
            SelectionVoiceHotkeyEdge::Released { .. }
                if active && preferences.hotkey.mode == HotkeyMode::Hold =>
            {
                SelectionVoiceHotkeyAction::Finish
            }
            SelectionVoiceHotkeyEdge::Released { at }
                if active && preferences.hotkey.mode == HotkeyMode::Auto =>
            {
                let pressed_at = self
                    .auto_press_at
                    .write()
                    .expect("selection voice hotkey lock poisoned")
                    .take();
                if pressed_at.is_some_and(|pressed| {
                    at.saturating_duration_since(pressed) >= AUTO_HOLD_THRESHOLD
                }) {
                    SelectionVoiceHotkeyAction::Finish
                } else {
                    SelectionVoiceHotkeyAction::Noop
                }
            }
            SelectionVoiceHotkeyEdge::Pressed { .. }
            | SelectionVoiceHotkeyEdge::Released { .. } => SelectionVoiceHotkeyAction::Noop,
        })
    }

    fn recording_fault(
        &self,
        session_id: SessionId,
        _error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let voice_sessions = Arc::clone(&self.voice_sessions);
        let polisher = self.workflow.polisher.clone();
        Box::pin(async move {
            let (snapshot, control) = {
                let mut state = state.write().expect("selection voice state lock poisoned");
                state.ensure_session(session_id)?;
                if state.phase != SelectionVoicePhase::Recording {
                    return Err(invalid_state("selection voice is not recording"));
                }
                state.phase = SelectionVoicePhase::Failed;
                (state.snapshot(), state.recording_control.take())
            };
            events.publish(
                Some(session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            voice_sessions.release(session_id);
            let host_result = cancel_recording_control(control, session_id);
            if let Some(polisher) = polisher {
                polisher.cancel(session_id).await?;
            }
            host_result
        })
    }

    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionVoiceSnapshot, BackendError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            Ok(state
                .read()
                .expect("selection voice state lock poisoned")
                .snapshot())
        })
    }

    fn begin(
        &self,
        capture: SelectionCapture,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.begin_session(capture, SelectionVoicePhase::Recording) })
    }

    fn mark_processing(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        Box::pin(async move {
            let mut state = state.write().expect("selection voice state lock poisoned");
            state.ensure_session(session_id)?;
            if state.phase != SelectionVoicePhase::Recording {
                return Err(invalid_state("selection voice is not recording"));
            }
            state.phase = SelectionVoicePhase::Processing;
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                Some(session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            Ok(())
        })
    }

    fn process_transcript(
        &self,
        session_id: SessionId,
        transcript: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            {
                let state = service
                    .state
                    .read()
                    .expect("selection voice state lock poisoned");
                state.ensure_session(session_id)?;
                if state.phase != SelectionVoicePhase::Processing {
                    return Err(invalid_state("selection voice is not processing"));
                }
            }
            let transcript = transcript.trim();
            if transcript.is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection voice instruction must not be empty",
                ));
            }
            let raw = service.workflow.corrected_instruction(transcript)?;
            let preferences = service.workflow.preferences.get();
            let polished = service
                .workflow
                .polish_instruction(session_id, &preferences, raw.clone())
                .await?;
            let auto_classification = service
                .workflow
                .auto_classification(session_id, &preferences, &polished)
                .await;
            service
                .resolve_instruction(SelectionVoiceInstructionRequest {
                    session_id,
                    raw,
                    polished,
                    intent_mode: preferences.selection_voice_intent_mode,
                    manual_intent: preferences.selection_voice_manual_intent,
                    question_keywords: preferences.selection_voice_edit_keywords,
                    auto_classification,
                })
                .await
        })
    }

    fn resolve_instruction(
        &self,
        request: SelectionVoiceInstructionRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        Box::pin(async move {
            if request.polished.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection voice instruction must not be empty",
                ));
            }
            let mut state = state.write().expect("selection voice state lock poisoned");
            state.ensure_session(request.session_id)?;
            if state.phase != SelectionVoicePhase::Processing {
                return Err(invalid_state("selection voice is not processing"));
            }
            state.instruction_raw = Some(request.raw);
            state.instruction_polished = Some(request.polished.clone());
            if request.intent_mode == crate::types::SelectionVoiceIntentMode::Prompt {
                let prompt = SelectionVoiceIntentPrompt {
                    session_id: request.session_id,
                    instruction: request.polished,
                    source_text: state
                        .selection
                        .as_ref()
                        .map(|capture| capture.text.clone())
                        .unwrap_or_default(),
                };
                state.intent_prompt = Some(prompt.clone());
                state.phase = SelectionVoicePhase::AwaitingIntent;
                let snapshot = state.snapshot();
                drop(state);
                events.publish(
                    Some(request.session_id),
                    BackendEventKind::SelectionVoiceStateChanged(snapshot),
                );
                return Ok(SelectionVoiceDisposition::AwaitingIntent { prompt });
            }
            let classification = classify_selection_voice_intent_with_provider_result(
                request.intent_mode,
                request.manual_intent,
                &request.question_keywords,
                &request.polished,
                request.auto_classification.as_deref(),
            );
            state.resolved_intent = Some(classification.intent);
            let disposition = state.disposition(classification.intent)?;
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                Some(request.session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            Ok(disposition)
        })
    }

    fn confirm_intent(
        &self,
        session_id: SessionId,
        intent: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        Box::pin(async move {
            let intent = match intent.trim().to_ascii_lowercase().as_str() {
                "question" => SelectionVoiceIntent::Question,
                "edit" => SelectionVoiceIntent::Edit,
                other => {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        format!("invalid selection voice intent: {other}"),
                    ))
                }
            };
            let mut state = state.write().expect("selection voice state lock poisoned");
            state.ensure_session(session_id)?;
            if state.phase != SelectionVoicePhase::AwaitingIntent || state.intent_prompt.is_none() {
                return Err(invalid_state(
                    "selection voice intent prompt is unavailable",
                ));
            }
            state.intent_prompt = None;
            state.phase = SelectionVoicePhase::Processing;
            state.resolved_intent = Some(intent);
            let disposition = state.disposition(intent)?;
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                Some(session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            Ok(disposition)
        })
    }

    fn route_disposition(
        &self,
        disposition: SelectionVoiceDisposition,
    ) -> BoxFuture<'static, Result<SelectionVoiceRoute, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let session_id = match &disposition {
                SelectionVoiceDisposition::AwaitingIntent { prompt } => prompt.session_id,
                SelectionVoiceDisposition::Question { session_id, .. }
                | SelectionVoiceDisposition::Edit { session_id, .. } => *session_id,
            };
            let routed = async {
                match disposition {
                    SelectionVoiceDisposition::AwaitingIntent { prompt } => {
                        Ok(SelectionVoiceRoute::AwaitingIntent { prompt })
                    }
                    SelectionVoiceDisposition::Question { instruction, .. } => {
                        let qa = service.qa()?;
                        qa.show().await?;
                        qa.set_edit_instruction_mode(false).await?;
                        qa.submit_text(instruction).await?;
                        service.complete(session_id).await?;
                        Ok(SelectionVoiceRoute::QuestionCompleted { session_id })
                    }
                    SelectionVoiceDisposition::Edit { .. } => {
                        match service.prepare_edit(session_id, None).await? {
                            SelectionVoiceEditAction::OpenConversation {
                                session_id,
                                selection,
                                instruction,
                            } => {
                                let qa = service.qa()?;
                                qa.show().await?;
                                qa.set_edit_instruction_mode(true).await?;
                                // The capture predates the QA window. Passing
                                // it explicitly prevents a focus change from
                                // silently replacing the source text/target
                                // with whatever happens to be selected now.
                                qa.submit_selection_edit(session_id, selection, instruction)
                                    .await?;
                                Ok(SelectionVoiceRoute::EditConversationOpened { session_id })
                            }
                            SelectionVoiceEditAction::ReadyToApply { preview } => {
                                Ok(SelectionVoiceRoute::ReadyToApply { preview })
                            }
                        }
                    }
                }
            }
            .await;
            if routed.is_err() {
                let _ = service.cancel(Some(session_id)).await;
            }
            routed
        })
    }

    fn prepare_edit(
        &self,
        session_id: SessionId,
        owner_session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditAction, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let (selection, instruction) = {
                let state = service
                    .state
                    .read()
                    .expect("selection voice state lock poisoned");
                state.ensure_session(session_id)?;
                if state.phase != SelectionVoicePhase::Processing
                    || state.resolved_intent != Some(SelectionVoiceIntent::Edit)
                {
                    return Err(invalid_state("selection voice edit is not ready"));
                }
                (
                    state
                        .selection
                        .clone()
                        .ok_or_else(|| invalid_state("selection voice capture is unavailable"))?,
                    state.instruction_polished.clone().ok_or_else(|| {
                        invalid_state("selection voice instruction is unavailable")
                    })?,
                )
            };
            if service
                .workflow
                .preferences
                .get()
                .selection_polish_output_mode
                != SelectionPolishOutputMode::DirectReplace
            {
                return Ok(SelectionVoiceEditAction::OpenConversation {
                    session_id,
                    selection,
                    instruction,
                });
            }

            let (text, summary) = service
                .workflow
                .generate_preview(session_id, &selection.text, &instruction)
                .await?;
            service
                .set_preview(SelectionVoicePreviewUpdate {
                    session_id,
                    owner_session_id,
                    text,
                    summary,
                })
                .await?;
            let preview = service
                .preview(owner_session_id)
                .await?
                .ok_or_else(|| invalid_state("selection voice preview is unavailable"))?;
            Ok(SelectionVoiceEditAction::ReadyToApply { preview })
        })
    }

    fn edit_preview(
        &self,
        request: SelectionVoiceEditRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditPreviewResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            let instruction = request.instruction.trim().to_string();
            if instruction.is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection voice instruction must not be empty",
                ));
            }
            let owner = Some(request.owner_session_id);
            if let Some(existing) = service.preview(owner).await? {
                let (text, summary) = service
                    .workflow
                    .generate_preview(existing.session_id, &existing.text, &instruction)
                    .await?;
                service.replace_preview(owner, text, summary).await?;
                let preview = service
                    .preview(owner)
                    .await?
                    .ok_or_else(|| invalid_state("selection voice preview is unavailable"))?;
                return Ok(SelectionVoiceEditPreviewResult {
                    preview,
                    replaced_existing: true,
                });
            }

            if request.capture.text.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selected text must not be empty",
                ));
            }
            let reusable_session = {
                let state = service
                    .state
                    .read()
                    .expect("selection voice state lock poisoned");
                if state.phase == SelectionVoicePhase::Processing
                    && state.resolved_intent == Some(SelectionVoiceIntent::Edit)
                {
                    let selection = state
                        .selection
                        .as_ref()
                        .ok_or_else(|| invalid_state("selection voice capture is unavailable"))?;
                    if selection.text != request.capture.text {
                        return Err(cancelled(
                            "selection voice capture changed before edit preview",
                        ));
                    }
                    state.session_id
                } else {
                    None
                }
            };
            let created_session = reusable_session.is_none();
            let session_id = match reusable_session {
                Some(session_id) => session_id,
                None => service
                    .begin_session(request.capture.clone(), SelectionVoicePhase::Processing)?,
            };
            let generated = service
                .workflow
                .generate_preview(session_id, &request.capture.text, &instruction)
                .await;
            let (text, summary) = match generated {
                Ok(generated) => generated,
                Err(error) => {
                    if created_session {
                        let _ = service.cancel(Some(session_id)).await;
                    }
                    return Err(error);
                }
            };
            service
                .set_preview(SelectionVoicePreviewUpdate {
                    session_id,
                    owner_session_id: owner,
                    text,
                    summary,
                })
                .await?;
            let preview = service
                .preview(owner)
                .await?
                .ok_or_else(|| invalid_state("selection voice preview is unavailable"))?;
            Ok(SelectionVoiceEditPreviewResult {
                preview,
                replaced_existing: false,
            })
        })
    }

    fn set_preview(
        &self,
        update: SelectionVoicePreviewUpdate,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let voice_sessions = Arc::clone(&self.voice_sessions);
        Box::pin(async move {
            if update.text.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection voice preview must not be empty",
                ));
            }
            let mut state = state.write().expect("selection voice state lock poisoned");
            state.ensure_session(update.session_id)?;
            if state.phase != SelectionVoicePhase::Processing {
                return Err(invalid_state("selection voice is not processing"));
            }
            state.preview = Some(StoredPreview {
                owner_session_id: update.owner_session_id,
                text: update.text,
                previous_text: None,
                summary: update.summary,
            });
            state.phase = SelectionVoicePhase::Preview;
            // A pending text preview does not own audio. Release only this
            // session's logical lease: any native cleanup hold remains Busy,
            // and a QA/dictation microphone with a different owner is untouched.
            voice_sessions.release(update.session_id);
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                Some(update.session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            Ok(())
        })
    }

    fn replace_preview(
        &self,
        owner_session_id: Option<SessionId>,
        text: String,
        summary: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        Box::pin(async move {
            if text.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection voice preview must not be empty",
                ));
            }
            let mut state = state.write().expect("selection voice state lock poisoned");
            if state.phase != SelectionVoicePhase::Preview {
                return Err(invalid_state("selection voice preview is unavailable"));
            }
            let preview = matching_preview_mut(&mut state, owner_session_id)?;
            preview.previous_text = Some(std::mem::replace(&mut preview.text, text));
            preview.summary = summary;
            let session_id = state.session_id;
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                session_id,
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            Ok(())
        })
    }

    fn preview(
        &self,
        owner_session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<Option<SelectionVoicePreview>, BackendError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let state = state.read().expect("selection voice state lock poisoned");
            Ok(state
                .preview()
                .filter(|preview| preview.owner_session_id == owner_session_id))
        })
    }

    fn revert_preview(
        &self,
        owner_session_id: Option<SessionId>,
    ) -> Result<SelectionVoicePreview, BackendError> {
        let mut state = self
            .state
            .write()
            .expect("selection voice state lock poisoned");
        if state.phase != SelectionVoicePhase::Preview {
            return Err(invalid_state("selection voice preview is unavailable"));
        }
        let preview = matching_preview_mut(&mut state, owner_session_id)?;
        let previous = preview
            .previous_text
            .take()
            .ok_or_else(|| invalid_state("selection voice preview cannot be reverted"))?;
        preview.text = previous;
        preview.summary = None;
        let snapshot = state.snapshot();
        let preview = snapshot
            .preview
            .clone()
            .ok_or_else(|| invalid_state("selection voice preview is unavailable"))?;
        drop(state);
        self.events.publish(
            snapshot.session_id,
            BackendEventKind::SelectionVoiceStateChanged(snapshot),
        );
        Ok(preview)
    }

    fn begin_preview_apply(
        &self,
        owner_session_id: Option<SessionId>,
        text: String,
    ) -> Result<SelectionVoiceApplyTicket, BackendError> {
        let replacement_text = self.persistence.corrected_text(text.trim().to_string());
        if replacement_text.is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "selection voice output must not be empty",
            ));
        }
        let mut state = self
            .state
            .write()
            .expect("selection voice state lock poisoned");
        if state.phase != SelectionVoicePhase::Preview || state.applying_ticket.is_some() {
            return Err(invalid_state("selection voice preview is not applyable"));
        }
        let summary = {
            let preview = matching_preview_mut(&mut state, owner_session_id)?;
            preview.text = replacement_text.clone();
            preview.summary.clone()
        };
        let session_id = state
            .session_id
            .ok_or_else(|| invalid_state("selection voice session is unavailable"))?;
        let selection = state
            .selection
            .as_ref()
            .ok_or_else(|| invalid_state("selection voice capture is unavailable"))?;
        let ticket = SelectionVoiceApplyTicket {
            ticket_id: SessionId::new(),
            session_id,
            owner_session_id,
            source_text: selection.text.clone(),
            replacement_text,
            summary,
            source_app: selection.source_app.clone(),
        };
        state.applying_ticket = Some(ticket.clone());
        state.apply_outcome = None;
        state.phase = SelectionVoicePhase::Applying;
        let snapshot = state.snapshot();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionVoiceStateChanged(snapshot),
        );
        Ok(ticket)
    }

    fn finish_preview_apply(
        &self,
        ticket_id: SessionId,
        outcome: SelectionVoiceApplyOutcome,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let persistence = Arc::clone(&self.persistence);
        let voice_sessions = Arc::clone(&self.voice_sessions);
        Box::pin(async move {
            let mut state = state.write().expect("selection voice state lock poisoned");
            let ticket = state
                .applying_ticket
                .as_ref()
                .filter(|ticket| ticket.ticket_id == ticket_id)
                .cloned()
                .ok_or_else(|| cancelled("selection voice apply ticket is stale"))?;
            if state.session_id != Some(ticket.session_id) {
                return Err(cancelled("selection voice apply session is stale"));
            }
            state.applying_ticket = None;
            state.apply_outcome = Some(outcome);
            if outcome.may_have_applied() {
                state.preview = None;
                state.phase = SelectionVoicePhase::Completed;
            } else {
                state.phase = SelectionVoicePhase::Preview;
            }
            let session_id = state.session_id;
            let duration_ms = state.started_at.map(|started_at| {
                started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
            });
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                session_id,
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            if outcome.may_have_applied() {
                persistence.persist_completed(ticket, outcome, duration_ms);
                if let Some(session_id) = session_id {
                    voice_sessions.release(session_id);
                }
            }
            Ok(())
        })
    }

    fn complete(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let voice_sessions = Arc::clone(&self.voice_sessions);
        Box::pin(async move {
            let mut state = state.write().expect("selection voice state lock poisoned");
            state.ensure_session(session_id)?;
            if matches!(
                state.phase,
                SelectionVoicePhase::Idle
                    | SelectionVoicePhase::Completed
                    | SelectionVoicePhase::Cancelled
                    | SelectionVoicePhase::Failed
                    | SelectionVoicePhase::Applying
            ) {
                return Err(invalid_state("selection voice session cannot be completed"));
            }
            state.phase = SelectionVoicePhase::Completed;
            state.intent_prompt = None;
            state.preview = None;
            state.applying_ticket = None;
            let snapshot = state.snapshot();
            drop(state);
            events.publish(
                Some(session_id),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            voice_sessions.release(session_id);
            Ok(())
        })
    }

    fn cancel(
        &self,
        session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let polisher = self.workflow.polisher.clone();
        let voice_sessions = Arc::clone(&self.voice_sessions);
        Box::pin(async move {
            let (active_session, snapshot, control) = {
                let mut state = state.write().expect("selection voice state lock poisoned");
                let Some(active_session) = state.session_id else {
                    return Ok(());
                };
                if session_id.is_some() && session_id != Some(active_session) {
                    return Err(cancelled("selection voice session is stale"));
                }
                if state.phase == SelectionVoicePhase::Cancelled {
                    return Ok(());
                }
                state.phase = SelectionVoicePhase::Cancelled;
                state.intent_prompt = None;
                state.preview = None;
                state.applying_ticket = None;
                (
                    active_session,
                    state.snapshot(),
                    state.recording_control.take(),
                )
            };
            events.publish(
                Some(active_session),
                BackendEventKind::SelectionVoiceStateChanged(snapshot),
            );
            voice_sessions.release(active_session);
            // The token prevents late startup; this controller actually takes
            // the Host capture and invokes its owned stop/ASR cleanup. Its
            // resource hold remains live until those native effects settle.
            let host_result = cancel_recording_control(control, active_session);
            if let Some(polisher) = polisher {
                if let Err(error) = polisher.cancel(active_session).await {
                    log::warn!("failed to cancel selection voice model request: {error}");
                }
            }
            host_result
        })
    }
}

fn cancel_recording_control(
    control: Option<Arc<dyn crate::ports::RecordingControlSink>>,
    session_id: SessionId,
) -> Result<(), BackendError> {
    control.map_or(Ok(()), |control| {
        control.request(session_id, crate::events::RecordingControlAction::Cancel)
    })
}

fn matching_preview_mut(
    state: &mut SelectionVoiceState,
    owner_session_id: Option<SessionId>,
) -> Result<&mut StoredPreview, BackendError> {
    let preview = state
        .preview
        .as_mut()
        .ok_or_else(|| invalid_state("selection voice preview is unavailable"))?;
    if preview.owner_session_id != owner_session_id {
        return Err(cancelled("selection voice preview owner is stale"));
    }
    Ok(preview)
}

fn invalid_state(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidState, message)
}

fn cancelled(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, message)
}
