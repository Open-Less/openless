//! UI-independent capsule messages. The Tauri host does not enable `ui`, so it
//! does not link egui, a graphics driver, or a second native event loop.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

/// v2: the default (OpenLess) capsule needs warming/operating/inserted counts,
/// the translation target, theme, the leaving (exit) frame and the full label set.
pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

#[cfg(feature = "ui")]
mod icons;
#[cfg(feature = "ui")]
mod layout;
#[cfg(feature = "ui")]
mod motion;
#[cfg(feature = "ui")]
pub mod pill;
#[cfg(feature = "ui")]
pub mod renderer;
#[cfg(feature = "ui")]
mod shapes;
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
    /// Optimistically shown before the first microphone frame.
    #[serde(default)]
    pub warming: bool,
    /// Less Computer session: processing reads "using" instead of "thinking".
    #[serde(default)]
    pub operating: bool,
    #[serde(default)]
    pub inserted_chars: Option<u32>,
    /// Selection polish shares the capsule window but shows a one-line notice.
    #[serde(default)]
    pub selection_polish: bool,
    /// Display name of the translation target, already localized by the host.
    #[serde(default)]
    pub translation_target: Option<String>,
}

/// Copy shared with the WebView capsule (`src/i18n/*.ts`, `capsule.*`).
/// `inserted` keeps the i18next `{{count}}` placeholder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapsuleLabels {
    pub thinking: String,
    pub using: String,
    pub cancelled: String,
    pub error: String,
    pub inserted: String,
    pub translate_to: String,
    pub translating: String,
    pub polishing: String,
    pub replaced: String,
    pub no_selection: String,
    pub polish_failed: String,
}

impl Default for CapsuleLabels {
    fn default() -> Self {
        Self::for_locale("en")
    }
}

impl CapsuleLabels {
    pub fn for_locale(locale: &str) -> Self {
        let labels: [&str; 11] = if locale.starts_with("zh-TW") || locale.starts_with("zh-HK") {
            [
                "thinking",
                "using",
                "已取消",
                "出錯了",
                "已插入 {{count}}",
                "翻譯為",
                "正在翻譯",
                "正在潤色...",
                "已替換",
                "未選中內容",
                "潤色失敗，請重試",
            ]
        } else if locale.starts_with("zh") {
            [
                "thinking",
                "using",
                "已取消",
                "出错了",
                "已插入 {{count}}",
                "翻译为",
                "正在翻译",
                "正在润色...",
                "已替换",
                "未选中内容",
                "润色失败，请重试",
            ]
        } else if locale.starts_with("ja") {
            [
                "thinking",
                "using",
                "キャンセルしました",
                "エラーが発生しました",
                "{{count}} 文字を入力しました",
                "翻訳先",
                "翻訳中",
                "推敲中...",
                "置き換えました",
                "選択されていません",
                "推敲に失敗しました。もう一度お試しください",
            ]
        } else if locale.starts_with("ko") {
            [
                "thinking",
                "using",
                "취소됨",
                "오류 발생",
                "{{count}}자 입력됨",
                "번역 언어",
                "번역 중",
                "다듬는 중...",
                "교체됨",
                "선택된 내용 없음",
                "다듬기 실패, 다시 시도하세요",
            ]
        } else if locale.starts_with("es") {
            [
                "pensando",
                "actuando",
                "Cancelado",
                "Se ha producido un error",
                "{{count}} insertados",
                "Traducir al",
                "Traduciendo",
                "Mejorando el texto…",
                "Reemplazado",
                "No hay texto seleccionado",
                "No se pudo mejorar el texto. Inténtalo de nuevo",
            ]
        } else if locale.starts_with("fr") {
            [
                "réflexion",
                "action",
                "Annulé",
                "Une erreur s’est produite",
                "{{count}} insérés",
                "Traduire en",
                "Traduction",
                "Amélioration du texte…",
                "Remplacé",
                "Aucun texte sélectionné",
                "L’amélioration a échoué. Réessayez",
            ]
        } else if locale.starts_with("de") {
            [
                "denkt nach",
                "verwendet",
                "Abgebrochen",
                "Ein Fehler ist aufgetreten",
                "{{count}} eingefügt",
                "Übersetzen in",
                "Wird übersetzt",
                "Wird überarbeitet…",
                "Ersetzt",
                "Kein Text ausgewählt",
                "Überarbeitung fehlgeschlagen. Erneut versuchen",
            ]
        } else {
            [
                "thinking",
                "using",
                "Cancelled",
                "Something went wrong",
                "Inserted {{count}}",
                "Translate to",
                "Translating",
                "Polishing…",
                "Replaced",
                "Nothing selected",
                "Polish failed, try again",
            ]
        };
        let [thinking, using, cancelled, error, inserted, translate_to, translating, polishing, replaced, no_selection, polish_failed] =
            labels.map(String::from);
        Self {
            thinking,
            using,
            cancelled,
            error,
            inserted,
            translate_to,
            translating,
            polishing,
            replaced,
            no_selection,
            polish_failed,
        }
    }
}

