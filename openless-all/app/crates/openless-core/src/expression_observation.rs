//! Local, text-free signals for conservative expression learning.
//!
//! The feature and safety model is informed by Type4Me's MIT-licensed
//! ExpressionProfileStore and UserEditClassifier; see type4me-LICENSE.txt.

use regex::{Regex, RegexSet};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use unicode_normalization::UnicodeNormalization;

const MIN_CHARS: usize = 12;
const MAX_CHARS: usize = 16_384;

/// Only counts and directions leave this module; neither input text is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpressionSignal {
    pub line_breaks: i8,
    pub exclamations: i8,
    pub terminal_punctuation: i8,
}

const MAX_EVIDENCE: usize = 1024;
const MIN_DIRECTIONAL_SAMPLES: usize = 10;
const MIN_DAY_SPAN: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExpressionFeature {
    LineBreaks,
    Exclamations,
    TerminalPunctuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StableExpressionDirection {
    pub feature: ExpressionFeature,
    pub direction: i8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpressionEvidence {
    session_id: uuid::Uuid,
    day: i32,
    signal: ExpressionSignal,
}

/// Bounded aggregate inputs contain no recognized or edited text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionProfile {
    evidence: Vec<ExpressionEvidence>,
}

const MAX_PROFILE_BYTES: u64 = 512 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileDocument {
    version: u32,
    evidence: Vec<ExpressionEvidence>,
}

/// The store never retains recognized or edited text and rejects unknown files.
pub struct ExpressionProfileStore {
    path: PathBuf,
    profile: Mutex<Option<ExpressionProfile>>,
    generation: AtomicU64,
}

pub type SharedExpressionProfile = Arc<Mutex<Option<Arc<ExpressionProfileStore>>>>;

pub fn open_shared_expression_profile(path: PathBuf) -> SharedExpressionProfile {
    let profile = match ExpressionProfileStore::open(path) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            log::warn!("expression profile unavailable: {error}");
            None
        }
    };
    Arc::new(Mutex::new(profile))
}

