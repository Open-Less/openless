use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::domains::{
    LocalAsrRuntimeStatus, QaPhase, QaSnapshot, RemoteInputStatus, SelectionSnapshot,
    SelectionVoiceSnapshot,
};
use crate::shared_types::{CredentialsStatus, HotkeyStatus, PendingCorrection, QaChatMessage};
use crate::types::{
    DictationResult, DictationStateSnapshot, DownloadProgress, HistoryChange,
    InsertFallbackPayload, NotificationPayload, PermissionSnapshot, PolishDelta, PreferencesChange,
    SessionId, StylePackChange, TranscriptDelta, VocabularyChange,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendEvent {
    pub sequence: u64,
    pub session_id: Option<SessionId>,
    pub kind: BackendEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAsrRuntimeKind {
    Foundry,
    SherpaOnnx,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAsrPreparePhase {
    Runtime,
    Model,
    Load,
    Finished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrPrepareProgress {
    pub runtime: LocalAsrRuntimeKind,
    pub phase: LocalAsrPreparePhase,
    pub model_alias: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalAsrDownloadPhase {
    Started,
    Progress,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrDownloadProgress {
    pub runtime: LocalAsrRuntimeKind,
    pub model_id: String,
    pub file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub phase: LocalAsrDownloadPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CodingAgentStreamEvent {
    Started {
        session_id: String,
    },
    Delta {
        session_id: String,
        text: String,
    },
    ToolUse {
        session_id: String,
        name: String,
    },
    Compaction {
        session_id: String,
    },
    Completed {
        session_id: String,
        text: String,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },
    Cancelled {
        session_id: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LessComputerEventKind {
    /// Core voice presentation snapshot. Session ownership prevents a late
    /// meter or terminal update from replacing a newer recording.
    VoiceState {
        session_id: SessionId,
        phase: LessComputerVoicePhase,
        level: f32,
        elapsed_ms: u64,
    },
    User {
        text: String,
        fresh: bool,
    },
    Started,
    Delta {
        text: String,
    },
    Tool {
        name: String,
    },
    Compaction,
    Approval {
        token: String,
        command: String,
        reason: String,
    },
    Completed {
        text: String,
        #[serde(rename = "costUsd")]
        cost_usd: Option<f64>,
    },
    Error {
        message: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessComputerVoicePhase {
    Starting,
    Recording,
    Transcribing,
    Idle,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LessComputerEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(flatten)]
    pub kind: LessComputerEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaStateKind {
    Idle,
    Loading,
    Thinking,
    Recording,
    AnswerDelta,
    Answer,
    AwaitingApproval,
    Cancelled,
    Error,
}

/// Typed superset of the legacy QA state payload.
///
/// Optional fields preserve the existing per-kind wire shape while ensuring
/// producers cannot publish arbitrary JSON through the shared event stream.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaStateEvent {
    pub kind: QaStateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<QaChatMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_instruction_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_apply_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_revert_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_token: Option<String>,
}

impl QaStateEvent {
    pub fn simple(kind: QaStateKind) -> Self {
        Self {
            kind,
            session_id: None,
            messages: None,
            selection_preview: None,
            chunk: None,
            error: None,
            edit_instruction_mode: None,
            edit_apply_available: None,
            edit_revert_available: None,
            approval_token: None,
        }
    }

    /// Convert the current QA source-of-truth snapshot into the same typed
    /// payload used by live events. Hosts use this after an event-stream lag so
    /// they do not need a second phase or optional-field mapping.
    pub fn from_snapshot(snapshot: &QaSnapshot) -> Self {
        let kind = match snapshot.phase {
            QaPhase::Idle => QaStateKind::Idle,
            QaPhase::Recording => QaStateKind::Recording,
            QaPhase::Thinking => QaStateKind::Thinking,
            QaPhase::AwaitingApproval => QaStateKind::AwaitingApproval,
            QaPhase::Completed => QaStateKind::Answer,
            QaPhase::Cancelled => QaStateKind::Cancelled,
            QaPhase::Failed => QaStateKind::Error,
        };
        Self::from_snapshot_transition(
            snapshot,
            kind,
            None,
            (snapshot.phase == QaPhase::Failed)
                .then(|| snapshot.last_error.clone())
                .flatten(),
            false,
        )
    }

    pub(crate) fn from_snapshot_transition(
        snapshot: &QaSnapshot,
        kind: QaStateKind,
        chunk: Option<String>,
        error: Option<String>,
        force_edit_fields: bool,
    ) -> Self {
        let messages = snapshot
            .messages
            .iter()
            .map(|message| QaChatMessage {
                role: message.role.clone(),
                content: message.content.clone(),
                selection_text: message.selection_text.clone(),
            })
            .collect();
        let carries_messages = matches!(
            kind,
            QaStateKind::Idle
                | QaStateKind::Loading
                | QaStateKind::Thinking
                | QaStateKind::Recording
                | QaStateKind::Answer
                | QaStateKind::AwaitingApproval
                | QaStateKind::Cancelled
                | QaStateKind::Error
        );
        let carries_selection = matches!(
            kind,
            QaStateKind::Loading | QaStateKind::Thinking | QaStateKind::Recording
        );
        let carries_edit_state = force_edit_fields
            || kind == QaStateKind::Idle
            || (kind == QaStateKind::Answer
                && (snapshot.edit_instruction_mode
                    || snapshot.edit_apply_available
                    || snapshot.edit_revert_available));
        Self {
            kind,
            session_id: snapshot.session_id.map(|session_id| session_id.to_string()),
            messages: carries_messages.then_some(messages),
            selection_preview: carries_selection
                .then(|| snapshot.selection_preview.clone())
                .flatten(),
            chunk,
            error,
            edit_instruction_mode: carries_edit_state.then_some(snapshot.edit_instruction_mode),
            edit_apply_available: carries_edit_state.then_some(snapshot.edit_apply_available),
            edit_revert_available: carries_edit_state.then_some(snapshot.edit_revert_available),
            approval_token: snapshot.pending_approval_token.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaRecordingLevel {
    pub session_id: String,
    pub level: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputRuntimeEvent {
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
}

impl From<&RemoteInputStatus> for RemoteInputRuntimeEvent {
    fn from(status: &RemoteInputStatus) -> Self {
        Self {
            running: status.running,
            port: status.running.then_some(status.port),
            urls: status.urls.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputErrorEvent {
    pub reason: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingControlAction {
    Stop,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingControlRequest {
    pub session_id: SessionId,
    pub action: RecordingControlAction,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum BackendEventKind {
    BackendStarted,
    BackendStopping,
    DictationStateChanged(DictationStateSnapshot),
    TranscriptDelta(TranscriptDelta),
    PolishDelta(PolishDelta),
    DictationCompleted(DictationResult),
    RecordingControlRequested(RecordingControlRequest),
    SelectionStateChanged(SelectionSnapshot),
    SelectionVoiceStateChanged(SelectionVoiceSnapshot),
    InsertFallback(InsertFallbackPayload),
    PreferencesChanged(PreferencesChange),
    CredentialsChanged(CredentialsStatus),
    HistoryChanged(HistoryChange),
    VocabularyChanged(VocabularyChange),
    StylePacksChanged(StylePackChange),
    DownloadProgress(DownloadProgress),
    PermissionChanged(PermissionSnapshot),
    HotkeyStatusChanged(HotkeyStatus),
    Notification(NotificationPayload),
    CodingAgentTest(CodingAgentStreamEvent),
    LessComputerEvent(LessComputerEvent),
    LocalAsrPrepareProgress(LocalAsrPrepareProgress),
    LocalAsrDownloadProgress(LocalAsrDownloadProgress),
    LocalAsrEngineChanged(LocalAsrRuntimeStatus),
    MicrophoneDevicesChanged,
    QaLevel(QaRecordingLevel),
    QaState(QaStateEvent),
    RemoteInputStatusChanged(RemoteInputRuntimeEvent),
    RemoteInputFailed(RemoteInputErrorEvent),
    VocabularySuggestionsChanged(Vec<PendingCorrection>),
}

/// Bounded, instance-local replay result used when a host mounts after events
/// were already published or needs to recover after subscription lag.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventReplay {
    pub events: Vec<BackendEvent>,
    pub oldest_sequence: Option<u64>,
    pub latest_sequence: u64,
    pub truncated: bool,
}

const EVENT_REPLAY_CAPACITY: usize = 2048;

#[derive(Debug)]
pub struct EventBus {
    sequence: AtomicU64,
    sender: broadcast::Sender<BackendEvent>,
    backlog: Mutex<VecDeque<BackendEvent>>,
    // Retain one presentation snapshot independently of the bounded replay so
    // a reopened host can recover a long-running transcription.
    latest_less_voice: Mutex<Option<LessComputerEvent>>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self {
            sequence: AtomicU64::new(0),
            sender,
            backlog: Mutex::new(VecDeque::with_capacity(EVENT_REPLAY_CAPACITY)),
            latest_less_voice: Mutex::new(None),
        }
    }

    pub fn publish(&self, session_id: Option<SessionId>, mut kind: BackendEventKind) {
        // One existing lock linearizes sequence allocation, projection, replay
        // and live delivery. Native audio and Agent threads publish concurrently:
        // assigning a number before locking (or sending after unlocking) lets a
        // higher sequence overtake an earlier event, which UI watermarks discard.
        // No asynchronous work or Host callback belongs in this critical section.
        let mut backlog = self.backlog.lock().expect("event backlog lock poisoned");
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        if let BackendEventKind::LessComputerEvent(event) = &mut kind {
            event.seq = Some(sequence);
        }
        let event = BackendEvent {
            sequence,
            session_id,
            kind,
        };
        if let BackendEventKind::LessComputerEvent(voice) = &event.kind {
            if let LessComputerEventKind::VoiceState {
                session_id, phase, ..
            } = &voice.kind
            {
                let mut latest = self
                    .latest_less_voice
                    .lock()
                    .expect("voice projection lock poisoned");
                let superseded = latest.as_ref().is_some_and(|previous| {
                    if previous.seq >= voice.seq {
                        return true;
                    }
                    let LessComputerEventKind::VoiceState {
                        session_id: previous_id,
                        phase: previous_phase,
                        ..
                    } = &previous.kind
                    else {
                        return false;
                    };
                    if previous_id != session_id {
                        return *phase != LessComputerVoicePhase::Starting;
                    }
                    *previous_phase == LessComputerVoicePhase::Idle
                        || (*previous_phase == LessComputerVoicePhase::Transcribing
                            && *phase == LessComputerVoicePhase::Recording)
                });
                if !superseded {
                    *latest = Some(voice.clone());
                }
            }
        }
        backlog.push_back(event.clone());
        while backlog.len() > EVENT_REPLAY_CAPACITY {
            backlog.pop_front();
        }
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription {
            receiver: self.sender.subscribe(),
        }
    }

    pub fn replay_after(&self, sequence: u64) -> EventReplay {
        let backlog = self.backlog.lock().expect("event backlog lock poisoned");
        let oldest_sequence = backlog.front().map(|event| event.sequence);
        // Publish holds this same lock through send, so this watermark never
        // advances past an event which has not yet entered the replay buffer.
        let latest_sequence = self.sequence.load(Ordering::Acquire);
        let truncated = oldest_sequence.is_some_and(|oldest| sequence.saturating_add(1) < oldest);
        let events = backlog
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect();
        EventReplay {
            events,
            oldest_sequence,
            latest_sequence,
            truncated,
        }
    }

    pub fn latest_less_computer_voice_state(&self) -> Option<LessComputerEvent> {
        self.latest_less_voice
            .lock()
            .expect("voice projection lock poisoned")
            .clone()
    }
}

/// Cloneable typed event sink for platform and transport Adapters.
///
/// Adapters publish semantic core events through this Interface instead of
/// creating a host-only event stream. The publisher shares the backend's
/// sequence counter and subscriptions, so lag detection and snapshot resync
/// work identically for core- and Adapter-originated events.
#[derive(Clone)]
pub struct BackendEventPublisher {
    bus: Arc<EventBus>,
}

impl BackendEventPublisher {
    pub(crate) fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }

    pub fn publish(&self, session_id: Option<SessionId>, kind: BackendEventKind) {
        self.bus.publish(session_id, kind);
    }

    pub fn replay_after(&self, sequence: u64) -> EventReplay {
        self.bus.replay_after(sequence)
    }

    pub fn latest_less_computer_voice_state(&self) -> Option<LessComputerEvent> {
        self.bus.latest_less_computer_voice_state()
    }
}

pub struct EventSubscription {
    receiver: broadcast::Receiver<BackendEvent>,
}

impl EventSubscription {
    pub async fn recv(&mut self) -> Result<BackendEvent, EventRecvError> {
        self.receiver.recv().await.map_err(EventRecvError::from)
    }

    /// Drain one event without ever waiting on the UI thread.
    ///
    /// A frame should call this repeatedly until [`EventRecvError::Empty`],
    /// then request a repaint when at least one event was received.  A lagged
    /// receiver is deliberately surfaced so the caller can resynchronise from
    /// [`OpenLessBackend::snapshot`](crate::OpenLessBackend::snapshot).
    pub fn try_recv(&mut self) -> Result<BackendEvent, EventRecvError> {
        self.receiver.try_recv().map_err(EventRecvError::from)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EventRecvError {
    #[error("event subscription has no pending event")]
    Empty,
    #[error("event subscription lagged by {0} event(s)")]
    Lagged(u64),
    #[error("event bus closed")]
    Closed,
}

impl From<broadcast::error::RecvError> for EventRecvError {
    fn from(error: broadcast::error::RecvError) -> Self {
        match error {
            broadcast::error::RecvError::Lagged(count) => Self::Lagged(count),
            broadcast::error::RecvError::Closed => Self::Closed,
        }
    }
}

impl From<broadcast::error::TryRecvError> for EventRecvError {
    fn from(error: broadcast::error::TryRecvError) -> Self {
        match error {
            broadcast::error::TryRecvError::Empty => Self::Empty,
            broadcast::error::TryRecvError::Lagged(count) => Self::Lagged(count),
            broadcast::error::TryRecvError::Closed => Self::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DictationPhase;

    #[tokio::test]
    async fn sequence_is_monotonic_and_lag_is_explicit() {
        let bus = EventBus::new(1);
        let mut subscription = bus.subscribe();
        bus.publish(None, BackendEventKind::BackendStarted);
        bus.publish(
            None,
            BackendEventKind::DictationStateChanged(DictationStateSnapshot {
                phase: DictationPhase::Idle,
                ..DictationStateSnapshot::default()
            }),
        );

        assert_eq!(subscription.recv().await, Err(EventRecvError::Lagged(1)));
        let event = subscription.recv().await;
        assert_eq!(event.unwrap().sequence, 2);
    }

    #[test]
    fn native_publishers_keep_live_and_replay_sequences_in_the_same_order() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 1000;
        let bus = EventBus::new(THREADS * PER_THREAD);
        let mut subscription = bus.subscribe();
        let barrier = std::sync::Barrier::new(THREADS);
        // Native audio, Agent output and Core transitions are independent OS
        // threads. Capacity covers every message, so a failure here is order
        // corruption, not the documented bounded-subscription lag behavior.
        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    barrier.wait();
                    for _ in 0..PER_THREAD {
                        bus.publish(None, BackendEventKind::BackendStarted);
                    }
                });
            }
        });
        for expected in 1..=(THREADS * PER_THREAD) as u64 {
            assert_eq!(subscription.try_recv().unwrap().sequence, expected);
        }
        assert_eq!(subscription.try_recv(), Err(EventRecvError::Empty));
        let replay = bus.replay_after(0);
        assert!(replay.truncated);
        assert_eq!(replay.events.len(), EVENT_REPLAY_CAPACITY);
        assert_eq!(replay.latest_sequence, (THREADS * PER_THREAD) as u64);
        let first = replay.latest_sequence - EVENT_REPLAY_CAPACITY as u64 + 1;
        for (expected, event) in (first..=replay.latest_sequence).zip(replay.events) {
            assert_eq!(event.sequence, expected);
        }
    }

    #[test]
    fn try_recv_is_non_blocking_and_reports_empty() {
        let bus = EventBus::new(2);
        let mut subscription = bus.subscribe();
        assert_eq!(subscription.try_recv(), Err(EventRecvError::Empty));
        bus.publish(None, BackendEventKind::BackendStarted);
        assert_eq!(subscription.try_recv().unwrap().sequence, 1);
        assert_eq!(subscription.try_recv(), Err(EventRecvError::Empty));
    }

    #[test]
    fn event_serialization_is_tagged_and_does_not_add_secret_fields() {
        let event = BackendEvent {
            sequence: 7,
            session_id: None,
            kind: BackendEventKind::CredentialsChanged(CredentialsStatus {
                active_asr_provider: "fixture-asr".to_string(),
                active_llm_provider: "fixture-llm".to_string(),
                asr_configured: true,
                llm_configured: true,
                ..CredentialsStatus::default()
            }),
        };
        let json = serde_json::to_string(&event).expect("event should serialize");
        assert!(json.contains("credentials_changed"));
        assert!(json.contains("activeAsrProvider"));
        assert!(!json.contains("token"));
        assert!(!json.contains("authorization"));
    }

    #[test]
    fn adapter_publisher_shares_sequence_and_subscription_with_backend_events() {
        let bus = Arc::new(EventBus::new(4));
        let publisher = BackendEventPublisher::new(Arc::clone(&bus));
        let mut subscription = bus.subscribe();

        bus.publish(None, BackendEventKind::BackendStarted);
        publisher.publish(
            None,
            BackendEventKind::Notification(NotificationPayload {
                level: crate::types::NotificationLevel::Info,
                message: "adapter-ready".to_string(),
            }),
        );

        assert_eq!(subscription.try_recv().unwrap().sequence, 1);
        let adapter_event = subscription.try_recv().unwrap();
        assert_eq!(adapter_event.sequence, 2);
        assert!(matches!(
            adapter_event.kind,
            BackendEventKind::Notification(NotificationPayload { ref message, .. })
                if message == "adapter-ready"
        ));
    }

    #[test]
    fn replay_is_instance_local_bounded_and_reports_truncation() {
        let bus = EventBus::new(2);
        for _ in 0..(EVENT_REPLAY_CAPACITY + 2) {
            bus.publish(None, BackendEventKind::BackendStarted);
        }

        let replay = bus.replay_after(0);
        assert_eq!(replay.events.len(), EVENT_REPLAY_CAPACITY);
        assert_eq!(replay.oldest_sequence, Some(3));
        assert_eq!(replay.latest_sequence, (EVENT_REPLAY_CAPACITY + 2) as u64);
        assert!(replay.truncated);

        let other = EventBus::new(2);
        assert!(other.replay_after(0).events.is_empty());
    }

    #[test]
    fn less_computer_payload_uses_the_backend_sequence_for_replay_deduplication() {
        let bus = EventBus::new(2);
        bus.publish(
            None,
            BackendEventKind::LessComputerEvent(LessComputerEvent {
                seq: None,
                kind: LessComputerEventKind::Started,
            }),
        );

        let replay = bus.replay_after(0);
        let BackendEventKind::LessComputerEvent(event) = &replay.events[0].kind else {
            panic!("expected Less Computer event");
        };
        assert_eq!(event.seq, Some(replay.events[0].sequence));
    }

    #[test]
    fn voice_projection_survives_replay_eviction_and_ignores_a_stale_owner() {
        let bus = EventBus::new(2);
        let a = SessionId::new();
        let b = SessionId::new();
        for (session_id, phase) in [
            (a, LessComputerVoicePhase::Starting),
            (b, LessComputerVoicePhase::Starting),
            (b, LessComputerVoicePhase::Transcribing),
            (a, LessComputerVoicePhase::Idle),
        ] {
            bus.publish(
                Some(session_id),
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::VoiceState {
                        session_id,
                        phase,
                        level: 0.0,
                        elapsed_ms: 120,
                    },
                }),
            );
        }
        for _ in 0..EVENT_REPLAY_CAPACITY {
            bus.publish(None, BackendEventKind::BackendStarted);
        }
        assert!(bus.replay_after(0).truncated);
        assert!(!bus
            .replay_after(0)
            .events
            .iter()
            .any(|event| matches!(event.kind, BackendEventKind::LessComputerEvent(_))));
        let current = bus.latest_less_computer_voice_state().unwrap();
        assert_eq!(
            current.seq,
            Some(3),
            "projection retains the original event sequence"
        );
        assert!(matches!(current.kind, LessComputerEventKind::VoiceState {
            session_id, phase: LessComputerVoicePhase::Transcribing, ..
        } if session_id == b));
    }

    #[test]
    fn migration_events_are_typed_and_keep_legacy_payload_fields() {
        let qa = QaStateEvent {
            kind: QaStateKind::AnswerDelta,
            session_id: Some("qa-session".into()),
            messages: None,
            selection_preview: None,
            chunk: Some("hello".into()),
            error: None,
            edit_instruction_mode: None,
            edit_apply_available: None,
            edit_revert_available: None,
            approval_token: None,
        };
        let qa_json = serde_json::to_value(&qa).unwrap();
        assert_eq!(qa_json["kind"], "answer_delta");
        assert_eq!(qa_json["sessionId"], "qa-session");
        assert_eq!(qa_json["chunk"], "hello");

        let less_computer = LessComputerEvent {
            seq: Some(3),
            kind: LessComputerEventKind::Completed {
                text: "done".into(),
                cost_usd: Some(0.01),
            },
        };
        let less_json = serde_json::to_value(&less_computer).unwrap();
        assert_eq!(less_json["kind"], "completed");
        assert_eq!(less_json["seq"], 3);
        assert_eq!(less_json["costUsd"], 0.01);
    }

    #[test]
    fn remote_input_events_cannot_serialize_pairing_secrets() {
        let events = [
            BackendEventKind::RemoteInputStatusChanged(RemoteInputRuntimeEvent {
                running: true,
                port: Some(18989),
                urls: vec!["https://192.168.1.2:18989".into()],
            }),
            BackendEventKind::RemoteInputFailed(RemoteInputErrorEvent {
                reason: "address already in use".into(),
                port: 18989,
            }),
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap().to_ascii_lowercase();
            assert!(!json.contains("\"pin\""));
            assert!(!json.contains("authorization"));
            assert!(!json.contains("credential"));
        }
    }
}
