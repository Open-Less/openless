//! Independent experimental engines. They never reuse/release the legacy ASR
//! caches, and all model I/O and inference runs off the audio/UI threads.
use futures_util::future::BoxFuture;
use openless_core::experimental_asr::{
    ExperimentalAsrModel as Model, ExperimentalAsrRuntime, ExperimentalAsrService,
};
use openless_core::{
    AudioConsumer, BackendError, BackendErrorCode, DictationContext, SessionId, TextStreamSink,
    TranscriptOutput, TranscriptionEngine, TranscriptionSession,
};
#[path = "experimental/compute_policy.rs"]
mod compute_policy;

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "experimental/textstream.rs"]
mod textstream;

#[cfg(all(debug_assertions, target_os = "macos", target_arch = "aarch64"))]
#[path = "experimental/acceptance.rs"]
pub(crate) mod acceptance;

fn failure(e: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, e.to_string())
}
struct Loaded {
    directory: PathBuf,
    cpu_threads: i32,
    engine: Engine,
}
enum Engine {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Unavailable,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Sense(sherpa_onnx::OfflineRecognizer),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    Gemma(gemma::Handle),
}
pub struct ExperimentalNativeRuntime {
    work: Arc<Mutex<()>>,
    loaded: [Arc<Mutex<Option<Loaded>>>; Model::ALL.len()],
    generation: [Arc<AtomicU64>; Model::ALL.len()],
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    textstream: textstream::Runtime,
}
fn index(model: Model) -> usize {
    match model {
        Model::SenseVoice => 0,
        Model::EmbeddingGemma => 1,
        Model::TextStream => 2,
    }
}
impl ExperimentalNativeRuntime {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            work: Arc::new(Mutex::new(())),
            loaded: std::array::from_fn(|_| Arc::new(Mutex::new(None))),
            generation: std::array::from_fn(|_| Arc::new(AtomicU64::new(0))),
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            textstream: textstream::Runtime::default(),
        })
    }
    pub fn unavailable(model: Model) -> Option<String> {
        match model {
            Model::TextStream if cfg!(all(target_os = "macos", target_arch = "aarch64")) => None,
            Model::TextStream => Some("appleSiliconMac14Required".into()),
            Model::SenseVoice if cfg!(target_os = "macos") => None,
            Model::SenseVoice => Some("desktopRuntimeRequired".into()),
            Model::EmbeddingGemma => {
                #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                {
                    // The shipped model, not merely the App bundle, requires macOS 15.
                    static MAJOR: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
                    let major = MAJOR.get_or_init(|| {
                        std::process::Command::new("/usr/bin/sw_vers")
                            .arg("-productVersion")
                            .output()
                            .ok()
                            .filter(|o| o.status.success())
                            .and_then(|o| String::from_utf8(o.stdout).ok())
                            .and_then(|s| s.trim().split('.').next()?.parse().ok())
                            .unwrap_or(0)
                    });
                    if *major >= 15 {
                        return None;
                    }
                }
                Some("appleSiliconMac15Required".into())
            }
        }
    }
    fn transcribe(
        &self,
        model: Model,
        directory: PathBuf,
        pcm: Vec<u8>,
        cancelled: Arc<AtomicBool>,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let loaded = Arc::clone(&self.loaded[index(model)]);
        let generation = Arc::clone(&self.generation[index(model)]);
        let expected = generation.load(Ordering::Acquire);
        let work = Arc::clone(&self.work);
        Box::pin(async move {
            let timeout_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                let _background = compute_policy::BackgroundScope::enter();
                let _work = work.try_lock().map_err(|_| {
                    BackendError::new(
                        BackendErrorCode::Busy,
                        "Another local model is computing; retry shortly",
                    )
                })?;
                let budget = compute_policy::Budget::capture();
                let pacer = compute_policy::Pacer::new(budget);
                let mut guard = loaded.lock().map_err(failure)?;
                let ready = guard
                    .as_mut()
                    .filter(|m| m.directory == directory)
                    .ok_or_else(|| failure("Experimental model was unloaded; enable it again"))?;
                let check = || {
                    if cancelled.load(Ordering::Acquire)
                        || generation.load(Ordering::Acquire) != expected
                    {
                        Err(BackendError::new(
                            BackendErrorCode::Cancelled,
                            "Experimental transcription cancelled",
                        ))
                    } else {
                        Ok(())
                    }
                };
                check()?;
                if pcm.len() % 2 != 0 {
                    return Err(failure("Invalid PCM length"));
                }
                let samples = pcm
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect::<Vec<_>>();
                let text = match &mut ready.engine {
                    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                    Engine::Unavailable => return Err(failure("Unsupported experimental runtime")),
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    Engine::Sense(recognizer) => {
                        let mut parts = Vec::new();
                        // Bound offline attention memory; the experiment does not claim native streaming.
                        for chunk in samples.chunks(16_000 * 20) {
                            check()?;
                            let stream = recognizer.create_stream();
                            stream.accept_waveform(16_000, chunk);
                            let decode_started = std::time::Instant::now();
                            recognizer.decode(&stream);
                            log::info!("[experimental-asr] model=sensevoice stage=decode audio_ms={} elapsed_ms={}", chunk.len() / 16, decode_started.elapsed().as_millis());
                            let text = stream
                                .get_result()
                                .ok_or_else(|| failure("SenseVoice returned no result"))?
                                .text;
                            parts.push(text);
                            if !pacer.checkpoint(|| check().is_err()) {
                                check()?;
                            }
                        }
                        openless_core::asr::whisper::join_transcript_chunks(&parts)
                    }
                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    Engine::Gemma(handle) => {
                        let result = handle.transcribe(&samples, budget, &|| check().is_err());
                        check()?;
                        result?
                    }
                };
                check()?;
                Ok(text)
            });
            // The worker owns the lock/model even after timeout. Release waits
            // for native work before deleting its mapped files.
            tokio::time::timeout(std::time::Duration::from_secs(180), task)
                .await
                .map_err(|_| {
                    timeout_cancelled.store(true, Ordering::Release);
                    failure("Experimental transcription timed out")
                })?
                .map_err(failure)?
        })
    }
}
impl ExperimentalAsrRuntime for ExperimentalNativeRuntime {
    fn unavailable_reason(&self, model: Model) -> Option<String> {
        Self::unavailable(model)
    }
    fn install(
        &self,
        model: Model,
        directory: PathBuf,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if model == Model::TextStream {
            return Box::pin(textstream::install(directory));
        }
        let _ = (model, directory);
        Box::pin(async { Ok(()) })
    }
    fn installed(&self, model: Model, directory: &std::path::Path) -> bool {
        if model == Model::TextStream {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            return textstream::installed(directory);
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            return false;
        }
        let _ = directory;
        true
    }
    fn prepare(
        &self,
        model: Model,
        directory: PathBuf,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let loaded = Arc::clone(&self.loaded[index(model)]);
        let generation = Arc::clone(&self.generation[index(model)]);
        let expected = generation.load(Ordering::Acquire);
        let work = Arc::clone(&self.work);
        Box::pin(async move {
            if let Some(reason) = Self::unavailable(model) {
                return Err(failure(reason));
            }
            tokio::task::spawn_blocking(move || {
                let _background = compute_policy::BackgroundScope::enter();
                let _work = work.try_lock().map_err(|_| {
                    BackendError::new(
                        BackendErrorCode::Busy,
                        "Another local model is computing; retry shortly",
                    )
                })?;
                let budget = compute_policy::Budget::capture();
                let mut slot = loaded.lock().map_err(failure)?;
                if generation.load(Ordering::Acquire) != expected {
                    return Err(failure("Experimental model was disabled while loading"));
                }
                if slot.as_ref().is_some_and(|v| {
                    v.directory == directory
                        && (model != Model::SenseVoice || v.cpu_threads <= budget.threads)
                }) {
                    return Ok(());
                }
                openless_core::experimental_asr::verify_package(model, &directory)?;
                // A changed thread budget must not construct a second native model
                // while the previous model/allocator is still resident.
                *slot = None;
                let loading = std::time::Instant::now();
                let engine = match model {
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    Model::SenseVoice => {
                        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
                        // One CPU thread avoids a persistent ONNX worker pool and
                        // keeps the cached model valid as the idle budget fluctuates.
                        config.model_config.num_threads = 1;
                        config.model_config.provider = Some("cpu".into());
                        config.model_config.tokens =
                            Some(directory.join("tokens.txt").to_string_lossy().into());
                        config.model_config.sense_voice =
                            sherpa_onnx::OfflineSenseVoiceModelConfig {
                                model: Some(
                                    directory.join("model.int8.onnx").to_string_lossy().into(),
                                ),
                                language: Some("auto".into()),
                                use_itn: true,
                            };
                        Engine::Sense(
                            sherpa_onnx::OfflineRecognizer::create(&config)
                                .ok_or_else(|| failure("Unable to load SenseVoice"))?,
                        )
                    }
                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    Model::EmbeddingGemma => {
                        Engine::Gemma(gemma::Handle::load(&directory, budget, &|| {
                            generation.load(Ordering::Acquire) != expected
                        })?)
                    }
                    #[allow(unreachable_patterns)]
                    _ => return Err(failure("Unsupported experimental runtime")),
                };
                if generation.load(Ordering::Acquire) != expected {
                    return Err(failure("Experimental model was disabled while loading"));
                }
                log::info!(
                    "[experimental-asr] model={model:?} stage=load elapsed_ms={}",
                    loading.elapsed().as_millis()
                );
                *slot = Some(Loaded {
                    directory,
                    cpu_threads: if model == Model::SenseVoice {
                        1
                    } else {
                        budget.threads
                    },
                    engine,
                });
                Ok(())
            })
            .await
            .map_err(failure)?
        })
    }
    fn release(&self, model: Model) -> BoxFuture<'static, Result<(), BackendError>> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if model == Model::TextStream {
            return self.textstream.stop();
        }
        self.generation[index(model)].fetch_add(1, Ordering::AcqRel);
        let loaded = Arc::clone(&self.loaded[index(model)]);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                loaded.lock().map_err(failure)?.take();
                Ok(())
            })
            .await
            .map_err(failure)?
        })
    }
}

