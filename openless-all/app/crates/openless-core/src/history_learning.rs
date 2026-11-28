//! Explicit user feedback, separate from external control observation.
use super::*;
use chrono::Datelike;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningSource {
    pub token: String,
    pub history_id: String,
    pub original_text: String,
    pub expires_at_ms: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningCandidate {
    pub id: String,
    pub pattern: String,
    pub replacement: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningResult {
    pub candidates: Vec<LearningCandidate>,
    pub expression_recorded: bool,
}

pub(super) struct Feedback {
    pub(super) source: LearningSource,
    revision: u64,
    generation: u64,
    request: Option<(String, bool, bool)>,
    result: Option<LearningResult>,
    accepted: Vec<String>,
}

#[derive(Default)]
pub(super) struct LearningState {
    pub(super) tokens: HashMap<String, Feedback>,
}

fn failure(key: &str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, key)
}
fn invalid() -> BackendError { failure("learningSourceExpired") }
fn storage(_: impl std::fmt::Display) -> BackendError { failure("learningStoreFailed") }

fn eligible(entry: &DictationSession) -> bool {
    entry.source == HistorySource::Voice
        && entry.error_code.is_none()
        && !entry.translation_active
        && entry.polish_source.as_deref() != Some("translation")
        && !entry.final_text.trim().is_empty()
        && entry.final_text.chars().count() <= 16_384
        && uuid::Uuid::parse_str(&entry.id).is_ok()
        && !crate::expression_observation::contains_sensitive_content(&entry.final_text)
}

impl OpenLessBackend {
    pub fn begin_history_learning(&self, history_id: &str) -> Result<LearningSource, BackendError> {
        let _settings = self.settings_write_gate.lock().map_err(storage)?;
        let preferences = self.get_preferences();
        if !preferences.vocabulary_learning_enabled && !preferences.expression_learning_enabled {
            return Err(failure("learningPermissionOff"));
        }
        let mut state = self.history_learning.lock().map_err(storage)?;
        let now = self.clock.now_utc().timestamp_millis();
        state.tokens.retain(|_, feedback| feedback.source.expires_at_ms > now);
        if state.tokens.len() >= 8 { return Err(failure("learningLimit")); }
        self.history.with_learning_source(history_id, None, |entry, revision| {
            if !eligible(entry) { return Err(failure("learningNotEligible")); }
            let source = LearningSource {
                token: uuid::Uuid::new_v4().to_string(),
                history_id: history_id.into(), original_text: entry.final_text.clone(),
                expires_at_ms: now + 300_000,
            };
            state.tokens.insert(source.token.clone(), Feedback {
                source: source.clone(), revision,
                generation: self.edit_observation_generation.load(Ordering::Acquire),
                request: None, result: None, accepted: Vec::new(),
            });
            Ok(source)
        })
    }

    pub fn cancel_history_learning(&self, token: &str) {
        if let Ok(mut state) = self.history_learning.lock() { state.tokens.remove(token); }
    }

    pub fn submit_history_learning(&self, token: &str, corrected_text: String,
        request_vocabulary: bool, request_expression: bool) -> Result<LearningResult, BackendError> {
        let _settings = self.settings_write_gate.lock().map_err(storage)?;
        let preferences = self.get_preferences();
        if (request_vocabulary && !preferences.vocabulary_learning_enabled)
            || (request_expression && !preferences.expression_learning_enabled)
            || (!request_vocabulary && !request_expression) {
            return Err(failure("learningPermissionOff"));
        }
        if corrected_text.trim().is_empty() { return Err(failure("learningEmpty")); }
        if corrected_text.chars().count() > 16_384 { return Err(failure("learningLimit")); }
        if crate::expression_observation::contains_sensitive_content(&corrected_text) {
            return Err(failure("learningNotEligible"));
        }
        let mut state = self.history_learning.lock().map_err(storage)?;
        let feedback = state.tokens.get_mut(token).ok_or_else(invalid)?;
        let request = (corrected_text, request_vocabulary, request_expression);
        self.history.with_learning_source(&feedback.source.history_id, Some(feedback.revision), |entry, _| {
            self.validate_learning_feedback(feedback, entry)?;
            if feedback.request.as_ref().is_some_and(|previous| previous != &request) {
                return Err(failure("learningPayloadConflict"));
            }
            if let Some(result) = &feedback.result { return Ok(result.clone()); }
            // Freeze before any fallible sink: retries cannot change their payload.
            feedback.request = Some(request.clone());
            let edit = crate::host_document::minimal_edit(&feedback.source.original_text, &request.0);
            let mut result = LearningResult { candidates: Vec::new(), expression_recorded: false };
            if let Some(edit) = edit {
                if request_vocabulary {
                    result.candidates = crate::correction_observation::vocabulary_candidates(
                        &feedback.source.original_text, &edit,
                        preferences.vocabulary_learning_settings.max_phrase_chars as usize,
                    ).into_iter().map(|rule| LearningCandidate {
                        id: uuid::Uuid::new_v4().to_string(), pattern: rule.pattern,
                        replacement: rule.replacement,
                    }).collect();
                }
                if request_expression {
                    if let Ok(signal) = crate::expression_observation::signal_from_observed_edit(
                        &feedback.source.original_text, &edit) {
                        let profile = self.expression_profile.lock().map_err(storage)?
                            .clone().ok_or_else(|| failure("learningStoreFailed"))?;
                        result.expression_recorded = profile.record(
                            uuid::Uuid::parse_str(&entry.id).map_err(|_| invalid())?,
                            self.clock.today_local().num_days_from_ce(), signal).map_err(storage)?;
                    }
                }
            }
            feedback.result = Some(result.clone());
            Ok(result)
        })
    }

    fn validate_learning_feedback(&self, feedback: &Feedback, entry: &DictationSession)
        -> Result<(), BackendError> {
        if feedback.source.expires_at_ms <= self.clock.now_utc().timestamp_millis()
            || feedback.generation != self.edit_observation_generation.load(Ordering::Acquire)
            || !eligible(entry) || entry.final_text != feedback.source.original_text {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn accept_history_learning(&self, token: &str, candidate_id: &str) -> Result<bool, BackendError> {
        let _settings = self.settings_write_gate.lock().map_err(storage)?;
        if !self.get_preferences().vocabulary_learning_enabled { return Err(failure("learningPermissionOff")); }
        let mut state = self.history_learning.lock().map_err(storage)?;
        let feedback = state.tokens.get_mut(token).ok_or_else(invalid)?;
        self.history.with_learning_source(&feedback.source.history_id, Some(feedback.revision), |entry, _| {
            self.validate_learning_feedback(feedback, entry)?;
            if feedback.accepted.iter().any(|id| id == candidate_id) { return Ok(false); }
            let candidate = feedback.result.as_ref().and_then(|result|
                result.candidates.iter().find(|candidate| candidate.id == candidate_id)).ok_or_else(invalid)?;
            let added = self.vocabulary.add_if_absent(candidate.replacement.clone(), Some(LEARNED_VOCAB_NOTE.into())).map_err(storage)?;
            feedback.accepted.push(candidate_id.into());
            if added.is_some() { self.publish_vocabulary_changed(); }
            Ok(added.is_some())
        })
    }
}
