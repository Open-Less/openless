mod cloud_note;
use futures_util::future::BoxFuture;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::activity::ActivityDay;
use crate::coding_agent::{
    normalize_coding_agent_executable, normalize_coding_agent_workdir,
    normalize_less_computer_permission_mode, resolve_coding_agent_model, CodingAgentProvider,
};
use crate::config::{BackendConfig, BackendDependencies, Clock, SystemClock, TaskSpawner};
use crate::correction::apply_correction_rules;
use crate::credentials::{
    ChannelKind, ChannelMutation, ChannelMutationResult, ChannelSummary, CredentialKey,
    CredentialStore, ProviderSlot, SecretValue,
};
use crate::dictation_context::{
    DictationAudioSource, DictationContext, DictationOutputTarget, DictationProviderInvocations,
    DictationStartOptions, DictationStopOptions,
};
use crate::domains::{LessComputerRunRequest, LessComputerRunResult};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{
    BackendEventKind, BackendEventPublisher, EventBus, EventReplay, EventSubscription,
};
use crate::ports::{
    ActiveRecording, AudioConsumer, CaptureStartRequest, CapturedPcm, EditObservationSink,
    EngineFailureStage, EngineProgress, EngineProgressSink, EngineStage, HostAction,
    HostContextAdapter, InsertOutcome, PendingAudioCapture, TextInserter, TextInsertionSession,
    TextStreamChunk, TextStreamSink, TranscriptionSession,
};
use crate::shared_types::{
    CredentialsStatus, PendingCorrection, UserPreferences, LEARNED_VOCAB_NOTE,
    MAX_PENDING_CORRECTIONS,
};
use crate::style_pack_store::sync_style_pack_preferences;
use crate::style_packs::StylePack;
use crate::types::{
    CorrectionRule, DictationPhase, DictationResult, DictationSession, DictationStateSnapshot,
    DictionaryEntry, HistoryChange, HistoryInsertStatus, HistorySource, PreferencesChange,
    SessionId, StylePackChange, TranscriptAccumulator, VocabPresetStore, VocabularyChange,
};
use crate::vocabulary::DictionaryStore;
use crate::{ActivityStore, CorrectionRuleStore, HistoryStore, PreferencesStore, StylePackStore};

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendSnapshot {
    pub running: bool,
    pub dictation: DictationStateSnapshot,
    #[serde(default)]
    pub vocabulary_revision: u64,
    #[serde(default)]
    pub history_revision: u64,
    #[serde(default)]
    pub style_pack_revision: u64,
    #[serde(default)]
    pub preferences_revision: u64,
    #[serde(default)]
    pub credentials: CredentialsStatus,
}

/// Session-bound translation picker. Working languages come from the frozen recording context.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationTranslationTarget {
    pub session_id: SessionId,
    pub target_language: String,
    pub working_languages: Vec<String>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSnapshot {
    pub contract_version: String,
    pub backend: BackendSnapshot,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CliDispatchOutcome {
    DictationStarted(SessionId),
    DictationCompleted(DictationResult),
    QaToggled,
    DictationCancelled,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationHotkeyEdge {
    Pressed {
        press_id: u64,
        at: std::time::Instant,
    },
    Released {
        press_id: u64,
        at: std::time::Instant,
    },
    Combined {
        press_id: u64,
        at: std::time::Instant,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessComputerHotkeyAction {
    Start,
    Finish,
    Cancel,
    Noop,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DictationHotkeyDispatchOptions {
    pub start: DictationStartOptions,
    pub stop: DictationStopOptions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DictationContextPurpose {
    Dictation,
    /// Selection Voice + Less Computer: respect `effective_pipeline_mode`, but
    /// skip resolving an unused LLM at capture time (intent/EditPlan/Agent run
    /// later). Replaces the old `AsrOnly` force-Traditional gate (#1119 B).
    AuxiliaryVoice,
    QaText,
    QaVoice,
}

struct DictationReservation {
    session_id: SessionId,
    resources: Arc<crate::voice_session::VoiceResourceHold>,
    inserter: Arc<dyn TextInserter>,
    host_context: Arc<dyn HostContextAdapter>,
    capture_generation: Option<u64>,
    pending_capture: Option<Box<dyn PendingAudioCapture>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PendingAudioCaptureKey {
    Hotkey(u64),
    VoiceActivation(u64),
}

/// Removes a provisional capture if the async hotkey dispatch is aborted before
/// it transfers ownership to a session reservation.
struct PendingAudioCaptureAbortGuard {
    captures: Arc<Mutex<HashMap<PendingAudioCaptureKey, Box<dyn PendingAudioCapture>>>>,
    task_spawner: Arc<dyn TaskSpawner>,
    key: PendingAudioCaptureKey,
}

impl Drop for PendingAudioCaptureAbortGuard {
    fn drop(&mut self) {
        let capture = self
            .captures
            .lock()
            .expect("pending audio capture lock poisoned")
            .remove(&self.key);
        let Some(capture) = capture else {
            return;
        };
        self.task_spawner.spawn(Box::pin(async move {
            let _ = capture.discard().await;
        }));
    }
}

/// Core-owned audio-to-Agent session shared by native hosts.
///
/// Hosts feed only canonical PCM and observe the existing `TranscriptDelta` /
/// `LessComputerEvent` stream. Provider/model selection, cancellation and Agent
/// submission stay inside Core so Tauri and Linux cannot drift apart.
pub struct LessComputerVoiceSession {
    session_id: SessionId,
    control: Arc<VoiceCaptureControl>,
    controls: Arc<Mutex<HashMap<SessionId, Arc<VoiceCaptureControl>>>>,
    less_computer: Arc<dyn crate::domains::LessComputerApi>,
    request: crate::domains::LessComputerRunRequest,
    partials: Arc<VoiceTranscriptSink>,
    archive_successful_recording: bool,
    mode: crate::events::LessComputerVoiceMode,
    feedback: Arc<LessVoiceFeedback>,
}

/// Start options for a Core-owned Less Computer voice capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LessComputerVoiceOptions {
    pub mode: crate::events::LessComputerVoiceMode,
    /// Hotkey captures surface start failures in the conversation stream. A
    /// panel request receives the error from its command and shows it inline.
    pub publish_start_error: bool,
}

impl Default for LessComputerVoiceOptions {
    fn default() -> Self {
        Self {
            mode: crate::events::LessComputerVoiceMode::Submit,
            publish_start_error: true,
        }
    }
}

/// How a finished Less Computer capture was delivered.
#[derive(Debug, Clone, PartialEq)]
pub enum LessComputerVoiceFinish {
    /// The transcript became an Agent turn (hotkey / voice-mode semantics).
    Submitted(crate::domains::LessComputerRunResult),
    /// Dictation only: the transcript was published for the host composer.
    /// An empty transcript means nothing was recognized.
    Dictated { transcript: String },
}

pub struct VoiceTranscriptionSession {
    session_id: SessionId,
    /// Traditional ASR session. Absent when multimodal Omni will produce text.
    transcription: Option<Arc<dyn TranscriptionSession>>,
    /// Multimodal PCM buffer filled by `start_audio_capture` (or host feed).
    pcm: Option<Arc<CapturedPcm>>,
    /// Credentials + context + prompt for Omni finish. Present with `pcm`.
    omni: Option<AuxiliaryOmniFinish>,
    recording: Mutex<Option<Box<dyn crate::ports::ActiveRecording>>>,
    partials: Arc<VoiceTranscriptSink>,
    lifecycle: Arc<VoiceCaptureLifecycle>,
    task_spawner: Arc<dyn TaskSpawner>,
}

/// Shared Omni finish inputs for Selection Voice / Less Computer multimodal.
#[derive(Clone)]
struct AuxiliaryOmniFinish {
    credentials: Arc<dyn CredentialStore>,
    context: Arc<DictationContext>,
    system_prompt: String,
}

pub struct QaVoiceCaptureSession {
    context: Arc<DictationContext>,
    recording: Mutex<Option<Box<dyn ActiveRecording>>>,
    transcription: Option<Arc<dyn TranscriptionSession>>,
    pcm: Option<Arc<CapturedPcm>>,
    recording_progress: Arc<QaRecordingProgress>,
    lifecycle: Arc<VoiceCaptureLifecycle>,
    task_spawner: Arc<dyn TaskSpawner>,
}

#[derive(Default, PartialEq, Eq)]
enum VoiceCapturePhase {
    #[default]
    Recording,
    Finishing,
    Completed,
    Cancelled,
}

/// Stopping the microphone begins provider finalization; it does not close
/// the cancellation window. Both voice entry points retain this shared state
/// until the provider result is committed or cancellation wins.
#[derive(Default)]
struct VoiceCaptureLifecycle {
    phase: Mutex<VoiceCapturePhase>,
    resources: Mutex<Option<Arc<crate::voice_session::VoiceResourceHold>>>,
}

impl VoiceCaptureLifecycle {
    fn with_resources(resources: Arc<crate::voice_session::VoiceResourceHold>) -> Self {
        Self {
            phase: Mutex::default(),
            resources: Mutex::new(Some(resources)),
        }
    }

    fn resources(&self) -> Option<Arc<crate::voice_session::VoiceResourceHold>> {
        self.resources
            .lock()
            .expect("voice resource lock poisoned")
            .clone()
    }

    fn release_resources(&self) {
        self.resources
            .lock()
            .expect("voice resource lock poisoned")
            .take();
    }

    fn begin_finish(&self) -> Result<(), BackendError> {
        let mut phase = self
            .phase
            .lock()
            .expect("voice capture lifecycle lock poisoned");
        match *phase {
            VoiceCapturePhase::Recording => {
                *phase = VoiceCapturePhase::Finishing;
                Ok(())
            }
            VoiceCapturePhase::Cancelled => Err(Self::cancelled_error()),
            _ => Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "voice capture is already finishing or completed",
            )),
        }
    }

    fn is_cancelled(&self) -> bool {
        *self
            .phase
            .lock()
            .expect("voice capture lifecycle lock poisoned")
            == VoiceCapturePhase::Cancelled
    }

    /// The winner alone owns provider cancellation. Recorder ownership is
    /// independently transferred with take(), so concurrent finish/cancel can
    /// never stop the native recorder or discard its archive twice.
    fn claim_cancel(&self) -> bool {
        let mut phase = self
            .phase
            .lock()
            .expect("voice capture lifecycle lock poisoned");
        if matches!(
            *phase,
            VoiceCapturePhase::Completed | VoiceCapturePhase::Cancelled
        ) {
            return false;
        }
        *phase = VoiceCapturePhase::Cancelled;
        true
    }

    fn settle<T>(&self, result: Result<T, BackendError>) -> (Result<T, BackendError>, bool) {
        let mut phase = self
            .phase
            .lock()
            .expect("voice capture lifecycle lock poisoned");
        if *phase == VoiceCapturePhase::Cancelled {
            return (Err(Self::cancelled_error()), false);
        }
        let cancel_provider = result.is_err();
        *phase = if cancel_provider {
            VoiceCapturePhase::Cancelled
        } else {
            VoiceCapturePhase::Completed
        };
        (result, cancel_provider)
    }

    fn cancelled_error() -> BackendError {
        BackendError::new(BackendErrorCode::Cancelled, "voice capture was cancelled")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaVoiceCaptureResult {
    pub transcript: Option<String>,
    pub audio_wav: Option<Vec<u8>>,
    pub duration_ms: u64,
}

/// Core-side recording observer for Selection Voice.
///
/// The audio adapter reports only measurements and typed faults. This observer
/// owns the silence state machine and asks the host to close its opaque capture
/// only after Core has chosen the product-level Stop/Cancel outcome.
struct SelectionVoiceRecordingProgress {
    session_id: SessionId,
    selection_voice: Arc<dyn crate::domains::SelectionVoiceApi>,
    control: Arc<dyn crate::ports::RecordingControlSink>,
    events: BackendEventPublisher,
    task_spawner: Arc<dyn TaskSpawner>,
    started_at: std::time::Instant,
    silence: Mutex<Option<crate::silence_auto_stop::SilenceAutoStop>>,
}

/// Voice entries without a product-specific level surface intentionally drop
/// meter updates; their capture/session state is still owned by Core.
#[cfg(test)]
struct VoiceRecordingProgress;

#[cfg(test)]
impl crate::ports::RecordingProgressSink for VoiceRecordingProgress {
    fn publish_level(&self, _elapsed_ms: u64, _level: f32) -> Result<(), BackendError> {
        Ok(())
    }
}

/// Core policy wrapper for Less Computer capture. As with Selection Voice,
/// Stop/Cancel are decisions; closing the platform-owned capture is a narrow
/// host effect requested through `RecordingControlSink`.
struct LessComputerRecordingProgress {
    session_id: SessionId,
    less_computer: Arc<dyn crate::domains::LessComputerApi>,
    control: Arc<dyn crate::ports::RecordingControlSink>,
    task_spawner: Arc<dyn TaskSpawner>,
    started_at: std::time::Instant,
    silence: Mutex<Option<crate::silence_auto_stop::SilenceAutoStop>>,
    feedback: Arc<LessVoiceFeedback>,
}

impl crate::ports::RecordingProgressSink for LessComputerRecordingProgress {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        if self.less_computer.capture_cancelled(self.session_id) {
            return Ok(());
        }
        self.feedback.level(elapsed_ms, level);
        let decision = self
            .silence
            .lock()
            .expect("Less Computer silence lock poisoned")
            .as_mut()
            .and_then(|detector| {
                let now = self
                    .started_at
                    .checked_add(std::time::Duration::from_millis(elapsed_ms))
                    .unwrap_or_else(std::time::Instant::now);
                detector.on_level(level, now)
            });
        let Some(decision) = decision else {
            return Ok(());
        };

        let session_id = self.session_id;
        let less_computer = Arc::clone(&self.less_computer);
        let control = Arc::clone(&self.control);
        self.task_spawner.spawn(Box::pin(async move {
            if less_computer.capture_cancelled(session_id) {
                return;
            }
            let action = match decision {
                crate::silence_auto_stop::SilenceDecision::Stop => {
                    crate::events::RecordingControlAction::Stop
                }
                crate::silence_auto_stop::SilenceDecision::Cancel => {
                    if let Err(error) = less_computer.cancel(Some(session_id)).await {
                        log::warn!("failed to cancel silent Less Computer capture: {error}");
                    }
                    crate::events::RecordingControlAction::Cancel
                }
            };
            if let Err(error) = control.request(session_id, action) {
                log::warn!("failed to apply Less Computer recording directive: {error}");
            }
        }));
        Ok(())
    }

    fn publish(&self, event: crate::ports::RecordingEvent) -> Result<(), BackendError> {
        match event {
            crate::ports::RecordingEvent::Level { elapsed_ms, level } => {
                self.publish_level(elapsed_ms, level)
            }
            crate::ports::RecordingEvent::LimitReached => {
                let session_id = self.session_id;
                let less_computer = Arc::clone(&self.less_computer);
                let control = Arc::clone(&self.control);
                self.task_spawner.spawn(Box::pin(async move {
                    if less_computer.capture_cancelled(session_id) {
                        return;
                    }
                    if let Err(error) =
                        control.request(session_id, crate::events::RecordingControlAction::Stop)
                    {
                        log::warn!("failed to stop Less Computer capture at PCM limit: {error}");
                    }
                }));
                Ok(())
            }
            crate::ports::RecordingEvent::Fatal(error) => {
                let session_id = self.session_id;
                let less_computer = Arc::clone(&self.less_computer);
                let control = Arc::clone(&self.control);
                self.task_spawner.spawn(Box::pin(async move {
                    match less_computer.capture_fault(session_id, error).await {
                        Ok(()) => {
                            if let Err(error) = control
                                .request(session_id, crate::events::RecordingControlAction::Cancel)
                            {
                                log::warn!(
                                    "failed to close faulted Less Computer capture: {error}"
                                );
                            }
                        }
                        Err(error) if error.code == BackendErrorCode::Cancelled => {}
                        Err(error) => {
                            log::warn!("failed to record Less Computer capture fault: {error}")
                        }
                    }
                }));
                Ok(())
            }
        }
    }
}

/// Core policy wrapper for a QA microphone stream. Meter rendering remains a
/// Host effect delegated to `progress`; fault, silence and terminal routing do
/// not leave Core and therefore cannot drift between desktop frontends.
struct QaRecordingProgress {
    session_id: SessionId,
    qa: Arc<dyn crate::domains::QaApi>,
    progress: Arc<dyn crate::ports::RecordingProgressSink>,
    task_spawner: Arc<dyn TaskSpawner>,
    started_at: std::time::Instant,
    silence: Mutex<Option<crate::silence_auto_stop::SilenceAutoStop>>,
    terminal: Mutex<QaRecordingTerminalState>,
}

enum QaRecordingTerminal {
    Stop,
    Cancel,
    Fault(BackendError),
}

#[derive(Default)]
struct QaRecordingTerminalState {
    armed: bool,
    dispatched: bool,
    pending: Option<QaRecordingTerminal>,
}

impl QaRecordingProgress {
    fn submit_terminal(&self, terminal: QaRecordingTerminal) {
        {
            let mut state = self.terminal.lock().expect("QA terminal lock poisoned");
            if state.dispatched {
                return;
            }
            if !state.armed {
                // The recorder may fail while `start_qa_voice_capture` is
                // still returning. Keep the first terminal until the Host has
                // installed the capture handle that the Core action will use.
                state.pending.get_or_insert(terminal);
                return;
            }
            state.dispatched = true;
        }
        self.spawn_terminal(terminal);
    }

    fn arm(&self) {
        let pending = {
            let mut state = self.terminal.lock().expect("QA terminal lock poisoned");
            state.armed = true;
            let pending = state.pending.take();
            if pending.is_some() {
                state.dispatched = true;
            }
            pending
        };
        if let Some(terminal) = pending {
            self.spawn_terminal(terminal);
        }
    }

    fn spawn_terminal(&self, terminal: QaRecordingTerminal) {
        let session_id = self.session_id;
        let qa = Arc::clone(&self.qa);
        self.task_spawner.spawn(Box::pin(async move {
            let result = match terminal {
                QaRecordingTerminal::Stop => qa.stop_recording(session_id).await,
                QaRecordingTerminal::Cancel => qa.cancel(Some(session_id)).await,
                QaRecordingTerminal::Fault(error) => qa.recording_fault(session_id, error).await,
            };
            if let Err(error) = result {
                if error.code != BackendErrorCode::Cancelled {
                    log::warn!("failed to apply QA recording terminal: {error}");
                }
            }
        }));
    }
}

impl crate::ports::RecordingProgressSink for QaRecordingProgress {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        self.progress.publish_level(elapsed_ms, level)?;
        let decision = self
            .silence
            .lock()
            .expect("QA silence lock poisoned")
            .as_mut()
            .and_then(|detector| {
                // Recorder elapsed time is monotonic and testable; anchoring it
                // here also avoids wall-clock adjustments changing silence
                // semantics midway through a capture.
                let now = self
                    .started_at
                    .checked_add(std::time::Duration::from_millis(elapsed_ms))
                    .unwrap_or_else(std::time::Instant::now);
                detector.on_level(level, now)
            });
        let Some(decision) = decision else {
            return Ok(());
        };

        // `Stop` performs the ordinary finish/transcribe/answer path. A
        // ten-second no-speech result is cancellation, so it must never
        // manufacture an empty QA question.
        self.submit_terminal(match decision {
            crate::silence_auto_stop::SilenceDecision::Stop => QaRecordingTerminal::Stop,
            crate::silence_auto_stop::SilenceDecision::Cancel => QaRecordingTerminal::Cancel,
        });
        Ok(())
    }

    fn publish(&self, event: crate::ports::RecordingEvent) -> Result<(), BackendError> {
        match event {
            crate::ports::RecordingEvent::Level { elapsed_ms, level } => {
                self.publish_level(elapsed_ms, level)
            }
            crate::ports::RecordingEvent::LimitReached => {
                self.submit_terminal(QaRecordingTerminal::Stop);
                Ok(())
            }
            crate::ports::RecordingEvent::Fatal(error) => {
                self.submit_terminal(QaRecordingTerminal::Fault(error));
                Ok(())
            }
        }
    }
}

impl crate::ports::RecordingProgressSink for SelectionVoiceRecordingProgress {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        let level = if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Shared capsule (Siri / bars) needs live meter updates; silence policy
        // alone previously swallowed every level and left the waveform flat.
        self.events.publish(
            Some(self.session_id),
            BackendEventKind::SelectionVoiceLevel(crate::events::SelectionVoiceRecordingLevel {
                session_id: self.session_id.to_string(),
                level,
                elapsed_ms,
            }),
        );
        let decision = self
            .silence
            .lock()
            .expect("selection voice silence lock poisoned")
            .as_mut()
            .and_then(|detector| {
                let now = self
                    .started_at
                    .checked_add(std::time::Duration::from_millis(elapsed_ms))
                    .unwrap_or_else(std::time::Instant::now);
                detector.on_level(level, now)
            });
        let Some(decision) = decision else {
            return Ok(());
        };

        let session_id = self.session_id;
        let selection_voice = Arc::clone(&self.selection_voice);
        let control = Arc::clone(&self.control);
        self.task_spawner.spawn(Box::pin(async move {
            let action = match decision {
                crate::silence_auto_stop::SilenceDecision::Stop => {
                    crate::events::RecordingControlAction::Stop
                }
                crate::silence_auto_stop::SilenceDecision::Cancel => {
                    // No-speech is a Core terminal state. Publish it before
                    // telling the host to tear down capture so a late native
                    // callback cannot revive the same generation.
                    if let Err(error) = selection_voice.cancel(Some(session_id)).await {
                        log::warn!("failed to cancel silent selection voice session: {error}");
                    }
                    return;
                }
            };
            if let Err(error) = control.request(session_id, action) {
                log::warn!("failed to apply selection voice recording directive: {error}");
            }
        }));
        Ok(())
    }

    fn publish(&self, event: crate::ports::RecordingEvent) -> Result<(), BackendError> {
        match event {
            crate::ports::RecordingEvent::Level { elapsed_ms, level } => {
                self.publish_level(elapsed_ms, level)
            }
            crate::ports::RecordingEvent::LimitReached => {
                let session_id = self.session_id;
                let control = Arc::clone(&self.control);
                self.task_spawner.spawn(Box::pin(async move {
                    if let Err(error) =
                        control.request(session_id, crate::events::RecordingControlAction::Stop)
                    {
                        log::warn!("failed to stop selection voice at PCM limit: {error}");
                    }
                }));
                Ok(())
            }
            crate::ports::RecordingEvent::Fatal(error) => {
                let session_id = self.session_id;
                let selection_voice = Arc::clone(&self.selection_voice);
                self.task_spawner.spawn(Box::pin(async move {
                    // The service owns both the terminal state and its one
                    // registered Host cleanup callback, including cold starts.
                    match selection_voice.recording_fault(session_id, error).await {
                        Ok(()) => {}
                        Err(error) if error.code == BackendErrorCode::Cancelled => {}
                        Err(error) => {
                            log::warn!("failed to record selection voice capture fault: {error}")
                        }
                    }
                }));
                Ok(())
            }
        }
    }
}

struct VoiceTranscriptSink {
    publisher: crate::events::BackendEventPublisher,
    session_id: SessionId,
    transcript: Mutex<crate::types::TranscriptAccumulator>,
    /// Less Computer mirrors the accumulated transcript into its voice
    /// projection; other voice surfaces only publish `TranscriptDelta`.
    feedback: Option<Arc<LessVoiceFeedback>>,
}

struct VoiceCaptureControl {
    transcription: Option<Arc<dyn TranscriptionSession>>,
    pcm: Option<Arc<CapturedPcm>>,
    omni: Option<AuxiliaryOmniFinish>,
    recording: Mutex<Option<Box<dyn crate::ports::ActiveRecording>>>,
    closed: std::sync::atomic::AtomicBool,
    resources: Mutex<Option<Arc<crate::voice_session::VoiceResourceHold>>>,
    task_spawner: Arc<dyn TaskSpawner>,
    feedback: Mutex<Option<LessVoiceFeedbackGuard>>,
}

struct LessVoiceFeedback {
    publisher: BackendEventPublisher,
    session_id: SessionId,
    mode: crate::events::LessComputerVoiceMode,
    state: Mutex<LessVoiceFeedbackState>,
}

struct LessVoiceFeedbackState {
    phase: crate::events::LessComputerVoicePhase,
    elapsed_ms: u64,
    level: f32,
    transcript: String,
    outcome: Option<crate::events::LessComputerVoiceOutcome>,
}

impl LessVoiceFeedbackState {
    fn new(phase: crate::events::LessComputerVoicePhase) -> Self {
        Self {
            phase,
            elapsed_ms: 0,
            level: 0.0,
            transcript: String::new(),
            outcome: None,
        }
    }
}

impl LessVoiceFeedback {
    fn new(
        publisher: BackendEventPublisher,
        session_id: SessionId,
        mode: crate::events::LessComputerVoiceMode,
        phase: crate::events::LessComputerVoicePhase,
    ) -> Self {
        Self {
            publisher,
            session_id,
            mode,
            state: Mutex::new(LessVoiceFeedbackState::new(phase)),
        }
    }

    fn phase(&self, phase: crate::events::LessComputerVoicePhase) {
        let mut state = self.state.lock().expect("voice feedback lock poisoned");
        if state.phase == crate::events::LessComputerVoicePhase::Idle {
            return;
        }
        state.phase = phase;
        state.level = 0.0;
        // Only a delivered transcript survives into the terminal snapshot;
        // cancelled or failed captures must not leave partial text behind.
        if phase == crate::events::LessComputerVoicePhase::Idle
            && !matches!(
                state.outcome,
                Some(
                    crate::events::LessComputerVoiceOutcome::Committed
                        | crate::events::LessComputerVoiceOutcome::Submitted
                )
            )
        {
            state.transcript.clear();
        }
        self.emit(&state);
    }

    fn level(&self, elapsed_ms: u64, level: f32) {
        let mut state = self.state.lock().expect("voice feedback lock poisoned");
        if !matches!(
            state.phase,
            crate::events::LessComputerVoicePhase::Starting
                | crate::events::LessComputerVoicePhase::Recording
        ) {
            return;
        }
        // AudioRecorder reports levels only after consuming a non-empty PCM
        // frame. A native start receipt alone cannot make capture look ready.
        state.phase = crate::events::LessComputerVoicePhase::Recording;
        state.elapsed_ms = elapsed_ms;
        state.level = level.clamp(0.0, 1.0);
        self.emit(&state);
    }

    /// Mirror the accumulated live transcript while the capture is still open.
    fn transcript(&self, text: &str) {
        let mut state = self.state.lock().expect("voice feedback lock poisoned");
        if state.phase == crate::events::LessComputerVoicePhase::Idle || state.transcript == text {
            return;
        }
        state.transcript = text.to_string();
        self.emit(&state);
    }

    /// Record how the capture ends. The terminal `idle` snapshot emitted when
    /// the feedback guard drops carries it; the first recorded outcome wins.
    fn settle(&self, outcome: crate::events::LessComputerVoiceOutcome, transcript: Option<String>) {
        let mut state = self.state.lock().expect("voice feedback lock poisoned");
        if state.phase == crate::events::LessComputerVoicePhase::Idle || state.outcome.is_some() {
            return;
        }
        state.outcome = Some(outcome);
        if let Some(transcript) = transcript {
            state.transcript = transcript;
        }
    }

    fn emit(&self, state: &LessVoiceFeedbackState) {
        let idle = state.phase == crate::events::LessComputerVoicePhase::Idle;
        self.publisher.publish(
            Some(self.session_id),
            BackendEventKind::LessComputerEvent(crate::events::LessComputerEvent {
                seq: None,
                kind: crate::events::LessComputerEventKind::VoiceState {
                    session_id: self.session_id,
                    phase: state.phase,
                    level: state.level,
                    elapsed_ms: state.elapsed_ms,
                    mode: self.mode,
                    transcript: state.transcript.clone(),
                    outcome: if idle { state.outcome } else { None },
                },
            }),
        );
    }
}

struct LessVoiceFeedbackGuard(Arc<LessVoiceFeedback>);
impl Drop for LessVoiceFeedbackGuard {
    fn drop(&mut self) {
        self.0.phase(crate::events::LessComputerVoicePhase::Idle);
    }
}

/// Committed native effects outlive the caller waiting on their reply. In
/// particular, dropping an IPC future cannot release a hold while a native
/// spawn_blocking recorder stop or decoder is still running.
fn own_voice_effect<T: Send + 'static>(
    spawner: &Arc<dyn TaskSpawner>,
    effect: futures_util::future::BoxFuture<'static, Result<T, BackendError>>,
) -> futures_util::future::BoxFuture<'static, Result<T, BackendError>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    spawner.spawn(Box::pin(async move {
        let _ = sender.send(effect.await);
    }));
    Box::pin(async move {
        receiver.await.map_err(|_| {
            BackendError::new(
                BackendErrorCode::Internal,
                "voice task did not return a result",
            )
        })?
    })
}

async fn own_voice_start<T: Send + 'static>(
    spawner: &Arc<dyn TaskSpawner>,
    resources: Arc<crate::voice_session::VoiceResourceHold>,
    start: futures_util::future::BoxFuture<'static, Result<T, BackendError>>,
    cleanup: impl FnOnce(T) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>>
        + Send
        + 'static,
) -> Result<T, BackendError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let (claimed, claim) = tokio::sync::oneshot::channel();
    let slot = Arc::new(Mutex::new(None));
    let task_slot = Arc::clone(&slot);
    let cancel = resources.cancel.clone();
    spawner.spawn(Box::pin(async move {
        if resources.cancel.is_cancelled() {
            let _ = sender.send(Err(VoiceCaptureLifecycle::cancelled_error()));
            return;
        }
        let result = start.await;
        // Even an adapter which cannot interrupt initialization must close a
        // late handle. The resource hold remains live through this cleanup.
        if resources.cancel.is_cancelled() {
            if let Ok(resource) = result {
                let _ = cleanup(resource).await;
            }
            let _ = sender.send(Err(VoiceCaptureLifecycle::cancelled_error()));
        } else {
            match result {
                Ok(resource) => {
                    *task_slot
                        .lock()
                        .expect("voice startup handoff lock poisoned") = Some(resource);
                    // Keep the native handle here until the caller actually
                    // claims it. Sending a handle through oneshot directly can
                    // drop it without stop() if the reply is never consumed.
                    if sender.send(Ok(())).is_err() || claim.await.is_err() {
                        let resource = task_slot
                            .lock()
                            .expect("voice startup handoff lock poisoned")
                            .take();
                        if let Some(resource) = resource {
                            let _ = cleanup(resource).await;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                }
            }
        }
        drop(resources);
    }));
    receiver.await.map_err(|_| {
        BackendError::new(
            BackendErrorCode::Internal,
            "voice startup task did not return a result",
        )
    })??;
    if cancel.is_cancelled() {
        return Err(VoiceCaptureLifecycle::cancelled_error());
    }
    let resource = slot
        .lock()
        .expect("voice startup handoff lock poisoned")
        .take()
        .ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Internal,
                "voice startup handle is unavailable",
            )
        })?;
    let _ = claimed.send(());
    Ok(resource)
}

fn discard_voice_capture(
    capture: crate::ports::VoiceCapture,
) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
    Box::pin(async move {
        let stopped = capture.recording.stop().await;
        let cancelled = capture.transcription.cancel().await;
        stopped.and(cancelled)
    })
}

async fn fail_less_voice_capture(
    less_computer: &Arc<dyn crate::domains::LessComputerApi>,
    session_id: SessionId,
    error: BackendError,
) -> BackendError {
    if error.code == BackendErrorCode::Cancelled {
        let _ = less_computer.abort_capture(session_id);
        return VoiceCaptureLifecycle::cancelled_error();
    }
    // Use the service's atomic terminal claim, shared with native faults and
    // user cancellation. Never expose provider URLs, headers or raw errors.
    let public = BackendError::new(error.code, crate::less_computer::VOICE_CAPTURE_FAILED)
        .retryable(error.retryable);
    match less_computer
        .capture_fault(session_id, public.clone())
        .await
    {
        Err(error) if error.code == BackendErrorCode::Cancelled => {
            VoiceCaptureLifecycle::cancelled_error()
        }
        _ => public,
    }
}

impl VoiceCaptureControl {
    fn take_recording(&self) -> Option<Box<dyn crate::ports::ActiveRecording>> {
        self.recording
            .lock()
            .expect("voice recording lock poisoned")
            .take()
    }

    fn cancel_resources(
        self: &Arc<Self>,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        self.closed.store(true, Ordering::Release);
        let control = Arc::clone(self);
        let resources = self
            .resources
            .lock()
            .expect("voice resource lock poisoned")
            .take();
        own_voice_effect(
            &self.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let recording = match control.take_recording() {
                    Some(recording) => recording.stop().await,
                    None => Ok(()),
                };
                let transcription = match control.transcription.as_ref() {
                    Some(transcription) => transcription.cancel().await,
                    None => Ok(()),
                };
                control
                    .feedback
                    .lock()
                    .expect("voice feedback lock poisoned")
                    .take();
                recording.and(transcription)
            }),
        )
    }
}

struct VoiceControlGuard {
    session_id: SessionId,
    control: Arc<VoiceCaptureControl>,
    controls: Arc<Mutex<HashMap<SessionId, Arc<VoiceCaptureControl>>>>,
}

impl Drop for VoiceControlGuard {
    fn drop(&mut self) {
        // Release the hold first: observers of the terminal idle snapshot may
        // immediately start another capture or text turn.
        self.control
            .resources
            .lock()
            .expect("voice resource lock poisoned")
            .take();
        self.control
            .feedback
            .lock()
            .expect("voice feedback lock poisoned")
            .take();
        let mut controls = self.controls.lock().expect("voice control lock poisoned");
        if controls
            .get(&self.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.control))
        {
            controls.remove(&self.session_id);
        }
    }
}

impl TextStreamSink for VoiceTranscriptSink {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        let delta = crate::types::TranscriptDelta {
            text: chunk.text,
            offset: chunk.offset,
            is_final: false,
        };
        let mut transcript = self
            .transcript
            .lock()
            .expect("voice transcript lock poisoned");
        transcript.apply(&delta)?;
        if let Some(feedback) = &self.feedback {
            feedback.transcript(transcript.text());
        }
        drop(transcript);
        self.publisher.publish(
            Some(self.session_id),
            BackendEventKind::TranscriptDelta(delta),
        );
        Ok(())
    }
}

impl VoiceTranscriptSink {
    fn publish_final(&self, transcript: String) -> Result<(), BackendError> {
        let mut current = self
            .transcript
            .lock()
            .expect("voice transcript lock poisoned");
        let (offset, text) = match transcript.strip_prefix(current.text()) {
            Some(suffix) => (current.text().chars().count() as u64, suffix.to_string()),
            None => (0, transcript),
        };
        let delta = crate::types::TranscriptDelta {
            text,
            offset,
            is_final: true,
        };
        current.apply(&delta)?;
        drop(current);
        self.publisher.publish(
            Some(self.session_id),
            BackendEventKind::TranscriptDelta(delta),
        );
        Ok(())
    }
}

impl LessComputerVoiceSession {
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Feed one 16 kHz / mono / signed 16-bit little-endian PCM frame.
    pub fn feed_pcm(&self, pcm: &[u8]) -> Result<(), BackendError> {
        if self
            .control
            .closed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "Less Computer voice session is closed",
            ));
        }
        if self.less_computer.capture_cancelled(self.session_id) {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "Less Computer voice session was cancelled",
            ));
        }
        if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "Less Computer PCM must be non-empty and contain complete 16-bit samples",
            ));
        }
        if let Some(transcription) = self.control.transcription.as_ref() {
            transcription.consume_pcm_chunk(pcm);
        }
        if let Some(buffer) = self.control.pcm.as_ref() {
            buffer.consume_pcm_chunk(pcm);
        }
        Ok(())
    }

    pub fn cancel(&self) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        if self
            .control
            .closed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Box::pin(async { Ok(()) });
        }
        self.feedback
            .settle(crate::events::LessComputerVoiceOutcome::Cancelled, None);
        let control = Arc::clone(&self.control);
        let controls = Arc::clone(&self.controls);
        let less_computer = Arc::clone(&self.less_computer);
        let session_id = self.session_id;
        own_voice_effect(
            &self.control.task_spawner,
            Box::pin(async move {
                let _guard = VoiceControlGuard {
                    session_id,
                    control: Arc::clone(&control),
                    controls,
                };
                let service_result = less_computer.cancel(Some(session_id)).await;
                let resource_result = control.cancel_resources().await;
                let _ = less_computer.abort_capture(session_id);
                resource_result.and(service_result)
            }),
        )
    }

    pub fn mode(&self) -> crate::events::LessComputerVoiceMode {
        self.mode
    }

    /// Stop capture and deliver the transcript according to the session mode:
    /// `Submit` starts an Agent turn, `Dictate` only publishes the text.
    pub fn finish(
        self,
    ) -> futures_util::future::BoxFuture<'static, Result<LessComputerVoiceFinish, BackendError>>
    {
        use crate::events::{LessComputerVoiceMode, LessComputerVoiceOutcome};
        if self
            .control
            .closed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "Less Computer voice session has already been finalized",
                ))
            });
        }
        let control = Arc::clone(&self.control);
        let controls = Arc::clone(&self.controls);
        let transcription = control.transcription.clone();
        let pcm = control.pcm.clone();
        let omni = control.omni.clone();
        let recording = control.take_recording();
        let archive = recording.as_ref().and_then(|recording| recording.archive());
        let archive_successful_recording = self.archive_successful_recording;
        let less_computer = Arc::clone(&self.less_computer);
        let request = self.request;
        let partials = Arc::clone(&self.partials);
        let session_id = self.session_id;
        let mode = self.mode;
        let feedback = Arc::clone(&self.feedback);
        let resources = control
            .resources
            .lock()
            .expect("voice resource lock poisoned")
            .clone();
        let task_spawner = Arc::clone(&control.task_spawner);
        let omni_task_spawner = Arc::clone(&task_spawner);
        if let Some(feedback) = control
            .feedback
            .lock()
            .expect("voice feedback lock poisoned")
            .as_ref()
        {
            feedback
                .0
                .phase(crate::events::LessComputerVoicePhase::Transcribing);
        }
        own_voice_effect(
            &task_spawner,
            Box::pin(async move {
                let resources = resources;
                let _guard = VoiceControlGuard {
                    session_id,
                    control,
                    controls,
                };
                if less_computer.capture_cancelled(session_id) {
                    feedback.settle(LessComputerVoiceOutcome::Cancelled, None);
                    if let Some(recording) = recording {
                        let _ = recording.stop().await;
                    }
                    if let Some(transcription) = transcription.as_ref() {
                        let _ = transcription.cancel().await;
                    }
                    let _ = less_computer.abort_capture(session_id);
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "Less Computer voice session was cancelled",
                    ));
                }
                if let Some(recording) = recording {
                    if let Err(error) = recording.stop().await {
                        if let Some(transcription) = transcription.as_ref() {
                            let _ = transcription.cancel().await;
                        }
                        return Err(fail_less_voice_finish(
                            &less_computer,
                            session_id,
                            &feedback,
                            mode,
                            error,
                        )
                        .await);
                    }
                }
                let transcript = match (&transcription, &pcm, &omni) {
                    (Some(transcription), _, _) => match transcription.finish().await {
                        Ok(output) => output.text,
                        Err(error) => {
                            let _ = transcription.cancel().await;
                            return Err(fail_less_voice_finish(
                                &less_computer,
                                session_id,
                                &feedback,
                                mode,
                                error,
                            )
                            .await);
                        }
                    },
                    (None, Some(pcm), Some(omni)) => {
                        let wav = match crate::audio::encode_dictation_wav(&pcm.snapshot()) {
                            Ok(wav) => wav,
                            Err(error) => {
                                return Err(fail_less_voice_finish(
                                    &less_computer,
                                    session_id,
                                    &feedback,
                                    mode,
                                    error,
                                )
                                .await);
                            }
                        };
                        match crate::cloud_providers::complete_omni_instruction_from_wav(
                            omni.credentials.as_ref(),
                            omni.context.as_ref(),
                            &wav,
                            &omni.system_prompt,
                            omni_task_spawner,
                        )
                        .await
                        {
                            Ok(text) => text,
                            Err(error) => {
                                return Err(fail_less_voice_finish(
                                    &less_computer,
                                    session_id,
                                    &feedback,
                                    mode,
                                    error,
                                )
                                .await);
                            }
                        }
                    }
                    _ => {
                        return Err(fail_less_voice_finish(
                            &less_computer,
                            session_id,
                            &feedback,
                            mode,
                            BackendError::new(
                                BackendErrorCode::Internal,
                                "Less Computer voice capture has an invalid pipeline shape",
                            ),
                        )
                        .await);
                    }
                };
                if less_computer.capture_cancelled(session_id) {
                    feedback.settle(LessComputerVoiceOutcome::Cancelled, None);
                    if let Some(transcription) = transcription.as_ref() {
                        let _ = transcription.cancel().await;
                    }
                    let _ = less_computer.abort_capture(session_id);
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "Less Computer voice session was cancelled while transcribing",
                    ));
                }
                let transcript = transcript.trim().to_string();
                if transcript.is_empty() {
                    if mode == LessComputerVoiceMode::Dictate {
                        // Silence is not an error for dictation; the composer shows a hint.
                        feedback.settle(LessComputerVoiceOutcome::Empty, Some(String::new()));
                        let _ = less_computer.abort_capture(session_id);
                        drop(resources);
                        drop(_guard);
                        return Ok(LessComputerVoiceFinish::Dictated {
                            transcript: String::new(),
                        });
                    }
                    return Err(fail_less_voice_finish(
                        &less_computer,
                        session_id,
                        &feedback,
                        mode,
                        BackendError::new(
                            BackendErrorCode::Provider,
                            "transcription provider returned an empty transcript",
                        ),
                    )
                    .await);
                }
                // Preserve failed/debug audio, but successful voice-agent ASR obeys
                // the same retention switch as ordinary 1.x dictation. Agent failure
                // after this point is not an ASR failure and must not retain the WAV.
                if !archive_successful_recording {
                    if let Some(archive) = archive.filter(|archive| archive.is_available()) {
                        if let Err(error) = archive.discard().await {
                            log::warn!(
                                "failed to discard successful Less Computer recording: {error}"
                            );
                        }
                    }
                }
                if less_computer.capture_cancelled(session_id) {
                    feedback.settle(LessComputerVoiceOutcome::Cancelled, None);
                    return Err(VoiceCaptureLifecycle::cancelled_error());
                }
                if let Err(error) = partials.publish_final(transcript.clone()) {
                    feedback.settle(LessComputerVoiceOutcome::Failed, None);
                    if mode == LessComputerVoiceMode::Dictate {
                        let _ = less_computer.abort_capture(session_id);
                    }
                    return Err(error);
                }
                if mode == LessComputerVoiceMode::Dictate {
                    // Release the capture before the terminal snapshot so a text
                    // submit triggered by the composer never observes it as busy.
                    let _ = less_computer.abort_capture(session_id);
                    drop(resources);
                    feedback.settle(
                        LessComputerVoiceOutcome::Committed,
                        Some(transcript.clone()),
                    );
                    drop(_guard);
                    return Ok(LessComputerVoiceFinish::Dictated { transcript });
                }
                feedback.settle(
                    LessComputerVoiceOutcome::Submitted,
                    Some(transcript.clone()),
                );
                drop(_guard);
                // Keep the hold through capture -> run promotion. If cancellation
                // won immediately before submit, acquire() must reject this old id
                // instead of treating its removed capture as a fresh text request.
                let _resources = resources;
                let mut request = request;
                request.transcript = transcript;
                match less_computer.submit(request).await {
                    Ok(result) => Ok(LessComputerVoiceFinish::Submitted(result)),
                    Err(error) => {
                        let _ = less_computer.abort_capture(session_id);
                        Err(error)
                    }
                }
            }),
        )
    }
}

/// Terminal failure while finishing a capture. Agent-bound captures keep the
/// existing conversation error; dictation never became a turn, so it only
/// releases the capture and reports through its idle snapshot.
async fn fail_less_voice_finish(
    less_computer: &Arc<dyn crate::domains::LessComputerApi>,
    session_id: SessionId,
    feedback: &LessVoiceFeedback,
    mode: crate::events::LessComputerVoiceMode,
    error: BackendError,
) -> BackendError {
    use crate::events::LessComputerVoiceOutcome;
    if mode == crate::events::LessComputerVoiceMode::Dictate {
        let cancelled = error.code == BackendErrorCode::Cancelled
            || less_computer.capture_cancelled(session_id);
        feedback.settle(
            if cancelled {
                LessComputerVoiceOutcome::Cancelled
            } else {
                LessComputerVoiceOutcome::Failed
            },
            None,
        );
        let _ = less_computer.abort_capture(session_id);
        return if cancelled {
            VoiceCaptureLifecycle::cancelled_error()
        } else {
            BackendError::new(error.code, crate::less_computer::VOICE_CAPTURE_FAILED)
                .retryable(error.retryable)
        };
    }
    let public = fail_less_voice_capture(less_computer, session_id, error).await;
    feedback.settle(
        if public.code == BackendErrorCode::Cancelled {
            LessComputerVoiceOutcome::Cancelled
        } else {
            LessComputerVoiceOutcome::Failed
        },
        None,
    );
    public
}

impl VoiceTranscriptionSession {
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn finish(&self) -> futures_util::future::BoxFuture<'static, Result<String, BackendError>> {
        if let Err(error) = self.lifecycle.begin_finish() {
            return Box::pin(async move { Err(error) });
        }
        let recording = self
            .recording
            .lock()
            .expect("voice transcription recording lock poisoned")
            .take();
        let transcription = self.transcription.clone();
        let pcm = self.pcm.clone();
        let omni = self.omni.clone();
        let partials = Arc::clone(&self.partials);
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources = lifecycle.resources();
        let omni_task_spawner = Arc::clone(&self.task_spawner);
        own_voice_effect(
            &self.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let result = async {
                    if let Some(recording) = recording {
                        stop_and_discard_recording(recording).await?;
                    }
                    if lifecycle.is_cancelled() {
                        return Err(VoiceCaptureLifecycle::cancelled_error());
                    }
                    let transcript = match (&transcription, &pcm, &omni) {
                        (Some(transcription), _, _) => {
                            transcription.finish().await?.text.trim().to_string()
                        }
                        (None, Some(pcm), Some(omni)) => {
                            // #1119 B: multimodal first stage — Omni audio→instruction text.
                            // Host keeps finish() -> String so selection_voice_session stays thin.
                            let wav = crate::audio::encode_dictation_wav(&pcm.snapshot())?;
                            crate::cloud_providers::complete_omni_instruction_from_wav(
                                omni.credentials.as_ref(),
                                omni.context.as_ref(),
                                &wav,
                                &omni.system_prompt,
                                omni_task_spawner,
                            )
                            .await?
                        }
                        _ => {
                            return Err(BackendError::new(
                                BackendErrorCode::Internal,
                                "voice transcription capture has an invalid pipeline shape",
                            ));
                        }
                    };
                    if transcript.is_empty() {
                        return Err(BackendError::new(
                            BackendErrorCode::Provider,
                            "transcription provider returned an empty transcript",
                        ));
                    }
                    Ok(transcript)
                }
                .await;
                let (result, cancel_provider) = lifecycle.settle(result);
                if cancel_provider {
                    if let Some(transcription) = transcription.as_ref() {
                        let _ = transcription.cancel().await;
                    }
                }
                lifecycle.release_resources();
                let transcript = result?;
                partials.publish_final(transcript.clone())?;
                Ok(transcript)
            }),
        )
    }

    pub fn cancel(&self) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        if !self.lifecycle.claim_cancel() {
            return Box::pin(async { Ok(()) });
        }
        let recording = self
            .recording
            .lock()
            .expect("voice transcription recording lock poisoned")
            .take();
        let transcription = self.transcription.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources = lifecycle.resources();
        own_voice_effect(
            &self.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let (recording_result, transcription_result) = futures_util::future::join(
                    async move {
                        match recording {
                            Some(recording) => stop_and_discard_recording(recording).await,
                            None => Ok(()),
                        }
                    },
                    async move {
                        match transcription {
                            Some(transcription) => transcription.cancel().await,
                            None => Ok(()),
                        }
                    },
                )
                .await;
                lifecycle.release_resources();
                recording_result.and(transcription_result)
            }),
        )
    }
}

impl QaVoiceCaptureSession {
    pub fn context(&self) -> Arc<DictationContext> {
        Arc::clone(&self.context)
    }

    /// Allow queued silence/fault decisions to enter the QA state machine only
    /// after the Host has installed this capture in its session registry.
    #[doc(hidden)]
    pub fn arm_recording_progress(&self) {
        self.recording_progress.arm();
    }

    pub fn finish(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Result<QaVoiceCaptureResult, BackendError>> {
        if let Err(error) = self.lifecycle.begin_finish() {
            return Box::pin(async move { Err(error) });
        }
        let recording = self
            .recording
            .lock()
            .expect("QA voice recording lock poisoned")
            .take();
        let transcription = self.transcription.clone();
        let pcm = self.pcm.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources = lifecycle.resources();
        own_voice_effect(
            &self.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let result = async {
                    if let Some(recording) = recording {
                        stop_and_discard_recording(recording).await?;
                    }
                    if lifecycle.is_cancelled() {
                        return Err(VoiceCaptureLifecycle::cancelled_error());
                    }
                    match (&transcription, pcm) {
                        (Some(transcription), None) => {
                            let output = transcription.finish().await?;
                            let transcript = output.text.trim().to_string();
                            if transcript.is_empty() {
                                return Err(BackendError::new(
                                    BackendErrorCode::Provider,
                                    "transcription provider returned an empty transcript",
                                ));
                            }
                            Ok(QaVoiceCaptureResult {
                                transcript: Some(transcript),
                                audio_wav: None,
                                duration_ms: output.duration_ms,
                            })
                        }
                        (None, Some(pcm)) => Ok(QaVoiceCaptureResult {
                            transcript: None,
                            audio_wav: Some(crate::audio::encode_dictation_wav(&pcm.snapshot())?),
                            duration_ms: pcm.duration_ms(),
                        }),
                        _ => Err(BackendError::new(
                            BackendErrorCode::Internal,
                            "QA voice capture has an invalid pipeline shape",
                        )),
                    }
                }
                .await;
                let (result, cancel_provider) = lifecycle.settle(result);
                if cancel_provider {
                    if let Some(transcription) = transcription {
                        let _ = transcription.cancel().await;
                    }
                }
                lifecycle.release_resources();
                result
            }),
        )
    }

    pub fn cancel(&self) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        if !self.lifecycle.claim_cancel() {
            return Box::pin(async { Ok(()) });
        }
        let recording = self
            .recording
            .lock()
            .expect("QA voice recording lock poisoned")
            .take();
        let transcription = self.transcription.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources = lifecycle.resources();
        own_voice_effect(
            &self.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let (recording_result, transcription_result) = futures_util::future::join(
                    async move {
                        match recording {
                            Some(recording) => stop_and_discard_recording(recording).await,
                            None => Ok(()),
                        }
                    },
                    async move {
                        match transcription {
                            Some(transcription) => transcription.cancel().await,
                            None => Ok(()),
                        }
                    },
                )
                .await;
                lifecycle.release_resources();
                recording_result.and(transcription_result)
            }),
        )
    }
}

async fn stop_and_discard_recording(
    recording: Box<dyn ActiveRecording>,
) -> Result<(), BackendError> {
    let archive = recording.archive();
    let stop_result = recording.stop().await;
    let discard_result = match archive.filter(|archive| archive.is_available()) {
        Some(archive) => archive.discard().await,
        None => Ok(()),
    };
    stop_result.and(discard_result)
}

struct DiscardTextStream;

impl TextStreamSink for DiscardTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

impl AudioConsumer for LessComputerVoiceSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let _ = self.feed_pcm(pcm);
    }
}

/// Repository bundle shared by every host-facing facade in one process.
///
/// Tauri's compatibility coordinator and the new core facade must use the same
/// instances; opening the same JSON files twice would create divergent
/// in-memory snapshots even though the paths are identical.
#[derive(Clone)]
pub struct BackendRepositories {
    pub preferences: Arc<PreferencesStore>,
    pub history: Arc<HistoryStore>,
    pub activity: Arc<ActivityStore>,
    pub vocabulary: Arc<DictionaryStore>,
    pub correction_rules: Arc<CorrectionRuleStore>,
    pub style_packs: Arc<StylePackStore>,
}

impl BackendRepositories {
    pub fn open(data_dir: &std::path::Path) -> Result<Self, BackendError> {
        let preferences = Arc::new(PreferencesStore::open(data_dir.join("preferences.json"))?);
        let style_packs = Arc::new(
            StylePackStore::at_data_dir_with_preferences(data_dir, &preferences.get())
                .unwrap_or_else(|_| StylePackStore::in_memory()),
        );
        let mut preference_snapshot = preferences.get();
        if !crate::cloud_sync_e2ee_store::gate::recovery_pending_for_path(
            &data_dir.join("preferences.json"),
        ) && sync_style_pack_preferences(&mut preference_snapshot, &style_packs.list()?)
        {
            preferences.set(preference_snapshot)?;
        }
        Ok(Self {
            preferences,
            history: Arc::new(HistoryStore::at_data_dir(data_dir)),
            activity: Arc::new(
                ActivityStore::at_data_dir(data_dir).unwrap_or_else(|_| ActivityStore::in_memory()),
            ),
            vocabulary: Arc::new(DictionaryStore::at_data_dir(data_dir)),
            correction_rules: Arc::new(CorrectionRuleStore::at_data_dir(data_dir)),
            style_packs,
        })
    }
}

struct MutableState {
    running: bool,
    dictation: DictationStateSnapshot,
    dictation_context: Option<Arc<DictationContext>>,
    /// The requested output target is published before async context capture
    /// completes, so a second quick-note hotkey edge can still resolve the
    /// Starting session instead of being swallowed.
    dictation_start_output_target: Option<DictationOutputTarget>,
    /// Session-local intent, including a modifier pressed while AX/credentials
    /// are still being captured. The accepted request is applied before finish
    /// by every stop entry; no Host latch survives into the next session.
    dictation_translation_requested: Option<bool>,
    dictation_destination_changed: bool,
    credentials: CredentialsStatus,
    transcripts: HashMap<SessionId, crate::types::TranscriptAccumulator>,
    recording_fault_cleanup: Option<SessionId>,
    silence_monitor: Option<SilenceMonitor>,
}

struct SilenceMonitor {
    session_id: SessionId,
    started_at: std::time::Instant,
    detector: crate::silence_auto_stop::SilenceAutoStop,
}

// Preparation itself can switch a native input source. Register one shared
// future before polling it so cancellation can join and restore even when the
// original start caller is dropped. Synchronous progress only sees ready values.
type TextInsertionPreparation = futures_util::future::Shared<
    futures_util::future::BoxFuture<'static, Result<Arc<ActiveTextInsertion>, BackendError>>,
>;

struct ActiveTextInsertion {
    session_id: SessionId,
    insert_attempts: AtomicU64,
    platform: Arc<dyn TextInsertionSession>,
    streaming: bool,
    script: crate::shared_types::ChineseScriptPreference,
    save_streamed_text_to_clipboard: bool,
    /// Live ASR partials are shown as an in-place composition in the target.
    composition_input: bool,
    state: Mutex<ActiveTextInsertionState>,
    drained: tokio::sync::Notify,
    task_spawner: Arc<dyn TaskSpawner>,
    _resources: Mutex<Option<Arc<crate::voice_session::VoiceResourceHold>>>,
    cancel_result: Mutex<Option<Result<(), BackendError>>>,
    /// Only one native-cleanup retry worker may operate on an insertion at a time. The resource
    /// hold remains retained until native cleanup succeeds, while the worker keeps retrying with
    /// a bounded backoff rather than leaving the session with no cleanup path.
    cleanup_retry_running: AtomicBool,
    // 0: open, 1: native finalization in flight, 2: cancellation owns cleanup,
    // 3: native finalization settled, 4: cancellation cleanup settled.
    // Cancellation must join state 1 or 2 rather
    // than release the next voice session while its committed input continues.
    terminal: AtomicU8,
}

#[derive(Default)]
struct ActiveTextInsertionState {
    stream: crate::streaming_insert::StreamingInsertState,
    scheduled: bool,
    /// Newest transcript partial not yet sent as a composition update.
    partial_pending: Option<String>,
    partial_running: bool,
    /// A composition update failed; the host can no longer show partials.
    partial_failed: bool,
    /// The dictation is finishing or cancelling; no further partials are sent.
    partial_closed: bool,
}

impl ActiveTextInsertion {
    fn new(
        session_id: SessionId,
        platform: Arc<dyn TextInsertionSession>,
        context: &DictationContext,
        task_spawner: Arc<dyn TaskSpawner>,
        resources: Arc<crate::voice_session::VoiceResourceHold>,
    ) -> Arc<Self> {
        let windows_non_streaming = cfg!(target_os = "windows")
            && context.insertion.windows_insertion_mode
                != crate::shared_types::WindowsInsertionMode::SendInput;
        let platform_streaming = platform.supports_streaming();
        // Composition already owns the target text until finish replaces it. Streaming
        // polish chunks would type alongside (or into) that composition and leave the
        // raw transcript behind, so the two paths are mutually exclusive.
        let composition_input = context.insertion.composition_input;
        Arc::new(Self {
            session_id,
            insert_attempts: AtomicU64::new(0),
            platform,
            streaming: !composition_input
                && context.uses_llm_polisher()
                && platform_streaming
                && crate::streaming_insert::streaming_insert_eligible(
                    context.insertion.streaming,
                    context.polish.translation_active,
                    context.polish.chinese_script_preference
                        == crate::shared_types::ChineseScriptPreference::Traditional,
                    windows_non_streaming,
                ),
            script: context.polish.chinese_script_preference,
            save_streamed_text_to_clipboard: context.insertion.save_streamed_text_to_clipboard,
            composition_input,
            state: Mutex::new(ActiveTextInsertionState::default()),
            drained: tokio::sync::Notify::new(),
            task_spawner,
            _resources: Mutex::new(Some(resources)),
            cancel_result: Mutex::new(None),
            cleanup_retry_running: AtomicBool::new(false),
            terminal: AtomicU8::new(0),
        })
    }

    fn try_start_cleanup_retry(&self) -> bool {
        self.cleanup_retry_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn finish_cleanup_retry(&self) {
        self.cleanup_retry_running.store(false, Ordering::Release);
    }

    fn abandon_cleanup_retry(&self) {
        // The target can disappear permanently (for example, after its process exits). Release
        // this insertion's resource hold once the bounded cleanup window is exhausted; retaining
        // it forever would make every later dictation appear busy until restart.
        self._resources
            .lock()
            .expect("text insertion resources lock poisoned")
            .take();
        self.finish_cleanup_retry();
    }

    fn begin_insert_attempt(&self, operation: &str, text: &str) -> u64 {
        let attempt = self.insert_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        log::info!(
            "[insert-diagnostics] session_id={} attempt={} operation={} chars={} hash={}",
            self.session_id,
            attempt,
            operation,
            text.chars().count(),
            crate::output_cleaning::privacy_text_hash(text),
        );
        attempt
    }

    async fn write_text(
        &self,
        text: String,
        operation: &str,
    ) -> Result<crate::ports::InsertWriteResult, BackendError> {
        let attempt = self.begin_insert_attempt(operation, &text);
        let result = self.platform.write(text).await;
        log::info!(
            "[insert-diagnostics] session_id={} attempt={} operation={} result={:?}",
            self.session_id,
            attempt,
            operation,
            result
                .as_ref()
                .map(|written| written.written_chars)
                .map_err(|error| error.code),
        );
        result
    }

    async fn finish_platform(&self, text: String) -> Result<InsertOutcome, BackendError> {
        let operation = if text.is_empty() {
            "finalize"
        } else {
            "insert_final"
        };
        let attempt = self.begin_insert_attempt(operation, &text);
        let result = self.platform.finish(text).await;
        log::info!(
            "[insert-diagnostics] session_id={} attempt={} operation={} result={:?}",
            self.session_id,
            attempt,
            operation,
            result.as_ref().map_err(|error| error.code),
        );
        result
    }

    fn push(self: &Arc<Self>, delta: &crate::types::PolishDelta) {
        if !self.streaming || delta.is_final {
            return;
        }
        let text =
            crate::streaming_insert::apply_chinese_script_preference(&delta.text, self.script);
        let should_spawn = {
            let mut state = self.state.lock().expect("text insertion lock poisoned");
            state.stream.push_delta(delta.offset, &text);
            if state.scheduled || state.stream.pending.is_empty() || state.stream.failed.is_some() {
                false
            } else {
                state.scheduled = true;
                true
            }
        };
        if should_spawn {
            let insertion = Arc::clone(self);
            self.task_spawner
                .spawn(Box::pin(async move { insertion.flush_loop().await }));
        }
    }

    fn has_written_text(&self) -> bool {
        !self
            .state
            .lock()
            .expect("text insertion lock poisoned")
            .stream
            .typed_text
            .is_empty()
    }

    async fn flush_loop(self: Arc<Self>) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(
                crate::streaming_insert::STREAMING_FLUSH_INTERVAL_MS,
            ))
            .await;
            let delta = {
                let mut state = self.state.lock().expect("text insertion lock poisoned");
                if state.stream.failed.is_some() {
                    state.scheduled = false;
                    self.drained.notify_waiters();
                    return;
                }
                let delta = state.stream.take_ready_prefix();
                if delta.is_empty() {
                    // Whitespace alone waits for another delta, not another timer
                    // tick. Final reconciliation will flush a trailing suffix.
                    state.scheduled = false;
                    self.drained.notify_waiters();
                    return;
                }
                delta
            };
            let expected = delta.chars().count();
            let result = if delta.is_empty() {
                Ok(crate::ports::InsertWriteResult { written_chars: 0 })
            } else {
                self.write_text(delta.clone(), "stream").await
            };
            let mut state = self.state.lock().expect("text insertion lock poisoned");
            match result {
                Ok(result) if result.written_chars >= expected => {
                    state.stream.typed_text.push_str(&delta);
                }
                Ok(result) => {
                    let written = crate::streaming_insert::append_typed_prefix(
                        &mut state.stream.typed_text,
                        &delta,
                        result.written_chars,
                    );
                    state.stream.failed = Some(format!(
                        "host inserted only {written}/{expected} characters"
                    ));
                }
                Err(error) => state.stream.failed = Some(error.to_string()),
            }
            if state.stream.failed.is_some() || state.stream.pending.is_empty() {
                state.scheduled = false;
                self.drained.notify_waiters();
                return;
            }
        }
    }

    /// Coalesces transcript partials: only the newest one matters, so a slow host
    /// never builds a backlog of stale compositions.
    fn push_partial(self: &Arc<Self>, text: String) {
        let chars = text.chars().count();
        if !self.composition_input {
            log::debug!("[insertion] partial ignored composition_disabled chars={chars}");
            return;
        }
        let should_spawn = {
            let mut state = self.state.lock().expect("text insertion lock poisoned");
            if state.partial_closed || state.partial_failed {
                log::warn!(
                    "[insertion] partial ignored closed={} failed={} chars={chars}",
                    state.partial_closed,
                    state.partial_failed,
                );
                return;
            }
            state.partial_pending = Some(text);
            if state.partial_running {
                false
            } else {
                state.partial_running = true;
                true
            }
        };
        log::info!("[insertion] partial accepted chars={chars} scheduled={should_spawn}");
        if should_spawn {
            let insertion = Arc::clone(self);
            self.task_spawner
                .spawn(Box::pin(async move { insertion.partial_loop().await }));
        }
    }

    async fn partial_loop(self: Arc<Self>) {
        loop {
            let next = {
                let mut state = self.state.lock().expect("text insertion lock poisoned");
                let next = if state.partial_closed {
                    None
                } else {
                    state.partial_pending.take()
                };
                match next {
                    Some(text) => text,
                    None => {
                        state.partial_pending = None;
                        state.partial_running = false;
                        self.drained.notify_waiters();
                        return;
                    }
                }
            };
            let chars = next.chars().count();
            log::info!("[insertion] composition update begin chars={chars}");
            if let Err(error) = self.platform.update_composition(next).await {
                log::warn!("[insertion] composition update stopped chars={chars}: {error}");
                let mut state = self.state.lock().expect("text insertion lock poisoned");
                state.partial_failed = true;
                state.partial_pending = None;
                state.partial_running = false;
                self.drained.notify_waiters();
                return;
            }
            log::info!("[insertion] composition update complete chars={chars}");
        }
    }

    fn close_partials(&self) {
        let mut state = self.state.lock().expect("text insertion lock poisoned");
        state.partial_closed = true;
        state.partial_pending = None;
    }

    async fn wait_for_partial_drain(&self) {
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self
                .state
                .lock()
                .expect("text insertion lock poisoned")
                .partial_running
            {
                break;
            }
            notified.await;
        }
    }

    async fn wait_for_stream_drain(&self) {
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self
                .state
                .lock()
                .expect("text insertion lock poisoned")
                .scheduled
            {
                break;
            }
            notified.await;
        }
    }

    async fn finish(self: &Arc<Self>, final_text: String) -> Result<InsertOutcome, BackendError> {
        self.wait_for_stream_drain().await;
        // The final text must replace the composition only after the last
        // partial update has landed, or that update would overwrite it.
        self.close_partials();
        self.wait_for_partial_drain().await;
        self.terminal
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "text insertion session was cancelled before completion",
                )
            })?;
        // Once native finalization is committed, a dropped IPC/stop future
        // must not abandon its input-source restoration or leave terminal=1
        // forever. The host executor owns this effect until it settles; the
        // caller only owns its response, and cancel() joins the same operation.
        let insertion = Arc::clone(self);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        self.task_spawner.spawn(Box::pin(async move {
            let result = insertion.finish_committed(final_text).await;
            // Only Windows TSF can retain a composition after failed finalization. Other hosts
            // have already settled cleanup and may cache the delivery error in their terminal.
            insertion.terminal.store(
                if cfg!(target_os = "windows") && result.is_err() {
                    0
                } else {
                    3
                },
                Ordering::Release,
            );
            insertion.drained.notify_waiters();
            let _ = result_tx.send(result);
        }));
        result_rx.await.map_err(|_| {
            BackendError::new(
                BackendErrorCode::Internal,
                "native insertion finalization task did not return a result",
            )
        })?
    }

    async fn finish_committed(&self, final_text: String) -> Result<InsertOutcome, BackendError> {
        let reconciliation = self
            .state
            .lock()
            .expect("text insertion lock poisoned")
            .stream
            .reconcile_final(&final_text);
        use crate::streaming_insert::FinalReconciliation;
        match reconciliation {
            FinalReconciliation::InsertFinal(text) => self.finish_platform(text).await,
            FinalReconciliation::WriteTail(tail) => {
                let expected = tail.chars().count();
                let written = self.write_text(tail, "tail").await;
                let complete = matches!(written, Ok(result) if result.written_chars >= expected);
                if complete {
                    if self.save_streamed_text_to_clipboard {
                        if let Err(error) = self.platform.copy(final_text).await {
                            log::warn!("failed to preserve streamed text on clipboard: {error}");
                        }
                    }
                    self.finish_platform(String::new()).await
                } else {
                    self.finish_with_fallback(final_text).await
                }
            }
            FinalReconciliation::Complete => {
                if self.save_streamed_text_to_clipboard {
                    if let Err(error) = self.platform.copy(final_text).await {
                        log::warn!("failed to preserve streamed text on clipboard: {error}");
                    }
                }
                self.finish_platform(String::new()).await
            }
            FinalReconciliation::CopyFallback(text) => self.finish_with_fallback(text).await,
        }
    }

    async fn finish_with_fallback(&self, text: String) -> Result<InsertOutcome, BackendError> {
        let copied = self.platform.copy(text).await;
        log::info!(
            "[insert-diagnostics] session_id={} operation=copy_fallback result={:?}",
            self.session_id,
            copied.as_ref().map_err(|error| error.code),
        );
        let closed = self.finish_platform(String::new()).await;
        copied?;
        closed?;
        Ok(InsertOutcome::CopiedFallback)
    }

    async fn cancel(self: &Arc<Self>) -> Result<(), BackendError> {
        self.close_partials();
        {
            let mut state = self.state.lock().expect("text insertion lock poisoned");
            state.stream.pending.clear();
            state.stream.failed = Some("text insertion session was cancelled".to_string());
        }
        // A late starter and an explicit cancel can reach this same session.
        // Both must await the single cleanup; treating the second call as an
        // immediate success would admit B while A still restores its source.
        // A failed finalization returns terminal to 0, so cancellation must
        // retry ownership instead of treating that transition as success.
        let mut cleanup_retries = 0;
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.terminal.load(Ordering::Acquire) {
                0 => {
                    if self
                        .terminal
                        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        let insertion = Arc::clone(self);
                        self.task_spawner.spawn(Box::pin(async move {
                            insertion.wait_for_stream_drain().await;
                            // An in-flight composition update must land before the cancel clears
                            // the composition, or it would leave the transcript behind.
                            insertion.wait_for_partial_drain().await;
                            let result = insertion.platform.cancel().await;
                            log::info!(
                                "[insert-diagnostics] session_id={} operation=cancel attempts={} result={:?}",
                                insertion.session_id,
                                insertion.insert_attempts.load(Ordering::Relaxed),
                                result.as_ref().map_err(|error| error.code),
                            );
                            *insertion
                                .cancel_result
                                .lock()
                                .expect("cancel result lock poisoned") = Some(result);
                            insertion.terminal.store(4, Ordering::Release);
                            insertion.drained.notify_waiters();
                        }));
                    }
                }
                1 | 2 => notified.await,
                3 => return Ok(()),
                4 => {
                    let mut cancel_result = self
                        .cancel_result
                        .lock()
                        .expect("cancel result lock poisoned");
                    let Some(result) = cancel_result.clone() else {
                        continue;
                    };
                    match result {
                        Ok(()) => return Ok(()),
                        Err(_error) if cfg!(target_os = "windows") && cleanup_retries < 2 => {
                            if self
                                .terminal
                                .compare_exchange(4, 0, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                            {
                                cleanup_retries += 1;
                                *cancel_result = None;
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}

struct BackendEngineProgress {
    events: Arc<EventBus>,
    state: Arc<RwLock<MutableState>>,
    phase_changed: Arc<tokio::sync::Notify>,
    text_insertions: Arc<Mutex<HashMap<SessionId, TextInsertionPreparation>>>,
    raw_transcripts: Mutex<HashMap<SessionId, TranscriptAccumulator>>,
}

impl EngineProgressSink for BackendEngineProgress {
    fn publish(&self, session_id: SessionId, progress: EngineProgress) -> Result<(), BackendError> {
        match progress {
            EngineProgress::CaptureReady => {
                let state = self.state.read().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                self.phase_changed.notify_waiters();
            }
            EngineProgress::RecordingLevel { elapsed_ms, level } => {
                let mut state = self.state.write().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                if !matches!(
                    state.dictation.phase,
                    DictationPhase::Starting | DictationPhase::Recording
                ) {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "recording progress arrived after recording stopped",
                    ));
                }
                let level = if level.is_finite() {
                    level.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let silence_decision = state
                    .silence_monitor
                    .as_mut()
                    .filter(|monitor| monitor.session_id == session_id)
                    .and_then(|monitor| {
                        monitor.detector.on_level(
                            level,
                            monitor.started_at + std::time::Duration::from_millis(elapsed_ms),
                        )
                    });
                if !state.dictation.recording_ready
                    || state.dictation.elapsed_ms != elapsed_ms
                    || state.dictation.level != level
                {
                    // A silent first frame at 0 ms is still proof that capture
                    // is live. Do not deduplicate it against initial zeroes.
                    state.dictation.recording_ready = true;
                    state.dictation.elapsed_ms = elapsed_ms;
                    state.dictation.level = level;
                    self.events.publish(
                        Some(session_id),
                        BackendEventKind::DictationStateChanged(state.dictation.clone()),
                    );
                }
                drop(state);
                if let Some(decision) = silence_decision {
                    let action = match decision {
                        crate::silence_auto_stop::SilenceDecision::Stop => {
                            crate::events::RecordingControlAction::Stop
                        }
                        crate::silence_auto_stop::SilenceDecision::Cancel => {
                            crate::events::RecordingControlAction::Cancel
                        }
                    };
                    self.events.publish(
                        Some(session_id),
                        BackendEventKind::RecordingControlRequested(
                            crate::events::RecordingControlRequest { session_id, action },
                        ),
                    );
                }
            }
            EngineProgress::RecordingLimitReached => {
                let state = self.state.read().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                if !matches!(
                    state.dictation.phase,
                    DictationPhase::Starting | DictationPhase::Recording
                ) {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "recording limit arrived after recording stopped",
                    ));
                }
                drop(state);
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::RecordingControlRequested(
                        crate::events::RecordingControlRequest {
                            session_id,
                            action: crate::events::RecordingControlAction::Stop,
                        },
                    ),
                );
            }
            EngineProgress::RecordingFault(error) => {
                let mut state = self.state.write().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                state.dictation.phase = DictationPhase::Failed;
                state.dictation.message = Some(error.message);
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::DictationStateChanged(state.dictation.clone()),
                );
                self.phase_changed.notify_waiters();
            }
            EngineProgress::Notification(notification) => {
                let state = self.state.read().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                drop(state);
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::Notification(notification),
                );
            }
            EngineProgress::Stage(stage) => {
                let mut state = self.state.write().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                let phase = match stage {
                    EngineStage::Transcribing => DictationPhase::Transcribing,
                    EngineStage::Polishing => DictationPhase::Polishing,
                };
                if state.dictation.phase != phase {
                    state.dictation.phase = phase;
                    self.events.publish(
                        Some(session_id),
                        BackendEventKind::DictationStateChanged(state.dictation.clone()),
                    );
                    self.phase_changed.notify_waiters();
                }
            }
            EngineProgress::TranscriptDelta(mut delta) => {
                let mut state = self.state.write().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                if !matches!(
                    state.dictation.phase,
                    DictationPhase::Starting
                        | DictationPhase::Recording
                        | DictationPhase::Transcribing
                ) {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "transcript delta arrived after transcription completed",
                    ));
                }
                // Traditional final text was cleaned before correction and polishing.
                let activation_phrases = state
                    .dictation_context
                    .as_ref()
                    .filter(|context| {
                        !delta.is_final
                            || context.pipeline_mode
                                == crate::shared_types::PipelineMode::Multimodal
                    })
                    .and_then(|context| context.voice_activation.clone());
                if let Some(commands) = activation_phrases {
                    let mut raw_transcripts = self
                        .raw_transcripts
                        .lock()
                        .expect("raw transcript lock poisoned");
                    let raw_transcript = raw_transcripts.entry(session_id).or_insert_with(|| {
                        let mut transcript = TranscriptAccumulator::default();
                        if let Some(existing) = state.transcripts.get(&session_id) {
                            let seed = crate::types::TranscriptDelta {
                                text: existing.text().to_string(),
                                offset: 0,
                                is_final: false,
                            };
                            let _ = transcript.apply(&seed);
                        }
                        transcript
                    });
                    raw_transcript.apply(&delta)?;
                    let cleaned = commands.clean(raw_transcript.text());
                    delta = crate::types::TranscriptDelta {
                        text: cleaned,
                        offset: 0,
                        is_final: delta.is_final,
                    };
                }
                let transcript = {
                    let accumulator = state.transcripts.entry(session_id).or_default();
                    accumulator.apply(&delta)?;
                    accumulator.text().to_string()
                };
                let is_partial = !delta.is_final;
                let transcript_chars = transcript.chars().count();
                drop(state);
                if is_partial {
                    let insertion = self
                        .text_insertions
                        .lock()
                        .expect("text insertion registry lock poisoned")
                        .get(&session_id)
                        .and_then(|preparation| preparation.peek())
                        .and_then(|result| result.as_ref().ok())
                        .cloned();
                    log::info!(
                        "[transcript] partial routed session={} offset={} delta_chars={} accumulated_chars={} insertion_ready={}",
                        session_id,
                        delta.offset,
                        delta.text.chars().count(),
                        transcript_chars,
                        insertion.is_some(),
                    );
                    if let Some(insertion) = insertion {
                        insertion.push_partial(transcript);
                    }
                }
                self.events
                    .publish(Some(session_id), BackendEventKind::TranscriptDelta(delta));
            }
            EngineProgress::PolishDelta(delta) => {
                let state = self.state.read().expect("backend state lock poisoned");
                ensure_active_session(&state, session_id)?;
                if state.dictation.phase != DictationPhase::Polishing {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "polish delta arrived outside the polishing phase",
                    ));
                }
                drop(state);
                if let Some(insertion) = self
                    .text_insertions
                    .lock()
                    .expect("text insertion registry lock poisoned")
                    .get(&session_id)
                    .and_then(|preparation| preparation.peek())
                    .and_then(|result| result.as_ref().ok())
                    .cloned()
                {
                    insertion.push(&delta);
                }
                self.events
                    .publish(Some(session_id), BackendEventKind::PolishDelta(delta));
            }
        }
        Ok(())
    }
}

/// Which kind of recognizer the next dictation uses — see OpenLessBackend::asr_route().
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AsrRoute {
    Cloud,
    Local,
    /// Local recognition with no usable polish channel: the raw transcript
    /// is the output (see capture_dictation_context_with_host).
    LocalRaw,
    /// No usable channel: the next dictation would fail to start.
    Unavailable,
}

/// Which recognizer produced the dictations in history — see
/// OpenLessBackend::asr_usage().
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AsrUsage {
    pub local: u64,
    pub cloud: u64,
    /// Whether the latest dictation was recognized locally; None without history.
    pub last_local: Option<bool>,
}

pub struct OpenLessBackend {
    config: BackendConfig,
    startup_error: Option<BackendError>,
    deps: BackendDependencies,
    clock: Arc<dyn Clock>,
    events: Arc<EventBus>,
    state: Arc<RwLock<MutableState>>,
    phase_changed: Arc<tokio::sync::Notify>,
    hotkey: Mutex<crate::hotkey_interpreter::HotkeyInterpreter>,
    hotkey_dispatch_gate: tokio::sync::Mutex<()>,
    pending_audio_captures:
        Arc<Mutex<HashMap<PendingAudioCaptureKey, Box<dyn PendingAudioCapture>>>>,
    less_computer_hotkey_press_at: Mutex<Option<std::time::Instant>>,
    vocabulary: Arc<DictionaryStore>,
    correction_rules: Arc<CorrectionRuleStore>,
    vocabulary_revision: Arc<AtomicU64>,
    history: Arc<HistoryStore>,
    history_revision: Arc<AtomicU64>,
    activity: Arc<ActivityStore>,
    style_packs: Arc<StylePackStore>,
    style_pack_revision: Arc<AtomicU64>,
    preferences: Arc<PreferencesStore>,
    preferences_revision: Arc<AtomicU64>,
    settings_write_gate: Arc<Mutex<()>>,
    // Serialize channel ownership changes through runtime cleanup, not status reads.
    local_asr_channel_gate: tokio::sync::Mutex<()>,
    cloud_sync: Option<crate::cloud_sync::CloudSyncService>,
    encrypted_sync: Option<crate::cloud_sync_e2ee::EncryptedSyncService>,
    encrypted_sync_store: Option<Arc<crate::cloud_sync_e2ee_store::CoreSyncStore>>,
    local_migration_store: Arc<crate::cloud_sync_e2ee_store::CoreSyncStore>,
    pending_corrections: Arc<Mutex<Vec<PendingCorrection>>>,
    edit_observation_generation: Arc<AtomicU64>,
    text_insertions: Arc<Mutex<HashMap<SessionId, TextInsertionPreparation>>>,
    voice_sessions: Arc<crate::voice_session::VoiceSessionGate>,
    runtime_start_work: Arc<crate::voice_session::RuntimeActivityGate>,
    less_computer_voice_controls: Arc<Mutex<HashMap<SessionId, Arc<VoiceCaptureControl>>>>,
    voice_activation_handle: Arc<Mutex<Option<Arc<dyn crate::ports::VoiceActivationHandle>>>>,
    voice_activation_generation: Arc<AtomicU64>,
    voice_activation_handoff_generation: AtomicU64,
    voice_activation_configure_gate: tokio::sync::Mutex<()>,
    backend_lifecycle_gate: tokio::sync::Mutex<()>,
    backend_shutdown_requested: AtomicBool,
    voice_activation_session: Arc<Mutex<Option<crate::voice_activation::VoiceActivationSession>>>,
}

struct VoiceActivationHandoffGuard {
    backend: Arc<OpenLessBackend>,
    generation: u64,
}

impl Drop for VoiceActivationHandoffGuard {
    fn drop(&mut self) {
        let _ = self
            .backend
            .voice_activation_handoff_generation
            .compare_exchange(self.generation, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[path = "cloud_sync_e2ee/api.rs"]
mod encrypted_sync_api;

struct HistoryProviderAttribution {
    asr_provider: Option<String>,
    asr_model: Option<String>,
    llm_provider: Option<String>,
    llm_model: Option<String>,
    asr_ms: Option<u64>,
    polish_ms: Option<u64>,
}

struct CoreEditObservationSink {
    settings: crate::shared_types::VocabularyLearningSettings,
    expected_generation: u64,
    generation: Arc<AtomicU64>,
    typed_text: String,
    pending: Arc<Mutex<Vec<PendingCorrection>>>,
    events: Arc<EventBus>,
}

impl EditObservationSink for CoreEditObservationSink {
    fn observation_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.settings.observation_seconds as u64)
    }

    fn publish(&self, edit: crate::host_document::EditPair) -> bool {
        // Dropping the native watcher is asynchronous on macOS: a queued AX
        // callback may still arrive after the next session starts. The Core
        // generation is therefore the authoritative stale-report barrier.
        if self.generation.load(Ordering::Acquire) != self.expected_generation {
            return false;
        }
        let rules = crate::host_document::learned_rules_with_max_chars(
            &edit,
            self.settings.max_phrase_chars as usize,
        );
        let mut published = false;
        for rule in rules {
            // Validate the recovered word, not a lone changed character (or
            // an empty spelling insertion). Reject words absent from this insertion.
            if !self.typed_text.contains(&rule.pattern) {
                log::debug!("observed vocabulary candidate outside inserted text");
                continue;
            }
            match queue_pending_correction_state(
                &self.pending,
                &self.events,
                rule.pattern,
                rule.replacement,
                Some((&self.generation, self.expected_generation)),
                self.settings.suggestion_seconds,
            ) {
                Ok(_) => published = true,
                Err(error) => log::warn!("failed to queue observed correction: {error}"),
            }
        }
        if !published {
            log::debug!("observed edit produced no eligible vocabulary candidate");
        }
        published
    }
}

fn queue_pending_correction_state(
    pending: &Arc<Mutex<Vec<PendingCorrection>>>,
    events: &Arc<EventBus>,
    pattern: String,
    replacement: String,
    generation: Option<(&AtomicU64, u64)>,
    suggestion_seconds: u32,
) -> Result<Option<PendingCorrection>, BackendError> {
    if pattern.trim().is_empty() || replacement.trim().is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "correction pattern and replacement are required",
        ));
    }
    let (suggestion, snapshot) = {
        let mut pending = pending.lock().expect("pending correction lock poisoned");
        if generation.is_some_and(|(current, expected)| current.load(Ordering::Acquire) != expected)
        {
            return Ok(None);
        }
        let now = chrono::Utc::now().timestamp_millis();
        pending.retain(|item| item.expires_at_ms > now);
        if pending
            .iter()
            .any(|item| item.pattern == pattern && item.replacement == replacement)
        {
            return Ok(None);
        }
        if pending.len() >= MAX_PENDING_CORRECTIONS {
            pending.remove(0);
        }
        let suggestion = PendingCorrection {
            id: uuid::Uuid::new_v4().to_string(),
            expires_at_ms: now + i64::from(suggestion_seconds.clamp(5, 60)) * 1000,
            pattern,
            replacement,
        };
        pending.push(suggestion.clone());
        (suggestion, pending.clone())
    };
    events.publish(
        None,
        BackendEventKind::VocabularySuggestionsChanged(snapshot),
    );
    Ok(Some(suggestion))
}

fn settings_transaction_error(
    mut primary: BackendError,
    compensation_errors: Vec<BackendError>,
) -> BackendError {
    if compensation_errors.is_empty() {
        return primary;
    }
    primary.details = Some(serde_json::json!({
        "primaryError": primary.clone(),
        "compensationErrors": compensation_errors,
    }));
    primary
}

impl HistoryProviderAttribution {
    fn from_context(
        context: &DictationContext,
        llm_used: bool,
        asr_ms: Option<u64>,
        polish_ms: Option<u64>,
        asr_call_label: Option<&crate::auxiliary::AsrCallLabel>,
        llm_call_label: Option<&crate::polish::LlmCallLabel>,
    ) -> Self {
        match context.pipeline_mode {
            crate::shared_types::PipelineMode::Traditional => Self {
                asr_provider: Some(
                    asr_call_label
                        .map(|label| label.provider.clone())
                        .unwrap_or_else(|| context.asr.provider_id.clone()),
                ),
                asr_model: asr_call_label
                    .and_then(|label| label.model.clone())
                    .or_else(|| context.asr.model.clone()),
                llm_provider: llm_used.then(|| {
                    llm_call_label
                        .map(|label| label.provider.clone())
                        .unwrap_or_else(|| context.llm.provider_id.clone())
                }),
                llm_model: llm_used
                    .then(|| {
                        llm_call_label
                            .map(|label| label.model.clone())
                            .or_else(|| context.llm.model.clone())
                    })
                    .flatten(),
                asr_ms,
                polish_ms,
            },
            crate::shared_types::PipelineMode::Multimodal => Self {
                asr_provider: None,
                asr_model: None,
                llm_provider: Some(context.omni.provider_id.clone()),
                llm_model: context.omni.model.clone(),
                asr_ms: None,
                polish_ms,
            },
        }
    }
}

struct CoreVoiceActivationSink {
    events: Arc<EventBus>,
}

impl crate::ports::VoiceActivationSink for CoreVoiceActivationSink {
    fn publish(
        &self,
        signal: crate::voice_activation::VoiceActivationSignal,
    ) -> Result<(), BackendError> {
        use crate::voice_activation::{VoiceActivationPhase, VoiceActivationSignal};
        let (generation, phase, session_id) = match signal {
            VoiceActivationSignal::WakeDetected {
                activation_generation,
            } => (
                activation_generation,
                VoiceActivationPhase::WakeDetected,
                None,
            ),
            VoiceActivationSignal::FinishDetected {
                activation_generation,
                session_id,
            } => (
                activation_generation,
                VoiceActivationPhase::Finishing,
                Some(session_id),
            ),
            VoiceActivationSignal::RuntimeStateChanged {
                activation_generation,
                phase,
            } => (activation_generation, phase, None),
        };
        self.events.publish(
            None,
            BackendEventKind::VoiceActivationStateChanged(
                crate::voice_activation::VoiceActivationStateSnapshot {
                    phase,
                    activation_generation: generation,
                    session_id,
                    message: None,
                },
            ),
        );
        Ok(())
    }
}

fn map_voice_activation_error(error: crate::ports::VoiceActivationError) -> BackendError {
    let (code, message) = match error {
        crate::ports::VoiceActivationError::Unsupported => (
            BackendErrorCode::Unsupported,
            "voice activation is unsupported".into(),
        ),
        crate::ports::VoiceActivationError::ModelMissing => (
            BackendErrorCode::InvalidState,
            "voice activation model is not installed".into(),
        ),
        crate::ports::VoiceActivationError::PermissionDenied => (
            BackendErrorCode::PermissionDenied,
            "microphone permission was denied".into(),
        ),
        crate::ports::VoiceActivationError::MicBusy => (
            BackendErrorCode::Busy,
            "microphone is already in use".into(),
        ),
        crate::ports::VoiceActivationError::InvalidConfig(message)
        | crate::ports::VoiceActivationError::Runtime(message) => {
            (BackendErrorCode::InvalidState, message)
        }
    };
    BackendError::new(code, message)
}

fn voice_activation_failure_phase(
    code: BackendErrorCode,
) -> crate::voice_activation::VoiceActivationPhase {
    match code {
        BackendErrorCode::Unsupported => crate::voice_activation::VoiceActivationPhase::Unavailable,
        BackendErrorCode::PermissionDenied => {
            crate::voice_activation::VoiceActivationPhase::PermissionDenied
        }
        BackendErrorCode::Busy => crate::voice_activation::VoiceActivationPhase::MicBusy,
        _ => crate::voice_activation::VoiceActivationPhase::Failed,
    }
}

impl OpenLessBackend {
    /// Construct an explicitly unavailable Core so the native shell can show
    /// its startup error instead of panicking before any window is visible.
    /// This path never opens the user's files or a credential store.
    pub fn blocked_startup(mut config: BackendConfig, error: BackendError) -> Self {
        // A blocked backend must never touch the caller's failed data path. Use
        // an isolated temporary store solely to satisfy the normal constructor.
        config.data_dir =
            std::env::temp_dir().join(format!("openless-startup-blocked-{}", uuid::Uuid::new_v4()));
        let repositories = BackendRepositories {
            preferences: Arc::new(PreferencesStore::in_memory()),
            history: Arc::new(HistoryStore::at_path(std::path::PathBuf::new())),
            activity: Arc::new(ActivityStore::in_memory()),
            vocabulary: Arc::new(DictionaryStore::at_path(std::path::PathBuf::new())),
            correction_rules: Arc::new(CorrectionRuleStore::at_path(std::path::PathBuf::new())),
            style_packs: Arc::new(StylePackStore::in_memory()),
        };
        let denied = error.clone();
        let guard: crate::domains::RuntimeRestoreGuard = Arc::new(move || Err(denied.clone()));
        // The constructor binds the runtime guard for every runtime entry point.
        // Supply the blocked guard during construction so it is not replaced after
        // the OnceLock-based bindings have already been initialized.
        let mut backend = Self::new_with_repositories_and_clock_and_restore_guard(
            config,
            BackendDependencies::unsupported(),
            repositories,
            Arc::new(SystemClock),
            Some(Arc::clone(&guard)),
        )
        .expect("memory-only blocked backend is constructible");
        backend.startup_error = Some(error);
        backend
    }

    pub fn startup_error(&self) -> Option<BackendError> {
        self.startup_error.clone()
    }

    pub fn bind_restore_runtime_effects(
        &self,
        effects: Arc<dyn crate::config::RestoreRuntimeEffects>,
    ) -> Result<(), BackendError> {
        self.local_migration_store
            .bind_runtime_effects(effects)
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "restore runtime effects binding failed",
                )
            })?;
        Ok(())
    }

    /// Called after a complete journal target has reached its Host effects.
    /// This resets ephemeral interpreters only; it never writes repositories.
    pub fn reset_restored_runtime_preferences(&self) {
        self.hotkey
            .lock()
            .expect("hotkey interpreter lock poisoned")
            .reset();
        self.disarm_edit_observation();
    }

    pub fn new(config: BackendConfig, deps: BackendDependencies) -> Result<Self, BackendError> {
        if config.data_dir.as_os_str().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "data directory is required",
            ));
        }
        let repositories = BackendRepositories::open(&config.data_dir)?;
        Self::new_with_repositories_and_clock(config, deps, repositories, Arc::new(SystemClock))
    }

    pub fn new_with_clock(
        config: BackendConfig,
        deps: BackendDependencies,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, BackendError> {
        if config.data_dir.as_os_str().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "data directory is required",
            ));
        }
        let repositories = BackendRepositories::open(&config.data_dir)?;
        Self::new_with_repositories_and_clock(config, deps, repositories, clock)
    }

    pub fn new_with_repositories(
        config: BackendConfig,
        deps: BackendDependencies,
        repositories: BackendRepositories,
    ) -> Result<Self, BackendError> {
        Self::new_with_repositories_and_clock(config, deps, repositories, Arc::new(SystemClock))
    }

    pub fn new_with_repositories_and_clock(
        config: BackendConfig,
        deps: BackendDependencies,
        repositories: BackendRepositories,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, BackendError> {
        Self::new_with_repositories_and_clock_and_restore_guard(
            config,
            deps,
            repositories,
            clock,
            None,
        )
    }

    fn new_with_repositories_and_clock_and_restore_guard(
        config: BackendConfig,
        mut deps: BackendDependencies,
        repositories: BackendRepositories,
        clock: Arc<dyn Clock>,
        restore_guard: Option<crate::domains::RuntimeRestoreGuard>,
    ) -> Result<Self, BackendError> {
        if config.data_dir.as_os_str().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "data directory is required",
            ));
        }
        let events = Arc::new(EventBus::new(256));
        // 保留裁剪与「最近 N 分钟」必须跟着注入的时钟走：同一个后端里
        // `created_at` 已经用 `clock` 生成，裁剪却偷读系统时钟的话，测试/
        // 回放的固定时钟下刚写入的条目会在写入瞬间被判超期。
        repositories.history.attach_clock(Arc::clone(&clock));
        let preferences_revision = Arc::new(AtomicU64::new(0));
        let style_pack_revision = Arc::new(AtomicU64::new(0));
        let history_revision = Arc::new(AtomicU64::new(0));
        let vocabulary_revision = Arc::new(AtomicU64::new(0));
        let settings_write_gate = Arc::new(Mutex::new(()));
        deps.services.less_computer.bind_testing_features(
            Arc::clone(&repositories.preferences),
            Arc::clone(&settings_write_gate),
        );
        let voice_sessions = Arc::clone(&deps.services.voice_sessions);
        let runtime_start_work = Arc::new(crate::voice_session::RuntimeActivityGate::default());
        deps.services.selection_voice =
            Arc::new(crate::selection_voice_service::SelectionVoiceService::new(
                BackendEventPublisher::new(Arc::clone(&events)),
                Arc::clone(&repositories.preferences),
                Arc::clone(&repositories.history),
                Arc::clone(&history_revision),
                Arc::clone(&clock),
                Arc::clone(&repositories.vocabulary),
                Arc::clone(&vocabulary_revision),
                Arc::clone(&repositories.correction_rules),
                Arc::clone(&repositories.activity),
                Arc::clone(&deps.credential_store),
                Arc::clone(&repositories.style_packs),
                deps.selection_polisher.clone(),
                Arc::clone(&voice_sessions),
            ));
        if let Some(process) = deps.services.take_coding_agent_process() {
            let runner = Arc::new(crate::coding_agent::CodingAgentRunner::new(Arc::clone(
                &process,
            )));
            deps.services.less_computer.bind_runner(Arc::clone(&runner));
            deps.services.coding_agent = Arc::new(crate::coding_agent::CodingAgentService::new(
                runner,
                process,
                Arc::clone(&deps.services.less_computer),
                BackendEventPublisher::new(Arc::clone(&events)),
                Arc::clone(&repositories.preferences),
                Arc::clone(&settings_write_gate),
            ));
        }
        if let Some(runtime) = deps.local_asr_runtime.take() {
            let model_store = match deps.services.model_store.clone() {
                Some(store) => store,
                None => {
                    let store = Arc::new(crate::model_store::ModelStore::new(
                        crate::model_store::ModelStoreConfig::new(config.data_dir.join("models"))?,
                    )?);
                    deps.services.configure_model_store(Arc::clone(&store));
                    store
                }
            };
            let legacy_root = config.data_dir.join("models");
            model_store.migrate_legacy_root(&legacy_root)?;
            let current_root = model_store.models_root_dir();
            if current_root != legacy_root {
                model_store.migrate_legacy_root(&current_root)?;
            }
            let model_events = BackendEventPublisher::new(Arc::clone(&events));
            model_store.set_progress_sink(Arc::new(
                move |progress: crate::ModelDownloadProgress| {
                    model_events.publish(
                        None,
                        BackendEventKind::LocalAsrDownloadProgress(
                            crate::events::LocalAsrDownloadProgress {
                                runtime: match progress.runtime {
                                    crate::LocalAsrRuntime::Generic => {
                                        crate::events::LocalAsrRuntimeKind::Generic
                                    }
                                    crate::LocalAsrRuntime::Foundry => {
                                        crate::events::LocalAsrRuntimeKind::Foundry
                                    }
                                    crate::LocalAsrRuntime::SherpaOnnx => {
                                        crate::events::LocalAsrRuntimeKind::SherpaOnnx
                                    }
                                },
                                model_id: progress.model_id,
                                file: progress.file,
                                file_index: progress.file_index,
                                file_count: progress.file_count,
                                bytes_downloaded: progress.bytes_downloaded,
                                bytes_total: progress.bytes_total,
                                phase: match progress.phase {
                                    crate::ModelDownloadPhase::Started => {
                                        crate::events::LocalAsrDownloadPhase::Started
                                    }
                                    crate::ModelDownloadPhase::Progress => {
                                        crate::events::LocalAsrDownloadPhase::Progress
                                    }
                                    crate::ModelDownloadPhase::Finished => {
                                        crate::events::LocalAsrDownloadPhase::Finished
                                    }
                                    crate::ModelDownloadPhase::Cancelled => {
                                        crate::events::LocalAsrDownloadPhase::Cancelled
                                    }
                                    crate::ModelDownloadPhase::Failed => {
                                        crate::events::LocalAsrDownloadPhase::Failed
                                    }
                                },
                                error: progress.error,
                            },
                        ),
                    );
                },
            ));
            deps.services.local_asr = Arc::new(crate::local_asr_service::LocalAsrService::new(
                Arc::clone(&repositories.preferences),
                runtime,
                model_store,
                config.data_dir.join("models"),
                BackendEventPublisher::new(Arc::clone(&events)),
                Arc::clone(&preferences_revision),
                Arc::clone(&deps.credential_store),
            ));
        }
        let mut encrypted_sync = None;
        let mut encrypted_sync_store = None;
        let cloud_sync = if let Some(mut marketplace_config) = deps.marketplace_config.take() {
            let github_client_id = marketplace_config.github_client_id.clone();
            let sync_config = marketplace_config.encrypted_sync_config.take();
            let marketplace = Arc::new(crate::marketplace::MarketplaceService::new(
                marketplace_config,
                Arc::clone(&deps.credential_store),
                Arc::clone(&repositories.preferences),
                Arc::clone(&repositories.style_packs),
                BackendEventPublisher::new(Arc::clone(&events)),
                Arc::clone(&style_pack_revision),
            )?);
            deps.services.marketplace = marketplace.clone();
            if let Some(sync_config) = sync_config {
                let (service, store) = crate::cloud_sync_e2ee::build(
                    sync_config,
                    &config.data_dir,
                    repositories.clone(),
                    Arc::clone(&deps.credential_store),
                    Arc::clone(&marketplace),
                    github_client_id,
                    BackendEventPublisher::new(Arc::clone(&events)),
                    Arc::clone(&deps.task_spawner),
                )?;
                encrypted_sync = Some(service);
                encrypted_sync_store = Some(store);
            }
            Some(crate::cloud_sync::CloudSyncService::new(
                marketplace,
                repositories.clone(),
                Arc::clone(&settings_write_gate),
                encrypted_sync.is_none(),
            ))
        } else {
            None
        };
        let local_migration_store = if let Some(store) = &encrypted_sync_store {
            Arc::clone(store)
        } else {
            crate::cloud_sync_e2ee::build_local_store(
                &config.data_dir,
                repositories.clone(),
                Arc::clone(&deps.credential_store),
                env!("CARGO_PKG_VERSION").into(),
                Arc::clone(&deps.task_spawner),
            )?
        };
        match (
            deps.selection_runtime.take(),
            deps.selection_polisher.take(),
        ) {
            (Some(runtime), Some(polisher)) => {
                deps.services.selection =
                    Arc::new(crate::selection_service::SelectionService::new(
                        crate::selection_service::SelectionServiceDependencies {
                            preferences: Arc::clone(&repositories.preferences),
                            style_packs: Arc::clone(&repositories.style_packs),
                            runtime,
                            polisher,
                            host_actions: Arc::clone(&deps.host_actions),
                            events: BackendEventPublisher::new(Arc::clone(&events)),
                            history: Arc::clone(&repositories.history),
                            history_revision: Arc::clone(&history_revision),
                            clock: Arc::clone(&clock),
                            vocabulary: Arc::clone(&repositories.vocabulary),
                            vocabulary_revision: Arc::clone(&vocabulary_revision),
                            correction_rules: Arc::clone(&repositories.correction_rules),
                            activity: Arc::clone(&repositories.activity),
                            credential_store: Arc::clone(&deps.credential_store),
                        },
                    ));
            }
            (None, None) => {}
            _ => {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "selection runtime and polisher must be configured together",
                ));
            }
        }
        if let Some(runtime) = deps.qa_runtime.take() {
            deps.services.qa = Arc::new(crate::qa_service::QaService::new_with_persistence(
                runtime,
                Arc::clone(&deps.host_actions),
                crate::qa_service::QaPersistence::new(
                    Arc::clone(&repositories.preferences),
                    Arc::clone(&repositories.history),
                    Arc::clone(&history_revision),
                    Arc::clone(&clock),
                ),
                Arc::clone(&deps.services.selection_voice),
                Arc::clone(&voice_sessions),
            ));
        }
        deps.services
            .selection_voice
            .bind_qa(Arc::downgrade(&deps.services.qa));
        if let Some((polisher, transcription)) = deps.services.take_auxiliary_runtime() {
            deps.services.auxiliary = Arc::new(crate::auxiliary::AuxiliaryService::new(
                Arc::clone(&repositories.preferences),
                Arc::clone(&repositories.style_packs),
                Arc::clone(&repositories.vocabulary),
                Arc::clone(&deps.credential_store),
                polisher,
                transcription,
                Arc::clone(&deps.task_spawner),
            ));
        }
        deps.services
            .qa
            .bind_event_publisher(BackendEventPublisher::new(Arc::clone(&events)));
        deps.services
            .less_computer
            .bind_event_publisher(BackendEventPublisher::new(Arc::clone(&events)));
        deps.services
            .remote_input
            .bind_event_publisher(BackendEventPublisher::new(Arc::clone(&events)));
        {
            let store = &local_migration_store;
            let guard: crate::domains::RuntimeRestoreGuard = restore_guard.unwrap_or_else(|| {
                let weak_store = Arc::downgrade(&local_migration_store);
                Arc::new(move || {
                    let store = weak_store.upgrade().ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidState,
                            "runtime restore source is unavailable",
                        )
                    })?;
                    store.ensure_runtime_available().map_err(|_| {
                        BackendError::new(
                            BackendErrorCode::Busy,
                            "encrypted sync restore must finish before starting runtime work",
                        )
                    })
                })
            });
            voice_sessions.bind_restore_guard(Arc::clone(&guard))?;
            runtime_start_work.bind(Arc::clone(&guard), Arc::clone(&deps.task_spawner))?;
            deps.services
                .qa
                .bind_runtime_restore_guard(Arc::clone(&guard), Arc::clone(&deps.task_spawner))?;
            deps.services
                .selection
                .bind_runtime_restore_guard(Arc::clone(&guard), Arc::clone(&deps.task_spawner))?;
            deps.services
                .selection_voice
                .bind_runtime_restore_guard(guard, Arc::clone(&deps.task_spawner))?;
            let voice = Arc::downgrade(&voice_sessions);
            let starts = Arc::downgrade(&runtime_start_work);
            let qa = Arc::downgrade(&deps.services.qa);
            let selection = Arc::downgrade(&deps.services.selection);
            let selection_voice = Arc::downgrade(&deps.services.selection_voice);
            // Each probe releases its lock before the next one. Starts check
            // Store's already-raised flag while holding the matching lock.
            // Both directions are weak, so neither binding owns the backend.
            store
                .bind_runtime_idle_probe(Arc::new(move || {
                    voice
                        .upgrade()
                        .is_some_and(|gate| gate.runtime_restore_idle())
                        && starts
                            .upgrade()
                            .is_some_and(|gate| gate.runtime_restore_idle())
                        && qa
                            .upgrade()
                            .is_some_and(|service| service.runtime_restore_idle())
                        && selection
                            .upgrade()
                            .is_some_and(|service| service.runtime_restore_idle())
                        && selection_voice
                            .upgrade()
                            .is_some_and(|service| service.runtime_restore_idle())
                }))
                .map_err(|_| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "failed to bind encrypted sync runtime barrier",
                    )
                })?;
        }
        Ok(Self {
            config,
            startup_error: None,
            deps,
            clock,
            events,
            state: Arc::new(RwLock::new(MutableState {
                running: false,
                dictation: DictationStateSnapshot::default(),
                dictation_context: None,
                dictation_start_output_target: None,
                dictation_translation_requested: None,
                dictation_destination_changed: false,
                credentials: CredentialsStatus::default(),
                transcripts: HashMap::new(),
                recording_fault_cleanup: None,
                silence_monitor: None,
            })),
            phase_changed: Arc::new(tokio::sync::Notify::new()),
            hotkey: Mutex::new(crate::hotkey_interpreter::HotkeyInterpreter::default()),
            hotkey_dispatch_gate: tokio::sync::Mutex::new(()),
            pending_audio_captures: Arc::new(Mutex::new(HashMap::new())),
            less_computer_hotkey_press_at: Mutex::new(None),
            vocabulary: repositories.vocabulary,
            correction_rules: repositories.correction_rules,
            vocabulary_revision,
            history: repositories.history,
            history_revision,
            activity: repositories.activity,
            style_packs: repositories.style_packs,
            style_pack_revision,
            preferences: repositories.preferences,
            preferences_revision,
            settings_write_gate,
            local_asr_channel_gate: tokio::sync::Mutex::new(()),
            cloud_sync,
            encrypted_sync,
            encrypted_sync_store,
            local_migration_store,
            pending_corrections: Arc::new(Mutex::new(Vec::new())),
            edit_observation_generation: Arc::new(AtomicU64::new(0)),
            text_insertions: Arc::new(Mutex::new(HashMap::new())),
            voice_sessions,
            runtime_start_work,
            less_computer_voice_controls: Arc::new(Mutex::new(HashMap::new())),
            voice_activation_handle: Arc::new(Mutex::new(None)),
            voice_activation_generation: Arc::new(AtomicU64::new(0)),
            voice_activation_handoff_generation: AtomicU64::new(0),
            voice_activation_configure_gate: tokio::sync::Mutex::new(()),
            backend_lifecycle_gate: tokio::sync::Mutex::new(()),
            backend_shutdown_requested: AtomicBool::new(false),
            voice_activation_session: Arc::new(Mutex::new(None)),
        })
    }

    /// Hosts can inspect settings during recovery, but must defer runtime
    /// startup effects until this succeeds. Task admission itself is also
    /// guarded under Core's runtime locks; this check is not a session lease.
    pub fn ensure_runtime_ready(&self) -> Result<(), BackendError> {
        if let Some(error) = &self.startup_error {
            return Err(error.clone());
        }
        self.local_migration_store
            .ensure_runtime_available()
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Busy,
                    "encrypted sync restore must finish before starting runtime work",
                )
            })?;
        Ok(())
    }

    /// Configure the opt-in local voice activation listener from the current
    /// preference snapshot. Reconfiguration replaces the previous native
    /// handle only after it has been stopped, so one process owns one mic.
    pub async fn configure_voice_activation(&self) -> Result<(), BackendError> {
        let _configure_gate = self.voice_activation_configure_gate.lock().await;
        let preferences = self.get_preferences();
        if self
            .voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .is_some()
            && preferences.voice_activation_enabled
        {
            return Ok(());
        }
        // Invalidate callbacks before awaiting the old runtime. Sherpa may have
        // queued a final callback while its cancellation is being observed.
        let generation = self
            .voice_activation_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        self.voice_activation_handoff_generation
            .store(0, Ordering::Release);
        self.voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .take();
        let previous = self
            .voice_activation_handle
            .lock()
            .expect("voice activation handle lock poisoned")
            .take();
        let had_previous_handle = previous.is_some();
        if let Some(previous) = previous {
            let _ = previous
                .stop(crate::voice_activation::VoiceActivationStopReason::Disabled)
                .await;
        }
        if !preferences.voice_activation_enabled
            || self.backend_shutdown_requested.load(Ordering::Acquire)
            || !self.snapshot().running
        {
            // Do not add a synthetic event to the first startup transition when
            // activation was never enabled. Existing subscribers expect
            // BackendStarted to be the first startup event; later disable/error
            // transitions still publish Disabled below when a visible state exists.
            let should_publish_disabled = had_previous_handle
                || self
                    .events
                    .latest_voice_activation_state()
                    .map(|state| {
                        state.phase != crate::voice_activation::VoiceActivationPhase::Disabled
                    })
                    .unwrap_or(false);
            if should_publish_disabled {
                self.events.publish(
                    None,
                    BackendEventKind::VoiceActivationStateChanged(
                        crate::voice_activation::VoiceActivationStateSnapshot {
                            phase: crate::voice_activation::VoiceActivationPhase::Disabled,
                            activation_generation: generation,
                            session_id: None,
                            message: None,
                        },
                    ),
                );
            }
            return Ok(());
        }
        let Some(runtime) = self.deps.voice_activation_runtime.as_ref() else {
            self.events.publish(
                None,
                BackendEventKind::VoiceActivationStateChanged(
                    crate::voice_activation::VoiceActivationStateSnapshot {
                        phase: crate::voice_activation::VoiceActivationPhase::Unavailable,
                        activation_generation: generation,
                        session_id: None,
                        message: Some("voice activation host runtime is unavailable".into()),
                    },
                ),
            );
            return if preferences.voice_activation_enabled {
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "voice activation host runtime is unavailable",
                ))
            } else {
                Ok(())
            };
        };

        let config = crate::voice_activation::VoiceActivationConfig {
            enabled: preferences.voice_activation_enabled,
            wake_enabled: preferences.voice_activation_wake_enabled,
            finish_enabled: preferences.voice_activation_finish_enabled,
            wake_phrase: preferences.voice_activation_wake_phrase,
            finish_phrase: preferences.voice_activation_finish_phrase,
            timeout_enabled: preferences.voice_activation_timeout_enabled,
            timeout_seconds: preferences.voice_activation_timeout_seconds,
            microphone_device_name: (!preferences.microphone_device_name.trim().is_empty())
                .then_some(preferences.microphone_device_name),
            activation_generation: generation,
        }
        .normalized()
        .map_err(|error| BackendError::new(BackendErrorCode::InvalidArgument, error.to_string()))?;
        self.events.publish(
            None,
            BackendEventKind::VoiceActivationStateChanged(
                crate::voice_activation::VoiceActivationStateSnapshot {
                    phase: crate::voice_activation::VoiceActivationPhase::Starting,
                    activation_generation: generation,
                    session_id: None,
                    message: None,
                },
            ),
        );
        let sink: Arc<dyn crate::ports::VoiceActivationSink> = Arc::new(CoreVoiceActivationSink {
            events: Arc::clone(&self.events),
        });
        match runtime.configure(config, sink).await {
            Ok(handle) => {
                let stale_handle = {
                    let mut slot = self
                        .voice_activation_handle
                        .lock()
                        .expect("voice activation handle lock poisoned");
                    if self.voice_activation_generation.load(Ordering::Acquire) != generation {
                        Some(handle)
                    } else {
                        *slot = Some(handle.into());
                        None
                    }
                };
                if let Some(handle) = stale_handle {
                    let _ = handle
                        .stop(crate::voice_activation::VoiceActivationStopReason::Cancelled)
                        .await;
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "voice activation configuration became stale",
                    ));
                }
                self.events.publish(
                    None,
                    BackendEventKind::VoiceActivationStateChanged(
                        crate::voice_activation::VoiceActivationStateSnapshot {
                            phase: crate::voice_activation::VoiceActivationPhase::ListeningForWake,
                            activation_generation: generation,
                            session_id: None,
                            message: None,
                        },
                    ),
                );
                Ok(())
            }
            Err(error) => {
                let error = map_voice_activation_error(error);
                self.events.publish(
                    None,
                    BackendEventKind::VoiceActivationStateChanged(
                        crate::voice_activation::VoiceActivationStateSnapshot {
                            phase: voice_activation_failure_phase(error.code),
                            activation_generation: generation,
                            session_id: None,
                            message: Some(error.message.clone()),
                        },
                    ),
                );
                Err(error)
            }
        }
    }

    pub async fn stop_voice_activation(
        &self,
        reason: crate::voice_activation::VoiceActivationStopReason,
    ) -> Result<(), BackendError> {
        let _configure_gate = self.voice_activation_configure_gate.lock().await;
        let handle = self
            .voice_activation_handle
            .lock()
            .expect("voice activation handle lock poisoned")
            .take();
        self.voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .take();
        let next_generation = self
            .voice_activation_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        self.voice_activation_handoff_generation
            .store(0, Ordering::Release);
        let should_publish_disabled = handle.is_some()
            || self
                .events
                .latest_voice_activation_state()
                .map(|state| state.phase != crate::voice_activation::VoiceActivationPhase::Disabled)
                .unwrap_or(false);
        if let Some(handle) = handle {
            handle
                .stop(reason)
                .await
                .map_err(map_voice_activation_error)?;
        }
        if should_publish_disabled {
            self.events.publish(
                None,
                BackendEventKind::VoiceActivationStateChanged(
                    crate::voice_activation::VoiceActivationStateSnapshot {
                        phase: crate::voice_activation::VoiceActivationPhase::Disabled,
                        activation_generation: next_generation,
                        session_id: None,
                        message: None,
                    },
                ),
            );
        }
        Ok(())
    }

    pub async fn stop_voice_activation_if_current(
        &self,
        generation: u64,
        reason: crate::voice_activation::VoiceActivationStopReason,
    ) -> Result<bool, BackendError> {
        let _configure_gate = self.voice_activation_configure_gate.lock().await;
        if self.voice_activation_generation.load(Ordering::Acquire) != generation {
            return Ok(false);
        }
        let handle = self
            .voice_activation_handle
            .lock()
            .expect("voice activation handle lock poisoned")
            .take();
        self.voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .take();
        let next_generation = self
            .voice_activation_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        self.voice_activation_handoff_generation
            .store(0, Ordering::Release);
        let should_publish_disabled = handle.is_some()
            || self
                .events
                .latest_voice_activation_state()
                .map(|state| state.phase != crate::voice_activation::VoiceActivationPhase::Disabled)
                .unwrap_or(false);
        if let Some(handle) = handle {
            handle
                .stop(reason)
                .await
                .map_err(map_voice_activation_error)?;
        }
        if should_publish_disabled {
            self.events.publish(
                None,
                BackendEventKind::VoiceActivationStateChanged(
                    crate::voice_activation::VoiceActivationStateSnapshot {
                        phase: crate::voice_activation::VoiceActivationPhase::Disabled,
                        activation_generation: next_generation,
                        session_id: None,
                        message: None,
                    },
                ),
            );
        }
        Ok(true)
    }

    pub fn voice_activation_generation_is_current(&self, generation: u64) -> bool {
        self.voice_activation_generation.load(Ordering::Acquire) == generation
    }

    /// Called by the host runtime after it recognizes a semantic command. The
    /// host callback is synchronous, so the actual handoff/finalization is
    /// owned by Core's task spawner and survives a dropped IPC request.
    pub fn handle_voice_activation_signal(
        self: &Arc<Self>,
        signal: crate::voice_activation::VoiceActivationSignal,
    ) {
        use crate::voice_activation::VoiceActivationSignal;
        let generation = self.voice_activation_generation.load(Ordering::Acquire);
        let signal_generation = match signal {
            VoiceActivationSignal::WakeDetected {
                activation_generation,
            }
            | VoiceActivationSignal::FinishDetected {
                activation_generation,
                ..
            }
            | VoiceActivationSignal::RuntimeStateChanged {
                activation_generation,
                ..
            } => activation_generation,
        };
        if signal_generation != generation {
            return;
        }
        match signal {
            VoiceActivationSignal::WakeDetected { .. } => {
                if self.snapshot().dictation.session_id.is_some()
                    || self
                        .voice_activation_session
                        .lock()
                        .expect("voice activation session lock poisoned")
                        .is_some()
                {
                    return;
                }
                let handle = self
                    .voice_activation_handle
                    .lock()
                    .expect("voice activation handle lock poisoned")
                    .clone();
                let Some(handle) = handle else {
                    return;
                };
                if self
                    .voice_activation_handoff_generation
                    .compare_exchange(0, signal_generation, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }
                let backend = Arc::clone(self);
                self.deps.task_spawner.spawn(Box::pin(async move {
                    let _handoff_guard = VoiceActivationHandoffGuard {
                        backend: Arc::clone(&backend),
                        generation: signal_generation,
                    };
                    if backend
                        .voice_activation_generation
                        .load(Ordering::Acquire)
                        != signal_generation
                    {
                        return;
                    }
                    let handoff = match handle
                        .handoff_to_dictation(
                            crate::ports::VoiceDictationHandoffRequest {
                                activation_generation: signal_generation,
                            },
                        )
                        .await
                    {
                        Ok(handoff) => handoff,
                        Err(error) => {
                            log::warn!("voice activation handoff failed: {error}");
                            match backend
                                .stop_voice_activation_if_current(
                                    signal_generation,
                                    crate::voice_activation::VoiceActivationStopReason::RuntimeFailure,
                                )
                                .await
                            {
                                Ok(true) if backend.get_preferences().voice_activation_enabled => {
                                    let _ = backend.configure_voice_activation().await;
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    log::warn!("voice activation recovery stop failed: {error}");
                                }
                            }
                            return;
                        }
                    };
                    if backend
                        .voice_activation_generation
                        .load(Ordering::Acquire)
                        != signal_generation
                    {
                        let _ = handoff.capture.discard().await;
                        return;
                    }
                    match backend
                        .start_voice_dictation_with_capture(handoff.capture, signal_generation)
                        .await
                    {
                        Ok(session_id) => {
                            if backend
                                .voice_activation_generation
                                .load(Ordering::Acquire)
                                != signal_generation
                            {
                                let _ = backend.cancel_dictation(Some(session_id)).await;
                                return;
                            }
                            let committed = {
                                let _configure_gate =
                                    backend.voice_activation_configure_gate.lock().await;
                                let session_is_current = backend
                                    .state
                                    .read()
                                    .expect("backend state lock poisoned")
                                    .dictation
                                    .session_id
                                    == Some(session_id);
                                if backend.voice_activation_generation.load(Ordering::Acquire)
                                    != signal_generation
                                    || !session_is_current
                                {
                                    false
                                } else {
                                    handle.bind_dictation_session(session_id);
                                    true
                                }
                            };
                            if !committed {
                                let _ = backend.cancel_dictation(Some(session_id)).await;
                                return;
                            }
                            backend.events.publish(
                                None,
                                BackendEventKind::VoiceActivationStateChanged(
                                    crate::voice_activation::VoiceActivationStateSnapshot {
                                        phase: crate::voice_activation::VoiceActivationPhase::ListeningForFinish,
                                        activation_generation: signal_generation,
                                        session_id: Some(session_id),
                                        message: None,
                                    },
                                ),
                            );
                        }
                        Err(error) => {
                            log::warn!("voice dictation start failed: {error}");
                            match backend
                                .stop_voice_activation_if_current(
                                    signal_generation,
                                    crate::voice_activation::VoiceActivationStopReason::RuntimeFailure,
                                )
                                .await
                            {
                                Ok(true) if backend.get_preferences().voice_activation_enabled => {
                                    let _ = backend.configure_voice_activation().await;
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    log::warn!("voice activation recovery stop failed: {error}");
                                }
                            }
                        }
                    }
                }));
            }
            VoiceActivationSignal::FinishDetected {
                session_id,
                activation_generation,
            } => {
                let backend = Arc::clone(self);
                self.deps.task_spawner.spawn(Box::pin(async move {
                    backend
                        .finish_voice_activation(
                            activation_generation,
                            session_id,
                            crate::voice_activation::VoiceActivationStopReason::FinishPhrase,
                        )
                        .await;
                }));
            }
            VoiceActivationSignal::RuntimeStateChanged {
                activation_generation,
                phase: crate::voice_activation::VoiceActivationPhase::Failed,
            } => {
                let backend = Arc::clone(self);
                self.deps.task_spawner.spawn(Box::pin(async move {
                    match backend
                        .stop_voice_activation_if_current(
                            activation_generation,
                            crate::voice_activation::VoiceActivationStopReason::RuntimeFailure,
                        )
                        .await
                    {
                        Ok(true) if backend.get_preferences().voice_activation_enabled => {
                            let _ = backend.configure_voice_activation().await;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            log::warn!("voice activation recovery stop failed: {error}");
                        }
                    }
                }));
            }
            VoiceActivationSignal::RuntimeStateChanged { .. } => {}
        }
    }

    async fn start_voice_dictation_with_capture(
        &self,
        capture: Box<dyn PendingAudioCapture>,
        generation: u64,
    ) -> Result<SessionId, BackendError> {
        if self.voice_activation_generation.load(Ordering::Acquire) != generation {
            let _ = capture.discard().await;
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "voice activation generation is stale",
            ));
        }
        let preferences = self.get_preferences();
        let activation_config = match (crate::voice_activation::VoiceActivationConfig {
            wake_phrase: preferences.voice_activation_wake_phrase.clone(),
            finish_enabled: preferences.voice_activation_finish_enabled,
            finish_phrase: preferences.voice_activation_finish_phrase.clone(),
            timeout_enabled: preferences.voice_activation_timeout_enabled,
            timeout_seconds: preferences.voice_activation_timeout_seconds,
            ..crate::voice_activation::VoiceActivationConfig::default()
        })
        .normalized()
        {
            Ok(config) => config,
            Err(error) => {
                let _ = capture.discard().await;
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    error.to_string(),
                ));
            }
        };
        self.pending_audio_captures
            .lock()
            .expect("pending audio capture lock poisoned")
            .insert(PendingAudioCaptureKey::VoiceActivation(generation), capture);
        let mut reservation = match self.reserve_dictation_session(
            true,
            DictationOutputTarget::ForegroundApp,
            Some(PendingAudioCaptureKey::VoiceActivation(generation)),
        ) {
            Ok(reservation) => reservation,
            Err(error) => {
                self.discard_pending_audio_capture(PendingAudioCaptureKey::VoiceActivation(
                    generation,
                ))
                .await?;
                return Err(error);
            }
        };
        let session_id = reservation.session_id;
        {
            let _configure_gate = self.voice_activation_configure_gate.lock().await;
            if self.voice_activation_generation.load(Ordering::Acquire) != generation {
                if let Some(capture) = reservation.pending_capture.take() {
                    if let Err(error) = capture.discard().await {
                        log::warn!("discard stale voice capture failed: {error}");
                    }
                }
                self.reset_dictation_session(session_id);
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "voice activation generation became stale while starting dictation",
                ));
            }
            *self
                .voice_activation_session
                .lock()
                .expect("voice activation session lock poisoned") =
                Some(crate::voice_activation::VoiceActivationSession {
                    generation: crate::voice_activation::VoiceActivationGeneration(generation),
                    session_id,
                    timeout: activation_config.timeout(),
                    commands: crate::voice_activation::VoiceActivationTextPolicy {
                        wake_phrase: activation_config.wake_phrase.clone(),
                        finish_phrase: activation_config
                            .finish_enabled
                            .then(|| activation_config.finish_phrase.clone()),
                    },
                });
        }
        match self
            .start_reserved_dictation(reservation, DictationStartOptions::default())
            .await
        {
            Ok(session_id) => Ok(session_id),
            Err(error) => {
                let mut session = self
                    .voice_activation_session
                    .lock()
                    .expect("voice activation session lock poisoned");
                if session.as_ref().is_some_and(|session| {
                    session.generation.0 == generation && session.session_id == session_id
                }) {
                    session.take();
                }
                Err(error)
            }
        }
    }

    async fn cleanup_voice_activation_after_dictation(&self, session_id: SessionId) {
        let is_voice_session = self
            .voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .as_ref()
            .is_some_and(|session| session.session_id == session_id);
        if !is_voice_session {
            return;
        }
        let enabled = self.get_preferences().voice_activation_enabled;
        let _ = self
            .stop_voice_activation(crate::voice_activation::VoiceActivationStopReason::Cancelled)
            .await;
        if enabled {
            let _ = self.configure_voice_activation().await;
        }
    }

    async fn finish_voice_activation(
        &self,
        generation: u64,
        session_id: SessionId,
        reason: crate::voice_activation::VoiceActivationStopReason,
    ) {
        let accepted = self
            .voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .as_ref()
            .is_some_and(|session| {
                session.accepts(
                    crate::voice_activation::VoiceActivationGeneration(generation),
                    session_id,
                )
            });
        if !accepted {
            return;
        }
        if self.stop_dictation_session(session_id).await.is_err() {
            return;
        }
        let still_voice_session = self
            .voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .as_ref()
            .is_some_and(|session| session.session_id == session_id);
        if still_voice_session {
            let _ = self.stop_voice_activation(reason).await;
            if self.get_preferences().voice_activation_enabled {
                let _ = self.configure_voice_activation().await;
            }
        }
    }

    pub fn repositories(&self) -> BackendRepositories {
        BackendRepositories {
            preferences: Arc::clone(&self.preferences),
            history: Arc::clone(&self.history),
            activity: Arc::clone(&self.activity),
            vocabulary: Arc::clone(&self.vocabulary),
            correction_rules: Arc::clone(&self.correction_rules),
            style_packs: Arc::clone(&self.style_packs),
        }
    }

    pub fn config(&self) -> &BackendConfig {
        &self.config
    }

    /// Return the versioned domain interfaces used by non-Tauri hosts.
    ///
    /// Each service is an independently replaceable port.  A missing adapter
    /// returns `BackendErrorCode::Unsupported`; callers never need to inspect
    /// the concrete implementation.
    pub fn services(&self) -> &crate::domains::BackendServices {
        &self.deps.services
    }

    fn cloud_sync_service(&self) -> Result<&crate::cloud_sync::CloudSyncService, BackendError> {
        self.cloud_sync.as_ref().ok_or_else(|| {
            BackendError::new(BackendErrorCode::Unsupported, "官方云同步服务尚未配置。")
        })
    }

    /// Read cloud metadata without returning private snapshot contents or GitHub credentials to a UI.
    pub async fn cloud_sync_status(&self) -> Result<crate::CloudSyncStatus, BackendError> {
        self.cloud_sync_service()?.status().await
    }

    pub async fn cloud_sync_upload(
        &self,
        base_revision: u64,
        ui_preferences: crate::CloudSyncUiPreferences,
    ) -> Result<crate::CloudSyncStatus, BackendError> {
        self.cloud_sync_service()?
            .upload(base_revision, ui_preferences)
            .await
    }

    /// The caller must obtain the user's confirmation before replacing local portable data.
    pub async fn cloud_sync_restore(&self) -> Result<crate::CloudSyncRestoreResult, BackendError> {
        let restored = self.cloud_sync_service()?.restore().await?;
        self.publish_preferences_changed();
        self.publish_vocabulary_changed();
        self.publish_style_packs_changed();
        Ok(restored)
    }

    pub async fn cloud_sync_delete(
        &self,
        base_revision: u64,
    ) -> Result<crate::CloudSyncStatus, BackendError> {
        self.cloud_sync_service()?.delete(base_revision).await
    }

    /// Reserve one Less Computer voice-capture session before a host starts
    /// its recorder/native ASR resources.
    ///
    /// Core owns the session lease and cancellation identity; the host owns
    /// only the platform capture handles.  The same `session_id` must later be
    /// passed to [`Self::submit_less_computer_with_session`], or released with
    /// [`Self::abort_less_computer_capture`].
    pub fn begin_less_computer_capture(&self, session_id: SessionId) -> Result<(), BackendError> {
        let _runtime = self.runtime_start_work.acquire()?;
        if !self.get_preferences().less_computer_available() {
            return Err(BackendError::new(
                BackendErrorCode::PermissionDenied,
                "Less Computer is disabled",
            )
            .retryable(false));
        }
        let dictation_phase = self.snapshot().dictation.phase;
        if dictation_phase != DictationPhase::Idle {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "dictation is already active",
            ));
        }
        self.deps.services.less_computer.begin_capture(session_id)
    }

    /// Require this device's explicit opt-in for unfinished features.
    pub fn ensure_testing_features_enabled(&self) -> Result<(), BackendError> {
        crate::settings::ensure_testing_features_enabled(&self.get_preferences())
    }

    /// Return the current Less Computer capture/run session, if any.
    pub fn less_computer_active_session(&self) -> Option<SessionId> {
        self.deps.services.less_computer.active_session()
    }

    /// A host capture is invalid after cancellation, release, replacement or
    /// promotion to an Agent run; only its matching live capture may continue.
    pub fn less_computer_capture_cancelled(&self, session_id: SessionId) -> bool {
        self.deps
            .services
            .less_computer
            .capture_cancelled(session_id)
    }

    /// Release a capture lease that did not reach Agent submission.
    pub fn abort_less_computer_capture(&self, session_id: SessionId) -> Result<(), BackendError> {
        self.deps.services.less_computer.abort_capture(session_id)
    }

    /// Start a Core-owned voice session. The host only needs to feed PCM and
    /// call `finish`/`cancel`; all provider and Agent policy is snapshotted here.
    pub async fn start_less_computer_voice(
        &self,
        session_id: SessionId,
        recording_control: Arc<dyn crate::ports::RecordingControlSink>,
    ) -> Result<LessComputerVoiceSession, BackendError> {
        self.start_less_computer_voice_with(
            session_id,
            recording_control,
            LessComputerVoiceOptions::default(),
        )
        .await
    }

    /// Same as [`Self::start_less_computer_voice`], with an explicit delivery
    /// mode and start-error surface (panel requests report errors inline).
    pub async fn start_less_computer_voice_with(
        &self,
        session_id: SessionId,
        recording_control: Arc<dyn crate::ports::RecordingControlSink>,
        options: LessComputerVoiceOptions,
    ) -> Result<LessComputerVoiceSession, BackendError> {
        let _runtime = self.runtime_start_work.acquire()?;
        let preferences = self.get_preferences();
        if !preferences.less_computer_available() {
            return Err(BackendError::new(
                BackendErrorCode::PermissionDenied,
                "Less Computer is disabled",
            )
            .retryable(false));
        }
        if self.snapshot().dictation.phase != DictationPhase::Idle {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "dictation is already active",
            ));
        }
        self.deps.services.less_computer.begin_capture(session_id)?;
        let resources = self.voice_sessions.hold_resources(session_id)?;
        let feedback = Arc::new(LessVoiceFeedback::new(
            self.event_publisher(),
            session_id,
            options.mode,
            crate::events::LessComputerVoicePhase::Starting,
        ));
        feedback.phase(crate::events::LessComputerVoicePhase::Starting);
        let feedback_guard = LessVoiceFeedbackGuard(Arc::clone(&feedback));
        let result = async {
            // Reserve before the first await so Esc can revoke even a cold start.
            let ensure_capture = || {
                if self.less_computer_capture_cancelled(session_id) {
                    let _ = self.deps.services.less_computer.abort_capture(session_id);
                    return Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "Less Computer voice session was cancelled while starting",
                    ));
                }
                Ok(())
            };
            let qa = self.deps.services.qa.snapshot().await;
            ensure_capture()?;
            if qa.is_ok_and(|snapshot| {
                matches!(
                    snapshot.phase,
                    crate::domains::QaPhase::Recording
                        | crate::domains::QaPhase::Thinking
                        | crate::domains::QaPhase::AwaitingApproval
                )
            }) {
                let _ = self.deps.services.less_computer.abort_capture(session_id);
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "QA voice session is already active",
                ));
            }
            let selection_voice = self.deps.services.selection_voice.snapshot().await;
            ensure_capture()?;
            if selection_voice.is_ok_and(|snapshot| {
                matches!(
                    snapshot.phase,
                    crate::domains::SelectionVoicePhase::Recording
                        | crate::domains::SelectionVoicePhase::Processing
                        | crate::domains::SelectionVoicePhase::AwaitingIntent
                        | crate::domains::SelectionVoicePhase::Applying
                )
            }) {
                let _ = self.deps.services.less_computer.abort_capture(session_id);
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "selection voice session is already active",
                ));
            }
            if let Err(error) = self.deps.host_actions.request(HostAction::ShowLessComputer) {
                let _ = self.deps.services.less_computer.abort_capture(session_id);
                return Err(error);
            }
            let context = match self
                .capture_dictation_context(
                    &DictationStartOptions::default(),
                    DictationContextPurpose::AuxiliaryVoice,
                )
                .await
            {
                Ok(context) => Arc::new(context),
                Err(error) => {
                    let _ = self.deps.services.less_computer.abort_capture(session_id);
                    return Err(error);
                }
            };
            ensure_capture()?;
            let partials = Arc::new(VoiceTranscriptSink {
                publisher: self.event_publisher(),
                session_id,
                transcript: Mutex::new(crate::types::TranscriptAccumulator::default()),
                feedback: Some(Arc::clone(&feedback)),
            });
            let started_at = std::time::Instant::now();
            let recording_progress = Arc::new(LessComputerRecordingProgress {
                session_id,
                less_computer: Arc::clone(&self.deps.services.less_computer),
                control: recording_control,
                task_spawner: Arc::clone(&self.deps.task_spawner),
                started_at,
                silence: Mutex::new(context.recording.silence_after_ms.map(|silence_ms| {
                    crate::silence_auto_stop::SilenceAutoStop::new(
                        std::time::Duration::from_millis(silence_ms),
                        started_at,
                    )
                })),
                feedback: Arc::clone(&feedback),
            });
            let omni_finish = AuxiliaryOmniFinish {
                credentials: Arc::clone(&self.deps.credential_store),
                context: Arc::clone(&context),
                system_prompt: crate::prompts::auxiliary_voice_omni_instruction_prompt(),
            };
            let (transcription, recording, pcm, omni) = if context.pipeline_mode
                == crate::shared_types::PipelineMode::Multimodal
            {
                let audio_capture = own_voice_start(
                    &self.deps.task_spawner,
                    Arc::clone(&resources),
                    self.deps.dictation_engine.start_audio_capture(
                        session_id,
                        Arc::clone(&context),
                        Arc::clone(&recording_progress)
                            as Arc<dyn crate::ports::RecordingProgressSink>,
                        resources.cancel.clone(),
                    ),
                    |capture| {
                        Box::pin(async move { stop_and_discard_recording(capture.recording).await })
                    },
                )
                .await;
                match audio_capture {
                    Ok(capture) => (
                        None,
                        Some(capture.recording),
                        Some(capture.pcm),
                        Some(omni_finish),
                    ),
                    Err(error) if error.code == BackendErrorCode::Unsupported => {
                        // Host/external PCM feed path (same seam as ASR Unsupported).
                        ensure_capture()?;
                        (
                            None,
                            None,
                            Some(Arc::new(CapturedPcm::default())),
                            Some(omni_finish),
                        )
                    }
                    Err(error) => {
                        let _ = self.deps.services.less_computer.abort_capture(session_id);
                        return Err(error);
                    }
                }
            } else {
                let voice_capture = own_voice_start(
                    &self.deps.task_spawner,
                    Arc::clone(&resources),
                    self.deps.dictation_engine.start_voice_capture(
                        session_id,
                        Arc::clone(&context),
                        Arc::clone(&partials) as Arc<dyn TextStreamSink>,
                        Arc::clone(&recording_progress)
                            as Arc<dyn crate::ports::RecordingProgressSink>,
                        resources.cancel.clone(),
                    ),
                    discard_voice_capture,
                )
                .await;
                match voice_capture {
                    Ok(capture) => (
                        Some(capture.transcription),
                        Some(capture.recording),
                        None,
                        None,
                    ),
                    Err(error) if error.code == BackendErrorCode::Unsupported => {
                        ensure_capture()?;
                        match own_voice_start(
                            &self.deps.task_spawner,
                            Arc::clone(&resources),
                            Arc::clone(&self.deps.dictation_engine)
                                .start_transcription_with_progress(
                                    Arc::clone(&self.deps.task_spawner),
                                    session_id,
                                    Arc::clone(&context),
                                    Arc::clone(&partials) as Arc<dyn TextStreamSink>,
                                    Arc::clone(&recording_progress)
                                        as Arc<dyn crate::ports::RecordingProgressSink>,
                                ),
                            |transcription| transcription.cancel(),
                        )
                        .await
                        {
                            Ok(session) => (Some(session), None, None, None),
                            Err(error) => {
                                let _ = self.deps.services.less_computer.abort_capture(session_id);
                                return Err(error);
                            }
                        }
                    }
                    Err(error) => {
                        let _ = self.deps.services.less_computer.abort_capture(session_id);
                        return Err(error);
                    }
                }
            };
            let request = if self.less_computer_capture_cancelled(session_id) {
                Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Less Computer voice session was cancelled while starting",
                ))
            } else {
                self.build_less_computer_request(session_id, String::new(), &preferences)
            };
            let request = match request {
                Ok(request) => request,
                Err(error) => {
                    let less_computer = Arc::clone(&self.deps.services.less_computer);
                    let transcription = transcription.clone();
                    let _ = own_voice_effect(
                        &self.deps.task_spawner,
                        Box::pin(async move {
                            let _resources = resources;
                            if let Some(recording) = recording {
                                let _ = recording.stop().await;
                            }
                            if let Some(transcription) = transcription {
                                let _ = transcription.cancel().await;
                            }
                            less_computer.abort_capture(session_id)
                        }),
                    )
                    .await;
                    return Err(error);
                }
            };
            let control = Arc::new(VoiceCaptureControl {
                transcription,
                pcm,
                omni,
                recording: Mutex::new(recording),
                closed: std::sync::atomic::AtomicBool::new(false),
                resources: Mutex::new(Some(resources)),
                task_spawner: Arc::clone(&self.deps.task_spawner),
                feedback: Mutex::new(Some(feedback_guard)),
            });
            self.less_computer_voice_controls
                .lock()
                .expect("voice control lock poisoned")
                .insert(session_id, Arc::clone(&control));
            if let Err(error) = ensure_capture() {
                let _guard = VoiceControlGuard {
                    session_id,
                    control: Arc::clone(&control),
                    controls: Arc::clone(&self.less_computer_voice_controls),
                };
                let _ = control.cancel_resources().await;
                return Err(error);
            }
            Ok(LessComputerVoiceSession {
                session_id,
                control,
                controls: Arc::clone(&self.less_computer_voice_controls),
                less_computer: Arc::clone(&self.deps.services.less_computer),
                request,
                partials,
                archive_successful_recording: context.recording.archive_successful_recording,
                mode: options.mode,
                feedback: Arc::clone(&feedback),
            })
        }
        .await;
        if let Err(error) = &result {
            if options.publish_start_error && error.code != BackendErrorCode::Cancelled {
                self.event_publisher().publish(
                    Some(session_id),
                    BackendEventKind::LessComputerEvent(crate::events::LessComputerEvent {
                        seq: None,
                        kind: crate::events::LessComputerEventKind::Error {
                            message: error.message.clone(),
                        },
                    }),
                );
            }
        }
        result
    }

    pub async fn start_selection_voice_capture(
        &self,
        session_id: SessionId,
        control: Arc<dyn crate::ports::RecordingControlSink>,
    ) -> Result<VoiceTranscriptionSession, BackendError> {
        let resources = self.voice_sessions.hold_resources(session_id)?;
        self.deps
            .services
            .selection_voice
            .bind_recording_control(session_id, Arc::clone(&control))?;
        if self.snapshot().dictation.phase != DictationPhase::Idle
            || self.less_computer_active_session().is_some()
        {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "another voice session is already active",
            ));
        }
        let snapshot = self.deps.services.selection_voice.snapshot().await?;
        if snapshot.session_id != Some(session_id)
            || snapshot.phase != crate::domains::SelectionVoicePhase::Recording
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "selection voice session is not recording",
            ));
        }
        let mut context = self
            .capture_dictation_context(
                &DictationStartOptions::default(),
                DictationContextPurpose::AuxiliaryVoice,
            )
            .await?;
        context.recording.archive_enabled = false;
        let context = Arc::new(context);
        let started_at = std::time::Instant::now();
        let silence = context.recording.silence_after_ms.map(|silence_ms| {
            crate::silence_auto_stop::SilenceAutoStop::new(
                std::time::Duration::from_millis(silence_ms),
                started_at,
            )
        });
        let partials = Arc::new(VoiceTranscriptSink {
            publisher: self.event_publisher(),
            session_id,
            transcript: Mutex::new(crate::types::TranscriptAccumulator::default()),
            feedback: None,
        });
        let recording_progress = Arc::new(SelectionVoiceRecordingProgress {
            events: self.event_publisher(),
            session_id,
            selection_voice: Arc::clone(&self.deps.services.selection_voice),
            control,
            task_spawner: Arc::clone(&self.deps.task_spawner),
            started_at,
            silence: Mutex::new(silence),
        });
        if context.pipeline_mode == crate::shared_types::PipelineMode::Multimodal {
            let capture = own_voice_start(
                &self.deps.task_spawner,
                Arc::clone(&resources),
                self.deps.dictation_engine.start_audio_capture(
                    session_id,
                    Arc::clone(&context),
                    Arc::clone(&recording_progress) as Arc<dyn crate::ports::RecordingProgressSink>,
                    resources.cancel.clone(),
                ),
                |capture| {
                    Box::pin(async move { stop_and_discard_recording(capture.recording).await })
                },
            )
            .await?;
            Ok(VoiceTranscriptionSession {
                session_id,
                transcription: None,
                pcm: Some(capture.pcm),
                omni: Some(AuxiliaryOmniFinish {
                    credentials: Arc::clone(&self.deps.credential_store),
                    context,
                    system_prompt: crate::prompts::auxiliary_voice_omni_instruction_prompt(),
                }),
                recording: Mutex::new(Some(capture.recording)),
                partials,
                lifecycle: Arc::new(VoiceCaptureLifecycle::with_resources(resources)),
                task_spawner: Arc::clone(&self.deps.task_spawner),
            })
        } else {
            let capture = own_voice_start(
                &self.deps.task_spawner,
                Arc::clone(&resources),
                self.deps.dictation_engine.start_voice_capture(
                    session_id,
                    Arc::clone(&context),
                    Arc::clone(&partials) as Arc<dyn TextStreamSink>,
                    Arc::clone(&recording_progress) as Arc<dyn crate::ports::RecordingProgressSink>,
                    resources.cancel.clone(),
                ),
                discard_voice_capture,
            )
            .await?;
            Ok(VoiceTranscriptionSession {
                session_id,
                transcription: Some(capture.transcription),
                pcm: None,
                omni: None,
                recording: Mutex::new(Some(capture.recording)),
                partials,
                lifecycle: Arc::new(VoiceCaptureLifecycle::with_resources(resources)),
                task_spawner: Arc::clone(&self.deps.task_spawner),
            })
        }
    }

    #[doc(hidden)]
    pub async fn start_qa_voice_capture(
        &self,
        session_id: SessionId,
        options: DictationStartOptions,
        progress: Arc<dyn crate::ports::RecordingProgressSink>,
    ) -> Result<QaVoiceCaptureSession, BackendError> {
        let resources = self.voice_sessions.hold_resources(session_id)?;
        let mut context = self
            .capture_dictation_context(&options, DictationContextPurpose::QaVoice)
            .await?;
        // QA audio is private and memory-only in both ASR and Omni modes,
        // independent of the main dictation debug-recording preference.
        context.recording.archive_enabled = false;
        let context = Arc::new(context);
        let started_at = std::time::Instant::now();
        let recording_progress = Arc::new(QaRecordingProgress {
            session_id,
            qa: Arc::clone(&self.deps.services.qa),
            progress,
            task_spawner: Arc::clone(&self.deps.task_spawner),
            started_at,
            silence: Mutex::new(context.recording.silence_after_ms.map(|silence_ms| {
                crate::silence_auto_stop::SilenceAutoStop::new(
                    std::time::Duration::from_millis(silence_ms),
                    started_at,
                )
            })),
            terminal: Mutex::new(QaRecordingTerminalState::default()),
        });
        let progress =
            Arc::clone(&recording_progress) as Arc<dyn crate::ports::RecordingProgressSink>;
        if context.pipeline_mode == crate::shared_types::PipelineMode::Multimodal {
            let capture = own_voice_start(
                &self.deps.task_spawner,
                Arc::clone(&resources),
                self.deps.dictation_engine.start_audio_capture(
                    session_id,
                    Arc::clone(&context),
                    progress,
                    resources.cancel.clone(),
                ),
                |capture| {
                    Box::pin(async move { stop_and_discard_recording(capture.recording).await })
                },
            )
            .await?;
            Ok(QaVoiceCaptureSession {
                context,
                recording: Mutex::new(Some(capture.recording)),
                transcription: None,
                pcm: Some(capture.pcm),
                recording_progress,
                lifecycle: Arc::new(VoiceCaptureLifecycle::with_resources(resources)),
                task_spawner: Arc::clone(&self.deps.task_spawner),
            })
        } else {
            let capture = own_voice_start(
                &self.deps.task_spawner,
                Arc::clone(&resources),
                self.deps.dictation_engine.start_voice_capture(
                    session_id,
                    Arc::clone(&context),
                    Arc::new(DiscardTextStream),
                    progress,
                    resources.cancel.clone(),
                ),
                discard_voice_capture,
            )
            .await?;
            Ok(QaVoiceCaptureSession {
                context,
                recording: Mutex::new(Some(capture.recording)),
                transcription: Some(capture.transcription),
                pcm: None,
                recording_progress,
                lifecycle: Arc::new(VoiceCaptureLifecycle::with_resources(resources)),
                task_spawner: Arc::clone(&self.deps.task_spawner),
            })
        }
    }

    /// Interpret Less Computer hotkey edges with the same Hold/Toggle/Auto
    /// preference used by the other voice entry points.
    pub fn dispatch_less_computer_hotkey_edge(
        &self,
        edge: DictationHotkeyEdge,
    ) -> LessComputerHotkeyAction {
        use crate::shared_types::HotkeyMode;

        const AUTO_HOLD_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(350);
        let mode = self.get_preferences().hotkey.mode;
        let active = self.less_computer_active_session().is_some();
        match edge {
            DictationHotkeyEdge::Combined { .. } => {
                let created_by_press = self
                    .less_computer_hotkey_press_at
                    .lock()
                    .expect("Less Computer hotkey timestamp lock poisoned")
                    .take()
                    .is_some();
                if active && created_by_press {
                    LessComputerHotkeyAction::Cancel
                } else {
                    LessComputerHotkeyAction::Noop
                }
            }
            DictationHotkeyEdge::Pressed { at, .. } if !active => {
                *self
                    .less_computer_hotkey_press_at
                    .lock()
                    .expect("Less Computer hotkey timestamp lock poisoned") = Some(at);
                LessComputerHotkeyAction::Start
            }
            DictationHotkeyEdge::Pressed { .. } => match mode {
                HotkeyMode::Toggle | HotkeyMode::DoubleClick | HotkeyMode::Auto => {
                    *self
                        .less_computer_hotkey_press_at
                        .lock()
                        .expect("Less Computer hotkey timestamp lock poisoned") = None;
                    LessComputerHotkeyAction::Finish
                }
                HotkeyMode::Hold => LessComputerHotkeyAction::Noop,
            },
            DictationHotkeyEdge::Released { at, .. } => {
                let pressed_at = self
                    .less_computer_hotkey_press_at
                    .lock()
                    .expect("Less Computer hotkey timestamp lock poisoned")
                    .take();
                match mode {
                    HotkeyMode::Hold if active => LessComputerHotkeyAction::Finish,
                    HotkeyMode::Auto
                        if active
                            && pressed_at.is_some_and(|pressed| {
                                at.saturating_duration_since(pressed) >= AUTO_HOLD_THRESHOLD
                            }) =>
                    {
                        LessComputerHotkeyAction::Finish
                    }
                    HotkeyMode::Toggle
                    | HotkeyMode::DoubleClick
                    | HotkeyMode::Auto
                    | HotkeyMode::Hold => LessComputerHotkeyAction::Noop,
                }
            }
        }
    }

    /// Run one Less Computer turn using the preferences snapshot owned by
    /// Core. Hosts pass only user text; provider, model, permission, workdir,
    /// continuation and guard policy are resolved here before reaching the
    /// native runtime Adapter.
    pub async fn submit_less_computer(
        &self,
        transcript: String,
    ) -> Result<LessComputerRunResult, BackendError> {
        self.submit_less_computer_with_session(SessionId::new(), transcript)
            .await
    }

    /// Run one Less Computer turn with a host-owned session identifier.
    ///
    /// Audio-capable hosts use this overload so a physical hotkey release or
    /// Esc can cancel the same Core run that owns the transcript.  The host
    /// still supplies no provider policy; all preferences and safety rules are
    /// resolved here exactly as in [`Self::submit_less_computer`].
    pub async fn submit_less_computer_with_session(
        &self,
        session_id: SessionId,
        transcript: String,
    ) -> Result<LessComputerRunResult, BackendError> {
        let _runtime = self.runtime_start_work.acquire()?;
        let preferences = self.get_preferences();
        if !preferences.less_computer_available() {
            return Err(BackendError::new(
                BackendErrorCode::PermissionDenied,
                "Less Computer is disabled",
            )
            .retryable(false));
        }
        self.deps
            .host_actions
            .request(HostAction::ShowLessComputer)?;
        let request = self.build_less_computer_request(session_id, transcript, &preferences)?;
        self.deps.services.less_computer.submit(request).await
    }

    fn build_less_computer_request(
        &self,
        session_id: SessionId,
        transcript: String,
        preferences: &UserPreferences,
    ) -> Result<LessComputerRunRequest, BackendError> {
        let provider = CodingAgentProvider::from_pref(&preferences.coding_agent_provider);
        Ok(LessComputerRunRequest {
            session_id,
            transcript,
            provider,
            executable: Some(normalize_coding_agent_executable(
                provider,
                preferences.coding_agent_exe.clone(),
            )?),
            model: resolve_coding_agent_model(provider, preferences.coding_agent_model.clone()),
            permission_mode: normalize_less_computer_permission_mode(
                provider,
                &preferences.coding_agent_permission_mode,
            ),
            workdir: normalize_coding_agent_workdir(
                preferences.coding_agent_workdir.clone(),
                self.config.home_dir.clone(),
            )?,
            continue_session: false,
            continuation_context: None,
            approved_patterns: Vec::new(),
        })
    }

    /// Cancel a Less Computer run through its instance-local Core state.
    pub async fn cancel_less_computer(
        &self,
        session_id: Option<SessionId>,
    ) -> Result<(), BackendError> {
        let Some(session_id) = session_id.or_else(|| self.less_computer_active_session()) else {
            return Ok(());
        };
        own_voice_effect(
            &self.deps.task_spawner,
            self.less_computer_cancellation(session_id),
        )
        .await
    }

    fn less_computer_cancellation(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let control = self
            .less_computer_voice_controls
            .lock()
            .expect("voice control lock poisoned")
            .get(&session_id)
            .cloned();
        let controls = Arc::clone(&self.less_computer_voice_controls);
        let less_computer = Arc::clone(&self.deps.services.less_computer);
        Box::pin(async move {
            let service_result = less_computer.cancel(Some(session_id)).await;
            let resource_result = match control {
                Some(control) => {
                    let _guard = VoiceControlGuard {
                        session_id,
                        control: Arc::clone(&control),
                        controls,
                    };
                    control.cancel_resources().await
                }
                None => Ok(()),
            };
            let _ = less_computer.abort_capture(session_id);
            resource_result.and(service_result)
        })
    }

    pub async fn cancel_active_voice_session(
        &self,
        expected_session_id: Option<SessionId>,
    ) -> Result<(), BackendError> {
        if let Some(session_id) = self.snapshot().dictation.session_id {
            if expected_session_id.is_some_and(|expected| expected != session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "voice cancellation targets a different session",
                ));
            }
            return self.cancel_dictation(Some(session_id)).await;
        }

        if let Some(session_id) = self.less_computer_active_session() {
            if expected_session_id.is_some_and(|expected| expected != session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "voice cancellation targets a different session",
                ));
            }
            return self.cancel_less_computer(Some(session_id)).await;
        }

        if self
            .deps
            .services
            .qa
            .snapshot()
            .await
            .is_ok_and(|snapshot| {
                matches!(
                    snapshot.phase,
                    crate::domains::QaPhase::Recording
                        | crate::domains::QaPhase::Thinking
                        | crate::domains::QaPhase::AwaitingApproval
                )
            })
        {
            return self.deps.services.qa.cancel(expected_session_id).await;
        }
        if self
            .deps
            .services
            .selection_voice
            .snapshot()
            .await
            .is_ok_and(|snapshot| snapshot.phase != crate::domains::SelectionVoicePhase::Idle)
        {
            return self
                .deps
                .services
                .selection_voice
                .cancel(expected_session_id)
                .await;
        }
        Err(BackendError::new(
            BackendErrorCode::InvalidState,
            "no active voice session",
        ))
    }

    /// Capture the immutable response-provider/preferences snapshot for a QA
    /// text turn. ASR is not required; QA voice captures both stages through
    /// `start_qa_voice_capture`. This is an adapter seam, not a UI use-case.
    #[doc(hidden)]
    pub async fn capture_host_dictation_context(
        &self,
        options: DictationStartOptions,
    ) -> Result<Arc<DictationContext>, BackendError> {
        Ok(Arc::new(
            self.capture_dictation_context(&options, DictationContextPurpose::QaText)
                .await?,
        ))
    }

    pub fn subscribe(&self) -> EventSubscription {
        self.events.subscribe()
    }

    /// Replay the bounded instance-local event tail after `sequence`.
    ///
    /// Hosts use this after a cold UI mount or a lag notification. A truncated
    /// result means the caller must rebuild its complete view model from the
    /// current snapshots before applying the returned tail.
    pub fn replay_events_after(&self, sequence: u64) -> EventReplay {
        self.events.replay_after(sequence)
    }

    /// Return a typed publisher for platform/transport Adapters that need to
    /// report progress or capability changes on the backend event stream.
    pub fn event_publisher(&self) -> BackendEventPublisher {
        BackendEventPublisher::new(Arc::clone(&self.events))
    }

    pub fn request_host_action(&self, action: HostAction) -> Result<(), BackendError> {
        self.deps.host_actions.request(action)
    }

    fn engine_progress_sink(&self) -> Arc<dyn EngineProgressSink> {
        Arc::new(BackendEngineProgress {
            events: Arc::clone(&self.events),
            state: Arc::clone(&self.state),
            phase_changed: Arc::clone(&self.phase_changed),
            text_insertions: Arc::clone(&self.text_insertions),
            raw_transcripts: Mutex::new(HashMap::new()),
        })
    }

    pub async fn start(&self) -> Result<StartupSnapshot, BackendError> {
        let _lifecycle_gate = self.backend_lifecycle_gate.lock().await;
        self.backend_shutdown_requested
            .store(false, Ordering::Release);
        if let Some(error) = &self.startup_error {
            return Err(error.clone());
        }
        // Native status warms only through the already-bound sync gate. A
        // pending journal therefore uses the Host's read-only loader before
        // startup recovery/auto-sync tries to capture credentials.
        if !self.snapshot().running {
            self.recover_cloud_notes()?;
        }
        let before = self.get_preferences();
        let warmed = self.deps.credential_store.status(before).await;
        if let Some(sync) = &self.encrypted_sync {
            // Keep Settings available even after a denied/failed warm read.
            // The service records its own retryable recovery/storage error;
            // runtime admission remains fenced until recovery is complete.
            let _ = sync.start(Arc::clone(&self.deps.task_spawner)).await;
        } else {
            // Local migration has the same crash journal as cloud sync, but it
            // has no service startup hook when the user is signed out.
            let _ = self.local_migration_store.recover_registered().await;
        }
        let preferences = self.get_preferences();
        let status = match warmed {
            // Recovery can replace both preferences and credentials. Recompute
            // from the new in-memory target after it has settled.
            Ok(_) if self.encrypted_sync.is_some() => {
                self.deps.credential_store.status(preferences.clone()).await
            }
            Ok(credentials) => Ok(credentials),
            Err(error) => Err(error),
        };
        let credentials = match status {
            Ok(credentials) => credentials,
            Err(error) => {
                log::warn!(
                    "[core] startup credential status unavailable: {:?}",
                    error.code
                );
                CredentialsStatus {
                    pipeline_mode: crate::shared_types::effective_pipeline_mode(
                        preferences.multimodal_pipeline_enabled,
                        preferences.pipeline_mode,
                    ),
                    ..CredentialsStatus::default()
                }
            }
        };
        let snapshot = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            state.credentials = credentials;
            if state.running {
                return Ok(StartupSnapshot {
                    contract_version: crate::BACKEND_CONTRACT_VERSION.to_string(),
                    backend: BackendSnapshot {
                        running: true,
                        dictation: state.dictation.clone(),
                        vocabulary_revision: self.vocabulary_revision.load(Ordering::Acquire),
                        history_revision: self.history_revision.load(Ordering::Acquire),
                        style_pack_revision: self.style_pack_revision.load(Ordering::Acquire),
                        preferences_revision: self.preferences_revision.load(Ordering::Acquire),
                        credentials: state.credentials.clone(),
                    },
                });
            }
            state.running = true;
            self.events.publish(None, BackendEventKind::BackendStarted);
            StartupSnapshot {
                contract_version: crate::BACKEND_CONTRACT_VERSION.to_string(),
                backend: BackendSnapshot {
                    running: true,
                    dictation: state.dictation.clone(),
                    vocabulary_revision: self.vocabulary_revision.load(Ordering::Acquire),
                    history_revision: self.history_revision.load(Ordering::Acquire),
                    style_pack_revision: self.style_pack_revision.load(Ordering::Acquire),
                    preferences_revision: self.preferences_revision.load(Ordering::Acquire),
                    credentials: state.credentials.clone(),
                },
            }
        };
        if let Err(error) = self.configure_voice_activation().await {
            log::warn!("[voice-activation] startup configuration failed: {error}");
        }
        Ok(snapshot)
    }

    pub async fn shutdown(&self) -> Result<(), BackendError> {
        let _lifecycle_gate = self.backend_lifecycle_gate.lock().await;
        self.backend_shutdown_requested
            .store(true, Ordering::Release);
        let _ = self
            .stop_voice_activation(crate::voice_activation::VoiceActivationStopReason::Disabled)
            .await;
        if let Some(sync) = &self.encrypted_sync {
            sync.shutdown().await;
        }
        let (active_session, preserve_quick_note, was_running) = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            if !state.running {
                (None, false, false)
            } else {
                let active_session = state.dictation.session_id;
                let preserve_quick_note = active_session.is_some()
                    && state.dictation_context.as_ref().is_some_and(|context| {
                        context.output_target == DictationOutputTarget::QuickNote
                    });
                self.events.publish(None, BackendEventKind::BackendStopping);
                if active_session.is_some() {
                    state.dictation.phase = DictationPhase::Cancelled;
                    self.events.publish(
                        state.dictation.session_id,
                        BackendEventKind::DictationStateChanged(state.dictation.clone()),
                    );
                }
                state.running = false;
                state.dictation = DictationStateSnapshot::default();
                state.dictation_context = None;
                state.dictation_start_output_target = None;
                state.silence_monitor = None;
                state.transcripts.clear();
                self.phase_changed.notify_waiters();
                (active_session, preserve_quick_note, true)
            }
        };
        let pending_captures = std::mem::take(
            &mut *self
                .pending_audio_captures
                .lock()
                .expect("pending audio capture lock poisoned"),
        );
        for capture in pending_captures.into_values() {
            let _ = capture.discard().await;
        }
        if !was_running {
            return Ok(());
        }
        // Mirror cancel_dictation: settle provisional recording drafts before
        // adapter teardown so shutdown cannot leave orphan `recording` rows
        // (including when no abandoned starter remains to reconcile them).
        if let Some(session_id) = active_session {
            if preserve_quick_note {
                if let Ok(entries) = self.list_history() {
                    if let Some(mut entry) = entries
                        .into_iter()
                        .find(|entry| entry.id == session_id.to_string())
                    {
                        if entry.error_code.as_deref() == Some("recording") {
                            entry.error_code = Some("cancelled".to_string());
                            entry.has_audio_recording = Some(true);
                            let _ = self.update_history_entry(entry);
                        }
                    }
                }
            } else {
                self.remove_recording_draft(session_id);
            }
        }
        self.disarm_edit_observation();
        // Start native teardown before awaiting unrelated domain shutdown.
        // This also retains a cleanup hold if the shutdown caller disappears.
        let dictation_cleanup =
            active_session.map(|session_id| self.cancel_session_adapters(session_id));
        let selection_result = match self.deps.services.selection.snapshot().await {
            Ok(snapshot)
                if matches!(
                    snapshot.phase,
                    crate::domains::SelectionPhase::Capturing
                        | crate::domains::SelectionPhase::Preview
                        | crate::domains::SelectionPhase::Applying
                ) =>
            {
                self.deps
                    .services
                    .selection
                    .cancel(snapshot.session_id)
                    .await
            }
            Ok(_) => Ok(()),
            Err(error) if error.code == BackendErrorCode::Unsupported => Ok(()),
            Err(error) => Err(error),
        };
        let selection_voice_result = match self.deps.services.selection_voice.snapshot().await {
            Ok(snapshot)
                if matches!(
                    snapshot.phase,
                    crate::domains::SelectionVoicePhase::Recording
                        | crate::domains::SelectionVoicePhase::Processing
                        | crate::domains::SelectionVoicePhase::AwaitingIntent
                        | crate::domains::SelectionVoicePhase::Preview
                        | crate::domains::SelectionVoicePhase::Applying
                ) =>
            {
                self.deps
                    .services
                    .selection_voice
                    .cancel(snapshot.session_id)
                    .await
            }
            Ok(_) => Ok(()),
            Err(error) if error.code == BackendErrorCode::Unsupported => Ok(()),
            Err(error) => Err(error),
        };
        let qa_result = match self.deps.services.qa.snapshot().await {
            Ok(snapshot)
                if matches!(
                    snapshot.phase,
                    crate::domains::QaPhase::Recording
                        | crate::domains::QaPhase::Thinking
                        | crate::domains::QaPhase::AwaitingApproval
                ) =>
            {
                self.deps.services.qa.cancel(snapshot.session_id).await
            }
            Ok(_) => Ok(()),
            Err(error) if error.code == BackendErrorCode::Unsupported => Ok(()),
            Err(error) => Err(error),
        };
        let remote_input_result = match self.deps.services.remote_input.status() {
            Ok(status) if status.enabled || status.running => {
                self.deps
                    .services
                    .remote_input
                    .configure(crate::domains::RemoteInputConfig {
                        enabled: false,
                        port: status.port,
                    })
                    .await
            }
            Ok(_) => Ok(()),
            Err(error) if error.code == BackendErrorCode::Unsupported => Ok(()),
            Err(error) => Err(error),
        };
        if let Some(cleanup) = dictation_cleanup {
            let cancel_result = cleanup.await;
            let host_result = active_session
                .map(|session_id| self.hide_dictation_feedback(session_id))
                .unwrap_or(Ok(()));
            cancel_result?;
            host_result?;
        }
        selection_result?;
        selection_voice_result?;
        qa_result?;
        self.cancel_less_computer(None).await?;
        self.deps.services.less_computer.dismiss();
        self.dismiss_pending_corrections();
        remote_input_result
    }

    pub fn snapshot(&self) -> BackendSnapshot {
        let state = self.state.read().expect("backend state lock poisoned");
        BackendSnapshot {
            running: state.running,
            dictation: state.dictation.clone(),
            vocabulary_revision: self.vocabulary_revision.load(Ordering::Acquire),
            history_revision: self.history_revision.load(Ordering::Acquire),
            style_pack_revision: self.style_pack_revision.load(Ordering::Acquire),
            preferences_revision: self.preferences_revision.load(Ordering::Acquire),
            credentials: state.credentials.clone(),
        }
    }

    pub fn latest_voice_activation_state(
        &self,
    ) -> Option<crate::voice_activation::VoiceActivationStateSnapshot> {
        self.events.latest_voice_activation_state()
    }

    pub fn dictation_output_target(&self) -> Option<DictationOutputTarget> {
        let state = self.state.read().expect("backend state lock poisoned");
        state
            .dictation_context
            .as_ref()
            .map(|context| context.output_target)
            .or(state.dictation_start_output_target)
    }

    /// Dispatch a launcher/single-instance intent through the same state
    /// machine and domain Interfaces used by normal host calls.
    pub async fn dispatch_cli_intent(
        &self,
        intent: crate::cli::CliIntent,
    ) -> Result<CliDispatchOutcome, BackendError> {
        match intent {
            crate::cli::CliIntent::ToggleDictation => match self.snapshot().dictation.phase {
                DictationPhase::Idle => self
                    .start_dictation()
                    .await
                    .map(CliDispatchOutcome::DictationStarted),
                DictationPhase::Starting | DictationPhase::Recording => self
                    .stop_dictation()
                    .await
                    .map(CliDispatchOutcome::DictationCompleted),
                DictationPhase::Transcribing
                | DictationPhase::Polishing
                | DictationPhase::Inserting
                | DictationPhase::Completed
                | DictationPhase::Cancelled
                | DictationPhase::Failed => Ok(CliDispatchOutcome::Noop),
            },
            crate::cli::CliIntent::ToggleQa => {
                let qa = &self.deps.services.qa;
                let snapshot = qa.snapshot().await?;
                match (snapshot.phase, snapshot.session_id) {
                    (crate::QaPhase::Recording, Some(session_id)) => qa.stop_recording(session_id).await?,
                    (crate::QaPhase::Thinking | crate::QaPhase::AwaitingApproval, _) => return Ok(CliDispatchOutcome::Noop),
                    _ => qa.start_question().await?,
                }
                Ok(CliDispatchOutcome::QaToggled)
            }
            crate::cli::CliIntent::CancelDictation => {
                if let Some(session_id) = self.snapshot().dictation.session_id {
                    self.cancel_dictation(Some(session_id)).await?;
                } else if let Some(session_id) = self.less_computer_active_session() {
                    // 1.x voice_agent shared the dictation cancellation scope.
                    // Keep the separate QA domain outside this launcher intent.
                    self.cancel_less_computer(Some(session_id)).await?;
                } else {
                    return Ok(CliDispatchOutcome::Noop);
                }
                Ok(CliDispatchOutcome::DictationCancelled)
            }
        }
    }

    fn arm_pending_audio_capture(
        &self,
        press_id: u64,
        pressed_at: std::time::Instant,
        audio_source: DictationAudioSource,
    ) -> bool {
        if audio_source != DictationAudioSource::Microphone {
            return false;
        }
        if self
            .state
            .read()
            .expect("backend state lock poisoned")
            .dictation
            .session_id
            .is_some()
        {
            return false;
        }
        let preferences = self.get_preferences();
        let request = CaptureStartRequest {
            press_id,
            pressed_at,
            microphone_device_name: (!preferences.microphone_device_name.trim().is_empty())
                .then(|| preferences.microphone_device_name.clone()),
            low_latency: preferences.low_latency_dictation_enabled,
            pre_roll_ms: 500,
            audio_processing: crate::audio::AudioProcessingConfig {
                enhancement_enabled: preferences.audio_enhancement_enabled,
                dsp: preferences.audio_dsp,
            },
        };
        match self.deps.dictation_engine.arm_audio_capture(request) {
            Ok(capture) => {
                let replaced = self
                    .pending_audio_captures
                    .lock()
                    .expect("pending audio capture lock poisoned")
                    .insert(PendingAudioCaptureKey::Hotkey(press_id), capture);
                if let Some(replaced) = replaced {
                    self.deps.task_spawner.spawn(Box::pin(async move {
                        if let Err(error) = replaced.discard().await {
                            log::warn!("failed to discard replaced provisional capture: {error}");
                        }
                    }));
                }
                true
            }
            Err(error) if error.code == BackendErrorCode::Unsupported => false,
            Err(error) => {
                log::warn!("provisional dictation capture unavailable: {error}");
                false
            }
        }
    }

    fn take_pending_audio_capture(
        &self,
        key: PendingAudioCaptureKey,
    ) -> Option<Box<dyn PendingAudioCapture>> {
        self.pending_audio_captures
            .lock()
            .expect("pending audio capture lock poisoned")
            .remove(&key)
    }

    async fn discard_pending_audio_capture(
        &self,
        key: PendingAudioCaptureKey,
    ) -> Result<(), BackendError> {
        let capture = self.take_pending_audio_capture(key);
        match capture {
            Some(capture) => capture.discard().await,
            None => Ok(()),
        }
    }

    fn schedule_pending_audio_capture_discard(
        &self,
        capture: Option<Box<dyn PendingAudioCapture>>,
    ) {
        let Some(capture) = capture else {
            return;
        };
        self.deps.task_spawner.spawn(Box::pin(async move {
            if let Err(error) = capture.discard().await {
                log::warn!("failed to discard provisional dictation capture: {error}");
            }
        }));
    }

    /// Apply physical dictation-key edges using the shared hotkey-mode rules.
    ///
    /// Native listeners provide a stable physical-press id plus monotonic event
    /// timestamps; Core owns deduplication, arbitration, duration and cooldown.
    pub async fn dispatch_dictation_hotkey_edge(
        &self,
        edge: DictationHotkeyEdge,
    ) -> Result<CliDispatchOutcome, BackendError> {
        self.dispatch_dictation_hotkey_edge_with_session_options(
            edge,
            DictationHotkeyDispatchOptions::default(),
        )
        .await
    }

    /// Apply a physical dictation-key edge with host-captured start options.
    ///
    /// This is primarily used by native hotkey adapters that receive a
    /// translation modifier before the dictation press. The options are only
    /// consumed when this edge actually starts a new session.
    pub async fn dispatch_dictation_hotkey_edge_with_options(
        &self,
        edge: DictationHotkeyEdge,
        options: DictationStartOptions,
    ) -> Result<CliDispatchOutcome, BackendError> {
        self.dispatch_dictation_hotkey_edge_with_session_options(
            edge,
            DictationHotkeyDispatchOptions {
                start: options,
                stop: DictationStopOptions::default(),
            },
        )
        .await
    }

    /// Apply a physical hotkey edge with host-captured start and stop options.
    ///
    /// Start options are consumed only when the edge creates a session. Stop
    /// options are consumed only when it finalizes one. This preserves desktop
    /// translation-modifier semantics without making the host duplicate the
    /// Toggle/Hold/Auto state machine.
    pub async fn dispatch_dictation_hotkey_edge_with_session_options(
        &self,
        edge: DictationHotkeyEdge,
        options: DictationHotkeyDispatchOptions,
    ) -> Result<CliDispatchOutcome, BackendError> {
        use crate::hotkey_interpreter::HotkeyIntent;

        // Pressed and Released stay FIFO for toggle-like modes even though start/finalize await
        // native work. Hold-mode release deliberately bypasses the gate so a physical key-up can
        // cancel native startup instead of waiting for the recorder to become live. Combined also
        // bypasses this gate because it has a dedicated low-latency host bridge.
        let preferences = self.get_preferences();
        let mode = preferences.hotkey.mode;
        let bypass_dispatch_gate = matches!(edge, DictationHotkeyEdge::Combined { .. })
            || (matches!(edge, DictationHotkeyEdge::Released { .. })
                && matches!(mode, crate::shared_types::HotkeyMode::Hold));
        let _dispatch_guard = if bypass_dispatch_gate {
            None
        } else {
            Some(self.hotkey_dispatch_gate.lock().await)
        };
        let modifier_only =
            crate::shortcut_types::is_modifier_chord_binding(&preferences.dictation_hotkey)
                || crate::hotkey_interpreter::modifier_arbitration_required(
                    crate::shortcut_types::legacy_modifier_trigger(&preferences.dictation_hotkey),
                    mode,
                );
        let intent = {
            let mut hotkey = self
                .hotkey
                .lock()
                .expect("hotkey interpreter lock poisoned");
            let phase = self.snapshot().dictation.phase;
            match edge {
                DictationHotkeyEdge::Pressed { press_id, at } => {
                    hotkey.press(press_id, at, mode, phase, modifier_only)
                }
                DictationHotkeyEdge::Released { press_id, at } => {
                    hotkey.release(press_id, at, mode, phase)
                }
                DictationHotkeyEdge::Combined { press_id, at: _ } => hotkey.combined(press_id),
            }
        };
        let _pending_capture_guard = if let DictationHotkeyEdge::Pressed { press_id, at } = edge {
            if matches!(
                intent,
                HotkeyIntent::Start { .. } | HotkeyIntent::WaitForModifierGrace { .. }
            ) && options.start.audio_source == DictationAudioSource::Microphone
                && self.arm_pending_audio_capture(press_id, at, options.start.audio_source)
            {
                Some(PendingAudioCaptureAbortGuard {
                    captures: Arc::clone(&self.pending_audio_captures),
                    task_spawner: Arc::clone(&self.deps.task_spawner),
                    key: PendingAudioCaptureKey::Hotkey(press_id),
                })
            } else {
                None
            }
        } else {
            None
        };
        if let DictationHotkeyEdge::Combined { press_id, .. } = edge {
            let _ = self
                .discard_pending_audio_capture(PendingAudioCaptureKey::Hotkey(press_id))
                .await;
        }
        let (intent, reservation) = if let HotkeyIntent::WaitForModifierGrace { press_id } = intent
        {
            // Only modifier-only triggers pay this delay. The separate Combined
            // bridge can mark the same generation while this task is sleeping.
            tokio::time::sleep(
                crate::hotkey_interpreter::HotkeyInterpreter::MODIFIER_ARBITRATION_GRACE,
            )
            .await;
            let (intent, reservation) = {
                let mut hotkey = self
                    .hotkey
                    .lock()
                    .expect("hotkey interpreter lock poisoned");
                let intent = hotkey.after_modifier_grace(press_id, self.snapshot().dictation.phase);
                let reservation = if matches!(intent, HotkeyIntent::Start { .. }) {
                    Some(self.reserve_dictation_session(
                        options.start.insert_text,
                        options.start.output_target,
                        Some(PendingAudioCaptureKey::Hotkey(press_id)),
                    ))
                } else {
                    None
                };
                (intent, reservation)
            };
            if !matches!(intent, HotkeyIntent::Start { .. }) {
                let _ = self
                    .discard_pending_audio_capture(PendingAudioCaptureKey::Hotkey(press_id))
                    .await;
            }
            (intent, reservation)
        } else {
            match intent {
                HotkeyIntent::Start { press_id } => {
                    let reservation = {
                        let hotkey = self
                            .hotkey
                            .lock()
                            .expect("hotkey interpreter lock poisoned");
                        hotkey.start_is_current(press_id).then(|| {
                            self.reserve_dictation_session(
                                options.start.insert_text,
                                options.start.output_target,
                                Some(PendingAudioCaptureKey::Hotkey(press_id)),
                            )
                        })
                    };
                    if reservation.is_some() {
                        (intent, reservation)
                    } else {
                        let _ = self
                            .discard_pending_audio_capture(PendingAudioCaptureKey::Hotkey(press_id))
                            .await;
                        self.hotkey
                            .lock()
                            .expect("hotkey interpreter lock poisoned")
                            .combo_cancelled(press_id);
                        (HotkeyIntent::Noop, None)
                    }
                }
                _ => (intent, None),
            }
        };

        match intent {
            HotkeyIntent::Noop | HotkeyIntent::WaitForModifierGrace { .. } => {
                Ok(CliDispatchOutcome::Noop)
            }
            HotkeyIntent::Start { press_id } => {
                let result = match reservation.expect("Start intent must claim its session") {
                    Ok(reservation) => {
                        self.start_reserved_dictation(reservation, options.start)
                            .await
                    }
                    Err(error) => {
                        let _ = self
                            .discard_pending_audio_capture(PendingAudioCaptureKey::Hotkey(press_id))
                            .await;
                        Err(error)
                    }
                };
                // Microphone and ASR startup can take long enough for Combined
                // to overtake this task. Re-check before reporting a live session.
                let combined = self
                    .hotkey
                    .lock()
                    .expect("hotkey interpreter lock poisoned")
                    .start_finished(press_id, result.is_ok());
                match result {
                    Ok(session_id) if combined => {
                        self.cancel_dictation(Some(session_id)).await?;
                        self.hotkey
                            .lock()
                            .expect("hotkey interpreter lock poisoned")
                            .combo_cancelled(press_id);
                        Ok(CliDispatchOutcome::DictationCancelled)
                    }
                    Ok(session_id) => Ok(CliDispatchOutcome::DictationStarted(session_id)),
                    Err(error) => {
                        if combined {
                            self.hotkey
                                .lock()
                                .expect("hotkey interpreter lock poisoned")
                                .combo_cancelled(press_id);
                        }
                        Err(error)
                    }
                }
            }
            HotkeyIntent::Stop => self
                .stop_dictation_with_options(options.stop)
                .await
                .map(CliDispatchOutcome::DictationCompleted),
            HotkeyIntent::Cancel { press_id } => {
                let active = self.snapshot().dictation.session_id;
                if active.is_none() {
                    return Ok(CliDispatchOutcome::Noop);
                }
                self.cancel_dictation(active).await?;
                self.hotkey
                    .lock()
                    .expect("hotkey interpreter lock poisoned")
                    .combo_cancelled(press_id);
                Ok(CliDispatchOutcome::DictationCancelled)
            }
        }
    }

    /// Transfer the recording-key prefix into a fresh voice question. Serialize
    /// with physical dictation edges so a delayed prefix release cannot restart
    /// dictation after QA has acquired the microphone.
    pub async fn start_voice_question_from_recording_key(&self) -> Result<(), BackendError> {
        let _dispatch = self.hotkey_dispatch_gate.lock().await;
        let snapshot = self.snapshot().dictation;
        if let Some(session_id) = snapshot.session_id {
            if !matches!(snapshot.phase, DictationPhase::Starting | DictationPhase::Recording) {
                return Err(BackendError::new(BackendErrorCode::Busy, "dictation is finalizing"));
            }
            if let Err(error) = self.cancel_dictation(Some(session_id)).await {
                // The independent combo-abort bridge may have claimed this same
                // prefix already. Its native cleanup still owns the voice lease.
                if self.snapshot().dictation.session_id.is_some()
                    || error.code != BackendErrorCode::InvalidState { return Err(error); }
            }
        }
        let provisional = std::mem::take(&mut *self.pending_audio_captures.lock()
            .expect("pending audio capture lock poisoned"));
        for capture in provisional.into_values() { capture.discard().await?; }
        self.hotkey.lock().expect("hotkey interpreter lock poisoned").reset();
        // Logical cancellation can precede the final native cleanup hold. Never
        // cancel another domain or open a second microphone during that interval.
        for _ in 0..25 {
            if self.voice_sessions.runtime_restore_idle() {
                return self.deps.services.qa.start_question().await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Err(BackendError::new(BackendErrorCode::Busy, "microphone is still being released"))
    }

    /// Accept the only mutable dictation intent into the current session. The
    /// engine only uses it during final polish, so synchronize at the common
    /// stop boundary instead of racing engine registration during Starting.
    pub async fn update_dictation_translation_requested(
        &self,
        requested: bool,
    ) -> Result<(), BackendError> {
        let preferences = self.get_preferences();
        {
            let mut state = self.state.write().expect("backend state lock poisoned");
            ensure_running(&state)?;
            if !matches!(
                state.dictation.phase,
                DictationPhase::Starting | DictationPhase::Recording
            ) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "dictation translation can only change before finalization",
                ));
            }
            let session_id = state.dictation.session_id.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "no active dictation session",
                )
            })?;
            state.dictation_translation_requested = Some(requested);
            let effective = if let Some(context) = state.dictation_context.as_ref() {
                let updated = Arc::new(context.with_translation_requested(requested));
                let effective = updated.polish.translation_active;
                state.dictation_context = Some(updated);
                effective
            } else {
                // Startup has not frozen its context yet. Retain the raw intent
                // above; capture completion resolves it against the frozen
                // target/working languages, rather than losing this key edge.
                crate::shared_types::translation_effective(
                    requested,
                    &preferences.translation_target_language,
                    &preferences.working_languages,
                )
            };
            if state.dictation.translation_active != effective {
                state.dictation.translation_active = effective;
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::DictationStateChanged(state.dictation.clone()),
                );
            }
        }
        Ok(())
    }

    pub fn dictation_translation_target(&self) -> Option<DictationTranslationTarget> {
        let state = self.state.read().expect("backend state lock poisoned");
        let context = state.dictation_context.as_ref()?;
        Some(DictationTranslationTarget {
            session_id: state.dictation.session_id?,
            target_language: context.polish.translation_target_language.clone(),
            working_languages: context.polish.working_languages.clone(),
            mutable: state.running
                && state.dictation.phase == DictationPhase::Recording
                && context.polish.translation_active,
        })
    }

    /// Explicitly retarget this recording and remember the choice. Ordinary settings
    /// edits still leave an in-flight recording's frozen context untouched.
    pub fn select_dictation_translation_target(
        &self,
        session_id: SessionId,
        target_language: String,
    ) -> Result<DictationTranslationTarget, BackendError> {
        let target = crate::language_catalog::native_language_name(&target_language)
            .ok_or_else(|| BackendError::new(BackendErrorCode::InvalidArgument, "unsupported translation language"))?;
        let _write_guard = self.settings_write_gate.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self.state.write().expect("backend state lock poisoned");
        ensure_running(&state)?;
        if state.dictation.session_id != Some(session_id)
            || state.dictation.phase != DictationPhase::Recording
        {
            return Err(BackendError::new(BackendErrorCode::InvalidState,
                "translation picker belongs to a recording that has ended"));
        }
        let context = state.dictation_context.as_ref().ok_or_else(||
            BackendError::new(BackendErrorCode::InvalidState, "recording context is not ready"))?;
        if !context.polish.translation_active
            || !crate::shared_types::translation_effective(true, &target, &context.polish.working_languages)
        {
            return Err(BackendError::new(BackendErrorCode::InvalidArgument,
                "translation is inactive or the target matches the only working language"));
        }
        let mut updated = (**context).clone();
        updated.polish.translation_target_language = target.clone();
        let result = DictationTranslationTarget {
            session_id,
            target_language: target.clone(),
            working_languages: updated.polish.working_languages.clone(),
            mutable: true,
        };
        // Keep finalization outside this critical section. A failed save changes neither
        // the active target nor the remembered choice, and no other preference is overwritten.
        self.preferences.update(|preferences| preferences.translation_target_language = target)?;
        state.dictation_context = Some(Arc::new(updated));
        // The common stop boundary must also send the changed context to a streaming engine.
        state.dictation_translation_requested = Some(true);
        drop(state);
        self.publish_preferences_changed();
        Ok(result)
    }

    pub fn siwc_service(&self) -> Result<Arc<crate::siwc::SiwcService>, BackendError> {
        self.deps
            .credential_store
            .siwc()
            .ok_or_else(|| crate::siwc::error("unavailable").into_backend())
    }

    pub async fn siwc_for_channel(
        &self,
        channel: &str,
    ) -> Result<Arc<crate::siwc::SiwcService>, BackendError> {
        if !self
            .list_channels(ChannelKind::Llm)
            .await?
            .iter()
            .any(|c| c.id == channel && c.provider_type == crate::siwc::PROVIDER_ID)
        {
            return Err(crate::siwc::error("channel_missing").into_backend());
        }
        self.siwc_service()
    }

    pub async fn begin_siwc_sign_in(&self, channel: String, new_account: bool, enable_usage: bool) -> Result<crate::siwc::SiwcLaunch, BackendError> {
        let service = self.siwc_for_channel(&channel).await?;
        let launch = service.begin_sign_in(channel.clone(), new_account, enable_usage).await.map_err(crate::siwc::SiwcError::into_backend)?;
        // Storage/browser preparation can race channel deletion or a provider switch.
        let current = self.siwc_for_channel(&channel).await;
        if current.is_ok() && service.authorizing(&channel, &launch.attempt_id) { return Ok(launch); }
        service.cancel_attempt(&channel, &launch.attempt_id).await.map_err(crate::siwc::SiwcError::into_backend)?;
        current?;
        Err(crate::siwc::error("cancelled").into_backend())
    }

    async fn with_siwc_status(
        &self,
        mut status: CredentialsStatus,
    ) -> Result<CredentialsStatus, BackendError> {
        if let Some(service) = self.deps.credential_store.siwc() {
            let channel_id = self
                .deps
                .credential_store
                .active_provider(crate::ProviderSlot::Llm)
                .await?;
            let channel = &channel_id;
            if self
                .list_channels(ChannelKind::Llm)
                .await?
                .iter()
                .any(|c| &c.id == channel && c.provider_type == crate::siwc::PROVIDER_ID)
            {
                let session = service.status(channel).await.map_err(crate::siwc::SiwcError::into_backend)?;
                let model = self
                    .deps
                    .credential_store
                    .read(CredentialKey::new(
                        crate::CredentialNamespace::Llm,
                        Some(channel.clone()),
                        crate::credentials::LLM_MODEL_ACCOUNT,
                    )?)
                    .await?;
                status.llm_configured =
                    matches!(session.state.as_str(), "connected" | "usage_limited")
                        && model.is_some_and(|m| !m.expose_secret().trim().is_empty());
                status.ark_configured = status.llm_configured;
            }
        }
        Ok(status)
    }

    pub async fn get_credentials_status(&self) -> Result<CredentialsStatus, BackendError> {
        let status = self
            .deps
            .credential_store
            .status(self.get_preferences())
            .await?;
        let status = self.with_siwc_status(status).await?;
        self.state
            .write()
            .expect("backend state lock poisoned")
            .credentials = status.clone();
        Ok(status)
    }

    pub async fn read_credential(
        &self,
        key: CredentialKey,
    ) -> Result<Option<SecretValue>, BackendError> {
        self.deps.credential_store.read(key).await
    }

    pub async fn set_credential(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> Result<CredentialsStatus, BackendError> {
        if key.namespace == crate::CredentialNamespace::Llm
            && crate::llm_protocol::CONFIG_ACCOUNTS.contains(&key.account.as_str())
        {
            crate::llm_protocol::LlmProtocolConfig::default()
                .apply(&key.account, value.expose_secret())?;
        }
        let invalidate = if key.namespace == crate::CredentialNamespace::Llm {
            let id = match &key.provider_id {
                Some(id) => id.clone(),
                None => {
                    self.deps
                        .credential_store
                        .active_provider(crate::ProviderSlot::Llm)
                        .await?
                }
            };
            self.list_channels(ChannelKind::Llm)
                .await?
                .into_iter()
                .find(|channel| channel.id == id)
                .map(|_| id)
        } else {
            None
        };
        self.deps.credential_store.write(key, value).await?;
        if let Some(id) = invalidate {
            self.deps
                .credential_store
                .mutate_channel(ChannelMutation::InvalidateTest {
                    kind: ChannelKind::Llm,
                    id,
                })
                .await?;
        }
        self.refresh_and_publish_credentials().await
    }

    pub async fn remove_credential(
        &self,
        key: CredentialKey,
    ) -> Result<CredentialsStatus, BackendError> {
        self.deps.credential_store.remove(key).await?;
        self.refresh_and_publish_credentials().await
    }

    pub async fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> Result<Vec<ChannelSummary>, BackendError> {
        self.deps.credential_store.list_channels(kind).await
    }

    pub async fn create_channel(
        &self,
        kind: ChannelKind,
        provider_type: String,
        name: String,
    ) -> Result<String, BackendError> {
        match self
            .apply_channel_mutation(ChannelMutation::Create {
                kind,
                provider_type,
                name,
            })
            .await?
        {
            ChannelMutationResult::Created(id) => Ok(id),
            _ => Err(BackendError::new(
                BackendErrorCode::Internal,
                "credential store returned an invalid create-channel result",
            )),
        }
    }

    pub async fn set_channel_provider_type(
        &self,
        kind: ChannelKind,
        id: String,
        provider_type: String,
    ) -> Result<(), BackendError> {
        let provider_type = provider_type.trim().to_string();
        if provider_type.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "provider type must not be blank",
            ));
        }
        if kind == ChannelKind::Asr {
            return self
                .apply_channel_mutation(ChannelMutation::SetProviderType {
                    kind,
                    id,
                    provider_type,
                })
                .await
                .map(|_| ());
        }
        let previous = self
            .list_channels(kind)
            .await?
            .into_iter()
            .find(|channel| channel.id == id)
            .ok_or_else(|| {
                BackendError::new(BackendErrorCode::InvalidArgument, "unknown channel")
            })?;
        let key = CredentialKey::new(
            crate::CredentialNamespace::Llm,
            Some(id.clone()),
            crate::llm_protocol::REQUEST_FORMAT_ACCOUNT,
        )?;
        let service_tier_key = CredentialKey::new(
            crate::CredentialNamespace::Llm,
            Some(id.clone()),
            crate::llm_protocol::SERVICE_TIER_ACCOUNT,
        )?;
        let leaving_siwc = previous.provider_type == crate::siwc::PROVIDER_ID
            && previous.provider_type != provider_type;
        if leaving_siwc {
            self.siwc_service()?
                .cancel_sign_in(&id)
                .await
                .map_err(crate::siwc::SiwcError::into_backend)?;
        }
        let siwc_channel = id.clone();
        let reset = kind == ChannelKind::Llm && previous.provider_type != provider_type;
        let (old_format, old_service_tier) = if reset {
            let old_format = self.deps.credential_store.read(key.clone()).await?;
            let old_service_tier = self
                .deps
                .credential_store
                .read(service_tier_key.clone())
                .await?;
            self.deps.credential_store.remove(key.clone()).await?;
            if let Err(error) = self
                .deps
                .credential_store
                .remove(service_tier_key.clone())
                .await
            {
                if let Some(value) = old_format.clone() {
                    self.deps.credential_store.write(key.clone(), value).await?;
                }
                return Err(error);
            }
            (old_format, old_service_tier)
        } else {
            (None, None)
        };
        let result = self
            .deps
            .credential_store
            .mutate_channel(ChannelMutation::SetProviderType {
                kind,
                id,
                provider_type,
            })
            .await
            .map(|_| ());
        if result.is_err() {
            if let Some(value) = old_format {
                self.deps.credential_store.write(key, value).await?;
            }
            if let Some(value) = old_service_tier {
                self.deps
                    .credential_store
                    .write(service_tier_key, value)
                    .await?;
            }
        }
        result?;
        if leaving_siwc {
            self.siwc_service()?
                .detach(siwc_channel)
                .await
                .map_err(crate::siwc::SiwcError::into_backend)?;
        }
        self.refresh_and_publish_credentials().await.map(|_| ())
    }

    pub async fn delete_channel_if_blank(
        &self,
        kind: ChannelKind,
        id: String,
    ) -> Result<bool, BackendError> {
        if kind == ChannelKind::Llm
            && self
                .list_channels(kind)
                .await?
                .iter()
                .any(|c| c.id == id && c.provider_type == crate::siwc::PROVIDER_ID)
        {
            if let Some(service) = self.deps.credential_store.siwc() {
                if service.has_binding(&id).await.map_err(crate::siwc::SiwcError::into_backend)? {
                    return Ok(false);
                }
            }
        }
        match self
            .apply_channel_mutation(ChannelMutation::DeleteIfBlank { kind, id })
            .await?
        {
            ChannelMutationResult::DeletedIfBlank(deleted) => Ok(deleted),
            _ => Err(BackendError::new(
                BackendErrorCode::Internal,
                "credential store returned an invalid draft-cleanup result",
            )),
        }
    }

    pub async fn rename_channel(
        &self,
        kind: ChannelKind,
        id: String,
        name: String,
    ) -> Result<(), BackendError> {
        self.apply_channel_mutation(ChannelMutation::Rename { kind, id, name })
            .await
            .map(|_| ())
    }

    pub async fn delete_channel(&self, kind: ChannelKind, id: String) -> Result<(), BackendError> {
        let is_siwc = kind == ChannelKind::Llm
            && self
                .list_channels(kind)
                .await?
                .iter()
                .any(|c| c.id == id && c.provider_type == crate::siwc::PROVIDER_ID);
        let service = self.deps.credential_store.siwc().filter(|_| is_siwc);
        if let Some(service) = &service {
            service
                .cancel_sign_in(&id)
                .await
                .map_err(crate::siwc::SiwcError::into_backend)?;
        }
        self.apply_channel_mutation(ChannelMutation::Delete {
            kind,
            id: id.clone(),
        })
        .await?;
        if let Some(service) = service {
            service.detach(id).await.map_err(crate::siwc::SiwcError::into_backend)?;
        }
        Ok(())
    }

    pub async fn set_channel_enabled(
        &self,
        kind: ChannelKind,
        id: String,
        enabled: bool,
    ) -> Result<(), BackendError> {
        self.apply_channel_mutation(ChannelMutation::SetEnabled { kind, id, enabled })
            .await
            .map(|_| ())
    }

    pub async fn reorder_channels(
        &self,
        kind: ChannelKind,
        ids: Vec<String>,
    ) -> Result<(), BackendError> {
        self.apply_channel_mutation(ChannelMutation::Reorder { kind, ids })
            .await
            .map(|_| ())
    }

    pub async fn record_channel_test(
        &self,
        kind: ChannelKind,
        id: String,
        ok: bool,
        latency_ms: Option<u32>,
        error: Option<String>,
    ) -> Result<(), BackendError> {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        self.apply_channel_mutation(ChannelMutation::RecordTest {
            kind,
            id,
            ok,
            latency_ms,
            at,
            error,
        })
        .await
        .map(|_| ())
    }

    pub async fn invalidate_channel_test(&self, kind: ChannelKind, id: String) -> Result<(), BackendError> {
        self.apply_channel_mutation(ChannelMutation::InvalidateTest { kind, id }).await.map(|_| ())
    }

    pub async fn invalidate_channel_tests(&self, kind: ChannelKind) -> Result<(), BackendError> {
        self.apply_channel_mutation(ChannelMutation::InvalidateTests { kind })
            .await
            .map(|_| ())
    }

    pub async fn active_provider(&self, slot: ProviderSlot) -> Result<String, BackendError> {
        self.deps.credential_store.active_provider(slot).await
    }

    pub async fn set_active_provider(
        &self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> Result<CredentialsStatus, BackendError> {
        if provider_id.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "provider id must not be blank",
            ));
        }
        let owner_guard = if slot == ProviderSlot::Asr {
            Some(self.local_asr_channel_gate.lock().await)
        } else {
            None
        };
        let previous = if slot == ProviderSlot::Asr {
            self.asr_channel_owner().await?
        } else {
            None
        };
        self.deps
            .credential_store
            .set_active_provider(slot, provider_id)
            .await?;
        let retirement = self.release_previous_asr_owner(previous).await;
        // No runtime cleanup may trail an asynchronous credentials refresh: a
        // replacement channel/recording can already own the same runtime then.
        drop(owner_guard);
        let status = self.refresh_and_publish_credentials().await?;
        retirement?;
        Ok(status)
    }

    pub async fn activate_local_asr(
        &self,
        request: crate::domains::LocalAsrActivationRequest,
    ) -> Result<crate::domains::LocalAsrActivationResult, BackendError> {
        let owner_guard = self.local_asr_channel_gate.lock().await;
        let result = self.deps.services.local_asr.activate(request).await?;
        drop(owner_guard);
        self.refresh_and_publish_credentials().await?;
        Ok(result)
    }

    async fn apply_channel_mutation(
        &self,
        mutation: ChannelMutation,
    ) -> Result<ChannelMutationResult, BackendError> {
        let kind = match &mutation {
            ChannelMutation::ActivateLocalAsr { .. }
            | ChannelMutation::RegisterExperimentalAsr { .. } => ChannelKind::Asr,
            ChannelMutation::Create { kind, .. }
            | ChannelMutation::SetProviderType { kind, .. }
            | ChannelMutation::InvalidateTest { kind, .. }
            | ChannelMutation::InvalidateTests { kind }
            | ChannelMutation::DeleteIfBlank { kind, .. }
            | ChannelMutation::Rename { kind, .. }
            | ChannelMutation::Delete { kind, .. }
            | ChannelMutation::SetEnabled { kind, .. }
            | ChannelMutation::Reorder { kind, .. }
            | ChannelMutation::RecordTest { kind, .. } => *kind,
        };
        let owner_guard = if kind == ChannelKind::Asr {
            Some(self.local_asr_channel_gate.lock().await)
        } else {
            None
        };
        let previous = if kind == ChannelKind::Asr {
            self.asr_channel_owner().await?
        } else {
            None
        };
        let result = self.deps.credential_store.mutate_channel(mutation).await?;
        let retirement = self.release_previous_asr_owner(previous).await;
        drop(owner_guard);
        // The metadata commit still needs publishing when native cleanup fails.
        self.refresh_and_publish_credentials().await?;
        retirement?;
        Ok(result)
    }

    async fn asr_channel_owner(&self) -> Result<Option<(String, String)>, BackendError> {
        let id = match self
            .deps
            .credential_store
            .active_provider(ProviderSlot::Asr)
            .await
        {
            Ok(id) => id,
            Err(error) if error.code == BackendErrorCode::Unsupported => return Ok(None),
            Err(error) => return Err(error),
        };
        if id.is_empty() {
            return Ok(None);
        }
        let channels = match self
            .deps
            .credential_store
            .list_channels(ChannelKind::Asr)
            .await
        {
            Ok(channels) => channels,
            Err(error) if error.code == BackendErrorCode::Unsupported => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(
            match channels.into_iter().find(|channel| channel.id == id) {
                Some(channel) if channel.enabled => Some((id, channel.provider_type)),
                Some(_) => None,
                None => Some((id.clone(), id)), // Legacy provider-only selection.
            },
        )
    }

    async fn release_previous_asr_owner(
        &self,
        previous: Option<(String, String)>,
    ) -> Result<(), BackendError> {
        let Some(previous) = previous else {
            return Ok(());
        };
        if let Some(current) = self.asr_channel_owner().await? {
            // Editing an unrelated channel is not runtime retirement. A new
            // channel using the same shared runtime also keeps ownership; its
            // model can be replaced by the next explicit use, not a stale unload.
            if current == previous
                || current.1 == previous.1
                || Self::local_asr_runtime(&previous.1)
                    .is_some_and(|runtime| Self::local_asr_runtime(&current.1) == Some(runtime))
            {
                return Ok(());
            }
        }
        self.release_local_asr_provider(&previous.1).await
    }

    /// Whether the selected channel is one of the shared cloud ASR providers.
    /// Disabled channel errors also occur for local channels, which must not
    /// silently switch to another local model.
    async fn selected_asr_channel_is_shared_cloud(&self, preference_fallback: &str) -> bool {
        let selected_id = match self
            .deps
            .credential_store
            .active_provider(ProviderSlot::Asr)
            .await
        {
            Ok(provider) if !provider.trim().is_empty() => provider,
            _ => preference_fallback.to_string(),
        };
        let provider_type = match self
            .deps
            .credential_store
            .list_channels(ChannelKind::Asr)
            .await
        {
            Ok(channels) => {
                let provider = channels
                    .iter()
                    .find(|channel| channel.id == selected_id)
                    .or_else(|| channels.first())
                    .map(|channel| channel.provider_type.clone());
                provider.unwrap_or(selected_id)
            }
            Err(_) => selected_id,
        };
        crate::cloud_providers::SHARED_CLOUD_ASR_PROVIDER_TYPES.contains(&provider_type.as_str())
    }

    /// The local model dictation switches to when the selected cloud ASR
    /// channel is turned off or unconfigured: only where local ASR runs, with a sherpa-onnx
    /// model selected and its files installed — otherwise the disabled-channel
    /// error stands. Model, language and keep-loaded time are filled in from
    /// the same preferences a manual switch would use (DictationContext::capture).
    fn local_asr_switch_target(
        &self,
        preferences: &crate::UserPreferences,
    ) -> Option<crate::dictation_context::ProviderInvocation> {
        if !crate::shared_types::PlatformCapabilities::current().supports_local_asr {
            return None;
        }
        let target = crate::LocalAsrTarget::parse(
            crate::LocalAsrRuntime::SherpaOnnx,
            preferences.sherpa_onnx_model.trim(),
        )
        .ok()?;
        let installed = self
            .deps
            .services
            .model_store
            .as_ref()?
            .is_installed(&target)
            .unwrap_or(false);
        installed.then(|| {
            crate::dictation_context::ProviderInvocation::for_provider(
                crate::LocalAsrRuntime::SherpaOnnx.provider_id(),
            )
        })
    }

    async fn resolve_dictation_asr_provider(
        &self,
        preferences: &crate::UserPreferences,
    ) -> Result<crate::dictation_context::ProviderInvocation, BackendError> {
        let provider = match self
            .resolve_session_provider(ProviderSlot::Asr, &preferences.active_asr_provider)
            .await
        {
            Ok(provider) => provider,
            Err(error)
                if error.code == BackendErrorCode::InvalidState
                    && self
                        .selected_asr_channel_is_shared_cloud(&preferences.active_asr_provider)
                        .await =>
            {
                return self.local_asr_switch_target(preferences).ok_or(error);
            }
            Err(error) => return Err(error),
        };
        if crate::cloud_providers::SHARED_CLOUD_ASR_PROVIDER_TYPES
            .contains(&provider.provider_type.as_str())
        {
            if let Some(local) = self.local_asr_switch_target(preferences) {
                if !crate::cloud_providers::asr_channel_configured(
                    self.deps.credential_store.as_ref(),
                    &provider,
                )
                .await?
                {
                    return Ok(local);
                }
            }
        }
        Ok(provider)
    }

    async fn selected_local_asr_model_is_installed(
        &self,
        runtime: crate::LocalAsrRuntime,
        preferences: &crate::UserPreferences,
    ) -> bool {
        let model = match runtime {
            crate::LocalAsrRuntime::Foundry => &preferences.foundry_local_asr_model,
            crate::LocalAsrRuntime::SherpaOnnx => &preferences.sherpa_onnx_model,
            crate::LocalAsrRuntime::Generic => return false,
        };
        let Ok(target) = crate::LocalAsrTarget::parse(runtime, model.trim()) else {
            return false;
        };
        self.deps
            .services
            .local_asr
            .list_models(runtime)
            .await
            .ok()
            .is_some_and(|models| {
                models
                    .into_iter()
                    .any(|entry| entry.target == target && entry.installed)
            })
    }

    /// Which recognizer the next dictation will use, for hosts that show it
    /// (the Android keyboard's status dot): the selected channel's own kind,
    /// or Local when that channel is disabled or unconfigured and an installed
    /// local model steps in.
    pub async fn asr_route(&self) -> AsrRoute {
        let preferences = self.get_preferences();
        let local = match self.resolve_dictation_asr_provider(&preferences).await {
            Ok(provider) => {
                let Some(runtime) = Self::local_asr_runtime(&provider.provider_type) else {
                    return AsrRoute::Cloud;
                };
                if !self.deps.services.local_asr.engine_available(runtime) {
                    return AsrRoute::Unavailable;
                }
                if runtime == crate::LocalAsrRuntime::Foundry {
                    // Foundry's engine is compiled in even when the selected
                    // native model receipt is missing or a download/storage
                    // operation requires a process restart. Do not advertise a
                    // route that the next prepare call will reject.
                    let status = match self.deps.services.local_asr.runtime_status(runtime).await {
                        Ok(status) => status,
                        Err(_) => return AsrRoute::Unavailable,
                    };
                    if status.restart_required
                        || !self
                            .selected_local_asr_model_is_installed(runtime, &preferences)
                            .await
                    {
                        return AsrRoute::Unavailable;
                    }
                }
                // Sherpa is a real local route only when the selected model is
                // installed on this ABI. The native engine otherwise rejects
                // the dictation after the route has already been displayed.
                if runtime == crate::LocalAsrRuntime::SherpaOnnx
                    && self.local_asr_switch_target(&preferences).is_none()
                {
                    return AsrRoute::Unavailable;
                }
                true
            }
            Err(_) => return AsrRoute::Unavailable,
        };
        if !local {
            return AsrRoute::Cloud;
        }
        // Same test the dictation context applies: a polish channel that is
        // turned off or not configured means raw output.
        let polish_usable = match self
            .resolve_session_provider(ProviderSlot::Llm, &preferences.active_llm_provider)
            .await
        {
            Ok(llm) => {
                crate::cloud_providers::llm_channel_configured(
                    self.deps.credential_store.as_ref(),
                    &llm.provider_id,
                    &llm.provider_type,
                    llm.model.as_deref(),
                )
                .await
            }
            Err(_) => false,
        };
        if polish_usable {
            AsrRoute::Local
        } else {
            AsrRoute::LocalRaw
        }
    }

    /// How many dictations in history the local model and the cloud each
    /// recognized, and which one the latest used. Counted from history, so it
    /// survives restarts and follows the user's retention and deletions.
    pub fn asr_usage(&self) -> Result<AsrUsage, BackendError> {
        let mut usage = AsrUsage::default();
        let mut latest: Option<(String, bool)> = None;
        for entry in self.history.list()? {
            // Recording drafts and terminal ASR failures retain the provider
            // selected at start-up, but they never produced a recognition.
            if matches!(
                entry.error_code.as_deref(),
                Some(
                    "recording"
                        | "cancelled"
                        | "recordingFailed"
                        | "transcribeFailed"
                        | "emptyTranscript"
                )
            ) {
                continue;
            }
            let Some(provider) = entry.asr_provider.as_deref() else {
                continue;
            };
            let local = crate::dictation_context::is_local_asr_provider(provider);
            if local {
                usage.local += 1;
            } else {
                usage.cloud += 1;
            }
            if latest
                .as_ref()
                .is_none_or(|(created_at, _)| entry.created_at > *created_at)
            {
                latest = Some((entry.created_at, local));
            }
        }
        usage.last_local = latest.map(|(_, local)| local);
        Ok(usage)
    }

    fn local_asr_runtime(provider: &str) -> Option<crate::LocalAsrRuntime> {
        match provider {
            "local-qwen3" | "local-qwen3-mlx" | "local-qwen3-c" | "local-whisper"
            | "apple-whisper" => Some(crate::LocalAsrRuntime::Generic),
            "foundry-local" | "foundry-whisper" | "foundry-local-whisper" => {
                Some(crate::LocalAsrRuntime::Foundry)
            }
            "sherpa-onnx" | "sherpa-onnx-local" => Some(crate::LocalAsrRuntime::SherpaOnnx),
            _ => None,
        }
    }

    async fn release_local_asr_provider(&self, provider: &str) -> Result<(), BackendError> {
        if let Some(model) = crate::experimental_asr::ExperimentalAsrModel::from_provider(provider)
        {
            if let Some(service) = &self.deps.services.experimental_asr {
                service.release_runtime(model).await?;
            }
            return Ok(());
        }
        let Some(runtime) = Self::local_asr_runtime(provider) else {
            return Ok(());
        };
        match self.deps.services.local_asr.release(runtime).await {
            Err(error) if error.code == BackendErrorCode::Unsupported => Ok(()),
            result => result,
        }
    }

    pub async fn refresh_and_publish_credentials(&self) -> Result<CredentialsStatus, BackendError> {
        let status = self
            .deps
            .credential_store
            .status(self.get_preferences())
            .await?;
        let status = self.with_siwc_status(status).await?;
        self.state
            .write()
            .expect("backend state lock poisoned")
            .credentials = status.clone();
        self.events
            .publish(None, BackendEventKind::CredentialsChanged(status.clone()));
        Ok(status)
    }

    pub fn publish_local_migration_state_changed(&self) {
        self.publish_preferences_changed();
        self.publish_vocabulary_changed();
        self.publish_style_packs_changed();
    }

    pub fn get_preferences(&self) -> UserPreferences {
        self.preferences.get()
    }

    /// Consumes the "first launch of this major version" splash PV marker:
    /// returns false when the config's `splash_seen_version` matches the
    /// given current major (no replay); on mismatch it writes back the major
    /// and returns true, and the frontend plays the bundled splash animation
    /// (2.x upgrades and restarts within the same generation never replay).
    /// Returns true even when the disk write fails — better to replay once
    /// than silently swallow the first-launch experience; the marker retries
    /// on next launch. A successful write goes through
    /// publish_preferences_changed to bump the revision so in-flight settings
    /// pages with optimistic submits re-reconcile instead of wiping the
    /// marker with a stale document.
    pub fn take_splash_playback(&self, current_major: &str) -> bool {
        match self.preferences.update(|preferences| {
            if preferences.splash_seen_version == current_major {
                false
            } else {
                preferences.splash_seen_version = current_major.to_string();
                true
            }
        }) {
            Ok(should_play) => {
                if should_play {
                    self.publish_preferences_changed();
                }
                should_play
            }
            Err(error) => {
                log::warn!("[splash] failed to persist splash marker: {error}");
                true
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn set_preferences(
        &self,
        mut preferences: UserPreferences,
    ) -> Result<(), BackendError> {
        sync_style_pack_preferences(&mut preferences, &self.style_packs.list()?);
        self.preferences.set(preferences)?;
        self.hotkey
            .lock()
            .expect("hotkey interpreter lock poisoned")
            .reset();
        self.publish_preferences_changed();
        Ok(())
    }

    /// Persist a host-facing settings document after applying the shared
    /// shortcut compatibility and collision rules.
    #[cfg(test)]
    pub(crate) fn set_preferences_validated(
        &self,
        mut preferences: UserPreferences,
    ) -> Result<(), BackendError> {
        crate::sync_dictation_hotkey_legacy_fields(&mut preferences);
        crate::reject_hotkey_collisions(&preferences).map_err(|message| {
            BackendError::new(crate::BackendErrorCode::InvalidArgument, message)
        })?;
        self.set_preferences(preferences)
    }

    #[cfg(test)]
    pub(crate) fn set_preferences_preserving_style(
        &self,
        preferences: UserPreferences,
    ) -> Result<(), BackendError> {
        self.preferences
            .set_preserving_current_style_preferences(preferences)?;
        self.hotkey
            .lock()
            .expect("hotkey interpreter lock poisoned")
            .reset();
        self.publish_preferences_changed();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_preferences_preserving_style_validated(
        &self,
        mut preferences: UserPreferences,
    ) -> Result<(), BackendError> {
        crate::sync_dictation_hotkey_legacy_fields(&mut preferences);
        crate::reject_hotkey_collisions(&preferences).map_err(|message| {
            BackendError::new(crate::BackendErrorCode::InvalidArgument, message)
        })?;
        self.set_preferences_preserving_style(preferences)
    }

    pub fn update_settings<R: crate::SettingsRuntime + ?Sized>(
        &self,
        mut preferences: UserPreferences,
        options: crate::SettingsUpdateOptions,
        runtime: &R,
    ) -> Result<crate::SettingsUpdateOutcome, BackendError> {
        self.ensure_runtime_ready()?;
        let _write_guard = self
            .settings_write_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(expected) = options.expected_preferences_revision {
            let actual = self.preferences_revision.load(Ordering::Acquire);
            if actual != expected {
                return Err(BackendError {
                    code: BackendErrorCode::Busy,
                    message: "settings changed since the submitted document was read".into(),
                    retryable: true,
                    details: Some(serde_json::json!({
                        "expectedPreferencesRevision": expected,
                        "actualPreferencesRevision": actual,
                    })),
                });
            }
        }
        preferences.vocabulary_learning_settings =
            preferences.vocabulary_learning_settings.normalized();
        let mut previous = self.preferences.get();
        preferences.capsule_display_mode_v2 =
            if preferences.capsule_display_mode != previous.capsule_display_mode {
                Some(preferences.capsule_display_mode)
            } else {
                preferences
                    .capsule_display_mode_v2
                    .or(previous.capsule_display_mode_v2)
            };
        crate::sync_dictation_hotkey_legacy_fields(&mut previous);
        crate::sync_dictation_hotkey_legacy_fields(&mut preferences);
        // The splash marker may only advance via take_splash_playback: when a
        // whole-document submit (old frontend, or a request that hasn't read
        // the marker back) omits the field, serde's default would write the
        // empty string and replay the splash PV on next launch. Always keep
        // the on-disk current value here.
        preferences.splash_seen_version = previous.splash_seen_version.clone();
        if options.preserve_current_style {
            preferences.preserve_style_preferences_from(&previous);
        }
        sync_style_pack_preferences(&mut preferences, &self.style_packs.list()?);

        let reconciled_hotkey_count = match crate::reject_hotkey_collisions(&preferences) {
            Ok(()) => 0,
            Err(message)
                if options.collision_policy == crate::SettingsCollisionPolicy::Reconcile =>
            {
                let adjusted = crate::reconcile_hotkey_collisions(&mut preferences, &previous);
                crate::reject_hotkey_collisions(&preferences).map_err(|leftover| {
                    BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        format!(
                            "{message}; reconciled {adjusted} shortcuts but validation still failed: {leftover}"
                        ),
                    )
                })?;
                adjusted
            }
            Err(message) => {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    message,
                ));
            }
        };

        let voice_activation_config = crate::voice_activation::VoiceActivationConfig {
            enabled: preferences.voice_activation_enabled,
            wake_enabled: preferences.voice_activation_wake_enabled,
            finish_enabled: preferences.voice_activation_finish_enabled,
            wake_phrase: preferences.voice_activation_wake_phrase.clone(),
            finish_phrase: preferences.voice_activation_finish_phrase.clone(),
            timeout_enabled: preferences.voice_activation_timeout_enabled,
            timeout_seconds: preferences.voice_activation_timeout_seconds,
            microphone_device_name: (!preferences.microphone_device_name.trim().is_empty())
                .then(|| preferences.microphone_device_name.clone()),
            activation_generation: 0,
        };
        voice_activation_config.normalized().map_err(|error| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                format!("invalid voice activation settings: {error}"),
            )
        })?;

        let effects = crate::SettingsEffectPlan::between(&previous, &preferences);
        let mut receipt = match runtime.prepare(&effects) {
            Ok(receipt) => receipt,
            Err(failure) => {
                let compensation = runtime.restore(&effects, &failure.receipt).err();
                return Err(settings_transaction_error(
                    failure.error,
                    compensation.into_iter().collect(),
                ));
            }
        };

        if let Err(failure) = runtime.commit(&effects, &mut receipt) {
            for effect in failure.receipt.applied {
                if !receipt.applied.contains(&effect) {
                    receipt.applied.push(effect);
                }
            }
            let compensation_errors = runtime
                .restore(&effects, &receipt)
                .err()
                .into_iter()
                .collect();
            return Err(settings_transaction_error(
                failure.error,
                compensation_errors,
            ));
        }

        if let Err(error) = self.preferences.set(preferences.clone()) {
            let compensation = runtime.restore(&effects, &receipt).err();
            return Err(settings_transaction_error(
                error,
                compensation.into_iter().collect(),
            ));
        }

        if effects.hotkeys.is_some() {
            self.hotkey
                .lock()
                .expect("hotkey interpreter lock poisoned")
                .reset();
        }

        if (previous.vocabulary_learning_enabled && !preferences.vocabulary_learning_enabled)
            || previous.vocabulary_learning_settings != preferences.vocabulary_learning_settings
        {
            self.disarm_edit_observation();
            self.dismiss_pending_corrections();
        }
        if previous.testing_features_enabled && !preferences.testing_features_enabled {
            // Capture the old owners while settings admission is still locked.
            // A later opt-in must not let cleanup cancel a replacement task.
            let cancellation = self
                .less_computer_active_session()
                .map(|session_id| self.less_computer_cancellation(session_id));
            self.deps.services.less_computer.dismiss();
            let cancel_test = self.deps.services.coding_agent.cancel_test();
            self.deps.task_spawner.spawn(Box::pin(async move {
                let _ = cancel_test.await;
                if let Some(cancellation) = cancellation {
                    let _ = cancellation.await;
                }
            }));
        }
        self.publish_preferences_changed();
        Ok(crate::SettingsUpdateOutcome {
            preferences,
            reconciled_hotkey_count,
            effects,
        })
    }

    fn publish_preferences_changed(&self) {
        let revision = self.preferences_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.events.publish(
            None,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision }),
        );
    }

    pub fn list_style_packs(&self, active_id: &str) -> Result<Vec<StylePack>, BackendError> {
        self.style_packs.list_with_active(active_id)
    }

    /// Return settings-page prompt diagnostics assembled by Core. The DTO is
    /// owned and safe for any host to render; hosts must not duplicate prompt
    /// composition or hotword filtering.
    pub fn preview_style_pack_runtime(
        &self,
        style_pack: &StylePack,
    ) -> crate::style_packs::StylePackRuntimeDiagnostics {
        let preferences = self.get_preferences();
        let hotwords = self.enabled_vocabulary_phrases();
        crate::style_packs::build_style_pack_runtime_diagnostics(style_pack, &preferences, hotwords)
    }

    /// Persist the microphone selected by a host-owned menu or device picker.
    ///
    /// This focused use-case keeps callers away from whole-document writes and
    /// shares the settings write gate with validated settings transactions.
    pub fn select_microphone_device(&self, device_name: String) -> Result<(), BackendError> {
        let _write_guard = self
            .settings_write_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut preferences = self.preferences.get();
        preferences.microphone_device_name = device_name;
        self.preferences.set(preferences)?;
        self.publish_preferences_changed();
        Ok(())
    }

    /// Select the previous enabled style pack in the stable store order.
    ///
    /// Returns `None` when cycling is not meaningful (zero or one enabled pack).
    /// Window feedback and tray refresh remain host responsibilities.
    pub fn activate_previous_style_pack(&self) -> Result<Option<StylePack>, BackendError> {
        let _write_guard = self
            .settings_write_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut preferences = self.preferences.get();
        let packs = self
            .style_packs
            .list_with_active(&preferences.active_style_pack_id)?;
        let enabled = packs
            .into_iter()
            .filter(|pack| pack.enabled)
            .collect::<Vec<_>>();
        if enabled.len() <= 1 {
            return Ok(None);
        }
        let current_index = enabled
            .iter()
            .position(|pack| pack.id == preferences.active_style_pack_id)
            .unwrap_or(0);
        let next_index = if current_index == 0 {
            enabled.len() - 1
        } else {
            current_index - 1
        };
        let mut selected = enabled[next_index].clone();
        preferences.active_style_pack_id = selected.id.clone();
        sync_style_pack_preferences(&mut preferences, &enabled);
        self.preferences.set(preferences)?;
        self.publish_preferences_changed();
        selected.active = true;
        Ok(Some(selected))
    }

    pub fn get_style_pack(&self, id: &str) -> Result<StylePack, BackendError> {
        self.style_packs.get(id)
    }

    pub fn get_active_style_pack(&self, active_id: &str) -> Result<StylePack, BackendError> {
        self.style_packs.get_or_default_active(active_id)
    }

    pub fn activate_style_pack(&self, id: &str) -> Result<StylePack, BackendError> {
        let mut pack = self.style_packs.get(id)?;
        if !pack.enabled {
            pack = self.style_packs.set_enabled(id, true)?;
        }
        let mut preferences = self.preferences.get();
        preferences.active_style_pack_id = id.to_string();
        sync_style_pack_preferences(&mut preferences, &self.style_packs.list()?);
        self.preferences.set(preferences)?;
        self.publish_preferences_changed();
        self.publish_style_packs_changed();
        pack.active = true;
        Ok(pack)
    }

    pub fn create_style_pack(&self, pack: StylePack) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.create(pack)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn update_style_pack(&self, pack: StylePack) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.update(pack)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn set_style_pack_icon(
        &self,
        id: &str,
        png: Option<&[u8]>,
    ) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.update_icon(id, png)?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn read_style_pack_icon(&self, id: &str) -> Result<Option<String>, BackendError> {
        self.style_packs.icon_data_url(id)
    }

    pub fn set_style_pack_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.set_enabled(id, enabled)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn set_style_pack_origin(
        &self,
        id: &str,
        origin_pack_id: Option<String>,
        origin_author_login: Option<String>,
    ) -> Result<StylePack, BackendError> {
        let pack = self
            .style_packs
            .set_origin(id, origin_pack_id, origin_author_login)?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn reset_builtin_style_pack(&self, id: &str) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.reset_builtin(id)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn remove_style_pack(
        &self,
        id: &str,
    ) -> Result<crate::StylePackRemovalOutcome, BackendError> {
        let _write_guard = self
            .settings_write_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = self.preferences.get();
        self.style_packs.remove_imported(id)?;
        let mut preferences = previous.clone();
        preferences
            .style_pack_hotkeys
            .retain(|entry| entry.pack_id != id);
        if preferences.active_style_pack_id == id {
            preferences.active_style_pack_id = crate::style_packs::default_active_style_pack_id();
        }
        sync_style_pack_preferences(&mut preferences, &self.style_packs.list()?);
        let effects = crate::SettingsEffectPlan::between(&previous, &preferences);
        self.preferences.set(preferences)?;
        self.publish_preferences_changed();
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(crate::StylePackRemovalOutcome { effects })
    }

    pub fn import_style_pack_bytes(&self, bytes: &[u8]) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.import_from_zip_bytes(bytes)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn import_style_pack_path(
        &self,
        path: &std::path::Path,
    ) -> Result<StylePack, BackendError> {
        let pack = self.style_packs.import_from_zip(path)?;
        self.sync_preferences_after_style_pack_change()?;
        self.publish_style_packs_changed();
        Ok(pack)
    }

    pub fn export_style_pack_bytes(&self, id: &str) -> Result<Vec<u8>, BackendError> {
        self.style_packs.export_zip_bytes(id)
    }

    pub fn export_style_pack_path(
        &self,
        id: &str,
        path: &std::path::Path,
    ) -> Result<(), BackendError> {
        self.style_packs.export_to_zip(id, path)
    }

    fn sync_preferences_after_style_pack_change(&self) -> Result<(), BackendError> {
        let mut preferences = self.preferences.get();
        if sync_style_pack_preferences(&mut preferences, &self.style_packs.list()?) {
            self.preferences.set(preferences)?;
            self.publish_preferences_changed();
        }
        Ok(())
    }

    fn publish_style_packs_changed(&self) {
        let revision = self.style_pack_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.events.publish(
            None,
            BackendEventKind::StylePacksChanged(StylePackChange { revision }),
        );
    }

    pub fn list_history(&self) -> Result<Vec<DictationSession>, BackendError> {
        self.history.list()
    }

    pub fn recent_history_within_minutes(
        &self,
        minutes: u32,
    ) -> Result<Vec<DictationSession>, BackendError> {
        self.history.recent_within_minutes(minutes)
    }

    pub fn list_activity(&self) -> Result<Vec<ActivityDay>, BackendError> {
        self.activity.snapshot()
    }

    pub fn record_activity(
        &self,
        date: &str,
        chars: u64,
        duration_ms: u64,
    ) -> Result<(), BackendError> {
        self.activity.bump(date, chars, duration_ms)?;
        self.publish_history_changed();
        Ok(())
    }

    pub fn append_history(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<(), BackendError> {
        self.history
            .append_with_retention(session, retention_days, max_entries)?;
        self.publish_history_changed();
        Ok(())
    }

    pub fn upsert_history(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<(), BackendError> {
        self.history
            .upsert_with_retention(session, retention_days, max_entries)?;
        self.publish_history_changed();
        Ok(())
    }

    pub fn delete_history(&self, id: &str) -> Result<(), BackendError> {
        self.history.delete(id)?;
        self.publish_history_changed();
        Ok(())
    }

    pub fn update_history_entry(&self, session: DictationSession) -> Result<bool, BackendError> {
        let updated = self.history.update_entry(session)?;
        if updated {
            self.publish_history_changed();
        }
        Ok(updated)
    }

    pub fn apply_history_repolish(
        &self,
        session_id: &str,
        text: String,
        style_pack_id: Option<String>,
    ) -> Result<DictationSession, BackendError> {
        if text.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "repolish text is empty",
            ));
        }
        let mut entry = self
            .list_history()?
            .into_iter()
            .find(|entry| entry.id == session_id)
            .ok_or_else(|| {
                BackendError::new(BackendErrorCode::InvalidArgument, "history entry not found")
            })?;
        if entry.source != HistorySource::QuickNote {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "only quick-note history can apply repolish results",
            ));
        }
        entry.final_text = text;
        entry.error_code = None;
        if style_pack_id.is_some() {
            entry.style_pack_id = style_pack_id;
        }
        if !self.update_history_entry(entry.clone())? {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "history entry not found",
            ));
        }
        Ok(entry)
    }

    pub fn apply_history_retranscription(
        &self,
        session_id: &str,
        text: String,
        asr_call_label: &crate::auxiliary::AsrCallLabel,
        asr_ms: u64,
    ) -> Result<DictationSession, BackendError> {
        // The host supplies only bytes and the measured provider result. All
        // record attribution/mutation stays here so Tauri and future hosts
        // cannot retain different meanings for a successful retranscription.
        if text.trim().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "retranscription text is empty",
            ));
        }
        let mut entry = self
            .list_history()?
            .into_iter()
            .find(|entry| entry.id == session_id)
            .ok_or_else(|| {
                BackendError::new(BackendErrorCode::InvalidArgument, "history entry not found")
            })?;
        if entry.source != HistorySource::QuickNote
            && entry.error_code.as_deref() != Some("recording")
            && !matches!(
                entry.error_code.as_deref(),
                Some("transcribeFailed" | "emptyTranscript")
            )
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "history entry is not a failed transcription",
            ));
        }
        entry.raw_transcript = text.clone();
        entry.final_text = text;
        entry.error_code = None;
        entry.asr_provider = Some(asr_call_label.provider.clone());
        entry.asr_model = asr_call_label.model.clone();
        entry.asr_ms = Some(asr_ms);
        entry.llm_provider = None;
        entry.llm_model = None;
        entry.polish_ms = None;
        if !self.update_history_entry(entry.clone())? {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "history entry not found",
            ));
        }
        Ok(entry)
    }

    pub fn clear_history(&self) -> Result<(), BackendError> {
        self.history.clear()?;
        self.publish_history_changed();
        Ok(())
    }

    fn publish_history_changed(&self) {
        let revision = self.history_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.events.publish(
            None,
            BackendEventKind::HistoryChanged(HistoryChange { revision }),
        );
    }

    pub fn list_vocabulary(&self) -> Result<Vec<DictionaryEntry>, BackendError> {
        self.vocabulary.list()
    }

    /// Return enabled vocabulary phrases in persisted order. Hosts reuse this
    /// owned projection instead of duplicating the filtering rule.
    pub fn enabled_vocabulary_phrases(&self) -> Vec<String> {
        self.list_vocabulary()
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.phrase)
            .collect()
    }

    /// Return enabled vocabulary phrases in the Core ASR priority order.
    pub fn asr_vocabulary_phrases(&self) -> Vec<String> {
        let entries = self
            .list_vocabulary()
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.enabled)
            .collect();
        crate::vocabulary::prioritize_vocabulary_for_asr(entries)
    }

    /// Return the instance-local correction suggestions awaiting a user
    /// decision. The returned value is owned and safe to render on any host.
    pub fn pending_corrections(&self) -> Vec<PendingCorrection> {
        let mut pending = self
            .pending_corrections
            .lock()
            .expect("pending correction lock poisoned");
        pending.retain(|item| item.expires_at_ms > chrono::Utc::now().timestamp_millis());
        pending.clone()
    }

    /// Queue one observed manual correction. Duplicate pairs are ignored and
    /// the oldest item is dropped when the bounded card capacity is reached.
    pub fn queue_pending_correction(
        &self,
        pattern: String,
        replacement: String,
    ) -> Result<Option<PendingCorrection>, BackendError> {
        queue_pending_correction_state(
            &self.pending_corrections,
            &self.events,
            pattern,
            replacement,
            None,
            self.get_preferences()
                .vocabulary_learning_settings
                .suggestion_seconds,
        )
    }

    fn disarm_edit_observation(&self) {
        self.edit_observation_generation
            .fetch_add(1, Ordering::AcqRel);
        self.deps.services.edit_observation.disarm();
    }

    fn arm_edit_observation(
        &self,
        session_id: SessionId,
        enabled: bool,
        insert_outcome: Option<InsertOutcome>,
        typed_text: &str,
    ) {
        // Privacy changes are not a frozen dictation option. Serialize this
        // native registration with settings updates so a mid-session opt-out
        // cannot be undone by an older dictation completing afterwards.
        let _settings_guard = self
            .settings_write_gate
            .lock()
            .expect("settings write gate poisoned");
        // Native settings can hold this gate while A completes, is cancelled,
        // and B starts. Recheck ownership after the wait, before disarming or
        // reading any document for an obsolete completion.
        {
            let state = self.state.read().expect("backend state lock poisoned");
            if state.dictation.session_id != Some(session_id)
                || state.dictation.phase != DictationPhase::Completed
            {
                return;
            }
        }
        self.disarm_edit_observation();
        if !enabled
            || !self.get_preferences().vocabulary_learning_enabled
            || !matches!(insert_outcome, Some(InsertOutcome::Inserted))
            || typed_text.trim().is_empty()
        {
            return;
        }
        let expected_generation = self.edit_observation_generation.load(Ordering::Acquire);
        let sink = Arc::new(CoreEditObservationSink {
            settings: self
                .get_preferences()
                .vocabulary_learning_settings
                .normalized(),
            expected_generation,
            generation: Arc::clone(&self.edit_observation_generation),
            typed_text: typed_text.to_string(),
            pending: Arc::clone(&self.pending_corrections),
            events: Arc::clone(&self.events),
        });
        if let Err(error) = self
            .deps
            .services
            .edit_observation
            .arm(typed_text.to_string(), sink)
        {
            log::warn!("failed to arm edit observation: {error}");
        }
        if self.edit_observation_generation.load(Ordering::Acquire) != expected_generation {
            // A new start may disarm during the synchronous Host call. Another
            // completion cannot arm while this settings guard is held, so only
            // the obsolete registration is removed; preserve B's generation.
            self.deps.services.edit_observation.disarm();
        }
    }

    /// Accept one suggestion and atomically remove it only after the shared
    /// vocabulary mutation succeeds. Repeated or stale ids are idempotent.
    pub fn accept_pending_correction(
        &self,
        id: &str,
    ) -> Result<Option<PendingCorrection>, BackendError> {
        let (suggestion, added, snapshot) = {
            let mut pending = self
                .pending_corrections
                .lock()
                .expect("pending correction lock poisoned");
            let Some(index) = pending.iter().position(|item| item.id == id) else {
                return Ok(None);
            };
            let suggestion = pending[index].clone();
            if suggestion.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
                pending.remove(index);
                self.events.publish(
                    None,
                    BackendEventKind::VocabularySuggestionsChanged(pending.clone()),
                );
                return Ok(None);
            }
            let added = self.vocabulary.add_if_absent(
                suggestion.replacement.clone(),
                Some(LEARNED_VOCAB_NOTE.to_string()),
            )?;
            pending.remove(index);
            (suggestion, added.is_some(), pending.clone())
        };
        if added {
            self.publish_vocabulary_changed();
        }
        self.events.publish(
            None,
            BackendEventKind::VocabularySuggestionsChanged(snapshot),
        );
        Ok(Some(suggestion))
    }

    /// Reject one suggestion without creating a hidden deny-list.
    pub fn reject_pending_correction(&self, id: &str) -> bool {
        let snapshot = {
            let mut pending = self
                .pending_corrections
                .lock()
                .expect("pending correction lock poisoned");
            let Some(index) = pending.iter().position(|item| item.id == id) else {
                return false;
            };
            pending.remove(index);
            pending.clone()
        };
        self.events.publish(
            None,
            BackendEventKind::VocabularySuggestionsChanged(snapshot),
        );
        true
    }

    /// Dismiss the complete card. Empty dismissals are idempotent and do not
    /// publish redundant events.
    pub fn dismiss_pending_corrections(&self) {
        let changed = {
            let mut pending = self
                .pending_corrections
                .lock()
                .expect("pending correction lock poisoned");
            if pending.is_empty() {
                false
            } else {
                pending.clear();
                true
            }
        };
        if changed {
            self.events.publish(
                None,
                BackendEventKind::VocabularySuggestionsChanged(Vec::new()),
            );
        }
    }

    /// Explicit user-confirmed vocabulary collection from an in-app editor.
    /// This path intentionally does not require permission to read other apps.
    pub fn add_learned_vocabulary(
        &self,
        phrase: String,
    ) -> Result<Option<DictionaryEntry>, BackendError> {
        let phrase = phrase.trim();
        if phrase.is_empty() || phrase.chars().count() > 64 || phrase.chars().any(char::is_control)
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "enter a word or phrase of 1–64 characters",
            ));
        }
        self.add_vocabulary_if_absent(phrase.to_owned(), Some(LEARNED_VOCAB_NOTE.to_owned()))
    }

    pub fn add_vocabulary(
        &self,
        phrase: String,
        note: Option<String>,
    ) -> Result<DictionaryEntry, BackendError> {
        let entry = self.vocabulary.add(phrase, note)?;
        self.publish_vocabulary_changed();
        Ok(entry)
    }

    pub fn add_vocabulary_if_absent(
        &self,
        phrase: String,
        note: Option<String>,
    ) -> Result<Option<DictionaryEntry>, BackendError> {
        let entry = self.vocabulary.add_if_absent(phrase, note)?;
        if entry.is_some() {
            self.publish_vocabulary_changed();
        }
        Ok(entry)
    }

    pub fn record_vocabulary_hits(&self, text: &str) -> Result<u64, BackendError> {
        let hits = self.vocabulary.record_hits(text)?;
        if hits > 0 {
            self.publish_vocabulary_changed();
        }
        Ok(hits)
    }

    pub fn remove_vocabulary(&self, id: &str) -> Result<(), BackendError> {
        self.vocabulary.remove(id)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    pub fn set_vocabulary_enabled(&self, id: &str, enabled: bool) -> Result<(), BackendError> {
        self.vocabulary.set_enabled(id, enabled)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    pub fn update_vocabulary_phrase(&self, id: &str, phrase: String) -> Result<(), BackendError> {
        self.vocabulary.update_phrase(id, phrase)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    pub fn list_correction_rules(&self) -> Result<Vec<CorrectionRule>, BackendError> {
        self.correction_rules.list()
    }

    pub fn add_correction_rule(
        &self,
        pattern: String,
        replacement: String,
    ) -> Result<CorrectionRule, BackendError> {
        let rule = self.correction_rules.add(pattern, replacement)?;
        self.publish_vocabulary_changed();
        Ok(rule)
    }

    pub fn remove_correction_rule(&self, id: &str) -> Result<(), BackendError> {
        self.correction_rules.remove(id)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    pub fn set_correction_rule_enabled(&self, id: &str, enabled: bool) -> Result<(), BackendError> {
        self.correction_rules.set_enabled(id, enabled)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    pub fn list_vocabulary_presets(&self) -> Result<VocabPresetStore, BackendError> {
        crate::vocabulary::list_vocab_presets(&self.config.data_dir)
    }

    pub fn save_vocabulary_presets(&self, store: &VocabPresetStore) -> Result<(), BackendError> {
        crate::vocabulary::save_vocab_presets(&self.config.data_dir, store)?;
        self.publish_vocabulary_changed();
        Ok(())
    }

    fn publish_vocabulary_changed(&self) {
        let revision = self.vocabulary_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.events.publish(
            None,
            BackendEventKind::VocabularyChanged(VocabularyChange { revision }),
        );
    }

    pub async fn start_dictation(&self) -> Result<SessionId, BackendError> {
        self.start_dictation_with_options(DictationStartOptions::default())
            .await
    }

    pub async fn start_external_dictation(&self) -> Result<SessionId, BackendError> {
        self.start_external_dictation_with_options(DictationStartOptions::default())
            .await
    }

    pub async fn start_external_dictation_with_options(
        &self,
        mut options: DictationStartOptions,
    ) -> Result<SessionId, BackendError> {
        options.audio_source = DictationAudioSource::External;
        self.start_dictation_with_options(options).await
    }

    pub fn feed_external_pcm(&self, session_id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        {
            let state = self.state.read().expect("backend state lock poisoned");
            ensure_running(&state)?;
            if state.dictation.session_id != Some(session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "external PCM targets an inactive dictation session",
                ));
            }
            if !matches!(
                state.dictation.phase,
                DictationPhase::Starting | DictationPhase::Recording
            ) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "external PCM is only accepted while recording",
                ));
            }
            if state
                .dictation_context
                .as_ref()
                .is_none_or(|context| context.audio_source != DictationAudioSource::External)
            {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "active dictation session does not use external audio",
                ));
            }
        }
        self.deps.dictation_engine.feed_audio(session_id, pcm)
    }

    fn reserve_dictation_session(
        &self,
        insert_text: bool,
        output_target: DictationOutputTarget,
        capture_key: Option<PendingAudioCaptureKey>,
    ) -> Result<DictationReservation, BackendError> {
        {
            let state = self.state.read().expect("backend state lock poisoned");
            ensure_running(&state)?;
            if state.dictation.session_id.is_some() {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "a dictation session is already active",
                ));
            }
        }
        let session_id = SessionId::new();
        self.voice_sessions.acquire(
            session_id,
            crate::voice_session::VoiceSessionKind::Dictation,
        )?;
        let starting_resources = self.voice_sessions.hold_resources(session_id)?;
        // Freeze the destination and its matching host-context reader before context/credential
        // awaits or feedback can change foreground focus. No native preparation happens yet.
        let (inserter, host_context) = if insert_text {
            match self.deps.text_inserter.capture_target() {
                Some(captured) => (captured.inserter, captured.host_context),
                None => (
                    Arc::clone(&self.deps.text_inserter),
                    Arc::clone(&self.deps.services.host_context),
                ),
            }
        } else {
            (
                Arc::clone(&self.deps.text_inserter),
                Arc::clone(&self.deps.services.host_context),
            )
        };
        self.disarm_edit_observation();
        // Context capture can await AX, a keyring, or another host service.
        // Publish ownership before that first await so Esc/stop see Starting
        // instead of an invisible lease which would begin recording later.
        {
            let mut state = self.state.write().expect("backend state lock poisoned");
            if let Err(error) = ensure_running(&state) {
                self.voice_sessions.release(session_id);
                return Err(error);
            }
            state.dictation = DictationStateSnapshot {
                phase: DictationPhase::Starting,
                session_id: Some(session_id),
                ..DictationStateSnapshot::default()
            };
            state.dictation_start_output_target = Some(output_target);
            state.dictation_translation_requested = None;
            state.dictation_destination_changed = false;
            self.events.publish(
                Some(session_id),
                BackendEventKind::DictationStateChanged(state.dictation.clone()),
            );
            self.phase_changed.notify_waiters();
        }
        let capture_generation = capture_key.map(|key| match key {
            PendingAudioCaptureKey::Hotkey(generation)
            | PendingAudioCaptureKey::VoiceActivation(generation) => generation,
        });
        let pending_capture = capture_key.and_then(|key| self.take_pending_audio_capture(key));
        let capture_generation = pending_capture
            .as_ref()
            .map(|_| capture_generation.expect("capture key was checked"));
        Ok(DictationReservation {
            session_id,
            resources: starting_resources,
            inserter,
            capture_generation,
            pending_capture,
            host_context,
        })
    }

    pub async fn start_dictation_with_options(
        &self,
        options: DictationStartOptions,
    ) -> Result<SessionId, BackendError> {
        let reservation =
            self.reserve_dictation_session(options.insert_text, options.output_target, None)?;
        self.start_reserved_dictation(reservation, options).await
    }

    async fn start_reserved_dictation(
        &self,
        reservation: DictationReservation,
        options: DictationStartOptions,
    ) -> Result<SessionId, BackendError> {
        let DictationReservation {
            session_id,
            resources: starting_resources,
            inserter,
            capture_generation,
            mut pending_capture,
            host_context,
        } = reservation;
        if options.output_target == DictationOutputTarget::CloudNote {
            if let Err(error) = self.set_dictation_cloud_note(session_id, true) {
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                self.reset_dictation_session(session_id);
                return Err(error);
            }
        }
        let context = match self
            .capture_dictation_context_with_host(
                &options,
                DictationContextPurpose::Dictation,
                host_context.as_ref(),
            )
            .await
        {
            Ok(context) => Arc::new(context),
            Err(error) => {
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                self.mark_dictation_failed(session_id, &error);
                self.reset_dictation_session(session_id);
                return Err(error);
            }
        };
        let activation = self
            .voice_activation_session
            .lock()
            .expect("voice activation session lock poisoned")
            .as_ref()
            .filter(|session| session.session_id == session_id)
            .cloned();
        let context = if let Some(activation) = activation {
            let mut context = (*context).clone();
            context.voice_activation = Some(activation.commands);
            if let Some(timeout) = activation.timeout {
                let timeout_ms = timeout.as_millis() as u64;
                context.recording.silence_after_ms = Some(
                    context
                        .recording
                        .silence_after_ms
                        .map_or(timeout_ms, |general| general.min(timeout_ms)),
                );
            }
            Arc::new(context)
        } else {
            context
        };
        let context = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            if let Err(error) = ensure_running(&state) {
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                self.voice_sessions.release(session_id);
                return Err(error);
            }
            if state.dictation.session_id != Some(session_id)
                || state.dictation.phase != DictationPhase::Starting
            {
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                self.voice_sessions.release(session_id);
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "dictation was cancelled while capturing its context",
                ));
            }
            let context = match state.dictation_translation_requested {
                Some(requested) => Arc::new(context.with_translation_requested(requested)),
                None => context,
            };
            state.silence_monitor = context.recording.silence_after_ms.map(|silence_ms| {
                let started_at = std::time::Instant::now();
                SilenceMonitor {
                    session_id,
                    started_at,
                    detector: if context.voice_activation.is_some() {
                        crate::silence_auto_stop::SilenceAutoStop::after_speech(
                            std::time::Duration::from_millis(silence_ms),
                            started_at,
                        )
                    } else {
                        crate::silence_auto_stop::SilenceAutoStop::new(
                            std::time::Duration::from_millis(silence_ms),
                            started_at,
                        )
                    },
                }
            });
            let context = match state.dictation_start_output_target {
                Some(target) => Arc::new(context.with_output_target(target)),
                None => context,
            };
            let context = Arc::new(context.with_capture_generation(capture_generation));
            state.dictation.translation_active = context.polish.translation_active;
            state.dictation_context = Some(Arc::clone(&context));
            state.dictation_start_output_target = None;
            context
        };

        if let Err(error) = self
            .deps
            .host_actions
            .request(HostAction::ShowDictationFeedback)
        {
            self.schedule_pending_audio_capture_discard(pending_capture.take());
            self.mark_dictation_failed(session_id, &error);
            self.reset_dictation_session(session_id);
            return Err(error);
        }
        if context.insertion.enabled {
            let insertion_context = Arc::clone(&context);
            let task_spawner = Arc::clone(&self.deps.task_spawner);
            let resources = Arc::clone(&starting_resources);
            let preparing: futures_util::future::BoxFuture<
                'static,
                Result<Arc<ActiveTextInsertion>, BackendError>,
            > = Box::pin(async move {
                let platform = inserter
                    .begin(session_id, Arc::clone(&insertion_context))
                    .await?;
                Ok(ActiveTextInsertion::new(
                    session_id,
                    platform,
                    &insertion_context,
                    task_spawner,
                    resources,
                ))
            });
            let preparation = futures_util::FutureExt::shared(preparing);
            {
                // Publish preparation while ownership is still checked under
                // the same state lock cancellation uses. No native effect is
                // polled before its cancellable registry entry exists.
                let state = self.state.read().expect("backend state lock poisoned");
                if let Err(error) = ensure_active_session(&state, session_id) {
                    drop(state);
                    self.schedule_pending_audio_capture_discard(pending_capture.take());
                    return Err(error);
                }
                self.text_insertions
                    .lock()
                    .expect("text insertion registry lock poisoned")
                    .insert(session_id, preparation.clone());
            }
            let prepared = preparation.await;
            let still_active = self
                .state
                .read()
                .expect("backend state lock poisoned")
                .dictation
                .session_id
                == Some(session_id);
            if !still_active {
                let cleanup_result = self.cancel_text_insertion(session_id).await;
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                return match cleanup_result {
                    Ok(()) => Err(BackendError::new(
                        BackendErrorCode::Cancelled,
                        "dictation session was cancelled while insertion was starting",
                    )),
                    Err(error) => Err(error),
                };
            }
            match prepared {
                Ok(_) => {}
                Err(error) => {
                    self.text_insertions
                        .lock()
                        .expect("text insertion registry lock poisoned")
                        .remove(&session_id);
                    self.mark_dictation_failed(session_id, &error);
                    self.persist_failed_dictation(
                        &context,
                        session_id,
                        "insertFailed",
                        String::new(),
                        String::new(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                    );
                    let _ = self.hide_dictation_feedback(session_id);
                    self.schedule_pending_audio_capture_discard(pending_capture.take());
                    self.reset_dictation_session(session_id);
                    return Err(error);
                }
            }
        }
        if context.output_target != DictationOutputTarget::ForegroundApp {
            if let Err(error) = self.persist_recording_started_if_starting(&context, session_id) {
                self.schedule_pending_audio_capture_discard(pending_capture.take());
                return Err(error);
            }
        }
        let engine = Arc::clone(&self.deps.dictation_engine);
        let engine_context = Arc::clone(&context);
        let progress = self.engine_progress_sink();
        let resources = Arc::clone(&starting_resources);
        // Recorder startup can own a blocking native operation even when the
        // start caller disappears. Keep its hold in the executor, and perform
        // late cancellation here rather than relying on that caller to resume.
        let starting = own_voice_effect(
            &self.deps.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                if _resources.cancel.is_cancelled() {
                    if let Some(capture) = pending_capture.take() {
                        let _ = capture.discard().await;
                    }
                    return Err(VoiceCaptureLifecycle::cancelled_error());
                }
                let result = match pending_capture.take() {
                    Some(capture) => {
                        engine
                            .start_with_audio_capture(session_id, engine_context, progress, capture)
                            .await
                    }
                    None => engine.start(session_id, engine_context, progress).await,
                };
                if _resources.cancel.is_cancelled() {
                    let _ = engine.cancel(session_id).await;
                    return Err(VoiceCaptureLifecycle::cancelled_error());
                }
                result
            }),
        );
        if let Err(error) = starting.await {
            if error.code != BackendErrorCode::Cancelled {
                self.mark_dictation_failed(session_id, &error);
                self.persist_failed_dictation(
                    &context,
                    session_id,
                    "transcribeFailed",
                    String::new(),
                    String::new(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                );
            } else {
                self.reconcile_aborted_recording_draft(session_id, &context);
            }
            let _ = self.cancel_session_adapters(session_id).await;
            let _ = self.hide_dictation_feedback(session_id);
            self.reset_dictation_session(session_id);
            return Err(error);
        }
        let started = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            if state.dictation.session_id == Some(session_id)
                && state.dictation.phase == DictationPhase::Starting
            {
                state.dictation.phase = DictationPhase::Recording;
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::DictationStateChanged(state.dictation.clone()),
                );
                self.phase_changed.notify_waiters();
                true
            } else {
                false
            }
        };
        if !started {
            let _ = self.cancel_session_adapters(session_id).await;
            self.reconcile_aborted_recording_draft(session_id, &context);
            self.cleanup_cloud_note(session_id, true)?;
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "dictation session was cancelled while the engine was starting",
            ));
        }
        Ok(session_id)
    }

    pub async fn stop_dictation(&self) -> Result<DictationResult, BackendError> {
        self.stop_dictation_session_with_options(None, DictationStopOptions::default(), None)
            .await
    }

    pub async fn stop_dictation_with_options(
        &self,
        options: DictationStopOptions,
    ) -> Result<DictationResult, BackendError> {
        self.stop_dictation_session_with_options(None, options, None)
            .await
    }

    pub async fn stop_dictation_session(
        &self,
        session_id: SessionId,
    ) -> Result<DictationResult, BackendError> {
        self.stop_dictation_session_with_options(
            Some(session_id),
            DictationStopOptions::default(),
            None,
        )
        .await
    }

    /// Preserve the recorded question while releasing Dictation before QA takes over.
    pub async fn stop_dictation_for_qa(
        &self,
        session_id: SessionId,
    ) -> Result<DictationResult, BackendError> {
        self.stop_dictation_session_with_options(
            Some(session_id),
            DictationStopOptions::default(),
            Some(DictationOutputTarget::Qa),
        )
        .await
    }

    pub async fn stop_dictation_session_with_options(
        &self,
        mut expected_session_id: Option<SessionId>,
        options: DictationStopOptions,
        output_target: Option<DictationOutputTarget>,
    ) -> Result<DictationResult, BackendError> {
        let (session_id, context, context_changed) = loop {
            // Register before inspecting the phase so a Starting -> Recording
            // transition cannot be lost between the state read and await.
            let changed = self.phase_changed.notified();
            let ready = {
                let mut state = self.state.write().expect("backend state lock poisoned");
                ensure_running(&state)?;
                let session_id = state.dictation.session_id.ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "no active dictation session",
                    )
                })?;
                // A caller without an explicit ID means "stop this session",
                // not "stop whichever session exists after Starting wakes".
                // Bind under the state lock before the first await: cancelling
                // A and starting B must never retarget A's queued stop to B.
                if *expected_session_id.get_or_insert(session_id) != session_id {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "dictation stop targets a different session",
                    ));
                }
                match state.dictation.phase {
                    DictationPhase::Starting => None,
                    DictationPhase::Recording => {
                        let captured = state.dictation_context.clone().ok_or_else(|| {
                            BackendError::new(
                                BackendErrorCode::Internal,
                                "active dictation session has no captured context",
                            )
                        })?;
                        let target = output_target.unwrap_or_else(|| match options.quick_note {
                            Some(true) => DictationOutputTarget::QuickNote,
                            Some(false)
                                if captured.output_target != DictationOutputTarget::CloudNote =>
                            {
                                DictationOutputTarget::ForegroundApp
                            }
                            None if captured.output_target == DictationOutputTarget::Undecided => {
                                DictationOutputTarget::ForegroundApp
                            }
                            None | Some(false) => captured.output_target,
                        });
                        let targeted = captured.with_output_target(target);
                        let context = match options.translation_requested {
                            Some(requested) => {
                                Arc::new(targeted.with_translation_requested(requested))
                            }
                            None => Arc::new(targeted),
                        };
                        let context = match options.raw_requested {
                            Some(true) => Arc::new(context.with_raw_requested(true)),
                            _ => context,
                        };
                        let context_changed =
                            std::mem::take(&mut state.dictation_destination_changed)
                                || state.dictation_translation_requested.take().is_some()
                                || state.dictation_context.as_ref().is_some_and(|previous| {
                                    previous.polish.translation_active
                                        != context.polish.translation_active
                                        || previous.polish.mode != context.polish.mode
                                        || previous.polish.style_system_prompt
                                            != context.polish.style_system_prompt
                                        || previous.output_target != context.output_target
                                });
                        state.dictation_context = Some(Arc::clone(&context));
                        state.dictation.translation_active = context.polish.translation_active;
                        state.dictation.phase = DictationPhase::Transcribing;
                        self.events.publish(
                            Some(session_id),
                            BackendEventKind::DictationStateChanged(state.dictation.clone()),
                        );
                        self.phase_changed.notify_waiters();
                        Some((session_id, context, context_changed))
                    }
                    _ => {
                        return Err(BackendError::new(
                            BackendErrorCode::Busy,
                            "dictation session is already being finalized",
                        ));
                    }
                }
            };
            if let Some(session) = ready {
                break session;
            }
            let session_id = expected_session_id.expect("stop is bound to an active session");
            let resources = self.voice_sessions.hold_resources(session_id)?;
            let engine = Arc::clone(&self.deps.dictation_engine);
            let state = Arc::clone(&self.state);
            let phase_changed = Arc::clone(&self.phase_changed);
            own_voice_effect(
                &self.deps.task_spawner,
                Box::pin(async move {
                    let _resources = resources;
                    loop {
                        let changed = phase_changed.notified();
                        match engine.stop_recording(session_id).await {
                            Ok(false) => {}
                            // finish() reports cached stop errors without discarding recovery audio.
                            Ok(true) | Err(_) => return Ok(()),
                        }
                        {
                            let state = state.read().expect("backend state lock poisoned");
                            if state.dictation.session_id != Some(session_id)
                                || state.dictation.phase != DictationPhase::Starting
                            {
                                return Ok(());
                            }
                        }
                        tokio::select! {
                            _ = changed => {}
                            _ = _resources.cancel.cancelled() => return Ok(()),
                        }
                    }
                }),
            )
            .await?;
            changed.await;
        };

        // Android's undecided capture prepared a non-streaming insertion
        // resource so an ordinary tap can still finish normally. A quick-note
        // gesture changes the target at the stop boundary, so cancel that
        // unused foreground insertion before the engine finalizes.
        if !context.insertion.enabled {
            let _ = self.cancel_text_insertion(session_id).await;
        }

        if context_changed {
            if let Err(error) = self
                .deps
                .dictation_engine
                .update_context(session_id, Arc::clone(&context))
                .await
            {
                self.mark_dictation_failed(session_id, &error);
                self.persist_failed_dictation(
                    &context,
                    session_id,
                    "polishFailed",
                    String::new(),
                    String::new(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                );
                let _ = self.cancel_session_adapters(session_id).await;
                let _ = self.hide_dictation_feedback(session_id);
                self.reset_dictation_session(session_id);
                self.cleanup_voice_activation_after_dictation(session_id)
                    .await;
                return Err(error);
            }
        }

        let progress = self.engine_progress_sink();
        let resources = self.voice_sessions.hold_resources(session_id)?;
        let finishing = self.deps.dictation_engine.finish(session_id, progress);
        // finish() has taken the recorder out of the engine's registry. Cancel
        // can no longer join that stop through engine.cancel(), so its original
        // owner must retain the shared lease even without a text insertion.
        let mut engine_result = match own_voice_effect(
            &self.deps.task_spawner,
            Box::pin(async move {
                if resources.cancel.is_cancelled() {
                    return Ok(Err(crate::ports::EngineFailure::from(
                        VoiceCaptureLifecycle::cancelled_error(),
                    )));
                }
                let result = finishing.await;
                Ok(if resources.cancel.is_cancelled() {
                    Err(crate::ports::EngineFailure::from(
                        VoiceCaptureLifecycle::cancelled_error(),
                    ))
                } else {
                    result
                })
            }),
        )
        .await
        .map_err(crate::ports::EngineFailure::from)
        .and_then(std::convert::identity)
        {
            Ok(result) => result,
            Err(failure) => {
                let asr_call_label = failure.asr_call_label.clone();
                let llm_call_label = failure.llm_call_label.clone();
                let error = failure.error;
                if error.code != BackendErrorCode::Cancelled {
                    self.mark_dictation_failed(session_id, &error);
                    let raw_text = failure
                        .raw_text
                        .filter(|text| !text.trim().is_empty())
                        .unwrap_or_else(|| {
                            self.state
                                .read()
                                .expect("backend state lock poisoned")
                                .transcripts
                                .get(&session_id)
                                .map(|transcript| transcript.text().to_owned())
                                .unwrap_or_default()
                        });
                    let (error_code, final_text, llm_used) = match failure.stage {
                        EngineFailureStage::Transcribing => {
                            ("transcribeFailed", String::new(), false)
                        }
                        EngineFailureStage::Polishing => ("polishFailed", raw_text.clone(), true),
                    };
                    self.persist_failed_dictation(
                        &context,
                        session_id,
                        error_code,
                        raw_text,
                        final_text,
                        None,
                        failure.duration_ms,
                        failure.asr_ms,
                        failure.polish_ms,
                        failure.has_audio_recording,
                        llm_used,
                        asr_call_label,
                        llm_call_label,
                    );
                }
                // A failed finish can leave provider/recorder state in the engine.
                // Close both adapters before releasing this session's voice lease.
                let _ = self.cancel_session_adapters(session_id).await;
                let _ = self.hide_dictation_feedback(session_id);
                self.reset_dictation_session(session_id);
                self.cleanup_voice_activation_after_dictation(session_id)
                    .await;
                return Err(error);
            }
        };

        if context.pipeline_mode == crate::shared_types::PipelineMode::Multimodal {
            if let Some(commands) = context.voice_activation.as_ref() {
                engine_result.raw_text = commands.clean(&engine_result.raw_text);
                engine_result.polished_text = commands.clean(&engine_result.polished_text);
                if let Some(text) = engine_result.asr_transcript.as_mut() {
                    *text = commands.clean(text);
                }
            }
        }

        if context.output_target == DictationOutputTarget::CloudNote && engine_result.polish_failed
        {
            let error =
                BackendError::new(BackendErrorCode::Provider, "cloud note polishing failed");
            self.mark_dictation_failed(session_id, &error);
            let _ = self.hide_dictation_feedback(session_id);
            self.reset_dictation_session(session_id);
            self.cleanup_voice_activation_after_dictation(session_id)
                .await;
            self.cleanup_cloud_note(session_id, true)?;
            return Err(error);
        }

        if engine_result.raw_text.trim().is_empty() {
            let error = BackendError::new(
                BackendErrorCode::Provider,
                "transcription provider returned an empty transcript",
            );
            self.mark_dictation_failed(session_id, &error);
            self.persist_failed_dictation(
                &context,
                session_id,
                "emptyTranscript",
                engine_result.raw_text.clone(),
                String::new(),
                None,
                Some(engine_result.duration_ms),
                engine_result.asr_ms,
                None,
                engine_result.has_audio_recording,
                false,
                engine_result.asr_call_label.clone(),
                engine_result.llm_call_label.clone(),
            );
            let _ = self.cancel_text_insertion(session_id).await;
            let _ = self.hide_dictation_feedback(session_id);
            self.reset_dictation_session(session_id);
            self.cleanup_voice_activation_after_dictation(session_id)
                .await;
            return Err(error);
        }

        engine_result.polished_text = crate::streaming_insert::apply_chinese_script_preference(
            &engine_result.polished_text,
            context.polish.chinese_script_preference,
        );
        let correction_rules = match self.correction_rules.list() {
            Ok(rules) => rules,
            Err(error) => {
                log::warn!(
                    "failed to load correction rules for completed dictation: {error}; continuing without correction"
                );
                Vec::new()
            }
        };
        let streamed_text_is_visible = self
            .text_insertions
            .lock()
            .expect("text insertion registry lock poisoned")
            .get(&session_id)
            .and_then(|preparation| preparation.peek())
            .and_then(|result| result.as_ref().ok())
            .is_some_and(|insertion| insertion.has_written_text());
        if !correction_rules.is_empty() && !streamed_text_is_visible {
            engine_result.polished_text =
                apply_correction_rules(&engine_result.polished_text, &correction_rules);
        }

        // Cancellation may happen while ASR/LLM work is in flight. Never
        // insert a result after the session has been cancelled or replaced.
        let cancelled_before_insertion = {
            let state = self.state.read().expect("backend state lock poisoned");
            state.dictation.session_id != Some(session_id)
                || !matches!(
                    state.dictation.phase,
                    DictationPhase::Transcribing | DictationPhase::Polishing
                )
        };
        if cancelled_before_insertion {
            self.cleanup_voice_activation_after_dictation(session_id)
                .await;
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "dictation session was cancelled before insertion",
            ));
        }

        {
            let mut state = self.state.write().expect("backend state lock poisoned");
            ensure_active_session(&state, session_id)?;
            state.dictation.phase = if context.insertion.enabled {
                DictationPhase::Inserting
            } else {
                DictationPhase::Completed
            };
            self.events.publish(
                Some(session_id),
                BackendEventKind::DictationStateChanged(state.dictation.clone()),
            );
            self.phase_changed.notify_waiters();
        }

        let insert_outcome = if context.insertion.enabled {
            log::info!(
                "[insert-diagnostics] session_id={} phase=finalizing raw_chars={} raw_hash={} final_chars={} final_hash={} polish_failed={}",
                session_id,
                engine_result.raw_text.chars().count(),
                crate::output_cleaning::privacy_text_hash(&engine_result.raw_text),
                engine_result.polished_text.chars().count(),
                crate::output_cleaning::privacy_text_hash(&engine_result.polished_text),
                engine_result.polish_failed,
            );
            match self
                .finish_text_insertion(session_id, engine_result.polished_text.clone())
                .await
            {
                Ok(outcome) => Some(outcome),
                Err(error) => {
                    self.mark_dictation_failed(session_id, &error);
                    self.persist_failed_dictation(
                        &context,
                        session_id,
                        "insertFailed",
                        engine_result.raw_text.clone(),
                        engine_result.polished_text.clone(),
                        engine_result.polish_source.clone(),
                        Some(engine_result.duration_ms),
                        engine_result.asr_ms,
                        engine_result.polish_ms,
                        engine_result.has_audio_recording,
                        context.uses_llm_polisher()
                            && !context.skips_polish_for_short_text(&engine_result.raw_text),
                        engine_result.asr_call_label.clone(),
                        engine_result.llm_call_label.clone(),
                    );
                    let _ = self.cancel_text_insertion(session_id).await;
                    let _ = self.hide_dictation_feedback(session_id);
                    self.reset_dictation_session(session_id);
                    self.cleanup_voice_activation_after_dictation(session_id)
                        .await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        if let Some(outcome) = insert_outcome {
            log::info!(
                "[insert-diagnostics] session_id={} phase=completed outcome={:?}",
                session_id,
                outcome,
            );
        }
        let result = DictationResult {
            session_id,
            raw_text: engine_result.raw_text.clone(),
            polished_text: engine_result.polished_text.clone(),
            polish_source: engine_result.polish_source.clone(),
            duration_ms: engine_result.duration_ms,
            inserted: insert_outcome
                .map(InsertOutcome::into_status)
                .unwrap_or(crate::types::InsertStatus::NotRequested),
        };

        let session_replaced = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            let replaced = state.dictation.session_id != Some(session_id)
                || state.dictation.phase
                    != if context.insertion.enabled {
                        DictationPhase::Inserting
                    } else {
                        DictationPhase::Completed
                    };
            if !replaced {
                state.dictation.phase = DictationPhase::Completed;
                state.dictation.message = Some(match insert_outcome {
                    Some(InsertOutcome::Inserted) => "inserted".to_string(),
                    Some(InsertOutcome::PasteSent) => "paste_sent".to_string(),
                    Some(InsertOutcome::CopiedFallback) => "copied_fallback".to_string(),
                    None => "insertion_not_requested".to_string(),
                });
                if matches!(insert_outcome, Some(InsertOutcome::CopiedFallback)) {
                    self.events.publish(
                        Some(session_id),
                        BackendEventKind::InsertFallback(crate::types::InsertFallbackPayload {
                            reason: "clipboard_fallback".to_string(),
                            copied_text: Some(engine_result.polished_text.clone()),
                        }),
                    );
                }
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::DictationCompleted(result.clone()),
                );
                self.events.publish(
                    Some(session_id),
                    BackendEventKind::DictationStateChanged(state.dictation.clone()),
                );
                self.phase_changed.notify_waiters();
            }
            replaced
        };
        if session_replaced {
            self.cleanup_voice_activation_after_dictation(session_id)
                .await;
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "dictation session was replaced before completion",
            ));
        }
        self.arm_edit_observation(
            session_id,
            context.insertion.observe_edits,
            insert_outcome,
            &result.polished_text,
        );
        let host_result = self.hide_dictation_feedback(session_id);
        // Reuse the same identity-checked reset as failure/cancellation. A
        // delayed successful completion must not clear its successor's state,
        // transcript, silence detector or physical-hotkey generation.
        self.reset_dictation_session(session_id);
        self.cleanup_voice_activation_after_dictation(session_id)
            .await;
        self.persist_completed_dictation(&context, &result, insert_outcome, &engine_result);
        self.cleanup_cloud_note(session_id, true)?;
        host_result?;
        Ok(result)
    }

    fn history_source_for_context(context: &DictationContext) -> HistorySource {
        if context.output_target == DictationOutputTarget::QuickNote {
            HistorySource::QuickNote
        } else {
            HistorySource::Voice
        }
    }

    fn persist_recording_started(&self, context: &DictationContext, session_id: SessionId) {
        if context.output_target == DictationOutputTarget::CloudNote
            || self.is_cloud_note(session_id)
        {
            return;
        }
        let preferences = self.get_preferences();
        let front_app =
            crate::shared_types::split_front_app_opt(context.polish.front_app.as_deref());
        let pipeline_mode = match context.pipeline_mode {
            crate::shared_types::PipelineMode::Traditional => "traditional",
            crate::shared_types::PipelineMode::Multimodal => "multimodal",
        };
        let session = DictationSession {
            id: session_id.to_string(),
            created_at: self.clock.now_utc().to_rfc3339(),
            source: Self::history_source_for_context(context),
            raw_transcript: String::new(),
            asr_transcript: None,
            final_text: String::new(),
            mode: context.polish.mode,
            style_pack_id: Some(context.polish.style_pack_id.clone()),
            translation_active: context.polish.translation_active,
            polish_source: None,
            app_bundle_id: front_app.bundle_id,
            app_name: front_app.name,
            insert_status: HistoryInsertStatus::NotRequested,
            error_code: Some("recording".to_string()),
            duration_ms: None,
            dictionary_entry_count: None,
            has_audio_recording: Some(context.recording.archive_enabled),
            asr_provider: Some(context.asr.provider_id.clone()),
            asr_model: context.asr.model.clone(),
            llm_provider: None,
            llm_model: None,
            pipeline_mode: Some(pipeline_mode.to_string()),
            asr_ms: None,
            polish_ms: None,
        };
        if let Err(error) = self.upsert_history(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            log::warn!("failed to persist recording-start history: {error}");
        }
    }

    fn persist_recording_started_if_starting(
        &self,
        context: &DictationContext,
        session_id: SessionId,
    ) -> Result<(), BackendError> {
        let state = self.state.write().expect("backend state lock poisoned");
        if state.dictation.session_id != Some(session_id)
            || state.dictation.phase != DictationPhase::Starting
        {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "dictation session was cancelled before recording history was persisted",
            ));
        }
        // Keep the state write lock across the draft write. Cancellation also
        // needs this lock before it can update the same history row, so it
        // cannot overwrite a terminal result with a late recording draft.
        self.persist_recording_started(context, session_id);
        Ok(())
    }

    fn remove_recording_draft(&self, session_id: SessionId) {
        let id = session_id.to_string();
        if self
            .list_history()
            .ok()
            .and_then(|entries| entries.into_iter().find(|entry| entry.id == id))
            .is_some_and(|entry| entry.error_code.as_deref() == Some("recording"))
        {
            let _ = self.delete_history(&id);
        }
    }

    fn reconcile_aborted_recording_draft(&self, session_id: SessionId, context: &DictationContext) {
        // Quick Note cancellations must keep the provisional row (rewritten to
        // cancelled) so playback/export still work. A racing cancel_dictation may
        // already have done that rewrite; only touch still-provisional rows.
        if context.output_target == DictationOutputTarget::QuickNote {
            let id = session_id.to_string();
            if let Some(mut entry) = self
                .list_history()
                .ok()
                .and_then(|entries| entries.into_iter().find(|entry| entry.id == id))
            {
                if entry.error_code.as_deref() == Some("recording") {
                    entry.error_code = Some("cancelled".to_string());
                    entry.has_audio_recording = Some(true);
                    let _ = self.update_history_entry(entry);
                }
            }
            return;
        }
        self.remove_recording_draft(session_id);
    }

    fn history_created_at(&self, session_id: &str) -> String {
        self.list_history()
            .ok()
            .and_then(|entries| entries.into_iter().find(|entry| entry.id == session_id))
            .map(|entry| entry.created_at)
            .unwrap_or_else(|| self.clock.now_utc().to_rfc3339())
    }

    fn persist_completed_dictation(
        &self,
        context: &DictationContext,
        result: &DictationResult,
        insert_outcome: Option<InsertOutcome>,
        engine_result: &crate::ports::EngineResult,
    ) {
        if context.output_target == DictationOutputTarget::Qa
            || context.output_target == DictationOutputTarget::VoiceEdit
            || context.output_target == DictationOutputTarget::CloudNote
            || self.is_cloud_note(result.session_id)
        {
            self.remove_recording_draft(result.session_id);
            return;
        }
        let preferences = self.get_preferences();
        let dictionary_entry_count = match self.record_vocabulary_hits(&result.polished_text) {
            Ok(hits) => Some(hits.min(u32::MAX as u64) as u32),
            Err(error) => {
                log::warn!("failed to record vocabulary hits for completed dictation: {error}");
                None
            }
        };
        let front_app =
            crate::shared_types::split_front_app_opt(context.polish.front_app.as_deref());
        let insert_status = match insert_outcome {
            Some(InsertOutcome::Inserted) => HistoryInsertStatus::Inserted,
            Some(InsertOutcome::PasteSent) => HistoryInsertStatus::PasteSent,
            Some(InsertOutcome::CopiedFallback) => HistoryInsertStatus::CopiedFallback,
            None => HistoryInsertStatus::NotRequested,
        };
        let pipeline_mode = match context.pipeline_mode {
            crate::shared_types::PipelineMode::Traditional => "traditional",
            crate::shared_types::PipelineMode::Multimodal => "multimodal",
        };
        let llm_used = context.uses_llm_polisher()
            && !context.skips_polish_for_short_text(&engine_result.raw_text);
        let attribution = HistoryProviderAttribution::from_context(
            context,
            llm_used,
            engine_result.asr_ms,
            engine_result.polish_ms,
            engine_result.asr_call_label.as_ref(),
            engine_result.llm_call_label.as_ref(),
        );
        let session = DictationSession {
            id: result.session_id.to_string(),
            created_at: self.history_created_at(&result.session_id.to_string()),
            source: Self::history_source_for_context(context),
            raw_transcript: result.raw_text.clone(),
            asr_transcript: engine_result.asr_transcript.clone(),
            final_text: result.polished_text.clone(),
            mode: context.polish.mode,
            style_pack_id: Some(context.polish.style_pack_id.clone()),
            translation_active: context.polish.translation_active,
            polish_source: result.polish_source.clone(),
            app_bundle_id: front_app.bundle_id,
            app_name: front_app.name,
            insert_status,
            error_code: engine_result
                .polish_failed
                .then(|| "polishFailed".to_string()),
            duration_ms: Some(result.duration_ms),
            dictionary_entry_count,
            has_audio_recording: engine_result.has_audio_recording,
            asr_provider: attribution.asr_provider,
            asr_model: attribution.asr_model,
            llm_provider: attribution.llm_provider,
            llm_model: attribution.llm_model,
            pipeline_mode: Some(pipeline_mode.to_string()),
            asr_ms: attribution.asr_ms,
            polish_ms: attribution.polish_ms,
        };
        if let Err(error) = self.upsert_history(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            log::warn!("failed to persist completed dictation history: {error}");
        }
        if let Err(error) = self.record_activity(
            &self.clock.today_local().format("%Y-%m-%d").to_string(),
            result.polished_text.chars().count() as u64,
            result.duration_ms,
        ) {
            log::warn!("failed to persist completed dictation activity: {error}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn persist_failed_dictation(
        &self,
        context: &DictationContext,
        session_id: SessionId,
        error_code: &str,
        raw_text: String,
        final_text: String,
        polish_source: Option<String>,
        duration_ms: Option<u64>,
        asr_ms: Option<u64>,
        polish_ms: Option<u64>,
        has_audio_recording: Option<bool>,
        llm_used: bool,
        asr_call_label: Option<crate::auxiliary::AsrCallLabel>,
        llm_call_label: Option<crate::polish::LlmCallLabel>,
    ) {
        if context.output_target == DictationOutputTarget::CloudNote
            || self.is_cloud_note(session_id)
        {
            return;
        }
        let preferences = self.get_preferences();
        let front_app =
            crate::shared_types::split_front_app_opt(context.polish.front_app.as_deref());
        let pipeline_mode = match context.pipeline_mode {
            crate::shared_types::PipelineMode::Traditional => "traditional",
            crate::shared_types::PipelineMode::Multimodal => "multimodal",
        };
        let attribution = HistoryProviderAttribution::from_context(
            context,
            llm_used,
            asr_ms,
            polish_ms,
            asr_call_label.as_ref(),
            llm_call_label.as_ref(),
        );
        let session = DictationSession {
            id: session_id.to_string(),
            created_at: self.history_created_at(&session_id.to_string()),
            source: Self::history_source_for_context(context),
            raw_transcript: raw_text.clone(),
            asr_transcript: Some(raw_text),
            final_text,
            mode: context.polish.mode,
            style_pack_id: Some(context.polish.style_pack_id.clone()),
            translation_active: context.polish.translation_active,
            polish_source,
            app_bundle_id: front_app.bundle_id,
            app_name: front_app.name,
            insert_status: if context.insertion.enabled {
                HistoryInsertStatus::Failed
            } else {
                HistoryInsertStatus::NotRequested
            },
            error_code: Some(error_code.to_string()),
            duration_ms,
            dictionary_entry_count: None,
            has_audio_recording,
            asr_provider: attribution.asr_provider,
            asr_model: attribution.asr_model,
            llm_provider: attribution.llm_provider,
            llm_model: attribution.llm_model,
            pipeline_mode: Some(pipeline_mode.to_string()),
            asr_ms: attribution.asr_ms,
            polish_ms: attribution.polish_ms,
        };
        if let Err(error) = self.upsert_history(
            session,
            preferences.history_retention_days,
            preferences.history_max_entries,
        ) {
            log::warn!("failed to persist dictation failure history: {error}");
        }
    }

    fn hide_dictation_feedback(&self, session_id: SessionId) -> Result<(), BackendError> {
        let state = self.state.read().expect("backend state lock poisoned");
        if state
            .dictation
            .session_id
            .is_some_and(|current| current != session_id)
        {
            return Ok(());
        }
        // Cancellation resets its snapshot before awaiting native cleanup, so
        // Idle still permits its Hide. Any live successor suppresses it. Keep
        // the check and synchronous Host enqueue under one read guard so the
        // successor's Show cannot overtake an old terminal action. All success,
        // failure, cancellation and shutdown exits share this same boundary.
        self.deps
            .host_actions
            .request(HostAction::HideDictationFeedback)
    }

    fn reset_dictation_session(&self, session_id: SessionId) {
        // Match modifier-grace dispatch's hotkey -> state lock order. Clearing
        // the physical generation must precede exposing Idle/releasing audio;
        // otherwise an accepted successor press can be erased by this terminal.
        let mut hotkey = self
            .hotkey
            .lock()
            .expect("hotkey interpreter lock poisoned");
        let mut state = self.state.write().expect("backend state lock poisoned");
        if state.dictation.session_id != Some(session_id) {
            return;
        }
        state.dictation = DictationStateSnapshot::default();
        state.dictation_context = None;
        state.dictation_start_output_target = None;
        state.silence_monitor = None;
        state.transcripts.remove(&session_id);
        state.recording_fault_cleanup = None;
        hotkey.terminal(std::time::Instant::now());
        self.phase_changed.notify_waiters();
        drop(state);
        if let Err(error) = self.cleanup_cloud_note(session_id, false) {
            log::warn!("{error}");
        }
        self.voice_sessions.release(session_id);
    }

    fn cancel_session_adapters(
        &self,
        session_id: SessionId,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        let engine = Arc::clone(&self.deps.dictation_engine);
        let resources = self.voice_sessions.hold_resources(session_id).ok();
        let insertion = self.cancel_text_insertion(session_id);
        // Revoke startup before scheduling cleanup. A queued engine.start must
        // observe cancellation even when its original caller has disappeared.
        // The owned cleanup hold keeps Busy until native stop/restore settles.
        self.voice_sessions.release(session_id);
        own_voice_effect(
            &self.deps.task_spawner,
            Box::pin(async move {
                let _resources = resources;
                let engine_result = engine.cancel(session_id).await;
                let inserter_result = insertion.await;
                engine_result?;
                inserter_result
            }),
        )
    }

    async fn finish_text_insertion(
        &self,
        session_id: SessionId,
        final_text: String,
    ) -> Result<InsertOutcome, BackendError> {
        let insertion = self
            .text_insertions
            .lock()
            .expect("text insertion registry lock poisoned")
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "text insertion session is not active",
                )
            })?;
        let insertion = insertion.await?;
        let result = insertion.finish(final_text).await;
        let keep_for_cleanup = result.is_err();
        let mut insertions = self
            .text_insertions
            .lock()
            .expect("text insertion registry lock poisoned");
        if !keep_for_cleanup
            && insertions
                .get(&session_id)
                .and_then(|preparation| preparation.peek())
                .is_some_and(
                    |current| matches!(current, Ok(current) if Arc::ptr_eq(current, &insertion)),
                )
        {
            insertions.remove(&session_id);
        }
        result
    }

    fn cancel_text_insertion(
        &self,
        session_id: SessionId,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        let preparation = self
            .text_insertions
            .lock()
            .expect("text insertion registry lock poisoned")
            .get(&session_id)
            .cloned();
        let text_insertions = Arc::clone(&self.text_insertions);
        let task_spawner = Arc::clone(&self.deps.task_spawner);
        Box::pin(async move {
            let Some(preparation) = preparation else {
                return Ok(());
            };
            let insertion = match preparation.await {
                Ok(insertion) => insertion,
                Err(error) => {
                    // Preparation never created a native session, so there is nothing to retry.
                    let mut insertions = text_insertions
                        .lock()
                        .expect("text insertion registry lock poisoned");
                    if insertions
                        .get(&session_id)
                        .and_then(|current| current.peek())
                        .is_some_and(|current| {
                            matches!(current, Err(current_error) if current_error == &error)
                        })
                    {
                        insertions.remove(&session_id);
                    }
                    return Err(error);
                }
            };
            let result = insertion.cancel().await;
            if result.is_ok() || !cfg!(target_os = "windows") {
                // Only Windows TSF retains a native cleanup obligation after an error.
                // Never remove a newer replacement session.
                let mut insertions = text_insertions
                    .lock()
                    .expect("text insertion registry lock poisoned");
                if insertions
                    .get(&session_id)
                    .and_then(|current| current.peek())
                    .is_some_and(
                        |current| matches!(current, Ok(current) if Arc::ptr_eq(current, &insertion)),
                    )
                {
                    insertions.remove(&session_id);
                }
            } else if insertion.try_start_cleanup_retry() {
                let retry_insertion = Arc::clone(&insertion);
                let retry_map = Arc::clone(&text_insertions);
                let retry_session_id = session_id;
                task_spawner.spawn(Box::pin(async move {
                    const CLEANUP_RETRY_WINDOW: std::time::Duration =
                        std::time::Duration::from_secs(30);
                    let mut target_unavailable_since = None;
                    let mut delay_ms = 50u64;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        match retry_insertion.cancel().await {
                            Ok(()) => {
                                let mut insertions = retry_map
                                    .lock()
                                    .expect("text insertion registry lock poisoned");
                                if insertions
                                    .get(&retry_session_id)
                                    .and_then(|current| current.peek())
                                    .is_some_and(|current| {
                                        matches!(current, Ok(current) if Arc::ptr_eq(current, &retry_insertion))
                                    })
                                {
                                    insertions.remove(&retry_session_id);
                                }
                                retry_insertion.finish_cleanup_retry();
                                return;
                            }
                            Err(error) if error.is_target_unavailable() => {
                                let unavailable_since = *target_unavailable_since
                                    .get_or_insert_with(std::time::Instant::now);
                                if unavailable_since.elapsed() < CLEANUP_RETRY_WINDOW {
                                    delay_ms = delay_ms.saturating_mul(2).min(30_000);
                                    continue;
                                }
                                log::error!(
                                    "[insertion] abandoning native cleanup after target remained unavailable"
                                );
                                let mut insertions = retry_map
                                    .lock()
                                    .expect("text insertion registry lock poisoned");
                                if insertions
                                    .get(&retry_session_id)
                                    .and_then(|current| current.peek())
                                    .is_some_and(|current| {
                                        matches!(current, Ok(current) if Arc::ptr_eq(current, &retry_insertion))
                                    })
                                {
                                    insertions.remove(&retry_session_id);
                                }
                                retry_insertion.abandon_cleanup_retry();
                                return;
                            }
                            Err(_) => {
                                // A native cleanup failure may still represent a live composition.
                                // Keep its resource hold and retry indefinitely rather than releasing
                                // it beside a composition that the host may still own.
                                target_unavailable_since = None;
                                delay_ms = delay_ms.saturating_mul(2).min(30_000);
                            }
                        }
                    }
                }));
            }
            result
        })
    }

    pub async fn report_recording_fault(
        &self,
        session_id: SessionId,
        error: BackendError,
    ) -> Result<(), BackendError> {
        let (context, duration_ms, already_failed, raw_text) = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            ensure_running(&state)?;
            if state.dictation.session_id != Some(session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "recording fault belongs to an inactive session",
                ));
            }
            if state.recording_fault_cleanup == Some(session_id) {
                return Ok(());
            }
            let context = state.dictation_context.clone().ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    "active recording fault has no captured context",
                )
            })?;
            state.recording_fault_cleanup = Some(session_id);
            (
                context,
                state.dictation.elapsed_ms,
                state.dictation.phase == DictationPhase::Failed,
                state
                    .transcripts
                    .get(&session_id)
                    .map(|transcript| transcript.text().to_owned())
                    .unwrap_or_default(),
            )
        };
        // RecordingProgressForwarder marks the state Failed immediately so UI
        // feedback is real-time. The adapter then calls this method to perform
        // async cancellation/persistence; accepting that already-Failed phase
        // closes the two halves without emitting a second terminal event.
        if !already_failed {
            self.mark_dictation_failed(session_id, &error);
        }
        let archive = if context.output_target == DictationOutputTarget::CloudNote
            || self.is_cloud_note(session_id)
        {
            None
        } else {
            self.deps
                .dictation_engine
                .retain_failed_recording(session_id)
        };
        let cancel_result = self.cancel_session_adapters(session_id).await;
        self.persist_failed_dictation(
            &context,
            session_id,
            "recordingFailed",
            raw_text,
            String::new(),
            None,
            Some(duration_ms),
            None,
            None,
            archive.as_ref().map(|archive| archive.is_available()),
            false,
            None,
            None,
        );
        let _ = self.hide_dictation_feedback(session_id);
        self.reset_dictation_session(session_id);
        self.cleanup_voice_activation_after_dictation(session_id)
            .await;
        cancel_result
    }

    pub async fn cancel_dictation(
        &self,
        session_id: Option<SessionId>,
    ) -> Result<(), BackendError> {
        let (active, preserve_quick_note) = {
            let mut state = self.state.write().expect("backend state lock poisoned");
            ensure_running(&state)?;
            let active = state.dictation.session_id.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "no active dictation session",
                )
            })?;
            if session_id.is_some() && session_id != Some(active) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "session id does not match the active session",
                ));
            }
            let preserve_quick_note = state
                .dictation_context
                .as_ref()
                .is_some_and(|context| context.output_target == DictationOutputTarget::QuickNote);
            state.dictation.phase = DictationPhase::Cancelled;
            self.events.publish(
                Some(active),
                BackendEventKind::DictationStateChanged(state.dictation.clone()),
            );
            state.dictation = DictationStateSnapshot::default();
            state.dictation_context = None;
            state.dictation_start_output_target = None;
            state.silence_monitor = None;
            state.transcripts.remove(&active);
            self.phase_changed.notify_waiters();
            (active, preserve_quick_note)
        };
        // Settle history before tearing down adapters. The abandoned starter can
        // observe Cancelled as soon as cancel_session_adapters runs; rewriting or
        // deleting the draft first prevents it from racing remove_recording_draft
        // against a still-provisional Quick Note row.
        let history_result = if preserve_quick_note {
            self.list_history().map(|entries| {
                if let Some(mut entry) = entries
                    .into_iter()
                    .find(|entry| entry.id == active.to_string())
                {
                    entry.error_code = Some("cancelled".to_string());
                    entry.has_audio_recording = Some(true);
                    let _ = self.update_history_entry(entry);
                }
            })
        } else {
            // Undecided captures that are explicitly cancelled are not notes.
            let _ = self.delete_history(&active.to_string());
            Ok(())
        };
        let cancel_result = self.cancel_session_adapters(active).await;
        self.cleanup_voice_activation_after_dictation(active).await;
        // The state can already display cancellation, but native audio/input
        // cleanup still owns the shared resource. Reject new capture until that
        // cleanup finishes, including on its error path.
        self.voice_sessions.release(active);
        let host_result = self.hide_dictation_feedback(active);
        let first_error = cancel_result
            .err()
            .or_else(|| host_result.err())
            .or_else(|| history_result.err())
            .or_else(|| self.cleanup_cloud_note(active, false).err());
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn capture_dictation_context(
        &self,
        options: &DictationStartOptions,
        purpose: DictationContextPurpose,
    ) -> Result<DictationContext, BackendError> {
        self.capture_dictation_context_with_host(
            options,
            purpose,
            self.deps.services.host_context.as_ref(),
        )
        .await
    }

    async fn capture_dictation_context_with_host(
        &self,
        options: &DictationStartOptions,
        purpose: DictationContextPurpose,
        host_context: &dyn HostContextAdapter,
    ) -> Result<DictationContext, BackendError> {
        let preferences = self.get_preferences();
        let mut captured_options = options.clone();
        if !preferences.cursor_context_enabled {
            // This switch controls document text, including text supplied by
            // callers. Front-application metadata is still needed for history,
            // application-aware polish, and macOS Auto newline selection.
            captured_options.cursor_context = None;
        }
        if captured_options.front_app.is_none()
            || (preferences.cursor_context_enabled && captured_options.cursor_context.is_none())
        {
            match host_context
                .capture(preferences.cursor_context_enabled)
                .await
            {
                Ok(capture) => {
                    captured_options.front_app = captured_options.front_app.or(capture.front_app);
                    if preferences.cursor_context_enabled {
                        captured_options.cursor_context =
                            captured_options.cursor_context.or(capture.cursor_context);
                    }
                }
                Err(error) => log::warn!("host context capture failed: {error}"),
            }
        }
        let options = &captured_options;
        let style_pack_id = options
            .style_pack_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&preferences.active_style_pack_id);
        let style_pack = self.style_packs.get_or_default_active(style_pack_id)?;
        let hotwords = self
            .vocabulary
            .list()?
            .into_iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.phrase)
            .collect();
        // Auxiliary Voice (Selection / Less) and QA Voice respect multimodal
        // preferences. Skip unused traditional channels so a valid Omni route
        // is not blocked by missing ASR/LLM. QA text needs no microphone
        // provider.
        let pipeline_mode = crate::shared_types::effective_pipeline_mode(
            preferences.multimodal_pipeline_enabled,
            preferences.pipeline_mode,
        );
        let traditional = pipeline_mode == crate::shared_types::PipelineMode::Traditional;
        let active_asr_provider = if traditional && purpose != DictationContextPurpose::QaText {
            self.resolve_dictation_asr_provider(&preferences).await?
        } else {
            crate::dictation_context::ProviderInvocation::for_provider(
                &preferences.active_asr_provider,
            )
        };
        let mut deferred_llm_error = None;
        let skip_llm_at_capture = purpose == DictationContextPurpose::AuxiliaryVoice;
        let active_llm_provider = if traditional && !skip_llm_at_capture {
            match self
                .resolve_session_provider(ProviderSlot::Llm, &preferences.active_llm_provider)
                .await
            {
                Ok(provider) => provider,
                Err(error) => {
                    deferred_llm_error = Some(error);
                    crate::dictation_context::ProviderInvocation::for_provider(
                        &preferences.active_llm_provider,
                    )
                }
            }
        } else {
            crate::dictation_context::ProviderInvocation::for_provider(
                &preferences.active_llm_provider,
            )
        };
        let active_omni_provider = if traditional {
            crate::dictation_context::ProviderInvocation::for_provider(
                &preferences.active_omni_provider,
            )
        } else {
            self.resolve_session_provider(ProviderSlot::Omni, &preferences.active_omni_provider)
                .await?
        };
        let recent_history = if preferences.polish_context_window_minutes == 0 {
            Vec::new()
        } else {
            match self
                .history
                .recent_within_minutes(preferences.polish_context_window_minutes)
            {
                Ok(sessions) => sessions,
                Err(error) => {
                    log::warn!(
                        "failed to capture polish history context; using a single turn: {error}"
                    );
                    Vec::new()
                }
            }
        };
        let mut context = DictationContext::capture(
            &preferences,
            &style_pack,
            DictationProviderInvocations::new(
                active_asr_provider,
                active_llm_provider,
                active_omni_provider,
            ),
            hotwords,
            recent_history,
            options,
        );
        context.pipeline_mode = pipeline_mode;
        // Issue #1032: when the LLM channel is not configured, mark the
        // session as eligible for the Raw fallback instead of dispatching a
        // polish request that cannot succeed. The context keeps its original
        // style until the destination is known, so a later Cloud Note or
        // translation choice can still take the polish path. A configured
        // channel whose call fails still takes the polish fallback path below.
        if purpose == DictationContextPurpose::Dictation
            && traditional
            && deferred_llm_error.is_none()
            && context.uses_llm_polisher()
            && !crate::cloud_providers::llm_channel_configured(
                self.deps.credential_store.as_ref(),
                &context.llm.provider_id,
                &context.llm.provider_type,
                context.llm.model.as_deref(),
            )
            .await
        {
            context = context.with_unconfigured_llm_raw();
        }
        // Pure local: recognition runs on this device and the polish channel
        // is turned off too — output the raw transcript instead of refusing to
        // start. The error is kept below for a later translation request.
        if purpose == DictationContextPurpose::Dictation
            && traditional
            && deferred_llm_error.is_some()
            && context.uses_llm_polisher()
            && crate::dictation_context::is_local_asr_provider(&context.asr.provider_type)
        {
            context = context.with_unconfigured_llm_raw();
        }
        if let Some(error) = deferred_llm_error {
            if purpose != DictationContextPurpose::Dictation || context.uses_llm_polisher() {
                return Err(error);
            }
            context.deferred_llm_error = Some(error);
        }
        context.correction_rules = self
            .correction_rules
            .list()?
            .into_iter()
            .filter(|rule| rule.enabled)
            .collect();
        Ok(context)
    }

    async fn resolve_session_provider(
        &self,
        slot: ProviderSlot,
        preference_fallback: &str,
    ) -> Result<crate::dictation_context::ProviderInvocation, BackendError> {
        crate::provider_resolution::resolve_session_provider(
            &self.deps.credential_store,
            slot,
            preference_fallback,
        )
        .await
    }

    fn mark_dictation_failed(&self, session_id: SessionId, error: &BackendError) {
        let mut state = self.state.write().expect("backend state lock poisoned");
        if state.dictation.session_id != Some(session_id) {
            return;
        }
        state.dictation.phase = DictationPhase::Failed;
        state.dictation.message = Some(format!("{:?}", error.code));
        let snapshot = state.dictation.clone();
        self.events.publish(
            Some(session_id),
            BackendEventKind::DictationStateChanged(snapshot),
        );
        self.phase_changed.notify_waiters();
    }
}

fn ensure_running(state: &MutableState) -> Result<(), BackendError> {
    if state.running {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::InvalidState,
            "backend is not started",
        ))
    }
}

fn ensure_active_session(state: &MutableState, session_id: SessionId) -> Result<(), BackendError> {
    ensure_running(state)?;
    if state.dictation.session_id == Some(session_id)
        && !matches!(
            state.dictation.phase,
            DictationPhase::Idle
                | DictationPhase::Completed
                | DictationPhase::Cancelled
                | DictationPhase::Failed
        )
    {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Cancelled,
            "dictation progress belongs to an inactive session",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures_util::future::BoxFuture;

    use super::*;
    use crate::config::{BackendConfig, TokioTaskSpawner};
    use crate::errors::BackendError;
    use crate::ports::{
        boxed, DictationEngine, EditObservationAdapter, EditObservationSink, EngineFailure,
        EngineProgressSink, EngineResult, HostAction, HostActions, HostContextAdapter,
        HostContextCapture, InsertOutcome, InsertWriteResult, TextInserter, TextInsertionSession,
    };

    fn assert_send_sync<T: Send + Sync>() {}

    #[tokio::test]
    async fn voice_activation_cancelled_handoff_releases_reserved_session() {
        struct Capture(Arc<AtomicU64>, bool);
        impl PendingAudioCapture for Capture {
            fn commit(
                self: Box<Self>,
                _session_id: SessionId,
                _context: Arc<DictationContext>,
                _consumer: Arc<dyn crate::ports::AudioConsumer>,
                _progress: Arc<dyn crate::ports::RecordingProgressSink>,
            ) -> BoxFuture<'static, Result<Box<dyn crate::ports::ActiveRecording>, BackendError>>
            {
                panic!("stale handoff must not commit");
            }
            fn discard(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
                Box::pin(async move {
                    self.0.fetch_add(1, Ordering::SeqCst);
                    if self.1 {
                        Err(BackendError::new(
                            BackendErrorCode::Platform,
                            "discard failed",
                        ))
                    } else {
                        Ok(())
                    }
                })
            }
        }
        for discard_fails in [false, true] {
            let (backend, _) = backend();
            backend.start().await.unwrap();
            let generation = backend.voice_activation_generation.load(Ordering::Acquire);
            let discarded = Arc::new(AtomicU64::new(0));
            let gate = backend.voice_activation_configure_gate.lock().await;
            let mut start = Box::pin(backend.start_voice_dictation_with_capture(
                Box::new(Capture(Arc::clone(&discarded), discard_fails)),
                generation,
            ));
            assert!(futures_util::poll!(&mut start).is_pending());
            assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Starting);
            backend
                .voice_activation_generation
                .fetch_add(1, Ordering::AcqRel);
            drop(gate);
            assert_eq!(start.await.unwrap_err().code, BackendErrorCode::Cancelled);
            assert!(backend.snapshot().dictation.session_id.is_none());
            assert_eq!(discarded.load(Ordering::SeqCst), 1);
            let next = backend
                .reserve_dictation_session(false, DictationOutputTarget::ForegroundApp, None)
                .unwrap();
            backend.reset_dictation_session(next.session_id);
        }
    }

    #[test]
    fn vocabulary_learning_settings_migrate_normalize_and_roundtrip() {
        let old: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(!old.vocabulary_learning_enabled);
        assert_eq!(old.vocabulary_learning_settings, Default::default());
        let prefs: UserPreferences = serde_json::from_value(serde_json::json!({
            "vocabularyLearningSettings": {
                "observationSeconds": 0, "suggestionSeconds": 999, "maxPhraseChars": 99
            }
        }))
        .unwrap();
        assert_eq!(prefs.vocabulary_learning_settings.observation_seconds, 10);
        assert_eq!(prefs.vocabulary_learning_settings.suggestion_seconds, 60);
        assert_eq!(prefs.vocabulary_learning_settings.max_phrase_chars, 32);
        let restored: UserPreferences =
            serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
        assert_eq!(
            restored.vocabulary_learning_settings,
            prefs.vocabulary_learning_settings
        );
    }

    #[test]
    fn vocabulary_learning_config_change_invalidates_pending_and_stale_sink() {
        let (backend, _) = backend();
        let mut prefs = backend.get_preferences();
        prefs.vocabulary_learning_enabled = true;
        prefs.vocabulary_learning_settings.observation_seconds = 20;
        prefs.vocabulary_learning_settings.suggestion_seconds = 30;
        prefs.vocabulary_learning_settings.max_phrase_chars = 16;
        backend
            .update_settings(
                prefs,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        let sink = CoreEditObservationSink {
            settings: backend.get_preferences().vocabulary_learning_settings,
            expected_generation: backend.edit_observation_generation.load(Ordering::Acquire),
            generation: Arc::clone(&backend.edit_observation_generation),
            typed_text: "甲".repeat(13),
            pending: Arc::clone(&backend.pending_corrections),
            events: Arc::clone(&backend.events),
        };
        assert_eq!(
            sink.observation_duration(),
            std::time::Duration::from_secs(20)
        );
        let edit = crate::host_document::EditPair {
            source: "甲".repeat(13),
            target: "乙".repeat(13),
            before: String::new(),
            after: String::new(),
        };
        let before = chrono::Utc::now().timestamp_millis();
        assert!(sink.publish(edit.clone()));
        let after = chrono::Utc::now().timestamp_millis();
        let suggestion = backend.pending_corrections().pop().unwrap();
        assert!((before + 30_000..=after + 30_000).contains(&suggestion.expires_at_ms));
        let mut prefs = backend.get_preferences();
        prefs.vocabulary_learning_settings.observation_seconds = 999;
        backend
            .update_settings(
                prefs,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        assert_eq!(
            backend
                .get_preferences()
                .vocabulary_learning_settings
                .observation_seconds,
            60
        );
        assert!(backend.pending_corrections().is_empty());
        assert!(!sink.publish(edit));
        assert!(backend.pending_corrections().is_empty());
        assert!(backend.get_preferences().vocabulary_learning_enabled);
    }

    #[test]
    fn observed_spelling_correction_saves_only_the_complete_confirmed_word() {
        let (backend, _) = backend();
        let sink = CoreEditObservationSink {
            settings: backend.get_preferences().vocabulary_learning_settings,
            expected_generation: backend.edit_observation_generation.load(Ordering::Acquire),
            generation: Arc::clone(&backend.edit_observation_generation),
            typed_text: "你好，ZIP和codx".into(),
            pending: Arc::clone(&backend.pending_corrections),
            events: Arc::clone(&backend.events),
        };
        assert!(sink.publish(
            crate::host_document::minimal_edit("你好，ZIP和codx", "你好，VIP和codex。").unwrap()
        ));
        let suggestions = backend.pending_corrections();
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].pattern, "ZIP");
        assert_eq!(suggestions[0].replacement, "VIP");
        assert_eq!(suggestions[1].pattern, "codx");
        assert_eq!(suggestions[1].replacement, "codex");
        assert!(backend.list_vocabulary().unwrap().is_empty());
        backend
            .accept_pending_correction(&suggestions[0].id)
            .unwrap();
        let vocabulary = backend.list_vocabulary().unwrap();
        assert_eq!(vocabulary.len(), 1);
        assert_eq!(vocabulary[0].phrase, "VIP");
        assert!(backend.list_correction_rules().unwrap().is_empty());

        // A matching changed letter is insufficient when the recovered word
        // came from outside this insertion.
        assert!(!sink.publish(crate::host_document::minimal_edit("Zebra", "Vebra").unwrap()));
        assert_eq!(backend.pending_corrections().len(), 1);
    }

    #[test]
    fn vocabulary_learning_expired_suggestions_cannot_be_accepted() {
        let (backend, _) = backend();
        let suggestion = backend
            .queue_pending_correction("错词".into(), "Codex".into())
            .unwrap()
            .unwrap();
        backend.pending_corrections.lock().unwrap()[0].expires_at_ms = 0;
        assert!(backend
            .accept_pending_correction(&suggestion.id)
            .unwrap()
            .is_none());
        assert!(backend.list_vocabulary().unwrap().is_empty());
        assert!(backend.pending_corrections().is_empty());
    }

    #[test]
    fn vocabulary_learning_explicit_collection_is_confirmed_and_idempotent() {
        let (backend, _) = backend();
        assert!(!backend.get_preferences().vocabulary_learning_enabled);
        let word = backend
            .add_learned_vocabulary(" Codex ".into())
            .unwrap()
            .unwrap();
        assert_eq!(word.phrase, "Codex");
        assert_eq!(word.note.as_deref(), Some(LEARNED_VOCAB_NOTE));
        assert!(backend
            .add_learned_vocabulary("Codex".into())
            .unwrap()
            .is_none());
        assert!(backend.add_learned_vocabulary("\n".into()).is_err());
        assert!(backend
            .add_learned_vocabulary("first\nsecond".into())
            .is_err());
        assert!(backend.add_learned_vocabulary("a".repeat(65)).is_err());
        assert_eq!(backend.list_vocabulary().unwrap().len(), 1);
    }

    #[test]
    fn vocabulary_learning_old_generation_cannot_queue_after_opt_out() {
        let (backend, _) = backend();
        let generation = AtomicU64::new(2);
        assert!(queue_pending_correction_state(
            &backend.pending_corrections,
            &backend.events,
            "错词".into(),
            "Codex".into(),
            Some((&generation, 1)),
            10
        )
        .unwrap()
        .is_none());
        assert!(backend.pending_corrections().is_empty());
    }

    struct TestDataDir {
        path: std::path::PathBuf,
    }

    impl TestDataDir {
        fn new(label: &str) -> Self {
            Self {
                path: std::env::temp_dir().join(format!(
                    "openless-core-{label}-{}",
                    uuid::Uuid::new_v4().simple()
                )),
            }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TestDataDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct TestBackend {
        backend: OpenLessBackend,
        _data_dir: TestDataDir,
    }

    impl std::ops::Deref for TestBackend {
        type Target = OpenLessBackend;

        fn deref(&self) -> &Self::Target {
            &self.backend
        }
    }

    #[test]
    fn backend_is_safe_to_share_between_host_and_ui_tasks() {
        assert_send_sync::<OpenLessBackend>();
        assert_send_sync::<LessComputerVoiceSession>();
    }

    #[derive(Default)]
    struct FakeHost(Mutex<Vec<HostAction>>);

    impl HostActions for FakeHost {
        fn request(&self, action: HostAction) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(action);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeHostContext(
        std::sync::atomic::AtomicUsize,
        std::sync::atomic::AtomicUsize,
    );

    impl HostContextAdapter for FakeHostContext {
        fn capture(
            &self,
            include_cursor: bool,
        ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
            self.0.fetch_add(1, Ordering::AcqRel);
            if include_cursor {
                self.1.fetch_add(1, Ordering::AcqRel);
            }
            boxed(async move {
                Ok(HostContextCapture {
                    front_app: Some("Terminal (com.apple.Terminal)".into()),
                    cursor_context: include_cursor.then(|| "before <OPENLESS_CURSOR> after".into()),
                })
            })
        }
    }

    #[derive(Default)]
    struct FakeEditObservation {
        typed_texts: Mutex<Vec<String>>,
        sinks: Mutex<Vec<Arc<dyn EditObservationSink>>>,
    }

    impl FakeEditObservation {
        fn publish(&self, index: usize, edit: crate::host_document::EditPair) {
            let _ = self.sinks.lock().unwrap()[index].publish(edit);
        }
    }

    impl EditObservationAdapter for FakeEditObservation {
        fn arm(
            &self,
            typed_text: String,
            sink: Arc<dyn EditObservationSink>,
        ) -> Result<(), BackendError> {
            self.typed_texts.lock().unwrap().push(typed_text);
            self.sinks.lock().unwrap().push(sink);
            Ok(())
        }

        fn disarm(&self) {}
    }

    struct FakeEngine;

    impl DictationEngine for FakeEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async {
                Ok(EngineResult {
                    raw_text: "raw".to_string(),
                    asr_transcript: None,
                    polished_text: "polished".to_string(),
                    polish_source: None,
                    duration_ms: 1000,
                    polish_failed: false,
                    asr_ms: None,
                    polish_ms: None,
                    has_audio_recording: None,
                    asr_call_label: None,
                    llm_call_label: None,
                })
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    struct FakeInserter;

    impl TextInserter for FakeInserter {
        fn begin(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
        ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
            boxed(async { Ok(Arc::new(FakeInsertionSession) as Arc<dyn TextInsertionSession>) })
        }
    }

    struct FakeInsertionSession;

    impl TextInsertionSession for FakeInsertionSession {
        fn write(
            &self,
            text: String,
        ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
            boxed(async move {
                Ok(InsertWriteResult {
                    written_chars: text.chars().count(),
                })
            })
        }

        fn copy(&self, _text: String) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn finish(
            &self,
            _final_text: String,
        ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
            boxed(async { Ok(InsertOutcome::Inserted) })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    struct FailingEngine(std::sync::atomic::AtomicUsize);

    impl DictationEngine for FailingEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async {
                Err(EngineFailure::from(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture provider failure",
                )))
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.fetch_add(1, Ordering::AcqRel);
            boxed(async { Ok(()) })
        }
    }

    struct PolishMetadataFailingEngine;

    impl DictationEngine for PolishMetadataFailingEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async {
                Err(EngineFailure {
                    error: BackendError::new(BackendErrorCode::Provider, "fixture omni failure"),
                    stage: EngineFailureStage::Polishing,
                    raw_text: Some("omni raw".to_string()),
                    duration_ms: Some(900),
                    asr_ms: Some(300),
                    polish_ms: Some(600),
                    has_audio_recording: Some(true),
                    asr_call_label: None,
                    llm_call_label: None,
                })
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    struct StartFailingEngine;

    impl DictationEngine for StartFailingEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture recorder start failure",
                ))
            })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async {
                Err(EngineFailure::from(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "fixture start never completed",
                )))
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    struct BlockingStartEngine {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl DictationEngine for BlockingStartEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            let entered = Arc::clone(&self.entered);
            let release = Arc::clone(&self.release);
            boxed(async move {
                entered.notify_one();
                release.notified().await;
                Ok(())
            })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async {
                Ok(EngineResult {
                    raw_text: "raw".to_string(),
                    asr_transcript: None,
                    polished_text: "polished".to_string(),
                    polish_source: None,
                    duration_ms: 1000,
                    polish_failed: false,
                    asr_ms: None,
                    polish_ms: None,
                    has_audio_recording: None,
                    asr_call_label: None,
                    llm_call_label: None,
                })
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    fn backend() -> (TestBackend, Arc<FakeHost>) {
        let host = Arc::new(FakeHost::default());
        let data_dir = TestDataDir::new("facade");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: host.clone(),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        (
            TestBackend {
                backend,
                _data_dir: data_dir,
            },
            host,
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn old_cli_completion_cannot_erase_an_accepted_physical_hold_press() {
        use crate::shared_types::{HotkeyMode, ShortcutBinding};

        // Hold the interpreter while the public key entry and a CLI-style stop
        // queue on separate threads. Mutex wake order is not a contract, so
        // repeat the overlap and check every new press that was accepted.
        for iteration in 0..32 {
            let (backend, _) = backend();
            let backend = Arc::new(backend);
            let mut preferences = backend.get_preferences();
            preferences.hotkey.mode = HotkeyMode::Hold;
            preferences.dictation_hotkey = ShortcutBinding {
                primary: "F11".into(),
                modifiers: vec!["ctrl".into()],
            };
            backend
                .update_settings(
                    preferences,
                    crate::SettingsUpdateOptions::STRICT,
                    &crate::NoopSettingsRuntime,
                )
                .unwrap();
            backend.start().await.unwrap();
            let (locked, lock_ready) = tokio::sync::oneshot::channel();
            let (unlock, unlock_rx) = std::sync::mpsc::channel();
            let locking = std::thread::spawn({
                let backend = backend.clone();
                move || {
                    let _interpreter = backend.hotkey.lock().unwrap();
                    let _ = locked.send(());
                    let _ = unlock_rx.recv();
                }
            });
            lock_ready.await.unwrap();
            let pressed_at = std::time::Instant::now();
            let press_id = 1;
            let pressing = tokio::spawn({
                let backend = backend.clone();
                async move {
                    backend
                        .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                            press_id,
                            at: pressed_at,
                        })
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while backend.hotkey_dispatch_gate.try_lock().is_ok() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            // The key entry owns the FIFO dispatch gate and is waiting for the
            // interpreter. Starting/stopping by another public entry is legal.
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            let first = backend.start_dictation().await.unwrap();
            let stopping = tokio::spawn({
                let backend = backend.clone();
                async move { backend.stop_dictation_session(first).await }
            });
            let _ = tokio::time::timeout(std::time::Duration::from_millis(30), async {
                while backend.snapshot().dictation.phase != DictationPhase::Idle {
                    tokio::task::yield_now().await;
                }
            })
            .await;
            unlock.send(()).unwrap();
            tokio::task::spawn_blocking(move || locking.join().unwrap())
                .await
                .unwrap();
            let press = pressing.await.unwrap();
            stopping.await.unwrap().unwrap();

            match press {
                Ok(CliDispatchOutcome::DictationStarted(second)) => {
                    let released = backend
                        .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {
                            press_id,
                            at: pressed_at + std::time::Duration::from_secs(1),
                        })
                        .await
                        .unwrap();
                    assert!(matches!(released, CliDispatchOutcome::DictationCompleted(ref result)
                        if result.session_id == second),
                        "old stop erased accepted Hold generation in iteration {iteration}: {released:?}");
                }
                Ok(CliDispatchOutcome::Noop) => {}
                Err(error) if error.code == BackendErrorCode::Busy => {}
                result => panic!("unexpected key result: {result:?}"),
            }
            assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
            backend.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn voice_workflows_share_one_busy_lease_and_release_it_on_terminal_paths() {
        let (backend, _) = backend();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        backend
            .update_settings(
                preferences,
                crate::settings::SettingsUpdateOptions::STRICT,
                &crate::settings::NoopSettingsRuntime,
            )
            .unwrap();
        backend.start().await.unwrap();

        let dictation = backend.start_dictation().await.unwrap();
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            backend
                .services()
                .selection_voice
                .begin(crate::domains::SelectionCapture {
                    text: "selection".into(),
                    source_app: None,
                })
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        backend.cancel_dictation(Some(dictation)).await.unwrap();

        let selection = backend
            .services()
            .selection_voice
            .begin(crate::domains::SelectionCapture {
                text: "selection".into(),
                source_app: None,
            })
            .await
            .unwrap();
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        backend
            .services()
            .selection_voice
            .cancel(Some(selection))
            .await
            .unwrap();

        let less_computer = SessionId::new();
        backend.begin_less_computer_capture(less_computer).unwrap();
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        backend.abort_less_computer_capture(less_computer).unwrap();
        backend.start_dictation().await.unwrap();
    }

    #[derive(Default)]
    struct LessComputerCaptureRuntime {
        request: Mutex<Option<crate::coding_agent::AgentCommand>>,
    }

    impl crate::coding_agent::CodingAgentProcessAdapter for LessComputerCaptureRuntime {
        fn execute(
            &self,
            request: crate::coding_agent::AgentCommand,
            output: Arc<dyn crate::coding_agent::ProcessOutputSink>,
            _cancel: crate::coding_agent::CancellationToken,
        ) -> BoxFuture<'static, Result<crate::coding_agent::ProcessExit, BackendError>> {
            *self.request.lock().unwrap() = Some(request);
            boxed(async move {
                output.write(crate::coding_agent::ProcessOutputLine {
                    stream: crate::coding_agent::ProcessStream::Stdout,
                    line: "完成".into(),
                });
                Ok(crate::coding_agent::ProcessExit {
                    code: Some(0),
                    success: true,
                })
            })
        }
    }

    #[tokio::test]
    async fn less_computer_facade_resolves_provider_model_permission_and_workdir() {
        let data_dir = TestDataDir::new("less-computer-facade");
        let runtime = Arc::new(LessComputerCaptureRuntime::default());
        let dependencies = BackendDependencies::unsupported();
        dependencies.services.less_computer.bind_runner(Arc::new(
            crate::coding_agent::CodingAgentRunner::new(runtime.clone()),
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                home_dir: Some(std::env::temp_dir()),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        preferences.coding_agent_provider = "dsh-cli".into();
        preferences.coding_agent_permission_mode = "bypassPermissions".into();
        let workdir = std::env::temp_dir().join("openless-less-computer-workdir");
        preferences.coding_agent_workdir = Some(format!("  {}  ", workdir.display()));
        backend.set_preferences(preferences).unwrap();

        let session_id = SessionId::new();
        let result = backend
            .submit_less_computer_with_session(session_id, "  做一次检查  ".into())
            .await
            .unwrap();
        assert!(matches!(
            result.outcome,
            crate::domains::LessComputerRunOutcome::Completed { .. }
        ));

        let request = runtime.request.lock().unwrap().clone().unwrap();
        assert_eq!(request.executable, "dsh");
        assert_eq!(request.cwd, Some(workdir));
        assert_eq!(
            request.env.get("DSH_PERMISSION_MODE").map(String::as_str),
            Some("read-only")
        );
        assert!(request.temporary_files.iter().any(|file| {
            file.name == "openless.patch.yml"
                && String::from_utf8_lossy(&file.contents).contains("做一次检查")
        }));
    }

    #[tokio::test]
    async fn disabled_less_computer_is_rejected_before_runtime_access() {
        let data_dir = TestDataDir::new("less-computer-disabled");
        let runtime = Arc::new(LessComputerCaptureRuntime::default());
        let dependencies = BackendDependencies::unsupported();
        dependencies.services.less_computer.bind_runner(Arc::new(
            crate::coding_agent::CodingAgentRunner::new(runtime.clone()),
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();

        let error = backend
            .submit_less_computer("不应启动".into())
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::PermissionDenied);
        let mut legacy = backend.get_preferences();
        legacy.coding_agent_enabled = true;
        backend.set_preferences(legacy).unwrap();
        assert_eq!(
            backend
                .submit_less_computer("legacy".into())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::PermissionDenied
        );
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::PermissionDenied
        );
        assert!(runtime.request.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn less_computer_capture_facade_is_session_scoped_and_cancel_releases_it() {
        let (backend, _) = backend();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        backend.set_preferences(preferences).unwrap();

        let session_id = SessionId::new();
        let other_session = SessionId::new();
        backend.begin_less_computer_capture(session_id).unwrap();
        assert_eq!(backend.less_computer_active_session(), Some(session_id));

        let busy = backend
            .begin_less_computer_capture(other_session)
            .unwrap_err();
        assert_eq!(busy.code, BackendErrorCode::Busy);

        backend
            .cancel_less_computer(Some(session_id))
            .await
            .unwrap();
        assert_eq!(backend.less_computer_active_session(), None);
    }

    #[tokio::test]
    async fn disabling_testing_features_cancels_capture_and_preserves_configuration() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        preferences.coding_agent_model = Some("saved-model".into());
        backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        let owner = SessionId::new();
        backend.begin_less_computer_capture(owner).unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = false;
        let outcome = backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        let hotkeys = outcome.effects.hotkeys.unwrap();
        assert!(hotkeys.previous.coding_agent_enabled);
        assert!(!hotkeys.next.coding_agent_enabled);
        assert!(outcome.preferences.coding_agent_enabled);
        assert_eq!(
            outcome.preferences.coding_agent_model.as_deref(),
            Some("saved-model")
        );
        assert!(backend.less_computer_capture_cancelled(owner));
        assert!(backend.ensure_testing_features_enabled().is_err());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while backend.less_computer_active_session().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::PermissionDenied
        );
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        backend.begin_less_computer_capture(owner).unwrap();
        backend.cancel_less_computer(Some(owner)).await.unwrap();
    }

    #[test]
    fn less_computer_hotkey_modes_share_hold_toggle_auto_and_combined_rules() {
        let (backend, _) = backend();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;

        preferences.hotkey.mode = crate::HotkeyMode::Hold;
        backend.set_preferences(preferences.clone()).unwrap();
        let pressed = std::time::Instant::now();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 1,
                at: pressed,
            }),
            LessComputerHotkeyAction::Start
        );
        let hold_session = SessionId::new();
        backend.begin_less_computer_capture(hold_session).unwrap();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Released {
                press_id: 1,
                at: pressed + std::time::Duration::from_millis(20),
            }),
            LessComputerHotkeyAction::Finish
        );
        backend.abort_less_computer_capture(hold_session).unwrap();

        preferences.hotkey.mode = crate::HotkeyMode::Auto;
        backend.set_preferences(preferences.clone()).unwrap();
        let pressed = std::time::Instant::now();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 2,
                at: pressed,
            }),
            LessComputerHotkeyAction::Start
        );
        let auto_session = SessionId::new();
        backend.begin_less_computer_capture(auto_session).unwrap();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Released {
                press_id: 2,
                at: pressed + std::time::Duration::from_millis(349),
            }),
            LessComputerHotkeyAction::Noop
        );
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 3,
                at: pressed + std::time::Duration::from_millis(500),
            }),
            LessComputerHotkeyAction::Finish
        );
        backend.abort_less_computer_capture(auto_session).unwrap();

        preferences.hotkey.mode = crate::HotkeyMode::Toggle;
        backend.set_preferences(preferences).unwrap();
        let pressed = std::time::Instant::now();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 4,
                at: pressed,
            }),
            LessComputerHotkeyAction::Start
        );
        let combined_session = SessionId::new();
        backend
            .begin_less_computer_capture(combined_session)
            .unwrap();
        assert_eq!(
            backend.dispatch_less_computer_hotkey_edge(DictationHotkeyEdge::Combined {
                press_id: 4,
                at: std::time::Instant::now(),
            }),
            LessComputerHotkeyAction::Cancel
        );
        backend
            .abort_less_computer_capture(combined_session)
            .unwrap();
    }

    #[derive(Default)]
    struct VoiceTranscription {
        pcm: Mutex<Vec<u8>>,
        cancelled: std::sync::atomic::AtomicBool,
        starts: AtomicU64,
    }

    impl crate::ports::AudioConsumer for VoiceTranscription {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().extend_from_slice(pcm);
        }
    }

    impl crate::ports::TranscriptionSession for VoiceTranscription {
        fn finish(&self) -> BoxFuture<'static, Result<crate::TranscriptOutput, BackendError>> {
            boxed(async {
                Ok(crate::TranscriptOutput {
                    text: "执行语音任务".into(),
                    duration_ms: 100,
                })
            })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            boxed(async { Ok(()) })
        }
    }

    struct VoiceOnlyEngine(Arc<VoiceTranscription>);

    impl DictationEngine for VoiceOnlyEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn start_transcription(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            self.0.starts.fetch_add(1, Ordering::AcqRel);
            let session: Arc<dyn TranscriptionSession> = self.0.clone();
            boxed(async move { Ok(session) })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async { unreachable!("voice-only engine does not run dictation") })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn less_computer_cancelled_start_cannot_open_or_install_a_late_capture() {
        use std::sync::atomic::AtomicBool;
        struct PendingContext {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            block: bool,
        }
        impl HostContextAdapter for PendingContext {
            fn capture(
                &self,
                _: bool,
            ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
                let entered = self.entered.clone();
                let release = self.release.clone();
                let block = self.block;
                boxed(async move {
                    if block {
                        entered.notify_one();
                        release.notified().await;
                    }
                    Ok(HostContextCapture::default())
                })
            }
        }
        struct Recording(Arc<AtomicBool>);
        impl crate::ports::ActiveRecording for Recording {
            fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
                self.0.store(true, Ordering::Release);
                boxed(async { Ok(()) })
            }
        }
        struct PendingEngine {
            gate: PendingContext,
            started: Arc<AtomicBool>,
            stopped: Arc<AtomicBool>,
            transcription: Arc<VoiceTranscription>,
        }
        impl DictationEngine for PendingEngine {
            fn start(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
                _: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { unreachable!() })
            }
            fn finish(
                &self,
                _: SessionId,
                _: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
                boxed(async { unreachable!() })
            }
            fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
            fn start_voice_capture(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
                _: Arc<dyn TextStreamSink>,
                _: Arc<dyn crate::ports::RecordingProgressSink>,
                _: crate::CancellationToken,
            ) -> BoxFuture<'static, Result<crate::ports::VoiceCapture, BackendError>> {
                self.started.store(true, Ordering::Release);
                let gate = self.gate.capture(false);
                let stopped = self.stopped.clone();
                let transcription = self.transcription.clone();
                boxed(async move {
                    gate.await?;
                    Ok(crate::ports::VoiceCapture {
                        recording: Box::new(Recording(stopped)),
                        transcription,
                    })
                })
            }
        }
        for block_context in [true, false] {
            let data_dir = TestDataDir::new("less-computer-cancel-start");
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let started = Arc::new(AtomicBool::new(false));
            let stopped = Arc::new(AtomicBool::new(false));
            let transcription = Arc::new(VoiceTranscription::default());
            let mut dependencies = BackendDependencies::unsupported();
            dependencies.host_actions = Arc::new(FakeHost::default());
            dependencies.services.host_context = Arc::new(PendingContext {
                entered: entered.clone(),
                release: release.clone(),
                block: block_context,
            });
            dependencies.dictation_engine = Arc::new(PendingEngine {
                gate: PendingContext {
                    entered: entered.clone(),
                    release: release.clone(),
                    block: !block_context,
                },
                started: started.clone(),
                stopped: stopped.clone(),
                transcription: transcription.clone(),
            });
            let backend = Arc::new(
                OpenLessBackend::new(
                    BackendConfig {
                        data_dir: data_dir.path().to_path_buf(),
                        ..BackendConfig::default()
                    },
                    dependencies,
                )
                .unwrap(),
            );
            let mut preferences = backend.get_preferences();
            preferences.testing_features_enabled = true;
            preferences.coding_agent_enabled = true;
            backend.set_preferences(preferences).unwrap();
            let session_id = SessionId::new();
            let starting = tokio::spawn({
                let backend = backend.clone();
                async move {
                    backend
                        .start_less_computer_voice(
                            session_id,
                            Arc::new(FakeRecordingControl::default()),
                        )
                        .await
                }
            });
            entered.notified().await;
            backend
                .cancel_less_computer(Some(session_id))
                .await
                .unwrap();
            let successor = SessionId::new();
            assert_eq!(
                backend
                    .begin_less_computer_capture(successor)
                    .unwrap_err()
                    .code,
                BackendErrorCode::Busy
            );
            release.notify_one();
            match starting.await.unwrap() {
                Err(error) => assert_eq!(error.code, BackendErrorCode::Cancelled),
                Ok(_) => panic!("cancelled startup must not return a live capture"),
            }
            backend.begin_less_computer_capture(successor).unwrap();
            assert_eq!(started.load(Ordering::Acquire), !block_context);
            assert_eq!(stopped.load(Ordering::Acquire), !block_context);
            assert_eq!(
                transcription.cancelled.load(Ordering::Acquire),
                !block_context
            );
            assert_eq!(backend.less_computer_active_session(), Some(successor));
            assert!(!backend.less_computer_capture_cancelled(successor));
            backend.abort_less_computer_capture(successor).unwrap();
        }
    }

    #[tokio::test]
    async fn less_computer_voice_validates_pcm_and_submits_one_final_transcript() {
        let data_dir = TestDataDir::new("less-computer-voice");
        let transcription = Arc::new(VoiceTranscription::default());
        let runtime = Arc::new(LessComputerCaptureRuntime::default());
        let host = Arc::new(crate::testing::RecordingHostActions::default());
        let dependencies = BackendDependencies {
            voice_activation_runtime: None,
            host_actions: host.clone(),
            dictation_engine: Arc::new(VoiceOnlyEngine(Arc::clone(&transcription))),
            ..BackendDependencies::unsupported()
        };
        dependencies.services.less_computer.bind_runner(Arc::new(
            crate::coding_agent::CodingAgentRunner::new(runtime.clone()),
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        preferences.stable_transcription_enabled = true;
        backend.set_preferences(preferences).unwrap();
        let mut events = backend.subscribe();

        let session_id = SessionId::new();
        let session = backend
            .start_less_computer_voice(session_id, Arc::new(FakeRecordingControl::default()))
            .await
            .unwrap();
        assert_eq!(transcription.starts.load(Ordering::Acquire), 0);
        assert_eq!(host.actions(), vec![HostAction::ShowLessComputer]);
        assert_eq!(
            session.feed_pcm(&[]).unwrap_err().code,
            BackendErrorCode::InvalidArgument
        );
        assert_eq!(
            session.feed_pcm(&[1]).unwrap_err().code,
            BackendErrorCode::InvalidArgument
        );
        session.feed_pcm(&[1, 0, 2, 0]).unwrap();
        let LessComputerVoiceFinish::Submitted(result) = session.finish().await.unwrap() else {
            panic!("hotkey-mode capture must submit an Agent turn");
        };

        assert_eq!(result.session_id, session_id);
        assert_eq!(*transcription.pcm.lock().unwrap(), vec![1, 0, 2, 0]);
        assert_eq!(transcription.starts.load(Ordering::Acquire), 1);
        assert!(runtime.request.lock().unwrap().is_some());
        let transcript_events = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| matches!(event.kind, BackendEventKind::TranscriptDelta(_)))
            .collect::<Vec<_>>();
        assert_eq!(transcript_events.len(), 1);
        assert!(matches!(
            transcript_events[0].kind,
            BackendEventKind::TranscriptDelta(crate::TranscriptDelta { is_final: true, .. })
        ));

        let cancelled = backend
            .start_less_computer_voice(SessionId::new(), Arc::new(FakeRecordingControl::default()))
            .await
            .unwrap();
        cancelled.cancel().await.unwrap();
        assert_eq!(transcription.starts.load(Ordering::Acquire), 1);
    }

    struct DictationTranscription {
        text: String,
        partials: Mutex<Option<Arc<dyn TextStreamSink>>>,
        fail_start: std::sync::atomic::AtomicBool,
    }

    impl crate::ports::AudioConsumer for DictationTranscription {
        fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
    }

    impl crate::ports::TranscriptionSession for DictationTranscription {
        fn finish(&self) -> BoxFuture<'static, Result<crate::TranscriptOutput, BackendError>> {
            let text = self.text.clone();
            boxed(async move {
                Ok(crate::TranscriptOutput {
                    text,
                    duration_ms: 100,
                })
            })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    struct DictationOnlyEngine(Arc<DictationTranscription>);

    impl DictationEngine for DictationOnlyEngine {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }

        fn start_transcription(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            *self.0.partials.lock().unwrap() = Some(partials);
            let session: Arc<dyn TranscriptionSession> = self.0.clone();
            boxed(async move { Ok(session) })
        }

        fn start_voice_capture(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
            _progress: Arc<dyn crate::ports::RecordingProgressSink>,
            _cancel: crate::CancellationToken,
        ) -> BoxFuture<'static, Result<crate::ports::VoiceCapture, BackendError>> {
            let error = if self.0.fail_start.load(Ordering::Acquire) {
                BackendError::new(
                    BackendErrorCode::PermissionDenied,
                    "microphone permission denied",
                )
            } else {
                BackendError::new(BackendErrorCode::Unsupported, "use the transcription path")
            };
            boxed(async move { Err(error) })
        }

        fn finish(
            &self,
            _session_id: SessionId,
            _progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            boxed(async { unreachable!("dictation-only engine does not run dictation") })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    fn dictation_backend(
        name: &str,
        text: &str,
    ) -> (
        TestDataDir,
        OpenLessBackend,
        Arc<DictationTranscription>,
        Arc<LessComputerCaptureRuntime>,
    ) {
        let data_dir = TestDataDir::new(name);
        let transcription = Arc::new(DictationTranscription {
            text: text.into(),
            partials: Mutex::new(None),
            fail_start: std::sync::atomic::AtomicBool::new(false),
        });
        let runtime = Arc::new(LessComputerCaptureRuntime::default());
        let dependencies = BackendDependencies {
            voice_activation_runtime: None,
            host_actions: Arc::new(crate::testing::RecordingHostActions::default()),
            dictation_engine: Arc::new(DictationOnlyEngine(Arc::clone(&transcription))),
            ..BackendDependencies::unsupported()
        };
        dependencies.services.less_computer.bind_runner(Arc::new(
            crate::coding_agent::CodingAgentRunner::new(runtime.clone()),
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        backend.set_preferences(preferences).unwrap();
        (data_dir, backend, transcription, runtime)
    }

    const DICTATE: LessComputerVoiceOptions = LessComputerVoiceOptions {
        mode: crate::events::LessComputerVoiceMode::Dictate,
        publish_start_error: false,
    };

    fn less_computer_event_kinds(
        events: &mut crate::events::EventSubscription,
    ) -> Vec<crate::events::LessComputerEventKind> {
        std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event.kind {
                BackendEventKind::LessComputerEvent(event) => Some(event.kind),
                _ => None,
            })
            .collect()
    }

    fn last_voice_state(
        kinds: &[crate::events::LessComputerEventKind],
    ) -> &crate::events::LessComputerEventKind {
        kinds
            .iter()
            .rev()
            .find(|kind| {
                matches!(
                    kind,
                    crate::events::LessComputerEventKind::VoiceState { .. }
                )
            })
            .expect("voice state was published")
    }

    #[tokio::test]
    async fn less_computer_dictation_streams_partials_and_commits_without_submitting() {
        use crate::events::{
            LessComputerEventKind, LessComputerVoiceMode, LessComputerVoiceOutcome,
            LessComputerVoicePhase,
        };
        let (_data_dir, backend, transcription, runtime) =
            dictation_backend("less-computer-dictation", "  打开设置  ");
        let mut events = backend.subscribe();
        let session_id = SessionId::new();
        let session = backend
            .start_less_computer_voice_with(
                session_id,
                Arc::new(FakeRecordingControl::default()),
                DICTATE,
            )
            .await
            .unwrap();
        assert_eq!(session.mode(), LessComputerVoiceMode::Dictate);
        let partials = transcription.partials.lock().unwrap().clone().unwrap();
        partials
            .publish(crate::ports::TextStreamChunk {
                text: "打开".into(),
                offset: 0,
            })
            .unwrap();
        session.feed_pcm(&[1, 0]).unwrap();

        assert_eq!(
            session.finish().await.unwrap(),
            LessComputerVoiceFinish::Dictated {
                transcript: "打开设置".into()
            }
        );
        assert!(runtime.request.lock().unwrap().is_none());
        assert_eq!(backend.less_computer_active_session(), None);

        let kinds = less_computer_event_kinds(&mut events);
        assert!(kinds.iter().any(|kind| matches!(
            kind,
            LessComputerEventKind::VoiceState {
                phase: LessComputerVoicePhase::Starting,
                mode: LessComputerVoiceMode::Dictate,
                transcript,
                outcome: None,
                ..
            } if transcript == "打开"
        )));
        assert!(!kinds.iter().any(|kind| matches!(
            kind,
            LessComputerEventKind::User { .. }
                | LessComputerEventKind::Started
                | LessComputerEventKind::Error { .. }
        )));
        let committed = LessComputerEventKind::VoiceState {
            session_id,
            phase: LessComputerVoicePhase::Idle,
            level: 0.0,
            elapsed_ms: 0,
            mode: LessComputerVoiceMode::Dictate,
            transcript: "打开设置".into(),
            outcome: Some(LessComputerVoiceOutcome::Committed),
        };
        assert_eq!(last_voice_state(&kinds), &committed);
        assert_eq!(
            backend
                .event_publisher()
                .latest_less_computer_voice_state()
                .unwrap()
                .kind,
            committed
        );

        // The composer can send the edited text immediately after dictation.
        backend
            .submit_less_computer("打开设置并截图".into())
            .await
            .unwrap();
        assert!(runtime.request.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn less_computer_dictation_reports_silence_and_cancel_without_chat_errors() {
        use crate::events::{LessComputerEventKind, LessComputerVoiceOutcome};
        let (_data_dir, backend, transcription, runtime) =
            dictation_backend("less-computer-dictation-empty", "   ");
        let mut events = backend.subscribe();
        let silent = backend
            .start_less_computer_voice_with(
                SessionId::new(),
                Arc::new(FakeRecordingControl::default()),
                DICTATE,
            )
            .await
            .unwrap();
        // Valid quiet PCM reaches ASR; an empty transcript is distinct from a missing microphone input.
        silent.feed_pcm(&[1, 0]).unwrap();
        assert_eq!(
            silent.finish().await.unwrap(),
            LessComputerVoiceFinish::Dictated {
                transcript: String::new()
            }
        );
        let kinds = less_computer_event_kinds(&mut events);
        assert!(!kinds
            .iter()
            .any(|kind| matches!(kind, LessComputerEventKind::Error { .. })));
        assert!(matches!(
            last_voice_state(&kinds),
            LessComputerEventKind::VoiceState {
                outcome: Some(LessComputerVoiceOutcome::Empty),
                ..
            }
        ));
        assert_eq!(backend.less_computer_active_session(), None);

        let abandoned = backend
            .start_less_computer_voice_with(
                SessionId::new(),
                Arc::new(FakeRecordingControl::default()),
                DICTATE,
            )
            .await
            .unwrap();
        transcription
            .partials
            .lock()
            .unwrap()
            .clone()
            .unwrap()
            .publish(crate::ports::TextStreamChunk {
                text: "draft".into(),
                offset: 0,
            })
            .unwrap();
        abandoned.cancel().await.unwrap();
        let kinds = less_computer_event_kinds(&mut events);
        assert!(matches!(
            last_voice_state(&kinds),
            LessComputerEventKind::VoiceState {
                outcome: Some(LessComputerVoiceOutcome::Cancelled),
                transcript,
                ..
            } if transcript.is_empty()
        ));
        assert!(runtime.request.lock().unwrap().is_none());
        assert_eq!(backend.less_computer_active_session(), None);
    }

    #[tokio::test]
    async fn less_computer_missing_audio_fails_without_chat_errors_and_releases_capture() {
        use crate::events::{LessComputerEventKind, LessComputerVoiceOutcome};
        let (_data_dir, backend, _transcription, _runtime) =
            dictation_backend("less-computer-dictation-missing-audio", "must not commit");
        let mut events = backend.subscribe();
        for pcm in [Vec::new(), vec![0; 640]] {
            let capture = backend
                .start_less_computer_voice_with(
                    SessionId::new(),
                    Arc::new(FakeRecordingControl::default()),
                    DICTATE,
                )
                .await
                .unwrap();
            if !pcm.is_empty() {
                capture.feed_pcm(&pcm).unwrap();
            }
            let error = capture.finish().await.unwrap_err();
            assert_eq!(error.code, BackendErrorCode::InvalidArgument);
            assert!(!error.retryable);
            let kinds = less_computer_event_kinds(&mut events);
            assert!(!kinds
                .iter()
                .any(|kind| matches!(kind, LessComputerEventKind::Error { .. })));
            assert!(
                matches!(last_voice_state(&kinds), LessComputerEventKind::VoiceState {
                outcome: Some(LessComputerVoiceOutcome::Failed), transcript, ..
            } if transcript.is_empty())
            );
            assert_eq!(backend.less_computer_active_session(), None);
        }
    }

    #[tokio::test]
    async fn less_computer_panel_start_errors_are_returned_not_published() {
        let (_data_dir, backend, transcription, _runtime) =
            dictation_backend("less-computer-dictation-start-error", "unused");
        transcription.fail_start.store(true, Ordering::Release);
        let has_chat_error = |kinds: Vec<crate::events::LessComputerEventKind>| {
            kinds
                .iter()
                .any(|kind| matches!(kind, crate::events::LessComputerEventKind::Error { .. }))
        };

        let mut events = backend.subscribe();
        let error = match backend
            .start_less_computer_voice_with(
                SessionId::new(),
                Arc::new(FakeRecordingControl::default()),
                DICTATE,
            )
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("the engine refused to start"),
        };
        assert_eq!(error.code, BackendErrorCode::PermissionDenied);
        assert!(!has_chat_error(less_computer_event_kinds(&mut events)));
        assert_eq!(backend.less_computer_active_session(), None);

        assert!(backend
            .start_less_computer_voice(SessionId::new(), Arc::new(FakeRecordingControl::default()))
            .await
            .is_err());
        assert!(
            has_chat_error(less_computer_event_kinds(&mut events)),
            "hotkey captures keep reporting start failures in the conversation"
        );
    }

    #[tokio::test]
    async fn qa_voice_capture_owns_recorder_and_transcription_lifecycle() {
        let data_dir = TestDataDir::new("qa-voice-capture");
        let recorder = Arc::new(
            crate::testing::FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])
                .with_archived_recording(true),
        );
        let transcription = Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
            "voice question",
            120,
        ));
        let engine = crate::PipelineDictationEngine::new(
            recorder.clone(),
            transcription.clone(),
            Arc::new(crate::testing::FixtureTextPolisher::successful("unused")),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                voice_activation_runtime: None,
                dictation_engine: Arc::new(engine),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();

        let first_id = SessionId::new();
        backend
            .voice_sessions
            .acquire(first_id, crate::voice_session::VoiceSessionKind::Qa)
            .unwrap();
        let capture = backend
            .start_qa_voice_capture(
                first_id,
                DictationStartOptions::default(),
                Arc::new(VoiceRecordingProgress),
            )
            .await
            .unwrap();
        let result = capture.finish().await.unwrap();
        backend.voice_sessions.release(first_id);

        assert_eq!(result.transcript.as_deref(), Some("voice question"));
        assert!(result.audio_wav.is_none());
        assert_eq!(result.duration_ms, 120);
        assert_eq!(recorder.stop_count(), 1);
        assert_eq!(transcription.pcm(), vec![1, 0, 2, 0]);

        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        backend.set_preferences(preferences).unwrap();
        let second_id = SessionId::new();
        backend
            .voice_sessions
            .acquire(second_id, crate::voice_session::VoiceSessionKind::Qa)
            .unwrap();
        let capture = backend
            .start_qa_voice_capture(
                second_id,
                DictationStartOptions::default(),
                Arc::new(VoiceRecordingProgress),
            )
            .await
            .unwrap();
        let result = capture.finish().await.unwrap();
        assert!(result.transcript.is_none());
        assert!(result.audio_wav.is_some_and(|wav| wav.starts_with(b"RIFF")));
        assert_eq!(recorder.stop_count(), 2);
    }

    #[tokio::test]
    async fn selection_voice_capture_respects_pipeline_mode() {
        let data_dir = TestDataDir::new("selection-voice-pipeline-mode");
        let recorder = Arc::new(crate::testing::FixtureAudioRecorder::new(
            vec![vec![1, 0, 2, 0]],
            vec![],
        ));
        let transcription = Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
            "edit this",
            120,
        ));
        let engine = crate::PipelineDictationEngine::new(
            recorder.clone(),
            transcription.clone(),
            Arc::new(crate::testing::FixtureTextPolisher::successful("unused")),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                voice_activation_runtime: None,
                dictation_engine: Arc::new(engine),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.selection_voice_enabled = true;
        backend.set_preferences(preferences).unwrap();

        let traditional_id = backend
            .services()
            .selection_voice
            .begin(crate::domains::SelectionCapture {
                text: "draft".into(),
                source_app: None,
            })
            .await
            .unwrap();
        let traditional = backend
            .start_selection_voice_capture(
                traditional_id,
                Arc::new(FakeRecordingControl::default()),
            )
            .await
            .unwrap();
        assert_eq!(traditional.finish().await.unwrap(), "edit this");
        assert_eq!(transcription.pcm(), vec![1, 0, 2, 0]);
        backend
            .services()
            .selection_voice
            .cancel(Some(traditional_id))
            .await
            .unwrap();

        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        backend.set_preferences(preferences).unwrap();
        let asr_pcm_before = transcription.pcm().len();
        let multimodal_id = backend
            .services()
            .selection_voice
            .begin(crate::domains::SelectionCapture {
                text: "draft".into(),
                source_app: None,
            })
            .await
            .unwrap();
        let multimodal = backend
            .start_selection_voice_capture(multimodal_id, Arc::new(FakeRecordingControl::default()))
            .await
            .unwrap();
        // Multimodal finish needs Omni credentials; capture must not feed ASR.
        assert!(multimodal.finish().await.is_err());
        assert_eq!(transcription.pcm().len(), asr_pcm_before);
        assert_eq!(recorder.stop_count(), 2);
        backend
            .services()
            .selection_voice
            .cancel(Some(multimodal_id))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn qa_and_selection_voice_capture_can_cancel_during_transcription_finish() {
        struct PendingTranscription {
            entered: Arc<tokio::sync::Semaphore>,
            gate: Arc<tokio::sync::Semaphore>,
            cancellations: std::sync::atomic::AtomicUsize,
        }
        impl crate::ports::AudioConsumer for PendingTranscription {
            fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
        }
        impl TranscriptionSession for PendingTranscription {
            fn finish(&self) -> BoxFuture<'static, Result<crate::TranscriptOutput, BackendError>> {
                let entered = self.entered.clone();
                let gate = self.gate.clone();
                Box::pin(async move {
                    entered.add_permits(1);
                    gate.acquire().await.unwrap().forget();
                    // Model a provider which still returns a buffered result
                    // after abort. Core must suppress this late success.
                    Ok(crate::TranscriptOutput {
                        text: "late question".into(),
                        duration_ms: 10,
                    })
                })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                self.cancellations.fetch_add(1, Ordering::AcqRel);
                self.gate.add_permits(1);
                Box::pin(async { Ok(()) })
            }
        }
        let transcription = Arc::new(PendingTranscription {
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
            cancellations: std::sync::atomic::AtomicUsize::new(0),
        });
        let capture = QaVoiceCaptureSession {
            context: Arc::new(DictationContext::default()),
            recording: Mutex::new(None),
            transcription: Some(transcription.clone()),
            pcm: None,
            recording_progress: Arc::new(QaRecordingProgress {
                session_id: SessionId::new(),
                qa: Arc::new(FakeQaControl::default()),
                progress: Arc::new(VoiceRecordingProgress),
                task_spawner: Arc::new(TokioTaskSpawner),
                started_at: std::time::Instant::now(),
                silence: Mutex::new(None),
                terminal: Mutex::new(QaRecordingTerminalState::default()),
            }),
            lifecycle: Arc::new(VoiceCaptureLifecycle::default()),
            task_spawner: Arc::new(TokioTaskSpawner),
        };
        let finishing = tokio::spawn(capture.finish());
        transcription.entered.acquire().await.unwrap().forget();
        assert_eq!(
            capture.finish().await.unwrap_err().code,
            BackendErrorCode::InvalidState
        );
        capture.cancel().await.unwrap();
        capture.cancel().await.unwrap();
        assert_eq!(transcription.cancellations.load(Ordering::Acquire), 1);
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), finishing)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);

        let data_dir = TestDataDir::new("selection-voice-finish-cancel");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap();
        let mut events = backend.subscribe();
        let transcription = Arc::new(PendingTranscription {
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
            cancellations: std::sync::atomic::AtomicUsize::new(0),
        });
        let capture = VoiceTranscriptionSession {
            session_id: SessionId::new(),
            transcription: Some(transcription.clone()),
            pcm: None,
            omni: None,
            recording: Mutex::new(None),
            partials: Arc::new(VoiceTranscriptSink {
                publisher: backend.event_publisher(),
                session_id: SessionId::new(),
                transcript: Mutex::new(crate::types::TranscriptAccumulator::default()),
                feedback: None,
            }),
            lifecycle: Arc::new(VoiceCaptureLifecycle::default()),
            task_spawner: Arc::new(TokioTaskSpawner),
        };
        let finishing = tokio::spawn(capture.finish());
        transcription.entered.acquire().await.unwrap().forget();
        capture.cancel().await.unwrap();
        capture.cancel().await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), finishing)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);
        assert_eq!(transcription.cancellations.load(Ordering::Acquire), 1);
        assert!(!std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event.kind, BackendEventKind::TranscriptDelta(_))));
    }

    #[derive(Default)]
    struct FakeRecordingControl {
        requests: Mutex<Vec<(SessionId, crate::events::RecordingControlAction)>>,
    }

    impl crate::ports::RecordingControlSink for FakeRecordingControl {
        fn request(
            &self,
            session_id: SessionId,
            action: crate::events::RecordingControlAction,
        ) -> Result<(), BackendError> {
            self.requests.lock().unwrap().push((session_id, action));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeQaControl {
        starts: std::sync::atomic::AtomicUsize,
        stops: std::sync::atomic::AtomicUsize,
        cancels: std::sync::atomic::AtomicUsize,
        faults: std::sync::atomic::AtomicUsize,
    }

    impl crate::domains::QaApi for FakeQaControl {
        fn start_question(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }
        fn show(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }

        fn snapshot(&self) -> BoxFuture<'static, Result<crate::domains::QaSnapshot, BackendError>> {
            Box::pin(async { Ok(crate::domains::QaSnapshot::default()) })
        }

        fn toggle_recording(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { panic!("deferred recording callbacks must not toggle QA") })
        }

        fn stop_recording(
            &self,
            _session_id: SessionId,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.stops.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }

        fn recording_fault(
            &self,
            _session_id: SessionId,
            _error: BackendError,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.faults.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }

        fn submit_text(&self, _text: String) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }

        fn set_edit_instruction_mode(
            &self,
            _enabled: bool,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }

        fn cancel(
            &self,
            _session_id: Option<SessionId>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancels.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        }

        fn dismiss(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn less_computer_recording_progress_routes_silence_and_faults_inside_core() {
        let data_dir = TestDataDir::new("less-computer-recording-policy");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        backend.set_preferences(preferences).unwrap();
        let control = Arc::new(FakeRecordingControl::default());

        let stop_session = SessionId::new();
        backend
            .services()
            .less_computer
            .begin_capture(stop_session)
            .unwrap();
        let started_at = std::time::Instant::now();
        let progress = LessComputerRecordingProgress {
            session_id: stop_session,
            feedback: Arc::new(LessVoiceFeedback::new(
                backend.event_publisher(),
                stop_session,
                crate::events::LessComputerVoiceMode::Submit,
                crate::events::LessComputerVoicePhase::Recording,
            )),
            less_computer: Arc::clone(&backend.services().less_computer),
            control: Arc::clone(&control) as Arc<dyn crate::ports::RecordingControlSink>,
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at,
            silence: Mutex::new(Some(crate::silence_auto_stop::SilenceAutoStop::new(
                std::time::Duration::from_secs(1),
                started_at,
            ))),
        };
        use crate::ports::RecordingProgressSink;
        progress
            .publish(crate::ports::RecordingEvent::LimitReached)
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            *control.requests.lock().unwrap(),
            vec![(stop_session, crate::events::RecordingControlAction::Stop)]
        );
        control.requests.lock().unwrap().clear();
        progress.publish_level(10, 0.1).unwrap();
        progress.publish_level(20, 0.1).unwrap();
        progress.publish_level(30, 0.1).unwrap();
        progress.publish_level(1_100, 0.0).unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            *control.requests.lock().unwrap(),
            vec![(stop_session, crate::events::RecordingControlAction::Stop)]
        );
        backend
            .services()
            .less_computer
            .abort_capture(stop_session)
            .unwrap();

        let fault_session = SessionId::new();
        backend
            .services()
            .less_computer
            .begin_capture(fault_session)
            .unwrap();
        let fault_progress = LessComputerRecordingProgress {
            session_id: fault_session,
            feedback: Arc::new(LessVoiceFeedback::new(
                backend.event_publisher(),
                fault_session,
                crate::events::LessComputerVoiceMode::Submit,
                crate::events::LessComputerVoicePhase::Recording,
            )),
            less_computer: Arc::clone(&backend.services().less_computer),
            control: Arc::clone(&control) as Arc<dyn crate::ports::RecordingControlSink>,
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at: std::time::Instant::now(),
            silence: Mutex::new(None),
        };
        fault_progress
            .publish(crate::ports::RecordingEvent::Fatal(BackendError::new(
                BackendErrorCode::Platform,
                "microphone disconnected",
            )))
            .unwrap();
        fault_progress
            .publish(crate::ports::RecordingEvent::Fatal(BackendError::new(
                BackendErrorCode::Platform,
                "duplicate native fault",
            )))
            .unwrap();
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        assert_eq!(backend.services().less_computer.active_session(), None);
        assert_eq!(
            *control.requests.lock().unwrap(),
            vec![
                (stop_session, crate::events::RecordingControlAction::Stop),
                (fault_session, crate::events::RecordingControlAction::Cancel),
            ]
        );
    }

    #[tokio::test]
    async fn qa_recording_progress_routes_silence_and_faults_inside_core() {
        let qa = Arc::new(FakeQaControl::default());
        let started_at = std::time::Instant::now();
        let progress = QaRecordingProgress {
            session_id: SessionId::new(),
            qa: Arc::clone(&qa) as Arc<dyn crate::domains::QaApi>,
            progress: Arc::new(VoiceRecordingProgress),
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at,
            silence: Mutex::new(Some(crate::silence_auto_stop::SilenceAutoStop::new(
                std::time::Duration::from_secs(1),
                started_at,
            ))),
            terminal: Mutex::new(QaRecordingTerminalState::default()),
        };
        progress.arm();
        use crate::ports::RecordingProgressSink;
        progress.publish_level(10, 0.1).unwrap();
        progress.publish_level(20, 0.1).unwrap();
        progress.publish_level(30, 0.1).unwrap();
        progress.publish_level(1_100, 0.0).unwrap();
        progress.publish_level(2_000, 0.0).unwrap();
        tokio::task::yield_now().await;

        assert_eq!(qa.stops.load(Ordering::Acquire), 1);
        assert_eq!(qa.cancels.load(Ordering::Acquire), 0);
        assert_eq!(qa.faults.load(Ordering::Acquire), 0);

        let limit_progress = QaRecordingProgress {
            session_id: SessionId::new(),
            qa: Arc::clone(&qa) as Arc<dyn crate::domains::QaApi>,
            progress: Arc::new(VoiceRecordingProgress),
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at: std::time::Instant::now(),
            silence: Mutex::new(None),
            terminal: Mutex::new(QaRecordingTerminalState::default()),
        };
        limit_progress.arm();
        limit_progress
            .publish(crate::ports::RecordingEvent::LimitReached)
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(qa.stops.load(Ordering::Acquire), 2);

        let started_at = std::time::Instant::now();
        let no_speech = QaRecordingProgress {
            session_id: SessionId::new(),
            qa: Arc::clone(&qa) as Arc<dyn crate::domains::QaApi>,
            progress: Arc::new(VoiceRecordingProgress),
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at,
            silence: Mutex::new(Some(crate::silence_auto_stop::SilenceAutoStop::new(
                std::time::Duration::from_secs(1),
                started_at,
            ))),
            terminal: Mutex::new(QaRecordingTerminalState::default()),
        };
        no_speech.arm();
        no_speech.publish_level(10_000, 0.0).unwrap();
        tokio::task::yield_now().await;
        assert_eq!(qa.cancels.load(Ordering::Acquire), 1);

        let queued_fault = QaRecordingProgress {
            session_id: SessionId::new(),
            qa: Arc::clone(&qa) as Arc<dyn crate::domains::QaApi>,
            progress: Arc::new(VoiceRecordingProgress),
            task_spawner: Arc::new(TokioTaskSpawner),
            started_at: std::time::Instant::now(),
            silence: Mutex::new(None),
            terminal: Mutex::new(QaRecordingTerminalState::default()),
        };
        queued_fault
            .publish(crate::ports::RecordingEvent::Fatal(BackendError::new(
                BackendErrorCode::Platform,
                "microphone disconnected",
            )))
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(qa.faults.load(Ordering::Acquire), 0);
        queued_fault.arm();
        tokio::task::yield_now().await;
        assert_eq!(qa.faults.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn selection_voice_capture_uses_core_silence_policy_before_host_control() {
        let data_dir = TestDataDir::new("selection-voice-silence");
        // Three voiced blocks establish speech; the later quiet block crosses
        // the configured one-second threshold and must request exactly one
        // host Stop. The fixture timestamps make this deterministic without a
        // wall-clock sleep.
        let recorder = Arc::new(crate::testing::FixtureAudioRecorder::new(
            vec![vec![1, 0, 2, 0]],
            vec![(10, 0.1), (20, 0.1), (30, 0.1), (1_100, 0.0), (2_000, 0.0)],
        ));
        let engine = crate::PipelineDictationEngine::new(
            recorder,
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "edit this",
                120,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::successful("unused")),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                voice_activation_runtime: None,
                dictation_engine: Arc::new(engine),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.selection_voice_enabled = true;
        preferences.silence_auto_stop_enabled = true;
        preferences.silence_auto_stop_seconds = 1.0;
        preferences.hotkey.mode = crate::shared_types::HotkeyMode::Toggle;
        backend.set_preferences(preferences).unwrap();

        let session_id = backend
            .services()
            .selection_voice
            .begin(crate::domains::SelectionCapture {
                text: "draft".into(),
                source_app: Some("Editor".into()),
            })
            .await
            .unwrap();
        let mut events = backend.subscribe();
        let control = Arc::new(FakeRecordingControl::default());
        let capture = backend
            .start_selection_voice_capture(
                session_id,
                Arc::clone(&control) as Arc<dyn crate::ports::RecordingControlSink>,
            )
            .await
            .unwrap();
        tokio::task::yield_now().await;

        let levels: Vec<f32> = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event.kind {
                BackendEventKind::SelectionVoiceLevel(level) => Some(level.level),
                _ => None,
            })
            .collect();
        assert!(
            levels
                .iter()
                .any(|level| (*level - 0.1).abs() < f32::EPSILON),
            "selection voice must publish voiced levels for the shared capsule meter: {levels:?}"
        );
        assert!(
            levels.contains(&0.0),
            "selection voice must publish quiet levels for the shared capsule meter: {levels:?}"
        );

        assert_eq!(
            *control.requests.lock().unwrap(),
            vec![(session_id, crate::events::RecordingControlAction::Stop)]
        );
        capture.cancel().await.unwrap();
        backend
            .services()
            .selection_voice
            .cancel(Some(session_id))
            .await
            .unwrap();
    }

    /// Dictation fixtures exercise the polish path, so they run with a configured
    /// default LLM channel (`ark`). Unconfigured channels now run as Raw (issue #1032).
    fn llm_configured_credentials() -> Arc<crate::credentials::InMemoryCredentialStore> {
        Arc::new(
            crate::credentials::InMemoryCredentialStore::default().with_secret(
                CredentialKey::new(
                    crate::credentials::CredentialNamespace::Llm,
                    Some("ark".to_string()),
                    crate::credentials::LLM_API_KEY_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("fixture-key"),
            ),
        )
    }

    fn backend_with_dictation_engine(
        data_dir: std::path::PathBuf,
        dictation_engine: Arc<dyn DictationEngine>,
    ) -> OpenLessBackend {
        OpenLessBackend::new(
            BackendConfig {
                data_dir,
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine,
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: llm_configured_credentials(),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap()
    }

    fn history_session(id: &str) -> DictationSession {
        DictationSession {
            id: id.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source: crate::types::HistorySource::Voice,
            raw_transcript: "raw".to_string(),
            asr_transcript: None,
            final_text: "final".to_string(),
            mode: crate::types::PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: None,
            insert_status: crate::types::HistoryInsertStatus::Inserted,
            error_code: None,
            duration_ms: Some(1000),
            dictionary_entry_count: None,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: None,
            llm_model: None,
            pipeline_mode: None,
            asr_ms: None,
            polish_ms: None,
        }
    }

    #[test]
    fn asr_usage_counts_local_and_cloud_dictations_and_the_latest_kind() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-asr-usage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(FakeEngine));
        assert_eq!(backend.asr_usage().unwrap(), AsrUsage::default());

        let entries = [
            ("a", "2026-10-18T10:00:00+00:00", Some("volcengine")),
            ("b", "2026-10-18T10:01:00+00:00", Some("sherpa-onnx-local")),
            ("c", "2026-10-18T10:02:00+00:00", None),
            ("d", "2026-10-18T10:03:00+00:00", Some("volcengine")),
            ("e", "2026-10-18T10:04:00+00:00", Some("sherpa-onnx-local")),
        ];
        for (id, created_at, provider) in entries {
            let mut entry = history_session(id);
            entry.created_at = created_at.into();
            entry.asr_provider = provider.map(Into::into);
            backend.append_history(entry, 30, None).unwrap();
        }

        assert_eq!(
            backend.asr_usage().unwrap(),
            AsrUsage {
                local: 2,
                cloud: 2,
                last_local: Some(true)
            }
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn asr_usage_ignores_entries_without_a_successful_recognition() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-asr-usage-failures-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(FakeEngine));

        let entries = [
            (
                "completed-local",
                "2026-10-18T10:00:00+00:00",
                "sherpa-onnx-local",
                None,
            ),
            (
                "recording-draft",
                "2026-10-18T10:01:00+00:00",
                "volcengine",
                Some("recording"),
            ),
            (
                "cancelled",
                "2026-10-18T10:02:00+00:00",
                "sherpa-onnx-local",
                Some("cancelled"),
            ),
            (
                "recording-failed",
                "2026-10-18T10:03:00+00:00",
                "volcengine",
                Some("recordingFailed"),
            ),
            (
                "transcribe-failed",
                "2026-10-18T10:04:00+00:00",
                "sherpa-onnx-local",
                Some("transcribeFailed"),
            ),
            (
                "empty-transcript",
                "2026-10-18T10:05:00+00:00",
                "volcengine",
                Some("emptyTranscript"),
            ),
            (
                "insert-failed-after-asr",
                "2026-10-18T10:06:00+00:00",
                "volcengine",
                Some("insertFailed"),
            ),
            (
                "polish-failed-after-asr",
                "2026-10-18T10:07:00+00:00",
                "sherpa-onnx-local",
                Some("polishFailed"),
            ),
        ];
        for (id, created_at, provider, error_code) in entries {
            let mut entry = history_session(id);
            entry.created_at = created_at.into();
            entry.asr_provider = Some(provider.into());
            entry.error_code = error_code.map(Into::into);
            backend.append_history(entry, 30, None).unwrap();
        }

        assert_eq!(
            backend.asr_usage().unwrap(),
            AsrUsage {
                local: 2,
                cloud: 1,
                last_local: Some(true)
            }
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn vocabulary_facade_persists_shared_types_and_publishes_revisions() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-vocab-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();

        let entry = backend.add_vocabulary("OpenLess".into(), None).unwrap();
        let rule = backend
            .add_correction_rule("几粒".into(), "几例".into())
            .unwrap();
        backend.set_vocabulary_enabled(&entry.id, false).unwrap();
        backend.remove_correction_rule(&rule.id).unwrap();

        assert!(!backend.list_vocabulary().unwrap()[0].enabled);
        assert!(backend.list_correction_rules().unwrap().is_empty());
        assert_eq!(backend.snapshot().vocabulary_revision, 4);
        for expected_revision in 1..=4 {
            let event = events.try_recv().unwrap();
            assert_eq!(
                event.kind,
                BackendEventKind::VocabularyChanged(VocabularyChange {
                    revision: expected_revision,
                })
            );
        }
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn correction_suggestions_are_bounded_idempotent_and_committed_by_core() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-correction-suggestions-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap();
        let mut events = backend.subscribe();

        let first = backend
            .queue_pending_correction("扣的爱思".into(), "Codex".into())
            .unwrap()
            .unwrap();
        assert!(backend
            .queue_pending_correction("扣的爱思".into(), "Codex".into())
            .unwrap()
            .is_none());
        for index in 0..MAX_PENDING_CORRECTIONS {
            backend
                .queue_pending_correction(format!("old-{index}"), format!("new-{index}"))
                .unwrap();
        }
        let pending = backend.pending_corrections();
        assert_eq!(pending.len(), MAX_PENDING_CORRECTIONS);
        assert!(pending.iter().all(|item| item.id != first.id));

        let accepted = pending[0].clone();
        assert_eq!(
            backend
                .accept_pending_correction(&accepted.id)
                .unwrap()
                .unwrap(),
            accepted
        );
        assert!(backend
            .accept_pending_correction(&accepted.id)
            .unwrap()
            .is_none());
        let learned = backend
            .list_vocabulary()
            .unwrap()
            .into_iter()
            .find(|entry| entry.phrase == accepted.replacement)
            .unwrap();
        assert_eq!(learned.note.as_deref(), Some("从手改中自动收集"));

        let rejected = backend.pending_corrections()[0].id.clone();
        assert!(backend.reject_pending_correction(&rejected));
        assert!(!backend.reject_pending_correction(&rejected));
        backend.dismiss_pending_corrections();
        backend.dismiss_pending_corrections();
        assert!(backend.pending_corrections().is_empty());

        let mut suggestion_events = 0;
        let mut vocabulary_events = 0;
        while let Ok(event) = events.try_recv() {
            match event.kind {
                BackendEventKind::VocabularySuggestionsChanged(_) => suggestion_events += 1,
                BackendEventKind::VocabularyChanged(_) => vocabulary_events += 1,
                _ => {}
            }
        }
        assert_eq!(suggestion_events, MAX_PENDING_CORRECTIONS + 4);
        assert_eq!(vocabulary_events, 1);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn history_facade_persists_shared_types_and_publishes_revisions() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();

        let mut entry = history_session("one");
        backend.append_history(entry.clone(), 30, Some(20)).unwrap();
        let asr_call = crate::auxiliary::AsrCallLabel {
            provider: "channel-b".into(),
            model: Some("model-b".into()),
        };
        let completed_error = backend
            .apply_history_retranscription(
                &entry.id,
                "must not replace history".into(),
                &asr_call,
                1,
            )
            .unwrap_err();
        assert_eq!(completed_error.code, BackendErrorCode::InvalidState);

        entry.final_text = "updated".to_string();
        entry.error_code = Some("polishFailed".to_string());
        assert!(backend.update_history_entry(entry.clone()).unwrap());
        let polish_error = backend
            .apply_history_retranscription(&entry.id, "must remain a preview".into(), &asr_call, 1)
            .unwrap_err();
        assert_eq!(polish_error.code, BackendErrorCode::InvalidState);

        entry.error_code = Some("transcribeFailed".to_string());
        assert!(backend.update_history_entry(entry.clone()).unwrap());
        assert!(!backend
            .update_history_entry(history_session("missing"))
            .unwrap());
        let retranscribed = backend
            .apply_history_retranscription(&entry.id, "retranscribed".into(), &asr_call, 480)
            .unwrap();
        assert_eq!(retranscribed.raw_transcript, "retranscribed");
        assert_eq!(retranscribed.final_text, "retranscribed");
        assert_eq!(retranscribed.asr_provider.as_deref(), Some("channel-b"));
        assert_eq!(retranscribed.asr_model.as_deref(), Some("model-b"));
        assert_eq!(retranscribed.asr_ms, Some(480));
        assert!(retranscribed.error_code.is_none());
        assert!(retranscribed.llm_provider.is_none());
        assert!(retranscribed.llm_model.is_none());
        assert!(retranscribed.polish_ms.is_none());
        backend.delete_history(&entry.id).unwrap();
        backend.clear_history().unwrap();
        backend.record_activity("2026-08-27", 42, 1000).unwrap();

        assert!(backend.list_history().unwrap().is_empty());
        assert_eq!(backend.list_activity().unwrap()[0].chars, 42);
        assert_eq!(backend.snapshot().history_revision, 7);
        for expected_revision in 1..=7 {
            assert_eq!(
                events.try_recv().unwrap().kind,
                BackendEventKind::HistoryChanged(HistoryChange {
                    revision: expected_revision,
                })
            );
        }
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn focused_microphone_selection_persists_and_publishes_once() {
        let (backend, _) = backend();
        let mut events = backend.subscribe();

        backend
            .select_microphone_device("Studio microphone".to_string())
            .unwrap();

        assert_eq!(
            backend.get_preferences().microphone_device_name,
            "Studio microphone"
        );
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        ));
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[test]
    fn previous_style_use_case_owns_cycle_order_and_preferences_event() {
        let (backend, _) = backend();
        let before = backend.get_preferences();
        let mut events = backend.subscribe();

        let selected = backend
            .activate_previous_style_pack()
            .unwrap()
            .expect("default store has multiple enabled packs");

        assert_ne!(selected.id, before.active_style_pack_id);
        assert!(selected.active);
        assert_eq!(backend.get_preferences().active_style_pack_id, selected.id);
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert_eq!(backend.snapshot().style_pack_revision, 0);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        ));
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[test]
    fn style_pack_facade_owns_mutations_and_publishes_revisions() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-style-packs-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();
        let pack = backend
            .create_style_pack(StylePack {
                name: "Linux contract".to_string(),
                prompt: "prompt".to_string(),
                ..StylePack::default()
            })
            .unwrap();
        backend.set_style_pack_enabled(&pack.id, false).unwrap();
        backend.remove_style_pack(&pack.id).unwrap();

        assert_eq!(backend.snapshot().style_pack_revision, 3);
        let mut style_revisions = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let BackendEventKind::StylePacksChanged(change) = event.kind {
                style_revisions.push(change.revision);
            }
        }
        assert_eq!(style_revisions, vec![1, 2, 3]);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn activating_style_pack_publishes_preferences_and_style_revisions() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-activate-style-pack-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap();
        let pack = backend
            .create_style_pack(StylePack {
                name: "Activate me".to_string(),
                prompt: "prompt".to_string(),
                ..StylePack::default()
            })
            .unwrap();
        let mut events = backend.subscribe();
        let before = backend.snapshot();

        let active = backend.activate_style_pack(&pack.id).unwrap();

        assert!(active.active);
        assert_eq!(backend.get_preferences().active_style_pack_id, pack.id);
        let after = backend.snapshot();
        assert_eq!(after.preferences_revision, before.preferences_revision + 1);
        assert_eq!(after.style_pack_revision, before.style_pack_revision + 1);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(_)
        ));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::StylePacksChanged(_)
        ));
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn removing_style_pack_cleans_active_id_and_orphan_hotkey() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-remove-style-pack-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies::unsupported(),
        )
        .unwrap();
        let pack = backend
            .create_style_pack(StylePack {
                name: "Temporary pack".to_string(),
                prompt: "prompt".to_string(),
                ..StylePack::default()
            })
            .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.active_style_pack_id = pack.id.clone();
        preferences
            .style_pack_hotkeys
            .push(crate::shared_types::StylePackHotkey {
                pack_id: pack.id.clone(),
                binding: crate::shared_types::ShortcutBinding {
                    primary: "K".to_string(),
                    modifiers: vec!["ctrl".to_string()],
                },
            });
        backend.set_preferences(preferences).unwrap();

        let outcome = backend.remove_style_pack(&pack.id).unwrap();

        let preferences = backend.get_preferences();
        assert_ne!(preferences.active_style_pack_id, pack.id);
        assert!(preferences
            .style_pack_hotkeys
            .iter()
            .all(|entry| entry.pack_id != pack.id));
        let hotkey_change = outcome
            .effects
            .hotkeys
            .expect("removal must expose the host hotkey effect");
        assert!(hotkey_change
            .previous
            .style_packs
            .iter()
            .any(|entry| entry.pack_id == pack.id));
        assert!(hotkey_change
            .next
            .style_packs
            .iter()
            .all(|entry| entry.pack_id != pack.id));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn preferences_facade_persists_shared_contract_and_publishes_revisions() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-backend-preferences-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();

        let mut preferences = backend.get_preferences();
        preferences.microphone_device_name = "Shared microphone".to_string();
        backend.set_preferences(preferences).unwrap();

        assert_eq!(
            backend.get_preferences().microphone_device_name,
            "Shared microphone"
        );
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert_eq!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        );
        assert!(data_dir.join("preferences.json").is_file());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn validated_preferences_write_syncs_legacy_fields_and_publishes_once() {
        let (backend, _) = backend();
        let mut events = backend.subscribe();
        let mut preferences = backend.get_preferences();
        preferences.dictation_hotkey = crate::shared_types::ShortcutBinding {
            primary: "RightControl".to_string(),
            modifiers: Vec::new(),
        };

        backend.set_preferences_validated(preferences).unwrap();

        let saved = backend.get_preferences();
        assert_eq!(
            saved.hotkey.trigger,
            crate::shared_types::HotkeyTrigger::RightControl
        );
        assert_eq!(saved.custom_combo_hotkey, None);
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert_eq!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        );
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[test]
    fn validated_preferences_write_rejects_conflicts_without_mutation_or_event() {
        let (backend, _) = backend();
        let before = backend.get_preferences();
        let before_json = serde_json::to_value(&before).unwrap();
        let mut events = backend.subscribe();
        let mut conflicting = before.clone();
        conflicting.translation_hotkey = conflicting.dictation_hotkey.clone();

        let error = backend
            .set_preferences_validated(conflicting)
            .expect_err("conflicting shortcut must be rejected");

        assert_eq!(error.code, BackendErrorCode::InvalidArgument);
        assert_eq!(
            serde_json::to_value(backend.get_preferences()).unwrap(),
            before_json
        );
        assert_eq!(backend.snapshot().preferences_revision, 0);
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[test]
    fn validated_preferences_write_can_preserve_current_style_fields() {
        let (backend, _) = backend();
        let before = backend.get_preferences();
        let mut events = backend.subscribe();
        let mut incoming = before.clone();
        incoming.microphone_device_name = "Updated microphone".to_string();
        incoming.default_mode = crate::types::PolishMode::Raw;
        incoming.enabled_modes = vec![crate::types::PolishMode::Raw];
        incoming.active_style_pack_id = "incoming.style".to_string();
        incoming.custom_style_prompts.raw = "incoming prompt".to_string();

        backend
            .set_preferences_preserving_style_validated(incoming)
            .unwrap();

        let saved = backend.get_preferences();
        assert_eq!(saved.microphone_device_name, "Updated microphone");
        assert_eq!(saved.default_mode, before.default_mode);
        assert_eq!(saved.enabled_modes, before.enabled_modes);
        assert_eq!(saved.active_style_pack_id, before.active_style_pack_id);
        assert_eq!(saved.style_system_prompts, before.style_system_prompts);
        assert_eq!(saved.custom_style_prompts, before.custom_style_prompts);
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert_eq!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        );
        assert_eq!(events.try_recv(), Err(crate::events::EventRecvError::Empty));
    }

    #[derive(Default)]
    struct FailingSettingsRuntime {
        restore_error: Option<BackendError>,
    }

    impl crate::SettingsRuntime for FailingSettingsRuntime {
        fn prepare(
            &self,
            _plan: &crate::SettingsEffectPlan,
        ) -> Result<crate::SettingsEffectReceipt, crate::SettingsEffectFailure> {
            Err(crate::SettingsEffectFailure::after_side_effect(
                BackendError::new(BackendErrorCode::Platform, "runtime apply failed"),
                crate::SettingsEffectReceipt {
                    applied: vec![crate::SettingsEffectKind::ActiveAsrProvider],
                },
            ))
        }

        fn restore(
            &self,
            _plan: &crate::SettingsEffectPlan,
            _receipt: &crate::SettingsEffectReceipt,
        ) -> Result<(), BackendError> {
            self.restore_error.clone().map_or(Ok(()), Err)
        }
    }

    #[derive(Default)]
    struct RecordingSettingsRuntime {
        actions: Mutex<Vec<&'static str>>,
        fail_commit: bool,
    }

    impl crate::SettingsRuntime for RecordingSettingsRuntime {
        fn prepare(
            &self,
            plan: &crate::SettingsEffectPlan,
        ) -> Result<crate::SettingsEffectReceipt, crate::SettingsEffectFailure> {
            self.actions.lock().unwrap().push("prepare");
            Ok(crate::SettingsEffectReceipt {
                applied: plan
                    .active_asr_provider
                    .as_ref()
                    .map(|_| vec![crate::SettingsEffectKind::ActiveAsrProvider])
                    .unwrap_or_default(),
            })
        }

        fn commit(
            &self,
            _plan: &crate::SettingsEffectPlan,
            receipt: &mut crate::SettingsEffectReceipt,
        ) -> Result<(), crate::SettingsEffectFailure> {
            self.actions.lock().unwrap().push("commit");
            receipt.applied.push(crate::SettingsEffectKind::Hotkeys);
            if self.fail_commit {
                Err(crate::SettingsEffectFailure::after_side_effect(
                    BackendError::new(BackendErrorCode::Platform, "listener registration failed"),
                    receipt.clone(),
                ))
            } else {
                Ok(())
            }
        }

        fn restore(
            &self,
            _plan: &crate::SettingsEffectPlan,
            _receipt: &crate::SettingsEffectReceipt,
        ) -> Result<(), BackendError> {
            self.actions.lock().unwrap().push("restore");
            Ok(())
        }
    }

    #[test]
    fn settings_transaction_success_persists_and_publishes_once() {
        let (backend, _) = backend();
        let mut events = backend.subscribe();
        let mut next = backend.get_preferences();
        next.dictation_hotkey = crate::shared_types::ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };

        let outcome = backend
            .update_settings(
                next,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();

        assert_eq!(
            outcome.preferences.hotkey.trigger,
            crate::shared_types::HotkeyTrigger::RightControl
        );
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        ));
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
    }

    #[test]
    fn capsule_display_whole_settings_updates_preserve_v2_intent() {
        let (backend, _) = backend();
        let mut next = backend.get_preferences();
        next.show_capsule = false;
        backend
            .update_settings(
                next,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        assert_eq!(backend.get_preferences().capsule_display_mode_v2, None);
        for mode in [
            crate::CapsuleDisplayMode::AllScreens,
            crate::CapsuleDisplayMode::CursorScreen,
        ] {
            let mut next = backend.get_preferences();
            next.capsule_display_mode = mode;
            next.capsule_display_mode_v2 = None;
            backend
                .update_settings(
                    next,
                    crate::SettingsUpdateOptions::STRICT,
                    &crate::NoopSettingsRuntime,
                )
                .unwrap();
            assert_eq!(
                backend.get_preferences().capsule_display_mode_v2,
                Some(mode)
            );
            let mut unrelated = backend.get_preferences();
            unrelated.show_capsule = !unrelated.show_capsule;
            unrelated.capsule_display_mode_v2 = None;
            backend
                .update_settings(
                    unrelated,
                    crate::SettingsUpdateOptions::STRICT,
                    &crate::NoopSettingsRuntime,
                )
                .unwrap();
            assert_eq!(
                backend.get_preferences().capsule_display_mode_v2,
                Some(mode)
            );
        }
    }

    #[test]
    fn settings_runtime_failure_preserves_preferences_revision_and_events() {
        let (backend, _) = backend();
        let previous = backend.get_preferences();
        let mut events = backend.subscribe();
        let mut next = previous.clone();
        next.active_asr_provider = "fixture-asr".into();

        let error = backend
            .update_settings(
                next,
                crate::SettingsUpdateOptions::STRICT,
                &FailingSettingsRuntime::default(),
            )
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Platform);
        assert_eq!(
            serde_json::to_value(backend.get_preferences()).unwrap(),
            serde_json::to_value(previous).unwrap()
        );
        assert_eq!(backend.snapshot().preferences_revision, 0);
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
    }

    #[test]
    fn settings_transaction_reports_primary_and_compensation_errors() {
        let (backend, _) = backend();
        let mut next = backend.get_preferences();
        next.active_asr_provider = "fixture-asr".into();
        let runtime = FailingSettingsRuntime {
            restore_error: Some(BackendError::new(
                BackendErrorCode::Platform,
                "runtime restore failed",
            )),
        };

        let error = backend
            .update_settings(next, crate::SettingsUpdateOptions::STRICT, &runtime)
            .unwrap_err();

        assert_eq!(error.message, "runtime apply failed");
        let details = error.details.expect("structured transaction details");
        assert_eq!(details["primaryError"]["message"], "runtime apply failed");
        assert_eq!(
            details["compensationErrors"][0]["message"],
            "runtime restore failed"
        );
    }

    #[test]
    fn settings_commit_failure_never_persists_or_publishes() {
        let (backend, _) = backend();
        let previous = backend.get_preferences();
        let mut next = previous.clone();
        next.dictation_hotkey = crate::shared_types::ShortcutBinding {
            primary: "F9".into(),
            modifiers: vec!["ctrl".into()],
        };
        let mut events = backend.subscribe();
        let runtime = RecordingSettingsRuntime {
            fail_commit: true,
            ..RecordingSettingsRuntime::default()
        };

        let error = backend
            .update_settings(next, crate::SettingsUpdateOptions::STRICT, &runtime)
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Platform);
        assert_eq!(
            serde_json::to_value(backend.get_preferences()).unwrap(),
            serde_json::to_value(&previous).unwrap()
        );
        assert_eq!(backend.snapshot().preferences_revision, 0);
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
        assert_eq!(
            runtime.actions.lock().unwrap().as_slice(),
            ["prepare", "commit", "restore"]
        );
    }

    #[test]
    fn mouse_shortcut_registration_failure_rolls_back_before_retry() {
        let (backend, _) = backend();
        let previous = backend.get_preferences().dictation_hotkey;
        let mut next = backend.get_preferences();
        next.dictation_hotkey = crate::shared_types::ShortcutBinding {
            primary: "Mouse4".into(),
            modifiers: vec!["ctrl".into()],
        };
        let runtime = RecordingSettingsRuntime {
            fail_commit: true,
            ..RecordingSettingsRuntime::default()
        };
        assert!(backend
            .update_settings(next.clone(), crate::SettingsUpdateOptions::STRICT, &runtime)
            .is_err());
        assert_eq!(backend.get_preferences().dictation_hotkey, previous);
        assert_eq!(backend.snapshot().preferences_revision, 0);
        assert_eq!(
            runtime.actions.lock().unwrap().as_slice(),
            ["prepare", "commit", "restore"]
        );
        backend
            .update_settings(
                next.clone(),
                crate::SettingsUpdateOptions::STRICT,
                &RecordingSettingsRuntime::default(),
            )
            .unwrap();
        assert_eq!(
            backend.get_preferences().dictation_hotkey,
            next.dictation_hotkey
        );
        assert_eq!(backend.snapshot().preferences_revision, 1);
    }

    #[test]
    fn settings_persistence_failure_restores_prepared_effects() {
        let host = Arc::new(FakeHost::default());
        let data_dir = TestDataDir::new("settings-persistence-failure");
        let mut repositories = BackendRepositories::open(data_dir.path()).unwrap();
        repositories.preferences = Arc::new(crate::PreferencesStore::in_memory());
        let backend = OpenLessBackend::new_with_repositories(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: host,
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
            repositories,
        )
        .unwrap();
        let previous = backend.get_preferences();
        let mut next = previous.clone();
        next.active_asr_provider = "fixture-asr".into();
        let runtime = RecordingSettingsRuntime::default();
        let mut events = backend.subscribe();

        let error = backend
            .update_settings(next, crate::SettingsUpdateOptions::STRICT, &runtime)
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Persistence);
        assert_eq!(
            serde_json::to_value(backend.get_preferences()).unwrap(),
            serde_json::to_value(previous).unwrap()
        );
        assert_eq!(backend.snapshot().preferences_revision, 0);
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
        assert_eq!(
            runtime.actions.lock().unwrap().as_slice(),
            ["prepare", "commit", "restore"]
        );
    }

    #[test]
    fn settings_document_reconciles_conflicts_and_preserves_current_style() {
        let (backend, _) = backend();
        let previous = backend.get_preferences();
        let mut next = previous.clone();
        next.microphone_device_name = "updated microphone".into();
        next.default_mode = crate::types::PolishMode::Raw;
        next.enabled_modes = vec![crate::types::PolishMode::Raw];
        next.active_style_pack_id = "stale-style".into();
        next.translation_hotkey = next.dictation_hotkey.clone();
        let mut events = backend.subscribe();

        let outcome = backend
            .update_settings(
                next,
                crate::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();

        assert!(outcome.reconciled_hotkey_count > 0);
        assert_eq!(
            outcome.preferences.microphone_device_name,
            "updated microphone"
        );
        assert_eq!(outcome.preferences.default_mode, previous.default_mode);
        assert_eq!(outcome.preferences.enabled_modes, previous.enabled_modes);
        assert_eq!(
            outcome.preferences.active_style_pack_id,
            previous.active_style_pack_id
        );
        crate::reject_hotkey_collisions(&outcome.preferences).unwrap();
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        ));
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
    }

    #[test]
    fn settings_revision_guard_rejects_one_of_two_concurrent_stale_documents() {
        let (backend, _) = backend();
        let backend = Arc::new(backend);
        let expected_revision = backend.snapshot().preferences_revision;
        let mut microphone_update = backend.get_preferences();
        microphone_update.microphone_device_name = "concurrent microphone".into();
        let mut theme_update = backend.get_preferences();
        theme_update.theme_mode = crate::shared_types::ThemeMode::Light;
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let spawn = |preferences| {
            let backend = Arc::clone(&backend);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                backend.update_settings(
                    preferences,
                    crate::SettingsUpdateOptions::STRICT.at_revision(expected_revision),
                    &crate::NoopSettingsRuntime,
                )
            })
        };
        let first = spawn(microphone_update);
        let second = spawn(theme_update);
        let mut events = backend.subscribe();
        barrier.wait();

        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let stale = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one stale settings document");
        assert_eq!(stale.code, BackendErrorCode::Busy);
        assert!(stale.retryable);
        assert_eq!(backend.snapshot().preferences_revision, 1);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision: 1 })
        ));
        assert!(matches!(
            events.try_recv(),
            Err(crate::EventRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn credentials_facade_keeps_secrets_out_of_snapshots_and_publishes_status() {
        let credential_store = Arc::new(crate::credentials::InMemoryCredentialStore::default());
        credential_store.set_status(CredentialsStatus {
            active_asr_provider: "fixture-asr".to_string(),
            active_llm_provider: "fixture-llm".to_string(),
            asr_configured: true,
            ..CredentialsStatus::default()
        });
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: std::env::temp_dir().join(format!(
                    "openless-core-credentials-{}",
                    uuid::Uuid::new_v4().simple()
                )),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: credential_store.clone(),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();
        let startup = backend.start().await.unwrap();
        assert_eq!(
            startup.backend.credentials.active_asr_provider,
            "fixture-asr"
        );
        assert!(startup.backend.credentials.asr_configured);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::BackendStarted
        ));

        let key = crate::credentials::CredentialKey::new(
            crate::credentials::CredentialNamespace::Asr,
            Some("fixture-asr".to_string()),
            "api_key",
        )
        .unwrap();
        backend
            .set_credential(
                key.clone(),
                crate::credentials::SecretValue::new("not-in-the-snapshot"),
            )
            .await
            .unwrap();
        assert_eq!(
            backend
                .read_credential(key)
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "not-in-the-snapshot"
        );
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::CredentialsChanged(_)
        ));
        let snapshot_json = serde_json::to_string(&backend.snapshot()).unwrap();
        assert!(!snapshot_json.contains("not-in-the-snapshot"));
        assert!(!snapshot_json.contains("api_key"));
    }

    #[tokio::test]
    async fn provider_channel_facade_owns_mutations_and_active_selection() {
        let credential_store = Arc::new(crate::credentials::InMemoryCredentialStore::default());
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: std::env::temp_dir().join(format!(
                    "openless-core-provider-channels-{}",
                    uuid::Uuid::new_v4().simple()
                )),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store,
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();

        let id = backend
            .create_channel(
                crate::credentials::ChannelKind::Asr,
                "openai-compatible".to_string(),
                "Primary".to_string(),
            )
            .await
            .unwrap();
        backend
            .rename_channel(
                crate::credentials::ChannelKind::Asr,
                id.clone(),
                "Renamed".to_string(),
            )
            .await
            .unwrap();
        backend
            .set_active_provider(crate::credentials::ProviderSlot::Asr, id.clone())
            .await
            .unwrap();

        let channels = backend
            .list_channels(crate::credentials::ChannelKind::Asr)
            .await
            .unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Renamed");
        assert_eq!(
            backend
                .active_provider(crate::credentials::ProviderSlot::Asr)
                .await
                .unwrap(),
            id
        );
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::CredentialsChanged(_)
        ));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::CredentialsChanged(_)
        ));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            BackendEventKind::CredentialsChanged(_)
        ));
    }

    #[tokio::test]
    async fn invalidating_llm_tests_preserves_asr_test_results() {
        let (backend, _) = backend();
        for (kind, provider, name) in [
            (ChannelKind::Llm, "custom", "first"),
            (ChannelKind::Llm, "custom_messages", "second"),
            (ChannelKind::Asr, "openai-compatible", "asr"),
        ] {
            let id = backend
                .create_channel(kind, provider.into(), name.into())
                .await
                .unwrap();
            backend
                .record_channel_test(kind, id, true, Some(1), None)
                .await
                .unwrap();
        }

        backend
            .invalidate_channel_tests(ChannelKind::Llm)
            .await
            .unwrap();

        assert!(backend
            .list_channels(ChannelKind::Llm)
            .await
            .unwrap()
            .iter()
            .all(|channel| channel.last_test.is_none()));
        assert!(backend.list_channels(ChannelKind::Asr).await.unwrap()[0]
            .last_test
            .is_some());
    }

    #[tokio::test]
    async fn llm_protocol_mutations_reset_protocol_overrides_and_invalidate_tests() {
        use crate::credentials::{CredentialNamespace, InMemoryCredentialStore, SecretValue};
        use crate::llm_protocol::*;
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: std::env::temp_dir()
                    .join(format!("openless-protocol-{}", uuid::Uuid::new_v4())),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let id = backend
            .create_channel(ChannelKind::Llm, "custom".into(), "test".into())
            .await
            .unwrap();
        let key = |account: &str| {
            CredentialKey::new(CredentialNamespace::Llm, Some(id.clone()), account).unwrap()
        };
        backend
            .set_credential(key(REQUEST_FORMAT_ACCOUNT), SecretValue::new("messages"))
            .await
            .unwrap();
        backend
            .set_credential(key(SERVICE_TIER_ACCOUNT), SecretValue::new("fast"))
            .await
            .unwrap();
        assert_eq!(
            backend
                .read_credential(key(SERVICE_TIER_ACCOUNT))
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "fast"
        );
        backend
            .set_credential(
                key(crate::credentials::LLM_API_KEY_ACCOUNT),
                SecretValue::new("fixture-key"),
            )
            .await
            .unwrap();
        backend
            .record_channel_test(ChannelKind::Llm, id.clone(), true, Some(1), None)
            .await
            .unwrap();
        assert!(backend.list_channels(ChannelKind::Llm).await.unwrap()[0]
            .last_test
            .is_some());
        backend
            .set_credential(
                key(crate::credentials::LLM_MODEL_ACCOUNT),
                SecretValue::new("new-model"),
            )
            .await
            .unwrap();
        assert!(backend.list_channels(ChannelKind::Llm).await.unwrap()[0]
            .last_test
            .is_none());
        assert_eq!(
            backend
                .read_credential(key(REQUEST_FORMAT_ACCOUNT))
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "messages"
        );
        assert!(backend
            .set_credential(key(REQUEST_FORMAT_ACCOUNT), SecretValue::new("invalid"))
            .await
            .is_err());
        backend
            .set_channel_provider_type(ChannelKind::Llm, id.clone(), "custom_responses".into())
            .await
            .unwrap();
        assert!(backend
            .read_credential(key(REQUEST_FORMAT_ACCOUNT))
            .await
            .unwrap()
            .is_none());
        assert!(backend
            .read_credential(key(SERVICE_TIER_ACCOUNT))
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            backend
                .read_credential(key(crate::credentials::LLM_API_KEY_ACCOUNT))
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "fixture-key"
        );
        assert_eq!(
            backend
                .read_credential(key(crate::credentials::LLM_MODEL_ACCOUNT))
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "new-model"
        );
    }

    #[tokio::test]
    async fn lifecycle_is_idempotent_and_emits_started_once_per_transition() {
        let (backend, _) = backend();
        let mut events = backend.subscribe();
        backend.start().await.unwrap();
        backend.start().await.unwrap();
        assert_eq!(
            events.recv().await.unwrap().kind,
            BackendEventKind::BackendStarted
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), events.recv())
                .await
                .is_err()
        );
        backend.shutdown().await.unwrap();
        backend.shutdown().await.unwrap();
        assert_eq!(
            events.recv().await.unwrap().kind,
            BackendEventKind::BackendStopping
        );
    }

    struct PersistenceOnlyCredentialStore;

    impl crate::credentials::CredentialStore for PersistenceOnlyCredentialStore {
        fn status(
            &self,
            _preferences: crate::shared_types::UserPreferences,
        ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "无法读取已保存的凭据：temporarily unavailable",
                ))
            })
        }

        fn read(
            &self,
            _key: crate::credentials::CredentialKey,
        ) -> BoxFuture<'static, Result<Option<crate::credentials::SecretValue>, BackendError>>
        {
            Box::pin(async { Ok(None) })
        }

        fn write(
            &self,
            _key: crate::credentials::CredentialKey,
            _value: crate::credentials::SecretValue,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }

        fn remove(
            &self,
            _key: crate::credentials::CredentialKey,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn start_survives_persistent_vault_read_failure() {
        let data_dir = TestDataDir::new("vault-persistence-start");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(PersistenceOnlyCredentialStore),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let first = backend.start().await.expect("first start must not fail");
        let second = backend
            .start()
            .await
            .expect("handshake start must not fail");
        assert!(first.backend.running);
        assert!(second.backend.running);
        let _ = data_dir;
    }

    #[tokio::test]
    async fn front_app_is_captured_without_reading_documents_when_cursor_context_is_disabled() {
        let data_dir = TestDataDir::new("host-context-privacy");
        let host_context = Arc::new(FakeHostContext::default());
        let mut services = crate::domains::BackendServices::unsupported();
        services.host_context = host_context.clone();
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                voice_activation_runtime: None,
                services,
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                credential_store: Arc::new(crate::InMemoryCredentialStore::default()),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();

        let context = backend
            .capture_host_dictation_context(DictationStartOptions::default())
            .await
            .unwrap();
        assert_eq!(host_context.0.load(Ordering::Acquire), 1);
        assert_eq!(host_context.1.load(Ordering::Acquire), 0);
        assert!(context.polish.cursor_context.is_none());
        assert_eq!(
            context.polish.front_app.as_deref(),
            Some("Terminal (com.apple.Terminal)")
        );
        assert_eq!(
            context.insertion.macos_newline_mode,
            crate::shared_types::MacosNewlineMode::LineFeed
        );

        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = true;
        backend.set_preferences(preferences).unwrap();
        let context = backend
            .capture_host_dictation_context(DictationStartOptions::default())
            .await
            .unwrap();
        assert_eq!(host_context.0.load(Ordering::Acquire), 2);
        assert_eq!(host_context.1.load(Ordering::Acquire), 1);
        assert_eq!(
            context.polish.front_app.as_deref(),
            Some("Terminal (com.apple.Terminal)")
        );
        assert!(context.polish.cursor_context.is_some());

        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = false;
        backend.set_preferences(preferences).unwrap();
        let context = backend
            .capture_host_dictation_context(DictationStartOptions {
                cursor_context: Some("must not survive a disabled privacy switch".into()),
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        assert!(context.polish.cursor_context.is_none());
        backend.start().await.unwrap();
        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();
        let history = backend.history.list().unwrap();
        assert_eq!(
            history[0].app_bundle_id.as_deref(),
            cfg!(target_os = "macos").then_some("com.apple.Terminal")
        );
        assert_eq!(
            history[0].app_name.as_deref(),
            Some(if cfg!(target_os = "macos") {
                "Terminal"
            } else {
                "Terminal (com.apple.Terminal)"
            })
        );
        assert_eq!(
            host_context.1.load(Ordering::Acquire),
            1,
            "only the explicitly enabled capture may read a document"
        );
    }

    #[tokio::test]
    async fn realtime_recording_fault_finishes_once_and_rejects_stale_reports() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let session_id = backend.start_dictation().await.unwrap();
        backend
            .engine_progress_sink()
            .publish(
                session_id,
                EngineProgress::TranscriptDelta(crate::TranscriptDelta {
                    text: "recognized draft".into(),
                    offset: 0,
                    is_final: false,
                }),
            )
            .unwrap();
        let error = BackendError::new(BackendErrorCode::Platform, "device disconnected");
        backend
            .engine_progress_sink()
            .publish(session_id, EngineProgress::RecordingFault(error.clone()))
            .unwrap();
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Failed);

        backend
            .report_recording_fault(session_id, error.clone())
            .await
            .unwrap();
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code.as_deref(), Some("recordingFailed"));
        assert_eq!(history[0].raw_transcript, "recognized draft");
        assert_eq!(
            history[0].asr_transcript.as_deref(),
            Some("recognized draft")
        );
        assert!(history[0].final_text.is_empty());
        let next_session = backend.start_dictation().await.unwrap();
        assert_eq!(
            backend
                .report_recording_fault(session_id, error)
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );
        assert_eq!(backend.list_history().unwrap().len(), 1);
        assert_eq!(backend.snapshot().dictation.session_id, Some(next_session));
        backend.cancel_dictation(Some(next_session)).await.unwrap();
    }

    #[tokio::test]
    async fn recording_fault_preserves_draft_and_audio_but_explicit_cancel_discards_audio() {
        let data_dir = TestDataDir::new("recording-fault-retention");
        let engine = crate::PipelineDictationEngine::new(
            Arc::new(crate::ExternalAudioRecorder::with_recordings_directory(
                data_dir.path().join("recordings"),
            )),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "complete", 1000,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::successful("complete")),
        );
        let backend =
            backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine));
        backend.start().await.unwrap();
        let pcm = vec![1, 0, 2, 0];
        let id = backend.start_external_dictation().await.unwrap();
        backend.feed_external_pcm(id, &pcm).unwrap();
        backend
            .engine_progress_sink()
            .publish(
                id,
                EngineProgress::TranscriptDelta(crate::TranscriptDelta {
                    text: "unfinished draft".into(),
                    offset: 0,
                    is_final: false,
                }),
            )
            .unwrap();
        backend
            .report_recording_fault(
                id,
                BackendError::new(BackendErrorCode::Platform, "fixture backlog"),
            )
            .await
            .unwrap();
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].raw_transcript, "unfinished draft");
        assert!(history[0].final_text.is_empty());
        assert_eq!(history[0].has_audio_recording, Some(true));
        let wav =
            std::fs::read(data_dir.path().join("recordings").join(format!("{id}.wav"))).unwrap();
        assert_eq!(crate::audio::decode_dictation_wav(&wav).unwrap(), pcm);

        let next = backend.start_external_dictation().await.unwrap();
        backend.feed_external_pcm(next, &pcm).unwrap();
        backend.cancel_dictation(Some(next)).await.unwrap();
        assert!(!data_dir
            .path()
            .join("recordings")
            .join(format!("{next}.wav"))
            .exists());
        assert_eq!(backend.list_history().unwrap().len(), 1);
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn edit_observation_is_core_gated_and_rejects_stale_reports() {
        let data_dir = TestDataDir::new("edit-observation");
        let observation = Arc::new(FakeEditObservation::default());
        let mut services = crate::domains::BackendServices::unsupported();
        services.edit_observation = observation.clone();
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services,
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.vocabulary_learning_enabled = true;
        assert!(
            !preferences.cursor_context_enabled,
            "learning must not require uploading cursor context"
        );
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        assert_eq!(
            observation.typed_texts.lock().unwrap().as_slice(),
            ["polished"]
        );
        let edit = crate::host_document::EditPair {
            source: "polished".into(),
            target: "Polished".into(),
            before: String::new(),
            after: String::new(),
        };
        observation.publish(0, edit.clone());
        assert_eq!(backend.pending_corrections().len(), 1);

        backend.dismiss_pending_corrections();
        let mut preferences = backend.get_preferences();
        preferences.vocabulary_learning_enabled = false;
        backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        observation.publish(0, edit);
        assert!(backend.pending_corrections().is_empty());
    }

    #[tokio::test]
    async fn disabled_vocabulary_learning_is_not_rearmed_by_an_older_dictation() {
        for streaming in [false, true] {
            let data_dir = TestDataDir::new("privacy-disabled-during-dictation");
            let observation = Arc::new(FakeEditObservation::default());
            let mut services = crate::domains::BackendServices::unsupported();
            services.edit_observation = observation.clone();
            let backend = OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: Arc::new(FakeHost::default()),
                    text_inserter: Arc::new(FakeInserter),
                    dictation_engine: Arc::new(FakeEngine),
                    credential_store: Arc::new(
                        crate::credentials::InMemoryCredentialStore::default(),
                    ),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    services,
                    ..BackendDependencies::unsupported()
                },
            )
            .unwrap();
            let mut preferences = backend.get_preferences();
            preferences.vocabulary_learning_enabled = true;
            preferences.streaming_insert = streaming;
            backend.set_preferences(preferences).unwrap();
            backend.start().await.unwrap();
            backend.start_dictation().await.unwrap();
            let mut preferences = backend.get_preferences();
            preferences.vocabulary_learning_enabled = false;
            backend
                .update_settings(
                    preferences,
                    crate::SettingsUpdateOptions::STRICT,
                    &crate::NoopSettingsRuntime,
                )
                .unwrap();
            backend.stop_dictation().await.unwrap();
            assert!(
                observation.typed_texts.lock().unwrap().is_empty(),
                "a frozen true preference cannot restart native document observation"
            );
        }
    }

    #[tokio::test]
    async fn delayed_cancel_reply_cannot_hide_a_successor_session() {
        let (backend, host) = backend();
        backend.start().await.unwrap();
        let first = backend.start_dictation().await.unwrap();
        let mut cancelling = Box::pin(backend.cancel_dictation(Some(first)));
        assert!(futures_util::poll!(cancelling.as_mut()).is_pending());
        // Run executor-owned native cleanup, but intentionally do not resume
        // its caller. Releasing resources must not authorize a late Hide of B.
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match backend.start_dictation().await {
                    Ok(id) => break id,
                    Err(error) if error.code == BackendErrorCode::Busy => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("unexpected start failure: {error}"),
                }
            }
        })
        .await
        .unwrap();
        cancelling.await.unwrap();
        assert_eq!(backend.snapshot().dictation.session_id, Some(second));
        assert_eq!(
            host.0.lock().unwrap().last(),
            Some(&HostAction::ShowDictationFeedback)
        );
        backend.cancel_dictation(Some(second)).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn delayed_completed_callback_cannot_reset_or_observe_a_new_session() {
        struct BlockingCommit {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<(Mutex<bool>, std::sync::Condvar)>,
        }
        impl crate::SettingsRuntime for BlockingCommit {
            fn prepare(
                &self,
                _: &crate::SettingsEffectPlan,
            ) -> Result<crate::SettingsEffectReceipt, crate::SettingsEffectFailure> {
                Ok(crate::SettingsEffectReceipt::default())
            }
            fn commit(
                &self,
                _: &crate::SettingsEffectPlan,
                _: &mut crate::SettingsEffectReceipt,
            ) -> Result<(), crate::SettingsEffectFailure> {
                self.entered.notify_one();
                let mut released = self.release.0.lock().unwrap();
                while !*released {
                    released = self.release.1.wait(released).unwrap();
                }
                Ok(())
            }
            fn restore(
                &self,
                _: &crate::SettingsEffectPlan,
                _: &crate::SettingsEffectReceipt,
            ) -> Result<(), BackendError> {
                Ok(())
            }
        }
        struct ReleaseCommit(Arc<(Mutex<bool>, std::sync::Condvar)>);
        impl ReleaseCommit {
            fn release(&self) {
                *self
                    .0
                     .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                self.0 .1.notify_all();
            }
        }
        impl Drop for ReleaseCommit {
            fn drop(&mut self) {
                self.release();
            }
        }

        // Settings effects may synchronously wait for native hotkey registration.
        // Leave executor workers available while stop waits on that native
        // gate, so cancellation and a successor can make progress normally.
        let data_dir = TestDataDir::new("completed-callback-owner");
        let observation = Arc::new(FakeEditObservation::default());
        let host = Arc::new(FakeHost::default());
        let mut services = crate::domains::BackendServices::unsupported();
        services.edit_observation = observation.clone();
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: host.clone(),
                    text_inserter: Arc::new(FakeInserter),
                    dictation_engine: Arc::new(FakeEngine),
                    credential_store: Arc::new(
                        crate::credentials::InMemoryCredentialStore::default(),
                    ),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    services,
                    ..BackendDependencies::unsupported()
                },
            )
            .unwrap(),
        );
        let mut preferences = backend.get_preferences();
        preferences.vocabulary_learning_enabled = true;
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let first = backend.start_dictation().await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let release_guard = ReleaseCommit(release.clone());
        let settings_backend = backend.clone();
        let settings = std::thread::spawn({
            let entered = entered.clone();
            move || {
                let mut preferences = settings_backend.get_preferences();
                preferences.microphone_device_name = "pending settings microphone".into();
                settings_backend.update_settings(
                    preferences,
                    crate::SettingsUpdateOptions::STRICT,
                    &BlockingCommit { entered, release },
                )
            }
        });
        entered.notified().await;
        let mut events = backend.subscribe();
        let stopping_backend = backend.clone();
        let stopping = tokio::spawn(async move { stopping_backend.stop_dictation().await });
        loop {
            let event = events.recv().await.unwrap();
            if event.session_id == Some(first)
                && matches!(
                    event.kind,
                    BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                        phase: DictationPhase::Completed,
                        ..
                    })
                )
            {
                break;
            }
        }
        backend.cancel_dictation(Some(first)).await.unwrap();
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match backend.start_dictation().await {
                    Ok(id) => break id,
                    Err(error) if error.code == BackendErrorCode::Busy => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("unexpected start failure: {error}"),
                }
            }
        })
        .await
        .expect("successor dictation should become available after cancel");
        release_guard.release();
        settings.join().unwrap().unwrap();
        stopping.await.unwrap().unwrap();
        assert_eq!(backend.snapshot().dictation.session_id, Some(second));
        assert_eq!(
            backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );
        assert!(
            observation.typed_texts.lock().unwrap().is_empty(),
            "old completion cannot arm observation for the successor"
        );
        assert_eq!(
            host.0.lock().unwrap().last(),
            Some(&HostAction::ShowDictationFeedback)
        );
        backend.cancel_dictation(Some(second)).await.unwrap();
    }

    #[tokio::test]
    async fn question_prefix_handoff_cancels_dictation_and_discards_its_late_release() {
        let data_dir = TestDataDir::new("question-prefix-handoff");
        let engine = crate::testing::FixtureDictationEngine::successful("must not insert", "unused");
        let qa = Arc::new(FakeQaControl::default());
        let mut deps = BackendDependencies::unsupported();
        deps.dictation_engine = Arc::new(engine.clone());
        deps.host_actions = Arc::new(FakeHost::default());
        deps.text_inserter = Arc::new(crate::testing::FixtureTextInserter::with_outcome(InsertOutcome::Inserted));
        deps.credential_store = llm_configured_credentials();
        deps.services.qa = qa.clone();
        let backend = OpenLessBackend::new(BackendConfig {data_dir:data_dir.path().to_path_buf(), ..Default::default()}, deps).unwrap();
        backend.start().await.unwrap();
        let mut prefs = backend.get_preferences();
        prefs.hotkey.mode = crate::HotkeyMode::Hold;
        backend.set_preferences(prefs).unwrap();
        let at = std::time::Instant::now();
        backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {press_id:7,at}).await.unwrap();
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Recording);
        backend.start_voice_question_from_recording_key().await.unwrap();
        assert_eq!(qa.starts.load(Ordering::Acquire), 1);
        backend.dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {press_id:7,at:std::time::Instant::now()}).await.unwrap();
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert!(backend.list_history().unwrap().is_empty());
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn translation_picker_updates_current_engine_and_remembers_only_the_language() {
        let data_dir = TestDataDir::new("translation-picker-current");
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut prefs = backend.get_preferences();
        prefs.translation_target_language = "English".into();
        prefs.working_languages = vec!["简体中文".into()];
        prefs.microphone_device_name = "captured microphone".into();
        backend.set_preferences(prefs).unwrap();
        let session = backend.start_dictation_with_options(DictationStartOptions {
            translation_requested: true, ..Default::default()
        }).await.unwrap();
        let mut changed = backend.get_preferences();
        changed.microphone_device_name = "next microphone".into();
        backend.set_preferences(changed).unwrap();
        let selected = backend.select_dictation_translation_target(session, "ja".into()).unwrap();
        assert_eq!(selected.target_language, "日本語");
        assert_eq!(backend.get_preferences().translation_target_language, "日本語");
        assert_eq!(backend.get_preferences().microphone_device_name, "next microphone");
        backend.stop_dictation().await.unwrap();
        let contexts = engine.contexts();
        let last = contexts.last().unwrap();
        assert_eq!(last.polish.translation_target_language, "日本語");
        assert_eq!(last.recording.microphone_device_name.as_deref(), Some("captured microphone"));
        assert!(last.polish.translation_active);
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn translation_picker_save_failure_keeps_active_and_saved_targets() {
        let data_dir = TestDataDir::new("translation-picker-save-failure");
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine));
        backend.start().await.unwrap();
        let mut prefs = backend.get_preferences();
        prefs.translation_target_language = "English".into();
        prefs.working_languages = vec!["简体中文".into()];
        backend.set_preferences(prefs).unwrap();
        let session = backend.start_dictation_with_options(DictationStartOptions {
            translation_requested: true, ..Default::default()
        }).await.unwrap();
        let file = backend.preferences.persistence_path().to_path_buf();
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(backend.select_dictation_translation_target(session, "日本語".into()).is_err());
        assert_eq!(backend.dictation_translation_target().unwrap().target_language, "English");
        assert_eq!(backend.get_preferences().translation_target_language, "English");
        std::fs::remove_dir(&file).unwrap();
        backend.cancel_dictation(Some(session)).await.unwrap();
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn translation_picker_rejects_stale_sessions_finalization_and_redundant_targets() {
        let data_dir = TestDataDir::new("translation-picker-stale");
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine));
        backend.start().await.unwrap();
        let mut prefs = backend.get_preferences();
        prefs.translation_target_language = "English".into();
        prefs.working_languages = vec!["简体中文".into()];
        backend.set_preferences(prefs).unwrap();
        let options = DictationStartOptions { translation_requested: true, ..Default::default() };
        let first = backend.start_dictation_with_options(options.clone()).await.unwrap();
        backend.cancel_dictation(Some(first)).await.unwrap();
        let second = backend.start_dictation_with_options(options).await.unwrap();
        assert!(backend.select_dictation_translation_target(first, "日本語".into()).is_err());
        assert!(backend.select_dictation_translation_target(second, "简体中文".into()).is_err());
        assert!(backend.select_dictation_translation_target(second, "unknown".into()).is_err());
        assert_eq!(backend.dictation_translation_target().unwrap().target_language, "English");
        backend.stop_dictation().await.unwrap();
        assert!(backend.select_dictation_translation_target(second, "日本語".into()).is_err());
        assert_eq!(backend.get_preferences().translation_target_language, "English");
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dictation_captures_preferences_style_and_vocabulary_once_per_session() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-context-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "polished");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.microphone_device_name = "Session microphone".to_string();
        preferences.active_asr_provider = "local-qwen3".to_string();
        preferences.active_llm_provider = "openai-compatible".to_string();
        preferences.local_asr_active_model = "qwen3-asr-1.7b".to_string();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();
        backend
            .add_vocabulary("OpenLess".to_string(), None)
            .unwrap();

        backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: true,
                style_pack_id: None,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        assert!(backend.snapshot().dictation.translation_active);
        let mut changed = backend.get_preferences();
        changed.microphone_device_name = "Changed microphone".to_string();
        changed.active_asr_provider = "changed-provider".to_string();
        changed.translation_target_language = "日本語".to_string();
        backend.set_preferences(changed).unwrap();
        backend.stop_dictation().await.unwrap();

        let contexts = engine.contexts();
        assert_eq!(contexts.len(), 1);
        let context = &contexts[0];
        assert_eq!(
            context.recording.microphone_device_name.as_deref(),
            Some("Session microphone")
        );
        assert_eq!(context.asr.provider_id, "local-qwen3");
        assert_eq!(context.asr.model.as_deref(), Some("qwen3-asr-1.7b"));
        assert_eq!(context.asr.prompt.as_deref(), Some("OpenLess."));
        assert_eq!(context.polish.translation_target_language, "English");
        assert!(context.polish.translation_active);
        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn qa_handoff_releases_dictation_without_inserting_or_leaving_a_note() {
        let data_dir = TestDataDir::new("qa-dictation-handoff");
        let engine =
            crate::testing::FixtureDictationEngine::successful("question", "unused polish");
        let backend =
            backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let session = backend
            .start_dictation_with_options(DictationStartOptions {
                output_target: DictationOutputTarget::Undecided,
                ..Default::default()
            })
            .await
            .unwrap();
        let result = backend.stop_dictation_for_qa(session).await.unwrap();
        assert_eq!(result.raw_text, "question");
        assert_eq!(result.inserted, crate::types::InsertStatus::NotRequested);
        assert!(backend.list_history().unwrap().is_empty());
        let qa_session = SessionId::new();
        backend
            .voice_sessions
            .acquire(qa_session, crate::voice_session::VoiceSessionKind::Qa)
            .unwrap();
        backend.voice_sessions.release(qa_session);
        let contexts = engine.contexts();
        let context = contexts.last().unwrap();
        assert_eq!(context.output_target, DictationOutputTarget::Qa);
        assert!(!context.insertion.enabled);
        assert!(!context.uses_llm_polisher());
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn voice_edit_handoff_keeps_polish_without_inserting_or_recording_intermediate_history() {
        let data_dir = TestDataDir::new("voice-edit-dictation-handoff");
        let engine =
            crate::testing::FixtureDictationEngine::successful("raw draft", "polished draft");
        let backend =
            backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine.clone()));
        let mut preferences = backend.get_preferences();
        preferences.streaming_insert = true;
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let session = backend
            .start_dictation_with_options(DictationStartOptions {
                output_target: DictationOutputTarget::Undecided,
                ..Default::default()
            })
            .await
            .unwrap();
        let result = backend
            .stop_dictation_session_with_options(
                Some(session),
                DictationStopOptions::default(),
                Some(DictationOutputTarget::VoiceEdit),
            )
            .await
            .unwrap();
        assert_eq!(result.raw_text, "raw draft");
        assert_eq!(result.polished_text, "polished draft");
        assert_eq!(result.inserted, crate::types::InsertStatus::NotRequested);
        assert!(backend.list_history().unwrap().is_empty());
        let contexts = engine.contexts();
        let context = contexts.last().unwrap();
        assert_eq!(context.output_target, DictationOutputTarget::VoiceEdit);
        assert!(!context.insertion.enabled);
        assert!(!context.insertion.streaming);
        assert!(context.uses_llm_polisher());
        // Later instructions use the same output contract without another foreground handoff.
        let instruction = backend
            .start_dictation_with_options(DictationStartOptions {
                insert_text: false,
                output_target: DictationOutputTarget::VoiceEdit,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            backend
                .stop_dictation_session(instruction)
                .await
                .unwrap()
                .inserted,
            crate::types::InsertStatus::NotRequested
        );
        let contexts = engine.contexts();
        assert!(!contexts.last().unwrap().insertion.streaming);
        assert!(backend.list_history().unwrap().is_empty());
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stop_time_translation_updates_only_the_frozen_polish_choice() {
        use crate::testing::FixtureEngineAction;

        let data_dir = std::env::temp_dir().join(format!(
            "openless-stop-time-translation-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.microphone_device_name = "Frozen microphone".to_string();
        preferences.active_asr_provider = "local-qwen3".to_string();
        preferences.active_llm_provider = "openai-compatible".to_string();
        preferences.local_asr_active_model = "frozen-asr-model".to_string();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();
        let mut events = backend.subscribe();

        let session_id = backend.start_dictation().await.unwrap();
        assert!(!backend.snapshot().dictation.translation_active);
        let mut changed = backend.get_preferences();
        changed.microphone_device_name = "Changed microphone".to_string();
        changed.active_asr_provider = "changed-provider".to_string();
        changed.active_llm_provider = "changed-provider".to_string();
        changed.local_asr_active_model = "changed-asr-model".to_string();
        changed.translation_target_language = "日本語".to_string();
        changed.working_languages = vec!["English".to_string()];
        backend.set_preferences(changed).unwrap();

        backend
            .stop_dictation_with_options(DictationStopOptions {
                translation_requested: Some(true),
                ..DictationStopOptions::default()
            })
            .await
            .unwrap();

        assert_eq!(
            engine.actions(),
            vec![
                FixtureEngineAction::Start(session_id),
                FixtureEngineAction::UpdateContext(session_id),
                FixtureEngineAction::Finish(session_id),
            ]
        );
        let contexts = engine.contexts();
        assert_eq!(contexts.len(), 2);
        assert!(!contexts[0].polish.translation_active);
        let expected = contexts[0].with_translation_requested(true);
        assert_eq!(*contexts[1], expected);
        assert_eq!(
            contexts[1].recording.microphone_device_name.as_deref(),
            Some("Frozen microphone")
        );
        assert_eq!(contexts[1].asr.provider_id, "local-qwen3");
        assert_eq!(contexts[1].asr.model.as_deref(), Some("frozen-asr-model"));
        assert_eq!(contexts[1].llm.provider_id, "openai-compatible");
        assert_eq!(contexts[1].polish.translation_target_language, "English");
        assert_eq!(
            contexts[1].polish.working_languages,
            vec!["简体中文".to_string()]
        );
        assert!(backend.list_history().unwrap()[0].translation_active);
        let mut saw_translation_finalization = false;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event.kind,
                BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                    phase: DictationPhase::Transcribing,
                    translation_active: true,
                    ..
                })
            ) {
                saw_translation_finalization = true;
            }
        }
        assert!(saw_translation_finalization);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn stop_time_translation_can_disable_the_start_time_choice() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-stop-time-translation-off-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "polished");
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();

        backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: true,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        assert!(backend.snapshot().dictation.translation_active);
        backend
            .stop_dictation_with_options(DictationStopOptions {
                translation_requested: Some(false),
                ..DictationStopOptions::default()
            })
            .await
            .unwrap();

        let contexts = engine.contexts();
        assert_eq!(contexts.len(), 2);
        assert!(contexts[0].polish.translation_active);
        assert!(!contexts[1].polish.translation_active);
        assert!(!backend.list_history().unwrap()[0].translation_active);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn active_translation_update_changes_only_the_session_polish_choice() {
        use crate::testing::FixtureEngineAction;

        let data_dir = std::env::temp_dir().join(format!(
            "openless-active-translation-update-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.microphone_device_name = "Frozen microphone".to_string();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();

        let session_id = backend.start_dictation().await.unwrap();
        backend
            .update_dictation_translation_requested(true)
            .await
            .unwrap();
        assert!(backend.snapshot().dictation.translation_active);
        backend.stop_dictation().await.unwrap();

        assert_eq!(
            engine.actions(),
            vec![
                FixtureEngineAction::Start(session_id),
                FixtureEngineAction::UpdateContext(session_id),
                FixtureEngineAction::Finish(session_id),
            ]
        );
        let contexts = engine.contexts();
        assert_eq!(contexts.len(), 2);
        assert_eq!(
            contexts[1].recording.microphone_device_name.as_deref(),
            Some("Frozen microphone")
        );
        assert!(!contexts[0].polish.translation_active);
        assert!(contexts[1].polish.translation_active);
        assert!(backend.list_history().unwrap()[0].translation_active);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn translation_requested_during_context_capture_is_session_scoped() {
        struct SlowContext {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
        }
        impl HostContextAdapter for SlowContext {
            fn capture(
                &self,
                _: bool,
            ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
                let entered = self.entered.clone();
                let release = self.release.clone();
                boxed(async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(HostContextCapture::default())
                })
            }
        }
        let data_dir = TestDataDir::new("translation-during-context");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.host_actions = Arc::new(FakeHost::default());
        dependencies.text_inserter = Arc::new(FakeInserter);
        dependencies.dictation_engine = Arc::new(engine.clone());
        dependencies.services.host_context = Arc::new(SlowContext {
            entered: entered.clone(),
            release: release.clone(),
        });
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        let mut preferences = backend.get_preferences();
        preferences.translation_target_language = "English".into();
        preferences.working_languages = vec!["简体中文".into()];
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        for requested in [true, false] {
            let start = tokio::spawn({
                let backend = backend.clone();
                async move { backend.start_dictation().await }
            });
            entered.notified().await;
            if requested {
                let result = backend.update_dictation_translation_requested(true).await;
                release.notify_one();
                assert!(
                    result.is_ok(),
                    "Starting must retain translation before context exists: {result:?}"
                );
            } else {
                release.notify_one();
            }
            start.await.unwrap().unwrap();
            assert_eq!(backend.snapshot().dictation.translation_active, requested);
            backend.stop_dictation().await.unwrap();
            assert_eq!(
                engine.contexts().last().unwrap().polish.translation_active,
                requested
            );
        }
        backend.shutdown().await.unwrap();
    }

    async fn start_voice_test_session(
        backend: &OpenLessBackend,
        timeout: Option<std::time::Duration>,
        insert_text: bool,
    ) -> SessionId {
        let reservation = backend
            .reserve_dictation_session(insert_text, DictationOutputTarget::ForegroundApp, None)
            .unwrap();
        let id = reservation.session_id;
        *backend.voice_activation_session.lock().unwrap() =
            Some(crate::voice_activation::VoiceActivationSession {
                generation: crate::voice_activation::VoiceActivationGeneration(
                    backend.voice_activation_generation.load(Ordering::Acquire),
                ),
                session_id: id,
                timeout,
                commands: crate::voice_activation::VoiceActivationTextPolicy {
                    wake_phrase: "开始语音录入".into(),
                    finish_phrase: Some("完成语音录入".into()),
                },
            });
        backend
            .start_reserved_dictation(
                reservation,
                DictationStartOptions {
                    insert_text,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn voice_activation_silence_uses_earliest_enabled_timeout_and_rejects_old_session() {
        for (voice_timeout, general_timeout, expected) in [
            (Some(15), None, Some(15)),
            (None, None, None),
            (Some(15), Some(3), Some(3)),
            (None, Some(3), Some(3)),
        ] {
            let (backend, _) = backend();
            let mut preferences = backend.get_preferences();
            preferences.silence_auto_stop_enabled = general_timeout.is_some();
            preferences.silence_auto_stop_seconds = general_timeout.unwrap_or(3) as f32;
            preferences.hotkey.mode = crate::shared_types::HotkeyMode::Toggle;
            backend.set_preferences(preferences).unwrap();
            backend.start().await.unwrap();
            let id = start_voice_test_session(
                &backend,
                voice_timeout.map(std::time::Duration::from_secs),
                false,
            )
            .await;
            let context = backend
                .state
                .read()
                .unwrap()
                .dictation_context
                .clone()
                .unwrap();
            assert_eq!(
                context.recording.silence_after_ms,
                expected.map(|seconds| seconds * 1000)
            );
            let commands = context.voice_activation.clone();
            assert!(commands.is_some());
            assert_eq!(
                context.with_translation_requested(true).voice_activation,
                commands
            );
            assert_eq!(
                context
                    .with_output_target(DictationOutputTarget::QuickNote)
                    .voice_activation,
                commands
            );
            let progress = backend.engine_progress_sink();
            for elapsed_ms in (0..=30_000).step_by(100) {
                progress
                    .publish(
                        id,
                        EngineProgress::RecordingLevel {
                            elapsed_ms,
                            level: 0.1,
                        },
                    )
                    .unwrap();
            }
            let count_stops = || {
                backend.replay_events_after(0).events.iter().filter(|event|
                matches!(&event.kind, BackendEventKind::RecordingControlRequested(request) if request.session_id == id)
            ).count()
            };
            assert_eq!(count_stops(), 0);
            let deadline = 30_000 + expected.unwrap_or(300) * 1000;
            progress
                .publish(
                    id,
                    EngineProgress::RecordingLevel {
                        elapsed_ms: deadline - 1,
                        level: 0.0,
                    },
                )
                .unwrap();
            assert_eq!(count_stops(), 0);
            progress
                .publish(
                    id,
                    EngineProgress::RecordingLevel {
                        elapsed_ms: deadline,
                        level: 0.0,
                    },
                )
                .unwrap();
            assert_eq!(count_stops(), usize::from(expected.is_some()));
            progress
                .publish(
                    id,
                    EngineProgress::RecordingLevel {
                        elapsed_ms: deadline + 100,
                        level: 0.0,
                    },
                )
                .unwrap();
            assert_eq!(count_stops(), usize::from(expected.is_some()));
            let generation = backend.voice_activation_generation.load(Ordering::Acquire);
            backend.cancel_dictation(Some(id)).await.unwrap();
            let next = backend.start_dictation().await.unwrap();
            assert!(progress
                .publish(
                    id,
                    EngineProgress::RecordingLevel {
                        elapsed_ms: deadline + 200,
                        level: 0.0
                    }
                )
                .is_err());
            backend
                .finish_voice_activation(
                    generation,
                    id,
                    crate::voice_activation::VoiceActivationStopReason::Timeout,
                )
                .await;
            assert_eq!(backend.snapshot().dictation.session_id, Some(next));
            backend.cancel_dictation(Some(next)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn voice_activation_persists_clean_body_and_never_inserts_command_only_audio() {
        use crate::testing::{
            FixtureAudioRecorder, FixtureInsertionAction, FixtureTextInserter, FixtureTextPolisher,
            FixtureTranscriptionEngine,
        };
        for body in ["正文", "开始语音录入正文", ""] {
            let dir = TestDataDir::new("voice-command-history");
            let polisher = Arc::new(FixtureTextPolisher::successful("完整正文"));
            let inserter = Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted));
            let engine = crate::PipelineDictationEngine::new(
                Arc::new(FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])),
                Arc::new(FixtureTranscriptionEngine::successful(
                    format!("开始语音录入 {body} 完成语音录入"),
                    1000,
                )),
                polisher.clone(),
            );
            let backend = OpenLessBackend::new(
                BackendConfig {
                    data_dir: dir.path().to_path_buf(),
                    ..Default::default()
                },
                BackendDependencies {
                    host_actions: Arc::new(FakeHost::default()),
                    text_inserter: inserter.clone(),
                    dictation_engine: Arc::new(engine),
                    credential_store: llm_configured_credentials(),
                    ..BackendDependencies::unsupported()
                },
            )
            .unwrap();
            let mut prefs = backend.get_preferences();
            prefs.streaming_insert = false;
            backend.set_preferences(prefs).unwrap();
            backend.start().await.unwrap();
            let id = start_voice_test_session(&backend, None, true).await;
            let result = backend.stop_dictation_session(id).await;
            let history = backend.list_history().unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].raw_transcript, body);
            let final_transcript =
                backend
                    .replay_events_after(0)
                    .events
                    .into_iter()
                    .find_map(|event| match event.kind {
                        BackendEventKind::TranscriptDelta(delta) if delta.is_final => {
                            Some(delta.text)
                        }
                        _ => None,
                    });
            assert_eq!(final_transcript.as_deref(), Some(body));
            if body.is_empty() {
                assert!(result.is_err());
                assert!(history[0].final_text.is_empty());
                assert!(polisher.inputs().is_empty());
                assert!(!inserter.actions().iter().any(|action| matches!(
                    action,
                    FixtureInsertionAction::Insert { .. } | FixtureInsertionAction::Write { .. }
                )));
            } else {
                assert_eq!(result.unwrap().polished_text, "完整正文");
                assert_eq!(history[0].final_text, "完整正文");
                assert_eq!(polisher.inputs(), vec![body]);
                assert!(inserter.actions().iter().any(|action| matches!(action,
                    FixtureInsertionAction::Insert { text, .. } if text == "完整正文")));
            }
            assert!(backend.snapshot().dictation.session_id.is_none());
        }
    }

    #[tokio::test]
    async fn voice_activation_transcript_revisions_keep_raw_offsets() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let id = start_voice_test_session(&backend, None, false).await;
        let progress = backend.engine_progress_sink();
        for (text, offset, expected) in [
            ("开始语音录入你好", 0, "你好"),
            ("世界完成语音录入", 8, "你好世界"),
            ("你好，世界完成语音录入", 6, "你好，世界"),
        ] {
            progress
                .publish(
                    id,
                    EngineProgress::TranscriptDelta(crate::TranscriptDelta {
                        text: text.into(),
                        offset,
                        is_final: false,
                    }),
                )
                .unwrap();
            assert_eq!(
                backend.state.read().unwrap().transcripts[&id].text(),
                expected
            );
            if offset == 0 {
                backend
                    .stop_voice_activation(
                        crate::voice_activation::VoiceActivationStopReason::Disabled,
                    )
                    .await
                    .unwrap();
            }
        }
        backend.cancel_dictation(Some(id)).await.unwrap();
    }

    #[tokio::test]
    async fn recording_readiness_waits_for_the_first_pcm_even_when_its_meter_is_zero() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let session_id = backend.start_dictation().await.unwrap();
        assert_eq!(
            backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );
        assert_eq!(
            serde_json::to_value(backend.snapshot().dictation).unwrap()["recordingReady"],
            false
        );
        let before = backend.replay_events_after(0).latest_sequence;
        backend
            .engine_progress_sink()
            .publish(
                session_id,
                EngineProgress::RecordingLevel {
                    elapsed_ms: 0,
                    level: 0.0,
                },
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(backend.snapshot().dictation).unwrap()["recordingReady"],
            true
        );
        assert!(
            backend.replay_events_after(0).latest_sequence > before,
            "zero first meter must still publish readiness"
        );
        backend.cancel_dictation(None).await.unwrap();
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn recording_limit_requests_the_normal_dictation_stop_path() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let session_id = backend.start_dictation().await.unwrap();
        let mut events = backend.subscribe();

        backend
            .engine_progress_sink()
            .publish(session_id, EngineProgress::RecordingLimitReached)
            .unwrap();

        let request = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
            crate::events::BackendEvent {
                session_id: Some(id),
                kind: BackendEventKind::RecordingControlRequested(request),
                ..
            } if id == session_id => Some(request),
            _ => None,
        });
        assert_eq!(
            request.map(|request| request.action),
            Some(crate::events::RecordingControlAction::Stop)
        );

        backend.cancel_dictation(Some(session_id)).await.unwrap();
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stop_time_context_update_failure_cancels_and_resets_the_session() {
        use crate::testing::FixtureEngineAction;

        let data_dir = std::env::temp_dir().join(format!(
            "openless-stop-time-translation-failure-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::failing_context_update(
            "raw",
            "polished",
            BackendError::new(BackendErrorCode::Platform, "fixture context update failure"),
        );
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();

        let failed_session = backend.start_dictation().await.unwrap();
        let error = backend
            .stop_dictation_with_options(DictationStopOptions {
                translation_requested: Some(true),
                ..DictationStopOptions::default()
            })
            .await
            .expect_err("context update failure must abort finalization");
        assert_eq!(error.code, BackendErrorCode::Platform);
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert_eq!(backend.snapshot().dictation.session_id, None);
        assert_eq!(
            engine.actions(),
            vec![
                FixtureEngineAction::Start(failed_session),
                FixtureEngineAction::UpdateContext(failed_session),
                FixtureEngineAction::Cancel(failed_session),
            ]
        );
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code.as_deref(), Some("polishFailed"));
        assert!(history[0].translation_active);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn external_audio_uses_the_same_pipeline_and_is_strictly_session_scoped() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-external-audio-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let transcription = crate::testing::FixtureTranscriptionEngine::successful("raw", 125);
        let recorder = crate::AudioRecorderRouter::new(
            Arc::new(crate::testing::FixtureAudioRecorder::new(
                Vec::new(),
                Vec::new(),
            )),
            crate::ExternalAudioRecorder::default(),
        );
        let engine = crate::PipelineDictationEngine::new(
            Arc::new(recorder),
            Arc::new(transcription.clone()),
            Arc::new(crate::testing::FixtureTextPolisher::successful("polished")),
        );
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine));
        backend.start().await.unwrap();

        let session_id = backend.start_external_dictation().await.unwrap();
        assert_eq!(
            backend
                .feed_external_pcm(SessionId::new(), &[1, 0])
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );
        backend
            .feed_external_pcm(session_id, &[1, 0, 2, 0])
            .unwrap();
        assert_eq!(backend.snapshot().dictation.elapsed_ms, 0);
        let result = backend.stop_dictation_session(session_id).await.unwrap();
        assert_eq!(result.polished_text, "polished");
        assert_eq!(transcription.pcm(), vec![1, 0, 2, 0]);
        assert_eq!(
            backend
                .feed_external_pcm(session_id, &[3, 0])
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );

        let cancelled_session = backend.start_external_dictation().await.unwrap();
        backend
            .feed_external_pcm(cancelled_session, &[4, 0])
            .unwrap();
        backend
            .cancel_dictation(Some(cancelled_session))
            .await
            .unwrap();
        assert_eq!(
            backend
                .feed_external_pcm(cancelled_session, &[5, 0])
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn cloud_note_destination_can_be_revoked_and_cleanup_failure_is_visible() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-cloud-destination-{}",
            uuid::Uuid::new_v4()
        ));
        let engine = Arc::new(crate::testing::FixtureDictationEngine::successful(
            "raw", "polished",
        ));
        let backend = backend_with_dictation_engine(data_dir.clone(), engine.clone());
        backend.start().await.unwrap();
        let id = backend.start_dictation().await.unwrap();
        backend.set_dictation_cloud_note(id, true).unwrap();
        backend.set_dictation_cloud_note(id, false).unwrap();
        backend.stop_dictation_session(id).await.unwrap();
        assert_eq!(backend.list_history().unwrap().len(), 1);
        assert_eq!(
            engine.contexts().last().unwrap().output_target,
            DictationOutputTarget::ForegroundApp
        );
        let cloud = backend
            .start_dictation_with_options(DictationStartOptions {
                output_target: DictationOutputTarget::CloudNote,
                ..Default::default()
            })
            .await
            .unwrap();
        let blocked_path = data_dir.join("recordings").join(format!("{cloud}.wav"));
        std::fs::create_dir_all(&blocked_path).unwrap();
        assert_eq!(
            backend
                .stop_dictation_session(cloud)
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Persistence
        );
        assert_eq!(backend.list_history().unwrap().len(), 1);
        assert!(data_dir
            .join("cloud-note-cleanup")
            .join(cloud.to_string())
            .exists());
        std::fs::remove_dir(&blocked_path).unwrap();
        backend.shutdown().await.unwrap();
        backend.start().await.unwrap();
        assert!(!data_dir
            .join("cloud-note-cleanup")
            .join(cloud.to_string())
            .exists());
        backend.shutdown().await.unwrap();
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn cloud_notes_never_retain_history_or_audio_on_success_failure_or_cancel() {
        for outcome in [
            "success",
            "failure",
            "polish_failure",
            "empty",
            "cancel",
            "recording_fault",
        ] {
            let data_dir =
                std::env::temp_dir().join(format!("openless-cloud-note-{}", uuid::Uuid::new_v4()));
            let transcription = if outcome == "failure" {
                crate::testing::FixtureTranscriptionEngine::failing(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture",
                ))
            } else {
                crate::testing::FixtureTranscriptionEngine::successful(
                    if outcome == "empty" {
                        ""
                    } else {
                        "private text"
                    },
                    1000,
                )
            };
            let engine = crate::PipelineDictationEngine::new(
                Arc::new(crate::ExternalAudioRecorder::with_recordings_directory(
                    data_dir.join("recordings"),
                )),
                Arc::new(transcription),
                Arc::new(if outcome == "polish_failure" {
                    crate::testing::FixtureTextPolisher::failing(BackendError::new(
                        BackendErrorCode::Provider,
                        "fixture polish",
                    ))
                } else {
                    crate::testing::FixtureTextPolisher::successful("polished private text")
                }),
            );
            let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine));
            backend.start().await.unwrap();
            let mut prefs = backend.get_preferences();
            prefs.record_audio_for_debug = true;
            backend.set_preferences(prefs).unwrap();
            let id = backend.start_external_dictation().await.unwrap();
            backend.feed_external_pcm(id, &vec![1; 32000]).unwrap();
            backend.set_dictation_cloud_note(id, true).unwrap();
            if outcome == "cancel" {
                backend.cancel_dictation(Some(id)).await.unwrap();
            } else if outcome == "recording_fault" {
                backend
                    .report_recording_fault(
                        id,
                        BackendError::new(BackendErrorCode::Platform, "fixture recording fault"),
                    )
                    .await
                    .unwrap();
            } else {
                let result = backend.stop_dictation_session(id).await;
                assert_eq!(result.is_ok(), outcome == "success");
                if let Ok(result) = result {
                    assert_eq!(result.polished_text, "polished private text");
                }
            }
            assert!(backend.list_history().unwrap().is_empty(), "{outcome}");
            assert!(
                !data_dir
                    .join("recordings")
                    .join(format!("{id}.wav"))
                    .exists(),
                "{outcome}"
            );
            assert!(backend.set_dictation_cloud_note(id, true).is_err());
            backend.shutdown().await.unwrap();
            // Crash recovery may only remove recordings with an explicit cleanup marker.
            std::fs::create_dir_all(data_dir.join("cloud-note-cleanup")).unwrap();
            std::fs::write(data_dir.join("cloud-note-cleanup").join(id.to_string()), "").unwrap();
            let private_wav = data_dir.join("recordings").join(format!("{id}.wav"));
            std::fs::write(&private_wav, "private").unwrap();
            let retained = data_dir.join("recordings/ordinary.wav");
            std::fs::write(&retained, "ordinary").unwrap();
            backend.start().await.unwrap();
            assert!(!private_wav.exists());
            assert!(retained.exists());
            backend.shutdown().await.unwrap();
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    /// Android IME contract: the caret snapshot taken at start informs polish for
    /// ordinary dictation, quick notes and cloud notes alike, yet the result carries
    /// only the polished text and nothing on disk ever holds the snapshot.
    #[tokio::test]
    async fn cursor_context_informs_polish_for_every_target_but_is_never_persisted() {
        struct ContextRecordingPolisher(Arc<Mutex<Vec<Option<String>>>>);
        impl crate::ports::TextPolisher for ContextRecordingPolisher {
            fn polish(
                &self,
                _session_id: SessionId,
                context: Arc<DictationContext>,
                _raw_text: String,
                _partials: Arc<dyn crate::ports::TextStreamSink>,
            ) -> BoxFuture<'static, Result<crate::ports::PolishOutput, BackendError>> {
                self.0
                    .lock()
                    .unwrap()
                    .push(context.polish.cursor_context.clone());
                Box::pin(async { Ok(crate::ports::PolishOutput::text("polished words")) })
            }

            fn cancel(
                &self,
                _session_id: SessionId,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                Box::pin(async { Ok(()) })
            }
        }
        fn files_containing(
            dir: &std::path::Path,
            needle: &str,
            found: &mut Vec<std::path::PathBuf>,
        ) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files_containing(&path, needle, found);
                } else if std::fs::read(&path)
                    .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle.as_bytes()))
                    .unwrap_or(false)
                {
                    found.push(path);
                }
            }
        }

        const HOST_TEXT: &str = "host-document-text-7c1e";
        for target in ["dictation", "quick_note", "cloud_note"] {
            let data_dir = std::env::temp_dir()
                .join(format!("openless-cursor-context-{}", uuid::Uuid::new_v4()));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let engine = crate::PipelineDictationEngine::new(
                Arc::new(crate::ExternalAudioRecorder::with_recordings_directory(
                    data_dir.join("recordings"),
                )),
                Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                    "spoken words",
                    1000,
                )),
                Arc::new(ContextRecordingPolisher(seen.clone())),
            );
            let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine));
            backend.start().await.unwrap();
            let mut prefs = backend.get_preferences();
            prefs.cursor_context_enabled = true;
            prefs.record_audio_for_debug = true;
            backend.set_preferences(prefs).unwrap();
            let id = backend
                .start_external_dictation_with_options(DictationStartOptions {
                    insert_text: false,
                    front_app: Some("com.example.notes".into()),
                    cursor_context: Some(crate::prompts::cursor_context_input(HOST_TEXT, "")),
                    ..DictationStartOptions::default()
                })
                .await
                .unwrap();
            backend.feed_external_pcm(id, &vec![1; 32000]).unwrap();
            if target == "cloud_note" {
                backend.set_dictation_cloud_note(id, true).unwrap();
            }
            let result = backend
                .stop_dictation_session_with_options(
                    Some(id),
                    DictationStopOptions {
                        quick_note: (target != "cloud_note").then_some(target == "quick_note"),
                        ..DictationStopOptions::default()
                    },
                    None,
                )
                .await
                .unwrap();
            assert_eq!(result.polished_text, "polished words", "{target}");
            let seen = seen.lock().unwrap().clone();
            assert_eq!(seen.len(), 1, "{target}");
            assert!(
                seen[0]
                    .as_deref()
                    .is_some_and(|context| context.contains(HOST_TEXT)),
                "{target}: polish must still see the cursor context"
            );
            assert_eq!(
                backend.list_history().unwrap().is_empty(),
                target == "cloud_note",
                "{target}"
            );
            backend.shutdown().await.unwrap();
            let mut leaked = Vec::new();
            files_containing(&data_dir, HOST_TEXT, &mut leaked);
            assert!(
                leaked.is_empty(),
                "{target}: cursor context persisted in {leaked:?}"
            );
            // The scan is meaningful: what does get stored is readable the same way.
            let mut stored = Vec::new();
            files_containing(&data_dir, "polished words", &mut stored);
            assert_eq!(stored.is_empty(), target == "cloud_note", "{target}");
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    #[tokio::test]
    async fn external_audio_saves_failed_recordings_for_history_retry_and_prunes_successful_audio()
    {
        for fail in [false, true] {
            let data_dir = std::env::temp_dir()
                .join(format!("openless-remote-history-{}", uuid::Uuid::new_v4()));
            let transcription = if fail {
                crate::testing::FixtureTranscriptionEngine::failing(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture ASR failure",
                ))
            } else {
                crate::testing::FixtureTranscriptionEngine::successful("received speech", 120_000)
            };
            let engine = crate::PipelineDictationEngine::new(
                Arc::new(crate::ExternalAudioRecorder::with_recordings_directory(
                    data_dir.join("recordings"),
                )),
                Arc::new(transcription.clone()),
                Arc::new(crate::testing::FixtureTextPolisher::successful(
                    "complete transcription",
                )),
            );
            let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine));
            backend.start().await.unwrap();
            let session = backend.start_external_dictation().await.unwrap();
            let path = data_dir.join("recordings").join(format!("{session}.wav"));
            for second in 0..120 {
                backend
                    .feed_external_pcm(session, &vec![second; 32_000])
                    .unwrap();
            }
            let expected = transcription.pcm();
            assert_eq!(&std::fs::read(&path).unwrap()[44..], expected);
            let result = backend.stop_dictation_session(session).await;
            assert_eq!(result.is_err(), fail);
            let history = backend.list_history().unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].id, session.to_string());
            assert_eq!(history[0].has_audio_recording, Some(fail));
            if fail {
                assert_eq!(history[0].error_code.as_deref(), Some("transcribeFailed"));
                assert_eq!(&std::fs::read(path).unwrap()[44..], expected);
            } else {
                assert_eq!(history[0].final_text, "complete transcription");
                assert!(!path.exists());
            }
            backend.shutdown().await.unwrap();
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }

    #[tokio::test]
    async fn dictation_freezes_channel_identity_protocol_and_model_for_the_session() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-channel-snapshot-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "polished");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let first_asr = backend
            .create_channel(
                ChannelKind::Asr,
                "openai-compatible".to_string(),
                "ASR first".to_string(),
            )
            .await
            .unwrap();
        let second_asr = backend
            .create_channel(
                ChannelKind::Asr,
                "openai-compatible".to_string(),
                "ASR second".to_string(),
            )
            .await
            .unwrap();
        let first_llm = backend
            .create_channel(
                ChannelKind::Llm,
                "deepseek".to_string(),
                "LLM first".to_string(),
            )
            .await
            .unwrap();
        let second_llm = backend
            .create_channel(
                ChannelKind::Llm,
                "deepseek".to_string(),
                "LLM second".to_string(),
            )
            .await
            .unwrap();
        for (namespace, provider_id, account, model) in [
            (
                crate::credentials::CredentialNamespace::Asr,
                first_asr.clone(),
                "asr.model",
                "asr-model-first",
            ),
            (
                crate::credentials::CredentialNamespace::Asr,
                second_asr.clone(),
                "asr.model",
                "asr-model-second",
            ),
            (
                crate::credentials::CredentialNamespace::Llm,
                first_llm.clone(),
                "ark.model_id",
                "llm-model-first",
            ),
            (
                crate::credentials::CredentialNamespace::Llm,
                second_llm.clone(),
                "ark.model_id",
                "llm-model-second",
            ),
        ] {
            backend
                .set_credential(
                    CredentialKey::new(namespace, Some(provider_id), account).unwrap(),
                    SecretValue::new(model),
                )
                .await
                .unwrap();
        }
        backend
            .set_active_provider(ProviderSlot::Asr, first_asr.clone())
            .await
            .unwrap();
        backend
            .set_active_provider(ProviderSlot::Llm, first_llm.clone())
            .await
            .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        backend
            .set_active_provider(ProviderSlot::Asr, second_asr)
            .await
            .unwrap();
        backend
            .set_active_provider(ProviderSlot::Llm, second_llm)
            .await
            .unwrap();
        backend.stop_dictation().await.unwrap();

        let contexts = engine.contexts();
        let context = &contexts[0];
        assert_eq!(context.asr.provider_id, first_asr);
        assert_eq!(context.asr.provider_type, "openai-compatible");
        assert_eq!(context.asr.model.as_deref(), Some("asr-model-first"));
        assert_eq!(context.llm.provider_id, first_llm);
        assert_eq!(context.llm.provider_type, "deepseek");
        assert_eq!(context.llm.model.as_deref(), Some("llm-model-first"));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn completed_dictation_persists_history_and_activity_from_the_session_snapshot() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-completed-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful_with_metadata(
            "raw voice",
            "translated output",
            Some("polished source".to_string()),
            1250,
        );
        // FixedClock stamps history/activity; retain_with_policy still uses wall-clock
        // Utc::now(). Keep the stamp within the last day so a 30-day retention window
        // never prunes the just-written entry as the calendar advances.
        use chrono::Timelike;
        let stamp = (chrono::Utc::now() - chrono::Duration::days(1))
            .with_nanosecond(0)
            .expect("truncate sub-second");
        let fixed_clock = Arc::new(crate::testing::FixedClock::new(stamp, stamp.date_naive()));
        let backend = OpenLessBackend::new_with_clock(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
            fixed_clock,
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        // This fixed-clock test checks snapshot persistence, not wall-clock retention.
        // Retention is covered independently by history::tests.
        preferences.history_retention_days = 0;
        preferences.history_max_entries = Some(20);
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();

        let session_id = backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: true,
                front_app: Some("Visual Studio Code".to_string()),
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        let result = backend.stop_dictation().await.unwrap();

        assert_eq!(result.session_id, session_id);
        assert_eq!(result.polish_source.as_deref(), Some("polished source"));
        assert_eq!(result.duration_ms, 1250);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        let entry = &history[0];
        assert_eq!(entry.id, session_id.to_string());
        assert_eq!(entry.created_at, stamp.to_rfc3339());
        assert_eq!(entry.raw_transcript, "raw voice");
        assert_eq!(entry.final_text, "translated output");
        assert_eq!(entry.polish_source.as_deref(), Some("polished source"));
        assert!(entry.translation_active);
        assert_eq!(entry.duration_ms, Some(1250));
        assert_eq!(
            entry.insert_status,
            crate::types::HistoryInsertStatus::Inserted
        );
        let activity = backend.list_activity().unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].date, stamp.date_naive().to_string());
        assert_eq!(
            activity[0].chars,
            "translated output".chars().count() as u64
        );
        assert_eq!(activity[0].duration_ms, 1250);

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn completed_dictation_applies_enabled_correction_rules_before_insert_and_history() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-completed-correction-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let inserter = crate::testing::FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(inserter.clone()),
                dictation_engine: Arc::new(crate::testing::FixtureDictationEngine::successful(
                    "10粒样品和禁用词",
                    "10粒样品和禁用词",
                )),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend
            .add_correction_rule("{num}粒".to_string(), "{num}例".to_string())
            .unwrap();
        let disabled = backend
            .add_correction_rule("禁用词".to_string(), "不应出现".to_string())
            .unwrap();
        backend
            .set_correction_rule_enabled(&disabled.id, false)
            .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        let result = backend.stop_dictation().await.unwrap();

        assert_eq!(result.polished_text, "10例样品和禁用词");
        assert!(inserter.actions().iter().any(|action| matches!(
            action,
            crate::testing::FixtureInsertionAction::Insert { text, .. }
                if text == "10例样品和禁用词"
        )));
        assert_eq!(
            backend.list_history().unwrap()[0].final_text,
            result.polished_text
        );

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn pipeline_passes_corrected_asr_text_to_the_polisher_and_preserves_original_history() {
        let data_dir = TestDataDir::new("correction-before-polish");
        let polisher = Arc::new(crate::testing::FixtureTextPolisher::successful("10例"));
        let engine = crate::PipelineDictationEngine::new(
            Arc::new(crate::testing::FixtureAudioRecorder::new(
                vec![vec![1, 0, 2, 0]],
                vec![],
            )),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "10粒", 10,
            )),
            polisher.clone(),
        );
        let backend =
            backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine));
        backend
            .add_correction_rule("{num}粒".to_string(), "{num}例".to_string())
            .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        assert_eq!(polisher.inputs(), vec!["10例"]);
        let history = backend.list_history().unwrap();
        assert_eq!(history[0].raw_transcript, "10例");
        assert_eq!(history[0].asr_transcript.as_deref(), Some("10粒"));
    }

    #[tokio::test]
    async fn multimodal_history_attributes_success_to_the_frozen_omni_provider() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-omni-success-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::dictation_engine::PipelineDictationEngine::new(
            Arc::new(crate::testing::FixtureAudioRecorder::new(
                vec![vec![1, 0, 2, 0]],
                vec![(40, 0.25)],
            )),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "omni raw", 40,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::successful(
                "omni final",
            )),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        backend.set_preferences(preferences).unwrap();
        backend
            .set_active_provider(ProviderSlot::Omni, "omni-channel".to_string())
            .await
            .unwrap();
        backend
            .set_credential(
                CredentialKey::new(
                    crate::credentials::CredentialNamespace::Omni,
                    None,
                    "omni.model",
                )
                .unwrap(),
                SecretValue::new("omni-model"),
            )
            .await
            .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        let history = backend.list_history().unwrap();
        let entry = &history[0];
        assert_eq!(entry.pipeline_mode.as_deref(), Some("multimodal"));
        assert_eq!(entry.asr_provider, None);
        assert_eq!(entry.asr_model, None);
        assert_eq!(entry.asr_ms, None);
        assert_eq!(entry.llm_provider.as_deref(), Some("omni-channel"));
        assert_eq!(entry.llm_model.as_deref(), Some("omni-model"));
        assert!(entry.polish_ms.is_some());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn multimodal_history_attributes_failure_to_the_frozen_omni_provider() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-omni-failure-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(PolishMetadataFailingEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        backend.set_preferences(preferences).unwrap();
        backend
            .set_active_provider(ProviderSlot::Omni, "omni-failure-channel".to_string())
            .await
            .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap_err();

        let history = backend.list_history().unwrap();
        let entry = &history[0];
        assert_eq!(entry.pipeline_mode.as_deref(), Some("multimodal"));
        assert_eq!(entry.asr_provider, None);
        assert_eq!(entry.asr_model, None);
        assert_eq!(entry.asr_ms, None);
        assert_eq!(entry.llm_provider.as_deref(), Some("omni-failure-channel"));
        assert_eq!(entry.polish_ms, Some(600));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn polish_fallback_persists_the_polish_failed_history_code() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-polish-fallback-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::dictation_engine::PipelineDictationEngine::new(
            Arc::new(
                crate::testing::FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![(25, 0.5)])
                    .with_archived_recording(true),
            ),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "raw fallback",
                25,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::failing(
                BackendError::new(BackendErrorCode::Provider, "fixture polish failure"),
            )),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        // Configure the default LLM channel so the polish call is actually dispatched
        // and fails; an unconfigured channel now runs as Raw (issue #1032).
        backend
            .set_credential(
                CredentialKey::new(
                    crate::credentials::CredentialNamespace::Llm,
                    Some("ark".to_string()),
                    crate::credentials::LLM_API_KEY_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("fixture-key"),
            )
            .await
            .unwrap();

        backend.start_dictation().await.unwrap();
        let result = backend.stop_dictation().await.unwrap();

        assert_eq!(result.polished_text, "raw fallback");
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code.as_deref(), Some("polishFailed"));
        assert!(history[0].asr_ms.is_some());
        assert!(history[0].polish_ms.is_some());
        assert_eq!(history[0].has_audio_recording, Some(false));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn unconfigured_llm_runs_as_raw_without_polish_failure() {
        // Issue #1032: with no LLM API key the default Light mode must not dispatch
        // polish (which would fail and record polishFailed); the dictation runs as Raw.
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-unconfigured-llm-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::dictation_engine::PipelineDictationEngine::new(
            Arc::new(
                crate::testing::FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![(25, 0.5)])
                    .with_archived_recording(true),
            ),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "raw fallback",
                25,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::failing(
                BackendError::new(BackendErrorCode::Provider, "polish must not be called"),
            )),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        let result = backend.stop_dictation().await.unwrap();

        assert_eq!(result.polished_text, "raw fallback");
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code, None);
        assert_eq!(history[0].polish_ms, None);

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn short_polish_history_matches_the_executed_path() {
        #[derive(Clone, Copy)]
        struct Case {
            raw: &'static str,
            threshold: u32,
            translation: bool,
            insert_failed: bool,
            correction: Option<(&'static str, &'static str)>,
            polish_failed: bool,
            uses_polisher: bool,
        }
        let short = Case {
            raw: "确定",
            threshold: 8,
            translation: false,
            insert_failed: false,
            correction: None,
            polish_failed: false,
            uses_polisher: false,
        };
        let long = "确定重试确定重试好";
        let cases = [
            ("short", short),
            (
                "insert-failed",
                Case {
                    insert_failed: true,
                    ..short
                },
            ),
            (
                "long",
                Case {
                    raw: long,
                    uses_polisher: true,
                    ..short
                },
            ),
            (
                "disabled",
                Case {
                    threshold: 0,
                    uses_polisher: true,
                    ..short
                },
            ),
            (
                "translation",
                Case {
                    translation: true,
                    uses_polisher: true,
                    ..short
                },
            ),
            (
                "expanded",
                Case {
                    correction: Some(("确定", long)),
                    uses_polisher: true,
                    ..short
                },
            ),
            (
                "shortened",
                Case {
                    raw: long,
                    correction: Some((long, "确定")),
                    ..short
                },
            ),
            (
                "polish-failed",
                Case {
                    raw: long,
                    polish_failed: true,
                    uses_polisher: true,
                    ..short
                },
            ),
        ];
        for (name, case) in cases {
            let data_dir = TestDataDir::new(name);
            let polisher = Arc::new(if case.polish_failed {
                crate::testing::FixtureTextPolisher::failing(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture polish failure",
                ))
            } else {
                crate::testing::FixtureTextPolisher::successful("polished text")
            });
            let engine = crate::PipelineDictationEngine::new(
                Arc::new(crate::testing::FixtureAudioRecorder::new(
                    vec![vec![1, 0, 2, 0]],
                    vec![(25, 0.5)],
                )),
                Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                    case.raw, 25,
                )),
                polisher.clone(),
            );
            let mut backend =
                backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(engine));
            if case.insert_failed {
                backend.deps.text_inserter =
                    Arc::new(crate::testing::FixtureTextInserter::failing(
                        BackendError::new(BackendErrorCode::Platform, "fixture insert failure"),
                    ));
            }
            backend
                .set_credential(
                    CredentialKey::new(
                        crate::credentials::CredentialNamespace::Llm,
                        Some("ark".to_string()),
                        crate::credentials::LLM_MODEL_ACCOUNT,
                    )
                    .unwrap(),
                    SecretValue::new("fixture-model"),
                )
                .await
                .unwrap();
            let mut preferences = backend.get_preferences();
            preferences.polish_skip_max_chars = case.threshold;
            preferences.streaming_insert = false;
            preferences.translation_target_language = "English".to_string();
            preferences.working_languages = vec!["简体中文".to_string(), "English".to_string()];
            backend.set_preferences(preferences).unwrap();
            if let Some((from, to)) = case.correction {
                backend
                    .add_correction_rule(from.to_string(), to.to_string())
                    .unwrap();
            }
            backend.start().await.unwrap();
            backend
                .activate_style_pack(crate::style_packs::BUILTIN_STYLE_PACK_LIGHT_ID)
                .unwrap();
            backend
                .start_dictation_with_options(DictationStartOptions {
                    translation_requested: case.translation,
                    ..Default::default()
                })
                .await
                .unwrap();
            let result = backend.stop_dictation().await;
            assert_eq!(result.is_err(), case.insert_failed, "{name}");
            let history = backend.list_history().unwrap();
            assert_eq!(history.len(), 1, "{name}");
            let entry = &history[0];
            assert_eq!(
                polisher.inputs().len(),
                if case.uses_polisher { 1 } else { 0 },
                "{name}"
            );
            assert_eq!(
                entry.llm_provider.as_deref(),
                case.uses_polisher.then_some("ark"),
                "{name}"
            );
            assert_eq!(
                entry.llm_model.as_deref(),
                case.uses_polisher.then_some("fixture-model"),
                "{name}"
            );
            assert_eq!(entry.polish_ms.is_some(), case.uses_polisher, "{name}");
            assert_eq!(entry.mode, crate::types::PolishMode::Light, "{name}");
            assert_eq!(entry.translation_active, case.translation, "{name}");
            let expected_error = if case.insert_failed {
                Some("insertFailed")
            } else if case.polish_failed {
                Some("polishFailed")
            } else {
                None
            };
            assert_eq!(entry.error_code.as_deref(), expected_error, "{name}");
            if !case.uses_polisher {
                assert_eq!(
                    entry.final_text,
                    case.correction.map_or(case.raw, |(_, to)| to),
                    "{name}"
                );
            }
        }
    }

    #[tokio::test]
    async fn empty_transcript_persists_a_failed_history_entry_without_activity() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-empty-transcript-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::dictation_engine::PipelineDictationEngine::new(
            Arc::new(crate::testing::FixtureAudioRecorder::new(
                vec![vec![1, 0, 2, 0]],
                vec![(40, 0.25)],
            )),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "   ", 40,
            )),
            Arc::new(crate::testing::FixtureTextPolisher::successful(
                "must not be inserted",
            )),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let session_id = backend.start_dictation().await.unwrap();
        let error = backend
            .stop_dictation()
            .await
            .expect_err("empty transcript must fail");

        assert_eq!(error.code, BackendErrorCode::Provider);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, session_id.to_string());
        assert_eq!(history[0].error_code.as_deref(), Some("emptyTranscript"));
        assert_eq!(
            history[0].insert_status,
            crate::types::HistoryInsertStatus::Failed
        );
        assert!(backend.list_activity().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn asr_finish_failure_persists_history_and_releases_the_session() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-asr-failure-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(crate::testing::FixtureDictationEngine::failing(
                    BackendError::new(BackendErrorCode::Provider, "fixture ASR failure"),
                )),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let session_id = backend.start_dictation().await.unwrap();
        backend
            .engine_progress_sink()
            .publish(
                session_id,
                EngineProgress::TranscriptDelta(crate::TranscriptDelta {
                    text: "draft before provider failure".into(),
                    offset: 0,
                    is_final: false,
                }),
            )
            .unwrap();
        let error = backend.stop_dictation().await.expect_err("ASR must fail");

        assert_eq!(error.code, BackendErrorCode::Provider);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, session_id.to_string());
        assert_eq!(history[0].error_code.as_deref(), Some("transcribeFailed"));
        assert_eq!(history[0].raw_transcript, "draft before provider failure");
        assert!(history[0].final_text.is_empty());
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert!(backend.start_dictation().await.is_ok());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn raw_style_runs_through_the_real_pipeline_without_a_polishing_stage() {
        let data_dir = TestDataDir::new("raw-real-pipeline");
        let polisher = crate::testing::FixtureTextPolisher::successful("must not run");
        let recorder = Arc::new(crate::testing::FixtureAudioRecorder::new(
            vec![vec![1, 0]],
            Vec::new(),
        ));
        let engine = Arc::new(crate::PipelineDictationEngine::new(
            recorder.clone(),
            Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                "raw words",
                80,
            )),
            Arc::new(polisher.clone()),
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..Default::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: engine,
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        backend.activate_style_pack("builtin.raw").unwrap();
        backend.start().await.unwrap();
        // Exercise the production backend progress validator, not the loose
        // sink used by engine-only tests; Raw never enters the LLM stage.
        for _ in 0..2 {
            backend.start_dictation().await.unwrap();
            let result = backend
                .stop_dictation()
                .await
                .expect("Raw ASR must remain usable");
            assert_eq!(result.raw_text, "raw words");
            assert_eq!(result.polished_text, "raw words");
            assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        }
        assert!(polisher.inputs().is_empty(), "Raw must not call the LLM");
        assert_eq!(recorder.stop_count(), 2);
        assert!(backend
            .list_history()
            .unwrap()
            .iter()
            .all(|entry| entry.error_code.is_none()));
        backend.shutdown().await.unwrap();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn unconfigured_cloud_capture_switches_only_with_an_installed_local_model() {
        let data_dir = TestDataDir::new("unconfigured-cloud-local-switch");
        let mut backend =
            backend_with_dictation_engine(data_dir.path().to_path_buf(), Arc::new(FakeEngine));
        backend.deps.services.model_store = Some(Arc::new(
            crate::ModelStore::new(
                crate::ModelStoreConfig::new(data_dir.path().join("models")).unwrap(),
            )
            .unwrap(),
        ));
        let channel = backend
            .create_channel(ChannelKind::Asr, "whisper".into(), "Cloud ASR".into())
            .await
            .unwrap();
        backend
            .set_active_provider(ProviderSlot::Asr, channel.clone())
            .await
            .unwrap();
        let options = DictationStartOptions::default();
        let capture =
            || backend.capture_dictation_context(&options, DictationContextPurpose::Dictation);
        assert_eq!(capture().await.unwrap().asr.provider_type, "whisper");
        let target = crate::LocalAsrTarget::parse(
            crate::LocalAsrRuntime::SherpaOnnx,
            "sense-voice-small-zh",
        )
        .unwrap();
        let dir = backend
            .deps
            .services
            .model_store
            .as_ref()
            .unwrap()
            .model_dir(&target)
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for (file, bytes) in [
            (crate::MODEL_READY_SENTINEL, "ready"),
            ("model.int8.onnx", "model"),
            ("tokens.txt", "tokens"),
        ] {
            std::fs::write(dir.join(file), bytes).unwrap();
        }
        let local = capture().await.unwrap();
        assert_eq!(local.asr.provider_type, "sherpa-onnx-local");
        assert_eq!(local.asr.model.as_deref(), Some("sense-voice-small-zh"));
        backend
            .deps
            .credential_store
            .write(
                crate::CredentialKey::new(
                    crate::CredentialNamespace::Asr,
                    Some(channel),
                    "asr.api_key",
                )
                .unwrap(),
                crate::SecretValue::new("fixture-key"),
            )
            .await
            .unwrap();
        assert_eq!(capture().await.unwrap().asr.provider_type, "whisper");
    }

    #[tokio::test]
    async fn local_recognition_with_a_disabled_llm_dictates_raw_text() {
        let data_dir = TestDataDir::new("local-disabled-llm");
        let polisher = Arc::new(crate::testing::FixtureTextPolisher::successful(
            "must not run",
        ));
        let backend = backend_with_dictation_engine(
            data_dir.path().to_path_buf(),
            Arc::new(crate::PipelineDictationEngine::new(
                Arc::new(crate::testing::FixtureAudioRecorder::new(
                    vec![vec![1, 0]],
                    Vec::new(),
                )),
                Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                    "raw words",
                    80,
                )),
                polisher.clone(),
            )),
        );
        let llm = backend
            .create_channel(
                ChannelKind::Llm,
                "ark".to_string(),
                "Unused LLM".to_string(),
            )
            .await
            .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.active_llm_provider = llm.clone();
        preferences.active_asr_provider = "sherpa-onnx-local".to_string();
        backend.set_preferences(preferences).unwrap();
        backend
            .set_active_provider(ProviderSlot::Llm, llm.clone())
            .await
            .unwrap();
        backend
            .set_channel_enabled(ChannelKind::Llm, llm, false)
            .await
            .unwrap();
        // A polishing style: with cloud ASR this start would be refused.
        backend.activate_style_pack("builtin.light").unwrap();
        backend.start().await.unwrap();

        backend
            .start_dictation()
            .await
            .expect("local recognition dictates raw text without a polish channel");
        let result = backend.stop_dictation().await.unwrap();

        assert_eq!(result.polished_text, "raw words");
        assert!(polisher.inputs().is_empty());
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn raw_dictation_remains_usable_when_the_last_llm_channel_is_disabled() {
        let data_dir = TestDataDir::new("raw-disabled-llm");
        let polisher = Arc::new(crate::testing::FixtureTextPolisher::successful(
            "must not run",
        ));
        let backend = backend_with_dictation_engine(
            data_dir.path().to_path_buf(),
            Arc::new(crate::PipelineDictationEngine::new(
                Arc::new(crate::testing::FixtureAudioRecorder::new(
                    vec![vec![1, 0]],
                    Vec::new(),
                )),
                Arc::new(crate::testing::FixtureTranscriptionEngine::successful(
                    "raw words",
                    80,
                )),
                polisher.clone(),
            )),
        );
        let llm = backend
            .create_channel(
                ChannelKind::Llm,
                "ark".to_string(),
                "Unused LLM".to_string(),
            )
            .await
            .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.active_llm_provider = llm.clone();
        // Cloud ASR: with local recognition a disabled LLM means raw output
        // instead (local_recognition_with_a_disabled_llm_dictates_raw_text).
        preferences.active_asr_provider = "volcengine".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        preferences.translation_target_language = "English".to_string();
        backend.set_preferences(preferences).unwrap();
        backend
            .set_active_provider(ProviderSlot::Llm, llm.clone())
            .await
            .unwrap();
        backend
            .set_channel_enabled(ChannelKind::Llm, llm.clone(), false)
            .await
            .unwrap();
        backend.activate_style_pack("builtin.raw").unwrap();
        backend.start().await.unwrap();

        backend
            .start_dictation()
            .await
            .expect("Raw does not require an enabled LLM channel");
        let result = backend.stop_dictation().await.unwrap();
        assert_eq!(result.polished_text, "raw words");

        // A late translation gesture may use only the start-time channel
        // snapshot, not the channel the user enables during this recording.
        backend.start_dictation().await.unwrap();
        backend
            .set_channel_enabled(ChannelKind::Llm, llm.clone(), true)
            .await
            .unwrap();
        let error = backend
            .stop_dictation_with_options(DictationStopOptions {
                translation_requested: Some(true),
                ..DictationStopOptions::default()
            })
            .await
            .expect_err("translation must retain the unavailable LLM snapshot");
        assert_eq!(error.code, BackendErrorCode::InvalidState);
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);

        // Unlike Raw, a normal polish route still rejects a disabled LLM.
        backend
            .set_channel_enabled(ChannelKind::Llm, llm, false)
            .await
            .unwrap();
        backend.activate_style_pack("builtin.light").unwrap();
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::InvalidState
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert!(polisher.inputs().is_empty());
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn omni_dictation_ignores_disabled_traditional_channels() {
        let data_dir = TestDataDir::new("omni-disabled-traditional-channels");
        let backend = backend_with_dictation_engine(
            data_dir.path().to_path_buf(),
            Arc::new(crate::testing::FixtureDictationEngine::successful(
                "omni raw",
                "omni final",
            )),
        );
        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        for (kind, slot, provider_type) in [
            (ChannelKind::Asr, ProviderSlot::Asr, "volcengine"),
            (ChannelKind::Llm, ProviderSlot::Llm, "ark"),
        ] {
            let id = backend
                .create_channel(kind, provider_type.to_string(), "Unused".to_string())
                .await
                .unwrap();
            match slot {
                ProviderSlot::Asr => preferences.active_asr_provider = id.clone(),
                ProviderSlot::Llm => preferences.active_llm_provider = id.clone(),
                ProviderSlot::Omni => unreachable!(),
            }
            backend.set_active_provider(slot, id.clone()).await.unwrap();
            backend.set_channel_enabled(kind, id, false).await.unwrap();
        }
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();

        backend
            .start_dictation()
            .await
            .expect("Omni does not require traditional ASR or LLM channels");
        let result = backend.stop_dictation().await.unwrap();
        assert_eq!(result.polished_text, "omni final");

        let mut preferences = backend.get_preferences();
        preferences.multimodal_pipeline_enabled = false;
        backend.set_preferences(preferences).unwrap();
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::InvalidState
        );
        assert_eq!(
            backend
                .capture_host_dictation_context(DictationStartOptions::default())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );
        let llm = backend
            .list_channels(ChannelKind::Llm)
            .await
            .unwrap()
            .remove(0)
            .id;
        backend
            .set_channel_enabled(ChannelKind::Llm, llm.clone(), true)
            .await
            .unwrap();
        backend
            .set_active_provider(ProviderSlot::Llm, llm)
            .await
            .unwrap();
        let qa_text = backend
            .capture_host_dictation_context(DictationStartOptions::default())
            .await
            .expect("text QA needs its LLM but not the disabled ASR channel");
        assert_eq!(qa_text.llm.provider_type, "ark");
        backend.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn pipeline_asr_failure_preserves_archive_and_timing_diagnostics() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-asr-diagnostics-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::dictation_engine::PipelineDictationEngine::new(
            Arc::new(
                crate::testing::FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![(80, 0.5)])
                    .with_archived_recording(true),
            ),
            Arc::new(crate::testing::FixtureTranscriptionEngine::failing(
                BackendError::new(BackendErrorCode::Provider, "fixture ASR failure"),
            )),
            Arc::new(crate::testing::FixtureTextPolisher::successful("unused")),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                    InsertOutcome::Inserted,
                )),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.expect_err("ASR must fail");

        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code.as_deref(), Some("transcribeFailed"));
        assert_eq!(history[0].has_audio_recording, Some(true));
        assert!(history[0].asr_ms.is_some());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn engine_start_failure_persists_history_and_releases_the_session() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-start-failure-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let inserter = crate::testing::FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(inserter.clone()),
                dictation_engine: Arc::new(StartFailingEngine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let error = backend
            .start_dictation()
            .await
            .expect_err("engine start must fail");

        assert_eq!(error.code, BackendErrorCode::Platform);
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error_code.as_deref(), Some("transcribeFailed"));
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        let actions = inserter.actions();
        assert_eq!(actions.len(), 2);
        let prepared_session = match &actions[0] {
            crate::testing::FixtureInsertionAction::Prepare(session_id) => *session_id,
            action => panic!("unexpected first insertion action: {action:?}"),
        };
        assert_eq!(
            actions[1],
            crate::testing::FixtureInsertionAction::Cancel(prepared_session)
        );
        assert!(backend.start_dictation().await.is_err());
        assert_eq!(backend.list_history().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn insertion_failure_persists_generated_text_and_releases_the_session() {
        crate::testing::logging::start();
        let data_dir = std::env::temp_dir().join(format!(
            "openless-core-insert-failure-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(crate::testing::FixtureTextInserter::failing(
                    BackendError::new(BackendErrorCode::Platform, "fixture insertion failure"),
                )),
                dictation_engine: Arc::new(
                    crate::testing::FixtureDictationEngine::successful_with_metadata(
                        "raw voice",
                        "generated text",
                        Some("polished source".to_string()),
                        600,
                    ),
                ),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let session_id = backend.start_dictation().await.unwrap();
        let error = backend
            .stop_dictation()
            .await
            .expect_err("insertion must fail");

        assert_eq!(error.code, BackendErrorCode::Platform);
        let logs = crate::testing::logging::matching(&format!(
            "[insert-diagnostics] session_id={session_id} "
        ));
        assert!(logs
            .iter()
            .any(|line| line.contains("attempt=1 operation=insert_final result=Err(Platform)")));
        assert!(!logs
            .iter()
            .any(|line| line.contains("generated text")
                || line.contains("fixture insertion failure")));
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, session_id.to_string());
        assert_eq!(history[0].raw_transcript, "raw voice");
        assert_eq!(history[0].final_text, "generated text");
        assert_eq!(history[0].polish_source.as_deref(), Some("polished source"));
        assert_eq!(history[0].error_code.as_deref(), Some("insertFailed"));
        assert_eq!(
            history[0].insert_status,
            crate::types::HistoryInsertStatus::Failed
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert!(backend.list_activity().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn dictation_runs_through_engine_inserter_and_host_actions() {
        let (backend, host) = backend();
        let mut events = backend.subscribe();
        backend.start().await.unwrap();
        let session = backend.start_dictation().await.unwrap();
        assert_eq!(
            backend.stop_dictation().await.unwrap().polished_text,
            "polished"
        );
        let mut emitted = Vec::new();
        while let Ok(event) = events.try_recv() {
            emitted.push(event);
        }
        assert!(matches!(emitted[0].kind, BackendEventKind::BackendStarted));
        assert_eq!(
            emitted
                .iter()
                .filter(|event| event.session_id == Some(session))
                .filter_map(|event| match &event.kind {
                    BackendEventKind::DictationStateChanged(snapshot) => Some(snapshot.phase),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![
                DictationPhase::Starting,
                DictationPhase::Recording,
                DictationPhase::Transcribing,
                DictationPhase::Inserting,
                DictationPhase::Completed,
            ]
        );
        assert!(emitted.iter().any(|event| {
            event.session_id == Some(session)
                && matches!(event.kind, BackendEventKind::DictationCompleted(_))
        }));
        let actions = host.0.lock().unwrap();
        assert_eq!(
            actions.as_slice(),
            &[
                HostAction::ShowDictationFeedback,
                HostAction::HideDictationFeedback
            ]
        );
    }

    #[tokio::test]
    async fn final_insertion_uses_polished_text_not_raw_transcript() {
        use crate::testing::{FixtureDictationEngine, FixtureInsertionAction, FixtureTextInserter};

        let data_dir = TestDataDir::new("polished-final-insertion");
        let inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let engine = FixtureDictationEngine::successful("raw transcript", "polished final");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(inserter.clone()),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let session_id = backend.start_dictation().await.unwrap();

        let result = backend.stop_dictation().await.unwrap();
        assert_eq!(result.raw_text, "raw transcript");
        assert_eq!(result.polished_text, "polished final");
        assert_eq!(
            inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(session_id),
                FixtureInsertionAction::Insert {
                    session_id,
                    text: "polished final".into(),
                },
            ]
        );
    }

    async fn assert_streaming_polish_deltas_flush_before_final_insert(translation: bool) {
        use crate::testing::{FixtureDictationEngine, FixtureInsertionAction, FixtureTextInserter};

        let data_dir = TestDataDir::new("streaming-insert");
        let (first, second) = if translation {
            ("Hello", " 🌍")
        } else {
            ("你", "好")
        };
        let output = format!("{first}{second}");
        let engine = FixtureDictationEngine::successful("raw", &output).with_polish_deltas(vec![
            crate::types::PolishDelta {
                text: first.into(),
                offset: 0,
                is_final: false,
            },
            crate::types::PolishDelta {
                text: second.into(),
                offset: first.chars().count() as u64,
                is_final: false,
            },
        ]);
        let inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(inserter.clone()),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: llm_configured_credentials(),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.streaming_insert = true;
        preferences.streaming_insert_save_clipboard = false;
        preferences.windows_insertion_mode = crate::shared_types::WindowsInsertionMode::SendInput;
        preferences.translation_target_language = "English".into();
        preferences.working_languages = vec!["简体中文".into()];
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let mut events = backend.subscribe();
        let session_id = backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: translation,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        assert_eq!(backend.snapshot().dictation.translation_active, translation);
        assert!(!std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event.kind, BackendEventKind::PolishDelta(_))));
        backend.stop_dictation().await.unwrap();
        let deltas: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event.kind {
                BackendEventKind::PolishDelta(delta) => Some(delta),
                _ => None,
            })
            .collect();
        assert_eq!(deltas.len(), 3);
        assert!(!deltas[0].is_final && !deltas[1].is_final);
        assert_eq!(format!("{}{}", deltas[0].text, deltas[1].text), output);
        assert!(deltas[2].is_final);
        assert_eq!(deltas[2].text, output);
        let history = backend.list_history().unwrap();
        assert_eq!(history[0].final_text, output);
        assert_eq!(history[0].translation_active, translation);

        assert_eq!(
            inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(session_id),
                FixtureInsertionAction::Write {
                    session_id,
                    text: output.clone(),
                },
                FixtureInsertionAction::Insert {
                    session_id,
                    text: String::new(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn streaming_polish_deltas_flush_before_the_final_insert() {
        assert_streaming_polish_deltas_flush_before_final_insert(false).await;
    }

    #[tokio::test]
    async fn streaming_translation_deltas_flush_without_duplicate_final_text() {
        assert_streaming_polish_deltas_flush_before_final_insert(true).await;
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum NativeInsertionEnd {
        Complete,
        Cancel,
        DropStopAndCancel,
    }

    async fn assert_native_insertion_lifecycle(
        streaming: bool,
        ending: NativeInsertionEnd,
        translation: bool,
    ) {
        crate::testing::logging::start();
        struct BlockingInsertion {
            actions: Arc<Mutex<Vec<&'static str>>>,
            started: Arc<tokio::sync::Semaphore>,
            release: Arc<tokio::sync::Semaphore>,
        }
        impl TextInsertionSession for BlockingInsertion {
            fn write(
                &self,
                text: String,
            ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
                let actions = Arc::clone(&self.actions);
                let started = Arc::clone(&self.started);
                let release = Arc::clone(&self.release);
                boxed(async move {
                    actions.lock().unwrap().push("write started");
                    started.add_permits(1);
                    release.acquire().await.unwrap().forget();
                    actions.lock().unwrap().push("write finished");
                    Ok(InsertWriteResult {
                        written_chars: text.chars().count(),
                    })
                })
            }
            fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
            fn finish(
                &self,
                text: String,
            ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
                let writing = (!text.is_empty()).then(|| self.write(text));
                let actions = Arc::clone(&self.actions);
                boxed(async move {
                    if let Some(writing) = writing {
                        writing.await?;
                    }
                    actions.lock().unwrap().push("input source restored");
                    Ok(InsertOutcome::Inserted)
                })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                self.actions.lock().unwrap().push("input source restored");
                boxed(async { Ok(()) })
            }
        }
        struct BlockingInserter(Arc<BlockingInsertion>);
        impl TextInserter for BlockingInserter {
            fn begin(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
            ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>>
            {
                let insertion: Arc<dyn TextInsertionSession> = self.0.clone();
                boxed(async move { Ok(insertion) })
            }
        }

        let actions = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let data_dir = TestDataDir::new("native-insertion-lifecycle");
        let prefix = "长文字🙂\r\n".repeat(256);
        let output = format!("{prefix}{}", "等待最后一段🌍\n".repeat(256));
        let host = Arc::new(FakeHost::default());
        let mut deps = BackendDependencies::unsupported();
        deps.host_actions = host.clone();
        deps.credential_store = llm_configured_credentials();
        deps.text_inserter = Arc::new(BlockingInserter(Arc::new(BlockingInsertion {
            actions: Arc::clone(&actions),
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })));
        deps.dictation_engine = Arc::new(
            crate::testing::FixtureDictationEngine::successful("raw", &output).with_polish_deltas(
                if streaming {
                    vec![crate::types::PolishDelta {
                        text: prefix,
                        offset: 0,
                        is_final: false,
                    }]
                } else {
                    Vec::new()
                },
            ),
        );
        deps.task_spawner = Arc::new(TokioTaskSpawner);
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                deps,
            )
            .unwrap(),
        );
        let mut preferences = backend.get_preferences();
        preferences.streaming_insert = streaming;
        preferences.windows_insertion_mode = crate::shared_types::WindowsInsertionMode::SendInput;
        preferences.translation_target_language = "English".into();
        preferences.working_languages = vec!["简体中文".into()];
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let mut events = backend.subscribe();
        let session_id = backend
            .start_dictation_with_options(DictationStartOptions {
                translation_requested: translation,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        let stopping_backend = Arc::clone(&backend);
        let stop = tokio::spawn(async move { stopping_backend.stop_dictation().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), started.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        if ending == NativeInsertionEnd::Complete {
            // Hold both the streamed prefix and the final reconciliation tail.
            // An LLM final delta must not end feedback before either native write.
            for write_index in 0..if streaming { 2 } else { 1 } {
                if write_index > 0 {
                    tokio::time::timeout(std::time::Duration::from_secs(2), started.acquire())
                        .await
                        .unwrap()
                        .unwrap()
                        .forget();
                }
                assert!(!stop.is_finished());
                assert_eq!(
                    backend.snapshot().dictation.phase,
                    DictationPhase::Inserting
                );
                assert!(!host
                    .0
                    .lock()
                    .unwrap()
                    .contains(&HostAction::HideDictationFeedback));
                while let Ok(event) = events.try_recv() {
                    assert!(!matches!(
                        event.kind,
                        BackendEventKind::DictationCompleted(_)
                            | BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                                phase: DictationPhase::Completed | DictationPhase::Idle,
                                ..
                            })
                    ));
                }
                release.add_permits(1);
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), stop)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(result.polished_text, output);
            assert_eq!(
                actions.lock().unwrap().last(),
                Some(&"input source restored")
            );
            assert_eq!(
                std::iter::from_fn(|| events.try_recv().ok())
                    .filter(|event| matches!(event.kind, BackendEventKind::DictationCompleted(_)))
                    .count(),
                1
            );
            assert!(host
                .0
                .lock()
                .unwrap()
                .contains(&HostAction::HideDictationFeedback));
            return;
        }
        let drop_stop = ending == NativeInsertionEnd::DropStopAndCancel;
        if drop_stop {
            stop.abort();
            tokio::task::yield_now().await;
        }

        // The write represents a native, non-interruptible CGEvent/SendInput
        // chunk. Cancelling may discard queued text but must not restore TIS or
        // admit another voice session before this in-flight effect has drained.
        let mut cancel = std::pin::pin!(backend.cancel_dictation(Some(session_id)));
        assert!(futures_util::poll!(cancel.as_mut()).is_pending());
        assert_eq!(*actions.lock().unwrap(), vec!["write started"]);
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(2), cancel)
            .await
            .expect("native cleanup must outlive a dropped stop caller")
            .unwrap();
        if drop_stop {
            assert!(stop.await.unwrap_err().is_cancelled());
        } else {
            assert_eq!(
                stop.await.unwrap().unwrap_err().code,
                BackendErrorCode::Cancelled
            );
        }
        assert_eq!(
            *actions.lock().unwrap(),
            vec!["write started", "write finished", "input source restored"]
        );
        if streaming {
            let logs = crate::testing::logging::matching(&format!(
                "[insert-diagnostics] session_id={session_id} "
            ));
            assert!(logs
                .iter()
                .any(|line| line.contains("attempt=1 operation=stream result=Ok(")));
            assert!(logs
                .iter()
                .any(|line| line.contains("operation=cancel attempts=1 result=Ok(())")));
        }
        let next = backend.start_dictation().await.unwrap();
        backend.cancel_dictation(Some(next)).await.unwrap();
    }

    #[tokio::test]
    async fn streaming_cancel_drains_native_write_before_restoring_and_releasing_voice() {
        assert_native_insertion_lifecycle(true, NativeInsertionEnd::Cancel, false).await;
    }

    #[tokio::test]
    async fn final_insert_cancel_waits_for_the_committed_native_effect() {
        assert_native_insertion_lifecycle(false, NativeInsertionEnd::Cancel, false).await;
    }

    #[tokio::test]
    async fn final_insert_cancellation_survives_a_dropped_stop_caller() {
        assert_native_insertion_lifecycle(false, NativeInsertionEnd::DropStopAndCancel, false)
            .await;
    }

    #[tokio::test]
    async fn streaming_translation_cancel_drains_native_write() {
        assert_native_insertion_lifecycle(true, NativeInsertionEnd::Cancel, true).await;
    }

    #[tokio::test]
    async fn long_text_feedback_waits_for_stream_and_final_tail() {
        assert_native_insertion_lifecycle(true, NativeInsertionEnd::Complete, false).await;
    }

    #[tokio::test]
    async fn translation_feedback_waits_for_stream_and_final_tail() {
        assert_native_insertion_lifecycle(true, NativeInsertionEnd::Complete, true).await;
    }

    #[tokio::test]
    async fn non_streaming_feedback_waits_for_native_completion() {
        assert_native_insertion_lifecycle(false, NativeInsertionEnd::Complete, false).await;
    }

    #[tokio::test]
    async fn streaming_final_writes_only_the_missing_tail() {
        crate::testing::logging::start();
        use crate::testing::{FixtureDictationEngine, FixtureInsertionAction, FixtureTextInserter};

        let data_dir = TestDataDir::new("streaming-final-tail");
        let engine = FixtureDictationEngine::successful("raw", "你好").with_polish_deltas(vec![
            crate::types::PolishDelta {
                text: "你".into(),
                offset: 0,
                is_final: false,
            },
        ]);
        let inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(inserter.clone()),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: llm_configured_credentials(),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut preferences = backend.get_preferences();
        preferences.streaming_insert = true;
        preferences.streaming_insert_save_clipboard = false;
        preferences.windows_insertion_mode = crate::shared_types::WindowsInsertionMode::SendInput;
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let session_id = backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        assert_eq!(
            inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(session_id),
                FixtureInsertionAction::Write {
                    session_id,
                    text: "你".into(),
                },
                FixtureInsertionAction::Write {
                    session_id,
                    text: "好".into(),
                },
                FixtureInsertionAction::Insert {
                    session_id,
                    text: String::new(),
                },
            ]
        );
        let logs = crate::testing::logging::matching(&format!(
            "[insert-diagnostics] session_id={session_id} "
        ));
        for operation in [
            "attempt=1 operation=stream chars=1",
            "attempt=2 operation=tail chars=1",
            "attempt=3 operation=finalize chars=0",
        ] {
            assert!(
                logs.iter().any(|line| line.contains(operation)),
                "missing {operation}: {logs:?}"
            );
        }
        assert_eq!(
            logs.iter().filter(|line| line.contains(" result=")).count(),
            3
        );
        assert!(!logs
            .iter()
            .any(|line| line.contains('你') || line.contains('好')));
    }

    #[tokio::test]
    async fn native_streaming_unavailable_keeps_one_final_delivery() {
        crate::testing::logging::start();
        struct FinalOnlySession(Arc<Mutex<Vec<String>>>);
        impl TextInsertionSession for FinalOnlySession {
            fn supports_streaming(&self) -> bool {
                false
            }
            fn write(
                &self,
                _: String,
            ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
                panic!("an unavailable native stream must never receive a chunk");
            }
            fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
                panic!("native preparation failure still allows final insertion");
            }
            fn finish(
                &self,
                text: String,
            ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
                self.0.lock().unwrap().push(text);
                boxed(async { Ok(InsertOutcome::Inserted) })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
        }
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let mut context = DictationContext::default();
        context.insertion.streaming = true;
        context.insertion.windows_insertion_mode =
            crate::shared_types::WindowsInsertionMode::SendInput;
        let gate = Arc::new(crate::voice_session::VoiceSessionGate::default());
        let session_id = SessionId::new();
        gate.acquire(
            session_id,
            crate::voice_session::VoiceSessionKind::Dictation,
        )
        .unwrap();
        let insertion = ActiveTextInsertion::new(
            session_id,
            Arc::new(FinalOnlySession(delivered.clone())),
            &context,
            Arc::new(TokioTaskSpawner),
            gate.hold_resources(session_id).unwrap(),
        );
        insertion.push(&crate::types::PolishDelta {
            offset: 0,
            text: "partial".into(),
            is_final: false,
        });
        assert_eq!(
            insertion.finish("final text".into()).await.unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(*delivered.lock().unwrap(), ["final text"]);
        let logs = crate::testing::logging::matching(&format!(
            "[insert-diagnostics] session_id={session_id} "
        ));
        assert_eq!(logs.len(), 2);
        assert!(logs[0].contains("attempt=1 operation=insert_final chars=10"));
        assert!(logs[1].contains("attempt=1 operation=insert_final result=Ok(Inserted)"));
    }

    #[tokio::test]
    async fn native_terminal_errors_retain_cleanup_only_on_windows() {
        struct FailedTerminalSession(Arc<AtomicBool>);
        impl TextInsertionSession for FailedTerminalSession {
            fn write(
                &self,
                _: String,
            ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
                panic!("a final-only session must not stream text");
            }
            fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
            fn finish(&self, _: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
                boxed(async {
                    Err(BackendError::new(
                        BackendErrorCode::Platform,
                        "native delivery failed",
                    ))
                })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                let fails = self.0.load(Ordering::Acquire);
                boxed(async move {
                    if fails {
                        Err(BackendError::new(
                            BackendErrorCode::Platform,
                            "cached terminal error",
                        ))
                    } else {
                        Ok(())
                    }
                })
            }
        }
        struct FailedTerminalInserter(Arc<AtomicBool>);
        impl TextInserter for FailedTerminalInserter {
            fn begin(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
            ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>>
            {
                let session: Arc<dyn TextInsertionSession> =
                    Arc::new(FailedTerminalSession(Arc::clone(&self.0)));
                boxed(async { Ok(session) })
            }
        }

        for finish in [true, false] {
            let data_dir = TestDataDir::new("native-terminal-error");
            let cancel_fails = Arc::new(AtomicBool::new(true));
            let mut deps = BackendDependencies::unsupported();
            deps.host_actions = Arc::new(FakeHost::default());
            deps.credential_store = llm_configured_credentials();
            deps.text_inserter = Arc::new(FailedTerminalInserter(Arc::clone(&cancel_fails)));
            deps.dictation_engine = Arc::new(crate::testing::FixtureDictationEngine::successful(
                "raw", "final",
            ));
            deps.task_spawner = Arc::new(TokioTaskSpawner);
            let backend = OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                deps,
            )
            .unwrap();
            backend.start().await.unwrap();
            let session_id = backend.start_dictation().await.unwrap();
            let error = if finish {
                backend.stop_dictation().await.unwrap_err()
            } else {
                backend
                    .cancel_dictation(Some(session_id))
                    .await
                    .unwrap_err()
            };
            assert_eq!(error.code, BackendErrorCode::Platform);
            assert_eq!(
                backend
                    .text_insertions
                    .lock()
                    .unwrap()
                    .contains_key(&session_id),
                cfg!(target_os = "windows"),
            );
            if cfg!(target_os = "windows") {
                assert_eq!(
                    backend.start_dictation().await.unwrap_err().code,
                    BackendErrorCode::Busy
                );
            }
            cancel_fails.store(false, Ordering::Release);
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while backend
                    .text_insertions
                    .lock()
                    .unwrap()
                    .contains_key(&session_id)
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("native cleanup must release the prior voice session");
            let next = backend.start_dictation().await.unwrap();
            backend.cancel_dictation(Some(next)).await.unwrap();
        }
    }

    struct CompositionRecorder(Arc<Mutex<Vec<String>>>);
    impl TextInsertionSession for CompositionRecorder {
        fn supports_streaming(&self) -> bool {
            false
        }
        fn write(&self, _: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
            panic!("composition display must never stream chunks");
        }
        fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
        fn update_composition(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>> {
            self.0.lock().unwrap().push(format!("update:{text}"));
            boxed(async { Ok(()) })
        }
        fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
            self.0.lock().unwrap().push(format!("finish:{text}"));
            boxed(async { Ok(InsertOutcome::Inserted) })
        }
        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            boxed(async { Ok(()) })
        }
    }

    fn composition_test_insertion(
        context: &DictationContext,
        log: Arc<Mutex<Vec<String>>>,
    ) -> Arc<ActiveTextInsertion> {
        let gate = Arc::new(crate::voice_session::VoiceSessionGate::default());
        let session_id = SessionId::new();
        gate.acquire(
            session_id,
            crate::voice_session::VoiceSessionKind::Dictation,
        )
        .unwrap();
        ActiveTextInsertion::new(
            session_id,
            Arc::new(CompositionRecorder(log)),
            context,
            Arc::new(TokioTaskSpawner),
            gate.hold_resources(session_id).unwrap(),
        )
    }

    #[tokio::test]
    async fn composition_partials_are_replaced_by_the_final_text() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut context = DictationContext::default();
        context.insertion.composition_input = true;
        let insertion = composition_test_insertion(&context, log.clone());
        insertion.push_partial("你".into());
        insertion.push_partial("你好".into());
        assert_eq!(
            insertion.finish("你好。".into()).await.unwrap(),
            InsertOutcome::Inserted
        );
        let entries = log.lock().unwrap().clone();
        // Partials may be coalesced or dropped at close, but the final text always comes last
        // and never before an update that was already in flight.
        assert_eq!(entries.last().map(String::as_str), Some("finish:你好。"));
        assert!(entries[..entries.len() - 1]
            .iter()
            .all(|entry| entry == "update:你" || entry == "update:你好"));
    }

    #[tokio::test]
    async fn composition_finalization_uses_polished_text_not_raw_transcript() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut context = DictationContext::default();
        context.insertion.composition_input = true;
        let insertion = composition_test_insertion(&context, log.clone());
        insertion.push_partial("raw transcript".into());

        assert_eq!(
            insertion.finish("polished final".into()).await.unwrap(),
            InsertOutcome::Inserted
        );
        let entries = log.lock().unwrap().clone();
        assert_eq!(
            entries.last().map(String::as_str),
            Some("finish:polished final")
        );
        assert!(!entries.iter().any(|entry| entry == "finish:raw transcript"));
    }

    #[tokio::test]
    async fn partial_transcripts_stay_local_unless_composition_is_enabled() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let context = DictationContext::default();
        let insertion = composition_test_insertion(&context, log.clone());
        insertion.push_partial("你好".into());
        assert_eq!(
            insertion.finish("你好。".into()).await.unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(*log.lock().unwrap(), ["finish:你好。"]);
    }

    #[tokio::test]
    async fn composition_input_disables_streaming_polish_chunks() {
        struct StreamingProbe;
        impl TextInsertionSession for StreamingProbe {
            fn supports_streaming(&self) -> bool {
                true
            }
            fn write(
                &self,
                _: String,
            ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
                panic!("composition sessions must not stream polish chunks");
            }
            fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
            fn update_composition(
                &self,
                _: String,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
            fn finish(&self, _: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
                boxed(async { Ok(InsertOutcome::Inserted) })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
        }

        let mut context = DictationContext::default();
        context.insertion.composition_input = true;
        context.insertion.streaming = true;
        // SendInput would otherwise allow streaming; composition must still win.
        context.insertion.windows_insertion_mode =
            crate::shared_types::WindowsInsertionMode::SendInput;
        context.polish.mode = crate::types::PolishMode::Formal;
        let gate = Arc::new(crate::voice_session::VoiceSessionGate::default());
        let session_id = SessionId::new();
        gate.acquire(
            session_id,
            crate::voice_session::VoiceSessionKind::Dictation,
        )
        .unwrap();
        let insertion = ActiveTextInsertion::new(
            session_id,
            Arc::new(StreamingProbe),
            &context,
            Arc::new(TokioTaskSpawner),
            gate.hold_resources(session_id).unwrap(),
        );
        assert!(context.uses_llm_polisher());
        insertion.push(&crate::types::PolishDelta {
            text: "should not type".into(),
            offset: 0,
            is_final: false,
        });
        assert_eq!(
            insertion.finish("final".into()).await.unwrap(),
            InsertOutcome::Inserted
        );
    }

    #[tokio::test]
    async fn context_capture_is_already_a_cancellable_dictation_session() {
        struct SlowContext {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
        }
        impl HostContextAdapter for SlowContext {
            fn capture(
                &self,
                include_cursor: bool,
            ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
                let entered = self.entered.clone();
                let release = self.release.clone();
                boxed(async move {
                    if include_cursor {
                        entered.notify_one();
                        release.notified().await;
                    }
                    Ok(HostContextCapture::default())
                })
            }
        }
        let data_dir = TestDataDir::new("cancel-context-capture");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut services = crate::domains::BackendServices::unsupported();
        services.host_context = Arc::new(SlowContext {
            entered: entered.clone(),
            release: release.clone(),
        });
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: Arc::new(FakeHost::default()),
                    text_inserter: Arc::new(FakeInserter),
                    dictation_engine: Arc::new(FakeEngine),
                    services,
                    ..BackendDependencies::unsupported()
                },
            )
            .unwrap(),
        );
        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = true;
        backend.set_preferences(preferences).unwrap();
        backend.start().await.unwrap();
        let start = tokio::spawn({
            let backend = backend.clone();
            async move { backend.start_dictation().await }
        });
        entered.notified().await;
        let cancelled = backend.cancel_active_voice_session(None).await;
        release.notify_one();
        let result = start.await.unwrap();
        assert!(
            cancelled.is_ok(),
            "Esc must own the session even while AX/credentials are being captured"
        );
        assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        let mut preferences = backend.get_preferences();
        preferences.cursor_context_enabled = false;
        backend.set_preferences(preferences).unwrap();
        backend.start_dictation().await.unwrap();
        backend.cancel_dictation(None).await.unwrap();
    }

    #[tokio::test]
    async fn cli_dictation_intents_use_the_same_core_state_machine() {
        let (backend, _) = backend();
        backend.start().await.unwrap();

        let started = backend
            .dispatch_cli_intent(crate::cli::CliIntent::ToggleDictation)
            .await
            .unwrap();
        let session_id = match started {
            CliDispatchOutcome::DictationStarted(session_id) => session_id,
            other => panic!("unexpected start outcome: {other:?}"),
        };
        assert_eq!(
            backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );

        let completed = backend
            .dispatch_cli_intent(crate::cli::CliIntent::ToggleDictation)
            .await
            .unwrap();
        assert!(matches!(
            completed,
            CliDispatchOutcome::DictationCompleted(DictationResult {
                session_id: completed_session,
                ..
            }) if completed_session == session_id
        ));
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);

        assert_eq!(
            backend
                .dispatch_cli_intent(crate::cli::CliIntent::CancelDictation)
                .await
                .unwrap(),
            CliDispatchOutcome::Noop
        );
    }

    #[tokio::test]
    async fn cancelling_wrong_session_is_rejected_without_mutating_state() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let active = backend.start_dictation().await.unwrap();
        let wrong = SessionId::new();
        let error = backend.cancel_dictation(Some(wrong)).await.unwrap_err();
        assert_eq!(error.code, BackendErrorCode::InvalidArgument);
        assert_eq!(backend.snapshot().dictation.session_id, Some(active));
    }

    #[test]
    fn take_splash_playback_marks_major_once_and_survives_restart() {
        let data_dir = TestDataDir::new("splash-playback");
        let make = || {
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies::unsupported(),
            )
            .unwrap()
        };

        let backend = make();
        // First launch: marker missing -> play once and write back the major.
        assert!(backend.take_splash_playback("2"));
        assert_eq!(backend.get_preferences().splash_seen_version, "2");
        // A restart within the same generation doesn't replay.
        assert!(!backend.take_splash_playback("2"));

        // Simulated process restart: the marker was read back from preferences.json.
        let reopened = make();
        assert!(!reopened.take_splash_playback("2"));
        // A new major generation: plays the new PV once, then settles the same way.
        assert!(reopened.take_splash_playback("3"));
        assert!(!reopened.take_splash_playback("3"));
        assert_eq!(reopened.get_preferences().splash_seen_version, "3");
    }

    #[tokio::test]
    async fn engine_failure_publishes_failed_state_and_preserves_session_identity() {
        let host = Arc::new(FakeHost::default());
        let data_dir = TestDataDir::new("engine-failure");
        let engine = Arc::new(FailingEngine(std::sync::atomic::AtomicUsize::new(0)));
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: host,
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: engine.clone(),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();

        let mut events = backend.subscribe();
        backend.start().await.unwrap();
        let session = backend.start_dictation().await.unwrap();
        let error = backend.stop_dictation().await.unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Provider);
        let snapshot = backend.snapshot();
        assert_eq!(snapshot.dictation.session_id, None);
        assert_eq!(snapshot.dictation.phase, DictationPhase::Idle);
        let mut failed_session = None;
        while let Ok(event) = events.try_recv() {
            if let BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                session_id,
                phase: DictationPhase::Failed,
                ..
            }) = event.kind
            {
                failed_session = session_id;
            }
        }
        assert_eq!(failed_session, Some(session));
        assert_eq!(
            engine.0.load(Ordering::Acquire),
            1,
            "finish errors must cancel remaining engine resources"
        );
    }

    #[tokio::test]
    async fn stop_is_rejected_after_the_session_has_reached_idle() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        let error = backend.stop_dictation().await.unwrap_err();
        assert_eq!(error.code, BackendErrorCode::InvalidState);
    }

    #[tokio::test]
    async fn clipboard_fallback_is_an_explicit_event() {
        for outcome in [InsertOutcome::CopiedFallback] {
            let host = Arc::new(FakeHost::default());
            let data_dir = TestDataDir::new("insert-outcome");
            let backend = OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: host,
                    text_inserter: Arc::new(crate::testing::FixtureTextInserter::with_outcome(
                        outcome,
                    )),
                    dictation_engine: Arc::new(FakeEngine),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    credential_store: Arc::new(
                        crate::credentials::InMemoryCredentialStore::default(),
                    ),
                    services: crate::domains::BackendServices::unsupported(),
                    local_asr_runtime: None,
                    marketplace_config: None,
                    selection_runtime: None,
                    selection_polisher: None,
                    qa_runtime: None,
                },
            )
            .unwrap();
            let mut events = backend.subscribe();
            backend.start().await.unwrap();
            backend.start_dictation().await.unwrap();
            let result = backend.stop_dictation().await.unwrap();
            assert_eq!(result.inserted, outcome.into_status());

            let mut fallback_payload = None;
            loop {
                match events.try_recv() {
                    Ok(event) => {
                        if let BackendEventKind::InsertFallback(payload) = event.kind {
                            fallback_payload = Some(payload);
                        }
                    }
                    Err(crate::events::EventRecvError::Empty) => break,
                    Err(error) => panic!("unexpected event error: {error}"),
                }
            }
            assert_eq!(
                fallback_payload
                    .expect("fallback outcome must be visible to both hosts")
                    .copied_text
                    .as_deref(),
                Some("polished")
            );
        }
    }

    #[tokio::test]
    async fn cancellation_emits_cancelled_state_and_clears_the_session() {
        let (backend, host) = backend();
        let mut events = backend.subscribe();
        backend.start().await.unwrap();
        let session = backend.start_dictation().await.unwrap();
        backend.cancel_dictation(Some(session)).await.unwrap();

        assert_eq!(
            backend.snapshot().dictation,
            DictationStateSnapshot::default()
        );
        assert_eq!(
            *host.0.lock().unwrap(),
            vec![
                HostAction::ShowDictationFeedback,
                HostAction::HideDictationFeedback
            ]
        );
        let mut saw_cancelled = false;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event.kind,
                BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                    phase: DictationPhase::Cancelled,
                    ..
                })
            ) {
                saw_cancelled = true;
            }
        }
        assert!(saw_cancelled);
        assert_eq!(
            backend.stop_dictation().await.unwrap_err().code,
            BackendErrorCode::InvalidState
        );
    }

    #[tokio::test]
    async fn aborted_startup_removes_orphan_recording_draft() {
        struct DelayedStartEngine {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Semaphore>,
        }

        impl DictationEngine for DelayedStartEngine {
            fn start(
                &self,
                _session_id: SessionId,
                _context: Arc<DictationContext>,
                _progress: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                let entered = Arc::clone(&self.entered);
                let release = Arc::clone(&self.release);
                boxed(async move {
                    entered.notify_waiters();
                    release.acquire().await.unwrap().forget();
                    Ok(())
                })
            }

            fn finish(
                &self,
                _session_id: SessionId,
                _progress: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
                boxed(async {
                    Ok(EngineResult {
                        raw_text: String::new(),
                        asr_transcript: None,
                        polished_text: String::new(),
                        polish_source: None,
                        duration_ms: 0,
                        polish_failed: false,
                        asr_ms: None,
                        polish_ms: None,
                        has_audio_recording: None,
                        asr_call_label: None,
                        llm_call_label: None,
                    })
                })
            }

            fn cancel(
                &self,
                _session_id: SessionId,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
        }

        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let data_dir = TestDataDir::new("abort-startup-orphan-draft");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(DelayedStartEngine {
                    entered: Arc::clone(&entered),
                    release: Arc::clone(&release),
                }),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                task_spawner: Arc::new(TokioTaskSpawner),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let mut starting = Box::pin(backend.start_dictation_with_options(DictationStartOptions {
            output_target: DictationOutputTarget::Undecided,
            insert_text: false,
            ..DictationStartOptions::default()
        }));
        // Drive the starter until the provisional recording draft is written and
        // the delayed engine is waiting inside start().
        let wait_entered = entered.notified();
        tokio::pin!(wait_entered);
        tokio::select! {
            biased;
            _ = &mut wait_entered => {},
            _ = &mut starting => panic!("start settled before delayed engine entered"),
        }
        let draft = backend
            .list_history()
            .unwrap()
            .into_iter()
            .find(|entry| entry.error_code.as_deref() == Some("recording"));
        assert!(
            draft.is_some(),
            "undecided startup must persist a provisional recording draft"
        );

        // Shutdown settles provisional drafts immediately; the abandoned starter
        // must still observe Cancelled without leaving a recording row behind.
        backend.shutdown().await.unwrap();
        release.add_permits(1);
        let error = starting.await.unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(
            backend
                .list_history()
                .unwrap()
                .into_iter()
                .all(|entry| entry.error_code.as_deref() != Some("recording")),
            "aborted startup must not leave an orphan recording draft"
        );
    }

    #[tokio::test]
    async fn shutdown_while_recording_settles_provisional_history() {
        let data_dir = TestDataDir::new("shutdown-while-recording-settles-draft");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(FakeEngine),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                task_spawner: Arc::new(TokioTaskSpawner),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let _undecided = backend
            .start_dictation_with_options(DictationStartOptions {
                output_target: DictationOutputTarget::Undecided,
                insert_text: false,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        assert_eq!(
            backend.list_history().unwrap()[0].error_code.as_deref(),
            Some("recording")
        );
        backend.shutdown().await.unwrap();
        assert!(
            backend.list_history().unwrap().is_empty(),
            "shutdown must drop undecided recording drafts"
        );

        backend.start().await.unwrap();
        let _quick_note = backend
            .start_dictation_with_options(DictationStartOptions {
                output_target: DictationOutputTarget::QuickNote,
                insert_text: false,
                ..DictationStartOptions::default()
            })
            .await
            .unwrap();
        let history_before = backend.list_history().unwrap();
        assert_eq!(history_before.len(), 1);
        assert_eq!(history_before[0].error_code.as_deref(), Some("recording"));
        let quick_note_id = history_before[0].id.clone();
        backend.shutdown().await.unwrap();
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, quick_note_id);
        assert_eq!(history[0].error_code.as_deref(), Some("cancelled"));
        assert_eq!(history[0].source, HistorySource::QuickNote);
    }

    #[tokio::test]
    async fn quick_note_cancel_during_startup_preserves_cancelled_history() {
        struct DelayedStartEngine {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Semaphore>,
        }

        impl DictationEngine for DelayedStartEngine {
            fn start(
                &self,
                _session_id: SessionId,
                _context: Arc<DictationContext>,
                _progress: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                let entered = Arc::clone(&self.entered);
                let release = Arc::clone(&self.release);
                boxed(async move {
                    entered.notify_waiters();
                    release.acquire().await.unwrap().forget();
                    Ok(())
                })
            }

            fn finish(
                &self,
                _session_id: SessionId,
                _progress: Arc<dyn EngineProgressSink>,
            ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
                boxed(async {
                    Ok(EngineResult {
                        raw_text: String::new(),
                        asr_transcript: None,
                        polished_text: String::new(),
                        polish_source: None,
                        duration_ms: 0,
                        polish_failed: false,
                        asr_ms: None,
                        polish_ms: None,
                        has_audio_recording: None,
                        asr_call_label: None,
                        llm_call_label: None,
                    })
                })
            }

            fn cancel(
                &self,
                _session_id: SessionId,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { Ok(()) })
            }
        }

        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let data_dir = TestDataDir::new("quick-note-cancel-startup-preserve");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(DelayedStartEngine {
                    entered: Arc::clone(&entered),
                    release: Arc::clone(&release),
                }),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                task_spawner: Arc::new(TokioTaskSpawner),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        let mut starting = Box::pin(backend.start_dictation_with_options(DictationStartOptions {
            output_target: DictationOutputTarget::QuickNote,
            insert_text: false,
            ..DictationStartOptions::default()
        }));
        let wait_entered = entered.notified();
        tokio::pin!(wait_entered);
        tokio::select! {
            biased;
            _ = &mut wait_entered => {},
            _ = &mut starting => panic!("start settled before delayed engine entered"),
        }
        let session_id = backend
            .snapshot()
            .dictation
            .session_id
            .expect("active session");
        assert_eq!(
            backend.list_history().unwrap()[0].error_code.as_deref(),
            Some("recording")
        );

        let cancel = backend.cancel_dictation(Some(session_id));
        release.add_permits(1);
        let (start_result, cancel_result) = tokio::join!(starting, cancel);
        assert_eq!(start_result.unwrap_err().code, BackendErrorCode::Cancelled);
        cancel_result.unwrap();

        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, session_id.to_string());
        assert_eq!(history[0].error_code.as_deref(), Some("cancelled"));
        assert_eq!(history[0].source, HistorySource::QuickNote);
        assert_eq!(history[0].has_audio_recording, Some(true));
    }

    #[tokio::test]
    async fn engine_receives_start_finish_and_cancel_lifecycle_calls() {
        use crate::testing::{
            FixtureDictationEngine, FixtureEngineAction, FixtureInsertionAction,
            FixtureTextInserter,
        };

        let completed_engine = FixtureDictationEngine::successful("raw", "polished");
        let completed_inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let completed_data_dir = TestDataDir::new("engine-lifecycle-completed");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: completed_data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(completed_inserter.clone()),
                dictation_engine: Arc::new(completed_engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let completed = backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();
        assert_eq!(
            completed_engine.actions(),
            vec![
                FixtureEngineAction::Start(completed),
                FixtureEngineAction::Finish(completed),
            ]
        );
        assert_eq!(
            completed_inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(completed),
                FixtureInsertionAction::Insert {
                    session_id: completed,
                    text: "polished".to_string(),
                },
            ]
        );

        let cancelled_engine = FixtureDictationEngine::successful("raw", "polished");
        let cancelled_inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let cancelled_data_dir = TestDataDir::new("engine-lifecycle-cancelled");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: cancelled_data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(cancelled_inserter.clone()),
                dictation_engine: Arc::new(cancelled_engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let cancelled = backend.start_dictation().await.unwrap();
        backend.cancel_dictation(Some(cancelled)).await.unwrap();
        assert_eq!(
            cancelled_engine.actions(),
            vec![
                FixtureEngineAction::Start(cancelled),
                FixtureEngineAction::Cancel(cancelled),
            ]
        );
        assert_eq!(
            cancelled_inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(cancelled),
                FixtureInsertionAction::Cancel(cancelled),
            ]
        );

        let shutdown_engine = FixtureDictationEngine::successful("raw", "polished");
        let shutdown_inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
        let shutdown_data_dir = TestDataDir::new("engine-lifecycle-shutdown");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: shutdown_data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(shutdown_inserter.clone()),
                dictation_engine: Arc::new(shutdown_engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let interrupted = backend.start_dictation().await.unwrap();
        backend.shutdown().await.unwrap();
        assert_eq!(
            shutdown_engine.actions(),
            vec![
                FixtureEngineAction::Start(interrupted),
                FixtureEngineAction::Cancel(interrupted),
            ]
        );
        assert_eq!(
            shutdown_inserter.actions(),
            vec![
                FixtureInsertionAction::Prepare(interrupted),
                FixtureInsertionAction::Cancel(interrupted),
            ]
        );
    }

    #[tokio::test]
    async fn engine_progress_is_session_scoped_and_orders_stage_delta_and_terminal_events() {
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "polished");
        let data_dir = TestDataDir::new("engine-progress");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(engine),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        let mut events = backend.subscribe();
        backend.start().await.unwrap();
        let session = backend.start_dictation().await.unwrap();
        backend.stop_dictation().await.unwrap();

        let mut session_events = Vec::new();
        while let Ok(event) = events.try_recv() {
            if event.session_id == Some(session) {
                session_events.push(event);
            }
        }
        assert!(session_events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence));
        assert!(matches!(
            session_events[0].kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Starting,
                ..
            })
        ));
        assert!(session_events.iter().any(|event| matches!(
            event.kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Recording,
                ..
            })
        )));
        assert!(session_events.iter().any(|event| matches!(
            event.kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Transcribing,
                ..
            })
        )));
        assert!(session_events
            .iter()
            .any(|event| matches!(event.kind, BackendEventKind::TranscriptDelta(_))));
        assert!(session_events.iter().any(|event| matches!(
            event.kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Polishing,
                ..
            })
        )));
        assert!(session_events
            .iter()
            .any(|event| matches!(event.kind, BackendEventKind::PolishDelta(_))));
        assert!(session_events.iter().any(|event| matches!(
            event.kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Inserting,
                ..
            })
        )));
        assert!(matches!(
            session_events[session_events.len() - 2].kind,
            BackendEventKind::DictationCompleted(_)
        ));
        assert!(matches!(
            session_events.last().unwrap().kind,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Completed,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn late_engine_progress_is_rejected_after_session_cancellation() {
        let (backend, _) = backend();
        backend.start().await.unwrap();
        let session = backend.start_dictation().await.unwrap();
        let progress = BackendEngineProgress {
            events: Arc::clone(&backend.events),
            state: Arc::clone(&backend.state),
            phase_changed: Arc::clone(&backend.phase_changed),
            text_insertions: Arc::clone(&backend.text_insertions),
            raw_transcripts: Mutex::new(HashMap::new()),
        };
        backend.cancel_dictation(Some(session)).await.unwrap();

        let error = progress
            .publish(
                session,
                EngineProgress::TranscriptDelta(crate::types::TranscriptDelta {
                    text: "late".to_string(),
                    offset: 0,
                    is_final: false,
                }),
            )
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn stop_requested_while_engine_starts_waits_and_finishes_same_session() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let data_dir = TestDataDir::new("stop-during-start");
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: Arc::new(FakeHost::default()),
                    text_inserter: Arc::new(FakeInserter),
                    dictation_engine: Arc::new(BlockingStartEngine {
                        entered: Arc::clone(&entered),
                        release: Arc::clone(&release),
                    }),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    credential_store: Arc::new(
                        crate::credentials::InMemoryCredentialStore::default(),
                    ),
                    services: crate::domains::BackendServices::unsupported(),
                    local_asr_runtime: None,
                    marketplace_config: None,
                    selection_runtime: None,
                    selection_polisher: None,
                    qa_runtime: None,
                },
            )
            .unwrap(),
        );
        backend.start().await.unwrap();
        let starting_backend = Arc::clone(&backend);
        let start_task = tokio::spawn(async move { starting_backend.start_dictation().await });
        entered.notified().await;
        let expected_session = backend.snapshot().dictation.session_id.unwrap();

        let stopping_backend = Arc::clone(&backend);
        let stop_task = tokio::spawn(async move { stopping_backend.stop_dictation().await });
        tokio::task::yield_now().await;
        assert!(!stop_task.is_finished());

        release.notify_one();
        assert_eq!(start_task.await.unwrap().unwrap(), expected_session);
        let result = stop_task.await.unwrap().unwrap();
        assert_eq!(result.session_id, expected_session);
        assert_eq!(result.polished_text, "polished");
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    }

    #[tokio::test]
    async fn stop_during_provider_preparation_releases_capture_before_finalization() {
        use crate::ports::{
            ActiveRecording, AudioConsumer, AudioRecorder, PreparedTranscription, RecordingArchive,
            RecordingProgressSink, TextStreamSink, TranscriptionEngine, TranscriptionSession,
        };
        use crate::testing::{
            FixtureAudioRecorder, FixtureTextPolisher, FixtureTranscriptionEngine,
        };

        struct BlockedPreparation {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            transcriber: FixtureTranscriptionEngine,
        }
        impl TranscriptionEngine for BlockedPreparation {
            fn prepare(
                self: Arc<Self>,
                session_id: SessionId,
                context: Arc<DictationContext>,
            ) -> BoxFuture<'static, Result<Arc<dyn PreparedTranscription>, BackendError>>
            {
                Box::pin(async move {
                    self.entered.notify_one();
                    self.release.notified().await;
                    Arc::new(self.transcriber.clone())
                        .prepare(session_id, context)
                        .await
                })
            }
            fn start(
                &self,
                session_id: SessionId,
                context: Arc<DictationContext>,
                partials: Arc<dyn TextStreamSink>,
            ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>>
            {
                self.transcriber.start(session_id, context, partials)
            }
        }

        struct Recorder {
            inner: FixtureAudioRecorder,
            stop_fails: bool,
            archive: Arc<Mutex<Option<Arc<dyn RecordingArchive>>>>,
        }
        struct Recording {
            inner: Box<dyn ActiveRecording>,
            stop_fails: bool,
        }
        impl ActiveRecording for Recording {
            fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
                self.inner.archive()
            }
            fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
                let Recording { inner, stop_fails } = *self;
                let stopping = inner.stop();
                Box::pin(async move {
                    stopping.await?;
                    if stop_fails {
                        Err(BackendError::new(
                            BackendErrorCode::Platform,
                            "fixture stop failure",
                        ))
                    } else {
                        Ok(())
                    }
                })
            }
        }
        impl AudioRecorder for Recorder {
            fn start(
                &self,
                session_id: SessionId,
                context: Arc<DictationContext>,
                consumer: Arc<dyn AudioConsumer>,
                progress: Arc<dyn RecordingProgressSink>,
            ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
                let starting = self.inner.start(session_id, context, consumer, progress);
                let archive = self.archive.clone();
                let stop_fails = self.stop_fails;
                Box::pin(async move {
                    let recording = starting.await?;
                    *archive.lock().unwrap() = recording.archive();
                    Ok(Box::new(Recording {
                        inner: recording,
                        stop_fails,
                    }) as Box<dyn ActiveRecording>)
                })
            }
        }

        for stable in [false, true] {
            for publish_level in [false, true] {
                for (stop_before_start, drop_stop, stop_fails) in [
                    (false, false, false),
                    (true, false, false),
                    (true, true, false),
                    (false, false, true),
                    (true, false, true),
                ] {
                    let entered = Arc::new(tokio::sync::Notify::new());
                    let release = Arc::new(tokio::sync::Notify::new());
                    let recorder = FixtureAudioRecorder::new(
                        vec![vec![1, 0, 2, 0]],
                        if publish_level {
                            vec![(1, 0.1)]
                        } else {
                            vec![]
                        },
                    )
                    .with_archived_recording(true);
                    let archive = Arc::new(Mutex::new(None));
                    let transcriber = FixtureTranscriptionEngine::successful("captured prefix", 1);
                    let mut deps = BackendDependencies::unsupported();
                    deps.credential_store =
                        Arc::new(crate::credentials::InMemoryCredentialStore::default());
                    deps.dictation_engine = Arc::new(crate::DictationEngineRouter::new(Arc::new(
                        crate::PipelineDictationEngine::new(
                            Arc::new(Recorder {
                                inner: recorder.clone(),
                                stop_fails,
                                archive: archive.clone(),
                            }),
                            Arc::new(BlockedPreparation {
                                entered: entered.clone(),
                                release: release.clone(),
                                transcriber: transcriber.clone(),
                            }),
                            Arc::new(FixtureTextPolisher::successful("unused")),
                        ),
                    )));
                    let data_dir = TestDataDir::new("stop-during-provider-preparation");
                    let backend = Arc::new(
                        OpenLessBackend::new(
                            BackendConfig {
                                data_dir: data_dir.path().to_path_buf(),
                                ..Default::default()
                            },
                            deps,
                        )
                        .unwrap(),
                    );
                    backend.start().await.unwrap();
                    let mut prefs = backend.get_preferences();
                    prefs.stable_transcription_enabled = stable;
                    prefs.record_audio_for_debug = true;
                    backend.set_preferences(prefs).unwrap();
                    let options = DictationStartOptions {
                        insert_text: false,
                        ..Default::default()
                    };
                    let reservation = backend
                        .reserve_dictation_session(false, options.output_target, None)
                        .unwrap();
                    let session_id = reservation.session_id;
                    let mut stopping = Some(Box::pin(backend.stop_dictation()));
                    if stop_before_start {
                        assert!(
                            futures_util::poll!(stopping.as_mut().unwrap().as_mut()).is_pending()
                        );
                        tokio::task::yield_now().await;
                        assert!(
                            futures_util::poll!(stopping.as_mut().unwrap().as_mut()).is_pending()
                        );
                    }
                    if drop_stop {
                        drop(stopping.take());
                    }
                    let starting = tokio::spawn({
                        let backend = backend.clone();
                        async move { backend.start_reserved_dictation(reservation, options).await }
                    });
                    entered.notified().await;
                    let wait_for_stop = async {
                        while recorder.stop_count() == 0 {
                            if let Some(stopping) = stopping.as_mut() {
                                tokio::select! {
                                    result = stopping => panic!("finalized before provider preparation: {result:?}"),
                                    _ = tokio::task::yield_now() => {}
                                }
                            } else {
                                tokio::task::yield_now().await;
                            }
                        }
                    };
                    let stopped_before_prepare =
                        tokio::time::timeout(std::time::Duration::from_secs(2), wait_for_stop)
                            .await;
                    assert!(transcriber.pcm().is_empty());
                    release.notify_one();
                    assert_eq!(starting.await.unwrap().unwrap(), session_id);
                    let result = match stopping {
                        Some(stopping) => stopping.await,
                        None => backend.stop_dictation().await,
                    };
                    if stop_fails {
                        assert_eq!(result.unwrap_err().code, BackendErrorCode::Platform);
                        assert_eq!(
                            backend.list_history().unwrap()[0].error_code.as_deref(),
                            Some("transcribeFailed")
                        );
                    } else {
                        let result = result.unwrap();
                        assert_eq!(result.session_id, session_id);
                        assert_eq!(result.polished_text, "captured prefix");
                        assert_eq!(transcriber.pcm(), vec![1, 0, 2, 0]);
                    }
                    assert_eq!(recorder.stop_count(), 1);
                    assert!(archive.lock().unwrap().as_ref().unwrap().is_available());
                    assert_eq!(
                        backend.list_history().unwrap()[0].has_audio_recording,
                        Some(true)
                    );
                    assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
                    assert!(stopped_before_prepare.is_ok(), "Stop kept capture open: stable={stable}, level={publish_level}, early={stop_before_start}, dropped={drop_stop}, failed={stop_fails}");
                }
            }
        }
    }

    async fn assert_pending_insertion_cleanup(drop_start: bool) {
        struct DelayedSession {
            restoring: Arc<tokio::sync::Semaphore>,
            restored: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl TextInsertionSession for DelayedSession {
            fn write(
                &self,
                _: String,
            ) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
                boxed(async { unreachable!() })
            }
            fn finish(&self, _: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
                boxed(async { unreachable!() })
            }
            fn copy(&self, _: String) -> BoxFuture<'static, Result<(), BackendError>> {
                boxed(async { unreachable!() })
            }
            fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
                let restoring = self.restoring.clone();
                let restored = self.restored.clone();
                boxed(async move {
                    restoring.acquire().await.unwrap().forget();
                    restored.fetch_add(1, Ordering::AcqRel);
                    Ok(())
                })
            }
        }
        struct DelayedInserter {
            preparing: Arc<tokio::sync::Semaphore>,
            session: Arc<DelayedSession>,
        }
        impl TextInserter for DelayedInserter {
            fn begin(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
            ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>>
            {
                let preparing = self.preparing.clone();
                let session: Arc<dyn TextInsertionSession> = self.session.clone();
                boxed(async move {
                    preparing.acquire().await.unwrap().forget();
                    Ok(session)
                })
            }
        }
        let preparing = Arc::new(tokio::sync::Semaphore::new(0));
        let restoring = Arc::new(tokio::sync::Semaphore::new(0));
        let restored = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let data_dir = TestDataDir::new("cancel-during-insertion-prepare");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(DelayedInserter {
                    preparing: preparing.clone(),
                    session: Arc::new(DelayedSession {
                        restoring: restoring.clone(),
                        restored: restored.clone(),
                    }),
                }),
                dictation_engine: Arc::new(FakeEngine),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                task_spawner: Arc::new(TokioTaskSpawner),
                ..BackendDependencies::unsupported()
            },
        )
        .unwrap();
        backend.start().await.unwrap();
        let mut starting = Some(Box::pin(backend.start_dictation()));
        assert!(futures_util::poll!(starting.as_mut().unwrap().as_mut()).is_pending());
        let session_id = backend.snapshot().dictation.session_id.unwrap();
        let mut cancel = std::pin::pin!(backend.cancel_dictation(Some(session_id)));
        assert!(
            futures_util::poll!(cancel.as_mut()).is_pending(),
            "cancel must still own the pending native preparation"
        );
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        if drop_start {
            // Cancellation still owns the shared preparation even if the IPC
            // caller disappears; dropping its response cannot abandon TIS.
            drop(starting.take());
        }
        preparing.add_permits(1);
        // The late starter and explicit cancel both reach the same prepared
        // session. Exactly one restores it; the other must join that cleanup.
        if let Some(starting) = starting.as_mut() {
            assert!(futures_util::poll!(starting.as_mut()).is_pending());
        }
        assert!(futures_util::poll!(cancel.as_mut()).is_pending());
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        restoring.add_permits(1);
        if let Some(starting) = starting {
            let (start_result, cancel_result) = tokio::join!(starting, cancel);
            assert_eq!(start_result.unwrap_err().code, BackendErrorCode::Cancelled);
            cancel_result.unwrap();
        } else {
            cancel.await.unwrap();
        }
        assert_eq!(restored.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cancelling_pending_insertion_joins_preparation_and_native_restore() {
        assert_pending_insertion_cleanup(false).await;
    }

    #[tokio::test]
    async fn pending_insertion_cleanup_survives_a_dropped_start_caller() {
        assert_pending_insertion_cleanup(true).await;
    }

    #[tokio::test]
    async fn pending_stop_does_not_follow_a_replacement_dictation_session() {
        let release = Arc::new(tokio::sync::Notify::new());
        let data_dir = TestDataDir::new("stop-does-not-follow-replacement");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().to_path_buf(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(FakeHost::default()),
                text_inserter: Arc::new(FakeInserter),
                dictation_engine: Arc::new(BlockingStartEngine {
                    entered: Arc::new(tokio::sync::Notify::new()),
                    release: Arc::clone(&release),
                }),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(crate::credentials::InMemoryCredentialStore::default()),
                services: crate::domains::BackendServices::unsupported(),
                local_asr_runtime: None,
                voice_activation_runtime: None,
                marketplace_config: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
            },
        )
        .unwrap();
        backend.start().await.unwrap();

        // Poll the public requests explicitly: the old stop has already seen
        // Starting, but cannot resume until after cancellation and replacement.
        // This fixes the interleaving without relying on scheduler timing.
        let mut first_start = std::pin::pin!(backend.start_dictation());
        assert!(futures_util::poll!(first_start.as_mut()).is_pending());
        let first_session = backend.snapshot().dictation.session_id.unwrap();
        let mut pending_stop = std::pin::pin!(backend.stop_dictation());
        assert!(futures_util::poll!(pending_stop.as_mut()).is_pending());
        backend.cancel_dictation(Some(first_session)).await.unwrap();
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        // Native startup still owns resources after logical cancellation. Let
        // it clean up before B starts, while deliberately not polling A's old
        // stop request: that request must remain bound to A after B is ready.
        release.notify_one();
        assert_eq!(
            first_start.await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );

        let mut second_start = std::pin::pin!(backend.start_dictation());
        assert!(futures_util::poll!(second_start.as_mut()).is_pending());
        let second_session = backend.snapshot().dictation.session_id.unwrap();
        assert_ne!(first_session, second_session);
        // Owned startup may not have polled the engine yet. Keep the permit
        // instead of sending a broadcast which only wakes existing waiters.
        release.notify_one();
        assert_eq!(second_start.await.unwrap(), second_session);

        let error = pending_stop
            .await
            .expect_err("old stop must not finalize the new recording");
        assert_eq!(error.code, BackendErrorCode::InvalidState);
        assert_eq!(
            backend.snapshot().dictation.session_id,
            Some(second_session)
        );
        assert_eq!(
            backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );
        backend
            .cancel_dictation(Some(second_session))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_while_engine_starts_never_reenters_recording() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let data_dir = TestDataDir::new("cancel-during-start");
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.path().to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    voice_activation_runtime: None,
                    host_actions: Arc::new(FakeHost::default()),
                    text_inserter: Arc::new(FakeInserter),
                    dictation_engine: Arc::new(BlockingStartEngine {
                        entered: Arc::clone(&entered),
                        release: Arc::clone(&release),
                    }),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    credential_store: Arc::new(
                        crate::credentials::InMemoryCredentialStore::default(),
                    ),
                    services: crate::domains::BackendServices::unsupported(),
                    local_asr_runtime: None,
                    marketplace_config: None,
                    selection_runtime: None,
                    selection_polisher: None,
                    qa_runtime: None,
                },
            )
            .unwrap(),
        );
        backend.start().await.unwrap();
        let mut events = backend.subscribe();
        let starting_backend = Arc::clone(&backend);
        let start_task = tokio::spawn(async move { starting_backend.start_dictation().await });
        entered.notified().await;

        let session = backend.snapshot().dictation.session_id.unwrap();
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Starting);
        backend.cancel_dictation(Some(session)).await.unwrap();
        release.notify_one();
        assert_eq!(
            start_task.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);

        let mut saw_recording_after_cancel = false;
        let mut cancelled_sequence = None;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event.kind,
                BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                    phase: DictationPhase::Cancelled,
                    ..
                })
            ) {
                cancelled_sequence = Some(event.sequence);
            }
            if cancelled_sequence.is_some_and(|sequence| event.sequence > sequence)
                && matches!(
                    event.kind,
                    BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                        phase: DictationPhase::Recording,
                        ..
                    })
                )
            {
                saw_recording_after_cancel = true;
            }
        }
        assert!(!saw_recording_after_cancel);
    }

    #[tokio::test]
    async fn modifier_chord_companion_does_not_start_dictation_engine() {
        for mode in [
            crate::shared_types::HotkeyMode::Hold,
            crate::shared_types::HotkeyMode::Auto,
            crate::shared_types::HotkeyMode::Toggle,
        ] {
            let data_dir = TestDataDir::new("modifier-chord-companion");
            let engine = crate::testing::FixtureDictationEngine::successful("raw", "polished");
            let backend = backend_with_dictation_engine(
                data_dir.path().to_path_buf(),
                Arc::new(engine.clone()),
            );
            backend.start().await.unwrap();
            let mut preferences = backend.get_preferences();
            preferences.hotkey.mode = mode;
            preferences.dictation_hotkey = crate::shared_types::ShortcutBinding {
                primary: "ModifierChord".into(),
                modifiers: vec!["ctrl-left".into(), "cmd-left".into()],
            };
            backend.set_preferences(preferences).unwrap();

            let at = std::time::Instant::now();
            let mut press = std::pin::pin!(backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed { press_id: 1, at }));
            // Poll the real public API until it waits. A companion key arrives
            // before the grace expires, so even native startup must stay idle.
            assert!(futures_util::poll!(press.as_mut()).is_pending());
            assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
            assert!(engine.actions().is_empty());
            assert_eq!(
                backend
                    .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Combined {
                        press_id: 1,
                        at: at + std::time::Duration::from_millis(1),
                    })
                    .await
                    .unwrap(),
                CliDispatchOutcome::Noop
            );
            assert_eq!(press.await.unwrap(), CliDispatchOutcome::Noop);
            assert!(engine.actions().is_empty());

            // A later chord without a companion remains a working trigger.
            assert!(matches!(
                backend
                    .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                        press_id: 2,
                        at: at + std::time::Duration::from_secs(1),
                    })
                    .await
                    .unwrap(),
                CliDispatchOutcome::DictationStarted(_)
            ));
            backend.cancel_dictation(None).await.unwrap();
        }
    }

    #[tokio::test]
    async fn hotkey_combined_edge_cancels_the_same_generation_during_start_await() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let data_dir = TestDataDir::new("hotkey-combined-during-start");
        let backend = Arc::new(backend_with_dictation_engine(
            data_dir.path().to_path_buf(),
            Arc::new(BlockingStartEngine {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            }),
        ));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.hotkey.mode = crate::shared_types::HotkeyMode::Hold;
        // An explicit combo has no modifier-only ambiguity, so this test
        // isolates the start/Combined race instead of waiting for grace.
        preferences.dictation_hotkey = crate::shared_types::ShortcutBinding {
            primary: "F9".into(),
            modifiers: vec!["ctrl".into()],
        };
        backend.set_preferences(preferences).unwrap();

        let pressed_at = std::time::Instant::now();
        let starting_backend = Arc::clone(&backend);
        let start_task = tokio::spawn(async move {
            starting_backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                    press_id: 41,
                    at: pressed_at,
                })
                .await
        });
        entered.notified().await;

        // Combined bypasses the serialized Pressed/Released gate. The stable
        // press id lets it cancel only the session opened by this physical key.
        assert_eq!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Combined {
                    press_id: 41,
                    at: pressed_at + std::time::Duration::from_millis(1),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationCancelled
        );
        release.notify_one();
        assert_eq!(
            start_task.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    }

    #[tokio::test]
    async fn shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics() {
        let (backend, _) = backend();
        backend.start().await.unwrap();

        let mut preferences = backend.get_preferences();
        preferences.hotkey.mode = crate::shared_types::HotkeyMode::Hold;
        backend.set_preferences(preferences).unwrap();
        let pressed_at = std::time::Instant::now();
        assert!(matches!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                    press_id: 1,
                    at: pressed_at,
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationStarted(_)
        ));
        assert!(matches!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {
                    press_id: 1,
                    at: pressed_at + std::time::Duration::from_millis(50),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationCompleted(_)
        ));

        let mut preferences = backend.get_preferences();
        preferences.hotkey.mode = crate::shared_types::HotkeyMode::Auto;
        backend.set_preferences(preferences).unwrap();
        let short_press = std::time::Instant::now() + std::time::Duration::from_secs(1);
        backend
            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 2,
                at: short_press,
            })
            .await
            .unwrap();
        assert_eq!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {
                    press_id: 2,
                    at: short_press + std::time::Duration::from_millis(100),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::Noop
        );
        assert_eq!(
            backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );
        assert!(matches!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                    press_id: 3,
                    at: short_press + std::time::Duration::from_millis(500),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationCompleted(_)
        ));

        let long_press = short_press + std::time::Duration::from_secs(2);
        assert!(matches!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                    press_id: 4,
                    at: long_press,
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationStarted(_)
        ));
        assert!(matches!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released {
                    press_id: 4,
                    at: long_press + std::time::Duration::from_millis(500),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationCompleted(_)
        ));

        backend
            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 5,
                at: long_press + std::time::Duration::from_secs(2),
            })
            .await
            .unwrap();
        assert_eq!(
            backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Combined {
                    press_id: 5,
                    at: long_press + std::time::Duration::from_secs(2),
                })
                .await
                .unwrap(),
            CliDispatchOutcome::DictationCancelled
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    }

    #[tokio::test]
    async fn shared_toggle_hotkey_applies_stop_time_translation_options() {
        use crate::testing::FixtureEngineAction;

        let data_dir = std::env::temp_dir().join(format!(
            "openless-hotkey-stop-translation-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = crate::testing::FixtureDictationEngine::successful("raw", "translated");
        let backend = backend_with_dictation_engine(data_dir.clone(), Arc::new(engine.clone()));
        backend.start().await.unwrap();
        let mut preferences = backend.get_preferences();
        preferences.hotkey.mode = crate::shared_types::HotkeyMode::Toggle;
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        backend.set_preferences(preferences).unwrap();

        let pressed_at = std::time::Instant::now();
        let session_id = match backend
            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 1,
                at: pressed_at,
            })
            .await
            .unwrap()
        {
            CliDispatchOutcome::DictationStarted(session_id) => session_id,
            other => panic!("unexpected start outcome: {other:?}"),
        };
        let outcome = backend
            .dispatch_dictation_hotkey_edge_with_session_options(
                DictationHotkeyEdge::Pressed {
                    press_id: 2,
                    at: pressed_at + std::time::Duration::from_secs(1),
                },
                DictationHotkeyDispatchOptions {
                    start: DictationStartOptions::default(),
                    stop: DictationStopOptions {
                        translation_requested: Some(true),
                        ..DictationStopOptions::default()
                    },
                },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CliDispatchOutcome::DictationCompleted(_)));
        assert_eq!(
            engine.actions(),
            vec![
                FixtureEngineAction::Start(session_id),
                FixtureEngineAction::UpdateContext(session_id),
                FixtureEngineAction::Finish(session_id),
            ]
        );
        assert!(backend.list_history().unwrap()[0].translation_active);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }
    fn runtime_restore_backend() -> TestBackend {
        runtime_restore_backend_with_polisher(Arc::new(
            crate::testing::FixtureTextPolisher::successful("fixture"),
        ))
    }

    fn runtime_restore_backend_with_polisher(
        polisher: Arc<dyn crate::ports::TextPolisher>,
    ) -> TestBackend {
        let data_dir = TestDataDir::new("runtime-restore");
        let mut deps = BackendDependencies::unsupported();
        deps.dictation_engine = Arc::new(FakeEngine);
        deps.credential_store = Arc::new(crate::credentials::InMemoryCredentialStore::default());
        deps.selection_runtime = Some(Arc::new(
            crate::testing::FixtureSelectionRuntime::successful(
                crate::domains::SelectionCapture {
                    text: "fixture".into(),
                    source_app: None,
                },
                InsertOutcome::Inserted,
            ),
        ));
        deps.selection_polisher = Some(polisher);
        deps.marketplace_config = Some(
            crate::marketplace::MarketplaceConfig::new("https://market.example")
                .unwrap()
                .with_encrypted_sync(crate::cloud_sync_e2ee::EncryptedSyncConfig {
                    service_origin: "https://sync.example".into(),
                    app_version: "runtime-test".into(),
                }),
        );
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.path().into(),
                ..BackendConfig::default()
            },
            deps,
        )
        .unwrap();
        // This fixture exercises local admission only; do not start the network sync worker.
        backend.state.write().unwrap().running = true;
        TestBackend {
            backend,
            _data_dir: data_dir,
        }
    }

    #[tokio::test]
    async fn runtime_restore_actual_store_recovery_rejects_active_voice_before_repository_mutation()
    {
        use crate::cloud_sync_e2ee_documents::DocumentError;
        let backend = runtime_restore_backend();
        backend.ensure_runtime_ready().unwrap();
        let mut preferences = backend.get_preferences();
        preferences.testing_features_enabled = true;
        preferences.coding_agent_enabled = true;
        backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        let owner = SessionId::new();
        backend.begin_less_computer_capture(owner).unwrap();
        let gate = crate::cloud_sync_e2ee_store::gate::open_for_data_dir(backend._data_dir.path())
            .unwrap();
        drop(gate.begin_mutation().unwrap()); // model an interrupted save that requires reconciliation
        let before = std::fs::read(
            backend
                ._data_dir
                .path()
                .join("encrypted-sync/generation.json"),
        )
        .unwrap();
        let store = backend.encrypted_sync_store.as_ref().unwrap();
        assert_eq!(
            store.recover_registered().await.unwrap_err(),
            DocumentError::RuntimeBusy
        );
        assert_eq!(
            std::fs::read(
                backend
                    ._data_dir
                    .path()
                    .join("encrypted-sync/generation.json")
            )
            .unwrap(),
            before
        );
        assert_eq!(
            backend.ensure_runtime_ready().unwrap_err().code,
            BackendErrorCode::Busy
        );
        assert!(
            backend.get_preferences().coding_agent_enabled,
            "settings remain readable for recovery UI"
        );
        backend.abort_less_computer_capture(owner).unwrap();
    }

    #[tokio::test]
    async fn runtime_restore_constructor_guard_blocks_all_configured_runtime_starts_on_recovery_latch(
    ) {
        let backend = runtime_restore_backend();
        let gate = crate::cloud_sync_e2ee_store::gate::open_for_data_dir(backend._data_dir.path())
            .unwrap();
        drop(gate.begin_mutation().unwrap());
        assert_eq!(
            backend.start_dictation().await.unwrap_err().code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            backend
                .submit_less_computer("fixture".into())
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            backend
                .services()
                .selection
                .begin_polish(crate::domains::SelectionPolishRequest {
                    selected_text: Some("fixture".into()),
                    mode: crate::PolishMode::Raw,
                    instruction: None,
                })
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(
            backend
                .services()
                .selection_voice
                .begin(crate::domains::SelectionCapture {
                    text: "fixture".into(),
                    source_app: None
                })
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    }

    #[tokio::test]
    async fn runtime_restore_cancelled_apply_ticket_stays_busy_until_native_completion() {
        let (backend, _) = backend();
        let service = &backend.services().selection_voice;
        let session = service
            .begin(crate::domains::SelectionCapture {
                text: "fixture".into(),
                source_app: None,
            })
            .await
            .unwrap();
        service.mark_processing(session).await.unwrap();
        service
            .set_preview(crate::domains::SelectionVoicePreviewUpdate {
                session_id: session,
                owner_session_id: None,
                text: "replacement".into(),
                summary: None,
            })
            .await
            .unwrap();
        let ticket = service
            .begin_preview_apply(None, "replacement".into())
            .unwrap();
        service.cancel(Some(session)).await.unwrap();
        assert!(
            !service.runtime_restore_idle(),
            "cancel does not complete a native apply ticket"
        );
        assert_eq!(
            service
                .finish_preview_apply(
                    ticket.ticket_id,
                    crate::domains::SelectionVoiceApplyOutcome::Failed
                )
                .await
                .unwrap_err()
                .code,
            BackendErrorCode::Cancelled
        );
        assert!(service.runtime_restore_idle());
    }

    #[test]
    fn runtime_restore_bindings_do_not_keep_store_or_services_alive() {
        let backend = runtime_restore_backend();
        let store = Arc::downgrade(backend.encrypted_sync_store.as_ref().unwrap());
        let voice = Arc::downgrade(&backend.voice_sessions);
        let qa = Arc::downgrade(&backend.services().qa);
        let selection_voice = Arc::downgrade(&backend.services().selection_voice);
        drop(backend);
        assert!(store.upgrade().is_none());
        assert!(voice.upgrade().is_none());
        assert!(qa.upgrade().is_none());
        assert!(selection_voice.upgrade().is_none());
    }
    struct RestoreCancelPolisher {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        drained: Arc<tokio::sync::Notify>,
    }
    impl crate::ports::TextPolisher for RestoreCancelPolisher {
        fn polish(
            &self,
            _: SessionId,
            _: Arc<DictationContext>,
            _: String,
            _: Arc<dyn crate::ports::TextStreamSink>,
        ) -> BoxFuture<'static, Result<crate::ports::PolishOutput, BackendError>> {
            Box::pin(async { Ok(crate::ports::PolishOutput::text("fixture")) })
        }
        fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            let entered = self.entered.clone();
            let release = self.release.clone();
            let drained = self.drained.clone();
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                drained.notify_one();
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn runtime_restore_selection_cancel_retains_lease_after_waiter_drop() {
        let polisher = Arc::new(RestoreCancelPolisher {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            drained: Arc::new(tokio::sync::Notify::new()),
        });
        let backend = runtime_restore_backend_with_polisher(polisher.clone());
        let mut preferences = backend.get_preferences();
        preferences.selection_polish_output_mode =
            crate::shared_types::SelectionPolishOutputMode::PreviewConfirm;
        backend
            .update_settings(
                preferences,
                crate::SettingsUpdateOptions::STRICT,
                &crate::NoopSettingsRuntime,
            )
            .unwrap();
        let selection = backend.services().selection.clone();
        selection
            .begin_polish(crate::domains::SelectionPolishRequest {
                selected_text: Some("fixture".into()),
                mode: crate::PolishMode::Raw,
                instruction: None,
            })
            .await
            .unwrap();
        let cancel = tokio::spawn(selection.cancel(None));
        polisher.entered.notified().await;
        assert_eq!(
            selection.snapshot().await.unwrap().phase,
            crate::domains::SelectionPhase::Cancelled
        );
        assert!(!selection.runtime_restore_idle());
        cancel.abort();
        assert!(cancel.await.unwrap_err().is_cancelled());
        assert!(!selection.runtime_restore_idle());
        polisher.release.notify_one();
        polisher.drained.notified().await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(selection.runtime_restore_idle());
    }
    #[tokio::test]
    async fn restore_host_blocked_startup_is_visible_and_never_opens_or_mutates_real_stores() {
        struct ForbiddenRuntime;
        impl crate::SettingsRuntime for ForbiddenRuntime {
            fn prepare(
                &self,
                _: &crate::SettingsEffectPlan,
            ) -> Result<crate::SettingsEffectReceipt, crate::SettingsEffectFailure> {
                panic!("blocked startup must not run Host effects")
            }
        }
        let data_dir = TestDataDir::new("blocked-startup");
        let error = BackendError::new(BackendErrorCode::Persistence, "local storage access denied");
        let backend = OpenLessBackend::blocked_startup(
            BackendConfig {
                data_dir: data_dir.path().into(),
                ..BackendConfig::default()
            },
            error,
        );
        assert!(!data_dir.path().exists());
        assert_eq!(
            backend.start().await.unwrap_err().code,
            BackendErrorCode::Persistence
        );
        assert!(!backend.snapshot().running);
        assert_eq!(
            backend.ensure_runtime_ready().unwrap_err().code,
            BackendErrorCode::Persistence
        );
        let mut target = backend.get_preferences();
        target.launch_at_login = true;
        assert_eq!(
            backend
                .update_settings(
                    target,
                    crate::SettingsUpdateOptions::STRICT,
                    &ForbiddenRuntime
                )
                .unwrap_err()
                .code,
            BackendErrorCode::Persistence
        );
        assert_eq!(
            backend
                .begin_less_computer_capture(SessionId::new())
                .unwrap_err()
                .code,
            BackendErrorCode::Persistence
        );
        assert!(!data_dir.path().exists());
    }
}
