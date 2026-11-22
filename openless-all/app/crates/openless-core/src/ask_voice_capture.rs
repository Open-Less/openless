use super::*;
use crate::ports::{NoopHostContextAdapter, RecordingEvent, RecordingProgressSink};
use crate::silence_auto_stop::{SilenceAutoStop, SilenceDecision};
use crate::voice_session::{VoiceResourceHold, VoiceSessionGate, VoiceSessionKind};
use std::time::{Duration, Instant};

pub struct Reservation {
    id: SessionId,
    gate: Arc<VoiceSessionGate>,
    cancelled: std::sync::atomic::AtomicBool,
}

impl Reservation {
    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.gate.release(self.id);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug, Clone)]
pub enum Progress {
    Level { elapsed_ms: u64, level: f32 },
    Stop,
    NoSpeech,
    Fatal(BackendError),
}

struct Observer {
    notify: Arc<dyn Fn(Progress) + Send + Sync>,
    started_at: Instant,
    silence: Mutex<Option<SilenceAutoStop>>,
}

impl RecordingProgressSink for Observer {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        let level = if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        };
        (self.notify)(Progress::Level { elapsed_ms, level });
        let decision = self
            .silence
            .lock()
            .expect("Ask silence lock poisoned")
            .as_mut()
            .and_then(|silence| {
                let now = self
                    .started_at
                    .checked_add(Duration::from_millis(elapsed_ms))?;
                silence.on_level(level, now)
            });
        match decision {
            Some(SilenceDecision::Stop) => (self.notify)(Progress::Stop),
            Some(SilenceDecision::Cancel) => (self.notify)(Progress::NoSpeech),
            None => {}
        }
        Ok(())
    }

    fn publish(&self, event: RecordingEvent) -> Result<(), BackendError> {
        match event {
            RecordingEvent::Level { elapsed_ms, level } => self.publish_level(elapsed_ms, level),
            RecordingEvent::LimitReached => {
                (self.notify)(Progress::Stop);
                Ok(())
            }
            RecordingEvent::Fatal(error) => {
                (self.notify)(Progress::Fatal(error));
                Ok(())
            }
        }
    }
}

enum Input {
    Asr(Arc<dyn TranscriptionSession>),
    Omni {
        pcm: Arc<CapturedPcm>,
        inputs: AuxiliaryOmniFinish,
    },
}

pub struct Capture {
    lifecycle: Arc<VoiceCaptureLifecycle>,
    input: Arc<Input>,
    recording: Mutex<Option<Box<dyn ActiveRecording>>>,
    spawner: Arc<dyn TaskSpawner>,
}

impl OpenLessBackend {
    pub fn reserve_ask_voice(&self, id: SessionId) -> Result<Reservation, BackendError> {
        let _runtime = self.runtime_start_work.acquire()?;
        let state = self.state.read().expect("backend state lock poisoned");
        ensure_running(&state)?;
        self.voice_sessions
            .acquire(id, VoiceSessionKind::Dictation)?;
        Ok(Reservation {
            id,
            gate: Arc::clone(&self.voice_sessions),
            cancelled: Default::default(),
        })
    }

