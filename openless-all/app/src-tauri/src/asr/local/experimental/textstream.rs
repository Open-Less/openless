//! TextStream's optional MLX worker, isolated from the application process.
//! No process/socket/native import exists until a recording explicitly starts.
#[path = "textstream/install.rs"]
mod install;
#[path = "textstream/process.rs"]
mod process;
#[path = "textstream/protocol.rs"]
mod protocol;
#[path = "textstream/package_cache.rs"]
mod package_cache;
use process::Process;

use base64::Engine;
use futures_util::future::BoxFuture;
use openless_core::{
    experimental_asr::{ExperimentalAsrModel as Model, ExperimentalAsrService},
    AudioConsumer, BackendError, BackendErrorCode, TextStreamChunk, TextStreamSink,
    TranscriptOutput, TranscriptionSession,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
};
use tokio::sync::oneshot;

pub use install::{install, installed};
const MAX_LINE: u64 = protocol::MAX_EVENT_BYTES as u64;
const MAX_QUEUED_BYTES: usize = 32_000 * 600; // Ten-minute capture/retranscribe cap; no dropped speech.
fn error(message: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message.to_string())
}

struct StartupGuard(Option<Arc<Process>>);
impl Drop for StartupGuard {
    fn drop(&mut self) {
        if let Some(process) = &self.0 {
            process.stop();
        }
    }
}

