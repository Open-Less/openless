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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::Arc;
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
/// Max queued PCM chunks for the archive writer thread. When full, new archive chunks are
/// dropped instead of blocking the realtime callback; ASR still receives the full PCM, and
/// archive messages accepted before stop are fully flushed.
const WAV_ARCHIVE_QUEUE_CAPACITY: usize = 256;

/// Downstream that receives resampled Int16 PCM bytes (little-endian).
pub trait AudioConsumer: Send + Sync {
    /// Each delivery is a little-endian byte sequence of Int16 samples;
    /// the length is always a multiple of 2.
    fn consume_pcm_chunk(&self, pcm: &[u8]);
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
#[derive(Debug, Error)]
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
        // Startup signal: the child thread reports the result via startup_tx once the Stream
        // is constructed.
        let (startup_tx, startup_rx) = channel::<Result<(), RecorderError>>();
        // Runtime errors: once the Stream has started successfully, cpal reports them
        // asynchronously via err_cb.
        let (runtime_error_tx, runtime_error_rx) = channel::<RecorderError>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop_flag);

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
            },
            runtime_error_rx,
            archive_active,
        ))
    }

    /// Stop capture and wait for the audio thread to exit.
    ///
    /// Takes `self` (consuming), matching the Swift API semantics — a one-shot resource.
    pub fn stop(self) {
        let Recorder {
            stop_flag,
            join_handle,
            archive_writer,
        } = self;
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
    stop_flag: Arc<AtomicBool>,
    startup_tx: Sender<Result<(), RecorderError>>,
    runtime_error_tx: Sender<RecorderError>,
) {
    let (stream, state) = match build_input_stream(
        microphone_device_name,
        consumer,
        level_handler,
        archiver,
        runtime_error_tx.clone(),
    ) {
        Ok(s) => s,
        Err(err) => {
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

    // Wait for the watchdog thread to exit
    if let Some(handle) = watchdog_handle {
        let _ = handle.join();
    }
}

/// Try the selected microphone's native format before other advertised formats/devices.
fn build_input_stream(
    microphone_device_name: Option<String>,
    consumer: Arc<dyn AudioConsumer>,
    level_handler: Arc<dyn Fn(f32) + Send + Sync>,
    archiver: Option<Arc<WavArchiveWriter>>,
    runtime_error_tx: Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let host = cpal::default_host();
    let selected = select_input_device(&host, microphone_device_name.as_deref())?;
    let selected_name = selected.name().ok();
    let start = |device: &cpal::Device| {
        start_device_stream(
            device,
            &consumer,
            &level_handler,
            archiver.clone(),
            &runtime_error_tx,
        )
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
    consumer: &Arc<dyn AudioConsumer>,
    level_handler: &Arc<dyn Fn(f32) + Send + Sync>,
    archiver: Option<Arc<WavArchiveWriter>>,
    runtime_error_tx: &Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let mut default = None;
    let initial_error = match device.default_input_config() {
        Ok(config) => {
            default = Some(config.clone());
            match start_native_config(
                device,
                config,
                consumer,
                level_handler,
                archiver.clone(),
                runtime_error_tx,
            ) {
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
        start_native_config(
            device,
            config,
            consumer,
            level_handler,
            archiver.clone(),
            runtime_error_tx,
        )
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
    consumer: &Arc<dyn AudioConsumer>,
    level_handler: &Arc<dyn Fn(f32) + Send + Sync>,
    archiver: Option<Arc<WavArchiveWriter>>,
    runtime_error_tx: &Sender<RecorderError>,
) -> Result<(cpal::Stream, Arc<StreamState>), RecorderError> {
    let config = supported.config();
    let state = Arc::new(StreamState::new());
    state.accepting_audio.store(false, Ordering::Release);
    let stream = build_stream_for_format(
        device,
        &config,
        supported.sample_format(),
        Arc::clone(consumer),
        Arc::clone(level_handler),
        archiver,
        Arc::clone(&state),
        config.sample_rate.0,
        config.channels as usize,
        runtime_error_tx.clone(),
    )?;
    stream
        .play()
        .map_err(|error| classify_default_config_err(format!("play: {error}")))?;
    state.accepting_audio.store(true, Ordering::Release);
    log::info!(
        "[recorder] inputDevice={} inputFormat sampleRate={} channels={} fmt={:?}",
        device.name().unwrap_or_else(|_| "<unknown>".into()),
        config.sample_rate.0,
        config.channels,
        supported.sample_format()
    );
    Ok((stream, state))
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
    consumer: Arc<dyn AudioConsumer>,
    level_handler: Arc<dyn Fn(f32) + Send + Sync>,
    archiver: Option<Arc<WavArchiveWriter>>,
    state: Arc<StreamState>,
    input_sr: u32,
    channels: usize,
    runtime_error_tx: Sender<RecorderError>,
) -> Result<cpal::Stream, RecorderError> {
    macro_rules! make_stream {
        ($t:ty, $to_f32:expr) => {{
            let consumer = Arc::clone(&consumer);
            let level_handler = Arc::clone(&level_handler);
            let archiver = archiver.clone();
            let state = Arc::clone(&state);
            let runtime_error_tx = runtime_error_tx.clone();
            let error_state = Arc::clone(&state);
            let err_cb = move |err| {
                if !error_state.accepting_audio.load(Ordering::Acquire) {
                    return;
                }
                log::error!("[recorder] stream error: {err}");
                let _ =
                    runtime_error_tx.send(RecorderError::EngineFailed(format!("stream: {err}")));
            };
            device
                .build_input_stream::<$t, _, _>(
                    config,
                    move |data: &[$t], _info| {
                        let mut floats = Vec::with_capacity(data.len());
                        for s in data {
                            floats.push($to_f32(*s));
                        }
                        process_callback(
                            &floats,
                            channels,
                            input_sr,
                            consumer.as_ref(),
                            level_handler.as_ref(),
                            archiver.as_deref(),
                            &state,
                        );
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

/// State carried across callbacks: resample leftovers, diagnostic counters and peak.
struct StreamState {
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

    let mono = downmix_to_mono(interleaved, channels);
    let input_rms = rms(&mono);

    let resampled = resample_to_target(&mono, input_sr, TARGET_SAMPLE_RATE, state);
    if resampled.is_empty() {
        return;
    }

    let (pcm_bytes, output_rms) = quantize_to_i16_le(&resampled);
    let level = (output_rms * LEVEL_RMS_GAIN).clamp(0.0, 1.0);

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
    }

    impl AudioConsumer for RecordingConsumer {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.chunks.lock().unwrap().push(pcm.to_vec());
        }
    }

    fn decode_i16_le(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
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
    fn process_callback_ignores_empty_or_zero_channel_input_without_liveness_marker() {
        let consumer = RecordingConsumer::default();
        let levels = Arc::new(StdMutex::new(Vec::new()));
        let levels_for_handler = Arc::clone(&levels);
        let state = StreamState::new();

        process_callback(
            &[],
            1,
            TARGET_SAMPLE_RATE,
            &consumer,
            &move |level| levels_for_handler.lock().unwrap().push(level),
            None,
            &state,
        );
        process_callback(
            &[0.25, -0.25],
            0,
            TARGET_SAMPLE_RATE,
            &consumer,
            &move |level| levels.lock().unwrap().push(level),
            None,
            &state,
        );

        assert!(consumer.chunks.lock().unwrap().is_empty());
        assert!(state.last_callback_time.lock().is_none());
        assert_eq!(state.callback_count.load(Ordering::Relaxed), 0);
    }
}
