//! Microphone capture: cpal stream pull -> 16 kHz mono Int16 PCM -> fed to `AudioConsumer`.
//!
//! Behavior-aligned with the Swift version `OpenLessRecorder/Recorder.swift`:
//! - Output is fixed at 16 kHz mono little-endian Int16 so ASR can consume it directly.
//! - Multi-channel input is downmixed to mono by arithmetic average; non-16 kHz input is
//!   resampled by linear interpolation.
//! - Each buffer computes RMS normalized to 0..1 (times 4, clamped) for the capsule level animation.
//! - Every ~50 callbacks one diagnostic log line is emitted, including peak RMS.
//!
//! Threading model:
//! - cpal `Stream` is `!Send`, so a dedicated thread owns it.
//! - The main thread signals "stop" via `AtomicBool` and `join`s the thread; the stream is
//!   `drop`ped inside that thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;
use serde::Serialize;
use thiserror::Error;

/// Target sample rate (matches the Swift-side constant; do not change).
const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Emit one diagnostic log line every N callbacks.
const LOG_EVERY_N_CALLBACKS: usize = 50;
/// RMS -> UI level gain, matching Swift's `min(1.0, rms * 4)`.
const LEVEL_RMS_GAIN: f32 = 4.0;
/// Bound capture backlog by duration and memory, independently of callback size.
const AUDIO_PROCESSING_MAX_QUEUED_US: u64 = 3_000_000;
const AUDIO_PROCESSING_MAX_QUEUED_BYTES: usize = 8 * 1024 * 1024;
const AUDIO_PROCESSING_QUEUE_CAPACITY: usize = 4096;
/// Shed optional enhancement before a temporary backlog can stop capture.
const AUDIO_PROCESSING_SHED_ENHANCEMENT_US: u64 = 500_000;
static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);
/// Max queued PCM chunks for the archive writer thread. When full, new archive chunks are
/// dropped instead of blocking the realtime callback; ASR still receives the full PCM, and
/// archive messages accepted before stop are fully flushed.
const WAV_ARCHIVE_QUEUE_CAPACITY: usize = 256;
/// The provisional capture cap is 500 ms at the canonical 16 kHz mono i16 format.
pub(crate) const STAGED_CAPTURE_MAX_BYTES: usize = TARGET_SAMPLE_RATE as usize;
/// Voice handoff staging keeps up to 30 seconds while the formal dictation session is prepared.
const VOICE_STAGED_CAPTURE_MAX_BYTES: usize = TARGET_SAMPLE_RATE as usize * 30 * 2;
/// Voice activation wake/finish inference is fed only after this cheap RMS gate passes.
/// The level uses the same normalized 0..1 scale as `pcm_rms_level`.
const ACTIVATION_VAD_RMS_THRESHOLD: f32 = 0.015;
/// Retain a short silence pre-roll so the first consonant is not lost at the gate boundary.
const ACTIVATION_VAD_PRE_ROLL_BYTES: usize = TARGET_SAMPLE_RATE as usize * 2 / 5;
/// Keep sending a few quiet chunks after speech so phrase tails reach the recognizer.
const ACTIVATION_VAD_HANGOVER_CHUNKS: usize = 8;
const VOICE_CAPTURE_KEY_PREFIX: u64 = 1 << 63;

// [DEBUG-VOICE-1003] Temporary P1-P6 diagnostics; remove only after user confirmation.
// Audio/model work never waits for disk I/O. Only the dedicated NDJSON writer opens a file.
pub(crate) fn voice_diagnostic(
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event: &'static str,
    outcome: &'static str,
    safe: serde_json::Value,
) {
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use std::io::Write;
        static WRITER: OnceLock<Option<SyncSender<serde_json::Value>>> = OnceLock::new();
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        static DROPPED: AtomicU64 = AtomicU64::new(0);
        static RUN: OnceLock<String> = OnceLock::new();
        let run = RUN.get_or_init(|| {
            format!(
                "r{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
        let writer = WRITER.get_or_init(|| {
            let (tx, rx) = sync_channel::<serde_json::Value>(4096);
            std::thread::Builder::new().name("voice-1003-diagnostics".into()).spawn(move || {
                let path = std::env::temp_dir().join("openless-1003-3c02ffc1.ndjson");
                let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
                    return;
                };
                for record in rx {
                    let Ok(mut line) = serde_json::to_vec(&record) else { continue; };
                    line.push(b'\n');
                    if file.write_all(&line).is_err() { break; }
                }
            }).ok().map(|_| tx)
        });
        if let Some(writer) = writer {
            let record = serde_json::json!({
                "schema": "diagnosing-bugs.v1", "session_id": "voice-1003-3c02ffc1",
                "attempt_id": "A2-terminal-tail", "change_id": "C2-terminal-padding",
                "run_id": run, "probe_id": probe_id, "hypothesis_ids": hypothesis_ids,
                "event": event, "outcome": outcome, "safe": safe,
                "seq": SEQUENCE.fetch_add(1, Ordering::Relaxed),
                "dropped": DROPPED.load(Ordering::Relaxed),
                "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default().as_millis().to_string(),
            });
            if writer.try_send(record).is_err() { DROPPED.fetch_add(1, Ordering::Relaxed); }
        }
    }
    #[cfg(any(not(target_os = "windows"), test))]
    let _ = (probe_id, hypothesis_ids, event, outcome, safe);
}

/// Low-memory mode suppresses optional prewarming, not an explicitly active voice listener.
pub(crate) fn warm_capture_enabled(
    low_latency: bool,
    low_memory: bool,
    voice_activation: bool,
) -> bool {
    voice_activation || (low_latency && !low_memory)
}

fn voice_capture_key(generation: u64) -> u64 {
    generation | VOICE_CAPTURE_KEY_PREFIX
}

/// Downstream that receives resampled Int16 PCM bytes (little-endian).
pub trait AudioConsumer: Send + Sync {
    /// Each delivery is a little-endian byte sequence of Int16 samples;
    /// the length is always a multiple of 2.
    fn consume_pcm_chunk(&self, pcm: &[u8]);

    /// Mark the end of the current VAD-gated segment without stopping the consumer.
    /// Consumers that do not maintain streaming endpoint state can ignore this boundary.
    fn finish_segment(&self) {}
}

pub(crate) struct AudioConsumerFanout {
    consumers: Vec<Arc<dyn AudioConsumer>>,
}

impl AudioConsumerFanout {
    pub(crate) fn new(consumers: Vec<Arc<dyn AudioConsumer>>) -> Arc<Self> {
        Arc::new(Self { consumers })
    }
}

impl AudioConsumer for AudioConsumerFanout {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        for consumer in &self.consumers {
            consumer.consume_pcm_chunk(pcm);
        }
    }

    fn finish_segment(&self) {
        for consumer in &self.consumers {
            consumer.finish_segment();
        }
    }
}

/// Compatibility bridge for legacy recorder call sites while cloud ASR
/// implementations live in the framework-independent core.
impl<T> AudioConsumer for T
where
    T: openless_core::AudioConsumer + ?Sized,
{
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        openless_core::AudioConsumer::consume_pcm_chunk(self, pcm);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneDevice {
    pub name: String,
    pub is_default: bool,
}

/// Capture error.
#[derive(Debug, Error, Clone)]
pub enum RecorderError {
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("no microphone input device detected")]
    NoInputDevice,
    #[error("audio engine failed: {0}")]
    EngineFailed(String),
}

impl RecorderError {
    /// User-facing startup error message: clear guidance for missing device / permission,
    /// original engine error text preserved for the rest to ease diagnosis.
    pub fn user_message(&self) -> String {
        match self {
            RecorderError::NoInputDevice => "未检测到麦克风，请连接麦克风后重试".to_string(),
            RecorderError::PermissionDenied => {
                "需要麦克风权限，请在系统设置中允许 OpenLess 使用麦克风".to_string()
            }
            other => format!("录音启动失败: {other}"),
        }
    }
}

enum WavArchiveMessage {
    Pcm(Vec<u8>),
    Finish,
}

/// WAV archive entry point off the realtime audio callback. The callback only copies PCM and
/// pushes messages into a bounded channel; file writing, seek, and sync all run on a dedicated
/// thread.
struct WavArchiveWriter {
    sender: SyncSender<WavArchiveMessage>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
    queue_full_warned: AtomicBool,
}

impl WavArchiveWriter {
    fn create(path: &Path) -> std::io::Result<Self> {
        let archiver = WavArchiver::create(path)?;
        let (sender, receiver) = sync_channel::<WavArchiveMessage>(WAV_ARCHIVE_QUEUE_CAPACITY);
        let join_handle = thread::Builder::new()
            .name("openless-wav-archive".into())
            .spawn(move || run_wav_archive_writer(archiver, receiver))?;
        Ok(Self {
            sender,
            join_handle: Mutex::new(Some(join_handle)),
            queue_full_warned: AtomicBool::new(false),
        })
    }

    fn append(&self, pcm_bytes: &[u8]) {
        match self
            .sender
            .try_send(WavArchiveMessage::Pcm(pcm_bytes.to_vec()))
        {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                if !self.queue_full_warned.swap(true, Ordering::Relaxed) {
                    log::warn!(
                        "[recorder] wav archive queue is full; dropping archive PCM until the writer catches up"
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                if !self.queue_full_warned.swap(true, Ordering::Relaxed) {
                    log::warn!("[recorder] wav archive writer stopped before PCM was queued");
                }
            }
        }
    }

    fn finish(&self) {
        let _ = self.sender.send(WavArchiveMessage::Finish);
        if let Some(handle) = self.join_handle.lock().take() {
            if let Err(error) = handle.join() {
                log::warn!("[recorder] wav archive writer join failed: {error:?}");
            }
        }
    }
}

#[derive(Clone)]
struct ForwardTarget {
    consumer: Arc<dyn AudioConsumer>,
    level_handler: Arc<dyn Fn(f32) + Send + Sync>,
    archive: Option<Arc<WavArchiveWriter>>,
}

enum CaptureSinkState {
    Staging { pcm: VecDeque<u8> },
    Promoted(ForwardTarget),
    Discarded,
}

/// Callback sink used while Core prepares the provider. The delivery gate makes
/// promotion linearizable without ever invoking downstream code under the state lock.
struct CaptureSink {
    state: Mutex<CaptureSinkState>,
    delivery_gate: Mutex<()>,
    max_bytes: usize,
}

impl CaptureSink {
    fn new(max_bytes: usize) -> Arc<Self> {
        let max_bytes = (max_bytes.max(2) & !1).min(STAGED_CAPTURE_MAX_BYTES.max(2));
        Arc::new(Self {
            state: Mutex::new(CaptureSinkState::Staging {
                pcm: VecDeque::with_capacity(max_bytes),
            }),
            delivery_gate: Mutex::new(()),
            max_bytes,
        })
    }

    fn deliver(target: &ForwardTarget, pcm: &[u8]) {
        target.consumer.consume_pcm_chunk(pcm);
        if let Some(archive) = target.archive.as_ref() {
            archive.append(pcm);
        }
    }

    fn promote(
        &self,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        archive: Option<Arc<WavArchiveWriter>>,
    ) -> Result<(), RecorderError> {
        let _delivery = self.delivery_gate.lock();
        let prefix = {
            let mut state = self.state.lock();
            let CaptureSinkState::Staging { pcm } = &mut *state else {
                return match &*state {
                    CaptureSinkState::Discarded => Err(RecorderError::EngineFailed(
                        "provisional capture was discarded".into(),
                    )),
                    CaptureSinkState::Promoted(_) => Err(RecorderError::EngineFailed(
                        "provisional capture was already promoted".into(),
                    )),
                    CaptureSinkState::Staging { .. } => unreachable!(),
                };
            };
            let prefix = pcm.drain(..).collect::<Vec<_>>();
            *state = CaptureSinkState::Promoted(ForwardTarget {
                consumer,
                level_handler,
                archive,
            });
            prefix
        };
        let target = {
            let state = self.state.lock();
            match &*state {
                CaptureSinkState::Promoted(target) => Some(target.clone()),
                CaptureSinkState::Staging { .. } | CaptureSinkState::Discarded => None,
            }
        };
        if let Some(target) = target {
            if !prefix.is_empty() {
                Self::deliver(&target, &prefix);
                (target.level_handler)(pcm_rms_level(&prefix));
            }
        }
        Ok(())
    }

    fn discard(&self) {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        match &mut *state {
            CaptureSinkState::Staging { pcm } => {
                pcm.clear();
                *state = CaptureSinkState::Discarded;
            }
            CaptureSinkState::Discarded | CaptureSinkState::Promoted(_) => {}
        }
    }
}

impl AudioConsumer for CaptureSink {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let target = {
            let mut state = self.state.lock();
            match &mut *state {
                CaptureSinkState::Staging { pcm: buffered } => {
                    let even_len = pcm.len() & !1;
                    let capacity = self.max_bytes & !1;
                    let remaining = capacity.saturating_sub(buffered.len());
                    buffered.extend(pcm[..even_len.min(remaining)].iter().copied());
                    None
                }
                CaptureSinkState::Promoted(target) => Some(ForwardTarget {
                    consumer: Arc::clone(&target.consumer),
                    level_handler: Arc::clone(&target.level_handler),
                    archive: target.archive.clone(),
                }),
                CaptureSinkState::Discarded => None,
            }
        };
        if let Some(target) = target {
            let _delivery = self.delivery_gate.lock();
            Self::deliver(&target, pcm);
        }
    }
}

impl CaptureSink {
    fn publish_level(&self, level: f32) {
        let target = {
            let state = self.state.lock();
            match &*state {
                CaptureSinkState::Promoted(target) => Some(Arc::clone(&target.level_handler)),
                CaptureSinkState::Staging { .. } | CaptureSinkState::Discarded => None,
            }
        };
        if let Some(level_handler) = target {
            let _delivery = self.delivery_gate.lock();
            level_handler(level);
        }
    }
}

struct WarmCaptureSink {
    state: Mutex<WarmCaptureSinkState>,
    delivery_gate: Mutex<()>,
    activation_consumer: Mutex<Option<Arc<dyn AudioConsumer>>>,
    activation_consumer_during_capture: AtomicBool,
    activation_generation: Mutex<Option<u64>>,
    activation_fault_handler: Mutex<Option<(u64, Arc<dyn Fn(RecorderError) + Send + Sync>)>>,
    activation_vad: Mutex<ActivationVad>,
    max_bytes: usize,
}

struct ActivationVadOutput {
    pcm: Option<Vec<u8>>,
    end_of_speech: bool,
}

struct ActivationVad {
    pre_roll: VecDeque<u8>,
    speech_active: bool,
    hangover_chunks: usize,
}

impl Default for ActivationVad {
    fn default() -> Self {
        Self {
            pre_roll: VecDeque::with_capacity(ACTIVATION_VAD_PRE_ROLL_BYTES),
            speech_active: false,
            hangover_chunks: 0,
        }
    }
}

impl ActivationVad {
    fn reset(&mut self) {
        self.pre_roll.clear();
        self.speech_active = false;
        self.hangover_chunks = 0;
    }

    /// Return gated PCM and report a single speech-to-silence boundary.
    /// This deliberately uses one RMS pass and a bounded queue rather than a second model.
    fn gate(&mut self, pcm: &[u8]) -> ActivationVadOutput {
        if pcm.is_empty() {
            return ActivationVadOutput {
                pcm: None,
                end_of_speech: false,
            };
        }
        let speech = pcm_rms_level(pcm) >= ACTIVATION_VAD_RMS_THRESHOLD;
        if speech {
            let mut output = if self.speech_active {
                Vec::new()
            } else {
                self.pre_roll.iter().copied().collect()
            };
            // The current block can be a complete handoff buffer, not just a device chunk.
            output.extend_from_slice(pcm);
            self.pre_roll.clear();
            self.speech_active = true;
            self.hangover_chunks = ACTIVATION_VAD_HANGOVER_CHUNKS;
            return ActivationVadOutput {
                pcm: Some(output),
                end_of_speech: false,
            };
        }
        if self.speech_active && self.hangover_chunks > 0 {
            self.hangover_chunks -= 1;
            return ActivationVadOutput {
                pcm: Some(pcm.to_vec()),
                end_of_speech: false,
            };
        }
        WarmCaptureSink::append_bounded(&mut self.pre_roll, pcm, ACTIVATION_VAD_PRE_ROLL_BYTES);
        let end_of_speech = self.speech_active;
        self.speech_active = false;
        ActivationVadOutput {
            pcm: None,
            end_of_speech,
        }
    }
}

