//! Whole-pipeline tests use synthetic PCM without waiting for wall-clock audio.
use super::*;
use crate::ports::{PolishOutput, TranscriptOutput};
use std::sync::atomic::AtomicUsize;

struct Archive {
    pcm: Arc<Vec<u8>>,
    discarded: Arc<AtomicBool>,
}

impl RecordingArchive for Archive {
    fn pcm_byte_len(&self) -> BoxFuture<'static, Result<u64, BackendError>> {
        let len = self.pcm.len() as u64;
        Box::pin(async move { Ok(len) })
    }
    fn is_available(&self) -> bool {
        !self.discarded.load(Ordering::Acquire)
    }
    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let pcm = Arc::clone(&self.pcm);
        Box::pin(async move { Ok((*pcm).clone()) })
    }
    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.discarded.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    }
}

struct Recording(Arc<Archive>);
impl ActiveRecording for Recording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        Some(self.0.clone())
    }
    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

struct Recorder(Arc<Archive>);
impl AudioRecorder for Recorder {
    fn start(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        _: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        let archive = Arc::clone(&self.0);
        Box::pin(async move {
            for chunk in archive.pcm.chunks(64 * 1024) {
                consumer.consume_pcm_chunk(chunk);
            }
            Ok(Box::new(Recording(archive)) as Box<dyn ActiveRecording>)
        })
    }
}

struct Asr {
    first: Result<String, BackendError>,
    received: Arc<Mutex<Vec<Vec<u8>>>>,
    calls: AtomicUsize,
}
struct AsrSession {
    result: Result<String, BackendError>,
    received: Arc<Mutex<Vec<Vec<u8>>>>,
    index: usize,
}
impl AudioConsumer for AsrSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.received.lock().unwrap()[self.index].extend_from_slice(pcm);
    }
}
impl TranscriptionSession for AsrSession {
    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        let result = self.result.clone();
        let duration_ms = crate::long_recording::duration_ms(
            self.received.lock().unwrap()[self.index].len() as u64,
        );
        Box::pin(async move { result.map(|text| TranscriptOutput { text, duration_ms }) })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
impl TranscriptionEngine for Asr {
    fn start(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        _: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let index = self.calls.fetch_add(1, Ordering::AcqRel);
        self.received.lock().unwrap().push(Vec::new());
        let result = if index == 0 {
            self.first.clone()
        } else {
            Ok(format!("segment-{index}"))
        };
        let session = AsrSession {
            result,
            received: Arc::clone(&self.received),
            index,
        };
        Box::pin(async move { Ok(Arc::new(session) as Arc<dyn TranscriptionSession>) })
    }
}

#[derive(Default)]
struct Polisher {
    inputs: Mutex<Vec<String>>,
    fail_once: AtomicBool,
    fail_always: bool,
}
impl TextPolisher for Polisher {
    fn polish(
        &self,
        _: SessionId,
        context: Arc<DictationContext>,
        input: String,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        assert!(!context.polish.llm_thinking_enabled);
        self.inputs.lock().unwrap().push(input.clone());
        let failed = self.fail_once.swap(false, Ordering::AcqRel) || self.fail_always;
        Box::pin(async move {
            partials.publish(TextStreamChunk {
                text: "UNVERIFIED_PARTIAL".into(),
                offset: 0,
            })?;
            if failed {
                Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "llmResponseIncomplete",
                ))
            } else {
                Ok(PolishOutput::text(input))
            }
        })
    }
    fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct Progress(Mutex<Vec<EngineProgress>>);
impl EngineProgressSink for Progress {
    fn publish(&self, _: SessionId, progress: EngineProgress) -> Result<(), BackendError> {
        self.0.lock().unwrap().push(progress);
        Ok(())
    }
}

fn audio(seconds: usize) -> Arc<Vec<u8>> {
    let mut pcm = vec![1; seconds * crate::DICTATION_SAMPLE_RATE as usize * 2];
    pcm[..4].copy_from_slice(&[1, 0, 2, 0]);
    let len = pcm.len();
    pcm[len - 4..].copy_from_slice(&[8, 0, 9, 0]);
    Arc::new(pcm)
}

