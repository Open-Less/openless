use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;

use futures_util::future::BoxFuture;
use openless_core::{
    ActiveRecording, AudioConsumer, AudioRecorder, BackendError, BackendErrorCode,
    DictationContext, RecordingProgressSink, SessionId,
};

#[derive(Debug, Clone, Default)]
pub struct LinuxCpalRecorder {
    preferred_device_name: Option<String>,
    recordings_dir: Option<std::path::PathBuf>,
}

impl LinuxCpalRecorder {
    pub fn new(preferred_device_name: Option<String>) -> Self {
        Self {
            preferred_device_name,
            recordings_dir: None,
        }
    }

    pub fn with_recordings_dir(
        preferred_device_name: Option<String>,
        recordings_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            preferred_device_name,
            recordings_dir: Some(recordings_dir),
        }
    }
}

#[cfg(target_os = "linux")]
const AUDIO_PROCESSING_QUEUE_CAPACITY: usize = 64;

#[cfg(target_os = "linux")]
struct RawAudioFrame {
    captured_at: std::time::Instant,
    samples: Vec<f32>,
    channels: usize,
    input_sample_rate: u32,
}

#[cfg(target_os = "linux")]
struct LinuxRecordingOptions {
    preferred_device_name: Option<String>,
    recordings_dir: Option<std::path::PathBuf>,
    require_archive: bool,
    mute_during_recording: bool,
    audio_processing: openless_core::AudioProcessingConfig,
}

#[cfg(target_os = "linux")]
struct LinuxAudioThreadArgs {
    preferred_device_name: Option<String>,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
    writer: Option<Arc<std::sync::Mutex<LinuxWavWriter>>>,
    audio_processing: openless_core::AudioProcessingConfig,
    stop: Arc<std::sync::atomic::AtomicBool>,
    runtime_error: Arc<std::sync::Mutex<Option<BackendError>>>,
    startup: std::sync::mpsc::SyncSender<Result<(), BackendError>>,
}

#[cfg(target_os = "linux")]
struct AudioProcessingWorker {
    join_handle: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl AudioProcessingWorker {
    fn start(
        processing: openless_core::AudioProcessingConfig,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
        writer: Option<Arc<std::sync::Mutex<LinuxWavWriter>>>,
        reference_slot: Arc<std::sync::Mutex<Option<Arc<openless_core::ReferenceAudioBuffer>>>>,
    ) -> Result<(Self, SyncSender<RawAudioFrame>), BackendError> {
        let (sender, receiver) = sync_channel::<RawAudioFrame>(AUDIO_PROCESSING_QUEUE_CAPACITY);
        let started = std::time::Instant::now();
        let join_handle = std::thread::Builder::new()
            .name("openless-linux-audio-processor".to_string())
            .spawn(move || {
                let mut processor = openless_core::AudioProcessor::new(processing);
                while let Ok(frame) = receiver.recv() {
                    let reference = reference_slot
                        .lock()
                        .expect("Linux echo reference lock poisoned")
                        .clone();
                    let reference_samples = reference.as_ref().and_then(|buffer| {
                        buffer.take_for_input_at(
                            frame.samples.len() / frame.channels.max(1),
                            frame.input_sample_rate,
                            frame.captured_at,
                        )
                    });
                    if let Some(chunk) = processor.process_with_reference(
                        &frame.samples,
                        frame.channels,
                        frame.input_sample_rate,
                        reference_samples.as_deref(),
                    ) {
                        consumer.consume_pcm_chunk(&chunk.pcm_i16_le);
                        if let Some(writer) = &writer {
                            if let Err(error) = writer
                                .lock()
                                .expect("Linux WAV writer lock poisoned")
                                .append(&chunk.pcm_i16_le)
                            {
                                log::warn!("Linux recording archive write failed: {error}");
                            }
                        }
                        let _ = progress
                            .publish_level(started.elapsed().as_millis() as u64, chunk.level);
                    }
                }
                if let Some(chunk) = processor.finish() {
                    consumer.consume_pcm_chunk(&chunk.pcm_i16_le);
                    if let Some(writer) = &writer {
                        if let Err(error) = writer
                            .lock()
                            .expect("Linux WAV writer lock poisoned")
                            .append(&chunk.pcm_i16_le)
                        {
                            log::warn!("Linux recording archive write failed: {error}");
                        }
                    }
                    let _ =
                        progress.publish_level(started.elapsed().as_millis() as u64, chunk.level);
                }
            })
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to spawn Linux audio processor: {error}"),
                )
            })?;
        Ok((
            Self {
                join_handle: Some(join_handle),
            },
            sender,
        ))
    }

    fn finish(mut self) -> Result<(), BackendError> {
        if let Some(handle) = self.join_handle.take() {
            handle.join().map_err(|_| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    "Linux audio processor thread panicked while stopping",
                )
            })?;
        }
        Ok(())
    }
}

