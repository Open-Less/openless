//! Local service-use metadata. Content, credentials and inferred prices are never stored.

use crate::errors::BackendError;
use crate::persistence::{atomic_write, persistence_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
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
    QuickNote,
    VoiceEdit,
    CloudNote,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheInputAccounting {
    Inclusive,
    ExcludesCache,
}

/// Values are retained as the provider reported them. Cache accounting differs
/// between protocols, so these fields must not be summed implicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportedTokens {
    pub input_accounting: Option<CacheInputAccounting>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached_input: Option<u64>,
    pub cache_write_input: Option<u64>,
    pub reasoning: Option<u64>,
}

impl ReportedTokens {
    pub fn from_response(value: &serde_json::Value) -> Option<Self> {
        let usage = value
            .get("usage")
            .filter(|usage| usage.is_object())
            .or_else(|| {
                value
                    .pointer("/response/usage")
                    .filter(|usage| usage.is_object())
            })
            .or_else(|| {
                value
                    .pointer("/message/usage")
                    .filter(|usage| usage.is_object())
            })
            .or_else(|| value.get("usageMetadata").filter(|usage| usage.is_object()))?;
        // Canonical response fields define accounting, never the selected vendor label.
        // Mixed field families from an unknown gateway remain unclassified.
        let additive = usage.get("cache_read_input_tokens").is_some()
            || usage.get("cache_creation_input_tokens").is_some();
        let inclusive = usage.get("prompt_tokens").is_some()
            || usage.get("promptTokenCount").is_some()
            || usage
                .pointer("/input_tokens_details/cached_tokens")
                .is_some()
            || usage
                .pointer("/input_tokens_details/cache_write_tokens")
                .is_some()
            || usage
                .pointer("/input_tokens_details/cache_creation_input_tokens")
                .is_some()
            || usage
                .pointer("/prompt_tokens_details/cache_creation_input_tokens")
                .is_some()
            || usage
                .pointer("/prompt_tokens_details/cache_write_tokens")
                .is_some()
            || usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .is_some()
            || usage.get("prompt_cache_hit_tokens").is_some()
            || usage.get("cachedContentTokenCount").is_some();
        let input_fields = ["input_tokens", "prompt_tokens", "promptTokenCount"]
            .iter()
            .filter(|name| usage.get(**name).is_some())
            .count();
        let input_accounting = match (additive, inclusive, input_fields > 1) {
            (true, false, false) => Some(CacheInputAccounting::ExcludesCache),
            (false, true, false) => Some(CacheInputAccounting::Inclusive),
            _ => None,
        };
        let tokens = Self {
            input_accounting,
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
                .or_else(|| usage.get("prompt_cache_hit_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
                .or_else(|| usage.get("cachedContentTokenCount"))
                .and_then(|value| value.as_u64()),
            cache_write_input: usage
                .get("cache_creation_input_tokens")
                .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
                .or_else(|| usage.pointer("/input_tokens_details/cache_creation_input_tokens"))
                .or_else(|| usage.pointer("/prompt_tokens_details/cache_creation_input_tokens"))
                .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
                .and_then(|value| value.as_u64()),
            reasoning: usage
                .pointer("/output_tokens_details/reasoning_tokens")
                .or_else(|| usage.pointer("/completion_tokens_details/reasoning_tokens"))
                .and_then(|value| value.as_u64()),
        };
        (tokens.input.is_some()
            || tokens.output.is_some()
            || tokens.cached_input.is_some()
            || tokens.cache_write_input.is_some()
            || tokens.reasoning.is_some())
        .then_some(tokens)
    }

    pub fn merge_report(&mut self, newer: Self) {
        if newer.input.is_some()
            || newer.cached_input.is_some()
            || newer.cache_write_input.is_some()
        {
            self.input_accounting = newer.input_accounting;
        }
        if newer.input.is_some() {
            self.input = newer.input;
        }
        if newer.output.is_some() {
            self.output = newer.output;
        }
        if newer.cached_input.is_some() {
            self.cached_input = newer.cached_input;
        }
        if newer.cache_write_input.is_some() {
            self.cache_write_input = newer.cache_write_input;
        }
        if newer.reasoning.is_some() {
            self.reasoning = newer.reasoning;
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
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub input_accounting: Option<CacheInputAccounting>,
    pub channel: Option<String>,
    pub model: Option<String>,
    pub audio_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub synthesized_characters: Option<u64>,
}

fn identifier_valid(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.is_empty()
        && value.len() <= 160
        && !value.contains("://")
        && !lower.starts_with("sk-")
        && !lower.starts_with("bearer")
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || "._-/+:".contains(character))
}

/// Adapters may omit untrusted or invalid identifiers without dropping usage.
pub fn sanitized_identifier(value: &str) -> Option<String> {
    let value = value.trim();
    identifier_valid(value).then(|| value.to_owned())
}

impl UsageRecord {
    fn valid(&self) -> bool {
        !self.id.is_nil()
            && self.occurred_at_ms > 0
            && self.provider.as_deref().is_none_or(identifier_valid)
            && self.service.as_deref().is_none_or(identifier_valid)
            && self.protocol.as_deref().is_none_or(identifier_valid)
            && self.channel.as_deref().is_none_or(identifier_valid)
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

enum WriterCommand {
    Append(u64, Box<UsageRecord>),
    Flush(mpsc::Sender<()>),
    Clear(mpsc::Sender<Result<(), BackendError>>),
}

/// A bounded worker keeps disk writes off recognition and model response paths.
pub struct UsageRecorder {
    ledger: Arc<UsageLedger>,
    sender: mpsc::SyncSender<WriterCommand>,
    dropped_or_failed: Arc<AtomicU64>,
}

impl UsageRecorder {
    pub fn start(ledger: Arc<UsageLedger>) -> Result<Self, BackendError> {
        let (sender, receiver) = mpsc::sync_channel(256);
        let worker_ledger = Arc::clone(&ledger);
        let dropped_or_failed = Arc::new(AtomicU64::new(0));
        let failures = Arc::clone(&dropped_or_failed);
        std::thread::Builder::new()
            .name("openless-usage-ledger".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        WriterCommand::Append(generation, record) => {
                            if worker_ledger.append(generation, *record).is_err() {
                                failures.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        WriterCommand::Flush(reply) => {
                            let _ = reply.send(());
                        }
                        WriterCommand::Clear(reply) => {
                            let _ = reply.send(worker_ledger.clear());
                        }
                    }
                }
            })
            .map_err(|_| persistence_error("start service usage worker"))?;
        Ok(Self {
            ledger,
            sender,
            dropped_or_failed,
        })
    }

    pub fn generation(&self) -> u64 {
        self.ledger.generation()
    }

    pub fn submit(&self, generation: u64, record: UsageRecord) -> bool {
        if self
            .sender
            .try_send(WriterCommand::Append(generation, Box::new(record)))
            .is_ok()
        {
            true
        } else {
            self.dropped_or_failed.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Call from a blocking worker, never from an audio or UI thread.
    pub fn snapshot(&self) -> Result<(Vec<UsageRecord>, u64), BackendError> {
        let (send, receive) = mpsc::channel();
        self.sender
            .try_send(WriterCommand::Flush(send))
            .map_err(|_| persistence_error("flush service usage"))?;
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| persistence_error("flush service usage"))?;
        Ok((
            self.ledger.list()?,
            self.dropped_or_failed.load(Ordering::Relaxed),
        ))
    }

    /// Queued records precede the clear; in-flight pre-clear requests are rejected later.
    pub fn clear(&self) -> Result<(), BackendError> {
        let (send, receive) = mpsc::channel();
        self.sender
            .try_send(WriterCommand::Clear(send))
            .map_err(|_| persistence_error("clear service usage"))?;
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| {
                BackendError::new(
                    crate::errors::BackendErrorCode::OutcomeUnknown,
                    "service usage clear outcome is unknown",
                )
            })?
    }
}

impl UsageLedger {
    /// Replacing an unreadable ledger requires an explicit user reset.
    pub fn reset(path: PathBuf) -> Result<Self, BackendError> {
        atomic_write(&path, br#"{"version":1,"records":[]}"#)?;
        Self::open(path)
    }

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
            service: Some("custom".into()),
            protocol: Some("chat_completions".into()),
            input_accounting: Some(CacheInputAccounting::Inclusive),
            channel: Some("channel-1".into()),
            model: Some("model-v1".into()),
            audio_ms: None,
            input_tokens: Some(80),
            output_tokens: Some(20),
            cached_input_tokens: None,
            cache_write_input_tokens: None,
            reasoning_tokens: None,
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
        assert_eq!(std::fs::read(&path).unwrap(), unknown);
        assert!(UsageLedger::reset(path).unwrap().list().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_records_without_optional_fields_remain_readable() {
        let dir = std::env::temp_dir().join(format!("openless-usage-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usage.json");
        let mut value = serde_json::to_value(record()).unwrap();
        value.as_object_mut().unwrap().remove("channel");
        for field in ["service", "protocol", "inputAccounting"] {
            value.as_object_mut().unwrap().remove(field);
        }
        value
            .as_object_mut()
            .unwrap()
            .remove("cacheWriteInputTokens");
        value.as_object_mut().unwrap().remove("reasoningTokens");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"version":1,"records":[value]})).unwrap(),
        )
        .unwrap();
        let saved = UsageLedger::open(path).unwrap().list().unwrap();
        assert_eq!(saved[0].channel, None);
        assert_eq!(saved[0].service, None);
        assert_eq!(saved[0].protocol, None);
        assert_eq!(saved[0].input_accounting, None);
        assert_eq!(saved[0].cache_write_input_tokens, None);
        assert_eq!(saved[0].reasoning_tokens, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recorder_flushes_and_rejects_inflight_records_after_clear() {
        let dir = std::env::temp_dir().join(format!("openless-usage-{}", Uuid::new_v4()));
        let ledger = Arc::new(UsageLedger::open(dir.join("usage.json")).unwrap());
        let recorder = UsageRecorder::start(ledger).unwrap();
        let generation = recorder.generation();
        assert!(recorder.submit(generation, record()));
        assert_eq!(recorder.snapshot().unwrap().0.len(), 1);
        recorder.clear().unwrap();
        assert!(recorder.submit(generation, record()));
        assert!(recorder.snapshot().unwrap().0.is_empty());
        let mut invalid = record();
        invalid.model = Some("sk-secret".into());
        assert!(recorder.submit(recorder.generation(), invalid));
        assert_eq!(recorder.snapshot().unwrap().1, 1);
        drop(recorder);
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
        event.model = Some("sk-example-secret".into());
        assert!(ledger.append(ledger.generation(), event).is_err());
        assert_eq!(sanitized_identifier("sk-example-secret"), None);
        assert_eq!(sanitized_identifier(" model-v1 "), Some("model-v1".into()));
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
                input_accounting: Some(CacheInputAccounting::Inclusive),
                input: Some(100),
                output: Some(8),
                cached_input: Some(30),
                cache_write_input: None,
                reasoning: None,
            }
        );
        let response = ReportedTokens::from_response(
            &json!({"response":{"usage":{"input_tokens":40,"output_tokens":5}}}),
        )
        .unwrap();
        assert_eq!(response.input, Some(40));
        assert_eq!(response.output, Some(5));
        let reasoning = ReportedTokens::from_response(
            &json!({"usage":{"output_tokens":10,"output_tokens_details":{"reasoning_tokens":7}}}),
        )
        .unwrap();
        assert_eq!(reasoning.reasoning, Some(7));
        let nested = ReportedTokens::from_response(
            &json!({"usage":null,"response":{"usage":{"input_tokens":40}}}),
        )
        .unwrap();
        assert_eq!(nested.input, Some(40));
        let mut messages = ReportedTokens::from_response(
            &json!({"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":12}}}),
        )
        .unwrap();
        messages.merge_report(
            ReportedTokens::from_response(&json!({"usage":{"output_tokens":7}})).unwrap(),
        );
        assert_eq!(
            messages,
            ReportedTokens {
                input_accounting: Some(CacheInputAccounting::ExcludesCache),
                input: Some(10),
                output: Some(7),
                cached_input: Some(90),
                cache_write_input: Some(12),
                reasoning: None,
            }
        );
        let gemini = ReportedTokens::from_response(&json!({"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,"cachedContentTokenCount":4}})).unwrap();
        assert_eq!(gemini.cached_input, Some(4));
        assert!(ReportedTokens::from_response(&json!({"usage":{"total_tokens":999}})).is_none());
        assert!(ReportedTokens::from_response(&json!({"text":"never stored"})).is_none());
    }

    #[test]
    fn cache_accounting_follows_report_fields_and_preserves_unknowns() {
        use serde_json::json;
        for value in [
            json!({"usage":{"prompt_tokens":100,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}),
            json!({"usage":{"prompt_tokens":0,"prompt_tokens_details":{"cached_tokens":0}}}),
            json!({"response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":80}}}}),
            json!({"usageMetadata":{"promptTokenCount":100,"cachedContentTokenCount":80}}),
        ] {
            let report = ReportedTokens::from_response(&value).unwrap();
            assert_eq!(
                report.input_accounting,
                Some(CacheInputAccounting::Inclusive)
            );
        }
        let zero = ReportedTokens::from_response(&json!({"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}})).unwrap();
        assert_eq!(zero.input, Some(0));
        assert_eq!(zero.cached_input, Some(0));
        assert_eq!(
            zero.input_accounting,
            Some(CacheInputAccounting::ExcludesCache)
        );
        for value in [
            json!({"usage":{"input_tokens":100}}),
            json!({"usage":{"input_tokens":10,"prompt_tokens":100,"prompt_cache_hit_tokens":80}}),
            json!({"usage":{"prompt_tokens":100,"cache_read_input_tokens":80}}),
        ] {
            assert_eq!(
                ReportedTokens::from_response(&value)
                    .unwrap()
                    .input_accounting,
                None
            );
        }
        let missing = ReportedTokens::from_response(
            &json!({"usage":{"input_tokens":10,"cache_read_input_tokens":80}}),
        )
        .unwrap();
        assert_eq!(missing.cache_write_input, None);
        assert!(
            ReportedTokens::from_response(&json!({"usage":{"prompt_tokens":"invalid"}})).is_none()
        );
        let mut report = ReportedTokens::from_response(
            &json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":30}}}),
        )
        .unwrap();
        report.merge_report(ReportedTokens::from_response(&json!({"usage":{"prompt_tokens_details":{"cached_tokens":40},"cache_read_input_tokens":40}})).unwrap());
        assert_eq!(report.input_accounting, None);
    }

    #[test]
    fn streaming_messages_preserve_input_accounting_across_output_reports() {
        use crate::llm_protocol::{LlmRequestFormat, TextEventStream};
        let mut events = TextEventStream::new(LlmRequestFormat::Messages);
        events.push(b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":80,\"cache_creation_input_tokens\":10,\"output_tokens\":0}}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\ndata: {\"type\":\"message_stop\"}\n\n").unwrap();
        while events.next().unwrap().is_some() {}
        let report = events.reported_tokens();
        assert_eq!(
            report.input_accounting,
            Some(CacheInputAccounting::ExcludesCache)
        );
        assert_eq!(report.input, Some(10));
        assert_eq!(report.cached_input, Some(80));
        assert_eq!(report.cache_write_input, Some(10));
        assert_eq!(report.output, Some(7));
    }

    #[test]
    fn nested_cache_write_fields_do_not_change_inclusive_denominators() {
        use serde_json::json;
        for value in [
            json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":30,"cache_creation_input_tokens":20}}}),
            json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":30,"cache_write_tokens":20}}}),
            json!({"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":30,"cache_creation_input_tokens":20}}}),
        ] {
            let tokens = ReportedTokens::from_response(&value).unwrap();
            assert_eq!(tokens.input, Some(100));
            assert_eq!(tokens.cached_input, Some(30));
            assert_eq!(tokens.cache_write_input, Some(20));
            assert_eq!(
                tokens.input_accounting,
                Some(CacheInputAccounting::Inclusive)
            );
        }
    }
}
