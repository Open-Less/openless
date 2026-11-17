//! Session-scoped external PCM input.
//!
//! Network transports and native hosts own authentication and framing. This
//! module only accepts the core's canonical 16 kHz / mono / signed 16-bit
//! little-endian PCM contract and routes it to the active pipeline session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;

use crate::dictation_context::{DictationAudioSource, DictationContext};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    ActiveRecording, AudioConsumer, AudioRecorder, CaptureStartRequest, PendingAudioCapture,
    RecordingArchive, RecordingProgressSink,
};
use crate::types::SessionId;

mod archive;
use archive::ExternalRecordingArchive;

#[derive(Clone, Default)]
pub struct ExternalAudioRecorder {
    sessions: Arc<Mutex<HashMap<SessionId, Arc<ExternalRecordingSession>>>>,
    recordings_dir: Option<PathBuf>,
}

struct ExternalRecordingSession {
    state: Mutex<ExternalRecordingState>,
    /// Primary archive: processed PCM when requested, otherwise raw PCM.
    archive: Option<Arc<ExternalRecordingArchive>>,
    /// Optional sidecar raw archive when both forms are requested.
    raw_archive: Option<Arc<ExternalRecordingArchive>>,
}

struct ExternalRecordingState {
    active: bool,
    bytes_received: u64,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
    processor: Option<crate::AudioProcessor>,
    samples: Vec<f32>,
    archive_processed: bool,
    archive_raw: bool,
}

struct ExternalActiveRecording {
    recorder: ExternalAudioRecorder,
    session_id: SessionId,
    session: Arc<ExternalRecordingSession>,
}

impl ExternalAudioRecorder {
    /// Shares the WAV directory and retention policy with desktop history, so failed remote recordings can be re-transcribed.
    pub fn with_recordings_directory(directory: PathBuf) -> Self {
        Self {
            recordings_dir: Some(directory),
            ..Self::default()
        }
    }

    fn release(
        &self,
        session_id: SessionId,
        expected: &Arc<ExternalRecordingSession>,
    ) -> Result<(), BackendError> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("external audio session lock poisoned");
        let Some(current) = sessions.get(&session_id) else {
            return Ok(());
        };
        if !Arc::ptr_eq(current, expected) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "external audio session was replaced",
            ));
        }
        let mut state = expected
            .state
            .lock()
            .expect("external audio state lock poisoned");
        if let Some(chunk) = state
            .processor
            .as_mut()
            .and_then(crate::AudioProcessor::finish)
        {
            state.consumer.consume_pcm_chunk(&chunk.pcm_i16_le);
            if state.archive_processed {
                if let Some(archive) = &expected.archive {
                    archive.append(&chunk.pcm_i16_le);
                }
            }
        }
        state.active = false;
        drop(state);
        if let Some(archive) = &expected.archive {
            archive.finish();
        }
        if let Some(archive) = &expected.raw_archive {
            archive.finish();
        }
        sessions.remove(&session_id);
        Ok(())
    }
}