enum WarmCaptureSinkState {
    Ring {
        pcm: VecDeque<u8>,
        claims: HashMap<u64, WarmCaptureClaim>,
    },
}

enum WarmCaptureClaim {
    Staging {
        pcm: VecDeque<u8>,
        pre_roll_bytes: usize,
        allow_postroll_eviction: bool,
        max_bytes: usize,
        max_level: f32,
    },
    Promoted(ForwardTarget),
}

impl WarmCaptureSink {
    fn new(max_bytes: usize) -> Arc<Self> {
        let max_bytes = (max_bytes.max(2) & !1).min(STAGED_CAPTURE_MAX_BYTES.max(2));
        Arc::new(Self {
            state: Mutex::new(WarmCaptureSinkState::Ring {
                pcm: VecDeque::with_capacity(max_bytes),
                claims: HashMap::new(),
            }),
            delivery_gate: Mutex::new(()),
            activation_consumer: Mutex::new(None),
            activation_consumer_during_capture: AtomicBool::new(false),
            activation_generation: Mutex::new(None),
            activation_fault_handler: Mutex::new(None),
            activation_vad: Mutex::new(ActivationVad::default()),
            max_bytes,
        })
    }

    fn activation_consumer_during_capture(&self) -> Option<Arc<dyn AudioConsumer>> {
        self.activation_consumer_during_capture
            .load(Ordering::Acquire)
            .then(|| self.activation_consumer.lock().clone())
            .flatten()
    }

    fn set_activation_consumer(&self, consumer: Option<Arc<dyn AudioConsumer>>) {
        self.activation_consumer_during_capture.store(false, Ordering::Release);
        *self.activation_consumer.lock() = consumer;
        self.activation_vad.lock().reset();
    }

    fn set_activation_consumer_during_capture(&self, consumer: Option<Arc<dyn AudioConsumer>>) {
        self.activation_consumer_during_capture
            .store(consumer.is_some(), Ordering::Release);
        *self.activation_consumer.lock() = consumer;
        self.activation_vad.lock().reset();
    }

    fn deliver_activation(&self, consumer: &Arc<dyn AudioConsumer>, pcm: &[u8]) {
        let (gated, was_active, active, hangover) = {
            let mut vad = self.activation_vad.lock();
            let was_active = vad.speech_active;
            let gated = vad.gate(pcm);
            (gated, was_active, vad.speech_active, vad.hangover_chunks)
        };
        // [DEBUG-VOICE-1003] P1 / H1: transitions plus bounded coverage of sustained noise/silence.
        static BLOCKS: AtomicU64 = AtomicU64::new(0);
        let blocks = BLOCKS.fetch_add(1, Ordering::Relaxed);
        if was_active != active || gated.end_of_speech || blocks % 64 == 0 {
            voice_diagnostic(
                "P1",
                &["H1"],
                "vad_gate",
                "observed",
                serde_json::json!({
                    "bytes": pcm.len(), "active": active, "hangover": hangover,
                    "forwarded": gated.pcm.is_some(), "boundary": gated.end_of_speech,
                    "count": blocks,
                }),
            );
        }
        if let Some(pcm) = gated.pcm {
            consumer.consume_pcm_chunk(&pcm);
        }
        if gated.end_of_speech {
            consumer.finish_segment();
        }
    }

    fn append_bounded(buffer: &mut VecDeque<u8>, pcm: &[u8], max_bytes: usize) {
        let even_len = pcm.len() & !1;
        buffer.extend(pcm[..even_len].iter().copied());
        while buffer.len() > max_bytes {
            let _ = buffer.pop_front();
            let _ = buffer.pop_front();
        }
    }

    fn append_staged(
        buffer: &mut VecDeque<u8>,
        pre_roll_bytes: &mut usize,
        allow_postroll_eviction: bool,
        pcm: &[u8],
        max_bytes: usize,
    ) {
        let even_len = pcm.len() & !1;
        let incoming = &pcm[..even_len];
        let excess = buffer.len().saturating_add(incoming.len()).saturating_sub(max_bytes);
        let evictable_preroll = excess.min(*pre_roll_bytes);
        for _ in 0..(evictable_preroll / 2) {
            let _ = buffer.pop_front();
            let _ = buffer.pop_front();
        }
        *pre_roll_bytes -= evictable_preroll;
        let evictable_postroll = if allow_postroll_eviction {
            excess.saturating_sub(evictable_preroll)
        } else {
            0
        };
        for _ in 0..(evictable_postroll / 2) {
            let _ = buffer.pop_front();
            let _ = buffer.pop_front();
        }
        let available = max_bytes.saturating_sub(buffer.len()) & !1;
        buffer.extend(incoming[..incoming.len().min(available)].iter().copied());
    }

    fn arm(&self, press_id: u64, include_preroll: bool) -> Result<(), RecorderError> {
        self.arm_with_limit(press_id, include_preroll, self.max_bytes)
    }

    fn arm_with_limit(
        &self,
        press_id: u64,
        include_preroll: bool,
        max_bytes: usize,
    ) -> Result<(), RecorderError> {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        let WarmCaptureSinkState::Ring { pcm, claims } = &mut *state;
        if claims.contains_key(&press_id) {
            return Err(RecorderError::EngineFailed(
                "warm capture generation is already armed".into(),
            ));
        }
        let max_bytes = max_bytes.max(2) & !1;
        let staged = include_preroll.then(|| pcm.clone()).unwrap_or_default();
        claims.insert(
            press_id,
            WarmCaptureClaim::Staging {
                pre_roll_bytes: staged.len(),
                allow_postroll_eviction: !include_preroll,
                max_bytes,
                pcm: staged,
                max_level: 0.0,
            },
        );
        Ok(())
    }

    fn promote(
        &self,
        press_id: u64,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        archive: Option<Arc<WavArchiveWriter>>,
    ) -> Result<(), RecorderError> {
        let _delivery = self.delivery_gate.lock();
        let (prefix, staged_level) = {
            let mut state = self.state.lock();
            let WarmCaptureSinkState::Ring { claims, .. } = &mut *state;
            let Some(claim) = claims.remove(&press_id) else {
                return Err(RecorderError::EngineFailed(
                    "warm provisional capture was not armed".into(),
                ));
            };
            let WarmCaptureClaim::Staging { pcm, max_level, .. } = claim else {
                return Err(RecorderError::EngineFailed(
                    "warm provisional capture was already promoted".into(),
                ));
            };
            let prefix = pcm.into_iter().collect::<Vec<_>>();
            claims.insert(
                press_id,
                WarmCaptureClaim::Promoted(ForwardTarget {
                    consumer,
                    level_handler,
                    archive,
                }),
            );
            (prefix, max_level)
        };
        let target = {
            let state = self.state.lock();
            let WarmCaptureSinkState::Ring { claims, .. } = &*state;
            match claims.get(&press_id) {
                Some(WarmCaptureClaim::Promoted(target)) => Some(target.clone()),
                Some(WarmCaptureClaim::Staging { .. }) | None => None,
            }
        };
        let activation = self
            .activation_consumer_during_capture
            .load(Ordering::Acquire)
            .then(|| self.activation_consumer.lock().clone())
            .flatten();
        if let Some(target) = target {
            if !prefix.is_empty() {
                CaptureSink::deliver(&target, &prefix);
                if let Some(activation) = activation {
                    self.deliver_activation(&activation, &prefix);
                }
            }
            (target.level_handler)(staged_level);
        }
        Ok(())
    }

    fn discard(&self, press_id: u64) {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &mut *state;
        claims.remove(&press_id);
    }

    fn take_claim(&self, press_id: u64) -> Option<ForwardTarget> {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &mut *state;
        match claims.remove(&press_id) {
            Some(WarmCaptureClaim::Promoted(target)) => Some(target),
            Some(WarmCaptureClaim::Staging { .. }) | None => None,
        }
    }

    fn staging_claim_ids(&self) -> Vec<u64> {
        let _delivery = self.delivery_gate.lock();
        let state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &*state;
        claims
            .iter()
            .filter_map(|(press_id, claim)| {
                matches!(claim, WarmCaptureClaim::Staging { .. }).then_some(*press_id)
            })
            .collect()
    }

    fn has_claims(&self) -> bool {
        let _delivery = self.delivery_gate.lock();
        let state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &*state;
        !claims.is_empty()
    }

    fn has_promoted_claims(&self) -> bool {
        let _delivery = self.delivery_gate.lock();
        let state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &*state;
        claims
            .values()
            .any(|claim| matches!(claim, WarmCaptureClaim::Promoted(_)))
    }

    fn is_last_promoted_claim(&self, press_id: u64) -> bool {
        let _delivery = self.delivery_gate.lock();
        let state = self.state.lock();
        let WarmCaptureSinkState::Ring { claims, .. } = &*state;
        matches!(claims.get(&press_id), Some(WarmCaptureClaim::Promoted(_)))
            && claims
                .values()
                .filter(|claim| matches!(claim, WarmCaptureClaim::Promoted(_)))
                .count()
                == 1
    }

    fn invalidate_staging(&self) {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        let WarmCaptureSinkState::Ring { pcm, claims } = &mut *state;
        pcm.clear();
        claims.retain(|_, claim| matches!(claim, WarmCaptureClaim::Promoted(_)));
    }

    fn clear(&self) -> Vec<Arc<WavArchiveWriter>> {
        let _delivery = self.delivery_gate.lock();
        let mut state = self.state.lock();
        let WarmCaptureSinkState::Ring { pcm, claims } = &mut *state;
        pcm.clear();
        let claims = std::mem::take(claims);
        claims
            .into_values()
            .filter_map(|claim| match claim {
                WarmCaptureClaim::Staging { .. } => None,
                WarmCaptureClaim::Promoted(target) => target.archive,
            })
            .collect()
    }
}

impl AudioConsumer for WarmCaptureSink {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let _delivery = self.delivery_gate.lock();
        let (targets, has_promoted_claim, has_staging_claim) = {
            let mut state = self.state.lock();
            let WarmCaptureSinkState::Ring { pcm: ring, claims } = &mut *state;
            Self::append_bounded(ring, pcm, self.max_bytes);
            let mut targets = Vec::new();
            let mut has_promoted_claim = false;
            let mut has_staging_claim = false;
            for claim in claims.values_mut() {
                match claim {
                    WarmCaptureClaim::Staging {
                        pcm: staged,
                        pre_roll_bytes,
                        allow_postroll_eviction,
                        max_bytes,
                        ..
                    } => {
                        has_staging_claim = true;
                        Self::append_staged(
                            staged,
                            pre_roll_bytes,
                            *allow_postroll_eviction,
                            pcm,
                            *max_bytes,
                        );
                    }
                    WarmCaptureClaim::Promoted(target) => {
                        has_promoted_claim = true;
                        targets.push(target.clone());
                    }
                }
            }
            (targets, has_promoted_claim, has_staging_claim)
        };
        let activation = ((!has_promoted_claim && !has_staging_claim)
            || (has_promoted_claim
                && self
                    .activation_consumer_during_capture
                    .load(Ordering::Acquire)))
            .then(|| self.activation_consumer.lock().clone())
            .flatten();
        for target in targets {
            CaptureSink::deliver(&target, pcm);
        }
        if let Some(activation) = activation {
            self.deliver_activation(&activation, pcm);
        }
    }
}

impl WarmCaptureSink {
    fn publish_level(&self, level: f32) {
        let _delivery = self.delivery_gate.lock();
        let level_handlers = {
            let mut state = self.state.lock();
            let WarmCaptureSinkState::Ring { claims, .. } = &mut *state;
            let mut level_handlers = Vec::new();
            for claim in claims.values_mut() {
                match claim {
                    WarmCaptureClaim::Promoted(target) => {
                        level_handlers.push(Arc::clone(&target.level_handler));
                    }
                    WarmCaptureClaim::Staging { max_level, .. } => {
                        *max_level = (*max_level).max(level);
                    }
                }
            }
            level_handlers
        };
        for level_handler in level_handlers {
            level_handler(level);
        }
    }
}

/// A committed or discarded claim on the process-wide warm microphone stream.
/// The warm stream can have multiple provisional generations, but only promoted
/// claims receive recording levels and downstream PCM.
pub(crate) struct WarmStagedRecorder {
    hub: Arc<WarmCaptureHub>,
    press_id: u64,
    consumed: bool,
}

pub(crate) struct WarmActiveCapture {
    hub: Arc<WarmCaptureHub>,
    press_id: u64,
}

impl WarmStagedRecorder {
    pub(crate) fn capture_hub(&self) -> Arc<WarmCaptureHub> {
        Arc::clone(&self.hub)
    }

    pub(crate) fn fallback_device_name(&self) -> Option<String> {
        self.hub.device_name.lock().clone()
    }

    pub(crate) fn commit(
        mut self,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        audio_archive_path: Option<PathBuf>,
    ) -> Result<(WarmActiveCapture, Receiver<RecorderError>, bool), RecorderError> {
        let archive_path = audio_archive_path;
        let archive_writer = archive_path
            .as_ref()
            .and_then(|path| match WavArchiveWriter::create(path) {
                Ok(writer) => Some(Arc::new(writer)),
                Err(error) => {
                    log::warn!("[recorder] wav archive create failed at {path:?}: {error}");
                    None
                }
            });
        let archive_active = archive_writer.is_some();
        let result = self.hub.promote(
            self.press_id,
            consumer,
            level_handler,
            archive_writer.clone(),
        );
        let runtime_errors = match result {
            Ok(runtime_errors) => runtime_errors,
            Err(error) => {
                if let Some(archive) = archive_writer {
                    archive.finish();
                }
                if let Some(path) = archive_path {
                    let _ = std::fs::remove_file(path);
                }
                self.consumed = true;
                return Err(error);
            }
        };
        self.consumed = true;
        Ok((
            WarmActiveCapture {
                hub: Arc::clone(&self.hub),
                press_id: self.press_id,
            },
            runtime_errors,
            archive_active,
        ))
    }

    pub(crate) fn discard(mut self) -> Result<(), RecorderError> {
        self.hub.release_claim(self.press_id);
        self.consumed = true;
        Ok(())
    }
}

impl Drop for WarmStagedRecorder {
    fn drop(&mut self) {
        if !self.consumed {
            self.hub.release_claim(self.press_id);
        }
    }
}

impl WarmActiveCapture {
    pub(crate) fn stop(self) {
        self.hub.release_claim(self.press_id);
    }
}

/// Owns the optional idle microphone stream. The stream and its ring are never
/// connected to an ASR consumer or archive until a matching press is promoted.
pub(crate) struct WarmCaptureHub {
    sink: Arc<WarmCaptureSink>,
    recorder: Mutex<Option<Recorder>>,
    runtime_errors: Mutex<HashMap<u64, Sender<RecorderError>>>,
    runtime_fallback_claims: Mutex<HashSet<u64>>,
    enabled: AtomicBool,
    preview_active: AtomicBool,
    cold_capture_active: AtomicBool,
    pending_configuration:
        Mutex<Option<(bool, Option<String>, openless_core::AudioProcessingConfig)>>,
    processing_config: Mutex<openless_core::AudioProcessingConfig>,
    stream_generation: AtomicU64,
    lifecycle_gate: Mutex<()>,
    device_name: Mutex<Option<String>>,
}