impl AudioRecorder for LinuxCpalRecorder {
    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        let preferred_device_name = context
            .recording
            .microphone_device_name
            .clone()
            .or_else(|| self.preferred_device_name.clone());
        // Platform effect, applied by the host recorder exactly like the Tauri
        // audio adapter. Never owned by Core; restore is the guard's Drop.
        let mute_during_recording = context.recording.mute_during_recording;
        let audio_processing = context.recording.audio_processing;
        let recordings_dir = self.recordings_dir.clone();
        let require_archive =
            context.output_target == openless_core::DictationOutputTarget::QuickNote;
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                tokio::task::spawn_blocking(move || {
                    start_linux_recording(
                        session_id,
                        LinuxRecordingOptions {
                            preferred_device_name,
                            recordings_dir,
                            require_archive,
                            mute_during_recording,
                            audio_processing,
                        },
                        consumer,
                        progress,
                    )
                })
                .await
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Internal,
                        format!("Linux recorder startup task failed: {error}"),
                    )
                })?
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (session_id, preferred_device_name, consumer, progress);
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Linux cpal recorder is unavailable on this target",
                ))
            }
        })
    }
}

#[cfg(target_os = "linux")]
struct LinuxActiveRecording {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    runtime_error: Arc<std::sync::Mutex<Option<BackendError>>>,
    archive: Option<Arc<LinuxRecordingArchive>>,
    /// Holds the output-mute guard while capture is live. Its `Drop` restores
    /// the sink on every terminal path (stop/cancel/error/drop/shutdown).
    mute: Option<crate::audio_mute::AudioMuteGuard>,
}

impl Drop for LinuxActiveRecording {
    fn drop(&mut self) {
        // Taking the guard here forces the field to be consumed (and therefore
        // restored) even if `stop()` is never reached — e.g. the handle is
        // dropped directly on an early error or during Core shutdown before it
        // had a chance to call stop. Double restore is harmless because Drop
        // of an already-taken guard is a no-op.
        self.mute.take();
    }
}

#[cfg(target_os = "linux")]
struct LinuxRecordingArchive {
    path: std::path::PathBuf,
    available: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(target_os = "linux")]
impl openless_core::RecordingArchive for LinuxRecordingArchive {
    fn is_available(&self) -> bool {
        self.available.load(std::sync::atomic::Ordering::Acquire)
    }

    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let wav = tokio::fs::read(path).await.map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("read Linux recording archive: {error}"),
                )
            })?;
            canonical_wav_pcm(&wav).map(ToOwned::to_owned)
        })
    }

    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let path = self.path.clone();
        let available = Arc::clone(&self.available);
        Box::pin(async move {
            match tokio::fs::remove_file(path).await {
                Ok(()) => available.store(false, std::sync::atomic::Ordering::Release),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    available.store(false, std::sync::atomic::Ordering::Release);
                }
                Err(error) => {
                    return Err(BackendError::new(
                        BackendErrorCode::Persistence,
                        format!("discard Linux recording archive: {error}"),
                    ));
                }
            }
            Ok(())
        })
    }
}

#[cfg(target_os = "linux")]
impl ActiveRecording for LinuxActiveRecording {
    fn archive(&self) -> Option<Arc<dyn openless_core::RecordingArchive>> {
        self.archive
            .as_ref()
            .map(|archive| Arc::clone(archive) as Arc<dyn openless_core::RecordingArchive>)
    }

    fn stop(mut self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            let thread = self.thread.take();
            let runtime_error = Arc::clone(&self.runtime_error);
            tokio::task::spawn_blocking(move || {
                if let Some(thread) = thread {
                    thread.join().map_err(|_| {
                        BackendError::new(
                            BackendErrorCode::Platform,
                            "Linux recorder thread panicked while stopping",
                        )
                    })?;
                }
                runtime_error
                    .lock()
                    .expect("Linux recorder error lock poisoned")
                    .take()
                    .map_or(Ok(()), Err)
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    format!("Linux recorder stop task failed: {error}"),
                )
            })?
        })
    }
}

