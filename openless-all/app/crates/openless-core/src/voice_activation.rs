//! Pure policy and matching primitives for voice-activated dictation.
//!
//! The host owns the microphone and local recognizer.  This module deliberately
//! only consumes revised recognition text and produces command decisions, so a
//! standby listener can never accidentally publish normal transcript events.

use std::time::Duration;

use unicode_normalization::UnicodeNormalization;

use crate::types::SessionId;

pub const DEFAULT_WAKE_PHRASE: &str = "开始语音录入";
pub const DEFAULT_FINISH_PHRASE: &str = "完成语音录入";
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 30;
pub const MIN_TIMEOUT_SECONDS: u32 = 5;
pub const MAX_TIMEOUT_SECONDS: u32 = 300;
pub const MAX_PHRASE_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhraseValidationError {
    Empty,
    TooLong { max_chars: usize },
    ControlCharacter,
}

impl std::fmt::Display for PhraseValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "voice activation phrase must not be empty"),
            Self::TooLong { max_chars } => {
                write!(
                    f,
                    "voice activation phrase must be at most {max_chars} characters"
                )
            }
            Self::ControlCharacter => {
                write!(
                    f,
                    "voice activation phrase must not contain control characters"
                )
            }
        }
    }
}

impl std::error::Error for PhraseValidationError {}

