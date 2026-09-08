//! Stable host-facing interfaces for application domains that have platform or
//! transport implementations.
//!
//! The traits in this module are deliberately grouped by use-case instead of
//! mirroring Tauri command names.  Linux/egui code only depends on these DTOs
//! and traits; Tauri remains a compatibility adapter for the legacy IPC names.

use std::path::PathBuf;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::coding_agent::{
    CodingAgentAvailability, CodingAgentDetectRequest, CodingAgentModelsRequest,
    CodingAgentPermissionMode, CodingAgentProvider, CodingAgentTestRequest, CodingAgentTestStatus,
    CommandRiskAssessment,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::local_asr_catalog::{
    FoundryRuntimeSource, LocalAsrMirror, LocalAsrRuntime, LocalAsrTarget,
};
use crate::style_packs::StylePack;
use crate::types::{PolishMode, SessionId};

fn unsupported<T>(domain: &'static str) -> BoxFuture<'static, Result<T, BackendError>> {
    Box::pin(async move {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            format!("{domain} service is not configured"),
        ))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Asr,
    Llm,
    Omni,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequest {
    pub kind: ProviderKind,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckResult {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelsResult {
    pub models: Vec<String>,
}

pub trait ProviderApi: Send + Sync {
    fn validate(
        &self,
        request: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderCheckResult, BackendError>>;

    fn list_models(
        &self,
        request: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderModelsResult, BackendError>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrSettings {
    pub runtime: LocalAsrRuntime,
    pub provider_id: String,
    pub active_model: String,
    pub mirror: LocalAsrMirror,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_base_dir: Option<PathBuf>,
    pub models_root_dir: PathBuf,
    pub engine_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_source: Option<FoundryRuntimeSource>,
    pub keep_loaded_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrStorageSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_base_dir: Option<PathBuf>,
    pub models_root_dir: PathBuf,
    pub is_default: bool,
    #[serde(default)]
    pub restart_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrModel {
    pub target: LocalAsrTarget,
    pub display_name: String,
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    pub installed: bool,
    pub downloaded_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrRuntimeStatus {
    pub runtime: LocalAsrRuntime,
    pub provider_id: String,
    pub available: bool,
    pub loaded: bool,
    pub active_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub keep_loaded_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_source: Option<FoundryRuntimeSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prepare_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transcribe_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_audio_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrRemoteFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrRemoteInfo {
    pub target: LocalAsrTarget,
    pub mirror: LocalAsrMirror,
    pub files: Vec<LocalAsrRemoteFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrModelCard {
    pub target: LocalAsrTarget,
    pub mirror: LocalAsrMirror,
    pub downloads: u64,
    pub likes: u64,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrTestResult {
    pub target: LocalAsrTarget,
    pub backend: String,
    pub expected_text: String,
    pub transcribed_text: String,
    pub audio_ms: u64,
    pub load_ms: u64,
    pub transcribe_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrActivationRequest {
    pub target: LocalAsrTarget,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrActivationResult {
    pub target: LocalAsrTarget,
    pub provider_id: String,
    pub generation: u64,
    pub prepared_model: String,
}

pub trait LocalAsrApi: Send + Sync {
    fn activate(
        &self,
        _request: LocalAsrActivationRequest,
    ) -> BoxFuture<'static, Result<LocalAsrActivationResult, BackendError>> {
        unsupported("local ASR activation")
    }

    fn settings(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrSettings, BackendError>>;
    fn storage_settings(&self)
        -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>>;
    fn list_models(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<Vec<LocalAsrModel>, BackendError>>;
    fn runtime_status(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrRuntimeStatus, BackendError>>;
    fn remote_info(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrRemoteInfo, BackendError>>;
    fn model_card(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrModelCard, BackendError>>;
    fn set_models_base_dir(
        &self,
        path: Option<PathBuf>,
    ) -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>>;
    fn set_active_model(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn set_mirror(&self, mirror: LocalAsrMirror) -> BoxFuture<'static, Result<(), BackendError>>;
    fn set_language_hint(
        &self,
        runtime: LocalAsrRuntime,
        language_hint: String,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn set_foundry_runtime_source(
        &self,
        source: FoundryRuntimeSource,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn set_keep_loaded_secs(
        &self,
        runtime: LocalAsrRuntime,
        seconds: u32,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn start_download(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn cancel_download(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn prepare(&self, target: LocalAsrTarget) -> BoxFuture<'static, Result<String, BackendError>>;
    fn cancel_prepare(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn release(&self, runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>>;
    fn preload(&self, runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>>;
    fn delete_model(&self, target: LocalAsrTarget) -> BoxFuture<'static, Result<(), BackendError>>;
    fn model_dir(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<PathBuf, BackendError>>;
    fn test_model(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<LocalAsrTestResult, BackendError>>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionPhase {
    #[default]
    Idle,
    Capturing,
    Preview,
    Applying,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCapture {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
}

/// Platform seam for capturing and replacing the current selection.
///
/// The core owns the session and preview state. Implementations may retain an
/// opaque platform target internally, keyed by `SessionId`, but must never
/// expose native handles through this Interface.
pub trait SelectionRuntimeAdapter: Send + Sync {
    fn capture(
        &self,
        session_id: SessionId,
        supplied_text: Option<String>,
    ) -> BoxFuture<'static, Result<SelectionCapture, BackendError>>;
    fn apply(
        &self,
        session_id: SessionId,
        source_text: String,
        replacement_text: String,
    ) -> BoxFuture<'static, Result<crate::ports::InsertOutcome, BackendError>>;
    fn prepare_preview(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
    fn revert(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<crate::ports::InsertOutcome, BackendError>>;
    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub phase: SelectionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_outcome: Option<crate::ports::InsertOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert_outcome: Option<crate::ports::InsertOutcome>,
}

impl Default for SelectionSnapshot {
    fn default() -> Self {
        Self {
            phase: SelectionPhase::Idle,
            session_id: None,
            source_text: None,
            preview_text: None,
            instruction: None,
            insert_outcome: None,
            revert_outcome: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPolishRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    pub mode: PolishMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

pub trait SelectionApi: Send + Sync {
    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionSnapshot, BackendError>>;
    fn begin_polish(
        &self,
        request: SelectionPolishRequest,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>>;
    fn confirm(
        &self,
        session_id: SessionId,
        text: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn cancel(&self, session_id: Option<SessionId>)
        -> BoxFuture<'static, Result<(), BackendError>>;
    fn revert(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionVoicePhase {
    #[default]
    Idle,
    Recording,
    Processing,
    AwaitingIntent,
    Preview,
    Applying,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoicePreview {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<SessionId>,
    pub source_text: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub can_revert: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoiceIntentPrompt {
    pub session_id: SessionId,
    pub instruction: String,
    pub source_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoiceSnapshot {
    pub phase: SelectionVoicePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_polished: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_prompt: Option<SelectionVoiceIntentPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<SelectionVoicePreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_outcome: Option<SelectionVoiceApplyOutcome>,
}

impl Default for SelectionVoiceSnapshot {
    fn default() -> Self {
        Self {
            phase: SelectionVoicePhase::Idle,
            session_id: None,
            source_text: None,
            instruction_raw: None,
            instruction_polished: None,
            intent_prompt: None,
            preview: None,
            apply_outcome: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoiceInstructionRequest {
    pub session_id: SessionId,
    pub raw: String,
    pub polished: String,
    pub intent_mode: crate::types::SelectionVoiceIntentMode,
    pub manual_intent: crate::types::SelectionVoiceManualIntent,
    #[serde(default)]
    pub question_keywords: Vec<String>,
    /// Optional raw classifier response produced by a host-provided model
    /// adapter. Core remains responsible for parsing it and for the heuristic
    /// fallback when the response is absent or malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_classification: Option<String>,
}

/// Host-independent request for creating or revising an edit preview from a
/// QA conversation. The host supplies only the captured selection metadata and
/// the user's instruction; correction, prompting, model routing, EditPlan
/// parsing and preview ownership remain in core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionVoiceEditRequest {
    pub owner_session_id: SessionId,
    pub capture: SelectionCapture,
    pub instruction: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionVoiceEditPreviewResult {
    pub preview: SelectionVoicePreview,
    pub replaced_existing: bool,
}

impl SelectionVoiceEditPreviewResult {
    /// Stable assistant-message projection used by every QA host.
    pub fn answer_text(&self) -> String {
        let summary = self
            .preview
            .summary
            .as_deref()
            .map(|summary| format!("（{summary}）\n\n"))
            .unwrap_or_default();
        format!("{summary}{}", self.preview.text)
    }
}

/// Core-owned delivery decision for a resolved selection-voice edit.
///
/// `OpenConversation` asks the host to present its QA surface and submit the
/// supplied instruction in edit mode. `ReadyToApply` means core has already
/// generated and validated the preview; the host only performs the opaque
/// native insertion handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionVoiceEditAction {
    OpenConversation {
        session_id: SessionId,
        selection: SelectionCapture,
        instruction: String,
    },
    ReadyToApply {
        preview: SelectionVoicePreview,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoicePreviewUpdate {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<SessionId>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectionVoiceDisposition {
    AwaitingIntent {
        prompt: SelectionVoiceIntentPrompt,
    },
    Question {
        session_id: SessionId,
        selection: SelectionCapture,
        instruction: String,
    },
    Edit {
        session_id: SessionId,
        selection: SelectionCapture,
        instruction: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionVoiceHotkeyEdge {
    Pressed { at: std::time::Instant },
    Released { at: std::time::Instant },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionVoiceHotkeyAction {
    Start,
    Finish,
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionVoiceRoute {
    AwaitingIntent { prompt: SelectionVoiceIntentPrompt },
    QuestionCompleted { session_id: SessionId },
    EditConversationOpened { session_id: SessionId },
    ReadyToApply { preview: SelectionVoicePreview },
}

impl SelectionVoiceDisposition {
    pub fn is_awaiting_intent(&self) -> bool {
        matches!(self, Self::AwaitingIntent { .. })
    }

    pub fn intent(&self) -> Option<crate::selection_voice_intent::SelectionVoiceIntent> {
        match self {
            Self::AwaitingIntent { .. } => None,
            Self::Question { .. } => {
                Some(crate::selection_voice_intent::SelectionVoiceIntent::Question)
            }
            Self::Edit { .. } => Some(crate::selection_voice_intent::SelectionVoiceIntent::Edit),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionVoiceApplyTicket {
    pub ticket_id: SessionId,
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<SessionId>,
    pub source_text: String,
    pub replacement_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionVoiceApplyOutcome {
    Inserted,
    PasteSent,
    CopiedFallback,
    Failed,
}

impl SelectionVoiceApplyOutcome {
    pub fn may_have_applied(self) -> bool {
        !matches!(self, Self::Failed)
    }
}

pub trait SelectionVoiceApi: Send + Sync {
    #[doc(hidden)]
    fn bind_qa(&self, _qa: std::sync::Weak<dyn QaApi>) {}
    /// Register the Host's capture/target cleanup before asynchronous startup.
    /// Every cancellation entry uses this same session-scoped controller.
    #[doc(hidden)]
    fn bind_recording_control(
        &self,
        _session_id: SessionId,
        _control: Arc<dyn crate::ports::RecordingControlSink>,
    ) -> Result<(), BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "selection voice recording control is unavailable",
        ))
    }
    fn dispatch_hotkey_edge(
        &self,
        _edge: SelectionVoiceHotkeyEdge,
    ) -> Result<SelectionVoiceHotkeyAction, BackendError> {
        Ok(SelectionVoiceHotkeyAction::Noop)
    }
    fn recording_fault(
        &self,
        _session_id: SessionId,
        _error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionVoiceSnapshot, BackendError>>;
    fn begin(
        &self,
        capture: SelectionCapture,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>>;
    fn mark_processing(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Correct, polish and classify one ASR transcript using a session-fixed
    /// core configuration. Hosts must not pre-process or classify the text.
    fn process_transcript(
        &self,
        session_id: SessionId,
        transcript: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>>;
    fn resolve_instruction(
        &self,
        request: SelectionVoiceInstructionRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>>;
    fn confirm_intent(
        &self,
        session_id: SessionId,
        intent: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>>;
    fn route_disposition(
        &self,
        _disposition: SelectionVoiceDisposition,
    ) -> BoxFuture<'static, Result<SelectionVoiceRoute, BackendError>> {
        unsupported("selection voice")
    }
    /// Resolve the configured edit delivery mode and, for direct replacement,
    /// generate the validated preview entirely inside core.
    fn prepare_edit(
        &self,
        session_id: SessionId,
        owner_session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditAction, BackendError>>;
    /// Create the first QA-owned preview or revise the current one. The result
    /// tells the QA host whether a one-step revert is now available.
    fn edit_preview(
        &self,
        request: SelectionVoiceEditRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditPreviewResult, BackendError>>;
    fn set_preview(
        &self,
        update: SelectionVoicePreviewUpdate,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn replace_preview(
        &self,
        owner_session_id: Option<SessionId>,
        text: String,
        summary: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn preview(
        &self,
        owner_session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<Option<SelectionVoicePreview>, BackendError>>;
    /// Revert and return the preview under one local state lock; no native work
    /// or await is involved. QA can include this mutation in its turn transaction.
    fn revert_preview(
        &self,
        owner_session_id: Option<SessionId>,
    ) -> Result<SelectionVoicePreview, BackendError>;
    /// Synchronously reserve one native apply ticket. QA uses this local
    /// operation while holding its turn guard; native work starts afterwards.
    fn begin_preview_apply(
        &self,
        owner_session_id: Option<SessionId>,
        text: String,
    ) -> Result<SelectionVoiceApplyTicket, BackendError>;
    fn finish_preview_apply(
        &self,
        ticket_id: SessionId,
        outcome: SelectionVoiceApplyOutcome,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn complete(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>>;
    fn cancel(&self, session_id: Option<SessionId>)
        -> BoxFuture<'static, Result<(), BackendError>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaPhase {
    Idle,
    Recording,
    Thinking,
    AwaitingApproval,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaSnapshot {
    pub phase: QaPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Stable owner for resources that span multiple successful turns, such as
    /// an edit preview. `session_id` remains a per-turn generation token so a
    /// late result from the previous turn can never be accepted by the next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<SessionId>,
    pub messages: Vec<QaMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_preview: Option<String>,
    pub edit_instruction_mode: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edit_apply_available: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edit_revert_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_approval_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for QaSnapshot {
    fn default() -> Self {
        Self {
            phase: QaPhase::Idle,
            session_id: None,
            conversation_id: None,
            messages: Vec::new(),
            selection_preview: None,
            edit_instruction_mode: false,
            edit_apply_available: false,
            edit_revert_available: false,
            pending_approval_token: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaInput {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_source_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaTurnRequest {
    /// Per-turn generation token used by runtime resource registries and
    /// progress sinks.
    pub session_id: SessionId,
    /// Stable owner shared by successful follow-up turns in the same panel.
    pub conversation_id: SessionId,
    pub input: QaInput,
    pub messages: Vec<QaMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaTurnResult {
    pub answer: String,
}

/// Host-owned metadata collected while completing one QA turn. The core uses
/// it only to apply the shared history policy; provider credentials, raw audio
/// and native handles must never cross this boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QaRuntimeCompletion {
    pub duration_ms: Option<u64>,
    pub front_app: Option<String>,
    /// `Some("")` is meaningful for multimodal voice turns whose question is
    /// present only in the audio payload.
    pub raw_transcript_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QaProgress {
    RecordingLevel(f32),
    SelectionCaptured(Option<String>),
    AnswerDelta(String),
    AwaitingApproval { token: String },
}

pub trait QaProgressSink: Send + Sync {
    fn publish(&self, session_id: SessionId, progress: QaProgress) -> Result<(), BackendError>;
}

/// Platform/provider seam for QA. The core owns the session and message log;
/// implementations only capture host context, operate recording resources and
/// execute the provider request described by [`QaTurnRequest`].
pub trait QaRuntimeAdapter: Send + Sync {
    fn prepare_text(
        &self,
        session_id: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>>;
    /// Prepare a Selection Voice edit turn whose text and opaque target were
    /// already captured before the QA window took focus. Hosts must not
    /// recapture the current selection in this path.
    fn prepare_selection_edit(
        &self,
        _session_id: SessionId,
        _selection_voice_session_id: SessionId,
        _capture: SelectionCapture,
        _instruction: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        unsupported("QA selection edit")
    }
    fn start_recording(
        &self,
        session_id: SessionId,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn finish_recording(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>>;
    fn answer(
        &self,
        request: QaTurnRequest,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>>;
    /// Attach the opaque native selection target captured for a QA turn to a
    /// Core-owned selection-voice preview. This is a host effect only.
    fn bind_selection_voice_target(
        &self,
        _qa_session_id: SessionId,
        _selection_voice_session_id: SessionId,
    ) -> Result<(), BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "selection voice target binding is unavailable",
        ))
    }
    /// Release a successfully completed runtime session and return the small
    /// amount of host metadata needed by the core history policy.
    fn complete(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaRuntimeCompletion, BackendError>> {
        Box::pin(async { Ok(QaRuntimeCompletion::default()) })
    }
    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>>;
}

pub trait QaApi: Send + Sync {
    #[doc(hidden)]
    fn bind_event_publisher(&self, _publisher: crate::events::BackendEventPublisher) {}
    /// Show the QA surface without implicitly starting a recording or creating
    /// a turn. Window and focus details remain a host concern.
    fn show(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn snapshot(&self) -> BoxFuture<'static, Result<QaSnapshot, BackendError>>;
    fn toggle_recording(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Stop only this recording generation. Deferred device/silence callbacks
    /// must never use the UI toggle: it could start or stop a newer turn.
    fn stop_recording(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA recording stop")
    }
    fn recording_fault(
        &self,
        _session_id: SessionId,
        _error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn submit_text(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Open a QA edit turn from an already captured Selection Voice session.
    /// This preserves the original text/target across the QA focus change.
    fn submit_selection_edit(
        &self,
        _selection_voice_session_id: SessionId,
        _capture: SelectionCapture,
        _instruction: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA selection edit")
    }
    fn set_edit_instruction_mode(
        &self,
        enabled: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Revert the edit preview and its displayed answer atomically for this
    /// completed turn. Reject stale requests with Cancelled even within one conversation.
    fn revert_edit_preview(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    /// Validate the completed turn and reserve its preview together, so a
    /// delayed confirmation cannot apply old text to a newer turn's preview.
    fn begin_edit_preview_apply(
        &self,
        _session_id: SessionId,
        _text: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceApplyTicket, BackendError>> {
        unsupported("QA selection edit")
    }
    fn cancel(&self, session_id: Option<SessionId>)
        -> BoxFuture<'static, Result<(), BackendError>>;
    fn dismiss(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Close only this turn. Stale requests return Cancelled without clearing
    /// or hiding a newer turn, including a newly shown panel with no turn yet.
    fn dismiss_session(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputStatus {
    pub enabled: bool,
    pub running: bool,
    pub starting: bool,
    pub port: u16,
    pub urls: Vec<String>,
    pub urls_stale: bool,
    pub locale: String,
    pub connection_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputConfig {
    pub enabled: bool,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAuthResult {
    Ok,
    BadPin,
    Locked,
}

pub struct RemoteInputServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInputServerBinding {
    pub port: u16,
    pub urls: Vec<String>,
    pub urls_stale: bool,
}

/// Native transport and shared-dictation bridge. TLS, sockets, H5 assets and
/// local address enumeration stay here; lifecycle/session rules stay in core.
pub trait RemoteInputRuntimeAdapter: Send + Sync {
    fn load_pairing_pin(
        &self,
    ) -> BoxFuture<'static, Result<Option<crate::credentials::SecretValue>, BackendError>>;
    fn persist_pairing_pin(
        &self,
        pin: crate::credentials::SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn start_server(
        &self,
        config: RemoteInputServerConfig,
    ) -> BoxFuture<'static, Result<RemoteInputServerBinding, BackendError>>;
    fn stop_server(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>>;
    fn start_audio_session(
        &self,
        insert_text: bool,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>>;
    fn feed_audio(
        &self,
        session_id: SessionId,
        pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn stop_audio_session(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn cancel_audio_session(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
}

pub trait RemoteInputApi: Send + Sync {
    #[doc(hidden)]
    fn bind_event_publisher(&self, _publisher: crate::events::BackendEventPublisher) {}
    /// Return the current in-process state without scheduling I/O. Hosts use
    /// this snapshot from synchronous render/menu code; transport operations
    /// remain asynchronous below.
    fn status(&self) -> Result<RemoteInputStatus, BackendError>;
    fn read_pairing_pin(
        &self,
    ) -> BoxFuture<'static, Result<crate::credentials::SecretValue, BackendError>>;
    fn regenerate_pairing_pin(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn set_locale(&self, locale: String) -> BoxFuture<'static, Result<(), BackendError>>;
    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>>;
    fn configure(
        &self,
        _config: RemoteInputConfig,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn authenticate(
        &self,
        _connection_id: SessionId,
        _peer: String,
        _pin: crate::credentials::SecretValue,
    ) -> BoxFuture<'static, Result<RemoteAuthResult, BackendError>> {
        unsupported("remote input")
    }
    fn disconnect(
        &self,
        _connection_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn start_stream(
        &self,
        _connection_id: SessionId,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        unsupported("remote input")
    }
    fn feed_pcm(
        &self,
        _connection_id: SessionId,
        _session_id: SessionId,
        _sequence: u64,
        _pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn set_insert(
        &self,
        _connection_id: SessionId,
        _insert_text: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn stop_stream(
        &self,
        _connection_id: SessionId,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn cancel_stream(
        &self,
        _connection_id: SessionId,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceListItem {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub author_login: String,
    pub version: String,
    pub base_mode: String,
    pub tags: Vec<String>,
    pub like_count: i64,
    pub download_count: i64,
    pub published_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_author_login: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceDetail {
    #[serde(flatten)]
    pub summary: MarketplaceListItem,
    pub prompt: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceUploadResult {
    pub id: String,
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceLikeResult {
    pub like_count: i64,
    pub already_liked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceMyPackItem {
    #[serde(flatten)]
    pub summary: MarketplaceListItem,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAuthStatus {
    pub signed_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDeviceFlow {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_secs: u64,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OAuthPollResult {
    Authorized { login: String },
    Pending,
    SlowDown,
    Error { message: String },
}

pub trait MarketplaceApi: Send + Sync {
    fn list(
        &self,
        query: MarketplaceQuery,
    ) -> BoxFuture<'static, Result<Vec<MarketplaceListItem>, BackendError>>;
    fn detail(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<MarketplaceDetail, BackendError>>;
    fn install(&self, pack_id: String) -> BoxFuture<'static, Result<StylePack, BackendError>>;
    fn download_archive(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>>;
    fn upload(
        &self,
        pack_id: String,
        origin_pack_id: Option<String>,
    ) -> BoxFuture<'static, Result<MarketplaceUploadResult, BackendError>>;
    fn toggle_like(
        &self,
        pack_id: String,
    ) -> BoxFuture<'static, Result<MarketplaceLikeResult, BackendError>>;
    fn delete(&self, pack_id: String) -> BoxFuture<'static, Result<(), BackendError>>;
    fn my_likes(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>>;
    fn my_packs(&self) -> BoxFuture<'static, Result<Vec<MarketplaceMyPackItem>, BackendError>>;
    fn auth_status(&self) -> BoxFuture<'static, Result<MarketplaceAuthStatus, BackendError>>;
    fn start_device_flow(&self) -> BoxFuture<'static, Result<OAuthDeviceFlow, BackendError>>;
    fn poll_device_flow(
        &self,
        flow_id: String,
    ) -> BoxFuture<'static, Result<OAuthPollResult, BackendError>>;
    fn cancel_device_flow(
        &self,
        flow_id: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn logout(&self) -> BoxFuture<'static, Result<(), BackendError>>;
}

pub trait CodingAgentApi: Send + Sync {
    fn detect(
        &self,
        request: CodingAgentDetectRequest,
    ) -> BoxFuture<'static, Result<CodingAgentAvailability, BackendError>>;
    fn list_models(
        &self,
        request: CodingAgentModelsRequest,
    ) -> BoxFuture<'static, Result<Vec<String>, BackendError>>;
    fn command_risk(
        &self,
        command: String,
    ) -> BoxFuture<'static, Result<CommandRiskAssessment, BackendError>>;
    fn run_test(
        &self,
        request: CodingAgentTestRequest,
    ) -> BoxFuture<'static, Result<CodingAgentTestStatus, BackendError>>;
    fn cancel_test(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn approve(
        &self,
        token: String,
        approved: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
}

/// Request for one Less Computer turn. Provider, permission and prompt policy
/// are resolved by the Core facade; hosts provide only the opaque runtime
/// implementation that can execute this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LessComputerRunRequest {
    pub session_id: SessionId,
    pub transcript: String,
    pub provider: CodingAgentProvider,
    pub executable: Option<String>,
    pub model: Option<String>,
    pub permission_mode: CodingAgentPermissionMode,
    pub workdir: Option<PathBuf>,
    pub continue_session: bool,
    pub continuation_context: Option<String>,
    pub approved_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LessComputerRunOutcome {
    Completed { text: String, cost_usd: Option<f64> },
    Failed { message: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LessComputerRunResult {
    pub session_id: SessionId,
    pub outcome: LessComputerRunOutcome,
}

/// Instance-scoped Less Computer lifecycle and approval use-cases.
///
/// The service owns continuation and pending approval state. Hosts only render
/// the typed event and return the user's decision; they must not maintain a
/// second token registry or conversation flag.
pub trait LessComputerApi: Send + Sync {
    #[doc(hidden)]
    fn bind_event_publisher(&self, _publisher: crate::events::BackendEventPublisher) {}
    #[doc(hidden)]
    fn bind_runner(&self, _runner: Arc<crate::coding_agent::CodingAgentRunner>) {}

    /// Reserve a host-owned audio capture session before recording starts.
    ///
    /// The host still owns the recorder and native ASR resources, while Core
    /// owns the session lease used for cancellation and the subsequent Agent
    /// submit.  Reservation is synchronous and side-effect free outside the
    /// in-memory lease registry.
    fn begin_capture(&self, _session_id: SessionId) -> Result<(), BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "Less Computer capture lifecycle is not configured",
        ))
    }

    /// Return the session currently reserved for host capture or running in
    /// the Core Agent service.  This is a status query only.
    fn active_session(&self) -> Option<SessionId> {
        None
    }

    /// Return whether cancellation has been requested for a host capture
    /// session.  Adapters use this to stop native recording/ASR promptly.
    fn capture_cancelled(&self, _session_id: SessionId) -> bool {
        false
    }

    /// Release a capture reservation that never reached `submit`.  The
    /// operation is idempotent and does not cancel an already-running Agent.
    fn abort_capture(&self, _session_id: SessionId) -> Result<(), BackendError> {
        Ok(())
    }

    /// Finish a reserved capture with one typed failure terminal. The host
    /// reports the native fault; Core owns event deduplication and lease
    /// release before the host tears down its opaque recorder handles.
    fn capture_fault(
        &self,
        _session_id: SessionId,
        _error: BackendError,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("Less Computer")
    }

    /// Run one text/voice turn through the Core-owned policy and state machine.
    fn submit(
        &self,
        request: LessComputerRunRequest,
    ) -> BoxFuture<'static, Result<LessComputerRunResult, BackendError>>;

    /// Cancel the active run when `session_id` matches; `None` cancels the
    /// current run. The operation is idempotent.
    fn cancel(&self, session_id: Option<SessionId>)
        -> BoxFuture<'static, Result<(), BackendError>>;

    /// Start one turn and report whether it should continue the current
    /// conversation (`true`) or begin a fresh one (`false`).
    fn begin_turn(&self) -> bool;

    /// End the current conversation and deny every pending approval.
    fn dismiss(&self);

    /// Publish an approval request and wait for its decision or timeout.
    fn request_approval(
        &self,
        command: String,
        reason: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>>;

    /// Resolve one pending request. Unknown, expired and duplicate tokens are
    /// deliberately idempotent.
    fn approve(
        &self,
        token: String,
        approved: bool,
    ) -> BoxFuture<'static, Result<(), BackendError>>;

    /// Deny all waiters without ending the conversation.
    fn cancel_pending(&self);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub trait PlatformApi: Send + Sync {
    fn capabilities(
        &self,
    ) -> BoxFuture<'static, Result<crate::shared_types::PlatformCapabilities, BackendError>>;
    fn microphone_devices(&self)
        -> BoxFuture<'static, Result<Vec<MicrophoneDevice>, BackendError>>;
    fn microphone_permission(
        &self,
    ) -> BoxFuture<'static, Result<crate::types::PermissionSnapshot, BackendError>>;
    fn accessibility_permission(
        &self,
    ) -> BoxFuture<'static, Result<crate::types::PermissionSnapshot, BackendError>>;
    fn request_microphone_permission(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn request_accessibility_permission(&self) -> BoxFuture<'static, Result<(), BackendError>>;
    fn hotkey_status(
        &self,
    ) -> BoxFuture<'static, Result<crate::shared_types::HotkeyStatus, BackendError>>;
}

/// Explicit unsupported adapter used until a host wires a domain implementation.
/// Every call fails with a stable `Unsupported` code; no operation reports fake
/// success and no background task is started.
pub struct UnsupportedDomainServices;

impl ProviderApi for UnsupportedDomainServices {
    fn validate(
        &self,
        _: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderCheckResult, BackendError>> {
        unsupported("provider")
    }
    fn list_models(
        &self,
        _: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderModelsResult, BackendError>> {
        unsupported("provider")
    }
}

impl LocalAsrApi for UnsupportedDomainServices {
    fn settings(
        &self,
        _: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrSettings, BackendError>> {
        unsupported("local ASR")
    }
    fn storage_settings(
        &self,
    ) -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>> {
        unsupported("local ASR")
    }
    fn list_models(
        &self,
        _: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<Vec<LocalAsrModel>, BackendError>> {
        unsupported("local ASR")
    }
    fn runtime_status(
        &self,
        _: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrRuntimeStatus, BackendError>> {
        unsupported("local ASR")
    }
    fn remote_info(
        &self,
        _: LocalAsrTarget,
        _: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrRemoteInfo, BackendError>> {
        unsupported("local ASR")
    }
    fn model_card(
        &self,
        _: LocalAsrTarget,
        _: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrModelCard, BackendError>> {
        unsupported("local ASR")
    }
    fn set_models_base_dir(
        &self,
        _: Option<PathBuf>,
    ) -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>> {
        unsupported("local ASR")
    }
    fn set_active_model(&self, _: LocalAsrTarget) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn set_mirror(&self, _: LocalAsrMirror) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn set_language_hint(
        &self,
        _: LocalAsrRuntime,
        _: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn set_foundry_runtime_source(
        &self,
        _: FoundryRuntimeSource,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn set_keep_loaded_secs(
        &self,
        _: LocalAsrRuntime,
        _: u32,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn start_download(
        &self,
        _: LocalAsrTarget,
        _: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn cancel_download(&self, _: LocalAsrTarget) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn prepare(&self, _: LocalAsrTarget) -> BoxFuture<'static, Result<String, BackendError>> {
        unsupported("local ASR")
    }
    fn cancel_prepare(&self, _: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn release(&self, _: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn preload(&self, _: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn delete_model(&self, _: LocalAsrTarget) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("local ASR")
    }
    fn model_dir(&self, _: LocalAsrTarget) -> BoxFuture<'static, Result<PathBuf, BackendError>> {
        unsupported("local ASR")
    }
    fn test_model(
        &self,
        _: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<LocalAsrTestResult, BackendError>> {
        unsupported("local ASR")
    }
}

impl SelectionApi for UnsupportedDomainServices {
    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionSnapshot, BackendError>> {
        unsupported("selection")
    }
    fn begin_polish(
        &self,
        _: SelectionPolishRequest,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        unsupported("selection")
    }
    fn confirm(
        &self,
        _: SessionId,
        _: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection")
    }
    fn cancel(&self, _: Option<SessionId>) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection")
    }
    fn revert(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection")
    }
}

impl SelectionVoiceApi for UnsupportedDomainServices {
    fn snapshot(&self) -> BoxFuture<'static, Result<SelectionVoiceSnapshot, BackendError>> {
        unsupported("selection voice")
    }
    fn begin(&self, _: SelectionCapture) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        unsupported("selection voice")
    }
    fn mark_processing(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn process_transcript(
        &self,
        _: SessionId,
        _: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        unsupported("selection voice")
    }
    fn resolve_instruction(
        &self,
        _: SelectionVoiceInstructionRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        unsupported("selection voice")
    }
    fn confirm_intent(
        &self,
        _: SessionId,
        _: String,
    ) -> BoxFuture<'static, Result<SelectionVoiceDisposition, BackendError>> {
        unsupported("selection voice")
    }
    fn prepare_edit(
        &self,
        _: SessionId,
        _: Option<SessionId>,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditAction, BackendError>> {
        unsupported("selection voice")
    }
    fn edit_preview(
        &self,
        _: SelectionVoiceEditRequest,
    ) -> BoxFuture<'static, Result<SelectionVoiceEditPreviewResult, BackendError>> {
        unsupported("selection voice")
    }
    fn set_preview(
        &self,
        _: SelectionVoicePreviewUpdate,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn replace_preview(
        &self,
        _: Option<SessionId>,
        _: String,
        _: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn preview(
        &self,
        _: Option<SessionId>,
    ) -> BoxFuture<'static, Result<Option<SelectionVoicePreview>, BackendError>> {
        unsupported("selection voice")
    }
    fn revert_preview(&self, _: Option<SessionId>) -> Result<SelectionVoicePreview, BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "selection voice service is not configured",
        ))
    }
    fn begin_preview_apply(
        &self,
        _: Option<SessionId>,
        _: String,
    ) -> Result<SelectionVoiceApplyTicket, BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "selection voice service is not configured",
        ))
    }
    fn finish_preview_apply(
        &self,
        _: SessionId,
        _: SelectionVoiceApplyOutcome,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn complete(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
    fn cancel(&self, _: Option<SessionId>) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("selection voice")
    }
}

impl QaApi for UnsupportedDomainServices {
    fn show(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn snapshot(&self) -> BoxFuture<'static, Result<QaSnapshot, BackendError>> {
        unsupported("QA")
    }
    fn toggle_recording(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn submit_text(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn set_edit_instruction_mode(&self, _: bool) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn cancel(&self, _: Option<SessionId>) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
    fn dismiss(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("QA")
    }
}

impl RemoteInputApi for UnsupportedDomainServices {
    fn status(&self) -> Result<RemoteInputStatus, BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "remote input service is not configured",
        ))
    }
    fn read_pairing_pin(
        &self,
    ) -> BoxFuture<'static, Result<crate::credentials::SecretValue, BackendError>> {
        unsupported("remote input")
    }
    fn regenerate_pairing_pin(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn set_locale(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("remote input")
    }
    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        unsupported("remote input")
    }
}

impl MarketplaceApi for UnsupportedDomainServices {
    fn list(
        &self,
        _: MarketplaceQuery,
    ) -> BoxFuture<'static, Result<Vec<MarketplaceListItem>, BackendError>> {
        unsupported("marketplace")
    }
    fn detail(&self, _: String) -> BoxFuture<'static, Result<MarketplaceDetail, BackendError>> {
        unsupported("marketplace")
    }
    fn install(&self, _: String) -> BoxFuture<'static, Result<StylePack, BackendError>> {
        unsupported("marketplace")
    }
    fn download_archive(&self, _: String) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        unsupported("marketplace")
    }
    fn upload(
        &self,
        _: String,
        _: Option<String>,
    ) -> BoxFuture<'static, Result<MarketplaceUploadResult, BackendError>> {
        unsupported("marketplace")
    }
    fn toggle_like(
        &self,
        _: String,
    ) -> BoxFuture<'static, Result<MarketplaceLikeResult, BackendError>> {
        unsupported("marketplace")
    }
    fn delete(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("marketplace")
    }
    fn my_likes(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        unsupported("marketplace")
    }
    fn my_packs(&self) -> BoxFuture<'static, Result<Vec<MarketplaceMyPackItem>, BackendError>> {
        unsupported("marketplace")
    }
    fn auth_status(&self) -> BoxFuture<'static, Result<MarketplaceAuthStatus, BackendError>> {
        unsupported("marketplace")
    }
    fn start_device_flow(&self) -> BoxFuture<'static, Result<OAuthDeviceFlow, BackendError>> {
        unsupported("marketplace")
    }
    fn poll_device_flow(
        &self,
        _: String,
    ) -> BoxFuture<'static, Result<OAuthPollResult, BackendError>> {
        unsupported("marketplace")
    }
    fn cancel_device_flow(
        &self,
        _: Option<String>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("marketplace")
    }
    fn logout(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("marketplace")
    }
}

impl CodingAgentApi for UnsupportedDomainServices {
    fn detect(
        &self,
        _: CodingAgentDetectRequest,
    ) -> BoxFuture<'static, Result<CodingAgentAvailability, BackendError>> {
        unsupported("coding agent")
    }
    fn list_models(
        &self,
        _: CodingAgentModelsRequest,
    ) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        unsupported("coding agent")
    }
    fn command_risk(
        &self,
        _: String,
    ) -> BoxFuture<'static, Result<CommandRiskAssessment, BackendError>> {
        unsupported("coding agent")
    }
    fn run_test(
        &self,
        _: CodingAgentTestRequest,
    ) -> BoxFuture<'static, Result<CodingAgentTestStatus, BackendError>> {
        unsupported("coding agent")
    }
    fn cancel_test(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("coding agent")
    }
    fn approve(&self, _: String, _: bool) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("coding agent")
    }
}

impl LessComputerApi for UnsupportedDomainServices {
    fn submit(
        &self,
        _request: LessComputerRunRequest,
    ) -> BoxFuture<'static, Result<LessComputerRunResult, BackendError>> {
        unsupported("Less Computer")
    }

    fn cancel(
        &self,
        _session_id: Option<SessionId>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("Less Computer")
    }

    fn begin_turn(&self) -> bool {
        false
    }

    fn dismiss(&self) {}

    fn request_approval(
        &self,
        _: String,
        _: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>> {
        unsupported("Less Computer")
    }

    fn approve(&self, _: String, _: bool) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("Less Computer")
    }

    fn cancel_pending(&self) {}
}

impl PlatformApi for UnsupportedDomainServices {
    fn capabilities(
        &self,
    ) -> BoxFuture<'static, Result<crate::shared_types::PlatformCapabilities, BackendError>> {
        unsupported("platform")
    }
    fn microphone_devices(
        &self,
    ) -> BoxFuture<'static, Result<Vec<MicrophoneDevice>, BackendError>> {
        unsupported("platform")
    }
    fn microphone_permission(
        &self,
    ) -> BoxFuture<'static, Result<crate::types::PermissionSnapshot, BackendError>> {
        unsupported("platform")
    }
    fn accessibility_permission(
        &self,
    ) -> BoxFuture<'static, Result<crate::types::PermissionSnapshot, BackendError>> {
        unsupported("platform")
    }
    fn request_microphone_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("platform")
    }
    fn request_accessibility_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("platform")
    }
    fn hotkey_status(
        &self,
    ) -> BoxFuture<'static, Result<crate::shared_types::HotkeyStatus, BackendError>> {
        unsupported("platform")
    }
}

#[derive(Clone)]
pub struct BackendServices {
    /// Shared Core model store. Host adapters may leave it unset when model
    /// downloads are unavailable; they must then return `Unsupported`.
    pub model_store: Option<Arc<crate::model_store::ModelStore>>,
    pub auxiliary: Arc<dyn crate::auxiliary::AuxiliaryApi>,
    pub provider: Arc<dyn ProviderApi>,
    pub local_asr: Arc<dyn LocalAsrApi>,
    pub selection: Arc<dyn SelectionApi>,
    pub selection_voice: Arc<dyn SelectionVoiceApi>,
    pub qa: Arc<dyn QaApi>,
    pub remote_input: Arc<dyn RemoteInputApi>,
    pub marketplace: Arc<dyn MarketplaceApi>,
    pub coding_agent: Arc<dyn CodingAgentApi>,
    pub less_computer: Arc<dyn LessComputerApi>,
    pub platform: Arc<dyn PlatformApi>,
    pub host_context: Arc<dyn crate::ports::HostContextAdapter>,
    pub edit_observation: Arc<dyn crate::ports::EditObservationAdapter>,
    coding_agent_process: Option<Arc<dyn crate::coding_agent::CodingAgentProcessAdapter>>,
    auxiliary_polisher: Option<Arc<dyn crate::ports::TextPolisher>>,
    auxiliary_transcription: Option<Arc<dyn crate::ports::TranscriptionEngine>>,
    pub(crate) voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
}

impl BackendServices {
    pub fn unsupported() -> Self {
        let voice_sessions = Arc::new(crate::voice_session::VoiceSessionGate::default());
        Self {
            model_store: None,
            auxiliary: Arc::new(crate::auxiliary::UnsupportedAuxiliaryApi),
            provider: Arc::new(UnsupportedDomainServices),
            local_asr: Arc::new(UnsupportedDomainServices),
            selection: Arc::new(UnsupportedDomainServices),
            selection_voice: Arc::new(UnsupportedDomainServices),
            qa: Arc::new(UnsupportedDomainServices),
            remote_input: Arc::new(UnsupportedDomainServices),
            marketplace: Arc::new(UnsupportedDomainServices),
            coding_agent: Arc::new(UnsupportedDomainServices),
            less_computer: Arc::new(
                crate::less_computer::LessComputerService::with_voice_sessions(Arc::clone(
                    &voice_sessions,
                )),
            ),
            platform: Arc::new(UnsupportedDomainServices),
            host_context: Arc::new(crate::ports::NoopHostContextAdapter),
            edit_observation: Arc::new(crate::ports::NoopEditObservationAdapter),
            coding_agent_process: None,
            auxiliary_polisher: None,
            auxiliary_transcription: None,
            voice_sessions,
        }
    }

    pub fn configure_model_store(&mut self, store: Arc<crate::model_store::ModelStore>) {
        self.model_store = Some(store);
    }

    pub fn configure_coding_agent_process(
        &mut self,
        process: Arc<dyn crate::coding_agent::CodingAgentProcessAdapter>,
    ) {
        self.coding_agent_process = Some(process);
    }

    pub(crate) fn take_coding_agent_process(
        &mut self,
    ) -> Option<Arc<dyn crate::coding_agent::CodingAgentProcessAdapter>> {
        self.coding_agent_process.take()
    }

    /// Configure host-owned provider adapters used by shared auxiliary
    /// use-cases. UI callers use [`Self::auxiliary`], never these adapters.
    #[doc(hidden)]
    pub fn configure_auxiliary_runtime(
        &mut self,
        polisher: Arc<dyn crate::ports::TextPolisher>,
        transcription: Arc<dyn crate::ports::TranscriptionEngine>,
    ) {
        self.auxiliary_polisher = Some(polisher);
        self.auxiliary_transcription = Some(transcription);
    }

    pub(crate) fn take_auxiliary_runtime(
        &mut self,
    ) -> Option<(
        Arc<dyn crate::ports::TextPolisher>,
        Arc<dyn crate::ports::TranscriptionEngine>,
    )> {
        match (
            self.auxiliary_polisher.take(),
            self.auxiliary_transcription.take(),
        ) {
            (Some(polisher), Some(transcription)) => Some((polisher, transcription)),
            (None, None) => None,
            _ => unreachable!("auxiliary runtime adapters are configured atomically"),
        }
    }
}

impl Default for BackendServices {
    fn default() -> Self {
        Self::unsupported()
    }
}

impl std::fmt::Debug for BackendServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackendServices")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_domains_fail_with_a_stable_code() {
        let services = BackendServices::unsupported();
        let error = services
            .local_asr
            .list_models(LocalAsrRuntime::Generic)
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Unsupported);
        assert!(!error.retryable);
    }

    #[test]
    fn remote_status_and_events_never_need_a_pairing_pin() {
        let status = RemoteInputStatus {
            enabled: true,
            running: true,
            starting: false,
            port: 18989,
            urls: vec!["https://192.168.1.2:18989".into()],
            urls_stale: false,
            locale: "zh-CN".into(),
            connection_count: 1,
            active_session_id: Some(SessionId::new()),
        };
        let serialized = serde_json::to_string(&status).unwrap();
        assert!(!serialized.contains("pin"));
        assert!(!serialized.contains("pairing"));
    }
}
