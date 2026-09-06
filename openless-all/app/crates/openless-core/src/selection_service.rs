use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use futures_util::future::BoxFuture;

use crate::config::Clock;
use crate::correction::apply_correction_rules;
use crate::credentials::{CredentialStore, ProviderSlot};
use crate::dictation_context::{
    DictationContext, DictationProviderInvocations, DictationStartOptions, ProviderInvocation,
};
use crate::domains::{
    SelectionApi, SelectionCapture, SelectionPhase, SelectionPolishRequest,
    SelectionRuntimeAdapter, SelectionSnapshot,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::ports::{HostAction, HostActions, TextPolisher, TextStreamChunk, TextStreamSink};
use crate::shared_types::SelectionPolishOutputMode;
use crate::style_packs::{style_pack_prompt, StylePromptKind};
use crate::types::{
    DictationSession, HistoryChange, HistoryInsertStatus, HistorySource, SessionId,
    VocabularyChange,
};
use crate::{
    ActivityStore, CorrectionRuleStore, DictionaryStore, HistoryStore, PreferencesStore,
    StylePackStore,
};

#[derive(Clone, Default)]
struct SelectionState {
    snapshot: SelectionSnapshot,
    source_app: Option<String>,
    context: Option<Arc<DictationContext>>,
    started_at: Option<std::time::Instant>,
    polish_source: Option<String>,
    polish_ms: Option<u64>,
    llm_used: bool,
    reverting: bool,
}

struct SelectionServiceInner {
    preferences: Arc<PreferencesStore>,
    style_packs: Arc<StylePackStore>,
    runtime: Arc<dyn SelectionRuntimeAdapter>,
    polisher: Arc<dyn TextPolisher>,
    host_actions: Arc<dyn HostActions>,
    events: BackendEventPublisher,
    history: Arc<HistoryStore>,
    history_revision: Arc<AtomicU64>,
    clock: Arc<dyn Clock>,
    vocabulary: Arc<DictionaryStore>,
    vocabulary_revision: Arc<AtomicU64>,
    correction_rules: Arc<CorrectionRuleStore>,
    activity: Arc<ActivityStore>,
    credential_store: Arc<dyn CredentialStore>,
    state: RwLock<SelectionState>,
}

pub(crate) struct SelectionService {
    inner: Arc<SelectionServiceInner>,
}

pub(crate) struct SelectionServiceDependencies {
    pub(crate) preferences: Arc<PreferencesStore>,
    pub(crate) style_packs: Arc<StylePackStore>,
    pub(crate) runtime: Arc<dyn SelectionRuntimeAdapter>,
    pub(crate) polisher: Arc<dyn TextPolisher>,
    pub(crate) host_actions: Arc<dyn HostActions>,
    pub(crate) events: BackendEventPublisher,
    pub(crate) history: Arc<HistoryStore>,
    pub(crate) history_revision: Arc<AtomicU64>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) vocabulary: Arc<DictionaryStore>,
    pub(crate) vocabulary_revision: Arc<AtomicU64>,
    pub(crate) correction_rules: Arc<CorrectionRuleStore>,
    pub(crate) activity: Arc<ActivityStore>,
    pub(crate) credential_store: Arc<dyn CredentialStore>,
}

impl SelectionService {
    pub(crate) fn new(dependencies: SelectionServiceDependencies) -> Self {
        Self {
            inner: Arc::new(SelectionServiceInner {
                preferences: dependencies.preferences,
                style_packs: dependencies.style_packs,
                runtime: dependencies.runtime,
                polisher: dependencies.polisher,
                host_actions: dependencies.host_actions,
                events: dependencies.events,
                history: dependencies.history,
                history_revision: dependencies.history_revision,
                clock: dependencies.clock,
                vocabulary: dependencies.vocabulary,
                vocabulary_revision: dependencies.vocabulary_revision,
                correction_rules: dependencies.correction_rules,
                activity: dependencies.activity,
                credential_store: dependencies.credential_store,
                state: RwLock::new(SelectionState::default()),
            }),
        }
    }
}

