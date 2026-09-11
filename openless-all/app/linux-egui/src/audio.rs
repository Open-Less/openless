use std::sync::Arc;

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
        let recordings_dir = self.recordings_dir.clone();
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                tokio::task::spawn_blocking(move || {
                    start_linux_recording(
                        session_id,
                        preferred_device_name,
                        recordings_dir,
                        mute_during_recording,
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
    preferred_device_name: Option<String>,
    recordings_dir: Option<std::path::PathBuf>,
    mute_during_recording: bool,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
) -> Result<Box<dyn ActiveRecording>, BackendError> {
    use std::sync::atomic::AtomicBool;

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
                    log::warn!("failed to create Linux recording archive: {error}");
                    (None, None)
                }
            }
        }
        None => (None, None),
    };
    let thread = std::thread::Builder::new()
        .name("openless-linux-recorder".to_string())
        .spawn(move || {
            run_audio_thread(
                preferred_device_name,
                consumer,
                progress,
                writer,
                stop_for_thread,
                runtime_error_for_thread,
                startup_tx,
            );
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
fn run_audio_thread(
    preferred_device_name: Option<String>,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
    writer: Option<Arc<std::sync::Mutex<LinuxWavWriter>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    runtime_error: Arc<std::sync::Mutex<Option<BackendError>>>,
    startup: std::sync::mpsc::SyncSender<Result<(), BackendError>>,
) {
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
            &consumer,
            &progress,
            &writer,
            &stop,
            &runtime_error,
        ) {
            Ok(stream) => {
                result = Ok(stream);
                break;
            }
            Err(error) => {
                log::warn!("{backend} audio backend failed; trying next backend: {error}");
                result = Err(error);
            }
        }
    }

    let stream = match result {
        Ok(stream) => {
            let _ = startup.send(Ok(()));
            stream
        }
        Err(error) => {
            let _ = startup.send(Err(error));
            return;
        }
    };
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        std::thread::park_timeout(std::time::Duration::from_millis(25));
    }
    drop(stream);
}

#[cfg(target_os = "linux")]
fn audio_backend_order() -> [&'static str; 3] {
    // Native desktop servers are preferred because they handle device policy,
    // hot-plugging and format conversion. ALSA remains the universal fallback.
    ["pipewire", "pulseaudio", "alsa"]
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn try_start_audio_stream(
    host: &cpal::Host,
    backend: &str,
    preferred_device_name: Option<&str>,
    consumer: &Arc<dyn AudioConsumer>,
    progress: &Arc<dyn RecordingProgressSink>,
    writer: &Option<Arc<std::sync::Mutex<LinuxWavWriter>>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
    runtime_error: &Arc<std::sync::Mutex<Option<BackendError>>>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let device = select_input_device(host, preferred_device_name).map_err(|error| {
        BackendError::new(error.code, format!("{backend} backend: {}", error.message))
    })?;
    let supported = device
        .default_input_config()
        .map_err(|error| classify_audio_error("default input config", error.to_string()))?;
    let sample_format = supported.sample_format();
    let input_sample_rate = supported.sample_rate();
    let channels = usize::from(supported.channels());
    let config: cpal::StreamConfig = supported.into();
    let stream = build_input_stream(
        &device,
        &config,
        sample_format,
        input_sample_rate,
        channels,
        Arc::clone(consumer),
        Arc::clone(progress),
        writer.clone(),
        Arc::clone(stop),
        Arc::clone(runtime_error),
    )?;
    stream
        .play()
        .map_err(|error| classify_audio_error("start input stream", error.to_string()))?;
    Ok(stream)
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
        for device in devices {
            if device.to_string() == preferred {
                return Ok(device);
            }
        }
        log::warn!(
            "preferred Linux microphone was not found; using the default device: {preferred}"
        );
    }
    host.default_input_device().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::Platform,
            "no Linux microphone input device is available",
        )
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    input_sample_rate: u32,
    channels: usize,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
    writer: Option<Arc<std::sync::Mutex<LinuxWavWriter>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    runtime_error: Arc<std::sync::Mutex<Option<BackendError>>>,
) -> Result<cpal::Stream, BackendError> {
    use cpal::traits::DeviceTrait;

    macro_rules! make_stream {
        ($sample:ty, $to_f32:expr) => {{
            let consumer = Arc::clone(&consumer);
            let progress = Arc::clone(&progress);
            let stop_for_error = Arc::clone(&stop);
            let runtime_error = Arc::clone(&runtime_error);
            let writer = writer.clone();
            let started = std::time::Instant::now();
            let mut normalizer = openless_core::PcmNormalizer::default();
            device
                .build_input_stream::<$sample, _, _>(
                    *config,
                    move |data: &[$sample], _| {
                        let samples = data.iter().copied().map($to_f32).collect::<Vec<f32>>();
                        if let Some(chunk) =
                            normalizer.process(&samples, channels, input_sample_rate)
                        {
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
                    },
                    move |error| {
                        let error = classify_audio_error("input stream", error.to_string());
                        let mut slot = runtime_error
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
}