async fn run(
    pcm: Arc<Vec<u8>>,
    first: Result<String, BackendError>,
    polisher: Arc<dyn TextPolisher>,
    raw: bool,
) -> (
    Result<EngineResult, EngineFailure>,
    Arc<Archive>,
    Arc<Asr>,
    Arc<Progress>,
) {
    let archive = Arc::new(Archive {
        pcm,
        discarded: Arc::new(AtomicBool::new(false)),
    });
    let asr = Arc::new(Asr {
        first,
        received: Default::default(),
        calls: AtomicUsize::new(0),
    });
    let engine =
        PipelineDictationEngine::new(Arc::new(Recorder(archive.clone())), asr.clone(), polisher);
    let mut context = DictationContext::default();
    context.recording.archive_successful_recording = false;
    context.recording.transcribe_after_stop = true;
    context.asr_fallback = None;
    context.asr.provider_id = "fixture-asr".into();
    context.asr.provider_type = "fixture-asr".into();
    context.polish.polish_skip_max_chars = 0;
    context.polish.llm_thinking_enabled = true;
    context.llm.provider_id = "long-http-fixture".into();
    context.llm.provider_type = "custom".into();
    context.llm.model = Some("fixture-model".into());
    if raw {
        context.polish.mode = crate::PolishMode::Raw;
        context.polish.style_system_prompt =
            crate::style_packs::default_style_system_prompt_for_mode(crate::PolishMode::Raw);
    }
    let progress = Arc::new(Progress::default());
    let id = SessionId::new();
    engine
        .start(id, Arc::new(context), progress.clone())
        .await
        .unwrap();
    let result = engine.finish(id, progress.clone()).await;
    (result, archive, asr, progress)
}

#[tokio::test]
async fn long_recording_thirty_one_minutes_keeps_audio_and_retries_only_llm() {
    let raw = format!("{}尾部必须保留", "中文长录音。\n".repeat(3_000));
    let polisher = Arc::new(Polisher {
        fail_once: AtomicBool::new(true),
        ..Default::default()
    });
    let (result, archive, asr, progress) =
        run(audio(31 * 60), Ok(raw.clone()), polisher.clone(), false).await;
    let result = result.unwrap();
    assert_eq!(result.duration_ms, 31 * 60 * 1_000);
    assert_eq!(result.raw_text, raw);
    assert!(result.polished_text.ends_with("尾部必须保留"));
    assert!(!result.polish_failed);
    assert_eq!(result.has_audio_recording, Some(true));
    assert!(!archive.discarded.load(Ordering::Acquire));
    assert_eq!(asr.calls.load(Ordering::Acquire), 1);
    let inputs = polisher.inputs.lock().unwrap();
    assert_eq!(inputs[0], inputs[1]);
    assert_eq!(inputs[1..].concat(), raw);
    assert!(inputs.iter().all(|text| text.chars().count() <= 4_000));
    let events = progress.0.lock().unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineProgress::PolishDelta(delta) if !delta.is_final)));
    assert_eq!(events.iter().filter(|event| matches!(event, EngineProgress::PolishDelta(delta) if delta.is_final && delta.text.ends_with("尾部必须保留"))).count(), 1);
}

#[tokio::test]
async fn long_recording_asr_disconnect_replays_every_minute_from_start_through_tail() {
    let pcm = audio(31 * 60 + 1);
    let (result, archive, asr, _) = run(
        pcm.clone(),
        Err(BackendError::new(
            BackendErrorCode::Provider,
            "stream disconnected",
        )),
        Arc::new(Polisher::default()),
        false,
    )
    .await;
    let result = result.unwrap();
    let received = asr.received.lock().unwrap();
    assert_eq!(received.len(), 33);
    assert_eq!(received[1..].concat(), *pcm);
    assert!(received[1..]
        .iter()
        .all(|chunk| chunk.len() <= crate::long_recording::RECOVERY_CHUNK_BYTES));
    assert!(result.raw_text.starts_with("segment-1\n"));
    assert!(result.polished_text.ends_with("segment-32"));
    assert!(!archive.discarded.load(Ordering::Acquire));
}