pub struct ExperimentalTranscriptionEngine {
    pub models: Arc<ExperimentalAsrService>,
    pub runtime: Arc<ExperimentalNativeRuntime>,
}
impl TranscriptionEngine for ExperimentalTranscriptionEngine {
    fn start(
        &self,
        _id: SessionId,
        context: Arc<DictationContext>,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let models = Arc::clone(&self.models);
        let runtime = Arc::clone(&self.runtime);
        Box::pin(async move {
            let model = Model::from_provider(context.asr.provider_type.as_str())
                .ok_or_else(|| failure("Unknown experimental provider"))?;
            let (directory, generation) = models.lease(model)?;
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            if model == Model::TextStream {
                return runtime
                    .textstream
                    .start(directory, models, generation, _partials)
                    .await;
            }
            runtime.prepare(model, directory.clone()).await?;
            models.check_lease(model, generation)?;
            Ok(Arc::new(ExperimentalSession {
                model,
                generation,
                directory,
                models,
                runtime,
                pcm: Mutex::new(Vec::new()),
                cancelled: Arc::new(AtomicBool::new(false)),
                overflowed: AtomicBool::new(false),
                finished: AtomicBool::new(false),
            }) as Arc<dyn TranscriptionSession>)
        })
    }
}
struct ExperimentalSession {
    model: Model,
    generation: u64,
    directory: PathBuf,
    models: Arc<ExperimentalAsrService>,
    runtime: Arc<ExperimentalNativeRuntime>,
    pcm: Mutex<Vec<u8>>,
    cancelled: Arc<AtomicBool>,
    overflowed: AtomicBool,
    finished: AtomicBool,
}
impl AudioConsumer for ExperimentalSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if self.cancelled.load(Ordering::Acquire) || self.finished.load(Ordering::Acquire) {
            return;
        }
        let mut buffer = self.pcm.lock().unwrap();
        if buffer.len().saturating_add(pcm.len()) > 32_000 * 600 {
            self.overflowed.store(true, Ordering::Release);
            return;
        }
        buffer.extend_from_slice(pcm);
    }
}
impl TranscriptionSession for ExperimentalSession {
    fn asr_call_label(&self) -> Option<openless_core::AsrCallLabel> {
        Some(openless_core::AsrCallLabel::new(
            self.model.provider_type(),
            Some(self.model.model_id().into()),
        ))
    }
    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        let invalid =
            self.finished.swap(true, Ordering::AcqRel) || self.overflowed.load(Ordering::Acquire);
        let pcm = std::mem::take(&mut *self.pcm.lock().unwrap());
        let cancelled = Arc::clone(&self.cancelled);
        let models = Arc::clone(&self.models);
        let runtime = Arc::clone(&self.runtime);
        let model = self.model;
        let generation = self.generation;
        let directory = self.directory.clone();
        Box::pin(async move {
            if invalid {
                return Err(failure(
                    "Recording already finalized or exceeds the experimental ten-minute limit",
                ));
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(failure("Transcription cancelled"));
            }
            models.check_lease(model, generation)?;
            let duration_ms = pcm.len() as u64 / 32;
            if pcm.is_empty() {
                return Ok(TranscriptOutput {
                    text: String::new(),
                    duration_ms,
                });
            }
            let text = runtime
                .transcribe(model, directory, pcm, Arc::clone(&cancelled))
                .await?;
            if cancelled.load(Ordering::Acquire) {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Transcription cancelled",
                ));
            }
            models.check_lease(model, generation)?; // Disable/re-enable cannot leak stale text.
            Ok(TranscriptOutput { text, duration_ms })
        })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.cancelled.store(true, Ordering::Release);
        self.pcm.lock().unwrap().clear();
        Box::pin(async { Ok(()) })
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;
    async fn smoke(model: Model, variable: &str) {
        let directory = PathBuf::from(
            std::env::var_os(variable)
                .expect("set the model-directory environment variable for this ignored smoke test"),
        );
        let runtime = ExperimentalNativeRuntime::new();
        let started = std::time::Instant::now();
        runtime.prepare(model, directory.clone()).await.unwrap();
        let load = started.elapsed();
        let warm_started = std::time::Instant::now();
        runtime.prepare(model, directory.clone()).await.unwrap();
        let warm_prepare = warm_started.elapsed();
        let wav = std::fs::read(
            std::env::var_os("OPENLESS_ASR_ENGLISH_TEST_WAV")
                .expect("provide the English 16kHz PCM smoke-test WAV"),
        )
        .unwrap();
        let samples = super::super::test_run::decode_wav_16k_mono(&wav).unwrap();
        let pcm = samples
            .iter()
            .flat_map(|s| ((s * 32768.0).clamp(-32768.0, 32767.0) as i16).to_le_bytes())
            .collect();
        let inference = std::time::Instant::now();
        let text = runtime
            .transcribe(model, directory, pcm, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        println!(
            "{model:?}: load={load:?} warm_prepare={warm_prepare:?} inference={:?} audio={:.3}s text={text}",
            inference.elapsed(),
            samples.len() as f64 / 16000.0
        );
        let normalized = text.to_lowercase();
        assert!(
            normalized.contains("hello")
                && normalized.contains("test")
                && normalized.contains("system"),
            "unexpected fixture transcription: {text}"
        );
        runtime.release(model).await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires pinned SenseVoice package; no download in the test"]
    async fn sensevoice_recognizes_public_fixture() {
        smoke(Model::SenseVoice, "OPENLESS_SENSEVOICE_MODEL_DIR").await;
    }
    #[tokio::test]
    #[ignore = "requires the pinned SenseVoice package and a 16 kHz Mandarin fixture"]
    async fn sensevoice_recognizes_mandarin() {
        let directory = PathBuf::from(
            std::env::var_os("OPENLESS_SENSEVOICE_MODEL_DIR").expect("model directory"),
        );
        let wav = std::fs::read(
            std::env::var_os("OPENLESS_SENSEVOICE_CHINESE_WAV").expect("Mandarin WAV path"),
        )
        .unwrap();
        let samples = super::super::test_run::decode_wav_16k_mono(&wav).unwrap();
        let pcm = samples
            .iter()
            .flat_map(|s| ((s * 32768.0).clamp(-32768.0, 32767.0) as i16).to_le_bytes())
            .collect();
        let runtime = ExperimentalNativeRuntime::new();
        runtime
            .prepare(Model::SenseVoice, directory.clone())
            .await
            .unwrap();
        let text = runtime
            .transcribe(
                Model::SenseVoice,
                directory,
                pcm,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        println!("SenseVoice Mandarin: {text}");
        assert!(
            text.chars()
                .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
                .count()
                >= 4
        );
        runtime.release(Model::SenseVoice).await.unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    #[ignore = "requires pinned English Core ML package and macOS 15+"]
    async fn gemma_recognizes_public_fixture() {
        smoke(Model::EmbeddingGemma, "OPENLESS_GEMMA_MODEL_DIR").await;
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "experimental/gemma.rs"]
mod gemma;