impl ExpressionProfileStore {
    /// Explicit user reset can recover an unreadable local profile.
    pub fn reset(path: PathBuf) -> Result<Self, crate::errors::BackendError> {
        crate::persistence::atomic_write(&path, br#"{"version":1,"evidence":[]}"#)?;
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Result<Self, crate::errors::BackendError> {
        let mut profile = ExpressionProfile::default();
        match std::fs::File::open(&path) {
            Ok(file) => {
                let mut data = Vec::new();
                file.take(MAX_PROFILE_BYTES + 1)
                    .read_to_end(&mut data)
                    .map_err(|_| {
                        crate::persistence::persistence_error("read expression profile")
                    })?;
                if data.len() as u64 > MAX_PROFILE_BYTES {
                    return Err(crate::persistence::persistence_error(
                        "expression profile too large",
                    ));
                }
                let document: ProfileDocument = serde_json::from_slice(&data).map_err(|_| {
                    crate::persistence::persistence_error("decode expression profile")
                })?;
                if document.version != 1 || document.evidence.len() > MAX_EVIDENCE {
                    return Err(crate::persistence::persistence_error(
                        "invalid expression profile",
                    ));
                }
                for evidence in document.evidence {
                    if !profile.observe(evidence.session_id, evidence.day, evidence.signal) {
                        return Err(crate::persistence::persistence_error(
                            "invalid expression evidence",
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(crate::persistence::persistence_error(
                    "read expression profile",
                ))
            }
        }
        Ok(Self {
            path,
            profile: Mutex::new(Some(profile)),
            generation: AtomicU64::new(0),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn invalidate_observers(&self) -> Result<(), crate::errors::BackendError> {
        let _profile = self
            .profile
            .lock()
            .map_err(|_| crate::persistence::persistence_error("lock expression profile"))?;
        self.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn record(
        &self,
        session_id: uuid::Uuid,
        day: i32,
        signal: ExpressionSignal,
    ) -> Result<bool, crate::errors::BackendError> {
        self.record_if_generation(session_id, day, signal, self.generation())
    }

    pub fn record_if_generation(
        &self,
        session_id: uuid::Uuid,
        day: i32,
        signal: ExpressionSignal,
        expected_generation: u64,
    ) -> Result<bool, crate::errors::BackendError> {
        let mut profile = self
            .profile
            .lock()
            .map_err(|_| crate::persistence::persistence_error("lock expression profile"))?;
        if self.generation.load(Ordering::Acquire) != expected_generation {
            return Ok(false);
        }
        let mut next = profile
            .as_ref()
            .ok_or_else(|| {
                crate::persistence::persistence_error("expression profile needs reload")
            })?
            .clone();
        if !next.observe(session_id, day, signal) {
            return Ok(false);
        }
        if let Err(error) = self.persist(&next) {
            *profile = None;
            return Err(error);
        }
        *profile = Some(next);
        Ok(true)
    }

    pub fn stable_directions(
        &self,
    ) -> Result<Vec<StableExpressionDirection>, crate::errors::BackendError> {
        let profile = self
            .profile
            .lock()
            .map_err(|_| crate::persistence::persistence_error("lock expression profile"))?;
        Ok(profile
            .as_ref()
            .ok_or_else(|| {
                crate::persistence::persistence_error("expression profile needs reload")
            })?
            .stable_directions())
    }

    pub fn clear(&self) -> Result<(), crate::errors::BackendError> {
        let mut profile = self
            .profile
            .lock()
            .map_err(|_| crate::persistence::persistence_error("lock expression profile"))?;
        self.generation.fetch_add(1, Ordering::AcqRel);
        if profile.is_none() {
            return Err(crate::persistence::persistence_error(
                "expression profile needs reload",
            ));
        }
        let next = ExpressionProfile::default();
        if let Err(error) = self.persist(&next) {
            *profile = None;
            return Err(error);
        }
        *profile = Some(next);
        Ok(())
    }

    fn persist(&self, profile: &ExpressionProfile) -> Result<(), crate::errors::BackendError> {
        let data = serde_json::to_vec(&ProfileDocument {
            version: 1,
            evidence: profile.evidence.clone(),
        })
        .map_err(|_| crate::persistence::persistence_error("encode expression profile"))?;
        crate::persistence::atomic_write(&self.path, &data)
    }
}

impl ExpressionProfile {
    pub fn observe(&mut self, session_id: uuid::Uuid, day: i32, signal: ExpressionSignal) -> bool {
        if self
            .evidence
            .iter()
            .any(|item| item.session_id == session_id)
            || [
                signal.line_breaks,
                signal.exclamations,
                signal.terminal_punctuation,
            ]
            .iter()
            .any(|direction| !(-1..=1).contains(direction))
            || signal
                == (ExpressionSignal {
                    line_breaks: 0,
                    exclamations: 0,
                    terminal_punctuation: 0,
                })
        {
            return false;
        }
        self.evidence.push(ExpressionEvidence {
            session_id,
            day,
            signal,
        });
        self.evidence.sort_unstable_by_key(|item| item.day);
        if self.evidence.len() > MAX_EVIDENCE {
            self.evidence.drain(..self.evidence.len() - MAX_EVIDENCE);
        }
        true
    }

    pub fn stable_directions(&self) -> Vec<StableExpressionDirection> {
        let selectors: [(ExpressionFeature, fn(ExpressionSignal) -> i8); 3] = [
            (ExpressionFeature::LineBreaks, |signal| signal.line_breaks),
            (ExpressionFeature::Exclamations, |signal| {
                signal.exclamations
            }),
            (ExpressionFeature::TerminalPunctuation, |signal| {
                signal.terminal_punctuation
            }),
        ];
        selectors
            .into_iter()
            .filter_map(|(feature, select)| {
                let directional: Vec<_> = self
                    .evidence
                    .iter()
                    .filter_map(|item| {
                        let value = select(item.signal);
                        (value != 0).then_some((item.day, value))
                    })
                    .collect();
                let first = directional.first()?.0;
                let last = directional.last()?.0;
                if directional.len() < MIN_DIRECTIONAL_SAMPLES
                    || last.saturating_sub(first) < MIN_DAY_SPAN
                {
                    return None;
                }
                let positive = directional
                    .iter()
                    .filter(|(_, direction)| *direction > 0)
                    .count();
                let negative = directional.len() - positive;
                let dominant = positive.max(negative);
                (dominant * 10 >= directional.len() * 7).then_some(StableExpressionDirection {
                    feature,
                    direction: if positive > negative { 1 } else { -1 },
                })
            })
            .collect()
    }
}

pub fn prompt_instructions(directions: &[StableExpressionDirection]) -> Option<String> {
    let statements: Vec<&str> = directions
        .iter()
        .filter_map(|item| match (item.feature, item.direction) {
            (ExpressionFeature::LineBreaks, 1) => Some("用户通常偏好更清晰的分行。"),
            (ExpressionFeature::LineBreaks, -1) => Some("用户通常偏好较少的分行。"),
            (ExpressionFeature::Exclamations, 1) => Some("用户通常偏好使用感叹号。"),
            (ExpressionFeature::Exclamations, -1) => Some("用户通常偏好少用感叹号。"),
            (ExpressionFeature::TerminalPunctuation, 1) => Some("用户通常偏好句末标点。"),
            (ExpressionFeature::TerminalPunctuation, -1) => {
                Some("在不影响语义时，用户通常偏好省略陈述句末标点；保留问号和必要标点。")
            }
            _ => None,
        })
        .collect();
    if statements.is_empty() {
        return None;
    }
    Some(format!("\n\n用户经多次编辑形成的形式偏好（仅供参考；不得改变事实、否定、原意、语气、明确指令或必要结构）：\n{}", statements.join("\n")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationRejection {
    Unchanged,
    InsufficientOrOversized,
    Sensitive,
    ContentOrAmbiguous,
}

fn sensitive_patterns() -> &'static RegexSet {
    static PATTERNS: OnceLock<RegexSet> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        RegexSet::new([
            r"(?i)\b[\w.%+-]+@[\w.-]+\.[a-z]{2,}\b",
            r"(?i)(?:https?://|www\.)\S+",
            r"(?:\d[ -]?){7,}",
            r"(?:^|\s)(?:/[^\s]+|[a-zA-Z]:\\[^\s]+)",
            r"\b[A-Za-z0-9_-]{20,}\b",
        ])
        .expect("fixed sensitivity expressions are valid")
    })
}

fn ascii_words() -> &'static Regex {
    static WORDS: OnceLock<Regex> = OnceLock::new();
    WORDS.get_or_init(|| Regex::new(r"[a-z0-9]+").expect("fixed word expression is valid"))
}

fn normalized(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

fn skeleton(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace() && !",.;:!，。；：！、".contains(*character))
        .collect()
}

fn direction(before: usize, after: usize) -> i8 {
    match after.cmp(&before) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn count(text: &str, chars: &[char]) -> usize {
    text.chars()
        .filter(|character| chars.contains(character))
        .count()
}

fn ends_in_terminal_punctuation(text: &str) -> usize {
    usize::from(
        text.trim_end()
            .chars()
            .last()
            .is_some_and(|c| ".!?。！？".contains(c)),
    )
}

/// Accept only a narrow set of formatting edits while preserving symbols,
/// question marks, content and Latin word boundaries.
pub fn expression_signal(
    original: &str,
    edited: &str,
) -> Result<ExpressionSignal, ObservationRejection> {
    if original == edited {
        return Err(ObservationRejection::Unchanged);
    }
    let original_length = original.chars().count();
    let edited_length = edited.chars().count();
    if !(MIN_CHARS..=MAX_CHARS).contains(&original_length)
        || !(MIN_CHARS..=MAX_CHARS).contains(&edited_length)
    {
        return Err(ObservationRejection::InsufficientOrOversized);
    }
    let original_normalized = normalized(original);
    let edited_normalized = normalized(edited);
    if sensitive_patterns().is_match(&original_normalized)
        || sensitive_patterns().is_match(&edited_normalized)
    {
        return Err(ObservationRejection::Sensitive);
    }
    if skeleton(&original_normalized) != skeleton(&edited_normalized)
        || !ascii_words()
            .find_iter(&original_normalized)
            .map(|word| word.as_str())
            .eq(ascii_words()
                .find_iter(&edited_normalized)
                .map(|word| word.as_str()))
    {
        return Err(ObservationRejection::ContentOrAmbiguous);
    }
    Ok(ExpressionSignal {
        line_breaks: direction(count(original, &['\n']), count(edited, &['\n'])),
        exclamations: direction(count(original, &['!', '！']), count(edited, &['!', '！'])),
        terminal_punctuation: direction(
            ends_in_terminal_punctuation(original),
            ends_in_terminal_punctuation(edited),
        ),
    })
}

/// Rebuild one edited insertion only when the source and surrounding context
/// identify a unique location within the text that OpenLess actually inserted.
pub fn signal_from_observed_edit(
    inserted: &str,
    edit: &crate::host_document::EditPair,
) -> Result<ExpressionSignal, ObservationRejection> {
    if inserted.chars().count() > MAX_CHARS {
        return Err(ObservationRejection::InsufficientOrOversized);
    }
    let before_anchor: String = edit
        .before
        .chars()
        .rev()
        .take(16)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let after_anchor: String = edit.after.chars().take(16).collect();
    let mut candidate = None;
    for (offset, _) in inserted
        .char_indices()
        .chain(std::iter::once((inserted.len(), '\0')))
    {
        let Some(remaining) = inserted[offset..].strip_prefix(&edit.source) else {
            continue;
        };
        if !inserted[..offset].ends_with(&before_anchor) || !remaining.starts_with(&after_anchor) {
            continue;
        }
        if candidate.is_some() {
            return Err(ObservationRejection::ContentOrAmbiguous);
        }
        candidate = Some((offset, offset + edit.source.len()));
    }
    let (start, end) = candidate.ok_or(ObservationRejection::ContentOrAmbiguous)?;
    let edited = format!("{}{}{}", &inserted[..start], edit.target, &inserted[end..]);
    expression_signal(inserted, &edited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_formatting_only_and_emits_no_text() {
        let signal = expression_signal(
            "今天我们先完成这个工作然后再讨论",
            "今天我们先完成这个工作，\n然后再讨论！",
        )
        .unwrap();
        assert_eq!(signal.line_breaks, 1);
        assert_eq!(signal.exclamations, 1);
        assert_eq!(signal.terminal_punctuation, 1);
        let serialized = serde_json::to_string(&signal).unwrap();
        assert!(!serialized.contains("今天"));
    }

    #[test]
    fn rejects_changed_facts_negation_and_word_boundaries() {
        for (before, after) in [
            ("今天我们需要购买十二个苹果", "今天我们需要购买十三个苹果"),
            ("这件事情现在我们不能这样做", "这件事情现在我们能这样做"),
            (
                "This is a notable result today",
                "This is a not able result today",
            ),
            ("请把这个结果标记为通过 ✅", "请把这个结果标记为通过 ❌"),
            ("这段程序应该使用 C++ 版本", "这段程序应该使用 C# 版本"),
        ] {
            assert_eq!(
                expression_signal(before, after),
                Err(ObservationRejection::ContentOrAmbiguous)
            );
        }
    }

    #[test]
    fn only_unambiguous_edits_inside_the_inserted_text_are_observed() {
        let inserted = "今天我们先完成这个工作然后再讨论";
        let edit = crate::host_document::EditPair {
            source: String::new(),
            target: "\n".into(),
            before: "今天我们先完成这个工作".into(),
            after: "然后再讨论".into(),
        };
        assert_eq!(
            signal_from_observed_edit(inserted, &edit)
                .unwrap()
                .line_breaks,
            1
        );
        let ambiguous = crate::host_document::EditPair {
            source: "，".into(),
            target: "。".into(),
            before: String::new(),
            after: String::new(),
        };
        assert_eq!(
            signal_from_observed_edit("这是一个足够长的句子，里面还有另一个，分隔符", &ambiguous),
            Err(ObservationRejection::ContentOrAmbiguous)
        );
        assert_eq!(
            signal_from_observed_edit(inserted, &ambiguous),
            Err(ObservationRejection::ContentOrAmbiguous)
        );
    }

    #[test]
    fn rejects_sensitive_and_unbounded_observations() {
        assert_eq!(
            expression_signal(
                "Email me at user@example.com please",
                "Email me at user@example.com, please!"
            ),
            Err(ObservationRejection::Sensitive)
        );
        assert_eq!(
            expression_signal("短句", "短句！"),
            Err(ObservationRejection::InsufficientOrOversized)
        );
        let long = "测".repeat(MAX_CHARS + 1);
        assert_eq!(
            expression_signal(&long, &(long.clone() + "！")),
            Err(ObservationRejection::InsufficientOrOversized)
        );
    }

    #[test]
    fn stable_directions_require_independent_consistent_samples_across_days() {
        let mut profile = ExpressionProfile::default();
        let id = uuid::Uuid::new_v4();
        let positive = ExpressionSignal {
            line_breaks: 1,
            exclamations: 0,
            terminal_punctuation: 0,
        };
        assert!(profile.observe(id, 100, positive));
        assert!(!profile.observe(id, 103, positive));
        for index in 0..9 {
            assert!(profile.observe(uuid::Uuid::new_v4(), 100 + (index % 3), positive));
        }
        assert!(profile.stable_directions().is_empty());
        assert!(profile.observe(uuid::Uuid::new_v4(), 103, positive));
        assert_eq!(
            profile.stable_directions(),
            vec![StableExpressionDirection {
                feature: ExpressionFeature::LineBreaks,
                direction: 1,
            }]
        );
        let negative = ExpressionSignal {
            line_breaks: -1,
            ..positive
        };
        for _ in 0..8 {
            assert!(profile.observe(uuid::Uuid::new_v4(), 104, negative));
        }
        assert!(profile.stable_directions().is_empty());
    }

    #[test]
    fn profile_store_roundtrips_abstract_evidence_and_clears() {
        let directory =
            std::env::temp_dir().join(format!("openless-expression-{}", uuid::Uuid::new_v4()));
        let path = directory.join("profile.json");
        let store = ExpressionProfileStore::open(path.clone()).unwrap();
        let signal = ExpressionSignal {
            line_breaks: 1,
            exclamations: 0,
            terminal_punctuation: 0,
        };
        for index in 0..10 {
            assert!(store
                .record(uuid::Uuid::new_v4(), 100 + (index % 4), signal)
                .unwrap());
        }
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("今天"));
        drop(store);
        let reloaded = ExpressionProfileStore::open(path.clone()).unwrap();
        assert_eq!(reloaded.stable_directions().unwrap().len(), 1);
        let previous_generation = reloaded.generation();
        reloaded.clear().unwrap();
        assert!(!reloaded
            .record_if_generation(uuid::Uuid::new_v4(), 104, signal, previous_generation)
            .unwrap());
        let opted_in_generation = reloaded.generation();
        reloaded.invalidate_observers().unwrap();
        assert!(!reloaded
            .record_if_generation(uuid::Uuid::new_v4(), 105, signal, opted_in_generation)
            .unwrap());
        drop(reloaded);
        assert!(ExpressionProfileStore::open(path)
            .unwrap()
            .stable_directions()
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_and_unknown_profile_is_not_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("openless-expression-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("profile.json");
        let content = br#"{"version":1,"evidence":[],"newField":"keep"}"#;
        std::fs::write(&path, content).unwrap();
        assert!(ExpressionProfileStore::open(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), content);
        let reset = ExpressionProfileStore::reset(path).unwrap();
        assert!(reset.stable_directions().unwrap().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