impl SelectionServiceInner {
    fn hide_preview(&self) {
        if let Err(error) = self.host_actions.request(HostAction::HideSelectionPreview) {
            log::warn!("failed to hide selection preview: {error}");
        }
    }

    fn begin(&self, request: &SelectionPolishRequest) -> Result<SessionId, BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        if matches!(
            state.snapshot.phase,
            SelectionPhase::Capturing | SelectionPhase::Preview | SelectionPhase::Applying
        ) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "a selection session is already active",
            ));
        }
        let session_id = SessionId::new();
        state.snapshot = SelectionSnapshot {
            phase: SelectionPhase::Capturing,
            session_id: Some(session_id),
            source_text: None,
            preview_text: None,
            instruction: request.instruction.clone(),
            insert_outcome: None,
            revert_outcome: None,
        };
        state.source_app = None;
        state.context = None;
        state.started_at = Some(std::time::Instant::now());
        state.polish_source = None;
        state.polish_ms = None;
        state.llm_used = false;
        state.reverting = false;
        let snapshot = state.snapshot.clone();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionStateChanged(snapshot),
        );
        Ok(session_id)
    }

    fn ensure_active(state: &SelectionState, session_id: SessionId) -> Result<(), BackendError> {
        if state.snapshot.session_id != Some(session_id)
            || matches!(
                state.snapshot.phase,
                SelectionPhase::Cancelled | SelectionPhase::Failed
            )
        {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "selection session is no longer active",
            ));
        }
        Ok(())
    }

    fn set_capture(
        &self,
        session_id: SessionId,
        capture: &SelectionCapture,
    ) -> Result<(), BackendError> {
        if capture.text.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "selected text must not be empty",
            ));
        }
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.snapshot.source_text = Some(capture.text.clone());
        state.source_app = capture.source_app.clone();
        Ok(())
    }

    async fn polish_context(
        &self,
        request: &SelectionPolishRequest,
        source_app: Option<String>,
    ) -> Result<(DictationContext, SelectionPolishOutputMode, bool), BackendError> {
        let preferences = self.preferences.get();
        let style_pack = self
            .style_packs
            .get_or_default_active(&preferences.selection_polish_style_pack_id)?;
        let mut selection_prompt = style_pack_prompt(&style_pack, StylePromptKind::Selection);
        let instruction = request
            .instruction
            .as_deref()
            .map(str::trim)
            .filter(|instruction| !instruction.is_empty());
        let uses_llm = request.mode != crate::types::PolishMode::Raw
            || selection_prompt
                != crate::style_packs::default_selection_polish_style_prompt_for_mode(
                    crate::types::PolishMode::Raw,
                )
            || instruction.is_some();
        if let Some(block) = instruction.and_then(crate::prompts::selection_instruction_block) {
            selection_prompt = format!("{selection_prompt}\n\n{block}");
        }
        let llm = if uses_llm {
            crate::provider_resolution::resolve_session_provider(
                &self.credential_store,
                ProviderSlot::Llm,
                &preferences.active_llm_provider,
            )
            .await?
        } else {
            ProviderInvocation::for_provider(preferences.active_llm_provider.clone())
        };
        let mut context = DictationContext::capture(
            &preferences,
            &style_pack,
            DictationProviderInvocations::new(
                ProviderInvocation::for_provider(preferences.active_asr_provider.clone()),
                llm,
                ProviderInvocation::for_provider(preferences.active_omni_provider.clone()),
            ),
            Vec::new(),
            Vec::new(),
            &DictationStartOptions {
                front_app: source_app,
                ..DictationStartOptions::default()
            },
        );
        context.asr.prompt = None;
        context.polish.mode = request.mode;
        context.polish.style_system_prompt = selection_prompt;
        context.polish.translation_active = false;
        context.polish.cursor_context = None;
        context.polish.context_window_minutes = 0;
        context.polish.prior_turns.clear();
        Ok((context, preferences.selection_polish_output_mode, uses_llm))
    }

    fn set_polish_output(
        &self,
        session_id: SessionId,
        output: &crate::ports::PolishOutput,
        polish_ms: Option<u64>,
        llm_used: bool,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.polish_source = output.source_text.clone();
        state.polish_ms = polish_ms;
        state.llm_used = llm_used;
        Ok(())
    }

    fn source_app(&self, session_id: SessionId) -> Result<Option<String>, BackendError> {
        let state = self.state.read().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        Ok(state.source_app.clone())
    }

    fn set_context(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.context = Some(context);
        Ok(())
    }

    fn set_preview(&self, session_id: SessionId, preview_text: String) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.snapshot.preview_text = Some(preview_text);
        state.snapshot.phase = SelectionPhase::Preview;
        let snapshot = state.snapshot.clone();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionStateChanged(snapshot),
        );
        Ok(())
    }

    fn applying_text(
        &self,
        session_id: SessionId,
        replacement: Option<String>,
    ) -> Result<(String, String), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        if state.snapshot.session_id != Some(session_id) {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "selection session is stale",
            ));
        }
        if state.snapshot.phase != SelectionPhase::Preview {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "selection preview is not awaiting confirmation",
            ));
        }
        let source_text = state.snapshot.source_text.clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "selection source is unavailable",
            )
        })?;
        let replacement_text = replacement
            .or_else(|| state.snapshot.preview_text.clone())
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "preview text must not be empty",
                )
            })?;
        state.snapshot.preview_text = Some(replacement_text.clone());
        state.snapshot.insert_outcome = None;
        state.snapshot.phase = SelectionPhase::Applying;
        let snapshot = state.snapshot.clone();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionStateChanged(snapshot),
        );
        Ok((source_text, replacement_text))
    }

    fn corrected_replacement(
        &self,
        session_id: SessionId,
        replacement_text: String,
    ) -> Result<String, BackendError> {
        let rules = match self.correction_rules.list() {
            Ok(rules) => rules,
            Err(error) => {
                log::warn!(
                    "failed to load correction rules for completed selection: {error}; continuing without correction"
                );
                Vec::new()
            }
        };
        let final_text = if rules.is_empty() {
            replacement_text
        } else {
            apply_correction_rules(&replacement_text, &rules)
        };
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        if state.snapshot.phase != SelectionPhase::Applying {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "selection session is not applying",
            ));
        }
        state.snapshot.preview_text = Some(final_text.clone());
        Ok(final_text)
    }

    fn complete(
        &self,
        session_id: SessionId,
        outcome: crate::ports::InsertOutcome,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.snapshot.insert_outcome = Some(outcome);
        state.snapshot.phase = SelectionPhase::Completed;
        // Completion makes a new capture admissible. Freeze all history fields
        // before publishing it so a successor cannot replace this turn's text.
        let completed = state.clone();
        let duration_ms = state
            .started_at
            .map(|started_at| started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionStateChanged(completed.snapshot.clone()),
        );
        self.persist_completed(session_id, completed, outcome, duration_ms);
        Ok(())
    }

    fn persist_completed(
        &self,
        session_id: SessionId,
        completed: SelectionState,
        outcome: crate::ports::InsertOutcome,
        duration_ms: Option<u64>,
    ) {
        let Some(context) = completed.context else {
            return;
        };
        let source_text = completed.snapshot.source_text.unwrap_or_default();
        let final_text = completed.snapshot.preview_text.unwrap_or_default();
        let front_app =
            crate::shared_types::split_front_app_opt(context.polish.front_app.as_deref());
        let insert_status = match outcome {
            crate::ports::InsertOutcome::Inserted => HistoryInsertStatus::Inserted,
            crate::ports::InsertOutcome::PasteSent => HistoryInsertStatus::PasteSent,
            crate::ports::InsertOutcome::CopiedFallback => HistoryInsertStatus::CopiedFallback,
        };
        let preferences = self.preferences.get();
        let final_text_chars = final_text.chars().count() as u64;
        let dictionary_entry_count = match self.vocabulary.record_hits(&final_text) {
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
                log::warn!("failed to record vocabulary hits for completed selection: {error}");
                None
            }
        };
        let session = DictationSession {
            id: session_id.to_string(),
            created_at: self.clock.now_utc().to_rfc3339(),
            source: HistorySource::SelectionPolish,
            raw_transcript: source_text,
            asr_transcript: None,
            final_text,
            mode: context.polish.mode,
            style_pack_id: Some(context.polish.style_pack_id.clone()),
            translation_active: false,
            polish_source: completed.polish_source,
            app_bundle_id: front_app.bundle_id,
            app_name: front_app.name,
            insert_status,
            error_code: None,
            duration_ms,
            dictionary_entry_count,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: completed.llm_used.then(|| context.llm.provider_id.clone()),
            llm_model: completed
                .llm_used
                .then(|| context.llm.model.clone())
                .flatten(),
            pipeline_mode: Some("traditional".to_string()),
            asr_ms: None,
            polish_ms: completed.polish_ms,
        };
        let mut changed = false;
        match self.history.append_with_retention(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            Ok(()) => changed = true,
            Err(error) => log::warn!("failed to persist completed selection history: {error}"),
        }
        if let Err(error) = self.activity.bump(
            &self.clock.today_local().format("%Y-%m-%d").to_string(),
            final_text_chars,
            duration_ms.unwrap_or_default(),
        ) {
            log::warn!("failed to persist completed selection activity: {error}");
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

    fn fail_if_active(&self, session_id: SessionId) -> bool {
        let mut state = self.state.write().expect("selection state lock poisoned");
        if state.snapshot.session_id == Some(session_id)
            && !matches!(state.snapshot.phase, SelectionPhase::Cancelled)
        {
            state.snapshot.phase = SelectionPhase::Failed;
            let snapshot = state.snapshot.clone();
            drop(state);
            self.events.publish(
                Some(session_id),
                BackendEventKind::SelectionStateChanged(snapshot),
            );
            true
        } else {
            false
        }
    }

    fn begin_revert(&self, session_id: SessionId) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        if state.snapshot.session_id != Some(session_id) {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "selection session is stale",
            ));
        }
        if state.snapshot.phase != SelectionPhase::Completed
            || state.reverting
            || state.snapshot.revert_outcome.is_some()
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "selection session is not revertible",
            ));
        }
        state.reverting = true;
        Ok(())
    }

    fn finish_revert(
        &self,
        session_id: SessionId,
        outcome: crate::ports::InsertOutcome,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("selection state lock poisoned");
        Self::ensure_active(&state, session_id)?;
        state.reverting = false;
        state.snapshot.revert_outcome = Some(outcome);
        let snapshot = state.snapshot.clone();
        drop(state);
        self.events.publish(
            Some(session_id),
            BackendEventKind::SelectionStateChanged(snapshot),
        );
        Ok(())
    }

    fn abort_revert(&self, session_id: SessionId) {
        let mut state = self.state.write().expect("selection state lock poisoned");
        if state.snapshot.session_id == Some(session_id) {
            state.reverting = false;
        }
    }
}

