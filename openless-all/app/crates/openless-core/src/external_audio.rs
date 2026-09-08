//! Session-scoped external PCM input.
//!
//! Network transports and native hosts own authentication and framing. This
//! module only accepts the core's canonical 16 kHz / mono / signed 16-bit
//! little-endian PCM contract and routes it to the active pipeline session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;

use crate::dictation_context::{DictationAudioSource, DictationContext};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{ActiveRecording, AudioConsumer, AudioRecorder, RecordingProgressSink};
use crate::types::SessionId;

#[derive(Clone, Default)]
pub struct ExternalAudioRecorder {
    sessions: Arc<Mutex<HashMap<SessionId, Arc<ExternalRecordingSession>>>>,
}

struct ExternalRecordingSession {
    state: Mutex<ExternalRecordingState>,
}

struct ExternalRecordingState {
    active: bool,
    bytes_received: u64,
    consumer: Arc<dyn AudioConsumer>,
    progress: Arc<dyn RecordingProgressSink>,
}

struct ExternalActiveRecording {
    recorder: ExternalAudioRecorder,
    session_id: SessionId,
    session: Arc<ExternalRecordingSession>,
}

impl ExternalAudioRecorder {
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
        expected
            .state
            .lock()
            .expect("external audio state lock poisoned")
            .active = false;
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
        let session = Arc::new(ExternalRecordingSession {
            state: Mutex::new(ExternalRecordingState {
                active: true,
                bytes_received: 0,
                consumer,
                progress,
            }),
        });
        {
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
            sessions.insert(session_id, Arc::clone(&session));
        }
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
        state.consumer.consume_pcm_chunk(pcm);
        state.bytes_received = state.bytes_received.saturating_add(pcm.len() as u64);
        let elapsed_ms = state.bytes_received.saturating_mul(1_000) / 32_000;
        state
            .progress
            .publish_level(elapsed_ms, pcm_i16_le_rms(pcm))
    }
}

impl ActiveRecording for ExternalActiveRecording {
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
