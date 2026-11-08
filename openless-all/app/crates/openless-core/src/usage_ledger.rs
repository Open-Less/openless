//! Local service-use metadata. Content, credentials and inferred prices are never stored.

use crate::errors::BackendError;
use crate::persistence::{atomic_write, persistence_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use uuid::Uuid;

const MAX_RECORDS: usize = 10_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceKind {
    Asr,
    Llm,
    Tts,
    Multimodal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServicePurpose {
    Dictation,
    DictationPostprocess,
    Translation,
    Question,
    Selection,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

/// Values are retained as the provider reported them. Cache accounting differs
/// between protocols, so these fields must not be summed implicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportedTokens {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached_input: Option<u64>,
}

impl ReportedTokens {
    pub fn from_response(value: &serde_json::Value) -> Option<Self> {
        let usage = value
            .get("usage")
            .or_else(|| value.pointer("/response/usage"))
            .or_else(|| value.pointer("/message/usage"))
            .or_else(|| value.get("usageMetadata"))?;
        let tokens = Self {
            input: usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .or_else(|| usage.get("promptTokenCount"))
                .and_then(|value| value.as_u64()),
            output: usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
                .or_else(|| usage.get("candidatesTokenCount"))
                .and_then(|value| value.as_u64()),
            cached_input: usage
                .pointer("/input_tokens_details/cached_tokens")
                .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
                .or_else(|| usage.get("cachedContentTokenCount"))
                .and_then(|value| value.as_u64()),
        };
        (tokens != Self::default()).then_some(tokens)
    }

    pub fn merge_report(&mut self, newer: Self) {
        if newer.input.is_some() {
            self.input = newer.input;
        }
        if newer.output.is_some() {
            self.output = newer.output;
        }
        if newer.cached_input.is_some() {
            self.cached_input = newer.cached_input;
        }
    }
}

/// Quantities are optional because provider responses may not disclose them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageRecord {
    pub id: Uuid,
    pub occurred_at_ms: i64,
    pub kind: ServiceKind,
    pub purpose: ServicePurpose,
    pub outcome: ServiceOutcome,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub audio_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub synthesized_characters: Option<u64>,
}

fn identifier_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && !value.contains("://")
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || "._-/+:".contains(character))
}