impl SelectionApi for SelectionService {
    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionSnapshot, BackendError>> {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            Ok(inner
                .state
                .read()
                .expect("selection state lock poisoned")
                .snapshot
                .clone())
        })
    }

    fn begin_polish(
        &self,
        request: SelectionPolishRequest,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let session_id = inner.begin(&request)?;
            let result = async {
                let capture = inner
                    .runtime
                    .capture(session_id, request.selected_text.clone())
                    .await?;
                inner.set_capture(session_id, &capture)?;
                let (context, output_mode, uses_llm) = inner
                    .polish_context(&request, inner.source_app(session_id)?)
                    .await?;
                let context = Arc::new(context);
                inner.set_context(session_id, Arc::clone(&context))?;
                let (output, polish_ms) = if uses_llm {
                    let polish_started = std::time::Instant::now();
                    let output = inner
                        .polisher
                        .polish(
                            session_id,
                            context,
                            capture.text.clone(),
                            Arc::new(DiscardTextStreamSink),
                        )
                        .await?;
                    (
                        output,
                        Some(
                            polish_started
                                .elapsed()
                                .as_millis()
                                .min(u128::from(u64::MAX)) as u64,
                        ),
                    )
                } else {
                    (crate::ports::PolishOutput::text(capture.text.clone()), None)
                };
                inner.set_polish_output(session_id, &output, polish_ms, uses_llm)?;
                match output_mode {
                    SelectionPolishOutputMode::PreviewConfirm => {
                        inner.runtime.prepare_preview(session_id).await?;
                        inner.set_preview(session_id, output.text)?;
                        inner
                            .host_actions
                            .request(HostAction::ShowSelectionPreview)?;
                    }
                    SelectionPolishOutputMode::DirectReplace => {
                        inner.set_preview(session_id, output.text)?;
                        let (source_text, replacement_text) =
                            inner.applying_text(session_id, None)?;
                        let replacement_text =
                            inner.corrected_replacement(session_id, replacement_text)?;
                        let outcome = inner
                            .runtime
                            .apply(session_id, source_text, replacement_text)
                            .await?;
                        inner.complete(session_id, outcome)?;
                    }
                }
                Ok(session_id)
            }
            .await;
            if result.is_err() && inner.fail_if_active(session_id) {
                let _ = inner.polisher.cancel(session_id).await;
                let _ = inner.runtime.cancel(session_id).await;
                inner.hide_preview();
            }
            result
        })
    }

    fn confirm(
        &self,
        session_id: SessionId,
        text: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let (source_text, replacement_text) = inner.applying_text(session_id, text)?;
            let replacement_text = inner.corrected_replacement(session_id, replacement_text)?;
            let result = inner
                .runtime
                .apply(session_id, source_text, replacement_text)
                .await;
            match result {
                Ok(outcome) => {
                    inner.complete(session_id, outcome)?;
                    inner.hide_preview();
                    Ok(())
                }
                Err(error) => {
                    if inner.fail_if_active(session_id) {
                        let _ = inner.polisher.cancel(session_id).await;
                        let _ = inner.runtime.cancel(session_id).await;
                        inner.hide_preview();
                    }
                    Err(error)
                }
            }
        })
    }

    fn cancel(
        &self,
        session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let active_session = {
                let mut state = inner.state.write().expect("selection state lock poisoned");
                let Some(active_session) = state.snapshot.session_id else {
                    return Ok(());
                };
                if session_id.is_some() && session_id != Some(active_session) {
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "selection session is stale",
                    ));
                }
                if state.snapshot.phase == SelectionPhase::Cancelled {
                    return Ok(());
                }
                state.snapshot.phase = SelectionPhase::Cancelled;
                let snapshot = state.snapshot.clone();
                (active_session, snapshot)
            };
            inner.events.publish(
                Some(active_session.0),
                BackendEventKind::SelectionStateChanged(active_session.1),
            );
            inner.hide_preview();
            let active_session = active_session.0;
            let polish_result = inner.polisher.cancel(active_session).await;
            let runtime_result = inner.runtime.cancel(active_session).await;
            polish_result?;
            runtime_result
        })
    }

    fn revert(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            inner.begin_revert(session_id)?;
            match inner.runtime.revert(session_id).await {
                Ok(outcome) => inner.finish_revert(session_id, outcome),
                Err(error) => {
                    inner.abort_revert(session_id);
                    Err(error)
                }
            }
        })
    }
}

struct DiscardTextStreamSink;

impl TextStreamSink for DiscardTextStreamSink {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}