impl AudioRecorder for ExternalAudioRecorder {
    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        if context.audio_source != DictationAudioSource::External {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "external recorder requires an external audio session",
                ))
            });
        }
        // Check for duplicate sessions before archive creation/cleanup, so an
        // in-progress recording's files are never deleted by mistake.
        let mut sessions = self
            .sessions
            .lock()
            .expect("external audio session lock poisoned");
        if sessions.contains_key(&session_id) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "external audio session already exists",
                ))
            });
        }
        let archive_processed = context.recording.archive_processed || !context.recording.archive_raw;
        let archive_raw = context.recording.archive_raw;
        let wants_archive = context.recording.archive_enabled;
        let (archive, raw_archive) = self
            .recordings_dir
            .as_ref()
            .filter(|_| wants_archive)
            .map(|directory| {
                let primary = if archive_processed {
                    ExternalRecordingArchive::create(directory, session_id, &context.recording)
                } else {
                    ExternalRecordingArchive::create_raw(directory, session_id, &context.recording)
                };
                let archive = primary
                    .map(Arc::new)
                    .map_err(|error| log::warn!("[remote-input] 录音归档不可用：{error}"))
                    .ok();
                let raw_archive = (archive_processed && archive_raw)
                    .then(|| ExternalRecordingArchive::create_raw(directory, session_id, &context.recording))
                    .and_then(|result| {
                        result
                            .map(Arc::new)
                            .map_err(|error| log::warn!("[remote-input] 原始录音归档不可用：{error}"))
                            .ok()
                    });
                (archive, raw_archive)
            })
            .unwrap_or((None, None));
        if let (Some(archive), Some(raw_archive)) = (&archive, &raw_archive) {
            archive.add_cleanup_path(raw_archive.path());
        }
        let session = Arc::new(ExternalRecordingSession {
            state: Mutex::new(ExternalRecordingState {
                samples: Vec::new(),
                active: true,
                bytes_received: 0,
                consumer,
                progress,
                processor: context
                    .recording
                    .audio_processing
                    .enhancement_enabled
                    .then(|| crate::AudioProcessor::new(context.recording.audio_processing)),
                archive_processed,
                archive_raw,
            }),
            archive,
            raw_archive,
        });
        sessions.insert(session_id, Arc::clone(&session));
        drop(sessions);
        let recording = ExternalActiveRecording {
            recorder: self.clone(),
            session_id,
            session,
        };
        Box::pin(async move { Ok(Box::new(recording) as Box<dyn ActiveRecording>) })
    }

    fn feed_pcm(&self, session_id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "external PCM must contain complete signed 16-bit samples",
            ));
        }
        let session = self
            .sessions
            .lock()
            .expect("external audio session lock poisoned")
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "external audio session is not active",
                )
            })?;
        let mut state = session
            .state
            .lock()
            .expect("external audio state lock poisoned");
        if !state.active {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "external audio session is not active",
            ));
        }
        state.bytes_received = state.bytes_received.saturating_add(pcm.len() as u64);
        let elapsed_ms = state.bytes_received.saturating_mul(1_000) / 32_000;
        let archive_processed = state.archive_processed;
        let archive_raw = state.archive_raw;
        if archive_raw && archive_processed {
            if let Some(raw_archive) = &session.raw_archive {
                raw_archive.append(pcm);
            }
        }
        if let ExternalRecordingState {
            processor: Some(processor),
            samples,
            ..
        } = &mut *state
        {
            samples.clear();
            samples.extend(
                pcm.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0),
            );
            if archive_raw && !archive_processed {
                if let Some(archive) = &session.archive {
                    archive.append(pcm);
                }
            }
            if let Some(chunk) = processor.process(samples, 1, crate::audio::DICTATION_SAMPLE_RATE)
            {
                state.consumer.consume_pcm_chunk(&chunk.pcm_i16_le);
                if archive_processed {
                    if let Some(archive) = &session.archive {
                        archive.append(&chunk.pcm_i16_le);
                    }
                }
                return state.progress.publish_level(elapsed_ms, chunk.level);
            }
            return Ok(());
        }
        if let Some(archive) = &session.archive {
            archive.append(pcm);
        }
        state.consumer.consume_pcm_chunk(pcm);
        state
            .progress
            .publish_level(elapsed_ms, pcm_i16_le_rms(pcm))
    }
}

impl ActiveRecording for ExternalActiveRecording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.session
            .archive
            .as_ref()
            .map(|archive| Arc::clone(archive) as Arc<dyn RecordingArchive>)
    }

    fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move { self.recorder.release(self.session_id, &self.session) })
    }
}

pub struct AudioRecorderRouter {
    microphone: Arc<dyn AudioRecorder>,
    external: ExternalAudioRecorder,
}

impl AudioRecorderRouter {
    pub fn new(microphone: Arc<dyn AudioRecorder>, external: ExternalAudioRecorder) -> Self {
        Self {
            microphone,
            external,
        }
    }
}

impl AudioRecorder for AudioRecorderRouter {
    fn arm_capture(
        &self,
        request: CaptureStartRequest,
    ) -> Result<Box<dyn PendingAudioCapture>, BackendError> {
        self.microphone.arm_capture(request)
    }

    fn discard_capture(&self, press_id: u64) -> BoxFuture<'static, Result<(), BackendError>> {
        self.microphone.discard_capture(press_id)
    }

    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn AudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        match context.audio_source {
            DictationAudioSource::Microphone => self
                .microphone
                .start(session_id, context, consumer, progress),
            DictationAudioSource::External => {
                self.external.start(session_id, context, consumer, progress)
            }
        }
    }

    fn feed_pcm(&self, session_id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        self.external.feed_pcm(session_id, pcm)
    }
}