/// The app's theme preference; `System` is resolved by the helper itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub session_id: String,
    /// Presentation epoch, independent of the Core's event sequence.
    pub sequence: u64,
    pub state: CapsulePopupState,
    /// Live recognition text for this session. The rail only shows it when
    /// `show_transcript` is set; the processing label may still use it, as the
    /// WebView capsule does.
    #[serde(default)]
    pub transcript: String,
    #[serde(default)]
    pub show_transcript: bool,
    #[serde(default = "default_transcript_font_size")]
    pub transcript_font_size: f32,
    #[serde(default)]
    pub labels: CapsuleLabels,
    #[serde(default)]
    pub theme: ThemeMode,
    /// The last visible frame, replayed while the helper plays the exit motion.
    /// The host stops the helper once the motion has finished.
    #[serde(default)]
    pub leaving: bool,
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
        if self.leaving
            || self.session_id != session_id
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
            && !self.leaving
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
    /// Turn the last visible frame into its exit frame: no controls, no rail.
    pub fn into_leaving(mut self, sequence: u64) -> Self {
        self.leaving = true;
        self.interactive = false;
        self.sequence = self.sequence.max(sequence);
        self.transcript.clear();
        self
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
            show_transcript: true,
            transcript_font_size: 15.0,
            labels: Default::default(),
            theme: ThemeMode::System,
            leaving: false,
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
    fn a_leaving_frame_keeps_its_look_but_drops_controls_rail_and_late_text() {
        let mut leaving = frame();
        leaving.transcript = "spoken".into();
        let mut leaving = leaving.into_leaving(50);
        assert!(leaving.visible() && leaving.leaving && !leaving.interactive);
        assert_eq!((leaving.sequence, leaving.transcript.as_str()), (50, ""));
        assert!(!leaving.accepts("new", 42, Action::Cancel));
        assert!(!leaving.update_transcript("new", "late".into()));
        // An exit can never move the presentation epoch backwards.
        assert_eq!(frame().into_leaving(7).sequence, 42);
    }
    #[test]
    fn v1_frames_without_new_fields_still_parse_with_safe_defaults() {
        let frame: Frame = serde_json::from_str(
            r#"{"session_id":"s","sequence":1,"state":{"phase":"recording","text":"","audio_level":0.2,"translation_active":false,"style":"classic"},"bounds":null,"interactive":true}"#,
        )
        .unwrap();
        assert!(!frame.leaving && !frame.state.warming && !frame.show_transcript);
        assert_eq!(frame.theme, ThemeMode::System);
        assert_eq!(frame.labels, CapsuleLabels::for_locale("en"));
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

    /// Source contract: the helper repeats the WebView's `capsule.*` copy, so a
    /// change to the i18n files must be mirrored in `CapsuleLabels::for_locale`.
    #[test]
    fn labels_match_the_webview_i18n_sources() {
        fn value(block: &str, key: &str) -> String {
            let start = block
                .find(&format!("\n    {key}: "))
                .or_else(|| block.find(&format!("\n      {key}: ")))
                .unwrap_or_else(|| panic!("missing capsule.{key}"));
            let rest = &block[start..];
            let quote = rest.find('\'').unwrap() + 1;
            let end = quote + rest[quote..].find("',").unwrap();
            rest[quote..end].to_string()
        }
        for (file, locale) in [
            ("en", "en"),
            ("zh-CN", "zh-CN"),
            ("zh-TW", "zh-TW"),
            ("ja", "ja"),
            ("ko", "ko"),
            ("es", "es"),
            ("fr", "fr"),
            ("de", "de"),
        ] {
            let path = format!("{}/../../src/i18n/{file}.ts", env!("CARGO_MANIFEST_DIR"));
            let source = std::fs::read_to_string(&path).unwrap();
            let start = source.find("\n  capsule: {").expect("capsule block");
            let block = &source[start..start + source[start..].find("\n  },").unwrap()];
            let labels = CapsuleLabels::for_locale(locale);
            for (key, actual) in [
                ("thinking", &labels.thinking),
                ("using", &labels.using),
                ("cancelled", &labels.cancelled),
                ("error", &labels.error),
                ("inserted", &labels.inserted),
                ("translateTo", &labels.translate_to),
                ("translating", &labels.translating),
                ("polishing", &labels.polishing),
                ("replaced", &labels.replaced),
                ("noSelection", &labels.no_selection),
                ("failed", &labels.polish_failed),
            ] {
                assert_eq!(&value(block, key), actual, "{file}: capsule.{key}");
            }
        }
    }
}
