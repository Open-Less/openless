//! UI-independent capsule messages. The Tauri host does not enable `ui`, so it
//! does not link egui, a graphics driver, or a second native event loop.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

#[cfg(feature = "ui")]
mod icons;
#[cfg(feature = "ui")]
mod layout;
#[cfg(feature = "ui")]
pub mod renderer;
#[cfg(feature = "ui")]
pub mod siri_gl;
#[cfg(feature = "host")]
pub mod supervisor;
#[cfg(feature = "ui")]
mod theme;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapsulePopupState {
    pub phase: String,
    pub text: String,
    pub audio_level: Option<f32>,
    pub translation_active: bool,
    pub style: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapsuleLabels {
    pub thinking: String,
    pub cancelled: String,
    pub error: String,
    pub translating: String,
}

impl Default for CapsuleLabels {
    fn default() -> Self {
        Self::for_locale("en")
    }
}

impl CapsuleLabels {
    pub fn for_locale(locale: &str) -> Self {
        let (thinking, cancelled, error, translating) =
            if locale.starts_with("zh-TW") || locale.starts_with("zh-HK") {
                ("正在處理", "已取消", "處理失敗", "正在翻譯")
            } else if locale.starts_with("zh") {
                ("正在处理", "已取消", "处理失败", "正在翻译")
            } else if locale.starts_with("ja") {
                ("処理中", "キャンセル済み", "処理に失敗しました", "翻訳中")
            } else if locale.starts_with("ko") {
                ("처리 중", "취소됨", "처리 실패", "번역 중")
            } else if locale.starts_with("es") {
                (
                    "pensando",
                    "Cancelado",
                    "Se ha producido un error",
                    "Traduciendo",
                )
            } else if locale.starts_with("fr") {
                (
                    "réflexion",
                    "Annulé",
                    "Une erreur s’est produite",
                    "Traduction",
                )
            } else if locale.starts_with("de") {
                (
                    "denkt nach",
                    "Abgebrochen",
                    "Ein Fehler ist aufgetreten",
                    "Wird übersetzt",
                )
            } else {
                ("Processing", "Cancelled", "Failed", "Translating")
            };
        Self {
            thinking: thinking.into(),
            cancelled: cancelled.into(),
            error: error.into(),
            translating: translating.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub session_id: String,
    /// Presentation epoch, independent of the Core's event sequence.
    pub sequence: u64,
    pub state: CapsulePopupState,
    #[serde(default)]
    pub transcript: String,
    #[serde(default = "default_transcript_font_size")]
    pub transcript_font_size: f32,
    #[serde(default)]
    pub labels: CapsuleLabels,
    /// Native pixels in the global virtual desktop; chosen by the host before
    /// the helper is shown, so the helper never has to activate itself.
    pub bounds: Option<[i32; 4]>,
    pub interactive: bool,
}
fn default_transcript_font_size() -> f32 {
    15.0
}

impl Frame {
    pub fn visible(&self) -> bool {
        self.state.phase != "idle"
    }
    pub fn update_transcript(&mut self, session_id: &str, text: String) -> bool {
        if self.session_id != session_id
            || !matches!(
                self.state.phase.as_str(),
                "recording" | "transcribing" | "polishing"
            )
        {
            return false;
        }
        self.transcript = text;
        true
    }
    pub fn accepts(&self, session_id: &str, sequence: u64, action: Action) -> bool {
        self.interactive
            && self.session_id == session_id
            && sequence <= self.sequence
            && match action {
                Action::Confirm => self.state.phase == "recording",
                Action::Cancel => matches!(
                    self.state.phase.as_str(),
                    "recording" | "transcribing" | "polishing" | "inserting"
                ),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Cancel,
    Confirm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Frame { version: u16, frame: Box<Frame> },
    Shutdown { version: u16 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiMessage {
    Ready {
        version: u16,
    },
    Action {
        version: u16,
        session_id: String,
        sequence: u64,
        action: Action,
    },
}

/// Read at most one bounded JSONL message, even from a corrupt or mismatched
/// helper. EOF is distinct from a malformed/oversized frame.
pub fn read_message<T: serde::de::DeserializeOwned>(
    reader: &mut impl BufRead,
) -> io::Result<Option<T>> {
    let mut bytes = Vec::new();
    let count = std::io::Read::take(&mut *reader, (MAX_LINE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_LINE_BYTES || bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized or incomplete capsule message",
        ));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
pub fn write_message(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() >= MAX_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized capsule message",
        ));
    }
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(feature = "ui")]
pub fn run_pass(
    ctx: &egui::Context,
    input: egui::RawInput,
    mut draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, |ui| draw(ui));
    output.textures_delta.clear();
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Frame {
        Frame {
            session_id: "new".into(),
            sequence: 42,
            state: CapsulePopupState {
                phase: "recording".into(),
                ..Default::default()
            },
            transcript: String::new(),
            transcript_font_size: 15.0,
            labels: Default::default(),
            bounds: None,
            interactive: true,
        }
    }
    #[test]
    fn delayed_buttons_cannot_cancel_a_new_session_or_a_completed_session() {
        let mut frame = frame();
        assert!(frame.accepts("new", 42, Action::Cancel));
        assert!(!frame.accepts("old", 42, Action::Cancel));
        assert!(frame.accepts("new", 41, Action::Cancel));
        assert!(!frame.accepts("new", 43, Action::Cancel));
        frame.state.phase = "transcribing".into();
        assert!(!frame.accepts("new", 42, Action::Confirm));
        frame.state.phase = "completed".into();
        assert!(!frame.accepts("new", 42, Action::Cancel));
    }
    #[test]
    fn transcript_updates_without_volume_frames_but_never_crosses_sessions_or_terminal_states() {
        let mut frame = frame();
        frame.state.phase = "transcribing".into();
        assert!(frame.update_transcript("new", "latest text".into()));
        assert!(!frame.update_transcript("old", "stale text".into()));
        assert_eq!(frame.transcript, "latest text");
        frame.state.phase = "completed".into();
        assert!(!frame.update_transcript("new", "late text".into()));
        assert_eq!(frame.transcript, "latest text");
    }
    #[test]
    fn jsonl_rejects_oversized_or_unterminated_frames() {
        assert!(
            read_message::<HostMessage>(&mut io::Cursor::new(vec![b'x'; MAX_LINE_BYTES + 2]))
                .is_err()
        );
        assert!(read_message::<HostMessage>(&mut io::Cursor::new(b"{}".to_vec())).is_err());
        assert!(
            read_message::<HostMessage>(&mut io::Cursor::new(Vec::<u8>::new()))
                .unwrap()
                .is_none()
        );
    }
}