#[cfg(target_os = "linux")]
fn start_linux_recording(
    session_id: SessionId,
    options: LinuxRecordingOptions,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
) -> Result<Box<dyn ActiveRecording>, BackendError> {
    use std::sync::atomic::AtomicBool;

    let LinuxRecordingOptions {
        preferred_device_name,
        recordings_dir,
        require_archive,
        mute_during_recording,
        audio_processing,
    } = options;

    // Mute is best-effort and independent of capture availability (mirrors the
    // Tauri reference): if it fails we log and continue recording. If capture
    // later fails on this path, the guard drops here and restores the sink.
    let mute = if mute_during_recording {
        match crate::audio_mute::AudioMuteGuard::activate() {
            Ok(guard) => Some(guard),
            Err(error) => {
                log::warn!("[audio-mute] failed to mute output; capture continues: {error}");
                None
            }
        }
    } else {
        None
    };
    let stop = Arc::new(AtomicBool::new(false));
    let runtime_error = Arc::new(std::sync::Mutex::new(None));
    let (startup_tx, startup_rx) = std::sync::mpsc::sync_channel(1);
    let stop_for_thread = Arc::clone(&stop);
    let runtime_error_for_thread = Arc::clone(&runtime_error);
    let (writer, archive) = match recordings_dir {
        Some(directory) => {
            match LinuxWavWriter::create(directory.join(format!("{session_id}.wav"))) {
                Ok((writer, archive)) => {
                    (Some(Arc::new(std::sync::Mutex::new(writer))), Some(archive))
                }
                Err(error) => {
                    if require_archive {
                        return Err(BackendError::new(
                            BackendErrorCode::Persistence,
                            format!("quick note recording archive is required: {error}"),
                        ));
                    }
                    log::warn!("failed to create Linux recording archive: {error}");
                    (None, None)
                }
            }
        }
        None if require_archive => {
            return Err(BackendError::new(
                BackendErrorCode::Persistence,
                "quick note recording archive directory is missing",
            ))
        }
        None => (None, None),
    };
    let thread = std::thread::Builder::new()
        .name("openless-linux-recorder".to_string())
        .spawn(move || {
            run_audio_thread(LinuxAudioThreadArgs {
                preferred_device_name,
                consumer,
                progress,
                writer,
                audio_processing,
                stop: stop_for_thread,
                runtime_error: runtime_error_for_thread,
                startup: startup_tx,
            });
        })
        .map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to spawn Linux recorder thread: {error}"),
            )
        })?;

    match startup_rx.recv() {
        Ok(Ok(())) => Ok(Box::new(LinuxActiveRecording {
            stop,
            thread: Some(thread),
            runtime_error,
            archive,
            mute,
        })),
        Ok(Err(error)) => {
            let _ = thread.join();
            // `mute` is dropped on the error path, restoring the sink.
            Err(error)
        }
        Err(error) => {
            let _ = thread.join();
            // `mute` is dropped on the error path, restoring the sink.
            Err(BackendError::new(
                BackendErrorCode::Platform,
                format!("Linux recorder thread exited during startup: {error}"),
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn run_audio_thread(args: LinuxAudioThreadArgs) {
    let LinuxAudioThreadArgs {
        preferred_device_name,
        consumer,
        progress,
        writer,
        audio_processing,
        stop,
        runtime_error,
        startup,
    } = args;
    let reference_slot = Arc::new(std::sync::Mutex::new(None));
    let (processor, frame_tx) = match AudioProcessingWorker::start(
        audio_processing,
        consumer,
        progress,
        writer,
        Arc::clone(&reference_slot),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = startup.send(Err(error));
            return;
        }
    };
    let mut result = Err(BackendError::new(
        BackendErrorCode::Platform,
        "no Linux audio backend is available",
    ));
    for backend in audio_backend_order() {
        let Some(host_id) = cpal::available_hosts()
            .into_iter()
            .find(|id| id.name().eq_ignore_ascii_case(backend))
        else {
            continue;
        };
        let host = match cpal::host_from_id(host_id) {
            Ok(host) => host,
            Err(error) => {
                result = Err(classify_audio_error(
                    &format!("initialize {backend} backend"),
                    error.to_string(),
                ));
                log::warn!("{backend} audio backend unavailable: {error}");
                continue;
            }
        };
        match try_start_audio_stream(
            &host,
            backend,
            preferred_device_name.as_deref(),
            &frame_tx,
            &stop,
            &runtime_error,
            &reference_slot,
            audio_processing.echo_cancellation_enabled,
        ) {
            Ok(stream) => {
                result = Ok(stream);
                break;
            }
            Err(error) if error.code == BackendErrorCode::PermissionDenied => {
                let _ = startup.send(Err(error));
                return;
            }
            Err(error) => {
                log::warn!("{backend} audio backend failed; trying next backend: {error}");
                result = Err(error);
            }
        }
    }

    let streams = match result {
        Ok(streams) => {
            let _ = startup.send(Ok(()));
            streams
        }
        Err(error) => {
            log::error!(
                "[audio] no input backend could open a stream (tried {:?}): {}",
                audio_backend_order(),
                error.message
            );
            drop(frame_tx);
            let _ = processor.finish();
            let _ = startup.send(Err(error));
            return;
        }
    };
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        std::thread::park_timeout(std::time::Duration::from_millis(25));
    }
    drop(streams.microphone);
    reference_slot
        .lock()
        .expect("Linux echo reference lock poisoned")
        .take();
    drop(streams.reference);
    drop(frame_tx);
    let _ = processor.finish();
}

#[cfg(target_os = "linux")]
fn audio_backend_order() -> [&'static str; 3] {
    // Native desktop servers are preferred because they handle device policy,
    // hot-plugging and format conversion. ALSA remains the universal fallback.
    ["pipewire", "pulseaudio", "alsa"]
}

#[cfg(target_os = "linux")]
struct LinuxAudioStreams {
    microphone: cpal::Stream,
    reference: Option<cpal::Stream>,
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn try_start_audio_stream(
    host: &cpal::Host,
    backend: &str,
    preferred_device_name: Option<&str>,
    frame_tx: &SyncSender<RawAudioFrame>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
    runtime_error: &Arc<std::sync::Mutex<Option<BackendError>>>,
    reference_slot: &Arc<std::sync::Mutex<Option<Arc<openless_core::ReferenceAudioBuffer>>>>,
    echo_cancellation_enabled: bool,
) -> Result<LinuxAudioStreams, BackendError> {
    let device = select_input_device(host, preferred_device_name).map_err(|error| {
        BackendError::new(error.code, format!("{backend} backend: {}", error.message))
    })?;
    let stream = try_start_microphone_stream(&device, backend, frame_tx, stop, runtime_error)?;
    let reference_stream = if echo_cancellation_enabled && backend != "alsa" {
        match start_reference_stream(host, backend, reference_slot, stop) {
            Ok(stream) => Some(stream),
            Err(error) => {
                log::warn!(
                    "{backend} playback monitor unavailable; recording without AEC: {error}"
                );
                if backend == "pipewire" {
                    start_pulse_reference_fallback(reference_slot, stop)
                } else {
                    None
                }
            }
        }
    } else {
        None
    };
    Ok(LinuxAudioStreams {
        microphone: stream,
        reference: reference_stream,
    })
}

#[cfg(target_os = "linux")]
fn select_playback_monitor_device(
    host: &cpal::Host,
    backend: &str,
) -> Result<cpal::Device, BackendError> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let default_output_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.id().to_owned());
    let devices = host
        .devices()
        .map_err(|error| classify_audio_error("enumerate playback monitors", error.to_string()))?;
    for device in devices {
        if !device.supports_input() {
            continue;
        }
        let Ok(id) = device.id() else {
            continue;
        };
        if is_default_playback_reference(backend, id.id(), default_output_id.as_deref()) {
            return Ok(device);
        }
    }
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        format!("{backend} monitor of the default playback output was not found"),
    ))
}