#[derive(Default)]
pub struct Runtime {
    active: Mutex<Option<Weak<Process>>>,
    generation: AtomicU64,
    verification: Arc<package_cache::VerificationCache>,
}
impl Runtime {
    #[cfg(debug_assertions)]
    pub(super) fn is_running(&self) -> bool {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|process| process.running())
    }

    fn check_start_generation(&self, expected: u64) -> Result<(), BackendError> {
        if self.generation.load(Ordering::Acquire) != expected {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "TextStream channel changed while loading",
            ));
        }
        Ok(())
    }

    pub fn stop(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        // A channel release also invalidates file preflight that has not yet
        // spawned a child, even when the experiment itself remains opted in.
        self.generation.fetch_add(1, Ordering::AcqRel);
        let process = self
            .active
            .lock()
            .unwrap()
            .take()
            .and_then(|weak| weak.upgrade());
        Box::pin(async move {
            if let Some(process) = process {
                tokio::task::spawn_blocking(move || process.stop())
                    .await
                    .map_err(error)?;
            }
            Ok(())
        })
    }

    pub async fn start(
        &self,
        directory: PathBuf,
        models: Arc<ExperimentalAsrService>,
        generation: u64,
        partials: Arc<dyn TextStreamSink>,
    ) -> Result<Arc<dyn TranscriptionSession>, BackendError> {
        let expected_runtime = self.generation.load(Ordering::Acquire);
        install::refresh_worker(&directory).await?;
        if !installed(&directory) {
            return Err(error(
                "TextStream runtime is not installed; explicitly retry its experimental switch",
            ));
        }
        let started = std::time::Instant::now();
        let verify = directory.clone();
        let verification = Arc::clone(&self.verification);
        let budget = tokio::task::spawn_blocking(move || {
            let _background = super::compute_policy::BackgroundScope::enter();
            let cached = verification.verify(Model::TextStream, &verify)?;
            log::info!(
                "[experimental-asr] model=textstream stage=verify cached={cached} elapsed_ms={}",
                started.elapsed().as_millis()
            );
            Ok::<_, BackendError>(super::compute_policy::Budget::capture())
        })
        .await
        .map_err(error)??;
        models.check_lease(Model::TextStream, generation)?;
        let (process, stdin, stdout) = {
            let mut active = self.active.lock().unwrap();
            self.check_start_generation(expected_runtime)?;
            models.check_lease(Model::TextStream, generation)?;
            if active
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|process| process.running())
            {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "TextStream is already recording",
                ));
            }
            let mut command = Command::new(install::python(&directory));
            command
                .arg("-I")
                .arg(install::script(&directory))
                .arg("--model-dir")
                .arg(&directory)
                .arg("--parent")
                .arg(std::process::id().to_string())
                .arg("--port")
                .arg("7890")
                .current_dir(&directory)
                .env_remove("PYTHONPATH")
                .env_remove("PYTHONHOME")
                .env("HF_HUB_OFFLINE", "1")
                .env("TRANSFORMERS_OFFLINE", "1")
                .env("HF_HUB_DISABLE_TELEMETRY", "1")
                .env("PYTHONUNBUFFERED", "1")
                .env("OPENLESS_CPU_BUDGET_CORES", budget.cores.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0);
            let mut child = command.spawn().map_err(error)?;
            let stdin = child.stdin.take().expect("worker stdin");
            let stdout = child.stdout.take().expect("worker stdout");
            let process = Arc::new(Process {
                child: Mutex::new(Some(child)),
            });
            *active = Some(Arc::downgrade(&process));
            (process, stdin, stdout)
        };
        // Dropping a cancelled/timed-out startup kills even a still-loading worker.
        let mut guard = StartupGuard(Some(process.clone()));
        let (ready_tx, ready_rx) = oneshot::channel();
        let (result_tx, result_rx) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(AudioQueue::default());
        let writer_process = process.clone();
        let writer_failed = failed.clone();
        let writer_queue = queue.clone();
        std::thread::spawn(move || {
            if let Err(_error) = write_audio(stdin, writer_queue) {
                writer_failed.store(true, Ordering::Release);
                writer_process.stop();
            }
        });
        let reader_process = process.clone();
        let reader_cancelled = cancelled.clone();
        let reader_models = models.clone();
        let reader_queue = queue.clone();
        std::thread::spawn(move || {
            let mut ready = Some(ready_tx);
            let result = read_events(
                stdout,
                &reader_process,
                &reader_models,
                generation,
                &reader_cancelled,
                partials.as_ref(),
                &mut ready,
            );
            // EOF, failed loading, cancellation and channel disable must also
            // wake the idle writer; killing a process alone leaves it parked.
            reader_queue.cancel();
            if let Some(ready) = ready {
                let message = result
                    .as_ref()
                    .err()
                    .map(|error| error.message.as_str())
                    .unwrap_or("TextStream failed before loading");
                let _ = ready.send(Err(error(message)));
            }
            if result.is_err() {
                reader_process.stop();
            }
            let _ = result_tx.send(result);
        });
        tokio::time::timeout(std::time::Duration::from_secs(120), ready_rx)
            .await
            .map_err(|_| error("TextStream model loading timed out"))?
            .map_err(error)??;
        models.check_lease(Model::TextStream, generation)?;
        self.check_start_generation(expected_runtime)?;
        log::info!(
            "[experimental-asr] model=textstream stage=ready elapsed_ms={}",
            started.elapsed().as_millis()
        );
        guard.0.take();
        Ok(Arc::new(Session {
            process,
            queue,
            result: Mutex::new(Some(result_rx)),
            models,
            generation,
            cancelled,
            failed,
            audio_bytes: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        }))
    }
}

#[derive(Default)]
struct Pending {
    bytes: std::collections::VecDeque<u8>,
    finish: bool,
    cancel: bool,
}
#[derive(Default)]
struct AudioQueue {
    pending: Mutex<Pending>,
    wake: Condvar,
}
impl AudioQueue {
    fn push(&self, bytes: &[u8]) -> bool {
        let mut pending = self.pending.lock().unwrap();
        if pending.finish
            || pending.cancel
            || pending.bytes.len().saturating_add(bytes.len()) > MAX_QUEUED_BYTES
        {
            return false;
        }
        pending.bytes.extend(bytes);
        self.wake.notify_one();
        true
    }
    fn finish(&self) {
        self.pending.lock().unwrap().finish = true;
        self.wake.notify_one();
    }
    fn cancel(&self) {
        let mut pending = self.pending.lock().unwrap();
        pending.cancel = true;
        pending.bytes.clear();
        self.wake.notify_one();
    }
}
fn write_audio(mut input: impl Write, queue: Arc<AudioQueue>) -> std::io::Result<()> {
    loop {
        let (bytes, finished) = {
            let mut pending = queue.pending.lock().unwrap();
            while pending.bytes.len() < 16_000 && !pending.finish && !pending.cancel {
                pending = queue.wake.wait(pending).unwrap();
            }
            if pending.cancel {
                return Ok(());
            }
            let count = pending.bytes.len().min(16_000);
            let bytes = pending.bytes.drain(..count).collect::<Vec<_>>();
            let finished = pending.finish && pending.bytes.is_empty();
            (bytes, finished)
        };
        if !bytes.is_empty() {
            let line = serde_json::json!({"type":"audio", "pcm":base64::engine::general_purpose::STANDARD.encode(&bytes)});
            writeln!(input, "{line}")?;
            input.flush()?;
        }
        if finished {
            writeln!(input, "{{\"type\":\"finish\"}}")?;
            input.flush()?;
            return Ok(());
        }
    }
}