pub fn pcm_i16_le_rms(pcm: &[u8]) -> f32 {
    let sample_count = pcm.len() / 2;
    if sample_count == 0 {
        return 0.0;
    }
    let sum = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f64)
        .map(|sample| sample * sample)
        .sum::<f64>();
    ((sum / sample_count as f64).sqrt() / i16::MAX as f64).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn external_archive_flags_create_processed_and_raw_files() {
        let directory = std::env::temp_dir().join(format!(
            "openless-external-archive-flags-{}",
            uuid::Uuid::new_v4()
        ));
        let recorder = ExternalAudioRecorder::with_recordings_directory(directory.clone());
        let id = SessionId::new();
        let mut context = DictationContext {
            audio_source: DictationAudioSource::External,
            ..DictationContext::default()
        };
        context.recording.archive_enabled = true;
        context.recording.archive_processed = true;
        context.recording.archive_raw = true;
        let recording = recorder
            .start(
                id,
                Arc::new(context),
                Arc::new(RecordingConsumer::default()),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        let archive = recording.archive().unwrap();
        recorder.feed_pcm(id, &[1, 0, 2, 0]).unwrap();
        recording.stop().await.unwrap();
        assert!(directory.join(format!("{id}.wav")).is_file());
        assert!(directory.join(format!("{id}.raw.wav")).is_file());
        archive.discard().await.unwrap();
        assert!(!directory.join(format!("{id}.raw.wav")).exists());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn enhanced_external_input_flushes_the_same_complete_pcm_to_asr_and_archive() {
        let directory = std::env::temp_dir().join(format!(
            "openless-enhanced-external-{}",
            uuid::Uuid::new_v4()
        ));
        let recorder = ExternalAudioRecorder::with_recordings_directory(directory.clone());
        let id = SessionId::new();
        let mut context = DictationContext {
            audio_source: DictationAudioSource::External,
            ..DictationContext::default()
        };
        context.recording.archive_enabled = true;
        context.recording.audio_processing.enhancement_enabled = true;
        let consumer = Arc::new(RecordingConsumer::default());
        let recording = recorder
            .start(
                id,
                Arc::new(context),
                consumer.clone(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        let archive = recording.archive().unwrap();
        let input: Vec<u8> = (0..1777)
            .flat_map(|i| (((i as f32 * 0.13).sin() * 8000.0) as i16).to_le_bytes())
            .collect();
        for frame in input.chunks(74) {
            recorder.feed_pcm(id, frame).unwrap();
        }
        let before_stop = consumer.0.lock().unwrap().len();
        recording.stop().await.unwrap();
        let output = consumer.0.lock().unwrap().clone();
        assert!(
            before_stop < output.len(),
            "end-of-stream tail was not delivered"
        );
        assert_eq!(output.len(), input.len());
        assert_ne!(output, input);
        assert_eq!(archive.read_pcm().await.unwrap(), output);
        assert!(recorder.feed_pcm(id, &[0, 0]).is_err());
        archive.discard().await.unwrap();
        let _ = std::fs::remove_dir(directory);
    }

    #[derive(Default)]
    struct RecordingConsumer(Mutex<Vec<u8>>);

    impl AudioConsumer for RecordingConsumer {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.0.lock().unwrap().extend_from_slice(pcm);
        }
    }

    #[derive(Default)]
    struct RecordingProgress(Mutex<Vec<(u64, f32)>>);

    impl RecordingProgressSink for RecordingProgress {
        fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
            self.0.lock().unwrap().push((elapsed_ms, level));
            Ok(())
        }
    }

    struct CountingMicrophone(Arc<AtomicUsize>);

    impl AudioRecorder for CountingMicrophone {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _consumer: Arc<dyn AudioConsumer>,
            _progress: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture microphone should not start",
                ))
            })
        }
    }

    #[tokio::test]
    async fn remote_archive_keeps_two_minutes_of_pcm_and_survives_recording_stop() {
        let directory =
            std::env::temp_dir().join(format!("openless-remote-archive-{}", uuid::Uuid::new_v4()));
        let recorder = ExternalAudioRecorder::with_recordings_directory(directory.clone());
        let id = SessionId::new();
        let mut context = DictationContext {
            audio_source: DictationAudioSource::External,
            ..DictationContext::default()
        };
        context.recording.archive_enabled = true;
        let consumer = Arc::new(RecordingConsumer::default());
        let recording = recorder
            .start(
                id,
                Arc::new(context.clone()),
                consumer.clone(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        let archive = recording.archive().unwrap();
        assert!(!archive.is_available());
        for second in 0..120 {
            recorder.feed_pcm(id, &vec![second; 32_000]).unwrap();
        }
        let expected = consumer.0.lock().unwrap().clone();
        let path = directory.join(format!("{id}.wav"));
        let wav = std::fs::read(&path).unwrap();
        assert_eq!(&wav[44..], expected);
        assert_eq!(
            u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize,
            120 * 32_000
        );
        assert!(recorder
            .start(
                id,
                Arc::new(context),
                consumer,
                Arc::new(RecordingProgress::default())
            )
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), wav);
        recording.stop().await.unwrap();
        assert!(archive.is_available());
        assert_eq!(archive.read_pcm().await.unwrap(), expected);
        archive.discard().await.unwrap();
        assert!(!archive.is_available());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn disabled_or_unavailable_archive_does_not_block_remote_capture() {
        let directory = std::env::temp_dir().join(format!(
            "openless-remote-no-archive-{}",
            uuid::Uuid::new_v4()
        ));
        for enabled in [false, true] {
            if enabled {
                std::fs::write(&directory, b"not a directory").unwrap();
            }
            let recorder = ExternalAudioRecorder::with_recordings_directory(directory.clone());
            let id = SessionId::new();
            let mut context = DictationContext {
                audio_source: DictationAudioSource::External,
                ..DictationContext::default()
            };
            context.recording.archive_enabled = enabled;
            let consumer = Arc::new(RecordingConsumer::default());
            let recording = recorder
                .start(
                    id,
                    Arc::new(context),
                    consumer.clone(),
                    Arc::new(RecordingProgress::default()),
                )
                .await
                .unwrap();
            assert!(recording.archive().is_none());
            recorder.feed_pcm(id, &[1, 0, 2, 0]).unwrap();
            assert_eq!(*consumer.0.lock().unwrap(), vec![1, 0, 2, 0]);
            recording.stop().await.unwrap();
            if !enabled {
                assert!(!directory.exists());
            }
        }
        let _ = std::fs::remove_file(directory);
    }

    #[tokio::test]
    async fn remote_archive_applies_the_existing_count_limit_without_deleting_other_files() {
        let directory = std::env::temp_dir().join(format!(
            "openless-remote-retention-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("user.wav"), b"keep").unwrap();
        let recorder = ExternalAudioRecorder::with_recordings_directory(directory.clone());
        let mut context = DictationContext {
            audio_source: DictationAudioSource::External,
            ..DictationContext::default()
        };
        context.recording.archive_enabled = true;
        context.recording.max_entries = Some(2);
        for _ in 0..4 {
            let id = SessionId::new();
            let recording = recorder
                .start(
                    id,
                    Arc::new(context.clone()),
                    Arc::new(RecordingConsumer::default()),
                    Arc::new(RecordingProgress::default()),
                )
                .await
                .unwrap();
            recorder.feed_pcm(id, &[1, 0]).unwrap();
            recording.stop().await.unwrap();
        }
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 3);
        assert_eq!(std::fs::read(directory.join("user.wav")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn external_sessions_route_pcm_progress_and_reject_late_or_wrong_frames() {
        let microphone_starts = Arc::new(AtomicUsize::new(0));
        let recorder = AudioRecorderRouter::new(
            Arc::new(CountingMicrophone(Arc::clone(&microphone_starts))),
            ExternalAudioRecorder::default(),
        );
        let session_id = SessionId::new();
        let consumer = Arc::new(RecordingConsumer::default());
        let progress = Arc::new(RecordingProgress::default());
        let context = Arc::new(DictationContext {
            audio_source: DictationAudioSource::External,
            ..DictationContext::default()
        });
        let recording = recorder
            .start(
                session_id,
                context.clone(),
                consumer.clone(),
                progress.clone(),
            )
            .await
            .unwrap();

        let duplicate = recorder
            .start(session_id, context, consumer.clone(), progress.clone())
            .await
            .err()
            .expect("duplicate external session must be rejected");
        assert_eq!(duplicate.code, BackendErrorCode::Busy);
        assert_eq!(microphone_starts.load(Ordering::Acquire), 0);
        assert_eq!(
            recorder.feed_pcm(session_id, &[1]).unwrap_err().code,
            BackendErrorCode::InvalidArgument
        );
        assert_eq!(
            recorder
                .feed_pcm(SessionId::new(), &[1, 0])
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidState
        );

        recorder
            .feed_pcm(session_id, &[0xff, 0x7f, 0x00, 0x00])
            .unwrap();
        assert_eq!(&*consumer.0.lock().unwrap(), &[0xff, 0x7f, 0x00, 0x00]);
        {
            let levels = progress.0.lock().unwrap();
            assert_eq!(levels.len(), 1);
            assert_eq!(levels[0].0, 0);
            assert!(levels[0].1 > 0.7 && levels[0].1 <= 1.0);
        }

        recording.stop().await.unwrap();
        assert_eq!(
            recorder.feed_pcm(session_id, &[1, 0]).unwrap_err().code,
            BackendErrorCode::InvalidState
        );
    }
}