/// Normalize ASR output and user phrases before suffix matching.
///
/// NFKC handles full-width punctuation and compatibility forms.  Whitespace
/// and punctuation are intentionally removed because streaming ASR may insert
/// them between Chinese command characters while English recognition may emit
/// multiple spaces or hyphens.
pub fn normalize_phrase(text: &str) -> String {
    text.nfkc()
        .filter(|character| !character.is_whitespace() && !is_terminal_punctuation(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Remove a recognized command from the end of a transcript while preserving
/// the spacing and punctuation in the spoken body.
pub fn strip_phrase_suffix(text: &str, phrase: &str) -> Option<String> {
    let normalized_phrase = validate_phrase(phrase).ok()?;
    let phrase_chars = normalized_phrase.chars().rev();
    let mut cursor = text.len();
    let mut cut_at = cursor;
    for expected in phrase_chars {
        while cursor > 0 {
            let previous = text[..cursor].char_indices().next_back()?;
            if previous.1.is_whitespace() || is_terminal_punctuation(previous.1) {
                cursor = previous.0;
                continue;
            }
            break;
        }
        let (start, character) = text[..cursor].char_indices().next_back()?;
        if normalize_phrase(&character.to_string()) != expected.to_string() {
            return None;
        }
        cut_at = start;
        cursor = start;
    }
    Some(text[..cut_at].trim_end().to_string())
}

/// Remove a recognized command from the beginning of a transcript while preserving
/// the spoken body. Only a true prefix match is accepted.
pub fn strip_phrase_prefix(text: &str, phrase: &str) -> Option<String> {
    let normalized_phrase = validate_phrase(phrase).ok()?;
    let mut cursor = 0;
    for expected in normalized_phrase.chars() {
        loop {
            let (_, character) = text[cursor..].char_indices().next()?;
            cursor += character.len_utf8();
            if character.is_whitespace() || is_terminal_punctuation(character) {
                continue;
            }
            if normalize_phrase(&character.to_string()) != expected.to_string() {
                return None;
            }
            break;
        }
    }
    while cursor < text.len() {
        let character = text[cursor..].chars().next()?;
        if !character.is_whitespace() && !is_terminal_punctuation(character) {
            break;
        }
        cursor += character.len_utf8();
    }
    Some(text[cursor..].to_string())
}

fn is_terminal_punctuation(character: char) -> bool {
    character.to_string().nfkc().any(|normalized| {
        normalized.is_ascii_punctuation()
            || matches!(
                normalized,
                '。' | '！'
                    | '？'
                    | '，'
                    | '、'
                    | '；'
                    | '：'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '（'
                    | '）'
                    | '【'
                    | '】'
                    | '《'
                    | '》'
                    | '…'
                    | '～'
                    | '—'
                    | '–'
            )
    })
}

pub fn validate_phrase(text: &str) -> Result<String, PhraseValidationError> {
    let normalized = normalize_phrase(text);
    if normalized.is_empty() {
        return Err(PhraseValidationError::Empty);
    }
    if normalized.chars().any(char::is_control) {
        return Err(PhraseValidationError::ControlCharacter);
    }
    if normalized.chars().count() > MAX_PHRASE_CHARS {
        return Err(PhraseValidationError::TooLong {
            max_chars: MAX_PHRASE_CHARS,
        });
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceActivationConfig {
    pub enabled: bool,
    pub wake_enabled: bool,
    pub finish_enabled: bool,
    pub wake_phrase: String,
    pub finish_phrase: String,
    pub timeout_enabled: bool,
    pub timeout_seconds: u32,
    /// Runtime-only selected input; never serialized as a user preference field.
    pub microphone_device_name: Option<String>,
    /// Runtime-owned generation; this is not persisted in preferences.
    pub activation_generation: u64,
}

impl Default for VoiceActivationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            wake_enabled: true,
            finish_enabled: true,
            wake_phrase: DEFAULT_WAKE_PHRASE.into(),
            finish_phrase: DEFAULT_FINISH_PHRASE.into(),
            timeout_enabled: true,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            microphone_device_name: None,
            activation_generation: 0,
        }
    }
}

impl VoiceActivationConfig {
    pub fn normalized(&self) -> Result<Self, PhraseValidationError> {
        let wake_phrase = validate_phrase(&self.wake_phrase)?;
        let finish_phrase = validate_phrase(&self.finish_phrase)?;
        Ok(Self {
            enabled: self.enabled,
            wake_enabled: self.wake_enabled,
            finish_enabled: self.finish_enabled,
            wake_phrase,
            finish_phrase,
            timeout_enabled: self.timeout_enabled,
            timeout_seconds: self
                .timeout_seconds
                .clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS),
            microphone_device_name: self.microphone_device_name.clone(),
            activation_generation: self.activation_generation,
        })
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_enabled
            .then(|| Duration::from_secs(u64::from(self.timeout_seconds)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceActivationCommand {
    Wake,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCommandMatch {
    pub command: VoiceActivationCommand,
    pub text_without_command: String,
}

/// Matches only the end of the revised ASR text.  A phrase in the middle of a
/// sentence must never trigger activation or stop an active dictation session.
#[derive(Debug, Clone)]
pub struct VoicePhraseMatcher {
    normalized_phrase: String,
    command: VoiceActivationCommand,
}

impl VoicePhraseMatcher {
    pub fn new(
        phrase: impl AsRef<str>,
        command: VoiceActivationCommand,
    ) -> Result<Self, PhraseValidationError> {
        Ok(Self {
            normalized_phrase: validate_phrase(phrase.as_ref())?,
            command,
        })
    }

    pub fn matches_suffix(&self, text: &str) -> bool {
        normalize_phrase(text).ends_with(&self.normalized_phrase)
    }

    pub fn strip_suffix(&self, text: &str) -> Option<String> {
        let normalized = normalize_phrase(text);
        normalized
            .strip_suffix(&self.normalized_phrase)
            .map(str::to_string)
    }

    pub fn command(&self) -> VoiceActivationCommand {
        self.command
    }
}

/// Handles ASR revisions.  A revised result may replace a previous result,
/// therefore a command is reported at most once for each observed text value.
#[derive(Debug, Clone)]
pub struct VoiceCommandRecognizer {
    wake: VoicePhraseMatcher,
    finish: VoicePhraseMatcher,
    last_text: String,
    wake_reported: bool,
    finish_reported: bool,
}

impl VoiceCommandRecognizer {
    pub fn new(config: &VoiceActivationConfig) -> Result<Self, PhraseValidationError> {
        let config = config.normalized()?;
        Ok(Self {
            wake: VoicePhraseMatcher::new(config.wake_phrase, VoiceActivationCommand::Wake)?,
            finish: VoicePhraseMatcher::new(config.finish_phrase, VoiceActivationCommand::Finish)?,
            last_text: String::new(),
            wake_reported: false,
            finish_reported: false,
        })
    }

    pub fn observe(
        &mut self,
        text: &str,
        command: VoiceActivationCommand,
    ) -> Option<VoiceCommandMatch> {
        let normalized = normalize_phrase(text);
        if normalized == self.last_text {
            return None;
        }
        self.last_text = normalized.clone();
        let matcher = match command {
            VoiceActivationCommand::Wake => &self.wake,
            VoiceActivationCommand::Finish => &self.finish,
        };
        let already_reported = match command {
            VoiceActivationCommand::Wake => self.wake_reported,
            VoiceActivationCommand::Finish => self.finish_reported,
        };
        if already_reported || !normalized.ends_with(&matcher.normalized_phrase) {
            return None;
        }
        match command {
            VoiceActivationCommand::Wake => self.wake_reported = true,
            VoiceActivationCommand::Finish => self.finish_reported = true,
        }
        Some(VoiceCommandMatch {
            command,
            text_without_command: normalized
                .strip_suffix(&matcher.normalized_phrase)
                .unwrap_or_default()
                .to_string(),
        })
    }

    pub fn reset(&mut self) {
        self.last_text.clear();
        self.wake_reported = false;
        self.finish_reported = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceActivationSignal {
    WakeDetected {
        activation_generation: u64,
    },
    FinishDetected {
        activation_generation: u64,
        session_id: SessionId,
    },
    RuntimeStateChanged {
        activation_generation: u64,
        phase: VoiceActivationPhase,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceActivationPhase {
    Disabled,
    Starting,
    ListeningForWake,
    WakeDetected,
    HandingOff,
    ListeningForFinish,
    Finishing,
    Unavailable,
    PermissionDenied,
    MicBusy,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceActivationStateSnapshot {
    pub phase: VoiceActivationPhase,
    pub activation_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceActivationStopReason {
    Disabled,
    DictationStarted,
    FinishPhrase,
    Timeout,
    Cancelled,
    RuntimeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceActivationGeneration(pub u64);

impl VoiceActivationGeneration {
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Commands frozen for a voice-started session; ordinary dictation has no policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceActivationTextPolicy {
    pub wake_phrase: String,
    pub finish_phrase: Option<String>,
}

impl VoiceActivationTextPolicy {
    pub fn clean(&self, text: &str) -> String {
        let body = strip_phrase_prefix(text, &self.wake_phrase).unwrap_or_else(|| text.to_owned());
        self.finish_phrase
            .as_ref()
            .and_then(|phrase| strip_phrase_suffix(&body, phrase))
            .unwrap_or(body)
    }
}

#[derive(Debug, Clone)]
pub struct VoiceActivationSession {
    pub generation: VoiceActivationGeneration,
    pub session_id: SessionId,
    pub timeout: Option<Duration>,
    pub commands: VoiceActivationTextPolicy,
}

impl VoiceActivationSession {
    pub fn accepts(&self, generation: VoiceActivationGeneration, session_id: SessionId) -> bool {
        self.generation == generation && self.session_id == session_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceActivationDecision {
    Ignore,
    StartDictation,
    FinishDictation,
    StopAfterTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_handles_spaces_width_case_and_punctuation() {
        assert_eq!(normalize_phrase(" Ａ 开始 语音录入 "), "a开始语音录入");
        assert_eq!(normalize_phrase("完成\n语音\t录入"), DEFAULT_FINISH_PHRASE);
        assert_eq!(normalize_phrase("完成，语音录入。"), DEFAULT_FINISH_PHRASE);
    }

    #[test]
    fn strip_phrase_prefix_preserves_body_and_requires_prefix() {
        assert_eq!(
            strip_phrase_prefix("开始语音录入，你好，世界", DEFAULT_WAKE_PHRASE),
            Some("你好，世界".into())
        );
        assert_eq!(
            strip_phrase_prefix("开始，语音录入 hello", DEFAULT_WAKE_PHRASE),
            Some("hello".into())
        );
        assert!(strip_phrase_prefix("你好，开始语音录入", DEFAULT_WAKE_PHRASE).is_none());
    }

    #[test]
    fn strip_phrase_prefix_handles_nfkc_and_empty_body() {
        assert_eq!(strip_phrase_prefix("開始語音錄入。", "开始语音录入"), None);
        assert_eq!(
            strip_phrase_prefix("开始语音录入。", DEFAULT_WAKE_PHRASE),
            Some(String::new())
        );
    }
    #[test]
    fn strip_phrase_suffix_preserves_spoken_body() {
        assert_eq!(
            strip_phrase_suffix("你好，世界 完成 语音录入", DEFAULT_FINISH_PHRASE),
            Some("你好，世界".into())
        );
        assert_eq!(
            strip_phrase_suffix("你好，世界 完成，语音录入。", DEFAULT_FINISH_PHRASE),
            Some("你好，世界".into())
        );
        assert_eq!(
            strip_phrase_suffix("你好，世界 完成语音录入．", DEFAULT_FINISH_PHRASE),
            Some("你好，世界".into())
        );
        assert!(strip_phrase_suffix("完成语音录入之前", DEFAULT_FINISH_PHRASE).is_none());
    }

    #[test]
    fn phrase_validation_rejects_empty_control_and_long_values() {
        assert_eq!(validate_phrase(" \n"), Err(PhraseValidationError::Empty));
        assert_eq!(
            validate_phrase("开始\u{0000}录入"),
            Err(PhraseValidationError::ControlCharacter)
        );
        assert!(matches!(
            validate_phrase(&"a".repeat(MAX_PHRASE_CHARS + 1)),
            Err(PhraseValidationError::TooLong { .. })
        ));
    }

    #[test]
    fn matcher_only_accepts_suffix_and_strips_command() {
        let matcher =
            VoicePhraseMatcher::new(DEFAULT_FINISH_PHRASE, VoiceActivationCommand::Finish).unwrap();
        assert!(!matcher.matches_suffix("完成语音录入之前的内容"));
        assert!(matcher.matches_suffix("正文 完成，语音录入。"));
        assert_eq!(
            matcher.strip_suffix("正文 完成语音录入").as_deref(),
            Some(
                "正文完成语音录入"
                    .strip_suffix(DEFAULT_FINISH_PHRASE)
                    .unwrap()
            )
        );
    }

    #[test]
    fn recognizer_does_not_repeat_a_command_for_asr_revisions() {
        let config = VoiceActivationConfig::default();
        let mut recognizer = VoiceCommandRecognizer::new(&config).unwrap();
        assert!(recognizer
            .observe("开始语音", VoiceActivationCommand::Wake)
            .is_none());
        assert!(recognizer
            .observe("开始语音录入", VoiceActivationCommand::Wake)
            .is_some());
        assert!(recognizer
            .observe("开始语音录入", VoiceActivationCommand::Wake)
            .is_none());
        recognizer.reset();
        assert!(recognizer
            .observe("开始语音录入", VoiceActivationCommand::Wake)
            .is_some());
    }

    #[test]
    fn generation_and_session_guards_reject_stale_events() {
        let session_id = SessionId::new();
        let session = VoiceActivationSession {
            generation: VoiceActivationGeneration(7),
            session_id,
            timeout: Some(Duration::from_secs(1)),
            commands: VoiceActivationTextPolicy {
                wake_phrase: DEFAULT_WAKE_PHRASE.into(),
                finish_phrase: Some(DEFAULT_FINISH_PHRASE.into()),
            },
        };
        assert!(session.accepts(VoiceActivationGeneration(7), session_id));
        assert!(!session.accepts(VoiceActivationGeneration(6), session_id));
        assert!(!session.accepts(VoiceActivationGeneration(7), SessionId::new()));
    }

    #[test]
    fn timeout_is_bounded_and_only_applies_after_the_voice_session_starts() {
        let config = VoiceActivationConfig {
            timeout_seconds: 1,
            ..VoiceActivationConfig::default()
        };
        assert_eq!(
            config.normalized().unwrap().timeout_seconds,
            MIN_TIMEOUT_SECONDS
        );
        assert_eq!(
            config.normalized().unwrap().timeout(),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            VoiceActivationConfig {
                timeout_enabled: false,
                ..config
            }
            .timeout(),
            None
        );
    }
}