impl WarmCaptureHub {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sink: WarmCaptureSink::new(STAGED_CAPTURE_MAX_BYTES),
            recorder: Mutex::new(None),
            runtime_errors: Mutex::new(HashMap::new()),
            runtime_fallback_claims: Mutex::new(HashSet::new()),
            enabled: AtomicBool::new(false),
            preview_active: AtomicBool::new(false),
            cold_capture_active: AtomicBool::new(false),
            pending_configuration: Mutex::new(None),
            processing_config: Mutex::new(openless_core::AudioProcessingConfig::default()),
            stream_generation: AtomicU64::new(0),
            lifecycle_gate: Mutex::new(()),
            device_name: Mutex::new(None),
        })
    }

    fn stop_recorder(&self) {
        if let Some(recorder) = self.recorder.lock().take() {
            recorder.stop();
        }
    }

    fn notify_runtime_error(&self, error: RecorderError) {
        let senders = std::mem::take(&mut *self.runtime_errors.lock());
        for sender in senders.into_values() {
            let _ = sender.send(error.clone());
        }
    }

    pub(crate) fn activation_consumer_for_capture(&self) -> Option<Arc<dyn AudioConsumer>> {
        self.sink.activation_consumer_during_capture()
    }

    pub(crate) fn set_activation_consumer(&self, consumer: Option<Arc<dyn AudioConsumer>>) {
        self.sink.set_activation_consumer(consumer);
    }

    pub(crate) fn set_activation_consumer_during_capture(
        &self,
        consumer: Option<Arc<dyn AudioConsumer>>,
    ) {
        self.sink.set_activation_consumer_during_capture(consumer);
    }

    pub(crate) fn set_activation_consumer_for_generation(
        &self,
        generation: u64,
        consumer: Option<Arc<dyn AudioConsumer>>,
        during_capture: bool,
    ) -> bool {
        let active_generation = self.sink.activation_generation.lock();
        if *active_generation != Some(generation) {
            return false;
        }
        if during_capture {
            self.sink.set_activation_consumer_during_capture(consumer);
        } else {
            self.sink.set_activation_consumer(consumer);
        }
        true
    }

    pub(crate) fn set_activation_generation(&self, generation: u64) {
        *self.sink.activation_generation.lock() = Some(generation);
    }

    pub(crate) fn clear_activation_generation_if(&self, generation: u64) -> bool {
        let mut active_generation = self.sink.activation_generation.lock();
        if *active_generation == Some(generation) {
            *active_generation = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn activation_is_active(&self) -> bool {
        self.sink.activation_generation.lock().is_some()
    }

    pub(crate) fn is_cold_capture_active(&self) -> bool {
        self.cold_capture_active.load(Ordering::Acquire)
    }

    pub(crate) fn set_activation_fault_handler(
        &self,
        generation: u64,
        handler: Option<Arc<dyn Fn(RecorderError) + Send + Sync>>,
    ) {
        *self.sink.activation_fault_handler.lock() = handler.map(|handler| (generation, handler));
    }

    pub(crate) fn clear_activation_fault_handler_if(&self, generation: u64) {
        let mut handler = self.sink.activation_fault_handler.lock();
        if handler.as_ref().is_some_and(|(current, _)| *current == generation) {
            *handler = None;
        }
    }

    pub(crate) fn configure(
        &self,
        enabled: bool,
        device_name: Option<String>,
        processing: openless_core::AudioProcessingConfig,
    ) {
        let _lifecycle = self.lifecycle_gate.lock();
        self.configure_locked(enabled, device_name, processing);
    }

    fn configure_locked(
        &self,
        enabled: bool,
        device_name: Option<String>,
        processing: openless_core::AudioProcessingConfig,
    ) {
        let device_changed = *self.device_name.lock() != device_name;
        let config_changed = *self.processing_config.lock() != processing;
        if self.cold_capture_active.load(Ordering::Acquire) {
            *self.pending_configuration.lock() = Some((enabled, device_name, processing));
            return;
        }
        if (device_changed || config_changed || !enabled) && self.sink.has_claims() {
            let staging_claims = self.sink.staging_claim_ids();
            self.runtime_fallback_claims.lock().extend(staging_claims);
            *self.pending_configuration.lock() = Some((enabled, device_name, processing));
            self.sink.invalidate_staging();
            return;
        }
        self.pending_configuration.lock().take();
        *self.processing_config.lock() = processing;
        if !enabled {
            self.enabled.store(false, Ordering::Release);
            self.stream_generation.fetch_add(1, Ordering::AcqRel);
            let staging_claims = self.sink.staging_claim_ids();
            self.stop_recorder();
            let archives = self.sink.clear();
            self.runtime_fallback_claims.lock().extend(staging_claims);
            self.notify_runtime_error(RecorderError::EngineFailed(
                "warm capture configuration changed".into(),
            ));
            *self.device_name.lock() = device_name;
            for archive in archives {
                archive.finish();
            }
            return;
        }

        self.enabled.store(true, Ordering::Release);
        if device_changed || config_changed {
            self.stream_generation.fetch_add(1, Ordering::AcqRel);
            let staging_claims = self.sink.staging_claim_ids();
            self.stop_recorder();
            let archives = self.sink.clear();
            self.runtime_fallback_claims.lock().extend(staging_claims);
            self.notify_runtime_error(RecorderError::EngineFailed(
                "warm capture configuration changed".into(),
            ));
            *self.device_name.lock() = device_name.clone();
            for archive in archives {
                archive.finish();
            }
            if !self.preview_active.load(Ordering::Acquire) {
                let _ = self.ensure_started_locked(device_name);
            }
            return;
        }
        if self.preview_active.load(Ordering::Acquire) {
            return;
        }
        let _ = self.ensure_started_locked(device_name);
    }

    fn ensure_started(&self, device_name: Option<String>) -> Result<(), RecorderError> {
        let _lifecycle = self.lifecycle_gate.lock();
        self.ensure_started_locked(device_name)
    }

    fn ensure_started_locked(&self, device_name: Option<String>) -> Result<(), RecorderError> {
        if !self.enabled.load(Ordering::Acquire) {
            return Err(RecorderError::EngineFailed(
                "warm capture is disabled".into(),
            ));
        }
        if self.preview_active.load(Ordering::Acquire) {
            return Err(RecorderError::EngineFailed(
                "warm capture is paused for microphone preview".into(),
            ));
        }
        if self.recorder.lock().is_some() {
            return Ok(());
        }
        let consumer: Arc<dyn AudioConsumer> = Arc::clone(&self.sink) as Arc<dyn AudioConsumer>;
        let sink_for_level = Arc::clone(&self.sink);
        let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
            sink_for_level.publish_level(level);
        });
        let (recorder, runtime_errors, _) = Recorder::start_with_processing(
            device_name,
            consumer,
            level_handler,
            None,
            *self.processing_config.lock(),
        )?;
        let generation = self.stream_generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.recorder.lock() = Some(recorder);
        let hub = warm_capture_hub();
        let _ = thread::Builder::new()
            .name("openless-warm-capture-watchdog".into())
            .spawn(move || {
                if let Ok(error) = runtime_errors.recv() {
                    log::warn!("[recorder] warm microphone stream stopped: {error}");
                    hub.stop_after_runtime_error(generation, error);
                }
            });
        Ok(())
    }

    fn stop_after_runtime_error(&self, generation: u64, error: RecorderError) {
        let _lifecycle = self.lifecycle_gate.lock();
        if self.stream_generation.load(Ordering::Acquire) != generation {
            return;
        }
        self.stream_generation.fetch_add(1, Ordering::AcqRel);
        self.stop_recorder();
        let staging_claims = self.sink.staging_claim_ids();
        self.runtime_fallback_claims.lock().extend(staging_claims);
        let archives = self.sink.clear();
        self.notify_runtime_error(error.clone());
        let active_activation_generation = *self.sink.activation_generation.lock();
        let activation_handler = {
            let mut handler = self.sink.activation_fault_handler.lock();
            match handler.take() {
                Some((handler_generation, callback))
                    if active_activation_generation == Some(handler_generation) =>
                {
                    Some(callback)
                }
                Some(other) => {
                    *handler = Some(other);
                    None
                }
                None => None,
            }
        };
        for archive in archives {
            archive.finish();
        }
        if let Some(handler) = activation_handler {
            handler(error);
        }
    }

    fn promote(
        &self,
        press_id: u64,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        archive: Option<Arc<WavArchiveWriter>>,
    ) -> Result<Receiver<RecorderError>, RecorderError> {
        let _lifecycle = self.lifecycle_gate.lock();
        let (runtime_error_tx, runtime_error_rx) = channel();
        if let Err(error) = self
            .sink
            .promote(press_id, consumer, level_handler, archive)
        {
            let fallback_allowed = self.runtime_fallback_claims.lock().remove(&press_id);
            if !fallback_allowed
                && matches!(
                    &error,
                    RecorderError::EngineFailed(message)
                        if message == "warm provisional capture was not armed"
                )
            {
                return Err(RecorderError::EngineFailed(
                    "warm provisional capture was invalidated".into(),
                ));
            }
            return Err(error);
        }
        self.runtime_fallback_claims.lock().remove(&press_id);
        self.runtime_errors
            .lock()
            .insert(press_id, runtime_error_tx);
        Ok(runtime_error_rx)
    }

    pub(crate) fn arm(
        self: &Arc<Self>,
        press_id: u64,
        device_name: Option<String>,
        processing: openless_core::AudioProcessingConfig,
    ) -> Result<WarmStagedRecorder, RecorderError> {
        self.arm_internal(
            press_id,
            device_name,
            true,
            STAGED_CAPTURE_MAX_BYTES,
            Some(processing),
        )
    }

    pub(crate) fn arm_with_preroll(
        self: &Arc<Self>,
        press_id: u64,
        device_name: Option<String>,
    ) -> Result<WarmStagedRecorder, RecorderError> {
        self.arm_internal(
            voice_capture_key(press_id),
            device_name,
            true,
            VOICE_STAGED_CAPTURE_MAX_BYTES,
            None,
        )
    }

    pub(crate) fn arm_without_preroll(
        self: &Arc<Self>,
        press_id: u64,
        device_name: Option<String>,
    ) -> Result<WarmStagedRecorder, RecorderError> {
        self.arm_internal(
            voice_capture_key(press_id),
            device_name,
            false,
            VOICE_STAGED_CAPTURE_MAX_BYTES,
            None,
        )
    }

    fn arm_internal(
        self: &Arc<Self>,
        press_id: u64,
        device_name: Option<String>,
        include_preroll: bool,
        staged_max_bytes: usize,
        processing: Option<openless_core::AudioProcessingConfig>,
    ) -> Result<WarmStagedRecorder, RecorderError> {
        if !self.enabled.load(Ordering::Acquire) {
            return Err(RecorderError::EngineFailed(
                "warm capture is disabled".into(),
            ));
        }
        if self.cold_capture_active.load(Ordering::Acquire) {
            return Err(RecorderError::EngineFailed(
                "warm capture is unavailable during cold capture".into(),
            ));
        }
        loop {
            let lifecycle = self.lifecycle_gate.lock();
            let desired_processing = processing.unwrap_or(*self.processing_config.lock());
            if *self.device_name.lock() != device_name
                || *self.processing_config.lock() != desired_processing
            {
                if self.sink.has_promoted_claims() {
                    return Err(RecorderError::EngineFailed(
                        "warm capture device change is deferred while capture is active".into(),
                    ));
                }
                drop(lifecycle);
                self.configure(true, device_name.clone(), desired_processing);
                continue;
            }
            self.sink
                .arm_with_limit(press_id, include_preroll, staged_max_bytes)?;
            if let Err(error) = self.ensure_started_locked(device_name.clone()) {
                self.sink.discard(press_id);
                return Err(error);
            }
            drop(lifecycle);
            return Ok(WarmStagedRecorder {
                hub: Arc::clone(self),
                press_id,
                consumed: false,
            });
        }
    }

    fn release_claim(&self, press_id: u64) {
        let _lifecycle = self.lifecycle_gate.lock();
        self.runtime_fallback_claims.lock().remove(&press_id);
        if self.sink.is_last_promoted_claim(press_id) {
            if let Some(recorder) = self.recorder.lock().as_ref() {
                recorder.drain_pending_audio();
            }
        }
        let target = self.sink.take_claim(press_id);
        self.runtime_errors.lock().remove(&press_id);
        if let Some(target) = target {
            if let Some(archive) = target.archive {
                archive.finish();
            }
        }
        if !self.sink.has_promoted_claims() {
            let pending_configuration = self.pending_configuration.lock().take();
            if let Some((enabled, device_name, processing)) = pending_configuration {
                self.configure_locked(enabled, device_name, processing);
            }
        }
    }

    pub(crate) fn begin_cold_capture(&self) -> Result<(), RecorderError> {
        let _lifecycle = self.lifecycle_gate.lock();
        self.begin_cold_capture_locked()
    }

    /// Atomically reserve the microphone for a cold recorder and suspend the warm stream.
    ///
    /// The reservation is acquired before the stream is stopped while holding the same lifecycle
    /// gate, so a concurrent warm arm cannot slip into the interval between admission and
    /// suspension.
    pub(crate) fn begin_cold_capture_and_suspend(&self) -> Result<(), RecorderError> {
        let _lifecycle = self.lifecycle_gate.lock();
        self.begin_cold_capture_locked()?;
        self.suspend_for_cold_capture_locked();
        Ok(())
    }

    fn begin_cold_capture_locked(&self) -> Result<(), RecorderError> {
        if self.cold_capture_active.swap(true, Ordering::AcqRel) {
            return Err(RecorderError::EngineFailed(
                "another microphone capture is already active".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn end_cold_capture(&self) {
        let _lifecycle = self.lifecycle_gate.lock();
        if !self.cold_capture_active.swap(false, Ordering::AcqRel) {
            return;
        }
        let pending_configuration = self.pending_configuration.lock().take();
        if let Some((enabled, device_name, processing)) = pending_configuration {
            self.configure_locked(enabled, device_name, processing);
        }
    }

    /// Temporarily stop the warm stream while the settings preview owns the microphone.
    pub(crate) fn begin_preview(&self) -> bool {
        let _lifecycle = self.lifecycle_gate.lock();
        if self.cold_capture_active.load(Ordering::Acquire) || self.sink.has_claims() {
            return false;
        }
        self.preview_active.store(true, Ordering::Release);
        self.stream_generation.fetch_add(1, Ordering::AcqRel);
        self.stop_recorder();
        let archives = self.sink.clear();
        self.runtime_fallback_claims.lock().clear();
        self.notify_runtime_error(RecorderError::EngineFailed(
            "warm capture paused for microphone preview".into(),
        ));
        for archive in archives {
            archive.finish();
        }
        true
    }

    /// Resume the configured warm stream after the settings preview releases the microphone.
    pub(crate) fn end_preview(&self) {
        let _lifecycle = self.lifecycle_gate.lock();
        self.preview_active.store(false, Ordering::Release);
        if !self.enabled.load(Ordering::Acquire) || self.cold_capture_active.load(Ordering::Acquire)
        {
            return;
        }
        let device_name = self.device_name.lock().clone();
        if self.recorder.lock().is_none() {
            let _ = self.ensure_started_locked(device_name);
        }
    }

    pub(crate) fn stop(&self) {
        let _lifecycle = self.lifecycle_gate.lock();
        self.pending_configuration.lock().take();
        self.enabled.store(false, Ordering::Release);
        self.stream_generation.fetch_add(1, Ordering::AcqRel);
        self.stop_recorder();
        let archives = self.sink.clear();
        self.runtime_fallback_claims.lock().clear();
        self.notify_runtime_error(RecorderError::EngineFailed("warm capture stopped".into()));
        *self.device_name.lock() = None;
        for archive in archives {
            archive.finish();
        }
    }

    /// Stop the warm stream while a legacy cold recorder owns the microphone, but keep the
    /// preference enabled so the stream can be rebuilt when that recorder stops.
    pub(crate) fn suspend_for_cold_capture(&self) {
        let _lifecycle = self.lifecycle_gate.lock();
        self.suspend_for_cold_capture_locked();
    }

    fn suspend_for_cold_capture_locked(&self) {
        self.stream_generation.fetch_add(1, Ordering::AcqRel);
        self.stop_recorder();
        let archives = self.sink.clear();
        self.runtime_fallback_claims.lock().clear();
        self.notify_runtime_error(RecorderError::EngineFailed(
            "warm capture suspended for cold capture".into(),
        ));
        for archive in archives {
            archive.finish();
        }
    }

    /// Rebuild the configured warm stream after a legacy cold recorder releases the microphone.
    pub(crate) fn resume_after_cold_capture(&self) {
        let _lifecycle = self.lifecycle_gate.lock();
        if !self.enabled.load(Ordering::Acquire)
            || self.preview_active.load(Ordering::Acquire)
            || self.recorder.lock().is_some()
        {
            return;
        }
        let device_name = self.device_name.lock().clone();
        let _ = self.ensure_started_locked(device_name);
    }
}

static WARM_CAPTURE_HUB: OnceLock<Arc<WarmCaptureHub>> = OnceLock::new();

pub(crate) fn warm_capture_hub() -> Arc<WarmCaptureHub> {
    Arc::clone(WARM_CAPTURE_HUB.get_or_init(WarmCaptureHub::new))
}

fn run_wav_archive_writer(mut archiver: WavArchiver, receiver: Receiver<WavArchiveMessage>) {
    while let Ok(message) = receiver.recv() {
        match message {
            WavArchiveMessage::Pcm(pcm_bytes) => archiver.append(&pcm_bytes),
            WavArchiveMessage::Finish => break,
        }
    }
}

/// Capture handle. Dropping does not stop it — call `stop` explicitly.
pub struct Recorder {
    stop_flag: Arc<AtomicBool>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
    archive_writer: Option<Arc<WavArchiveWriter>>,
    state: Arc<StreamState>,
}

/// A native stream whose callback output remains memory-only until promotion.
pub(crate) struct StagedRecorder {
    recorder: Option<Recorder>,
    runtime_errors: Option<Receiver<RecorderError>>,
    sink: Arc<CaptureSink>,
}

impl Recorder {
    /// Start capture. `consumer` receives 16 kHz/Mono/Int16-LE PCM;
    /// `level_handler` receives RMS levels in 0..1.
    /// When `audio_archive_path` is not None, the same 16 kHz/Mono/Int16-LE stream is also
    /// written to a WAV file for debugging mic sensitivity / ASR misrecognition. A dedicated
    /// writer thread persists it and backfills the RIFF / data lengths on Drop.
    ///
    /// The third return value `bool` = "archive actually created successfully": callers should
    /// use it, not the prefs switch, to set `has_audio_recording` in history. If the switch is
    /// on but writing failed (missing path / no permission / disk full), it still returns false
    /// so the frontend does not render a play button that would 404.
    ///
    /// The actual cpal Stream is constructed, played, and finally destroyed on a separate
    /// thread because it is `!Send`.
    pub fn start(
        microphone_device_name: Option<String>,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        audio_archive_path: Option<PathBuf>,
    ) -> Result<(Self, Receiver<RecorderError>, bool), RecorderError> {
        Self::start_with_processing(
            microphone_device_name,
            consumer,
            level_handler,
            audio_archive_path,
            openless_core::AudioProcessingConfig::default(),
        )
    }

    pub fn start_with_processing(
        microphone_device_name: Option<String>,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        audio_archive_path: Option<PathBuf>,
        processing: openless_core::AudioProcessingConfig,
    ) -> Result<(Self, Receiver<RecorderError>, bool), RecorderError> {
        // Startup signal: the child thread reports the result via startup_tx once the Stream
        // is constructed.
        let (startup_tx, startup_rx) = channel::<Result<(), RecorderError>>();
        // Runtime errors: once the Stream has started successfully, cpal reports them
        // asynchronously via err_cb.
        let (runtime_error_tx, runtime_error_rx) = channel::<RecorderError>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop_flag);
        let state = Arc::new(StreamState::new());
        let state_for_thread = Arc::clone(&state);

        // Try creating WavArchiveWriter on the synchronous path — success/failure is known
        // immediately and passed to the caller to decide whether history marks
        // has_audio_recording. Failure only logs a warning, no error; the main path continues.
        let archive_writer =
            audio_archive_path.and_then(|path| match WavArchiveWriter::create(&path) {
                Ok(writer) => Some(Arc::new(writer)),
                Err(err) => {
                    log::warn!("[recorder] wav archive create failed at {path:?}: {err}");
                    None
                }
            });
        let archive_active = archive_writer.is_some();
        let archive_for_thread = archive_writer.clone();

        let join_handle = match thread::Builder::new()
            .name("openless-recorder".into())
            .spawn(move || {
                run_audio_thread(
                    microphone_device_name,
                    consumer,
                    level_handler,
                    archive_for_thread,
                    processing,
                    state_for_thread,
                    stop_for_thread,
                    startup_tx,
                    runtime_error_tx,
                );
            }) {
            Ok(handle) => handle,
            Err(error) => {
                if let Some(archive) = archive_writer.as_ref() {
                    archive.finish();
                }
                return Err(RecorderError::EngineFailed(format!(
                    "spawn audio thread: {error}"
                )));
            }
        };

        // Wait for the child thread to report startup. It either sends Ok and keeps running,
        // or sends Err and exits immediately — recv unblocks in both cases.
        let startup_result = match startup_rx.recv() {
            Ok(result) => result,
            Err(error) => {
                if let Some(archive) = archive_writer.as_ref() {
                    archive.finish();
                }
                return Err(RecorderError::EngineFailed(format!(
                    "audio thread vanished: {error}"
                )));
            }
        };
        if let Err(error) = startup_result {
            let _ = join_handle.join();
            if let Some(archive) = archive_writer.as_ref() {
                archive.finish();
            }
            return Err(error);
        }

        Ok((
            Self {
                stop_flag,
                join_handle: Mutex::new(Some(join_handle)),
                archive_writer,
                state,
            },
            runtime_error_rx,
            archive_active,
        ))
    }

    pub(crate) fn drain_pending_audio(&self) {
        let sender = self.state.processor_sender.lock().clone();
        let Some(sender) = sender else {
            return;
        };
        let (done_tx, done_rx) = channel();
        if sender.send(AudioProcessingMessage::Flush(done_tx)).is_ok() {
            let _ = done_rx.recv();
        }
    }

    /// Stop capture and wait for the audio thread to exit.
    ///
    /// Takes `self` (consuming), matching the Swift API semantics — a one-shot resource.
    pub fn stop(self) {
        let Recorder {
            stop_flag,
            join_handle,
            archive_writer,
            state,
        } = self;
        *state.processor_sender.lock() = None;
        stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = join_handle.lock().take() {
            if let Err(err) = handle.join() {
                log::warn!("recorder 线程 join 失败: {:?}", err);
            }
        }
        if let Some(archive) = archive_writer {
            archive.finish();
        }
    }
}

impl Recorder {
    pub(crate) fn start_staged(
        microphone_device_name: Option<String>,
        max_buffer_bytes: usize,
    ) -> Result<StagedRecorder, RecorderError> {
        Self::start_staged_with_processing(
            microphone_device_name,
            max_buffer_bytes,
            openless_core::AudioProcessingConfig::default(),
        )
    }

    pub(crate) fn start_staged_with_processing(
        microphone_device_name: Option<String>,
        max_buffer_bytes: usize,
        processing: openless_core::AudioProcessingConfig,
    ) -> Result<StagedRecorder, RecorderError> {
        let sink = CaptureSink::new(max_buffer_bytes);
        let consumer: Arc<dyn AudioConsumer> = Arc::clone(&sink) as Arc<dyn AudioConsumer>;
        let sink_for_level = Arc::clone(&sink);
        let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
            sink_for_level.publish_level(level);
        });
        let (recorder, runtime_errors, _) = Self::start_with_processing(
            microphone_device_name,
            consumer,
            level_handler,
            None,
            processing,
        )?;
        Ok(StagedRecorder {
            recorder: Some(recorder),
            runtime_errors: Some(runtime_errors),
            sink,
        })
    }
}