#[cfg(any(target_os = "linux", test))]
fn is_default_playback_reference(backend: &str, id: &str, default_output: Option<&str>) -> bool {
    match backend {
        "pulseaudio" => {
            default_output.is_some_and(|sink| id.strip_suffix(".monitor") == Some(sink))
        }
        "pipewire" => id == "sink_default" || default_output == Some(id),
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn start_pulse_reference_fallback(
    reference_slot: &Arc<std::sync::Mutex<Option<Arc<openless_core::ReferenceAudioBuffer>>>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
) -> Option<cpal::Stream> {
    let host_id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name().eq_ignore_ascii_case("pulseaudio"))?;
    let host = match cpal::host_from_id(host_id) {
        Ok(host) => host,
        Err(error) => {
            log::debug!("PulseAudio reference fallback host unavailable: {error}");
            return None;
        }
    };
    match start_reference_stream(&host, "pulseaudio", reference_slot, stop) {
        Ok(stream) => {
            log::info!("using PulseAudio playback monitor as PipeWire AEC fallback");
            Some(stream)
        }
        Err(error) => {
            log::debug!("PulseAudio playback monitor fallback unavailable: {error}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn start_reference_stream(
    host: &cpal::Host,
    backend: &str,
    reference_slot: &Arc<std::sync::Mutex<Option<Arc<openless_core::ReferenceAudioBuffer>>>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let device = select_playback_monitor_device(host, backend)?;
    let supported = device.default_input_config().map_err(|error| {
        classify_audio_error("default playback monitor config", error.to_string())
    })?;
    let config: cpal::StreamConfig = supported.config();
    let sample_rate = config.sample_rate;
    let channels = usize::from(config.channels);
    let reference = Arc::new(openless_core::ReferenceAudioBuffer::new(sample_rate));
    let stream = build_linux_reference_stream(
        &device,
        &config,
        supported.sample_format(),
        Arc::clone(&reference),
        channels,
        Arc::clone(stop),
    )?;
    stream
        .play()
        .map_err(|error| classify_audio_error("start playback monitor", error.to_string()))?;
    *reference_slot
        .lock()
        .expect("Linux echo reference lock poisoned") = Some(reference);
    Ok(stream)
}

#[cfg(target_os = "linux")]
fn try_start_microphone_stream(
    device: &cpal::Device,
    backend: &str,
    frame_tx: &SyncSender<RawAudioFrame>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
    runtime_error: &Arc<std::sync::Mutex<Option<BackendError>>>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let mut native = None;
    let mut start = |supported: cpal::SupportedStreamConfig| -> Result<cpal::Stream, BackendError> {
        let accepting = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let config = supported.config();
        let stream = build_input_stream(
            device,
            &config,
            supported.sample_format(),
            supported.sample_rate(),
            usize::from(supported.channels()),
            frame_tx.clone(),
            Arc::clone(stop),
            Arc::clone(runtime_error),
            Arc::clone(&accepting),
        )?;
        stream
            .play()
            .map_err(|error| classify_audio_error("start input stream", error.to_string()))?;
        accepting.store(true, std::sync::atomic::Ordering::Release);
        // 「录到的全是静音」这类问题，第一个要回答的就是：落到哪个后端、哪只
        // 设备、什么采样格式。以前只有失败时才有日志，成功时什么都看不到。
        log::info!(
            "[audio] capture started backend={backend} device={} format={:?} rate={}Hz channels={}",
            device,
            supported.sample_format(),
            supported.sample_rate(),
            usize::from(supported.channels()),
        );
        Ok(stream)
    };
    let initial_error = match device.default_input_config() {
        Ok(config) => {
            native = Some(config);
            match start(config) {
                Ok(stream) => return Ok(stream),
                Err(error) if error.code == BackendErrorCode::PermissionDenied => {
                    return Err(error)
                }
                Err(error) => error,
            }
        }
        Err(error) => classify_audio_error("default input config", error.to_string()),
    };
    if initial_error.code == BackendErrorCode::PermissionDenied {
        return Err(initial_error);
    }
    let ranges = match device.supported_input_configs() {
        Ok(ranges) => ranges,
        Err(_) => return Err(initial_error),
    };
    try_input_candidates(
        native_input_alternatives(native.as_ref(), ranges),
        initial_error,
        &mut start,
    )
}

#[cfg(target_os = "linux")]
fn try_input_candidates<T, U>(
    candidates: impl IntoIterator<Item = T>,
    mut last_error: BackendError,
    mut start: impl FnMut(T) -> Result<U, BackendError>,
) -> Result<U, BackendError> {
    for candidate in candidates {
        match start(candidate) {
            Ok(stream) => return Ok(stream),
            Err(error) if error.code == BackendErrorCode::PermissionDenied => return Err(error),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(target_os = "linux")]
fn native_input_alternatives(
    default: Option<&cpal::SupportedStreamConfig>,
    ranges: impl IntoIterator<Item = cpal::SupportedStreamConfigRange>,
) -> Vec<cpal::SupportedStreamConfig> {
    let native_rate = default.map(|config| config.sample_rate());
    let mut candidates = Vec::new();
    for range in ranges {
        let min = range.min_sample_rate();
        let max = range.max_sample_rate();
        for rate in [native_rate.unwrap_or(max).clamp(min, max), min, max] {
            let candidate = range.with_sample_rate(rate);
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

#[cfg(target_os = "linux")]
fn build_linux_reference_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    reference: Arc<openless_core::ReferenceAudioBuffer>,
    channels: usize,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::DeviceTrait;
    macro_rules! make_stream {
        ($t:ty, $to_f32:expr) => {{
            let reference = Arc::clone(&reference);
            let error_reference = Arc::clone(&reference);
            let stop_for_callback = Arc::clone(&stop);
            device
                .build_input_stream(
                    *config,
                    move |data: &[$t], info| {
                        if stop_for_callback.load(std::sync::atomic::Ordering::Acquire) {
                            return;
                        }
                        let captured_at = capture_time(info);
                        let samples = data.iter().copied().map($to_f32).collect::<Vec<f32>>();
                        reference.push_interleaved_at(&samples, channels, captured_at);
                    },
                    move |error| {
                        error_reference.invalidate();
                        log::warn!("[linux-audio] playback monitor stream error: {error}");
                    },
                    None,
                )
                .map_err(|error| classify_audio_error("build playback monitor", error.to_string()))
        }};
    }
    match sample_format {
        cpal::SampleFormat::F32 => make_stream!(f32, |s: f32| s),
        cpal::SampleFormat::F64 => make_stream!(f64, |s: f64| s as f32),
        cpal::SampleFormat::I64 => make_stream!(i64, |s: i64| (s as f64 / i64::MAX as f64) as f32),
        cpal::SampleFormat::U64 => make_stream!(u64, |s: u64| ((s as f64 / u64::MAX as f64) * 2.0
            - 1.0) as f32),
        cpal::SampleFormat::U32 => make_stream!(u32, |s: u32| (s as f64 / u32::MAX as f64 * 2.0
            - 1.0) as f32),
        cpal::SampleFormat::I16 => make_stream!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::U16 => make_stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        cpal::SampleFormat::I32 => make_stream!(i32, |s: i32| s as f32 / i32::MAX as f32),
        cpal::SampleFormat::I8 => make_stream!(i8, |s: i8| s as f32 / i8::MAX as f32),
        cpal::SampleFormat::U8 => make_stream!(u8, |s: u8| (s as f32 - 128.0) / 128.0),
        other => Err(BackendError::new(
            BackendErrorCode::Unsupported,
            format!("unsupported playback monitor sample format: {other:?}"),
        )),
    }
}
#[cfg(target_os = "linux")]
fn select_input_device(
    host: &cpal::Host,
    preferred_device_name: Option<&str>,
) -> Result<cpal::Device, BackendError> {
    use cpal::traits::HostTrait;

    if let Some(preferred) = preferred_device_name.filter(|name| !name.trim().is_empty()) {
        let devices = host
            .input_devices()
            .map_err(|error| classify_audio_error("enumerate input devices", error.to_string()))?;
        let mut available: Vec<String> = Vec::new();
        for device in devices {
            let name = device.to_string();
            if name == preferred {
                return Ok(device);
            }
            available.push(name);
        }
        // 设备名是精确匹配：把候选项一并打出来，省得再猜。
        log::warn!(
            "preferred Linux microphone was not found; using the default device: {preferred}; available: {available:?}"
        );
    }
    let device = host.default_input_device().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::Platform,
            "no Linux microphone input device is available",
        )
    })?;
    log::info!("[audio] using input device: {}", device);
    Ok(device)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    input_sample_rate: u32,
    channels: usize,
    frame_tx: SyncSender<RawAudioFrame>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    runtime_error: Arc<std::sync::Mutex<Option<BackendError>>>,
    accepting: Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::DeviceTrait;

    macro_rules! make_stream {
        ($sample:ty, $to_f32:expr) => {{
            let frame_tx = frame_tx.clone();
            let stop_for_error = Arc::clone(&stop);
            let stop_for_callback = Arc::clone(&stop);
            let runtime_error_for_callback = Arc::clone(&runtime_error);
            let runtime_error_for_error = Arc::clone(&runtime_error);
            let accepting_audio = Arc::clone(&accepting);
            let accepting_error = Arc::clone(&accepting);
            device
                .build_input_stream::<$sample, _, _>(
                    *config,
                    move |data: &[$sample], info| {
                        if !accepting_audio.load(std::sync::atomic::Ordering::Acquire)
                            || stop_for_callback.load(std::sync::atomic::Ordering::Acquire)
                        {
                            return;
                        }
                        let captured_at = capture_time(info);
                        let samples = data.iter().copied().map($to_f32).collect::<Vec<f32>>();
                        match frame_tx.try_send(RawAudioFrame {
                            captured_at,
                            samples,
                            channels,
                            input_sample_rate,
                        }) {
                            Ok(()) => {}
                            Err(TrySendError::Disconnected(_)) => {
                                stop_for_callback.store(true, std::sync::atomic::Ordering::Release);
                            }
                            Err(TrySendError::Full(_)) => {
                                stop_for_callback.store(true, std::sync::atomic::Ordering::Release);
                                let mut slot = runtime_error_for_callback
                                    .lock()
                                    .expect("Linux recorder error lock poisoned");
                                if slot.is_none() {
                                    *slot = Some(BackendError::new(
                                        BackendErrorCode::Platform,
                                        "Linux audio processing queue overflow",
                                    ));
                                }
                            }
                        }
                    },
                    move |error| {
                        if !accepting_error.load(std::sync::atomic::Ordering::Acquire) {
                            return;
                        }
                        let error = classify_audio_error("input stream", error.to_string());
                        let mut slot = runtime_error_for_error
                            .lock()
                            .expect("Linux recorder error lock poisoned");
                        if slot.is_none() {
                            *slot = Some(error);
                        }
                        stop_for_error.store(true, std::sync::atomic::Ordering::Release);
                    },
                    None,
                )
                .map_err(|error| classify_audio_error("build input stream", error.to_string()))
        }};
    }

    match sample_format {
        cpal::SampleFormat::F32 => make_stream!(f32, |sample: f32| sample),
        cpal::SampleFormat::F64 => make_stream!(f64, |sample: f64| sample as f32),
        cpal::SampleFormat::I64 => {
            make_stream!(i64, |sample: i64| (sample as f64 / i64::MAX as f64) as f32)
        }
        cpal::SampleFormat::U64 => make_stream!(
            u64,
            |sample: u64| (sample as f64 / u64::MAX as f64 * 2.0 - 1.0) as f32
        ),
        cpal::SampleFormat::U32 => make_stream!(
            u32,
            |sample: u32| (sample as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
        ),
        cpal::SampleFormat::I16 => {
            make_stream!(i16, |sample: i16| sample as f32 / i16::MAX as f32)
        }
        cpal::SampleFormat::U16 => {
            make_stream!(u16, |sample: u16| { (sample as f32 - 32768.0) / 32768.0 })
        }
        cpal::SampleFormat::I32 => {
            make_stream!(i32, |sample: i32| sample as f32 / i32::MAX as f32)
        }
        cpal::SampleFormat::I8 => {
            make_stream!(i8, |sample: i8| sample as f32 / i8::MAX as f32)
        }
        cpal::SampleFormat::U8 => {
            make_stream!(u8, |sample: u8| (sample as f32 - 128.0) / 128.0)
        }
        other => Err(BackendError::new(
            BackendErrorCode::Unsupported,
            format!("unsupported Linux microphone sample format: {other:?}"),
        )),
    }
}

#[cfg(target_os = "linux")]
struct LinuxWavWriter {
    file: std::fs::File,
    path: std::path::PathBuf,
    bytes_written: u32,
    available: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(target_os = "linux")]
impl LinuxWavWriter {
    fn create(path: std::path::PathBuf) -> std::io::Result<(Self, Arc<LinuxRecordingArchive>)> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&wav_header(0))?;
        let available = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let archive = Arc::new(LinuxRecordingArchive {
            path: path.clone(),
            available: Arc::clone(&available),
        });
        Ok((
            Self {
                file,
                path,
                bytes_written: 0,
                available,
            },
            archive,
        ))
    }

    fn append(&mut self, pcm: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        self.file.write_all(pcm)?;
        self.bytes_written = self
            .bytes_written
            .saturating_add(pcm.len().min(u32::MAX as usize) as u32);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxWavWriter {
    fn drop(&mut self) {
        use std::io::{Seek as _, SeekFrom, Write as _};
        let result = self
            .file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&wav_header(self.bytes_written)))
            .and_then(|_| self.file.sync_all());
        if let Err(error) = result {
            self.available
                .store(false, std::sync::atomic::Ordering::Release);
            let _ = std::fs::remove_file(&self.path);
            log::warn!("failed to finalize Linux recording archive: {error}");
        }
    }
}

pub(crate) fn wav_header(data_size: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&data_size.saturating_add(36).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&1u16.to_le_bytes());
    header[24..28].copy_from_slice(&16_000u32.to_le_bytes());
    header[28..32].copy_from_slice(&32_000u32.to_le_bytes());
    header[32..34].copy_from_slice(&2u16.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_size.to_le_bytes());
    header
}

fn canonical_wav_pcm(wav: &[u8]) -> Result<&[u8], BackendError> {
    if wav.len() <= 44
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || &wav[36..40] != b"data"
        || !(wav.len() - 44).is_multiple_of(2)
    {
        return Err(BackendError::new(
            BackendErrorCode::Persistence,
            "Linux recording archive is not canonical 16 kHz mono PCM WAV",
        ));
    }
    Ok(&wav[44..])
}

#[cfg(any(target_os = "linux", test))]
fn classify_audio_error(context: &str, message: String) -> BackendError {
    let lower = message.to_ascii_lowercase();
    let code =
        if lower.contains("permission") || lower.contains("denied") || lower.contains("authoriz") {
            BackendErrorCode::PermissionDenied
        } else {
            BackendErrorCode::Platform
        };
    BackendError::new(code, format!("Linux audio {context} failed: {message}"))
}

#[cfg(target_os = "linux")]
fn capture_time(info: &cpal::InputCallbackInfo) -> std::time::Instant {
    let now = std::time::Instant::now();
    let timestamp = info.timestamp();
    let delay = timestamp.callback.duration_since(timestamp.capture);
    now.checked_sub(delay).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_errors_keep_permission_and_platform_failures_distinct() {
        assert_eq!(
            classify_audio_error("start", "Permission denied".to_string()).code,
            BackendErrorCode::PermissionDenied
        );
        assert_eq!(
            classify_audio_error("start", "device disappeared".to_string()).code,
            BackendErrorCode::Platform
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_alternatives_preserve_channels_format_and_advertised_rates() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            cpal::SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );
        let native = range.with_sample_rate(48_000);
        let alternatives = native_input_alternatives(Some(&native), [range, range]);
        assert_eq!(alternatives.len(), 1);
        assert_eq!(alternatives[0].channels(), 2);
        assert_eq!(alternatives[0].sample_format(), cpal::SampleFormat::F32);
        assert_eq!(alternatives[0].sample_rate(), 44_100);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn candidates_stop_at_success_or_permission_failure() {
        let mut attempts = Vec::new();
        let result = try_input_candidates(
            [1, 2, 3],
            classify_audio_error("start", "failed".into()),
            |candidate| {
                attempts.push(candidate);
                if candidate == 2 {
                    Ok(candidate)
                } else {
                    Err(classify_audio_error("start", "failed".into()))
                }
            },
        );
        assert_eq!(result.unwrap(), 2);
        assert_eq!(attempts, [1, 2]);
        let mut attempts = 0;
        let result: Result<(), _> = try_input_candidates(
            [1, 2],
            classify_audio_error("start", "failed".into()),
            |_| {
                attempts += 1;
                Err(classify_audio_error("start", "permission denied".into()))
            },
        );
        assert_eq!(result.unwrap_err().code, BackendErrorCode::PermissionDenied);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn wav_archive_header_and_pcm_round_trip() {
        let pcm = [1u8, 0, 2, 0];
        let mut wav = wav_header(pcm.len() as u32).to_vec();
        wav.extend_from_slice(&pcm);
        assert_eq!(canonical_wav_pcm(&wav).unwrap(), pcm);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn audio_backends_are_ordered_from_desktop_server_to_universal_fallback() {
        assert_eq!(audio_backend_order(), ["pipewire", "pulseaudio", "alsa"]);
    }

    #[test]
    fn monitor_selection_uses_route_ids_not_translated_display_names() {
        let sink = "alsa_output.usb-interface.analog-stereo";
        assert!(is_default_playback_reference(
            "pulseaudio",
            &format!("{sink}.monitor"),
            Some(sink)
        ));
        assert!(!is_default_playback_reference(
            "pulseaudio",
            "another_output.monitor",
            Some(sink)
        ));
        assert!(!is_default_playback_reference(
            "pulseaudio",
            sink,
            Some(sink)
        ));
        assert!(!is_default_playback_reference(
            "pulseaudio",
            "any.monitor",
            None
        ));
        assert!(is_default_playback_reference(
            "pipewire",
            "sink_default",
            None
        ));
        assert!(is_default_playback_reference("pipewire", sink, Some(sink)));
        assert!(!is_default_playback_reference("alsa", sink, Some(sink)));
    }
}