fn read_events(
    output: impl Read,
    process: &Process,
    models: &ExperimentalAsrService,
    generation: u64,
    cancelled: &AtomicBool,
    partials: &dyn TextStreamSink,
    ready: &mut Option<oneshot::Sender<Result<(), BackendError>>>,
) -> Result<String, BackendError> {
    let mut output = BufReader::new(output);
    let mut decoder = protocol::Decoder::default();
    loop {
        let mut line = String::new();
        let n = (&mut output)
            .take(MAX_LINE + 1)
            .read_line(&mut line)
            .map_err(error)?;
        if n == 0 {
            break;
        }
        if n as u64 > MAX_LINE {
            return Err(error("Oversized TextStream response"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(error("TextStream cancelled"));
        }
        models.check_lease(Model::TextStream, generation)?;
        match decoder.push(&line)? {
            protocol::Update::Ready => {
                let _ = ready.take().unwrap().send(Ok(()));
            }
            protocol::Update::Preview { finalized, draft } => {
                // Offset zero is a replacement: drafts may be corrected/shrunk.
                partials.publish(TextStreamChunk {
                    text: finalized + &draft,
                    offset: 0,
                })?;
            }
            protocol::Update::Finalized => {}
        }
    }
    // Wait for successful worker shutdown/SSE drain, not merely a last draft.
    process.completed()?;
    decoder.finish()
}

struct Session {
    process: Arc<Process>,
    queue: Arc<AudioQueue>,
    result: Mutex<Option<oneshot::Receiver<Result<String, BackendError>>>>,
    models: Arc<ExperimentalAsrService>,
    generation: u64,
    cancelled: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    audio_bytes: AtomicU64,
    finished: AtomicBool,
}
impl AudioConsumer for Session {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if self.cancelled.load(Ordering::Acquire)
            || self.finished.load(Ordering::Acquire)
            || self.failed.load(Ordering::Acquire)
        {
            return;
        }
        let total = self
            .audio_bytes
            .fetch_add(pcm.len() as u64, Ordering::AcqRel)
            + pcm.len() as u64;
        if pcm.len() % 2 != 0 || total > 32_000 * 600 {
            self.failed.store(true, Ordering::Release);
            return;
        }
        if !self.queue.push(pcm) {
            self.failed.store(true, Ordering::Release);
        }
    }
}
impl TranscriptionSession for Session {
    fn asr_call_label(&self) -> Option<openless_core::AsrCallLabel> {
        Some(openless_core::AsrCallLabel::new(
            Model::TextStream.provider_type(),
            Some(Model::TextStream.model_id().into()),
        ))
    }
    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        let repeated = self.finished.swap(true, Ordering::AcqRel);
        let result = self.result.lock().unwrap().take();
        let queue = self.queue.clone();
        let process = self.process.clone();
        let failed = self.failed.clone();
        let cancelled = self.cancelled.clone();
        let models = self.models.clone();
        let generation = self.generation;
        let duration_ms = self.audio_bytes.load(Ordering::Acquire) / 32;
        Box::pin(async move {
            let outcome = async {
                if repeated || failed.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire) {
                    return Err(error(
                        "TextStream cancelled or could not keep up; audio was not silently dropped",
                    ));
                }
                queue.finish();
                let text = result
                    .ok_or_else(|| error("TextStream already finalized"))?
                    .await
                    .map_err(error)??;
                models.check_lease(Model::TextStream, generation)?;
                if cancelled.load(Ordering::Acquire) {
                    return Err(error("TextStream cancelled"));
                }
                Ok(TranscriptOutput { text, duration_ms })
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(180), outcome)
                .await
                .unwrap_or_else(|_| Err(error("TextStream finalization timed out")));
            queue.cancel();
            tokio::task::spawn_blocking(move || process.stop())
                .await
                .map_err(error)?;
            result
        })
    }
    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.cancelled.store(true, Ordering::Release);
        self.queue.cancel();
        let process = self.process.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || process.stop())
                .await
                .map_err(error)?;
            Ok(())
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.queue.cancel();
        self.process.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_pcm_is_drained_before_finish_and_can_be_cancelled_without_finalizing() {
        let queue = Arc::new(AudioQueue::default());
        let audio = vec![0x12; 48_002];
        assert!(queue.push(&audio[..30_000]));
        assert!(queue.push(&audio[30_000..]));
        queue.finish();
        assert!(!queue.push(&[0, 0]));
        let mut wire = Vec::new();
        write_audio(&mut wire, queue).unwrap();
        let lines: Vec<serde_json::Value> = String::from_utf8(wire)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.last().unwrap()["type"], "finish");
        let decoded: Vec<u8> = lines[..lines.len() - 1]
            .iter()
            .flat_map(|line| {
                base64::engine::general_purpose::STANDARD
                    .decode(line["pcm"].as_str().unwrap())
                    .unwrap()
            })
            .collect();
        assert_eq!(decoded, audio);
        let queue = Arc::new(AudioQueue::default());
        queue.push(&[1, 0]);
        queue.cancel();
        let mut cancelled = Vec::new();
        write_audio(&mut cancelled, queue).unwrap();
        assert!(cancelled.is_empty());
    }

    #[test]
    fn startup_cancellation_kills_only_the_owned_process() {
        let child = Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let process = Arc::new(Process {
            child: Mutex::new(Some(child)),
        });
        let mut unrelated = Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        {
            let _guard = StartupGuard(Some(process.clone()));
            assert!(process.running());
        }
        assert!(!process.running());
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }

    #[tokio::test]
    async fn channel_release_invalidates_preflight_before_a_child_exists() {
        let runtime = Runtime::default();
        let expected = runtime.generation.load(Ordering::Acquire);
        runtime.check_start_generation(expected).unwrap();
        runtime.stop().await.unwrap();
        assert_eq!(
            runtime.check_start_generation(expected).unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(!runtime.is_running());
        runtime
            .check_start_generation(runtime.generation.load(Ordering::Acquire))
            .unwrap();
    }

    #[test]
    fn cancellation_wakes_an_idle_audio_writer() {
        let queue = Arc::new(AudioQueue::default());
        let writer_queue = queue.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut wire = Vec::new();
            write_audio(&mut wire, writer_queue).unwrap();
            tx.send(wire).unwrap();
        });
        queue.cancel();
        assert!(rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .is_empty());
        writer.join().unwrap();
    }

    #[tokio::test]
    #[ignore = "explicit isolated runtime installation; downloads Python/wheels but never model weights"]
    async fn install_optional_environment_without_importing_inference() {
        let directory = PathBuf::from(
            std::env::var_os("OPENLESS_TEXTSTREAM_INSTALL_TEST_DIR")
                .expect("isolated test directory"),
        );
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("openless-textstream-install-"));
        install::install(directory.clone()).await.unwrap();
        assert!(install::installed(&directory));
        let output = Command::new(install::python(&directory)).args(["-I", "-c",
            "import importlib.metadata as m,sys; assert not any(n.startswith('mlx') for n in sys.modules); print(m.version('mlx-qwen3-asr')); print(m.version('mlx')); print(m.version('silero-vad-lite'))"])
            .output().unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["0.4.4", "0.32.3", "0.4.0"]
        );
    }
}