impl StagedRecorder {
    pub(crate) fn capture_id(&self) -> u64 {
        self.recorder
            .as_ref()
            .map(|recorder| recorder.state.capture_id)
            .unwrap_or(0)
    }

    pub(crate) fn commit(
        mut self,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        audio_archive_path: Option<PathBuf>,
    ) -> Result<(Recorder, Receiver<RecorderError>, bool), RecorderError> {
        let archive_path = audio_archive_path;
        let archive_writer = archive_path
            .as_ref()
            .and_then(|path| match WavArchiveWriter::create(path) {
                Ok(writer) => Some(Arc::new(writer)),
                Err(error) => {
                    log::warn!("[recorder] wav archive create failed at {path:?}: {error}");
                    None
                }
            });
        let archive_active = archive_writer.is_some();
        if let Err(error) = self
            .sink
            .promote(consumer, level_handler, archive_writer.clone())
        {
            if let Some(archive) = archive_writer {
                archive.finish();
            }
            if let Some(path) = archive_path {
                let _ = std::fs::remove_file(path);
            }
            if let Some(recorder) = self.recorder.take() {
                recorder.stop();
            }
            return Err(error);
        }
        let mut recorder = self.recorder.take().ok_or_else(|| {
            RecorderError::EngineFailed("provisional recorder was already stopped".into())
        })?;
        recorder.archive_writer = archive_writer;
        let runtime_errors = self.runtime_errors.take().ok_or_else(|| {
            RecorderError::EngineFailed(
                "provisional recorder runtime channel was unavailable".into(),
            )
        })?;
        Ok((recorder, runtime_errors, archive_active))
    }

    pub(crate) fn discard(mut self) -> Result<(), RecorderError> {
        self.sink.discard();
        if let Some(recorder) = self.recorder.take() {
            recorder.stop();
        }
        Ok(())
    }
}

impl Drop for StagedRecorder {
    fn drop(&mut self) {
        self.sink.discard();
        if let Some(recorder) = self.recorder.take() {
            recorder.stop();
        }
    }
}

/// Enumerate input devices, marking the system default device.
pub fn list_input_devices() -> Result<Vec<MicrophoneDevice>, RecorderError> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let devices = host
        .input_devices()
        .map_err(|e| RecorderError::EngineFailed(format!("input_devices: {e}")))?;

    let mut result = Vec::new();
    for device in devices {
        let name = match device.name() {
            Ok(name) => name,
            Err(err) => {
                log::warn!("[recorder] failed to read input device name: {err}");
                continue;
            }
        };
        result.push(MicrophoneDevice {
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
        });
    }
    Ok(result)
}

/// Audio thread body: build Stream -> report via startup_tx -> loop until stop_flag.
/// `archiver` was already attempted by the caller on the synchronous path (Ok -> Some /
/// failed -> None); this only threads it through to build_input_stream for the cpal callback.
fn run_audio_thread(
    microphone_device_name: Option<String>,
    consumer: Arc<dyn AudioConsumer>,
    level_handler: Arc<dyn Fn(f32) + Send + Sync>,
    archiver: Option<Arc<WavArchiveWriter>>,
    processing: openless_core::AudioProcessingConfig,
    state: Arc<StreamState>,
    stop_flag: Arc<AtomicBool>,
    startup_tx: Sender<Result<(), RecorderError>>,
    runtime_error_tx: Sender<RecorderError>,
) {
    let (processor, frame_tx) = match AudioProcessingWorker::start(
        processing,
        consumer,
        level_handler,
        archiver,
        Arc::clone(&state),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = startup_tx.send(Err(error));
            return;
        }
    };
    *state.processor_sender.lock() = Some(frame_tx.clone());
    let (stream, state) = match build_input_stream(
        microphone_device_name,
        frame_tx.clone(),
        Arc::clone(&state),
        Arc::clone(&stop_flag),
        runtime_error_tx.clone(),
    ) {
        Ok(s) => s,
        Err(err) => {
            *state.processor_sender.lock() = None;
            drop(frame_tx);
            processor.finish();
            // Startup failed: notify the main thread and exit.
            let _ = startup_tx.send(Err(err));
            return;
        }
    };

    // build_input_stream returns only after native playback starts successfully.
    let _ = startup_tx.send(Ok(()));

    // Start the liveness watchdog: detect the capture callback silently stopping.
    const WATCHDOG_CHECK_INTERVAL_MS: u64 = 1000; // check once per second
    /// Each check interval is slept in slices of this size, re-reading stop_flag after each.
    ///
    /// Sleeping the full 1000ms would stall `Recorder::stop()`: it joins the audio thread,
    /// which joins this watchdog first, so the deeper the sleep the slower capture stops
    /// (measured at 0.8~1s, and `cancel_session` tears down the recorder before collecting the
    /// capsule, so the user sees "cancelled but the capsule lingers for a second"). Slicing
    /// reduces the worst-case stop wait from 1000ms to 50ms.
    ///
    /// The checks (the SECS constants below) use real elapsed time, not wake counts, so the
    /// sleep granularity does not change watchdog sensitivity: an error is still raised only
    /// after the callback is silent for CALLBACK_TIMEOUT_SECS. The cost is a few extra wakes
    /// during recording (each just reads a timestamp).
    const WATCHDOG_SLEEP_SLICE_MS: u64 = 50;
    const CALLBACK_TIMEOUT_SECS: u64 = 3; // no callback for 3s counts as abnormal
    const FIRST_CALLBACK_DEADLINE_SECS: u64 = 5; // first callback must arrive within 5s

    let stop_flag_for_watchdog = Arc::clone(&stop_flag);
    let state_for_watchdog = Arc::clone(&state);
    let runtime_error_tx_for_watchdog = runtime_error_tx.clone();

    let watchdog_handle = thread::Builder::new()
        .name("openless-recorder-watchdog".into())
        .spawn(move || {
            // Record the watchdog start time so the first-callback deadline counts from when
            // playback actually starts.
            let watchdog_start_time = std::time::Instant::now();

            while !stop_flag_for_watchdog.load(Ordering::SeqCst) {
                // Sleep one check interval in WATCHDOG_SLEEP_SLICE_MS slices, exiting early on
                // stop (see WATCHDOG_SLEEP_SLICE_MS: stopping joins this thread, so a deep
                // sleep would slow it down).
                let mut slept_ms = 0;
                while slept_ms < WATCHDOG_CHECK_INTERVAL_MS {
                    if stop_flag_for_watchdog.load(Ordering::SeqCst) {
                        return;
                    }
                    let slice = WATCHDOG_SLEEP_SLICE_MS.min(WATCHDOG_CHECK_INTERVAL_MS - slept_ms);
                    thread::sleep(std::time::Duration::from_millis(slice));
                    slept_ms += slice;
                }

                // Critical: after sleeping, re-check stop_flag before looking at elapsed.
                //
                // Otherwise this races with the hotkey-release stop path:
                //   1. User releases the hotkey -> end_session calls rec.stop() -> sets
                //      stop_flag -> audio thread pauses the cpal Stream -> callbacks really go
                //      silent
                //   2. But the watchdog is stuck inside the 1s sleep above
                //   3. When the sleep ends, without re-checking stop_flag it would read a
                //      last_callback_time "4s stale" and misreport the recording we stopped
                //      ourselves as EngineFailed("callback silent for N seconds"), causing the
                //      coordinator to kill the session and surface an error on the capsule.
                //
                // The fix is to load stop_flag once more right after sleep: during stop the
                // watchdog exits silently, while real faults (e.g. CoreAudio device
                // disconnection) during active recording are still caught.
                if stop_flag_for_watchdog.load(Ordering::SeqCst) {
                    break;
                }

                let last_callback = *state_for_watchdog.last_callback_time.lock();
                match last_callback {
                    Some(last_time) => {
                        // First callback received; check for it stopping
                        let elapsed = last_time.elapsed();
                        if elapsed.as_secs() > CALLBACK_TIMEOUT_SECS {
                            log::error!(
                                "[recorder] watchdog: 录音回调已停止 {} 秒，触发错误恢复",
                                elapsed.as_secs()
                            );
                            let _ =
                                runtime_error_tx_for_watchdog.send(RecorderError::EngineFailed(
                                    format!("录音回调静默停止 {} 秒", elapsed.as_secs()),
                                ));
                            break; // report only once
                        }
                    }
                    None => {
                        // First callback not yet received; check the deadline
                        let elapsed = watchdog_start_time.elapsed();
                        if elapsed.as_secs() > FIRST_CALLBACK_DEADLINE_SECS {
                            log::error!(
                                "[recorder] watchdog: {} 秒内未收到首次回调，触发错误恢复",
                                elapsed.as_secs()
                            );
                            let _ =
                                runtime_error_tx_for_watchdog.send(RecorderError::EngineFailed(
                                    format!("录音启动后 {} 秒内未收到回调", elapsed.as_secs()),
                                ));
                            break; // report only once
                        }
                    }
                }
            }
        })
        .ok();

    // Spin until the stop signal — cpal has no wait API, a 50ms sleep suffices.
    while !stop_flag.load(Ordering::SeqCst) {
        thread::sleep(std::time::Duration::from_millis(50));
    }

    // Stop accepting callback frames before pausing the native stream. This prevents a
    // callback racing with shutdown from adding work after the worker drain begins.
    state.accepting_audio.store(false, Ordering::Release);
    // Pause explicitly before drop.
    // On macOS coreaudio, cpal 0.15's plain drop(Stream) does not synchronously call
    // AudioOutputUnitStop, so the AudioUnit render callback keeps firing and process_callback
    // still logs cb# lines at ~5ms/frame, making macOS think the mic is still in use (the
    // orange dot never turns off). pause() goes through StreamTrait::pause — on the coreaudio
    // backend it calls AudioOutputUnitStop directly, terminating the callback synchronously.
    // The subsequent drop handles dispose / resource release. If pause fails, warn only and
    // do not block the drop.
    if let Err(err) = stream.pause() {
        log::warn!("[recorder] cpal Stream pause before drop failed: {err}");
    }
    drop(stream);
    log::info!("[recorder] cpal Stream dropped (mic released)");
    *state.processor_sender.lock() = None;
    drop(frame_tx);
    processor.finish();
    log::info!("[recorder] audio processor stopped (accepted frames drained)");

    // Wait for the watchdog thread to exit
    if let Some(handle) = watchdog_handle {
        let _ = handle.join();
    }
}

