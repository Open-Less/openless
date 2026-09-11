//! Native popup process protocol and lifecycle management.
//!
//! The egui frame must never own or wait for a child process.  [`PopupSupervisor`]
//! moves the child, its pipes and all waiting into Tokio tasks and exposes only
//! non-blocking `try_*` methods to the UI thread.

use std::collections::HashSet;
use std::fmt;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::runtime::Handle;
use tokio::sync::mpsc as tokio_mpsc;

pub const POPUP_PROTOCOL_VERSION: u16 = 1;
pub const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupKind {
    Qa,
    Preview,
    Capsule,
}

impl PopupKind {
    pub fn argument(self) -> &'static str {
        match self {
            Self::Qa => "--qa",
            Self::Preview => "--preview",
            Self::Capsule => "--capsule",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_text: Option<String>,
}

/// Messages written by the Linux host to a popup's stdin.
///
/// Every variant is independently versioned and ordered.  This deliberately
/// avoids an unversioned outer envelope that can accidentally be discarded by
/// a future enum deserializer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostToPopup {
    Preview {
        version: u16,
        session_id: String,
        sequence: u64,
        text: String,
        source: String,
    },
    QaSnapshot {
        version: u16,
        session_id: String,
        sequence: u64,
        phase: String,
        messages: Vec<PopupChatMessage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection_preview: Option<String>,
        #[serde(default)]
        streaming_answer: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Capsule {
        version: u16,
        session_id: String,
        sequence: u64,
        phase: String,
        #[serde(default)]
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        audio_level: Option<f32>,
    },
    Hide {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    Shutdown {
        version: u16,
        session_id: String,
        sequence: u64,
    },
}

impl HostToPopup {
    pub fn version(&self) -> u16 {
        match self {
            Self::Preview { version, .. }
            | Self::QaSnapshot { version, .. }
            | Self::Capsule { version, .. }
            | Self::Hide { version, .. }
            | Self::Shutdown { version, .. } => *version,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Preview { session_id, .. }
            | Self::QaSnapshot { session_id, .. }
            | Self::Capsule { session_id, .. }
            | Self::Hide { session_id, .. }
            | Self::Shutdown { session_id, .. } => session_id,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Preview { sequence, .. }
            | Self::QaSnapshot { sequence, .. }
            | Self::Capsule { sequence, .. }
            | Self::Hide { sequence, .. }
            | Self::Shutdown { sequence, .. } => *sequence,
        }
    }

    pub fn content_kind(&self) -> Option<PopupKind> {
        match self {
            Self::Preview { .. } => Some(PopupKind::Preview),
            Self::QaSnapshot { .. } => Some(PopupKind::Qa),
            Self::Capsule { .. } => Some(PopupKind::Capsule),
            Self::Hide { .. } | Self::Shutdown { .. } => None,
        }
    }
}

/// Actions written by a popup to the host's stdout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PopupToHost {
    Ready {
        version: u16,
        session_id: String,
        sequence: u64,
        kind: PopupKind,
    },
    ConfirmPreview {
        version: u16,
        session_id: String,
        sequence: u64,
        text: String,
    },
    CancelPreview {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    SubmitQa {
        version: u16,
        session_id: String,
        sequence: u64,
        text: String,
    },
    ToggleQaRecording {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    DismissQa {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    DismissCapsule {
        version: u16,
        session_id: String,
        sequence: u64,
    },
}

impl PopupToHost {
    pub fn version(&self) -> u16 {
        match self {
            Self::Ready { version, .. }
            | Self::ConfirmPreview { version, .. }
            | Self::CancelPreview { version, .. }
            | Self::SubmitQa { version, .. }
            | Self::ToggleQaRecording { version, .. }
            | Self::DismissQa { version, .. }
            | Self::DismissCapsule { version, .. } => *version,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Ready { session_id, .. }
            | Self::ConfirmPreview { session_id, .. }
            | Self::CancelPreview { session_id, .. }
            | Self::SubmitQa { session_id, .. }
            | Self::ToggleQaRecording { session_id, .. }
            | Self::DismissQa { session_id, .. }
            | Self::DismissCapsule { session_id, .. } => session_id,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Ready { sequence, .. }
            | Self::ConfirmPreview { sequence, .. }
            | Self::CancelPreview { sequence, .. }
            | Self::SubmitQa { sequence, .. }
            | Self::ToggleQaRecording { sequence, .. }
            | Self::DismissQa { sequence, .. }
            | Self::DismissCapsule { sequence, .. } => *sequence,
        }
    }

    pub fn kind(&self) -> PopupKind {
        match self {
            Self::Ready { kind, .. } => *kind,
            Self::ConfirmPreview { .. } | Self::CancelPreview { .. } => PopupKind::Preview,
            Self::SubmitQa { .. } | Self::ToggleQaRecording { .. } | Self::DismissQa { .. } => {
                PopupKind::Qa
            }
            Self::DismissCapsule { .. } => PopupKind::Capsule,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PopupActionSlot {
    session_id: Option<String>,
    sequence: u64,
}

/// Rejects stale, cross-session and cross-kind actions received from popup
/// children. Each newly spawned process resets only its own sequence domain.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PopupActionGuard {
    qa: PopupActionSlot,
    preview: PopupActionSlot,
    capsule: PopupActionSlot,
}

impl PopupActionGuard {
    fn slot_mut(&mut self, kind: PopupKind) -> &mut PopupActionSlot {
        match kind {
            PopupKind::Qa => &mut self.qa,
            PopupKind::Preview => &mut self.preview,
            PopupKind::Capsule => &mut self.capsule,
        }
    }

    pub fn reset(&mut self, kind: PopupKind) {
        *self.slot_mut(kind) = PopupActionSlot::default();
    }

    pub fn accept(
        &mut self,
        process_kind: PopupKind,
        message: &PopupToHost,
        expected_session_id: &str,
    ) -> bool {
        if message.version() != POPUP_PROTOCOL_VERSION
            || message.kind() != process_kind
            || message.session_id() != expected_session_id
        {
            return false;
        }
        let slot = self.slot_mut(process_kind);
        if slot.session_id.as_deref() != Some(expected_session_id) {
            slot.session_id = Some(expected_session_id.to_owned());
            slot.sequence = 0;
        }
        if message.sequence() <= slot.sequence {
            return false;
        }
        slot.sequence = message.sequence();
        true
    }
}

pub trait VersionedMessage {
    fn protocol_version(&self) -> u16;
}

impl VersionedMessage for HostToPopup {
    fn protocol_version(&self) -> u16 {
        self.version()
    }
}

impl VersionedMessage for PopupToHost {
    fn protocol_version(&self) -> u16 {
        self.version()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolErrorKind {
    Io,
    Eof,
    Truncated,
    Oversize,
    Malformed,
    UnsupportedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub kind: ProtocolErrorKind,
    pub message: String,
}

impl ProtocolError {
    fn new(kind: ProtocolErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ProtocolError {}

/// Write one complete JSONL frame. Serialization is used for all escaping.
pub fn write_jsonl<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), ProtocolError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| ProtocolError::new(ProtocolErrorKind::Malformed, error.to_string()))?;
    if encoded.len() > MAX_JSONL_LINE_BYTES {
        return Err(ProtocolError::new(
            ProtocolErrorKind::Oversize,
            format!("popup JSONL frame is {} bytes", encoded.len()),
        ));
    }
    writer
        .write_all(&encoded)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|error| ProtocolError::new(ProtocolErrorKind::Io, error.to_string()))
}

/// Read one complete, bounded JSONL frame.
pub fn read_jsonl<T>(reader: &mut impl BufRead) -> Result<T, ProtocolError>
where
    T: DeserializeOwned + VersionedMessage,
{
    let bytes = read_bounded_line(reader)?;
    decode_jsonl(&bytes)
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| ProtocolError::new(ProtocolErrorKind::Io, error.to_string()))?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Err(ProtocolError::new(
                    ProtocolErrorKind::Eof,
                    "popup stream closed",
                ))
            } else {
                Err(ProtocolError::new(
                    ProtocolErrorKind::Truncated,
                    "popup stream ended in the middle of a JSONL frame",
                ))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = bytes
            .len()
            .saturating_add(newline.unwrap_or(available.len()));
        let take = newline.map_or(available.len(), |index| index + 1);
        if content_len > MAX_JSONL_LINE_BYTES {
            reader.consume(take);
            if newline.is_none() {
                discard_through_newline(reader)?;
            }
            return Err(ProtocolError::new(
                ProtocolErrorKind::Oversize,
                "popup JSONL frame exceeds the 1 MiB limit",
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(bytes);
        }
    }
}

fn discard_through_newline(reader: &mut impl BufRead) -> Result<(), ProtocolError> {
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| ProtocolError::new(ProtocolErrorKind::Io, error.to_string()))?;
        if available.is_empty() {
            return Ok(());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        reader.consume(take);
        if newline.is_some() {
            return Ok(());
        }
    }
}

fn decode_jsonl<T>(bytes: &[u8]) -> Result<T, ProtocolError>
where
    T: DeserializeOwned + VersionedMessage,
{
    let message: T = serde_json::from_slice(bytes)
        .map_err(|error| ProtocolError::new(ProtocolErrorKind::Malformed, error.to_string()))?;
    if message.protocol_version() != POPUP_PROTOCOL_VERSION {
        return Err(ProtocolError::new(
            ProtocolErrorKind::UnsupportedVersion,
            format!(
                "unsupported popup protocol version {} (expected {})",
                message.protocol_version(),
                POPUP_PROTOCOL_VERSION
            ),
        ));
    }
    Ok(message)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreviewPopupState {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QaPopupState {
    pub phase: String,
    pub messages: Vec<PopupChatMessage>,
    pub selection_preview: Option<String>,
    pub streaming_answer: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CapsulePopupState {
    pub phase: String,
    pub text: String,
    pub audio_level: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PopupState {
    pub session_id: Option<String>,
    pub last_sequence: u64,
    pub visible: bool,
    pub shutdown_requested: bool,
    pub preview: PreviewPopupState,
    pub qa: QaPopupState,
    pub capsule: CapsulePopupState,
    retired_sessions: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Stale,
    Shutdown,
}

impl PopupState {
    /// Apply a host event while rejecting late messages from an old session or
    /// duplicate/out-of-order sequence numbers.
    pub fn apply(&mut self, message: HostToPopup) -> ApplyOutcome {
        let session_id = message.session_id().to_owned();
        let sequence = message.sequence();
        if let Some(current) = self.session_id.as_deref() {
            if current == session_id {
                if sequence <= self.last_sequence {
                    return ApplyOutcome::Stale;
                }
            } else {
                let starts_session = matches!(
                    message,
                    HostToPopup::Preview { .. }
                        | HostToPopup::QaSnapshot { .. }
                        | HostToPopup::Capsule { .. }
                );
                if !starts_session || self.retired_sessions.contains(&session_id) {
                    return ApplyOutcome::Stale;
                }
                self.retired_sessions.insert(current.to_owned());
                self.last_sequence = 0;
            }
        }
        if sequence <= self.last_sequence {
            return ApplyOutcome::Stale;
        }
        self.session_id = Some(session_id);
        self.last_sequence = sequence;
        match message {
            HostToPopup::Preview { text, source, .. } => {
                self.preview = PreviewPopupState { text, source };
                self.visible = true;
            }
            HostToPopup::QaSnapshot {
                phase,
                messages,
                selection_preview,
                streaming_answer,
                error,
                ..
            } => {
                self.qa = QaPopupState {
                    phase,
                    messages,
                    selection_preview,
                    streaming_answer,
                    error,
                };
                self.visible = true;
            }
            HostToPopup::Capsule {
                phase,
                text,
                audio_level,
                ..
            } => {
                self.capsule = CapsulePopupState {
                    phase,
                    text,
                    audio_level,
                };
                self.visible = true;
            }
            HostToPopup::Hide { .. } => self.visible = false,
            HostToPopup::Shutdown { .. } => {
                self.visible = false;
                self.shutdown_requested = true;
                return ApplyOutcome::Shutdown;
            }
        }
        ApplyOutcome::Applied
    }
}

/// Blocking popup-side protocol driver, intended to run on the popup's stdin
/// reader thread. UI mutation must be forwarded by `on_message` to the popup
/// frame through a channel.
pub fn run_popup(
    reader: &mut impl BufRead,
    mut on_message: impl FnMut(HostToPopup),
) -> Result<(), ProtocolError> {
    loop {
        match read_jsonl::<HostToPopup>(reader) {
            Ok(message) => {
                let shutdown = matches!(message, HostToPopup::Shutdown { .. });
                on_message(message);
                if shutdown {
                    return Ok(());
                }
            }
            Err(error) if error.kind == ProtocolErrorKind::Eof => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupSupervisorEvent {
    Message(PopupToHost),
    ProtocolError(ProtocolError),
    Exited { code: Option<i32>, crashed: bool },
    SpawnFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupSendError {
    Full,
    Closed,
}

enum SupervisorCommand {
    Send(HostToPopup),
    Shutdown,
}

/// Non-blocking handle held by the main egui application.
pub struct PopupSupervisor {
    commands: tokio_mpsc::Sender<SupervisorCommand>,
    events: Receiver<PopupSupervisorEvent>,
}

impl PopupSupervisor {
    pub fn spawn(runtime: &Handle, executable: impl AsRef<Path>, kind: PopupKind) -> Self {
        let mut command = Command::new(executable.as_ref());
        command.arg("--openless-egui-popup").arg(kind.argument());
        Self::spawn_command(runtime, command)
    }

    /// Low-level construction seam used by tests and alternative launchers.
    pub fn spawn_command(runtime: &Handle, mut command: Command) -> Self {
        let (command_tx, command_rx) = tokio_mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::sync_channel(256);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        runtime.spawn(supervise(command, command_rx, event_tx));
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    /// Queue a message without ever waiting in an egui frame.
    pub fn try_send(&self, message: HostToPopup) -> Result<(), PopupSendError> {
        match self.commands.try_send(SupervisorCommand::Send(message)) {
            Ok(()) => Ok(()),
            Err(tokio_mpsc::error::TrySendError::Full(_)) => Err(PopupSendError::Full),
            Err(tokio_mpsc::error::TrySendError::Closed(_)) => Err(PopupSendError::Closed),
        }
    }

    /// Poll one child event without blocking the egui frame.
    pub fn try_recv(&self) -> Result<PopupSupervisorEvent, mpsc::TryRecvError> {
        self.events.try_recv()
    }

    pub fn request_shutdown(&self) -> Result<(), PopupSendError> {
        match self.commands.try_send(SupervisorCommand::Shutdown) {
            Ok(()) => Ok(()),
            Err(tokio_mpsc::error::TrySendError::Full(_)) => Err(PopupSendError::Full),
            Err(tokio_mpsc::error::TrySendError::Closed(_)) => Err(PopupSendError::Closed),
        }
    }
}

impl Drop for PopupSupervisor {
    fn drop(&mut self) {
        let _ = self.commands.try_send(SupervisorCommand::Shutdown);
    }
}

async fn supervise(
    mut command: Command,
    mut commands: tokio_mpsc::Receiver<SupervisorCommand>,
    events: mpsc::SyncSender<PopupSupervisorEvent>,
) {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = events.try_send(PopupSupervisorEvent::SpawnFailed(error.to_string()));
            return;
        }
    };
    let Some(mut stdin) = child.stdin.take() else {
        let _ = events.try_send(PopupSupervisorEvent::SpawnFailed(
            "popup stdin pipe was not created".to_owned(),
        ));
        let _ = child.kill().await;
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = events.try_send(PopupSupervisorEvent::SpawnFailed(
            "popup stdout pipe was not created".to_owned(),
        ));
        let _ = child.kill().await;
        return;
    };

    let (reader_tx, mut reader_rx) = tokio_mpsc::channel(64);
    tokio::spawn(read_child_output(BufReader::new(stdout), reader_tx));

    let mut reader_open = true;
    let mut shutdown_requested = false;
    loop {
        tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(status) => {
                        let code = status.code();
                        let _ = events.try_send(PopupSupervisorEvent::Exited {
                            code,
                            crashed: !status.success() && !shutdown_requested,
                        });
                    }
                    Err(error) => {
                        let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                            ProtocolError::new(ProtocolErrorKind::Io, error.to_string()),
                        ));
                    }
                }
                return;
            }
            output = reader_rx.recv(), if reader_open => {
                match output {
                    Some(event) => { let _ = events.try_send(event); }
                    None => reader_open = false,
                }
            }
            command = commands.recv() => {
                match command {
                    Some(SupervisorCommand::Send(message)) => {
                        match serde_json::to_vec(&message) {
                            Ok(encoded) if encoded.len() <= MAX_JSONL_LINE_BYTES => {
                                if let Err(error) = stdin.write_all(&encoded).await {
                                    let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                                        ProtocolError::new(ProtocolErrorKind::Io, error.to_string()),
                                    ));
                                } else if let Err(error) = stdin.write_all(b"\n").await {
                                    let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                                        ProtocolError::new(ProtocolErrorKind::Io, error.to_string()),
                                    ));
                                } else if let Err(error) = stdin.flush().await {
                                    let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                                        ProtocolError::new(ProtocolErrorKind::Io, error.to_string()),
                                    ));
                                }
                            }
                            Ok(encoded) => {
                                let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                                    ProtocolError::new(
                                        ProtocolErrorKind::Oversize,
                                        format!("popup JSONL frame is {} bytes", encoded.len()),
                                    ),
                                ));
                            }
                            Err(error) => {
                                let _ = events.try_send(PopupSupervisorEvent::ProtocolError(
                                    ProtocolError::new(ProtocolErrorKind::Malformed, error.to_string()),
                                ));
                            }
                        }
                    }
                    Some(SupervisorCommand::Shutdown) | None => {
                        shutdown_requested = true;
                        let _ = child.start_kill();
                    }
                }
            }
        }
    }
}