impl UsageRecord {
    fn valid(&self) -> bool {
        self.occurred_at_ms > 0
            && self.provider.as_deref().is_none_or(identifier_valid)
            && self.model.as_deref().is_none_or(identifier_valid)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsageDocument {
    version: u32,
    records: Vec<UsageRecord>,
}

/// A single shared instance must own a given file within a process.
pub struct UsageLedger {
    path: PathBuf,
    records: Mutex<Option<Vec<UsageRecord>>>,
    generation: AtomicU64,
}

impl UsageLedger {
    pub fn open(path: PathBuf) -> Result<Self, BackendError> {
        let mut records = Vec::new();
        match std::fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| persistence_error("read service usage"))?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err(persistence_error("service usage too large"));
                }
                let document: UsageDocument = serde_json::from_slice(&bytes)
                    .map_err(|_| persistence_error("decode service usage"))?;
                if document.version != 1 || document.records.len() > MAX_RECORDS {
                    return Err(persistence_error("invalid service usage"));
                }
                let mut ids = HashSet::new();
                if document
                    .records
                    .iter()
                    .any(|record| !record.valid() || !ids.insert(record.id))
                {
                    return Err(persistence_error("invalid service usage record"));
                }
                records = document.records;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(persistence_error("read service usage")),
        }
        Ok(Self {
            path,
            records: Mutex::new(Some(records)),
            generation: AtomicU64::new(0),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn append(
        &self,
        expected_generation: u64,
        record: UsageRecord,
    ) -> Result<bool, BackendError> {
        if !record.valid() {
            return Err(persistence_error("invalid service usage record"));
        }
        let mut state = self
            .records
            .lock()
            .map_err(|_| persistence_error("lock service usage"))?;
        if self.generation.load(Ordering::Acquire) != expected_generation {
            return Ok(false);
        }
        let mut next = state
            .as_ref()
            .ok_or_else(|| persistence_error("service usage needs reload"))?
            .clone();
        if next.iter().any(|existing| existing.id == record.id) {
            return Ok(false);
        }
        next.push(record);
        next.sort_unstable_by_key(|record| record.occurred_at_ms);
        if next.len() > MAX_RECORDS {
            next.drain(..next.len() - MAX_RECORDS);
        }
        if let Err(error) = self.persist(&next) {
            *state = None;
            return Err(error);
        }
        *state = Some(next);
        Ok(true)
    }

    pub fn list(&self) -> Result<Vec<UsageRecord>, BackendError> {
        self.records
            .lock()
            .map_err(|_| persistence_error("lock service usage"))?
            .clone()
            .ok_or_else(|| persistence_error("service usage needs reload"))
    }

    pub fn clear(&self) -> Result<(), BackendError> {
        let mut state = self
            .records
            .lock()
            .map_err(|_| persistence_error("lock service usage"))?;
        self.generation.fetch_add(1, Ordering::AcqRel);
        if state.is_none() {
            return Err(persistence_error("service usage needs reload"));
        }
        if let Err(error) = self.persist(&[]) {
            *state = None;
            return Err(error);
        }
        *state = Some(Vec::new());
        Ok(())
    }

    fn persist(&self, records: &[UsageRecord]) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec(&UsageDocument {
            version: 1,
            records: records.to_vec(),
        })
        .map_err(|_| persistence_error("encode service usage"))?;
        atomic_write(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> UsageRecord {
        UsageRecord {
            id: Uuid::new_v4(),
            occurred_at_ms: 1_800_000_000_000,
            kind: ServiceKind::Llm,
            purpose: ServicePurpose::DictationPostprocess,
            outcome: ServiceOutcome::Succeeded,
            provider: Some("provider".into()),
            model: Some("model-v1".into()),
            audio_ms: None,
            input_tokens: Some(80),
            output_tokens: Some(20),
            cached_input_tokens: None,
            synthesized_characters: None,
        }
    }

    #[test]
    fn persists_reported_quantities_and_deduplicates_without_text() {
        let dir = std::env::temp_dir().join(format!("openless-usage-{}", Uuid::new_v4()));
        let path = dir.join("usage.json");
        let ledger = UsageLedger::open(path.clone()).unwrap();
        let event = record();
        assert!(ledger.append(ledger.generation(), event.clone()).unwrap());
        assert!(!ledger.append(ledger.generation(), event.clone()).unwrap());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("transcript"));
        assert!(!content.contains("apiKey"));
        drop(ledger);
        assert_eq!(
            UsageLedger::open(path).unwrap().list().unwrap(),
            vec![event]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clear_invalidates_old_writers_and_unknown_files_are_preserved() {
        let dir = std::env::temp_dir().join(format!("openless-usage-{}", Uuid::new_v4()));
        let path = dir.join("usage.json");
        let ledger = UsageLedger::open(path.clone()).unwrap();
        let generation = ledger.generation();
        assert!(ledger.append(generation, record()).unwrap());
        ledger.clear().unwrap();
        assert!(!ledger.append(generation, record()).unwrap());
        assert!(ledger.list().unwrap().is_empty());
        drop(ledger);
        let unknown = br#"{"version":2,"records":[],"newField":true}"#;
        std::fs::write(&path, unknown).unwrap();
        assert!(UsageLedger::open(path.clone()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), unknown);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_invalid_identifiers_without_inventing_token_semantics() {
        let dir = std::env::temp_dir().join(format!("openless-usage-{}", Uuid::new_v4()));
        let ledger = UsageLedger::open(dir.join("usage.json")).unwrap();
        let mut event = record();
        event.model = Some("https://example.invalid/?key=secret".into());
        assert!(ledger.append(ledger.generation(), event).is_err());
        let mut event = record();
        event.cached_input_tokens = Some(81);
        assert!(ledger.append(ledger.generation(), event).unwrap());
    }

    #[test]
    fn reads_only_provider_reported_token_fields() {
        use serde_json::json;
        let chat = ReportedTokens::from_response(&json!({"usage":{"prompt_tokens":100,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":30}}})).unwrap();
        assert_eq!(
            chat,
            ReportedTokens {
                input: Some(100),
                output: Some(8),
                cached_input: Some(30)
            }
        );
        let response = ReportedTokens::from_response(
            &json!({"response":{"usage":{"input_tokens":40,"output_tokens":5}}}),
        )
        .unwrap();
        assert_eq!(response.input, Some(40));
        assert_eq!(response.output, Some(5));
        let mut messages = ReportedTokens::from_response(
            &json!({"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":90}}}),
        )
        .unwrap();
        messages.merge_report(
            ReportedTokens::from_response(&json!({"usage":{"output_tokens":7}})).unwrap(),
        );
        assert_eq!(
            messages,
            ReportedTokens {
                input: Some(10),
                output: Some(7),
                cached_input: Some(90)
            }
        );
        let gemini = ReportedTokens::from_response(&json!({"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,"cachedContentTokenCount":4}})).unwrap();
        assert_eq!(gemini.cached_input, Some(4));
        assert!(ReportedTokens::from_response(&json!({"usage":{"total_tokens":999}})).is_none());
        assert!(ReportedTokens::from_response(&json!({"text":"never stored"})).is_none());
    }
}