/// Try the selected microphone's native format before other advertised formats/devices.
fn build_input_stream(
    microphone_device_name: Option<String>,
    frame_tx: AudioProcessingSender,
    state: Arc<StreamState>,
    stop_flag: Arc<AtomicBool>,
    runtime_error_tx: Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let selecting_at = std::time::Instant::now();
    let host = cpal::default_host();
    let selected = select_input_device(&host, microphone_device_name.as_deref())?;
    state.log_startup_phase("device_selected", selecting_at.elapsed());
    let selected_name = selected.name().ok();
    let start = |device: &cpal::Device| {
        start_device_stream(device, &frame_tx, &state, &stop_flag, &runtime_error_tx)
    };
    let initial_error = match start(&selected) {
        Ok(stream) => return Ok(stream),
        Err(RecorderError::PermissionDenied) => return Err(RecorderError::PermissionDenied),
        Err(error) => error,
    };
    log::warn!("[recorder] selected microphone failed: {initial_error}; trying other inputs");
    let default = host.default_input_device();
    let default_name = default.as_ref().and_then(|device| device.name().ok());
    let mut candidates = Vec::new();
    if let Some(default) = default {
        if default_name != selected_name {
            candidates.push(default);
        }
    }
    if let Ok(devices) = host.input_devices() {
        candidates.extend(devices.filter(|device| {
            let name = device.name().ok();
            name.is_none() || (name != selected_name && name != default_name)
        }));
    }
    try_input_candidates(candidates, initial_error, |device| start(&device))
}