#[tokio::test]
async fn long_recording_failed_llm_retains_whole_raw_text_and_original_audio() {
    let raw = format!("{}last-detail", "original detail. ".repeat(1_500));
    let polisher = Arc::new(Polisher {
        fail_always: true,
        ..Default::default()
    });
    let (result, archive, asr, _) = run(audio(301), Ok(raw.clone()), polisher.clone(), false).await;
    let result = result.unwrap();
    assert!(result.polish_failed);
    assert_eq!(result.raw_text, raw);
    assert_eq!(result.polished_text, raw);
    assert_eq!(asr.calls.load(Ordering::Acquire), 1);
    assert_eq!(polisher.inputs.lock().unwrap().len(), 2);
    assert!(!archive.discarded.load(Ordering::Acquire));
}

#[tokio::test]
async fn long_recording_threshold_keeps_normal_audio_policy_through_exactly_five_minutes() {
    for (seconds, retained) in [(300, false), (301, true)] {
        let (result, archive, asr, _) = run(
            audio(seconds),
            Ok("unchanged raw".into()),
            Arc::new(Polisher::default()),
            true,
        )
        .await;
        let result = result.unwrap();
        assert_eq!(result.polished_text, "unchanged raw");
        assert_eq!(archive.is_available(), retained);
        assert_eq!(asr.calls.load(Ordering::Acquire), 1);
    }
}

#[tokio::test]
async fn long_recording_real_http_truncation_is_retried_without_typing_partial_output() {
    use crate::{
        CredentialKey, CredentialNamespace, CredentialStore, InMemoryCredentialStore, SecretValue,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            let (body_at, length) = loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    return;
                }
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let header = std::str::from_utf8(&request[..end]).unwrap();
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while request.len() < body_at + length {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request: serde_json::Value =
                serde_json::from_slice(&request[body_at..body_at + length]).unwrap();
            let prompt = request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap();
            assert!(!prompt.contains("…[truncated]"));
            let input = prompt
                .split_once("<raw_transcript>\n")
                .unwrap()
                .1
                .split_once("\n</raw_transcript>")
                .unwrap()
                .0
                .to_string();
            let first = {
                let mut seen = seen.lock().unwrap();
                let first = seen.is_empty();
                seen.push(input.clone());
                first
            };
            let content = if first {
                "UNVERIFIED_HALF"
            } else {
                input.as_str()
            };
            let delta =
                serde_json::json!({"choices":[{"delta":{"content":content},"finish_reason":null}]});
            let terminal = serde_json::json!({"choices":[{"delta":{},"finish_reason":if first {"length"} else {"stop"}}]});
            let body = format!("data: {delta}\n\ndata: {terminal}\n\ndata: [DONE]\n\n");
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    let credentials = Arc::new(InMemoryCredentialStore::default());
    for (account, value) in [
        (
            crate::credentials::LLM_API_KEY_ACCOUNT,
            "fixture-key".to_string(),
        ),
        (crate::credentials::LLM_ENDPOINT_ACCOUNT, endpoint),
    ] {
        credentials
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some("long-http-fixture".into()),
                    account,
                )
                .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    let raw = format!("{}FINAL_COMPLETE_DETAIL", "全文内容。\n".repeat(3_000));
    let polisher = Arc::new(crate::SharedCloudTextPolisher::new(credentials));
    let (result, archive, asr, progress) = run(audio(301), Ok(raw.clone()), polisher, false).await;
    server.abort();
    let result = result.unwrap();
    assert!(!result.polish_failed);
    assert!(result.polished_text.ends_with("FINAL_COMPLETE_DETAIL"));
    assert!(!result.polished_text.contains("UNVERIFIED_HALF"));
    assert_eq!(asr.calls.load(Ordering::Acquire), 1);
    assert!(archive.is_available());
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[1..].concat(), raw);
    assert!(!progress
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|event| matches!(event, EngineProgress::PolishDelta(delta) if !delta.is_final)));
}
