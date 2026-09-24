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

pub const POPUP_PROTOCOL_VERSION: u16 = 7;
pub const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupKind {
    /// 选区助手面板：提问对话 + 润色结果编辑（合并后唯一的选区弹窗）。
    Qa,
    Capsule,
    LessComputer,
}

impl PopupKind {
    pub fn argument(self) -> &'static str {
        match self {
            Self::Qa => "--qa",
            Self::Capsule => "--capsule",
            Self::LessComputer => "--less-computer",
        }
    }
}

/// One rendered Less Computer turn entry.
///
/// Mirrors Core's `LessComputerEventKind` presentation: the panel prints the
/// entries in order and never re-derives product intent, so a new Core event
/// variant only needs a host-side translation into `kind` + display text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessComputerEntry {
    /// `user` / `assistant` / `tool` / `compaction` / `error` / `note`.
    pub kind: String,
    #[serde(default)]
    pub text: String,
}

/// A blocked command waiting for the user's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessComputerApproval {
    pub token: String,
    pub command: String,
    #[serde(default)]
    pub reason: String,
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
    /// 润色结果：送进**选区助手面板**的「润色结果」模式（不再有独立预览窗口）。
    PolishPreview {
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
        /// 「编辑指令」勾选框状态（Core `QaSnapshot.edit_instruction_mode`）。
        #[serde(default)]
        edit_instruction_mode: bool,
        /// 预览可用：底部出现「预览并确认插入」。
        #[serde(default)]
        edit_apply_available: bool,
        /// 可一键回退：额外出现「保留上一版本」。
        #[serde(default)]
        edit_revert_available: bool,
        /// 固定（不自动关闭）。Tauri `qa.pinTooltip` / `qa.unpinTooltip`。
        #[serde(default)]
        pinned: bool,
        /// GitHub 登录名，用于 `https://github.com/{login}.png` 头像。
        #[serde(default)]
        viewer_login: String,
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
        /// 正在翻译：药丸上方显示「正在翻译」徽章（Tauri `capsule.translating`）。
        #[serde(default)]
        translation_active: bool,
        /// 胶囊样式：`siri` / `classic` / `typeless`（Tauri `capsuleStyle`）。
        #[serde(default)]
        style: String,
    },
    Hide {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// Less Computer 面板状态（Tauri `LessComputerPanel.tsx`）。
    ///
    /// `entries` 是已发生的事件序列（用户指令 / 工具 / 压缩 / 助手正文 / 错误），
    /// `working` 表示本轮尚未终结，`approval` 是等待用户批准的阻塞命令。
    LessComputer {
        version: u16,
        session_id: String,
        sequence: u64,
        entries: Vec<LessComputerEntry>,
        #[serde(default)]
        working: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval: Option<LessComputerApproval>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 本地热键匹配所需的配置（见 `crate::local_hotkeys` 的模块文档）。
    ///
    /// 面板有焦点时 fcitx5 收不到按键，所以面板自己按这份配置匹配，命中后
    /// 用 [`PopupToHost::Hotkey`] 报回宿主。
    Hotkeys {
        version: u16,
        session_id: String,
        sequence: u64,
        bindings: Box<openless_core::HotkeyRuntimeTarget>,
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
            Self::PolishPreview { version, .. }
            | Self::QaSnapshot { version, .. }
            | Self::Capsule { version, .. }
            | Self::LessComputer { version, .. }
            | Self::Hotkeys { version, .. }
            | Self::Hide { version, .. }
            | Self::Shutdown { version, .. } => *version,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::PolishPreview { session_id, .. }
            | Self::QaSnapshot { session_id, .. }
            | Self::Capsule { session_id, .. }
            | Self::LessComputer { session_id, .. }
            | Self::Hotkeys { session_id, .. }
            | Self::Hide { session_id, .. }
            | Self::Shutdown { session_id, .. } => session_id,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::PolishPreview { sequence, .. }
            | Self::QaSnapshot { sequence, .. }
            | Self::Capsule { sequence, .. }
            | Self::LessComputer { sequence, .. }
            | Self::Hotkeys { sequence, .. }
            | Self::Hide { sequence, .. }
            | Self::Shutdown { sequence, .. } => *sequence,
        }
    }

    pub fn content_kind(&self) -> Option<PopupKind> {
        match self {
            // 润色结果现在由选区助手面板承载。
            Self::PolishPreview { .. } => Some(PopupKind::Qa),
            Self::QaSnapshot { .. } => Some(PopupKind::Qa),
            Self::Capsule { .. } => Some(PopupKind::Capsule),
            Self::LessComputer { .. } => Some(PopupKind::LessComputer),
            // 配置不属于任何一种面板内容，
            // 不能拿它当“这个帧是给谁的”。
            Self::Hotkeys { .. } => None,
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
    /// 本面板内命中的热键（面板有焦点时 fcitx5 收不到按键）。
    Hotkey {
        version: u16,
        session_id: String,
        sequence: u64,
        edge: crate::LocalHotkeyEdge,
    },
    /// 确认用编辑后的文本替换选区（由选区助手面板发出）。
    ConfirmPolish {
        version: u16,
        session_id: String,
        sequence: u64,
        text: String,
    },
    /// 取消本次润色（由选区助手面板发出）。
    CancelPolish {
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
    /// 胶囊上的 ✕：放弃这次听写（Tauri `cancelDictation`）。
    CancelDictation {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// 胶囊上的 ✓：结束录音并落字（Tauri `stopDictation`）。
    StopDictation {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// 划词追问头部图钉：固定后宿主不再自动收起（Tauri `qa.pinTooltip`）。
    SetPinned {
        version: u16,
        session_id: String,
        sequence: u64,
        pinned: bool,
    },
    /// 输入组左下角「编辑指令」勾选框。
    SetEditInstructionMode {
        version: u16,
        session_id: String,
        sequence: u64,
        enabled: bool,
    },
    /// 「预览并确认插入」：把编辑结果写回选区（Tauri `qa.editApplyReplace`）。
    ApplyEdit {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// 「保留上一版本」：回退这一轮的编辑预览（Tauri `qa.editRevertPrevious`）。
    RevertEdit {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// Less Computer 输入框：提交一条指令（Tauri `lessComputerSubmitText`）。
    SubmitLessComputer {
        version: u16,
        session_id: String,
        sequence: u64,
        text: String,
    },
    /// 批准/拒绝被阻塞的命令（Tauri `lessComputerApprove`）。
    ApproveLessComputer {
        version: u16,
        session_id: String,
        sequence: u64,
        token: String,
        approved: bool,
    },
    /// 停止当前这一轮（Esc / 关闭时的收尾，Tauri `less_computer_window_dismiss`）。
    CancelLessComputer {
        version: u16,
        session_id: String,
        sequence: u64,
    },
    /// ✕：只收起面板，不动已完成的对话（Tauri `cancel()` / `minimize()` 语义）。
    DismissLessComputer {
        version: u16,
        session_id: String,
        sequence: u64,
    },
}

impl PopupToHost {
    pub fn version(&self) -> u16 {
        match self {
            Self::Ready { version, .. }
            | Self::Hotkey { version, .. }
            | Self::ConfirmPolish { version, .. }
            | Self::CancelPolish { version, .. }
            | Self::SubmitQa { version, .. }
            | Self::ToggleQaRecording { version, .. }
            | Self::DismissQa { version, .. }
            | Self::DismissCapsule { version, .. }
            | Self::CancelDictation { version, .. }
            | Self::StopDictation { version, .. }
            | Self::SetPinned { version, .. }
            | Self::SetEditInstructionMode { version, .. }
            | Self::ApplyEdit { version, .. }
            | Self::RevertEdit { version, .. }
            | Self::SubmitLessComputer { version, .. }
            | Self::ApproveLessComputer { version, .. }
            | Self::CancelLessComputer { version, .. }
            | Self::DismissLessComputer { version, .. } => *version,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Ready { session_id, .. }
            | Self::Hotkey { session_id, .. }
            | Self::ConfirmPolish { session_id, .. }
            | Self::CancelPolish { session_id, .. }
            | Self::SubmitQa { session_id, .. }
            | Self::ToggleQaRecording { session_id, .. }
            | Self::DismissQa { session_id, .. }
            | Self::DismissCapsule { session_id, .. }
            | Self::CancelDictation { session_id, .. }
            | Self::StopDictation { session_id, .. }
            | Self::SetPinned { session_id, .. }
            | Self::SetEditInstructionMode { session_id, .. }
            | Self::ApplyEdit { session_id, .. }
            | Self::RevertEdit { session_id, .. }
            | Self::SubmitLessComputer { session_id, .. }
            | Self::ApproveLessComputer { session_id, .. }
            | Self::CancelLessComputer { session_id, .. }
            | Self::DismissLessComputer { session_id, .. } => session_id,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Ready { sequence, .. }
            | Self::Hotkey { sequence, .. }
            | Self::ConfirmPolish { sequence, .. }
            | Self::CancelPolish { sequence, .. }
            | Self::SubmitQa { sequence, .. }
            | Self::ToggleQaRecording { sequence, .. }
            | Self::DismissQa { sequence, .. }
            | Self::DismissCapsule { sequence, .. }
            | Self::CancelDictation { sequence, .. }
            | Self::StopDictation { sequence, .. }
            | Self::SetPinned { sequence, .. }
            | Self::SetEditInstructionMode { sequence, .. }
            | Self::ApplyEdit { sequence, .. }
            | Self::RevertEdit { sequence, .. }
            | Self::SubmitLessComputer { sequence, .. }
            | Self::ApproveLessComputer { sequence, .. }
            | Self::CancelLessComputer { sequence, .. }
            | Self::DismissLessComputer { sequence, .. } => *sequence,
        }
    }

    pub fn kind(&self) -> PopupKind {
        match self {
            Self::Ready { kind, .. } => *kind,
            Self::ConfirmPolish { .. } | Self::CancelPolish { .. } => PopupKind::Qa,
            // 本地热键边沿只由接受键盘的面板发出；`kind()` 的调用方（宿主）
            // 已先按发消息的面板分支处理，这里给 Qa 只是让类型上有个确定值。
            Self::Hotkey { .. } => PopupKind::Qa,
            Self::SubmitQa { .. }
            | Self::ToggleQaRecording { .. }
            | Self::DismissQa { .. }
            | Self::SetPinned { .. }
            | Self::SetEditInstructionMode { .. }
            | Self::ApplyEdit { .. }
            | Self::RevertEdit { .. } => PopupKind::Qa,
            Self::DismissCapsule { .. }
            | Self::CancelDictation { .. }
            | Self::StopDictation { .. } => PopupKind::Capsule,
            Self::SubmitLessComputer { .. }
            | Self::ApproveLessComputer { .. }
            | Self::CancelLessComputer { .. }
            | Self::DismissLessComputer { .. } => PopupKind::LessComputer,
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
    capsule: PopupActionSlot,
    less_computer: PopupActionSlot,
}

impl PopupActionGuard {
    fn slot_mut(&mut self, kind: PopupKind) -> &mut PopupActionSlot {
        match kind {
            PopupKind::Qa => &mut self.qa,
            PopupKind::Capsule => &mut self.capsule,
            PopupKind::LessComputer => &mut self.less_computer,
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
pub struct QaPolishState {
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
    pub edit_instruction_mode: bool,
    /// 润色结果模式：Some = 正在编辑润色结果（替代原来的独立预览窗口）。
    pub polish: Option<QaPolishState>,
    pub edit_apply_available: bool,
    pub edit_revert_available: bool,
    pub pinned: bool,
    pub viewer_login: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CapsulePopupState {
    pub phase: String,
    pub text: String,
    pub audio_level: Option<f32>,
    pub translation_active: bool,
    /// 胶囊样式（`siri` / `classic` / `typeless`）。
    pub style: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LessComputerPopupState {
    pub entries: Vec<LessComputerEntry>,
    pub working: bool,
    pub approval: Option<LessComputerApproval>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PopupState {
    pub session_id: Option<String>,
    pub last_sequence: u64,
    pub visible: bool,
    pub shutdown_requested: bool,
    pub qa: QaPopupState,
    pub capsule: CapsulePopupState,
    pub less_computer: LessComputerPopupState,
    /// 宿主下发的本地热键配置；没有它就不做本地匹配。
    pub hotkeys: Option<openless_core::HotkeyRuntimeTarget>,
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
        // 热键配置不是“会话内容”：它不推进会话、不参与序号排序（宿主可能在面板
        // 刚重连、还没收到第一条内容帧时先发它），所以在这里提前收下就返回。
        if let HostToPopup::Hotkeys { bindings, .. } = message {
            self.hotkeys = Some(*bindings);
            return ApplyOutcome::Applied;
        }
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
                    HostToPopup::PolishPreview { .. }
                        | HostToPopup::QaSnapshot { .. }
                        | HostToPopup::Capsule { .. }
                        | HostToPopup::LessComputer { .. }
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
            HostToPopup::PolishPreview { text, source, .. } => {
                // 润色结果落到选区助手面板的「润色结果」模式。
                self.qa.polish = Some(QaPolishState { text, source });
                self.visible = true;
            }
            HostToPopup::QaSnapshot {
                phase,
                messages,
                selection_preview,
                streaming_answer,
                error,
                edit_instruction_mode,
                edit_apply_available,
                edit_revert_available,
                pinned,
                viewer_login,
                ..
            } => {
                self.qa = QaPopupState {
                    polish: None,
                    phase,
                    messages,
                    selection_preview,
                    streaming_answer,
                    error,
                    edit_instruction_mode,
                    edit_apply_available,
                    edit_revert_available,
                    pinned,
                    viewer_login,
                };
                self.visible = true;
            }
            HostToPopup::Capsule {
                phase,
                text,
                audio_level,
                translation_active,
                style,
                ..
            } => {
                self.capsule = CapsulePopupState {
                    phase,
                    text,
                    audio_level,
                    translation_active,
                    style,
                };
                self.visible = true;
            }
            HostToPopup::LessComputer {
                entries,
                working,
                approval,
                error,
                ..
            } => {
                self.less_computer = LessComputerPopupState {
                    entries,
                    working,
                    approval,
                    error,
                };
                self.visible = true;
            }
            HostToPopup::Hide { .. } => self.visible = false,
            HostToPopup::Shutdown { .. } => {
                self.visible = false;
                self.shutdown_requested = true;
                return ApplyOutcome::Shutdown;
            }
            // 上面已提前收下：热键配置不参与会话与序号排序。
            HostToPopup::Hotkeys { .. } => {}
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

/// Whether this popup must be pushed onto X11 (XWayland counts).
///
/// The recording capsule is a pure overlay: it must sit inside the desktop work
/// area and never take the keyboard from the app being dictated into. When
/// XWayland is available, use it even if layer-shell exists: X11 exposes the
/// taskbar-excluded work area and `WM_HINTS.input = FALSE`, avoiding both the
/// taskbar overlap and the focus race on first map. Native layer-shell remains
/// the fallback on Wayland-only sessions. Typing panels keep their native
/// backend either way.
pub fn force_x11_for(kind: PopupKind, display: Option<&str>) -> bool {
    kind == PopupKind::Capsule && display.is_some_and(|value| !value.trim().is_empty())
}

/// Build the popup child command, including the backend choice above.
pub fn popup_command(
    executable: impl AsRef<Path>,
    kind: PopupKind,
    display: Option<&str>,
) -> Command {
    let mut command = Command::new(executable.as_ref());
    command.arg("--openless-egui-popup").arg(kind.argument());
    if force_x11_for(kind, display) {
        // winit prefers Wayland whenever `WAYLAND_DISPLAY` is set. Dropping it
        // also pins the child's `detect_capsule_path` to the X11 overlay.
        command.env_remove("WAYLAND_DISPLAY");
        command.env_remove("WAYLAND_SOCKET");
    }
    command
}

/// Non-blocking handle held by the main egui application.
pub struct PopupSupervisor {
    commands: tokio_mpsc::Sender<SupervisorCommand>,
    events: Receiver<PopupSupervisorEvent>,
}

impl PopupSupervisor {
    pub fn spawn(runtime: &Handle, executable: impl AsRef<Path>, kind: PopupKind) -> Self {
        let display = std::env::var("DISPLAY").ok();
        // Prefer XWayland when present for the taskbar-excluded work area and
        // pre-map no-focus hint. Without XWayland the child keeps Wayland and
        // uses layer-shell when available.
        let layer_shell = crate::popup_layer::layer_shell_available();
        log::debug!(
            "popup spawn: kind={kind:?} x11={} layer_shell={layer_shell}",
            display.as_deref().unwrap_or("none")
        );
        Self::spawn_command(runtime, popup_command(executable, kind, display.as_deref()))
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

    fn less_computer_snapshot(sequence: u64, text: &str) -> HostToPopup {
        HostToPopup::LessComputer {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".to_string(),
            sequence,
            entries: vec![LessComputerEntry {
                kind: "assistant".to_string(),
                text: text.to_string(),
            }],
            working: true,
            approval: None,
            error: None,
        }
    }

    #[test]
    fn less_computer_snapshots_drive_the_panel_state() {
        // 面板只呈现宿主序列：应用快照后要能看到条目、working 与可见性。
        let mut state = PopupState::default();
        assert_eq!(
            state.apply(less_computer_snapshot(1, "first")),
            ApplyOutcome::Applied
        );
        assert!(state.visible);
        assert!(state.less_computer.working);
        assert_eq!(state.less_computer.entries[0].text, "first");

        // 单调序号：迟到的旧帧不得覆盖新正文。
        assert_eq!(
            state.apply(less_computer_snapshot(2, "first+second")),
            ApplyOutcome::Applied
        );
        assert_eq!(state.less_computer.entries[0].text, "first+second");
        assert_eq!(
            state.apply(less_computer_snapshot(1, "stale")),
            ApplyOutcome::Stale
        );
        assert_eq!(state.less_computer.entries[0].text, "first+second");

        // Hide 只收起面板，不动对话内容（✕ 的语义）。
        assert_eq!(
            state.apply(HostToPopup::Hide {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "session".to_string(),
                sequence: 3,
            }),
            ApplyOutcome::Applied
        );
        assert!(!state.visible);
        assert_eq!(state.less_computer.entries[0].text, "first+second");
    }

    #[test]
    fn less_computer_actions_are_routed_to_their_kind() {
        let submit = PopupToHost::SubmitLessComputer {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".to_string(),
            sequence: 1,
            text: "open the editor".to_string(),
        };
        assert_eq!(submit.kind(), PopupKind::LessComputer);
        assert_eq!(PopupKind::LessComputer.argument(), "--less-computer");
        let approve = PopupToHost::ApproveLessComputer {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".to_string(),
            sequence: 2,
            token: "token".to_string(),
            approved: false,
        };
        assert_eq!(approve.kind(), PopupKind::LessComputer);
    }

    #[test]
    fn capsule_prefers_xwayland_for_work_area_and_no_focus() {
        assert!(force_x11_for(PopupKind::Capsule, Some(":0")));
        assert!(!force_x11_for(PopupKind::Capsule, None));
        assert!(!force_x11_for(PopupKind::Capsule, Some("  ")));
        // Even when layer-shell is advertised, XWayland provides the real
        // work area and an input hint before mapping, so it avoids panels and
        // prevents a lost hotkey release when recording starts.
        assert!(force_x11_for(PopupKind::Capsule, Some(":0")));
        // The panels take keyboard input, so they keep their Wayland windows
        // （选区助手面板现在也承担润色结果，同样是键盘输入的窗口）。
        assert!(!force_x11_for(PopupKind::Qa, Some(":0")));
    }

    #[test]
    fn capsule_command_uses_xwayland_when_display_is_available() {
        let command = popup_command(
            "/usr/bin/openless-linux-egui",
            PopupKind::Capsule,
            Some(":0"),
        );
        let envs: Vec<(String, Option<String>)> = command
            .as_std()
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&("WAYLAND_DISPLAY".to_string(), None)));
        assert!(envs.contains(&("WAYLAND_SOCKET".to_string(), None)));
        let args: Vec<String> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--openless-egui-popup", "--capsule"]);
    }

    /// If XWayland exists, prefer its work-area placement even when layer-shell
    /// is advertised by the compositor.
    #[test]
    fn capsule_command_uses_xwayland_when_layer_shell_is_available() {
        let command = popup_command(
            "/usr/bin/openless-linux-egui",
            PopupKind::Capsule,
            Some(":0"),
        );
        let envs: Vec<_> = command.as_std().get_envs().collect();
        assert!(envs
            .iter()
            .any(|(key, value)| { key == "WAYLAND_DISPLAY" && value.is_none() }));
    }

    #[test]
    fn qa_command_keeps_the_wayland_backend() {
        let command = popup_command("/usr/bin/openless-linux-egui", PopupKind::Qa, Some(":0"));
        assert_eq!(command.as_std().get_envs().count(), 0);
    }

    #[test]
    fn capsule_command_keeps_wayland_without_an_x_server() {
        let command = popup_command("/usr/bin/openless-linux-egui", PopupKind::Capsule, None);
        assert_eq!(command.as_std().get_envs().count(), 0);
    }
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    #[test]
    fn hotkey_bindings_round_trip_and_do_not_disturb_the_session_sequence() {
        let bindings =
            openless_core::HotkeyRuntimeTarget::from(&openless_core::UserPreferences::default());
        let message = HostToPopup::Hotkeys {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "qa".to_owned(),
            sequence: 7,
            bindings: Box::new(bindings),
        };
        let encoded = serde_json::to_string(&message).expect("encode hotkeys");
        let decoded: HostToPopup = serde_json::from_str(&encoded).expect("decode hotkeys");
        assert_eq!(decoded, message);

        let mut state = PopupState::default();
        assert_eq!(state.apply(message), ApplyOutcome::Applied);
        assert!(state.hotkeys.is_some());
        // 配置不是会话内容：后面的内容帧序号照常从 1 开始被采纳。
        assert_eq!(state.session_id, None);
        assert_eq!(state.last_sequence, 0);
        let mut content = polish_preview("x".to_owned());
        if let HostToPopup::PolishPreview { sequence, .. } = &mut content {
            *sequence = 1;
        }
        assert_eq!(state.apply(content), ApplyOutcome::Applied);
    }

    #[test]
    fn a_local_hotkey_edge_round_trips_from_the_popup() {
        let message = PopupToHost::Hotkey {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "qa".to_owned(),
            sequence: 3,
            edge: crate::LocalHotkeyEdge {
                hotkey: crate::LocalHotkey::Dictation,
                kind: crate::LocalHotkeyEdgeKind::Pressed,
                press_id: crate::local_hotkeys::LOCAL_PRESS_ID_BASE + 1,
            },
        };
        let encoded = serde_json::to_string(&message).expect("encode edge");
        let decoded: PopupToHost = serde_json::from_str(&encoded).expect("decode edge");
        assert_eq!(decoded, message);
        assert_eq!(decoded.session_id(), "qa");
        assert_eq!(decoded.sequence(), 3);
        assert_eq!(decoded.kind(), PopupKind::Qa);
    }

    fn polish_preview(text: String) -> HostToPopup {
        HostToPopup::PolishPreview {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session-一".to_owned(),
            sequence: 7,
            text,
            source: "原文 \\\\ source".to_owned(),
        }
    }

    #[test]
    fn qa_snapshot_carries_pin_and_edit_state() {
        let message = HostToPopup::QaSnapshot {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "qa".to_owned(),
            sequence: 11,
            phase: "IDLE".to_owned(),
            messages: Vec::new(),
            selection_preview: None,
            streaming_answer: String::new(),
            error: None,
            edit_instruction_mode: true,
            edit_apply_available: true,
            edit_revert_available: false,
            pinned: true,
            viewer_login: "octocat".to_owned(),
        };
        let mut state = PopupState::default();
        assert_eq!(state.apply(message.clone()), ApplyOutcome::Applied);
        assert!(state.qa.edit_instruction_mode);
        assert!(state.qa.edit_apply_available);
        assert!(!state.qa.edit_revert_available);
        assert!(state.qa.pinned);
        assert_eq!(state.qa.viewer_login, "octocat");

        // 老宿主（协议 v2）没有这些字段时保持默认值，而不是解析失败。
        let legacy = r#"{"type":"qa_snapshot","version":2,"session_id":"qa","sequence":12,"phase":"IDLE","messages":[],"streaming_answer":""}"#;
        let legacy: HostToPopup = serde_json::from_str(legacy).expect("legacy snapshot");
        let mut state = PopupState::default();
        assert_eq!(state.apply(legacy), ApplyOutcome::Applied);
        assert!(!state.qa.pinned);
        assert!(state.qa.viewer_login.is_empty());
    }

    #[test]
    fn capsule_carries_translation_active_and_style() {
        let message = HostToPopup::Capsule {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "dictation".to_owned(),
            sequence: 3,
            phase: "Recording".to_owned(),
            text: String::new(),
            audio_level: Some(0.5),
            translation_active: true,
            style: "typeless".to_owned(),
        };
        let mut state = PopupState::default();
        assert_eq!(state.apply(message), ApplyOutcome::Applied);
        assert!(state.capsule.translation_active);
        assert_eq!(
            state.capsule.style, "typeless",
            "the capsule style must travel with the frame to the popup process"
        );
    }

    #[test]
    fn a_legacy_capsule_frame_without_a_style_still_applies() {
        // 协议 v4 的帧没有 style 字段：必须按默认（siri）应用，而不是整帧丢弃。
        let legacy = r#"{"type":"capsule","version":4,"session_id":"dictation","sequence":7,"phase":"Recording","text":"","audio_level":0.3,"translation_active":false}"#;
        let legacy: HostToPopup = serde_json::from_str(legacy).expect("legacy capsule frame");
        let mut state = PopupState::default();
        assert_eq!(state.apply(legacy), ApplyOutcome::Applied);
        assert_eq!(state.capsule.style, "");
    }

    #[test]
    fn qa_actions_are_scoped_to_the_qa_popup_and_accepted_once() {
        for message in [
            PopupToHost::SetPinned {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".to_owned(),
                sequence: 1,
                pinned: true,
            },
            PopupToHost::SetEditInstructionMode {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".to_owned(),
                sequence: 2,
                enabled: true,
            },
            PopupToHost::ApplyEdit {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".to_owned(),
                sequence: 3,
            },
            PopupToHost::RevertEdit {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".to_owned(),
                sequence: 4,
            },
        ] {
            assert_eq!(message.kind(), PopupKind::Qa);
            let mut guard = PopupActionGuard::default();
            assert!(guard.accept(PopupKind::Qa, &message, "qa"));
            // 同一个 sequence 不能重复执行。
            assert!(!guard.accept(PopupKind::Qa, &message, "qa"));
            // 其它弹窗进程的同一 sequence 不受影响（各自独立）。
            let mut capsule = PopupActionGuard::default();
            assert!(!capsule.accept(PopupKind::Capsule, &message, "qa"));
        }
    }

    #[test]
    fn jsonl_round_trip_escapes_quotes_backslashes_and_unicode() {
        let expected = polish_preview("他说：\"你好\" C:\\\\tmp\\\\文件".to_owned());
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
        assert_eq!(
            state.apply(polish_preview("new".into())),
            ApplyOutcome::Applied
        );
        let mut late = polish_preview("late".into());
        if let HostToPopup::PolishPreview { sequence, .. } = &mut late {
            *sequence = 6;
        }
        assert_eq!(state.apply(late), ApplyOutcome::Stale);
        let other = HostToPopup::Hide {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "other".into(),
            sequence: 8,
        };
        assert_eq!(state.apply(other), ApplyOutcome::Stale);
        assert_eq!(
            state
                .qa
                .polish
                .as_ref()
                .map(|p| p.text.clone())
                .unwrap_or_default(),
            "new"
        );

        let mut next_session = polish_preview("next".into());
        if let HostToPopup::PolishPreview {
            session_id,
            sequence,
            ..
        } = &mut next_session
        {
            *session_id = "session-二".into();
            *sequence = 1;
        }
        assert_eq!(state.apply(next_session), ApplyOutcome::Applied);
        let mut retired = polish_preview("retired".into());
        if let HostToPopup::PolishPreview { sequence, .. } = &mut retired {
            *sequence = 99;
        }
        assert_eq!(state.apply(retired), ApplyOutcome::Stale);
        assert_eq!(
            state
                .qa
                .polish
                .as_ref()
                .map(|p| p.text.clone())
                .unwrap_or_default(),
            "next"
        );
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
        assert!(!guard.accept(PopupKind::Qa, &submit, "qa-session"));
        assert!(!guard.accept(PopupKind::Qa, &submit, "new-session"));
    }

    #[test]
    fn popup_action_guard_reset_starts_a_new_child_sequence_domain() {
        let mut guard = PopupActionGuard::default();
        let ready = PopupToHost::Ready {
            version: POPUP_PROTOCOL_VERSION,
            session_id: "session".into(),
            sequence: 1,
            kind: PopupKind::Qa,
        };
        assert!(guard.accept(PopupKind::Qa, &ready, "session"));
        assert!(!guard.accept(PopupKind::Qa, &ready, "session"));
        guard.reset(PopupKind::Qa);
        assert!(guard.accept(PopupKind::Qa, &ready, "session"));
    }

    #[test]
    fn popup_messages_are_bound_to_their_process_kind() {
        assert_eq!(
            polish_preview("text".into()).content_kind(),
            Some(PopupKind::Qa)
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
    async fn shutdown_ends_the_popup_process_so_nothing_is_left_on_screen() {
        // 自动收起（听写终态 2 秒/取消立即）靠的是结束弹窗进程：胶囊的
        // layer surface 只能随进程销毁，进程留着就会有一颗药丸永远贴屏。
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 30");
        let supervisor = PopupSupervisor::spawn_command(&Handle::current(), command);
        supervisor
            .request_shutdown()
            .expect("a fresh supervisor accepts shutdown");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match supervisor.try_recv() {
                Ok(PopupSupervisorEvent::Exited { crashed, .. }) => {
                    assert!(!crashed, "a requested shutdown must not look like a crash");
                    break;
                }
                Ok(_) | Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                result => panic!("popup process survived shutdown: {result:?}"),
            }
        }
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