fn try_input_candidates<T, U>(
    candidates: impl IntoIterator<Item = T>,
    mut last_error: RecorderError,
    mut start: impl FnMut(T) -> Result<U, RecorderError>,
) -> Result<U, RecorderError> {
    for candidate in candidates {
        match start(candidate) {
            Ok(stream) => return Ok(stream),
            Err(RecorderError::PermissionDenied) => return Err(RecorderError::PermissionDenied),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn start_device_stream(
    device: &cpal::Device,
    frame_tx: &AudioProcessingSender,
    state: &Arc<StreamState>,
    stop_flag: &Arc<AtomicBool>,
    runtime_error_tx: &Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let mut default = None;
    let initial_error = match device.default_input_config() {
        Ok(config) => {
            default = Some(config.clone());
            match start_native_config(device, config, frame_tx, state, stop_flag, runtime_error_tx)
            {
                Ok(stream) => return Ok(stream),
                Err(RecorderError::PermissionDenied) => {
                    return Err(RecorderError::PermissionDenied)
                }
                Err(error) => error,
            }
        }
        Err(error) => classify_default_config_err(error.to_string()),
    };
    if matches!(initial_error, RecorderError::PermissionDenied) {
        return Err(initial_error);
    }
    // Query alternatives only after failure; never force mono or an ASR rate on hardware.
    let ranges = match device.supported_input_configs() {
        Ok(ranges) => ranges,
        Err(_) => return Err(initial_error),
    };
    let configs = native_input_alternatives(default.as_ref(), ranges);
    try_input_candidates(configs, initial_error, |config| {
        start_native_config(device, config, frame_tx, state, stop_flag, runtime_error_tx)
    })
}

fn native_input_alternatives(
    default: Option<&cpal::SupportedStreamConfig>,
    ranges: impl IntoIterator<Item = cpal::SupportedStreamConfigRange>,
) -> Vec<cpal::SupportedStreamConfig> {
    let native_rate = default.map(|config| config.sample_rate().0);
    let mut candidates = Vec::new();
    for range in ranges {
        let min = range.min_sample_rate().0;
        let max = range.max_sample_rate().0;
        for rate in [native_rate.unwrap_or(max).clamp(min, max), min, max] {
            let candidate = range.with_sample_rate(cpal::SampleRate(rate));
            let same = |other: &cpal::SupportedStreamConfig| {
                other.config() == candidate.config()
                    && other.sample_format() == candidate.sample_format()
            };
            if !default.is_some_and(same) && !candidates.iter().any(same) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn start_native_config(
    device: &cpal::Device,
    supported: cpal::SupportedStreamConfig,
    frame_tx: &AudioProcessingSender,
    state: &Arc<StreamState>,
    stop_flag: &Arc<AtomicBool>,
    runtime_error_tx: &Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let config = supported.config();
    let building_at = std::time::Instant::now();
    state.accepting_audio.store(false, Ordering::Release);
    let stream = build_stream_for_format(
        device,
        &config,
        supported.sample_format(),
        frame_tx.clone(),
        Arc::clone(state),
        Arc::clone(stop_flag),
        config.sample_rate.0,
        config.channels as usize,
        runtime_error_tx.clone(),
    )?;
    state.log_startup_phase("stream_built", building_at.elapsed());
    let playing_at = std::time::Instant::now();
    stream
        .play()
        .map_err(|error| classify_default_config_err(format!("play: {error}")))?;
    state.accepting_audio.store(true, Ordering::Release);
    state.log_startup_phase("stream_playing", playing_at.elapsed());
    log::info!(
        "[recorder] inputDevice={} inputFormat sampleRate={} channels={} fmt={:?}",
        device.name().unwrap_or_else(|_| "<unknown>".into()),
        config.sample_rate.0,
        config.channels,
        supported.sample_format()
    );
    Ok((stream, Arc::clone(state)))
}

fn select_input_device(
    host: &cpal::Host,
    microphone_device_name: Option<&str>,
) -> Result<cpal::Device, RecorderError> {
    let preferred = microphone_device_name
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(preferred) = preferred {
        let devices = host
            .input_devices()
            .map_err(|e| RecorderError::EngineFailed(format!("input_devices: {e}")))?;
        for device in devices {
            if device.name().ok().as_deref() == Some(preferred) {
                return Ok(device);
            }
        }
        log::warn!(
            "[recorder] preferred input device not found; falling back to default: {preferred}"
        );
    }

    if let Some(device) = host.default_input_device() {
        return Ok(device);
    }
    host.input_devices()
        .map_err(|error| RecorderError::EngineFailed(format!("input_devices: {error}")))?
        .next()
        .ok_or(RecorderError::NoInputDevice)
}

/// Startup-time default_input_config failure: coarsely classify permission issues via error
/// string keywords. cpal usually returns `BackendSpecific` when macOS mic authorization is
/// missing; best-effort recognition.
fn classify_default_config_err(msg: String) -> RecorderError {
    let lower = msg.to_lowercase();
    if is_no_device_error(&lower) {
        RecorderError::NoInputDevice
    } else if lower.contains("permission") || lower.contains("denied") || lower.contains("authoriz")
    {
        RecorderError::PermissionDenied
    } else {
        RecorderError::EngineFailed(format!("default_input_config: {msg}"))
    }
}

/// Startup-time build_stream failure: same classification as above; may be a permission issue.
fn classify_build_stream_err(err: cpal::BuildStreamError) -> RecorderError {
    let msg = err.to_string();
    let lower = msg.to_lowercase();
    if is_no_device_error(&lower) {
        RecorderError::NoInputDevice
    } else if lower.contains("permission") || lower.contains("denied") || lower.contains("authoriz")
    {
        RecorderError::PermissionDenied
    } else {
        RecorderError::EngineFailed(format!("build_input_stream: {msg}"))
    }
}

/// Whether the error string implies "no usable input device right now" (as opposed to
/// permission denied). Keyword set kept in sync with permissions.rs::is_no_device_error; the
/// backend-tests harness compiles this module standalone, so no cross-module reference.
fn is_no_device_error(lower: &str) -> bool {
    [
        "no default input device",
        "no default input",
        "no input device",
        "no device",
        "device not found",
        "device not available",
        "not connected",
        "unplugged",
        "disconnected",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

/// `SupportedStreamConfig` -> the concrete build call for that SampleFormat.
/// Only common cpal float and integer formats are supported; others fall back to an error.
#[allow(clippy::too_many_arguments)]
fn build_stream_for_format(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    frame_tx: AudioProcessingSender,
    state: Arc<StreamState>,
    stop_flag: Arc<AtomicBool>,
    input_sr: u32,
    channels: usize,
    runtime_error_tx: Sender<RecorderError>,
) -> Result<cpal::Stream, RecorderError> {
    macro_rules! make_stream {
        ($t:ty, $to_f32:expr) => {{
            let frame_tx = frame_tx.clone();
            let state = Arc::clone(&state);
            let stop_flag = Arc::clone(&stop_flag);
            let runtime_error_for_callback = runtime_error_tx.clone();
            let runtime_error_for_error = runtime_error_tx.clone();
            let error_state = Arc::clone(&state);
            let err_cb = move |err| {
                if !error_state.accepting_audio.load(Ordering::Acquire) {
                    return;
                }
                log::error!("[recorder] stream error: {err}");
                let _ = runtime_error_for_error
                    .send(RecorderError::EngineFailed(format!("stream: {err}")));
            };
            device
                .build_input_stream::<$t, _, _>(
                    config,
                    move |data: &[$t], _info| {
                        if !state.accepting_audio.load(Ordering::Acquire) {
                            return;
                        }
                        if !data.is_empty() {
                            state.note_first_pcm(false);
                        }
                        let mut floats = Vec::with_capacity(data.len());
                        for s in data {
                            floats.push($to_f32(*s));
                        }
                        *state.last_callback_time.lock() = Some(std::time::Instant::now());
                        match frame_tx.try_send(AudioProcessingMessage::Frame(RawAudioFrame {
                            samples: floats,
                            channels,
                            input_sample_rate: input_sr,
                        })) {
                            Ok(()) => {}
                            Err(TrySendError::Disconnected(_)) => {
                                if state.accepting_audio.swap(false, Ordering::AcqRel) {
                                    let _ = runtime_error_for_callback.send(
                                        RecorderError::EngineFailed("audio processing worker disconnected".into()),
                                    );
                                }
                                stop_flag.store(true, Ordering::Release);
                            }
                            Err(TrySendError::Full(_)) => {
                                if state.accepting_audio.swap(false, Ordering::AcqRel) {
                                    let _ = runtime_error_for_callback.send(
                                        RecorderError::EngineFailed(
                                            format!("audio processing backlog exceeded limit: queued_us={} queued_bytes={} callback_samples={} sample_rate={} channels={}",
                                                frame_tx.backlog.micros.load(Ordering::Acquire),
                                                frame_tx.backlog.bytes.load(Ordering::Acquire),
                                                data.len(), input_sr, channels),
                                        ),
                                    );
                                }
                                stop_flag.store(true, Ordering::Release);
                            }
                        }
                    },
                    err_cb,
                    None,
                )
                .map_err(classify_build_stream_err)
        }};
    }

    match sample_format {
        SampleFormat::F32 => make_stream!(f32, |s: f32| s),
        SampleFormat::F64 => make_stream!(f64, |s: f64| s as f32),
        SampleFormat::I64 => make_stream!(i64, |s: i64| (s as f64 / i64::MAX as f64) as f32),
        SampleFormat::U64 => make_stream!(u64, |s: u64| ((s as f64 / u64::MAX as f64) * 2.0 - 1.0)
            as f32),
        SampleFormat::U32 => make_stream!(u32, |s: u32| (s as f64 / u32::MAX as f64 * 2.0 - 1.0)
            as f32),
        SampleFormat::I16 => make_stream!(i16, |s: i16| s as f32 / i16::MAX as f32),
        SampleFormat::U16 => {
            make_stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0)
        }
        SampleFormat::I32 => {
            make_stream!(i32, |s: i32| s as f32 / i32::MAX as f32)
        }
        SampleFormat::I8 => make_stream!(i8, |s: i8| s as f32 / i8::MAX as f32),
        SampleFormat::U8 => {
            make_stream!(u8, |s: u8| (s as f32 - 128.0) / 128.0)
        }
        other => Err(RecorderError::EngineFailed(format!(
            "unsupported sample format: {other:?}"
        ))),
    }
}

struct RawAudioFrame {
    samples: Vec<f32>,
    channels: usize,
    input_sample_rate: u32,
}

enum AudioProcessingMessage {
    Frame(RawAudioFrame),
    Flush(Sender<()>),
    Shutdown,
}

#[derive(Default)]
struct AudioProcessingBacklog {
    micros: AtomicU64,
    bytes: AtomicUsize,
}

struct AudioProcessingReservation {
    backlog: Arc<AudioProcessingBacklog>,
    micros: u64,
    bytes: usize,
}

impl Drop for AudioProcessingReservation {
    fn drop(&mut self) {
        self.backlog.micros.fetch_sub(self.micros, Ordering::AcqRel);
        self.backlog.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct QueuedAudioProcessingMessage {
    message: AudioProcessingMessage,
    _reservation: Option<AudioProcessingReservation>,
}

#[derive(Clone)]
struct AudioProcessingSender {
    sender: SyncSender<QueuedAudioProcessingMessage>,
    backlog: Arc<AudioProcessingBacklog>,
}

impl AudioProcessingSender {
    fn reserve(
        &self,
        message: AudioProcessingMessage,
    ) -> Result<QueuedAudioProcessingMessage, AudioProcessingMessage> {
        let reservation = if let AudioProcessingMessage::Frame(frame) = &message {
            let rate = u64::from(frame.input_sample_rate) * frame.channels as u64;
            if rate == 0 {
                return Err(message);
            }
            let micros = (frame.samples.len() as u64 * 1_000_000).div_ceil(rate);
            let bytes = frame.samples.len() * std::mem::size_of::<f32>();
            if self
                .backlog
                .bytes
                .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                    used.checked_add(bytes)
                        .filter(|next| *next <= AUDIO_PROCESSING_MAX_QUEUED_BYTES)
                })
                .is_err()
            {
                return Err(message);
            }
            if self
                .backlog
                .micros
                .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                    used.checked_add(micros)
                        .filter(|next| *next <= AUDIO_PROCESSING_MAX_QUEUED_US)
                })
                .is_err()
            {
                self.backlog.bytes.fetch_sub(bytes, Ordering::AcqRel);
                return Err(message);
            }
            Some(AudioProcessingReservation {
                backlog: Arc::clone(&self.backlog),
                micros,
                bytes,
            })
        } else {
            None
        };
        Ok(QueuedAudioProcessingMessage {
            message,
            _reservation: reservation,
        })
    }

    fn try_send(
        &self,
        message: AudioProcessingMessage,
    ) -> Result<(), TrySendError<AudioProcessingMessage>> {
        let queued = self.reserve(message).map_err(TrySendError::Full)?;
        self.sender.try_send(queued).map_err(|error| match error {
            TrySendError::Full(queued) => TrySendError::Full(queued.message),
            TrySendError::Disconnected(queued) => TrySendError::Disconnected(queued.message),
        })
    }

    fn send(
        &self,
        message: AudioProcessingMessage,
    ) -> Result<(), std::sync::mpsc::SendError<AudioProcessingMessage>> {
        let queued = self.reserve(message).map_err(std::sync::mpsc::SendError)?;
        self.sender
            .send(queued)
            .map_err(|error| std::sync::mpsc::SendError(error.0.message))
    }
}

struct AudioProcessingWorker {
    join_handle: Option<JoinHandle<()>>,
    shutdown: AudioProcessingSender,
}

impl AudioProcessingWorker {
    fn start(
        processing: openless_core::AudioProcessingConfig,
        consumer: Arc<dyn AudioConsumer>,
        level_handler: Arc<dyn Fn(f32) + Send + Sync>,
        archiver: Option<Arc<WavArchiveWriter>>,
        state: Arc<StreamState>,
    ) -> Result<(Self, AudioProcessingSender), RecorderError> {
        let (sender, receiver) =
            sync_channel::<QueuedAudioProcessingMessage>(AUDIO_PROCESSING_QUEUE_CAPACITY);
        let backlog = Arc::new(AudioProcessingBacklog::default());
        let sender = AudioProcessingSender {
            sender,
            backlog: Arc::clone(&backlog),
        };
        let join_handle = thread::Builder::new()
            .name("openless-audio-processor".into())
            .spawn(move || {
                let mut processor = openless_core::AudioProcessor::new(processing);
                let mut enhancement_enabled = processing.enhancement_enabled;
                let deliver = |chunk: openless_core::audio::NormalizedPcmChunk| {
                    state.note_first_pcm(true);
                    consumer.consume_pcm_chunk(&chunk.pcm_i16_le);
                    if let Some(archiver) = &archiver {
                        archiver.append(&chunk.pcm_i16_le);
                    }
                    level_handler(chunk.level);
                    update_peak(&state.peak_output_rms_milli, chunk.level);
                };
                while let Ok(queued) = receiver.recv() {
                    // Keep the reservation until downstream delivery completes.
                    let QueuedAudioProcessingMessage { message, _reservation } = queued;
                    match message {
                        AudioProcessingMessage::Frame(frame) => {
                            let queued_us = backlog.micros.load(Ordering::Acquire);
                            if enhancement_enabled && queued_us >= AUDIO_PROCESSING_SHED_ENHANCEMENT_US {
                                log::warn!("[recorder] disabling enhancement for this capture: processing backlog {} ms", queued_us / 1_000);
                                if let Some(tail) = processor.disable_enhancement() {
                                    deliver(tail);
                                }
                                enhancement_enabled = false;
                            }
                            if let Some(chunk) = processor.process(
                                &frame.samples,
                                frame.channels,
                                frame.input_sample_rate,
                            ) {
                                deliver(chunk);
                            }
                        }
                        AudioProcessingMessage::Flush(done) => {
                            if let Some(chunk) = processor.finish() {
                                deliver(chunk);
                            }
                            // A warm microphone reuses this worker across recording segments.
                            processor = openless_core::AudioProcessor::new(processing);
                            enhancement_enabled = processing.enhancement_enabled;
                            let _ = done.send(());
                        }
                        AudioProcessingMessage::Shutdown => {
                            if let Some(chunk) = processor.finish() {
                                deliver(chunk);
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|error| {
                RecorderError::EngineFailed(format!("spawn audio processor: {error}"))
            })?;
        Ok((
            Self {
                join_handle: Some(join_handle),
                shutdown: sender.clone(),
            },
            sender,
        ))
    }

    fn finish(mut self) {
        // CoreAudio may retain the callback (and its sender) after Stream::drop,
        // notably for explicitly selected devices in CPAL 0.15. Do not wait for
        // every producer to be destroyed. Native capture is paused before this
        // FIFO barrier, so all accepted frames are processed before shutdown.
        let _ = self.shutdown.send(AudioProcessingMessage::Shutdown);
        if let Some(handle) = self.join_handle.take() {
            if let Err(error) = handle.join() {
                log::warn!("audio processor thread join failed: {:?}", error);
            }
        }
    }
}

/// State carried across callbacks: resample leftovers, diagnostic counters and peak.
struct StreamState {
    capture_id: u64,
    created_at: std::time::Instant,
    first_raw_pcm_us: AtomicU64,
    first_output_pcm_us: AtomicU64,
    processor_sender: Mutex<Option<AudioProcessingSender>>,
    /// Fractional position left unconsumed by the previous callback; linear-interpolation
    /// resampling spans buffers.
    resample_phase: Mutex<f64>,
    /// Last frame of the previous callback (after mono downmix); interpolation start for the
    /// next callback.
    last_sample: Mutex<f32>,
    callback_count: AtomicUsize,
    accepting_audio: AtomicBool,
    peak_input_rms_milli: AtomicUsize,
    peak_output_rms_milli: AtomicUsize,
    /// Timestamp of the last successful consumer call (for liveness detection)
    last_callback_time: Mutex<Option<std::time::Instant>>,
}

impl StreamState {
    fn new() -> Self {
        Self {
            capture_id: NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed),
            created_at: std::time::Instant::now(),
            first_raw_pcm_us: AtomicU64::new(0),
            first_output_pcm_us: AtomicU64::new(0),
            processor_sender: Mutex::new(None),
            resample_phase: Mutex::new(0.0),
            last_sample: Mutex::new(0.0),
            callback_count: AtomicUsize::new(0),
            accepting_audio: AtomicBool::new(true),
            peak_input_rms_milli: AtomicUsize::new(0),
            peak_output_rms_milli: AtomicUsize::new(0),
            // Start as None: timing begins only after the first callback, avoiding false
            // positives for slow-starting devices
            last_callback_time: Mutex::new(None),
        }
    }

    fn log_startup_phase(&self, phase: &'static str, duration: std::time::Duration) {
        log::info!(
            "[audio-startup] capture={} phase={} duration_us={} since_open_us={}",
            self.capture_id,
            phase,
            duration.as_micros(),
            self.created_at.elapsed().as_micros()
        );
    }

    fn note_first_pcm(&self, processed: bool) {
        let slot = if processed {
            &self.first_output_pcm_us
        } else {
            &self.first_raw_pcm_us
        };
        let elapsed = self.created_at.elapsed().as_micros().max(1) as u64;
        if slot
            .compare_exchange(0, elapsed, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            log::info!(
                "[audio-startup] capture={} phase={} since_open_us={}",
                self.capture_id,
                if processed {
                    "first_output_pcm"
                } else {
                    "first_raw_pcm"
                },
                elapsed
            );
        }
    }
}

/// Per-callback pipeline: downmix -> resample -> quantize to i16 -> compute RMS -> feed downstream.
fn process_callback(
    interleaved: &[f32],
    channels: usize,
    input_sr: u32,
    consumer: &dyn AudioConsumer,
    level_handler: &(dyn Fn(f32) + Send + Sync),
    archiver: Option<&WavArchiveWriter>,
    state: &StreamState,
) {
    if !state.accepting_audio.load(Ordering::Acquire) || interleaved.is_empty() || channels == 0 {
        return;
    }
    state.note_first_pcm(false);

    let mono = downmix_to_mono(interleaved, channels);
    let input_rms = rms(&mono);

    let resampled = resample_to_target(&mono, input_sr, TARGET_SAMPLE_RATE, state);
    if resampled.is_empty() {
        return;
    }

    let (pcm_bytes, output_rms) = quantize_to_i16_le(&resampled);
    let level = (output_rms * LEVEL_RMS_GAIN).clamp(0.0, 1.0);

    state.note_first_pcm(true);
    consumer.consume_pcm_chunk(&pcm_bytes);
    if let Some(arch) = archiver {
        arch.append(&pcm_bytes);
    }
    level_handler(level);

    // Update the last successful call timestamp (for liveness detection)
    *state.last_callback_time.lock() = Some(std::time::Instant::now());

    // Diagnostics: peak tracking + periodic log.
    let count = state.callback_count.fetch_add(1, Ordering::Relaxed) + 1;
    update_peak(&state.peak_input_rms_milli, input_rms);
    update_peak(&state.peak_output_rms_milli, output_rms);
    if count == 1 || count % LOG_EVERY_N_CALLBACKS == 0 {
        let pk_in = state.peak_input_rms_milli.load(Ordering::Relaxed) as f32 / 1000.0;
        let pk_out = state.peak_output_rms_milli.load(Ordering::Relaxed) as f32 / 1000.0;
        log::info!(
            "[recorder] cb#{count} inLen={} outLen={} inRMS={:.5} outRMS={:.5} peakIn={:.5} peakOut={:.5}",
            mono.len(),
            resampled.len(),
            input_rms,
            output_rms,
            pk_in,
            pk_out
        );
    }
}

/// Multi-channel interleaved samples -> mono (arithmetic mean).
fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    let frames = interleaved.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        let base = i * channels;
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += interleaved[base + c];
        }
        out.push(sum / channels as f32);
    }
    out
}

/// Linear-interpolation resampling to the target sample rate; state spans buffers.
///
/// Algorithm: the previous callback's tail sample seeds this callback to avoid gaps; a float
/// `phase` tracks "how far past the previous frame we are" and advances by
/// `step = src_sr / dst_sr` per output sample.
fn resample_to_target(samples: &[f32], src_sr: u32, dst_sr: u32, state: &StreamState) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    if src_sr == dst_sr {
        // Pass-through — still update last_sample so switching devices doesn't glitch.
        if let Some(&last) = samples.last() {
            *state.last_sample.lock() = last;
        }
        return samples.to_vec();
    }

    let step = src_sr as f64 / dst_sr as f64;
    let mut phase = *state.resample_phase.lock();
    let prev = *state.last_sample.lock();

    // Estimate capacity: dst_len ≈ src_len / step.
    let estimated = ((samples.len() as f64) / step).ceil() as usize + 1;
    let mut out = Vec::with_capacity(estimated);

    // Treat prev as the sample at virtual index -1.
    // phase means "distance to the start of the current segment", in [0, 1).
    while phase < samples.len() as f64 {
        let idx_floor = phase.floor() as isize;
        let frac = (phase - phase.floor()) as f32;
        let a = if idx_floor < 0 {
            prev
        } else {
            samples[idx_floor as usize]
        };
        let b_index = (idx_floor + 1) as usize;
        if b_index >= samples.len() {
            // No next frame to interpolate — emit the current frame and stop; the next
            // callback continues from here.
            out.push(a);
            phase += step;
            break;
        }
        let b = samples[b_index];
        out.push(a + (b - a) * frac);
        phase += step;
    }

    // Fold phase back to "relative to the next callback's start" — subtract this buffer's length.
    let new_phase = phase - samples.len() as f64;
    *state.resample_phase.lock() = new_phase.max(0.0);
    *state.last_sample.lock() = *samples.last().unwrap_or(&0.0);

    out
}

/// f32 -> i16 little-endian byte stream; also computes RMS (normalized to 0..1).
fn quantize_to_i16_le(samples: &[f32]) -> (Vec<u8>, f32) {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    let mut sum_sq = 0.0f64;
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let q = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&q.to_le_bytes());
        let n = clamped as f64;
        sum_sq += n * n;
    }
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_sq / samples.len() as f64).sqrt() as f32
    };
    (bytes, rms)
}

/// RMS of an f32 slice (normalized to 0..1; input assumed already in [-1, 1]).
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum_sq = 0.0f64;
    for &s in samples {
        let n = s as f64;
        sum_sq += n * n;
    }
    (sum_sq / samples.len() as f64).sqrt() as f32
}

fn pcm_rms_level(pcm: &[u8]) -> f32 {
    let samples = pcm.as_chunks::<2>().0;
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq = samples
        .iter()
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f64)
        .map(|sample| sample * sample)
        .sum::<f64>();
    let rms = (sum_sq / samples.len() as f64).sqrt() / i16::MAX as f64;
    (rms as f32 * LEVEL_RMS_GAIN).clamp(0.0, 1.0)
}

/// Store the f32 peak approximately as an integer atomic in milli units (avoids an extra lock).
fn update_peak(slot: &AtomicUsize, current: f32) {
    let scaled = (current * 1000.0).round().max(0.0) as usize;
    let mut prev = slot.load(Ordering::Relaxed);
    while scaled > prev {
        match slot.compare_exchange_weak(prev, scaled, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => prev = observed,
        }
    }
}

/// Simple append-only writer for 16 kHz / mono / 16-bit PCM WAV.
/// Writes a placeholder header with data_size=0 at construction, appends i16 PCM bytes on each
/// append, and on Drop seeks back to 0 to backfill the RIFF / data length fields — no external
/// finalize call site required.
struct WavArchiver {
    file: std::fs::File,
    bytes_written: u32,
    last_checkpoint_bytes: u32,
}

impl WavArchiver {
    fn create(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(path)?;
        use std::io::Write;
        file.write_all(&build_wav_header(0))?;
        Ok(Self {
            file,
            bytes_written: 0,
            last_checkpoint_bytes: 0,
        })
    }

    fn append(&mut self, pcm_bytes: &[u8]) {
        use std::io::Write;
        if self.file.write_all(pcm_bytes).is_ok() {
            self.bytes_written = self
                .bytes_written
                .saturating_add(pcm_bytes.len().min(u32::MAX as usize) as u32);
            // Keep the header usable during a long meeting. Drop still does
            // the final sync, but a process kill should not leave a WAV with
            // data_size=0 for the entire recording.
            const CHECKPOINT_INTERVAL_BYTES: u32 = 160_000;
            if self
                .bytes_written
                .saturating_sub(self.last_checkpoint_bytes)
                >= CHECKPOINT_INTERVAL_BYTES
            {
                self.checkpoint_header();
            }
        }
    }

    fn checkpoint_header(&mut self) {
        use std::io::{Seek, SeekFrom, Write};
        if self.file.seek(SeekFrom::Start(0)).is_ok() {
            if self
                .file
                .write_all(&build_wav_header(self.bytes_written))
                .is_ok()
            {
                let _ = self.file.seek(SeekFrom::End(0));
                let _ = self.file.sync_data();
                self.last_checkpoint_bytes = self.bytes_written;
            }
        }
    }
}

impl Drop for WavArchiver {
    fn drop(&mut self) {
        self.checkpoint_header();
        let _ = self.file.sync_all();
    }
}