    /// Only ASR/Omni transcription uses this context; no editor, chat, or model history is input.
    pub async fn start_ask_voice(
        &self,
        reservation: &Reservation,
        notify: Arc<dyn Fn(Progress) + Send + Sync>,
    ) -> Result<Capture, BackendError> {
        let resources = self.voice_sessions.hold_resources(reservation.id)?;
        let options = DictationStartOptions {
            insert_text: false,
            ..Default::default()
        };
        let context = self.capture_dictation_context_with_host(
            &options,
            DictationContextPurpose::AuxiliaryVoice,
            &NoopHostContextAdapter,
        );
        let mut context = tokio::select! {
            biased;
            _ = resources.cancel.cancelled() => return Err(VoiceCaptureLifecycle::cancelled_error()),
            result = context => result?,
        };
        context.recording.archive_enabled = false;
        context.recording.archive_raw = false;
        context.recording.archive_processed = false;
        context.polish.prior_turns.clear();
        let started_at = Instant::now();
        let progress: Arc<dyn RecordingProgressSink> = Arc::new(Observer {
            notify,
            started_at,
            silence: Mutex::new(
                context
                    .recording
                    .silence_after_ms
                    .map(|millis| SilenceAutoStop::new(Duration::from_millis(millis), started_at)),
            ),
        });
        let context = Arc::new(context);
        let (recording, input) =
            if context.pipeline_mode == crate::shared_types::PipelineMode::Multimodal {
                let capture = own_voice_start(
                    &self.deps.task_spawner,
                    Arc::clone(&resources),
                    self.deps.dictation_engine.start_audio_capture(
                        reservation.id,
                        Arc::clone(&context),
                        progress,
                        resources.cancel.clone(),
                    ),
                    |capture| {
                        Box::pin(async move { stop_and_discard_recording(capture.recording).await })
                    },
                )
                .await?;
                let inputs = AuxiliaryOmniFinish {
                    credentials: Arc::clone(&self.deps.credential_store),
                    context,
                    system_prompt: crate::prompts::auxiliary_voice_omni_instruction_prompt(),
                };
                (
                    capture.recording,
                    Input::Omni {
                        pcm: capture.pcm,
                        inputs,
                    },
                )
            } else {
                let capture = own_voice_start(
                    &self.deps.task_spawner,
                    Arc::clone(&resources),
                    self.deps.dictation_engine.start_voice_capture(
                        reservation.id,
                        context,
                        Arc::new(DiscardTextStream),
                        progress,
                        resources.cancel.clone(),
                    ),
                    discard_voice_capture,
                )
                .await?;
                (capture.recording, Input::Asr(capture.transcription))
            };
        Ok(Capture {
            lifecycle: Arc::new(VoiceCaptureLifecycle::with_resources(resources)),
            input: Arc::new(input),
            recording: Mutex::new(Some(recording)),
            spawner: Arc::clone(&self.deps.task_spawner),
        })
    }
}

impl Capture {
    fn take_recording(&self) -> Option<Box<dyn ActiveRecording>> {
        self.recording
            .lock()
            .expect("Ask recorder lock poisoned")
            .take()
    }

    pub fn finish(&self) -> futures_util::future::BoxFuture<'static, Result<String, BackendError>> {
        if let Err(error) = self.lifecycle.begin_finish() {
            return Box::pin(async move { Err(error) });
        }
        let recording = self.take_recording();
        let input = Arc::clone(&self.input);
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources = lifecycle.resources();
        let spawner = Arc::clone(&self.spawner);
        own_voice_effect(
            &self.spawner,
            Box::pin(async move {
                let result = async {
                if let Some(recording) = recording {
                    stop_and_discard_recording(recording).await?;
                }
                if lifecycle.is_cancelled() {
                    return Err(VoiceCaptureLifecycle::cancelled_error());
                }
                let recognize = async { match &*input {
                    Input::Asr(session) => Ok(session.finish().await?.text),
                    Input::Omni { pcm, inputs } => {
                        let wav = crate::audio::encode_dictation_wav(&pcm.snapshot())?;
                        crate::cloud_providers::complete_omni_instruction_from_wav(
                            inputs.credentials.as_ref(), &inputs.context, &wav, &inputs.system_prompt, spawner,
                        ).await
                    }
                } };
                if let Some(resources) = &resources {
                    tokio::select! {
                        biased;
                        _ = resources.cancel.cancelled() => Err(VoiceCaptureLifecycle::cancelled_error()),
                        result = recognize => result,
                    }
                } else {
                    Err(VoiceCaptureLifecycle::cancelled_error())
                }
            }.await;
                let (result, cleanup) = lifecycle.settle(result);
                if cleanup {
                    let _ = input.cancel().await;
                }
                lifecycle.release_resources();
                result
            }),
        )
    }

    pub fn cancel(&self) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        if !self.lifecycle.claim_cancel() {
            return Box::pin(async { Ok(()) });
        }
        let recording = self.take_recording();
        let input = Arc::clone(&self.input);
        let lifecycle = Arc::clone(&self.lifecycle);
        let resources: Option<Arc<VoiceResourceHold>> = lifecycle.resources();
        own_voice_effect(
            &self.spawner,
            Box::pin(async move {
                let _resources = resources;
                let (recorder, provider) = futures_util::future::join(
                    async move {
                        match recording {
                            Some(recording) => stop_and_discard_recording(recording).await,
                            None => Ok(()),
                        }
                    },
                    input.cancel(),
                )
                .await;
                lifecycle.release_resources();
                recorder.and(provider)
            }),
        )
    }
}

impl Input {
    async fn cancel(&self) -> Result<(), BackendError> {
        match self {
            Self::Asr(session) => session.cancel().await,
            Self::Omni { .. } => Ok(()),
        }
    }
}