async fn read_child_output<R>(mut reader: R, events: tokio_mpsc::Sender<PopupSupervisorEvent>)
where
    R: AsyncBufRead + Unpin,
{
    loop {
        match read_async_bounded_line(&mut reader).await {
            Ok(bytes) => {
                let event = match decode_jsonl::<PopupToHost>(&bytes) {
                    Ok(message) => PopupSupervisorEvent::Message(message),
                    Err(error) => PopupSupervisorEvent::ProtocolError(error),
                };
                if events.send(event).await.is_err() {
                    return;
                }
            }
            Err(error) if error.kind == ProtocolErrorKind::Eof => return,
            Err(error) => {
                if events
                    .send(PopupSupervisorEvent::ProtocolError(error))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn read_async_bounded_line<R>(reader: &mut R) -> Result<Vec<u8>, ProtocolError>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| ProtocolError::new(ProtocolErrorKind::Io, error.to_string()))?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Err(ProtocolError::new(
                    ProtocolErrorKind::Eof,
                    "popup stream closed",
                ))
            } else {
                Err(ProtocolError::new(
                    ProtocolErrorKind::Truncated,
                    "popup stream ended in the middle of a JSONL frame",
                ))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = bytes
            .len()
            .saturating_add(newline.unwrap_or(available.len()));
        let take = newline.map_or(available.len(), |index| index + 1);
        if content_len > MAX_JSONL_LINE_BYTES {
            reader.consume(take);
            if newline.is_none() {
                discard_async_through_newline(reader).await?;
            }
            return Err(ProtocolError::new(
                ProtocolErrorKind::Oversize,
                "popup JSONL frame exceeds the 1 MiB limit",
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(bytes);
        }
    }
}

async fn discard_async_through_newline<R>(reader: &mut R) -> Result<(), ProtocolError>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| ProtocolError::new(ProtocolErrorKind::Io, error.to_string()))?;
        if available.is_empty() {
            return Ok(());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        reader.consume(take);
        if newline.is_some() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    fn preview(text: String) -> HostToPopup {
        HostToPopup::Preview {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session-一".to_owned(),
            sequence: 7,
            text,
            source: "原文 \\\\ source".to_owned(),
        }
    }

    #[test]
    fn jsonl_round_trip_escapes_quotes_backslashes_and_unicode() {
        let expected = preview("他说：\"你好\" C:\\\\tmp\\\\文件".to_owned());
        let mut bytes = Vec::new();
        write_jsonl(&mut bytes, &expected).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let actual: HostToPopup = read_jsonl(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn malformed_oversize_eof_and_truncation_are_classified() {
        let malformed = read_jsonl::<HostToPopup>(&mut Cursor::new(b"not json\n"));
        assert_eq!(malformed.unwrap_err().kind, ProtocolErrorKind::Malformed);

        let oversized = vec![b'x'; MAX_JSONL_LINE_BYTES + 2];
        let oversized = read_jsonl::<HostToPopup>(&mut Cursor::new(oversized));
        assert_eq!(oversized.unwrap_err().kind, ProtocolErrorKind::Oversize);

        let eof = read_jsonl::<HostToPopup>(&mut Cursor::new(Vec::<u8>::new()));
        assert_eq!(eof.unwrap_err().kind, ProtocolErrorKind::Eof);

        let truncated = read_jsonl::<HostToPopup>(&mut Cursor::new(b"{\"type\":"));
        assert_eq!(truncated.unwrap_err().kind, ProtocolErrorKind::Truncated);
    }

    #[test]
    fn popup_state_rejects_late_and_cross_session_messages() {
        let mut state = PopupState::default();
        assert_eq!(state.apply(preview("new".into())), ApplyOutcome::Applied);
        let mut late = preview("late".into());
        if let HostToPopup::Preview { sequence, .. } = &mut late {
            *sequence = 6;
        }
        assert_eq!(state.apply(late), ApplyOutcome::Stale);
        let other = HostToPopup::Hide {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "other".into(),
            sequence: 8,
        };
        assert_eq!(state.apply(other), ApplyOutcome::Stale);
        assert_eq!(state.preview.text, "new");

        let mut next_session = preview("next".into());
        if let HostToPopup::Preview {
            session_id,
            sequence,
            ..
        } = &mut next_session
        {
            *session_id = "session-二".into();
            *sequence = 1;
        }
        assert_eq!(state.apply(next_session), ApplyOutcome::Applied);
        let mut retired = preview("retired".into());
        if let HostToPopup::Preview { sequence, .. } = &mut retired {
            *sequence = 99;
        }
        assert_eq!(state.apply(retired), ApplyOutcome::Stale);
        assert_eq!(state.preview.text, "next");
    }

    #[test]
    fn popup_action_guard_rejects_replay_cross_session_and_cross_kind() {
        let mut guard = PopupActionGuard::default();
        let submit = PopupToHost::SubmitQa {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "qa-session".into(),
            sequence: 2,
            text: "question".into(),
        };
        assert!(guard.accept(PopupKind::Qa, &submit, "qa-session"));
        assert!(!guard.accept(PopupKind::Qa, &submit, "qa-session"));

        let stale = PopupToHost::DismissQa {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "qa-session".into(),
            sequence: 1,
        };
        assert!(!guard.accept(PopupKind::Qa, &stale, "qa-session"));
        assert!(!guard.accept(PopupKind::Preview, &submit, "qa-session"));
        assert!(!guard.accept(PopupKind::Qa, &submit, "new-session"));
    }

    #[test]
    fn popup_action_guard_reset_starts_a_new_child_sequence_domain() {
        let mut guard = PopupActionGuard::default();
        let ready = PopupToHost::Ready {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".into(),
            sequence: 1,
            kind: PopupKind::Preview,
        };
        assert!(guard.accept(PopupKind::Preview, &ready, "session"));
        assert!(!guard.accept(PopupKind::Preview, &ready, "session"));
        guard.reset(PopupKind::Preview);
        assert!(guard.accept(PopupKind::Preview, &ready, "session"));
    }

    #[test]
    fn popup_messages_are_bound_to_their_process_kind() {
        assert_eq!(
            preview("text".into()).content_kind(),
            Some(PopupKind::Preview)
        );
        let hide = HostToPopup::Hide {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".into(),
            sequence: 8,
        };
        assert_eq!(hide.content_kind(), None);

        let wrong_version = PopupToHost::DismissCapsule {
            version: POPUP_PROTOCOL_VERSION + 1,
            session_id: "session".into(),
            sequence: 1,
        };
        let mut guard = PopupActionGuard::default();
        assert!(!guard.accept(PopupKind::Capsule, &wrong_version, "session"));
    }

    #[tokio::test]
    async fn supervisor_reaps_a_crashed_child() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("exit 17");
        let supervisor = PopupSupervisor::spawn_command(&Handle::current(), command);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match supervisor.try_recv() {
                Ok(PopupSupervisorEvent::Exited { code, crashed }) => {
                    assert_eq!(code, Some(17));
                    assert!(crashed);
                    break;
                }
                Ok(_) | Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                result => panic!("did not observe crashed child: {result:?}"),
            }
        }
    }
}