fn build_wav_header(data_size: u32) -> [u8; 44] {
    // Standard 44-byte RIFF/WAVE PCM header, hardcoded for 16 kHz / mono / 16-bit.
    let total_size = data_size.saturating_add(36);
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&total_size.to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&(TARGET_SAMPLE_RATE).to_le_bytes());
    h[28..32].copy_from_slice(&(TARGET_SAMPLE_RATE * 2).to_le_bytes()); // byte rate (sr * block_align)
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_size.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    #[derive(Default)]
    struct RecordingConsumer {
        chunks: StdMutex<Vec<Vec<u8>>>,
        segment_flushes: StdMutex<usize>,
    }

    impl AudioConsumer for RecordingConsumer {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.chunks.lock().unwrap().push(pcm.to_vec());
        }

        fn finish_segment(&self) {
            *self.segment_flushes.lock().unwrap() += 1;
        }
    }

    #[test]
    fn audio_processing_budget_is_duration_based_and_released_on_disconnect() {
        for samples in [48, 480, 4800] {
            let (tx, rx) = sync_channel(AUDIO_PROCESSING_QUEUE_CAPACITY);
            let sender = AudioProcessingSender {
                sender: tx,
                backlog: Arc::new(AudioProcessingBacklog::default()),
            };
            let frame = || {
                AudioProcessingMessage::Frame(RawAudioFrame {
                    samples: vec![0.25; samples],
                    channels: 1,
                    input_sample_rate: 48_000,
                })
            };
            let frames = 3 * 48_000 / samples;
            for _ in 0..frames {
                assert!(sender.try_send(frame()).is_ok());
            }
            assert!(matches!(
                sender.try_send(frame()),
                Err(TrySendError::Full(_))
            ));
            assert_eq!(sender.backlog.micros.load(Ordering::Acquire), 3_000_000);
            assert_eq!(sender.backlog.bytes.load(Ordering::Acquire), 3 * 48_000 * 4);
            drop(rx);
            assert_eq!(sender.backlog.micros.load(Ordering::Acquire), 0);
            assert_eq!(sender.backlog.bytes.load(Ordering::Acquire), 0);
            assert!(matches!(
                sender.try_send(frame()),
                Err(TrySendError::Disconnected(_))
            ));
            assert_eq!(sender.backlog.micros.load(Ordering::Acquire), 0);
            assert_eq!(sender.backlog.bytes.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn audio_processing_budget_also_bounds_large_multichannel_frames() {
        let (tx, _rx) = sync_channel(AUDIO_PROCESSING_QUEUE_CAPACITY);
        let sender = AudioProcessingSender {
            sender: tx,
            backlog: Arc::new(AudioProcessingBacklog::default()),
        };
        let frame = AudioProcessingMessage::Frame(RawAudioFrame {
            samples: vec![0.0; 48_000 * 64],
            channels: 64,
            input_sample_rate: 48_000,
        });
        assert!(matches!(sender.try_send(frame), Err(TrySendError::Full(_))));
        assert_eq!(sender.backlog.micros.load(Ordering::Acquire), 0);
        assert_eq!(sender.backlog.bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn audio_processing_backlog_sheds_enhancement_without_losing_pcm() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (entered_tx, entered_rx) = channel();
        let (resume_tx, resume_rx) = channel();
        let resume_rx = Mutex::new(resume_rx);
        let block_next_level = Arc::new(AtomicBool::new(false));
        let block_for_level = Arc::clone(&block_next_level);
        let level_handler = Arc::new(move |_| {
            if block_for_level.swap(false, Ordering::AcqRel) {
                entered_tx.send(()).unwrap();
                resume_rx
                    .lock()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
        });
        let (worker, sender) = AudioProcessingWorker::start(
            openless_core::AudioProcessingConfig {
                enhancement_enabled: true,
                ..Default::default()
            },
            Arc::clone(&consumer) as Arc<dyn AudioConsumer>,
            level_handler,
            None,
            Arc::new(StreamState::new()),
        )
        .unwrap();
        for _ in 0..2 {
            block_next_level.store(true, Ordering::Release);
            sender
                .send(AudioProcessingMessage::Frame(RawAudioFrame {
                    samples: vec![0.25; 997],
                    channels: 1,
                    input_sample_rate: TARGET_SAMPLE_RATE,
                }))
                .unwrap();
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            // One second of short callbacks exceeds the old 64-message limit.
            for _ in 0..100 {
                assert!(sender
                    .try_send(AudioProcessingMessage::Frame(RawAudioFrame {
                        samples: vec![0.5; 160],
                        channels: 1,
                        input_sample_rate: TARGET_SAMPLE_RATE,
                    }))
                    .is_ok());
            }
            resume_tx.send(()).unwrap();
            let (done_tx, done_rx) = channel();
            sender.send(AudioProcessingMessage::Flush(done_tx)).unwrap();
            done_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        worker.finish();
        let delivered: Vec<u8> = consumer
            .chunks
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .copied()
            .collect();
        let segment_samples = 997 + 160 * 100;
        assert_eq!(delivered.len(), segment_samples * 2 * 2);
        let decoded = decode_i16_le(&delivered);
        assert_eq!(&decoded[..segment_samples], &decoded[segment_samples..]);
        assert!(decoded[..997].iter().any(|sample| *sample != 8191));
        assert!(decoded[997..segment_samples]
            .iter()
            .all(|sample| *sample == 16383));
        assert_eq!(sender.backlog.micros.load(Ordering::Acquire), 0);
        assert_eq!(sender.backlog.bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn audio_processing_worker_drains_accepted_frames_before_join() {
        let consumer = Arc::new(RecordingConsumer::default());
        let state = Arc::new(StreamState::new());
        let (worker, sender) = AudioProcessingWorker::start(
            openless_core::AudioProcessingConfig {
                enhancement_enabled: true,
                ..Default::default()
            },
            Arc::clone(&consumer) as Arc<dyn AudioConsumer>,
            Arc::new(|_| {}),
            None,
            state,
        )
        .unwrap();
        sender
            .send(AudioProcessingMessage::Frame(RawAudioFrame {
                samples: vec![0.25, -0.25],
                channels: 1,
                input_sample_rate: TARGET_SAMPLE_RATE,
            }))
            .unwrap();
        drop(sender);
        worker.finish();

        let chunks = consumer.chunks.lock().unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(decode_i16_le(&chunks[0]).len(), 2);
    }

    #[test]
    fn audio_processing_flush_delivers_tail_to_asr_and_archive_before_reset() {
        let consumer = Arc::new(RecordingConsumer::default());
        let path = std::env::temp_dir().join(format!(
            "openless-dsp-tail-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let archive = Arc::new(WavArchiveWriter::create(&path).unwrap());
        let (worker, sender) = AudioProcessingWorker::start(
            openless_core::AudioProcessingConfig {
                enhancement_enabled: true,
                ..Default::default()
            },
            Arc::clone(&consumer) as Arc<dyn AudioConsumer>,
            Arc::new(|_| {}),
            Some(archive.clone()),
            Arc::new(StreamState::new()),
        )
        .unwrap();
        sender
            .send(AudioProcessingMessage::Frame(RawAudioFrame {
                samples: vec![0.15; 317],
                channels: 1,
                input_sample_rate: TARGET_SAMPLE_RATE,
            }))
            .unwrap();
        let (done_tx, done_rx) = channel();
        sender.send(AudioProcessingMessage::Flush(done_tx)).unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            consumer
                .chunks
                .lock()
                .unwrap()
                .iter()
                .map(Vec::len)
                .sum::<usize>(),
            317 * 2
        );
        sender
            .send(AudioProcessingMessage::Frame(RawAudioFrame {
                samples: vec![-0.15; 211],
                channels: 1,
                input_sample_rate: TARGET_SAMPLE_RATE,
            }))
            .unwrap();
        worker.finish();
        archive.finish();
        let delivered: Vec<u8> = consumer
            .chunks
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .copied()
            .collect();
        assert_eq!(delivered.len(), (317 + 211) * 2);
        let wav = std::fs::read(&path).unwrap();
        assert_eq!(&wav[44..], delivered);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn audio_processing_shutdown_drains_frames_with_a_retained_native_sender() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (worker, sender) = AudioProcessingWorker::start(
            openless_core::AudioProcessingConfig {
                enhancement_enabled: false,
                ..Default::default()
            },
            Arc::clone(&consumer) as Arc<dyn AudioConsumer>,
            Arc::new(|_| {}),
            None,
            Arc::new(StreamState::new()),
        )
        .unwrap();
        for _ in 0..8 {
            sender
                .send(AudioProcessingMessage::Frame(RawAudioFrame {
                    samples: vec![0.25, -0.25],
                    channels: 1,
                    input_sample_rate: TARGET_SAMPLE_RATE,
                }))
                .unwrap();
        }
        // Model CPAL's retained callback: it must not prevent stop from returning.
        let retained_native_sender = sender.clone();
        drop(sender);
        let (done_tx, done_rx) = channel();
        let stopped = thread::spawn(move || {
            worker.finish();
            let _ = done_tx.send(());
        });
        let result = done_rx.recv_timeout(std::time::Duration::from_secs(2));
        // Drop even on failure so an EOF-based implementation cannot hang the suite.
        drop(retained_native_sender);
        assert!(
            result.is_ok(),
            "stopping must not depend on callback destruction"
        );
        stopped.join().unwrap();
        assert_eq!(consumer.chunks.lock().unwrap().len(), 8);
    }

    fn decode_i16_le(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    }

    #[test]
    fn warm_promotion_replays_staged_prefix_to_finish_consumer() {
        let sink = WarmCaptureSink::new(STAGED_CAPTURE_MAX_BYTES);
        let formal = Arc::new(RecordingConsumer::default());
        let finish = Arc::new(RecordingConsumer::default());
        sink.arm_with_limit(7, false, VOICE_STAGED_CAPTURE_MAX_BYTES)
            .unwrap();
        sink.set_activation_consumer_during_capture(Some(
            Arc::clone(&finish) as Arc<dyn AudioConsumer>
        ));
        let speech = [0x00, 0x40].repeat(16_000);
        sink.consume_pcm_chunk(&speech);

        sink.promote(
            7,
            Arc::clone(&formal) as Arc<dyn AudioConsumer>,
            Arc::new(|_| {}),
            None,
        )
        .unwrap();

        assert_eq!(formal.chunks.lock().unwrap().as_slice(), &[speech.to_vec()]);
        assert_eq!(finish.chunks.lock().unwrap().as_slice(), &[speech.to_vec()]);
    }

    #[test]
    fn warm_promotion_replays_bounded_preroll_to_formal_consumer() {
        let sink = WarmCaptureSink::new(STAGED_CAPTURE_MAX_BYTES);
        let formal = Arc::new(RecordingConsumer::default());
        let prefix = vec![1, 2, 3, 4];
        let speech = vec![0x00, 0x40, 0x00, 0x40];
        sink.consume_pcm_chunk(&prefix);
        sink.arm_with_limit(8, true, STAGED_CAPTURE_MAX_BYTES)
            .unwrap();
        sink.consume_pcm_chunk(&speech);

        sink.promote(
            8,
            Arc::clone(&formal) as Arc<dyn AudioConsumer>,
            Arc::new(|_| {}),
            None,
        )
        .unwrap();

        assert_eq!(
            formal.chunks.lock().unwrap().as_slice(),
            &[vec![1, 2, 3, 4, 0x00, 0x40, 0x00, 0x40]]
        );
    }
    #[test]
    fn low_memory_disables_only_optional_warm_capture() {
        for low_latency in [false, true] {
            for low_memory in [false, true] {
                assert!(warm_capture_enabled(low_latency, low_memory, true));
            }
        }
        assert!(warm_capture_enabled(true, false, false));
        assert!(!warm_capture_enabled(true, true, false));
        assert!(!warm_capture_enabled(false, false, false));
        assert!(!warm_capture_enabled(false, true, false));
    }

    #[test]
    fn activation_vad_skips_silence_and_preserves_speech_onset() {
        let sink = WarmCaptureSink::new(STAGED_CAPTURE_MAX_BYTES);
        let activation = Arc::new(RecordingConsumer::default());
        sink.set_activation_consumer(Some(Arc::clone(&activation) as Arc<dyn AudioConsumer>));
        let silence = vec![0; 320];
        sink.consume_pcm_chunk(&silence);
        assert!(activation.chunks.lock().unwrap().is_empty());

        let speech = vec![0x00, 0x40, 0x00, 0x40];
        sink.consume_pcm_chunk(&speech);
        let chunks = activation.chunks.lock().unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].ends_with(&speech));
    }

    #[test]
    fn activation_vad_preserves_large_blocks_and_does_not_replay_forwarded_audio() {
        let mut vad = ActivationVad::default();
        let silence = vec![0; ACTIVATION_VAD_PRE_ROLL_BYTES + 320];
        assert!(vad.gate(&silence).pcm.is_none());
        let speech = [0x00, 0x40].repeat(16_000);
        let first = vad.gate(&speech).pcm.unwrap();
        assert_eq!(first.len(), ACTIVATION_VAD_PRE_ROLL_BYTES + speech.len());
        assert_eq!(
            &first[..ACTIVATION_VAD_PRE_ROLL_BYTES],
            &silence[..ACTIVATION_VAD_PRE_ROLL_BYTES]
        );
        assert_eq!(&first[ACTIVATION_VAD_PRE_ROLL_BYTES..], &speech);
        assert_eq!(vad.gate(&speech[..320]).pcm.unwrap(), speech[..320]);
        for _ in 0..ACTIVATION_VAD_HANGOVER_CHUNKS {
            assert_eq!(vad.gate(&[0; 320]).pcm.unwrap(), [0; 320]);
        }
        assert!(vad.gate(&[0; 320]).end_of_speech);
        let next = vad.gate(&speech[..320]).pcm.unwrap();
        assert_eq!(next.len(), 640);
        assert_eq!(&next[320..], &speech[..320]);
    }

    #[test]
    fn activation_vad_hangover_is_bounded() {
        let mut vad = ActivationVad::default();
        let speech = vec![0x00, 0x40, 0x00, 0x40];
        assert!(vad.gate(&speech).pcm.is_some());
        for _ in 0..ACTIVATION_VAD_HANGOVER_CHUNKS {
            let output = vad.gate(&[0, 0, 0, 0]);
            assert!(output.pcm.is_some());
            assert!(!output.end_of_speech);
        }
        let output = vad.gate(&[0, 0, 0, 0]);
        assert!(output.pcm.is_none());
        assert!(output.end_of_speech);
        let output = vad.gate(&[0, 0, 0, 0]);
        assert!(output.pcm.is_none());
        assert!(!output.end_of_speech);
    }

    #[test]
    fn activation_vad_flushes_after_hangover_without_flushing_sustained_silence() {
        let sink = WarmCaptureSink::new(STAGED_CAPTURE_MAX_BYTES);
        let activation = Arc::new(RecordingConsumer::default());
        sink.set_activation_consumer(Some(Arc::clone(&activation) as Arc<dyn AudioConsumer>));
        let speech = vec![0x00, 0x40, 0x00, 0x40];
        sink.consume_pcm_chunk(&speech);
        for _ in 0..=ACTIVATION_VAD_HANGOVER_CHUNKS {
            sink.consume_pcm_chunk(&[0, 0, 0, 0]);
        }
        sink.consume_pcm_chunk(&[0, 0, 0, 0]);

        assert_eq!(*activation.segment_flushes.lock().unwrap(), 1);
        assert!(activation.chunks.lock().unwrap().len() >= 1);
    }
    #[test]
    fn activation_fault_handler_uses_voice_generation_not_stream_generation() {
        let hub = WarmCaptureHub::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_for_handler = Arc::clone(&called);
        hub.set_activation_generation(41);
        hub.set_activation_fault_handler(
            41,
            Some(Arc::new(move |_| {
                called_for_handler.store(true, Ordering::Release);
            })),
        );

        // The warm stream generation starts at zero; it is intentionally not the
        // voice-activation generation owned by Core.
        hub.stop_after_runtime_error(0, RecorderError::EngineFailed("stream failed".into()));

        assert!(called.load(Ordering::Acquire));
    }

    #[test]
    fn native_alternatives_preserve_channels_format_and_advertised_rates() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            cpal::SampleRate(44_100),
            cpal::SampleRate(48_000),
            cpal::SupportedBufferSize::Unknown,
            SampleFormat::F32,
        );
        let native = range.with_sample_rate(cpal::SampleRate(48_000));
        let alternatives = native_input_alternatives(Some(&native), [range, range]);
        assert_eq!(alternatives.len(), 1);
        assert_eq!(alternatives[0].channels(), 2);
        assert_eq!(alternatives[0].sample_format(), SampleFormat::F32);
        assert_eq!(alternatives[0].sample_rate().0, 44_100);
    }

    #[test]
    fn unstarted_candidate_cannot_feed_asr_or_mark_audio_valid() {
        let state = StreamState::new();
        state.accepting_audio.store(false, Ordering::Release);
        let consumer = RecordingConsumer::default();
        process_callback(
            &[0.5; 320],
            1,
            TARGET_SAMPLE_RATE,
            &consumer,
            &|_| {},
            None,
            &state,
        );
        assert!(consumer.chunks.lock().unwrap().is_empty());
        assert!(state.last_callback_time.lock().is_none());
    }

    #[test]
    fn startup_uses_candidates_in_order_and_stops_after_success() {
        let mut attempted = Vec::new();
        let result = try_input_candidates(
            ["native", "alternative", "other device"],
            RecorderError::NoInputDevice,
            |candidate| {
                attempted.push(candidate);
                if candidate == "alternative" {
                    Ok(candidate)
                } else {
                    Err(RecorderError::EngineFailed(candidate.into()))
                }
            },
        )
        .unwrap();
        assert_eq!(result, "alternative");
        assert_eq!(attempted, ["native", "alternative"]);
    }

    #[test]
    fn permission_failure_never_probes_more_devices() {
        let mut attempts = 0;
        let result: Result<(), _> =
            try_input_candidates([1, 2], RecorderError::NoInputDevice, |_| {
                attempts += 1;
                Err(RecorderError::PermissionDenied)
            });
        assert!(matches!(result, Err(RecorderError::PermissionDenied)));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn downmix_to_mono_averages_complete_interleaved_frames() {
        let mono = downmix_to_mono(&[1.0, -1.0, 0.5, 0.25, 0.0], 2);

        assert_eq!(mono, vec![0.0, 0.375]);
    }

    #[test]
    fn quantize_to_i16_le_clamps_and_reports_rms() {
        let (bytes, rms) = quantize_to_i16_le(&[-2.0, 0.0, 0.5, 2.0]);
        let samples = bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();

        assert_eq!(samples, vec![-32767, 0, 16383, 32767]);
        assert!((rms - 0.75).abs() < 0.0001);
    }

    #[test]
    fn resample_passthrough_updates_tail_sample_without_phase_drift() {
        let state = StreamState::new();
        *state.resample_phase.lock() = 0.5;

        let out = resample_to_target(
            &[0.1, -0.2, 0.3],
            TARGET_SAMPLE_RATE,
            TARGET_SAMPLE_RATE,
            &state,
        );

        assert_eq!(out, vec![0.1, -0.2, 0.3]);
        assert_eq!(*state.last_sample.lock(), 0.3);
        assert_eq!(*state.resample_phase.lock(), 0.5);
    }

    #[test]
    fn resample_upsamples_with_linear_interpolation_and_tail_state() {
        let state = StreamState::new();

        let out = resample_to_target(&[0.0, 1.0], 8_000, TARGET_SAMPLE_RATE, &state);

        assert_eq!(out, vec![0.0, 0.5, 1.0]);
        assert_eq!(*state.last_sample.lock(), 1.0);
        assert_eq!(*state.resample_phase.lock(), 0.0);
    }

    #[test]
    fn process_callback_resamples_non_target_input_before_emitting_pcm() {
        let consumer = RecordingConsumer::default();
        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        let state = StreamState::new();

        process_callback(
            &[0.0, 1.0],
            1,
            8_000,
            &consumer,
            &move |level| levels_for_handler.lock().unwrap().push(level),
            None,
            &state,
        );

        let chunks = consumer.chunks.lock().unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(decode_i16_le(&chunks[0]), vec![0, 16383, 32767]);
        assert_eq!(*levels.lock().unwrap(), vec![1.0]);
        assert_eq!(state.callback_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn process_callback_reports_scaled_rms_level_and_peaks() {
        let consumer = RecordingConsumer::default();
        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        let state = StreamState::new();

        process_callback(
            &[0.125, -0.125],
            1,
            TARGET_SAMPLE_RATE,
            &consumer,
            &move |level| levels_for_handler.lock().unwrap().push(level),
            None,
            &state,
        );

        let levels = levels.lock().unwrap();
        assert_eq!(levels.len(), 1);
        assert!((levels[0] - 0.5).abs() < 0.0001);
        assert_eq!(state.peak_input_rms_milli.load(Ordering::Relaxed), 125);
        assert_eq!(state.peak_output_rms_milli.load(Ordering::Relaxed), 125);
    }

    #[test]
    fn process_callback_emits_pcm_level_and_liveness_marker() {
        let consumer = RecordingConsumer::default();
        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        let state = StreamState::new();

        process_callback(
            &[0.25, -0.25],
            1,
            TARGET_SAMPLE_RATE,
            &consumer,
            &move |level| levels_for_handler.lock().unwrap().push(level),
            None,
            &state,
        );

        let chunks = consumer.chunks.lock().unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 4);
        assert_eq!(*levels.lock().unwrap(), vec![1.0]);
        assert!(state.last_callback_time.lock().is_some());
        assert_eq!(state.callback_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn staged_sink_is_bounded_and_keeps_complete_samples() {
        let sink = CaptureSink::new(6);
        sink.consume_pcm_chunk(&[1, 2, 3, 4, 5, 6, 7, 8]);
        sink.consume_pcm_chunk(&[9]);

        let state = sink.state.lock();
        let CaptureSinkState::Staging { pcm } = &*state else {
            panic!("sink was promoted unexpectedly");
        };
        assert_eq!(pcm.len(), 6);
        assert_eq!(
            pcm.iter().copied().collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn staged_sink_promotes_prefix_before_live_pcm_and_levels() {
        let sink = CaptureSink::new(32);
        let consumer = Arc::new(RecordingConsumer::default());
        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        sink.consume_pcm_chunk(&[1, 2]);
        sink.consume_pcm_chunk(&[3, 4]);
        sink.promote(
            Arc::clone(&consumer) as Arc<dyn AudioConsumer>,
            Arc::new(move |level| levels_for_handler.lock().unwrap().push(level)),
            None,
        )
        .unwrap();
        sink.consume_pcm_chunk(&[5, 6]);
        sink.publish_level(0.5);

        assert_eq!(
            consumer.chunks.lock().unwrap().as_slice(),
            &[vec![1, 2, 3, 4], vec![5, 6]]
        );
        let levels = levels.lock().unwrap();
        assert_eq!(levels.len(), 2);
        assert!(levels[0] > 0.0);
        assert_eq!(levels[1], 0.5);
    }

    #[test]
    fn discarded_staged_sink_never_forwards_pcm() {
        let sink = CaptureSink::new(32);
        let consumer = Arc::new(RecordingConsumer::default());
        sink.consume_pcm_chunk(&[1, 2]);
        sink.discard();
        assert!(sink
            .promote(consumer as Arc<dyn AudioConsumer>, Arc::new(|_| {}), None)
            .is_err());
        sink.consume_pcm_chunk(&[3, 4]);
        let is_discarded = matches!(&*sink.state.lock(), CaptureSinkState::Discarded);
        assert!(is_discarded);
    }

    #[test]
    fn warm_sink_captures_bounded_preroll_and_post_press_audio_in_order() {
        let sink = WarmCaptureSink::new(8);
        sink.consume_pcm_chunk(&[1, 2, 3, 4, 5, 6, 7, 8]);
        sink.arm(17, true).unwrap();
        sink.consume_pcm_chunk(&[9, 10]);
        sink.publish_level(0.75);

        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        let consumer = Arc::new(RecordingConsumer::default());
        sink.promote(
            17,
            consumer.clone(),
            Arc::new(move |level| levels_for_handler.lock().unwrap().push(level)),
            None,
        )
        .unwrap();
        sink.consume_pcm_chunk(&[11, 12]);
        sink.publish_level(0.5);

        assert_eq!(
            consumer.chunks.lock().unwrap().as_slice(),
            &[vec![3, 4, 5, 6, 7, 8, 9, 10], vec![11, 12]]
        );
        assert_eq!(levels.lock().unwrap().as_slice(), &[0.75, 0.5]);
    }

    #[test]
    fn warm_sink_activation_consumer_receives_standby_pcm_without_claiming_archive() {
        let sink = WarmCaptureSink::new(16);
        let consumer = Arc::new(RecordingConsumer::default());
        sink.set_activation_consumer(Some(consumer.clone() as Arc<dyn AudioConsumer>));
        sink.consume_pcm_chunk(&[1, 2, 3, 4]);
        sink.set_activation_consumer(None);
        sink.consume_pcm_chunk(&[5, 6]);

        assert_eq!(consumer.chunks.lock().unwrap().as_slice(), &[vec![1, 2, 3, 4]]);
        assert!(matches!(&*sink.state.lock(), WarmCaptureSinkState::Ring { .. }));
    }

    #[test]
    fn warm_sink_activation_arm_excludes_wake_preroll() {
        let sink = WarmCaptureSink::new(16);
        sink.consume_pcm_chunk(&[1, 2, 3, 4]);
        sink.arm(19, false).unwrap();
        sink.consume_pcm_chunk(&[5, 6]);
        let consumer = Arc::new(RecordingConsumer::default());
        sink.promote(19, consumer.clone(), Arc::new(|_| {}), None)
            .unwrap();
        assert_eq!(consumer.chunks.lock().unwrap().as_slice(), &[vec![5, 6]]);
    }
    #[test]
    fn warm_sink_staging_is_bounded_to_the_capture_capacity() {
        let sink = WarmCaptureSink::new(8);
        sink.consume_pcm_chunk(&[1, 2, 3, 4, 5, 6, 7, 8]);
        sink.arm(17, true).unwrap();
        sink.consume_pcm_chunk(&[9, 10, 11, 12, 13, 14, 15, 16]);
        sink.consume_pcm_chunk(&[17, 18, 19, 20, 21, 22]);

        let consumer = Arc::new(RecordingConsumer::default());
        sink.promote(17, consumer.clone(), Arc::new(|_| {}), None)
            .unwrap();

        assert_eq!(
            consumer.chunks.lock().unwrap().as_slice(),
            &[vec![9, 10, 11, 12, 13, 14, 15, 16]]
        );
    }

    #[test]
    fn warm_sink_without_preroll_keeps_latest_post_wake_audio() {
        let sink = WarmCaptureSink::new(8);
        sink.arm(17, false).unwrap();
        sink.consume_pcm_chunk(&[1, 2, 3, 4, 5, 6, 7, 8]);
        sink.consume_pcm_chunk(&[9, 10, 11, 12, 13, 14, 15, 16]);
        sink.consume_pcm_chunk(&[17, 18, 19, 20, 21, 22]);

        let consumer = Arc::new(RecordingConsumer::default());
        sink.promote(17, consumer.clone(), Arc::new(|_| {}), None)
            .unwrap();

        assert_eq!(
            consumer.chunks.lock().unwrap().as_slice(),
            &[vec![15, 16, 17, 18, 19, 20, 21, 22]]
        );
    }

    #[test]
    fn warm_sink_discard_removes_only_matching_generation() {
        let sink = WarmCaptureSink::new(16);
        sink.consume_pcm_chunk(&[1, 2]);
        sink.arm(17, true).unwrap();
        sink.arm(18, true).unwrap();
        sink.discard(17);
        sink.consume_pcm_chunk(&[3, 4]);

        let first = Arc::new(RecordingConsumer::default());
        let second = Arc::new(RecordingConsumer::default());
        sink.promote(18, second.clone(), Arc::new(|_| {}), None)
            .unwrap();
        sink.consume_pcm_chunk(&[5, 6]);

        assert!(first.chunks.lock().unwrap().is_empty());
        assert_eq!(
            second.chunks.lock().unwrap().as_slice(),
            &[vec![1, 2, 3, 4], vec![5, 6]]
        );
        assert!(sink.promote(17, first, Arc::new(|_| {}), None).is_err());
    }

    fn complete_hub_operation(operation: impl FnOnce() + Send + 'static) {
        let (finished, completion) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            operation();
            finished.send(()).unwrap();
        });
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("capture lifecycle operation did not finish");
        worker.join().unwrap();
    }

    #[test]
    fn cold_capture_applies_deferred_configuration_without_deadlock() {
        let hub = WarmCaptureHub::new();
        hub.begin_cold_capture().unwrap();
        hub.configure(
            false,
            Some("next microphone".into()),
            openless_core::AudioProcessingConfig::default(),
        );
        assert!(hub.pending_configuration.lock().is_some());
        assert!(hub.device_name.lock().is_none());

        let worker_hub = Arc::clone(&hub);
        complete_hub_operation(move || worker_hub.end_cold_capture());
        assert!(hub.pending_configuration.lock().is_none());
        assert!(!hub.enabled.load(Ordering::Acquire));
        assert_eq!(hub.device_name.lock().as_deref(), Some("next microphone"));
        // Applying the deferred change must leave the next capture admissible.
        hub.begin_cold_capture().unwrap();
        hub.end_cold_capture();
        assert!(hub.pending_configuration.lock().is_none());
    }

    #[test]
    fn warm_config_change_keeps_staged_claim_on_cold_fallback() {
        let hub = WarmCaptureHub::new();
        hub.enabled.store(true, Ordering::Release);
        hub.sink.arm(42, true).unwrap();

        hub.configure(
            false,
            Some("next microphone".into()),
            openless_core::AudioProcessingConfig::default(),
        );

        assert!(hub.runtime_fallback_claims.lock().contains(&42));
        assert!(hub.pending_configuration.lock().is_some());
        assert!(!hub.sink.has_claims());
        hub.release_claim(42);
    }

    #[test]
    fn last_warm_capture_applies_deferred_configuration_without_deadlock() {
        for claim_count in [1, 2] {
            let hub = WarmCaptureHub::new();
            hub.enabled.store(true, Ordering::Release);
            for press_id in 1..=claim_count {
                hub.sink.arm(press_id, true).unwrap();
                hub.sink
                    .promote(
                        press_id,
                        Arc::new(RecordingConsumer::default()),
                        Arc::new(|_| {}),
                        None,
                    )
                    .unwrap();
            }
            hub.configure(
                false,
                Some("next microphone".into()),
                openless_core::AudioProcessingConfig::default(),
            );
            if claim_count == 2 {
                hub.release_claim(1);
                assert!(hub.enabled.load(Ordering::Acquire));
                assert!(hub.pending_configuration.lock().is_some());
                assert!(hub.device_name.lock().is_none());
            }

            let worker_hub = Arc::clone(&hub);
            complete_hub_operation(move || worker_hub.release_claim(claim_count));
            assert!(hub.pending_configuration.lock().is_none());
            assert!(!hub.enabled.load(Ordering::Acquire));
            assert_eq!(hub.device_name.lock().as_deref(), Some("next microphone"));
            assert!(!hub.sink.has_claims());
            hub.release_claim(claim_count);
            hub.begin_cold_capture().unwrap();
            hub.end_cold_capture();
            assert!(hub.pending_configuration.lock().is_none());
        }
    }
}
