//! Shared dictation pipeline orchestration.
//!
//! The pipeline owns provider/recorder ordering, terminal progress events and
//! cancellation guards. Native hosts only implement the narrow ports from
//! [`crate::ports`]; they never duplicate session or fallback decisions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use futures_util::future::{BoxFuture, Shared};
use futures_util::FutureExt;

use crate::dictation_context::{DictationContext, DictationOutputTarget};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    ActiveRecording, AudioCapture, AudioConsumer, AudioRecorder, CaptureStartRequest, CapturedPcm,
    DictationEngine, EngineFailure, EngineFailureStage, EngineProgress, EngineProgressSink,
    EngineResult, EngineStage, PendingAudioCapture, PreparedTranscription, RecordingArchive,
    RecordingProgressSink, TextPolisher, TextStreamChunk, TextStreamSink, TranscriptionEngine,
    TranscriptionSession, VoiceCapture,
};
use crate::types::{PolishDelta, SessionId, TranscriptDelta};

// Keep one MiB for PCM callbacks that arrive while the host handles the stop request.
const MAX_BUFFERED_TRANSCRIPTION_PCM_BYTES: usize = 128 * 1024 * 1024;
const BUFFERED_TRANSCRIPTION_STOP_HEADROOM_BYTES: usize = 1024 * 1024;
const BUFFERED_TRANSCRIPTION_STOP_THRESHOLD_BYTES: usize =
    MAX_BUFFERED_TRANSCRIPTION_PCM_BYTES - BUFFERED_TRANSCRIPTION_STOP_HEADROOM_BYTES;
const BUFFERED_TRANSCRIPTION_FORWARD_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishFailurePolicy {
    Fail,
    UseRawText,
}

pub struct PipelineDictationEngine {
    recorder: Arc<dyn AudioRecorder>,
    transcription: Arc<dyn TranscriptionEngine>,
    polisher: Arc<dyn TextPolisher>,
    polish_failure_policy: PolishFailurePolicy,
    local_startup_spawner: Option<Arc<dyn crate::TaskSpawner>>,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<PipelineSession>>>>,
    pending_captures: Arc<Mutex<HashMap<u64, Box<dyn PendingAudioCapture>>>>,
}

/// Reuses the existing PCM buffer while capture starts ahead of provider
/// preparation. Preparation still completes before start() reports success,
/// including stable mode; concurrent finish/attach shares the same result.
struct CaptureFirstPreparation {
    engine: Arc<dyn TranscriptionEngine>,
    session_id: SessionId,
    context: Arc<DictationContext>,
    result: Arc<tokio::sync::OnceCell<Result<Arc<dyn PreparedTranscription>, BackendError>>>,
}

impl CaptureFirstPreparation {
    fn prepare(&self) -> BoxFuture<'static, Result<Arc<dyn PreparedTranscription>, BackendError>> {
        let engine = Arc::clone(&self.engine);
        let session_id = self.session_id;
        let context = Arc::clone(&self.context);
        let result = Arc::clone(&self.result);
        Box::pin(async move {
            result
                .get_or_init(|| engine.prepare(session_id, context))
                .await
                .clone()
        })
    }
}

impl PreparedTranscription for CaptureFirstPreparation {
    fn start(
        &self,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let ready = self.prepare();
        Box::pin(async move { ready.await?.start(partials).await })
    }
}

struct PipelineSession {
    context: RwLock<Arc<DictationContext>>,
    cancelled: AtomicBool,
    finishing: AtomicBool,
    transcription_finished: AtomicBool,
    transcription_cancelled: AtomicBool,
    polishing: AtomicBool,
    polisher_cancelled: AtomicBool,
    retain_failed_recording: AtomicBool,
    resources: Mutex<PipelineResources>,
}

type RecordingStop = Shared<BoxFuture<'static, Result<(), BackendError>>>;

// Early stop, finalization and cancellation must join the same native close.
struct PipelineRecording {
    recording: Option<Box<dyn ActiveRecording>>,
    archive: Option<Arc<dyn RecordingArchive>>,
    stopped: Option<RecordingStop>,
}

impl PipelineRecording {
    fn new(recording: Box<dyn ActiveRecording>) -> Self {
        Self {
            archive: recording.archive(),
            recording: Some(recording),
            stopped: None,
        }
    }

    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.archive.clone()
    }

    fn stop_recording(&mut self) -> RecordingStop {
        self.stopped
            .get_or_insert_with(|| {
                self.recording
                    .take()
                    .expect("pipeline recording is available until stop begins")
                    .stop()
                    .shared()
            })
            .clone()
    }

    fn stop(mut self) -> RecordingStop {
        self.stop_recording()
    }
}

#[derive(Default)]
struct PipelineResources {
    recording: Option<PipelineRecording>,
    stop_recording_requested: bool,
    transcription: Option<Arc<dyn TranscriptionSession>>,
    buffered: Option<Arc<BufferedTranscriptionSession>>,
    prepared: Option<Arc<dyn PreparedTranscription>>,
}

impl PipelineSession {
    fn new(context: Arc<DictationContext>) -> Self {
        Self {
            context: RwLock::new(context),
            cancelled: AtomicBool::new(false),
            finishing: AtomicBool::new(false),
            transcription_finished: AtomicBool::new(false),
            transcription_cancelled: AtomicBool::new(false),
            polishing: AtomicBool::new(false),
            polisher_cancelled: AtomicBool::new(false),
            retain_failed_recording: AtomicBool::new(false),
            resources: Mutex::new(PipelineResources::default()),
        }
    }

    fn context(&self) -> Arc<DictationContext> {
        Arc::clone(&self.context.read().expect("pipeline context lock poisoned"))
    }

    fn update_context(&self, context: Arc<DictationContext>) -> Result<(), BackendError> {
        if self.finishing.load(Ordering::Acquire) || self.cancelled.load(Ordering::Acquire) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "dictation pipeline context can only change before finalization",
            ));
        }
        *self
            .context
            .write()
            .expect("pipeline context lock poisoned") = context;
        Ok(())
    }
}

impl PipelineDictationEngine {
    pub fn new(
        recorder: Arc<dyn AudioRecorder>,
        transcription: Arc<dyn TranscriptionEngine>,
        polisher: Arc<dyn TextPolisher>,
    ) -> Self {
        Self {
            recorder,
            transcription,
            polisher,
            polish_failure_policy: PolishFailurePolicy::UseRawText,
            local_startup_spawner: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_captures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Once capture is running, local model loading must not hold the public
    /// session in Starting: Stop must release the microphone before inference is ready.
    /// Cloud routes and stable (record-first) mode keep their existing behavior.
    pub fn with_local_startup_spawner(mut self, spawner: Arc<dyn crate::TaskSpawner>) -> Self {
        self.local_startup_spawner = Some(spawner);
        self
    }

    pub fn with_polish_failure_policy(mut self, policy: PolishFailurePolicy) -> Self {
        self.polish_failure_policy = policy;
        self
    }
}

impl DictationEngine for PipelineDictationEngine {
    fn arm_audio_capture(
        &self,
        request: CaptureStartRequest,
    ) -> Result<Box<dyn PendingAudioCapture>, BackendError> {
        self.recorder.arm_capture(request)
    }

    fn start_with_audio_capture(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
        capture: Box<dyn PendingAudioCapture>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let generation = context.recording.capture_generation;
        let external_audio =
            context.audio_source == crate::dictation_context::DictationAudioSource::External;
        let pending_captures = Arc::clone(&self.pending_captures);
        let start = self.start(session_id, context, progress);
        Box::pin(async move {
            if external_audio {
                let _ = capture.discard().await;
                return start.await;
            }
            let Some(generation) = generation else {
                let _ = capture.discard().await;
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "provisional capture is missing its physical generation",
                ));
            };
            let replaced = pending_captures
                .lock()
                .expect("pending capture lock poisoned")
                .insert(generation, capture);
            if let Some(replaced) = replaced {
                let _ = replaced.discard().await;
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "a provisional capture already owns this physical generation",
                ));
            }
            let result = start.await;
            if result.is_err() {
                let _ = discard_pending_capture(&pending_captures, generation).await;
            }
            result
        })
    }

    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let recorder = Arc::clone(&self.recorder);
        let transcription_engine = Arc::clone(&self.transcription);
        let sessions = Arc::clone(&self.sessions);
        let pending_captures = Arc::clone(&self.pending_captures);
        let local_startup_spawner = self.local_startup_spawner.clone().filter(|_| {
            crate::experimental_asr::ExperimentalAsrModel::from_provider(&context.asr.provider_type)
                .is_some()
        });
        Box::pin(async move {
            let session = Arc::new(PipelineSession::new(Arc::clone(&context)));
            {
                let mut active = sessions.lock().expect("pipeline session lock poisoned");
                match active.entry(session_id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Arc::clone(&session));
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {
                        return Err(BackendError::new(
                            BackendErrorCode::Busy,
                            "dictation pipeline session already exists",
                        ));
                    }
                }
            }

            let preparation = Arc::new(CaptureFirstPreparation {
                engine: transcription_engine,
                session_id,
                context: Arc::clone(&context),
                result: Arc::new(tokio::sync::OnceCell::new()),
            });
            let prepared: Arc<dyn PreparedTranscription> = preparation.clone();
            if session.cancelled.load(Ordering::Acquire) {
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error(
                    "dictation cancelled while preparing the provider",
                ));
            }
            let stable = context.recording.transcribe_after_stop;
            let has_local_fallback = context.asr_fallback.is_some();
            let transcript_partials: Arc<dyn TextStreamSink> = if stable {
                Arc::new(DiscardTextStream)
            } else {
                Arc::new(TranscriptProgressForwarder {
                    session_id,
                    progress: Arc::clone(&progress),
                })
            };
            let recording_progress: Arc<dyn RecordingProgressSink> =
                Arc::new(RecordingProgressForwarder {
                    session_id,
                    progress: Arc::clone(&progress),
                });
            let buffered = Arc::new(BufferedTranscriptionSession::new(
                prepared,
                transcript_partials,
                Arc::clone(&recording_progress),
            ));
            let registered = {
                let mut resources = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned");
                if session.cancelled.load(Ordering::Acquire) {
                    false
                } else {
                    let transcription: Arc<dyn TranscriptionSession> = buffered.clone();
                    resources.transcription = Some(transcription);
                    resources.buffered = Some(Arc::clone(&buffered));
                    resources.prepared = Some(buffered.prepared());
                    true
                }
            };
            if !registered {
                let _ = buffered.cancel().await;
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error(
                    "dictation cancelled before the recorder started",
                ));
            }

            let audio_consumer: Arc<dyn AudioConsumer> = buffered.clone();
            let pending = context.recording.capture_generation.and_then(|generation| {
                pending_captures
                    .lock()
                    .expect("pending capture lock poisoned")
                    .remove(&generation)
            });
            let pending = if context.audio_source
                == crate::dictation_context::DictationAudioSource::External
            {
                if let Some(capture) = pending {
                    let _ = capture.discard().await;
                }
                None
            } else {
                pending
            };
            let recording = match pending {
                Some(capture) => {
                    capture
                        .commit(
                            session_id,
                            Arc::clone(&context),
                            Arc::clone(&audio_consumer),
                            Arc::clone(&recording_progress),
                        )
                        .await
                }
                None => {
                    recorder
                        .start(session_id, context, audio_consumer, recording_progress)
                        .await
                }
            };
            let recording = match recording {
                Ok(recording) => recording,
                Err(error) => {
                    let _ = cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                    remove_session(&sessions, session_id, &session);
                    return Err(error);
                }
            };

            let mut recording = Some(PipelineRecording::new(recording));
            let stopping = {
                let mut resources = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned");
                if !session.cancelled.load(Ordering::Acquire) {
                    resources.recording = recording.take();
                }
                if resources.stop_recording_requested {
                    resources
                        .recording
                        .as_mut()
                        .map(PipelineRecording::stop_recording)
                } else {
                    None
                }
            };
            if let Some(recording) = recording {
                let stop_result = recording.stop().await;
                let cancel_result =
                    cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                remove_session(&sessions, session_id, &session);
                stop_result?;
                cancel_result?;
                return Err(cancelled_error(
                    "dictation cancelled while the recorder was starting",
                ));
            }
            // No provider preparation, configuration I/O or network work may
            // delay opening/promoting the microphone. The existing bounded
            // transcription buffer retains the captured prefix during prepare.
            let preparation_result = async {
                progress.publish(session_id, EngineProgress::CaptureReady)?;
                if let Some(stopping) = stopping {
                    // finish() retains the archive and reports any native stop failure.
                    let _ = stopping.await;
                }
                preparation.prepare().await
            }
            .await;
            if preparation_result.is_err() || session.cancelled.load(Ordering::Acquire) {
                let recording = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned")
                    .recording
                    .take();
                if let Some(recording) = recording {
                    let _ = recording.stop().await;
                }
                let _ = cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                remove_session(&sessions, session_id, &session);
                return Err(if session.cancelled.load(Ordering::Acquire) {
                    cancelled_error("dictation cancelled while preparing the provider")
                } else {
                    preparation_result.err().expect("preparation failed")
                });
            }
            if let Some(spawner) = local_startup_spawner.filter(|_| !stable) {
                let attaching = buffered.attach();
                let progress = Arc::clone(&buffered.inner.progress);
                let session = Arc::clone(&session);
                spawner.spawn(Box::pin(async move {
                    if let Err(error) = attaching.await {
                        if !session.cancelled.load(Ordering::Acquire)
                            && !session.finishing.load(Ordering::Acquire)
                        {
                            let _ = progress.publish(crate::ports::RecordingEvent::Fatal(error));
                        }
                    }
                }));
            } else if !stable {
                if let Err(error) = buffered.attach().await {
                    // Keep capturing for the local replay; the buffer retains the cloud error.
                    if has_local_fallback && !session.cancelled.load(Ordering::Acquire) {
                        log::warn!(
                            "[dictation] cloud ASR startup failed; keeping local fallback: {error}"
                        );
                    } else {
                        let recording = session
                            .resources
                            .lock()
                            .expect("pipeline resource lock poisoned")
                            .recording
                            .take();
                        if let Some(recording) = recording {
                            let _ = recording.stop().await;
                        }
                        let _ = cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                        remove_session(&sessions, session_id, &session);
                        return Err(error);
                    }
                }
            }
            if session.cancelled.load(Ordering::Acquire) {
                let recording = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned")
                    .recording
                    .take();
                if let Some(recording) = recording {
                    let _ = recording.stop().await;
                }
                let _ = cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error("dictation cancelled while starting"));
            }
            Ok(())
        })
    }

    fn stop_recording(&self, session_id: SessionId) -> BoxFuture<'static, Result<bool, BackendError>> {
        let session = self
            .sessions
            .lock()
            .expect("pipeline session lock poisoned")
            .get(&session_id)
            .cloned();
        let Some(session) = session else {
            return Box::pin(async { Ok(false) });
        };
        let stopping = {
            let mut resources = session
                .resources
                .lock()
                .expect("pipeline resource lock poisoned");
            resources.stop_recording_requested = true;
            resources
                .recording
                .as_mut()
                .map(PipelineRecording::stop_recording)
        };
        Box::pin(async move {
            if let Some(stopping) = stopping {
                stopping.await?;
            }
            Ok(true)
        })
    }

    fn start_transcription(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        self.transcription.start(session_id, context, partials)
    }

    fn prepare_transcription(
        self: Arc<Self>,
        session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn PreparedTranscription>, BackendError>> {
        Arc::clone(&self.transcription).prepare(session_id, context)
    }

    fn start_voice_capture(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
        progress: Arc<dyn RecordingProgressSink>,
        cancel: crate::CancellationToken,
    ) -> BoxFuture<'static, Result<VoiceCapture, BackendError>> {
        let recorder = Arc::clone(&self.recorder);
        let transcription_engine = Arc::clone(&self.transcription);
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(cancelled_error(
                    "voice capture cancelled before recorder startup",
                ));
            }
            let prepared = transcription_engine
                .prepare(session_id, Arc::clone(&context))
                .await?;
            if cancel.is_cancelled() {
                return Err(cancelled_error(
                    "voice capture cancelled while preparing the provider",
                ));
            }
            let stable = context.recording.transcribe_after_stop;
            let partials = if stable {
                Arc::new(DiscardTextStream) as Arc<dyn TextStreamSink>
            } else {
                partials
            };
            let buffered = Arc::new(BufferedTranscriptionSession::new(
                prepared,
                partials,
                Arc::clone(&progress),
            ));
            let consumer: Arc<dyn AudioConsumer> = buffered.clone();
            let recording = recorder
                .start(session_id, context, consumer, progress)
                .await?;
            let transcription: Arc<dyn TranscriptionSession> = buffered.clone();
            if cancel.is_cancelled() {
                let (stop_result, cancel_result) =
                    futures_util::future::join(recording.stop(), transcription.cancel()).await;
                stop_result?;
                cancel_result?;
                return Err(cancelled_error(
                    "voice capture cancelled while recorder was starting",
                ));
            }
            if !stable {
                let attaching = buffered.attach();
                tokio::pin!(attaching);
                tokio::select! {
                    result = &mut attaching => {
                        if let Err(error) = result {
                            let (stop_result, _) = futures_util::future::join(
                                recording.stop(),
                                transcription.cancel(),
                            ).await;
                            let _ = stop_result;
                            return Err(error);
                        }
                    }
                    _ = cancel.cancelled() => {
                        let (stop_result, cancel_result, _) = tokio::join!(
                            recording.stop(),
                            transcription.cancel(),
                            &mut attaching,
                        );
                        stop_result?;
                        cancel_result?;
                        return Err(cancelled_error(
                            "voice capture cancelled while ASR was starting",
                        ));
                    }
                }
            }
            if cancel.is_cancelled() {
                let (stop_result, cancel_result) =
                    futures_util::future::join(recording.stop(), transcription.cancel()).await;
                stop_result?;
                cancel_result?;
                return Err(cancelled_error("voice capture cancelled while starting"));
            }
            Ok(VoiceCapture {
                recording,
                transcription,
            })
        })
    }

    fn start_audio_capture(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn RecordingProgressSink>,
        cancel: crate::CancellationToken,
    ) -> BoxFuture<'static, Result<AudioCapture, BackendError>> {
        let recorder = Arc::clone(&self.recorder);
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(cancelled_error(
                    "voice capture cancelled before recorder startup",
                ));
            }
            let pcm = Arc::new(CapturedPcm::default());
            let consumer: Arc<dyn AudioConsumer> = pcm.clone();
            let recording = recorder
                .start(session_id, context, consumer, progress)
                .await?;
            if cancel.is_cancelled() {
                let _ = recording.stop().await;
                return Err(cancelled_error(
                    "voice capture cancelled while recorder was starting",
                ));
            }
            Ok(AudioCapture { recording, pcm })
        })
    }

    fn finish(
        &self,
        session_id: SessionId,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
        let sessions = Arc::clone(&self.sessions);
        let polisher = Arc::clone(&self.polisher);
        let transcription_engine = Arc::clone(&self.transcription);
        let policy = self.polish_failure_policy;
        Box::pin(async move {
            let session = find_session(&sessions, session_id)?;
            if session.finishing.swap(true, Ordering::AcqRel) {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "dictation pipeline is already finishing",
                )
                .into());
            }
            if session.cancelled.load(Ordering::Acquire) {
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error("dictation was cancelled before finishing").into());
            }
            let context = session.context();

            let (recording, transcription, prepared) = {
                let mut resources = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned");
                (
                    resources.recording.take(),
                    resources.transcription.clone(),
                    resources.prepared.clone(),
                )
            };
            let recording = recording.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "dictation recorder has not started",
                )
            })?;
            let transcription = transcription.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "transcription session has not started",
                )
            })?;

            let archive = recording.archive();
            let mut has_audio_recording = archive.as_ref().map(|archive| archive.is_available());
            if let Err(error) = recording.stop().await {
                demote_failed_archive(
                    archive.as_ref(),
                    context.output_target,
                    &mut has_audio_recording,
                )
                .await;
                let _ = cancel_transcription_once(&session, transcription).await;
                remove_session(&sessions, session_id, &session);
                let mut failure = EngineFailure::new(error, EngineFailureStage::Transcribing);
                failure.has_audio_recording = has_audio_recording;
                return Err(failure);
            }
            if context.output_target == crate::dictation_context::DictationOutputTarget::QuickNote {
                if let Some(archive) = archive.as_ref() {
                    if let Err(error) = archive.promote_to_quick_note().await {
                        log::error!(
                            "[quick-note] failed to move the archive into permanent storage: {error}"
                        );
                    }
                }
            } else if context.recording.archive_successful_recording {
                if let Some(archive) = archive.as_ref() {
                    if let Err(error) = archive.demote_to_ordinary_recording().await {
                        log::warn!(
                            "[recording] failed to move retained debug archive to ordinary storage: {error}"
                        );
                    }
                }
            }
            if session.cancelled.load(Ordering::Acquire) {
                let _ = cancel_transcription_once(&session, transcription).await;
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error(
                    "dictation was cancelled before transcription finished",
                )
                .into());
            }

            publish_progress(
                &session,
                session_id,
                &progress,
                EngineProgress::Stage(EngineStage::Transcribing),
            )?;
            let asr_started = std::time::Instant::now();
            let mut asr_call_label = transcription.asr_call_label();
            // With a local fallback configured, the local model stands by on
            // the same recording in case the cloud is slow or fails.
            let standby = context
                .asr_fallback
                .is_some()
                .then(|| archive.clone().filter(|archive| archive.is_available()))
                .flatten()
                .map(|archive| {
                    LocalStandby::new(
                        Arc::clone(&transcription_engine),
                        Arc::clone(&context),
                        Arc::clone(&session),
                        session_id,
                        archive,
                    )
                });
            let race = match standby {
                Some(standby) => {
                    race_cloud_with_local_standby(transcription.finish(), standby).await
                }
                None => AsrRace::Cloud {
                    result: transcription.finish().await,
                    standby: None,
                },
            };
            asr_call_label = transcription.asr_call_label().or(asr_call_label);
            for notification in transcription.take_progress_notifications() {
                publish_progress(
                    &session,
                    session_id,
                    &progress,
                    EngineProgress::Notification(notification),
                )?;
            }
            // Set when the cloud ASR failed or was too slow and the local
            // fallback produced the transcript: the network is most likely down
            // or weak, so polish is skipped.
            let mut local_fallback_used = false;
            // Set when the local standby simply answered first: the cloud may be
            // healthy, so polish still runs.
            let mut local_won_race = false;
            let mut transcript = match race {
                AsrRace::Local((transcript, label)) => {
                    log::info!(
                        "[dictation] local {} answered before cloud ASR {}; using the local transcript",
                        context.asr_fallback.as_ref().map_or("", |fallback| fallback.provider_type.as_str()),
                        context.asr.provider_type,
                    );
                    let _ = cancel_transcription_once(&session, Arc::clone(&transcription)).await;
                    asr_call_label = label;
                    local_won_race = true;
                    transcript
                }
                AsrRace::Cloud {
                    result: Ok(transcript),
                    standby,
                } => {
                    session
                        .transcription_finished
                        .store(true, Ordering::Release);
                    if transcript.text.trim().is_empty() {
                        if let Some(standby) = standby {
                            if let Some((local, label)) = standby.result().await {
                                log::warn!(
                                    "[dictation] cloud ASR returned an empty transcript; using the local fallback"
                                );
                                asr_call_label = label;
                                local_fallback_used = true;
                                local
                            } else {
                                transcript
                            }
                        } else {
                            transcript
                        }
                    } else {
                        if let Some(standby) = standby {
                            standby.abandon().await;
                        }
                        transcript
                    }
                }
                AsrRace::Cloud {
                    result: Err(first_error),
                    standby,
                } => {
                    let cancelled = session.cancelled.load(Ordering::Acquire);
                    let prepared = prepared.ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Internal,
                            "transcription provider preparation is unavailable",
                        )
                    })?;
                    // The recording is replayed for a cloud retry (retryable
                    // errors only).
                    let retry_pcm = if !cancelled && first_error.retryable {
                        match archive.as_ref().filter(|archive| archive.is_available()) {
                            Some(archive) => {
                                archive.read_pcm().await.ok().filter(|pcm| !pcm.is_empty())
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
                    let _ = cancel_transcription_once(&session, Arc::clone(&transcription)).await;
                    // Local first, on any cloud error — an expired key or
                    // exhausted quota is as much a reason to go local as a dead
                    // network, and on a weak or absent network the cloud retry
                    // would only add its backoff and timeouts before failing.
                    let local = match standby {
                        Some(standby) if !cancelled => {
                            log::warn!(
                                "[dictation] cloud ASR {} failed ({:?}: {}); using the local fallback",
                                context.asr.provider_type,
                                first_error.code,
                                first_error.message,
                            );
                            standby.result().await
                        }
                        Some(standby) => {
                            standby.abandon().await;
                            None
                        }
                        None => None,
                    };
                    if let Some((transcript, label)) = local {
                        asr_call_label = label;
                        local_fallback_used = true;
                        transcript
                    } else {
                        match retry_pcm {
                            Some(pcm) => match retry_transcription(
                                prepared,
                                Arc::clone(&session),
                                session_id,
                                Arc::clone(&progress),
                                pcm,
                                CLOUD_RETRY,
                            )
                            .await
                            {
                                Ok((transcript, label)) => {
                                    asr_call_label = label;
                                    transcript
                                }
                                Err((error, label)) => {
                                    asr_call_label = label.or(asr_call_label);
                                    demote_failed_archive(
                                        archive.as_ref(),
                                        context.output_target,
                                        &mut has_audio_recording,
                                    )
                                    .await;
                                    remove_session(&sessions, session_id, &session);
                                    let mut failure =
                                        EngineFailure::new(error, EngineFailureStage::Transcribing);
                                    failure.asr_ms = Some(asr_started.elapsed().as_millis() as u64);
                                    failure.has_audio_recording = has_audio_recording;
                                    failure.asr_call_label = asr_call_label;
                                    return Err(failure);
                                }
                            },
                            None => {
                                demote_failed_archive(
                                    archive.as_ref(),
                                    context.output_target,
                                    &mut has_audio_recording,
                                )
                                .await;
                                remove_session(&sessions, session_id, &session);
                                let error = if cancelled {
                                    cancelled_error(
                                        "dictation was cancelled while transcription was finishing",
                                    )
                                } else {
                                    first_error
                                };
                                let mut failure =
                                    EngineFailure::new(error, EngineFailureStage::Transcribing);
                                failure.asr_ms = Some(asr_started.elapsed().as_millis() as u64);
                                failure.has_audio_recording = has_audio_recording;
                                failure.asr_call_label = asr_call_label;
                                return Err(failure);
                            }
                        }
                    }
                }
            };
            let asr_ms = Some(asr_started.elapsed().as_millis() as u64);
            if session.cancelled.load(Ordering::Acquire) {
                if !context.recording.archive_successful_recording {
                    discard_ephemeral_archive(archive.as_ref(), &mut has_audio_recording).await;
                }
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error(
                    "dictation was cancelled after transcription finished",
                )
                .into());
            }
            if let Some(commands) = context.voice_activation.as_ref() {
                transcript.text = commands.clean(&transcript.text);
            }
            let original_asr_text = transcript.text.clone();
            transcript.text = crate::correction::apply_correction_rules(
                &transcript.text,
                &context.correction_rules,
            );
            log::info!(
                "[asr-diagnostics] session_id={} asr_chars={} asr_hash={} asr_near_duplicates={} corrected_chars={} corrected_hash={} corrected_near_duplicates={}",
                session_id,
                original_asr_text.chars().count(),
                crate::output_cleaning::privacy_text_hash(&original_asr_text),
                crate::output_cleaning::find_adjacent_near_duplicates(&original_asr_text).len(),
                transcript.text.chars().count(),
                crate::output_cleaning::privacy_text_hash(&transcript.text),
                crate::output_cleaning::find_adjacent_near_duplicates(&transcript.text).len(),
            );
            let asr_transcript =
                (transcript.text != original_asr_text).then_some(original_asr_text);
            if !context.recording.archive_successful_recording {
                if transcript.text.trim().is_empty() {
                    demote_failed_archive(
                        archive.as_ref(),
                        context.output_target,
                        &mut has_audio_recording,
                    )
                    .await;
                } else {
                    discard_ephemeral_archive(archive.as_ref(), &mut has_audio_recording).await;
                }
            }
            publish_progress(
                &session,
                session_id,
                &progress,
                EngineProgress::TranscriptDelta(TranscriptDelta {
                    text: transcript.text.clone(),
                    offset: 0,
                    is_final: true,
                }),
            )?;
            // Issue #1032: short transcripts skip polish (user-configured threshold).
            let skips_polish_for_short_text = context.skips_polish_for_short_text(&transcript.text);
            // Optional polish may be skipped after fallback; an explicit translation is still required.
            let translation_active = context.polish.translation_active;
            let uses_polisher = context.uses_llm_polisher()
                && !(context.voice_activation.is_some() && transcript.text.trim().is_empty())
                && !skips_polish_for_short_text
                && (!local_fallback_used || translation_active);
            log::info!(
                "[polish-diagnostics] session_id={} stage=decision uses_polisher={} short_text_skipped={} composition_input={} translation_active={} mode={:?}",
                session_id,
                uses_polisher,
                skips_polish_for_short_text,
                context.insertion.composition_input,
                translation_active,
                context.polish.mode,
            );
            // Local recognition makes polish optional: when it fails because
            // the network never answered, the raw transcript is the result.
            let local_asr_used = local_fallback_used
                || local_won_race
                || crate::dictation_context::is_local_asr_provider(&context.asr.provider_type);
            let (polish_output, polish_failed, polish_ms) = if uses_polisher {
                publish_progress(
                    &session,
                    session_id,
                    &progress,
                    EngineProgress::Stage(EngineStage::Polishing),
                )?;

                session.polishing.store(true, Ordering::Release);
                if session.cancelled.load(Ordering::Acquire) {
                    let _ = cancel_polisher_once(&session, &polisher, session_id).await;
                    remove_session(&sessions, session_id, &session);
                    return Err(cancelled_error(
                        "dictation was cancelled before polishing started",
                    )
                    .into());
                }
                let polish_partials: Arc<dyn TextStreamSink> = Arc::new(PolishProgressForwarder {
                    session_id,
                    progress: Arc::clone(&progress),
                });
                let polish_started = std::time::Instant::now();
                let output = if let Some(error) = context.deferred_llm_error.clone() {
                    Err(error)
                } else {
                    polisher
                        .polish(
                            session_id,
                            context,
                            transcript.text.clone(),
                            polish_partials,
                        )
                        .await
                };
                let result = match output {
                    Ok(text) => (text, false),
                    // Offline after local recognition: the raw transcript is the
                    // expected output. A rejected key or quota is still reported.
                    Err(error) if can_fallback_to_raw(policy, &error) => (
                        crate::ports::PolishOutput::text(transcript.text.clone()),
                        !(local_asr_used && error.retryable && !translation_active),
                    ),
                    Err(error) => {
                        let polish_ms = Some(polish_started.elapsed().as_millis() as u64);
                        let cancelled = session.cancelled.load(Ordering::Acquire);
                        let _ = cancel_polisher_once(&session, &polisher, session_id).await;
                        remove_session(&sessions, session_id, &session);
                        let error = if cancelled {
                            cancelled_error("dictation was cancelled while polishing was running")
                        } else {
                            error
                        };
                        let mut failure = EngineFailure::new(error, EngineFailureStage::Polishing);
                        failure.raw_text = Some(transcript.text.clone());
                        failure.duration_ms = Some(transcript.duration_ms);
                        failure.asr_ms = asr_ms;
                        failure.polish_ms = polish_ms;
                        failure.has_audio_recording = has_audio_recording;
                        failure.asr_call_label = asr_call_label.clone();
                        return Err(failure);
                    }
                };
                let polish_ms = Some(polish_started.elapsed().as_millis() as u64);
                session.polishing.store(false, Ordering::Release);
                (result.0, result.1, polish_ms)
            } else {
                (
                    crate::ports::PolishOutput::text(transcript.text.clone()),
                    false,
                    None,
                )
            };
            if session.cancelled.load(Ordering::Acquire) {
                remove_session(&sessions, session_id, &session);
                return Err(
                    cancelled_error("dictation was cancelled after polishing finished").into(),
                );
            }
            // Untouched Raw is ASR passthrough: it never entered Polishing,
            // so emitting a polish event violates the real backend validator.
            // Its final text still travels in EngineResult for one-shot input.
            if uses_polisher {
                publish_progress(
                    &session,
                    session_id,
                    &progress,
                    EngineProgress::PolishDelta(PolishDelta {
                        text: polish_output.text.clone(),
                        offset: 0,
                        is_final: true,
                    }),
                )?;
            }

            if uses_polisher {
                let findings =
                    crate::output_cleaning::find_adjacent_near_duplicates(&polish_output.text);
                let finding_summary = findings
                    .iter()
                    .map(|finding| {
                        format!(
                            "{}x{}:{:.4}/{:.4}",
                            finding.left_chars,
                            finding.right_chars,
                            finding.similarity,
                            finding.length_adjusted_similarity
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                log::info!(
                    "[polish-diagnostics] session_id={} input_chars={} input_hash={} final_chars={} final_hash={} near_duplicate_count={} near_duplicates={} polish_failed={} provider={} model={}",
                    session_id,
                    transcript.text.chars().count(),
                    crate::output_cleaning::privacy_text_hash(&transcript.text),
                    polish_output.text.chars().count(),
                    crate::output_cleaning::privacy_text_hash(&polish_output.text),
                    findings.len(),
                    finding_summary,
                    polish_failed,
                    polish_output
                        .llm_call_label
                        .as_ref()
                        .map_or("", |label| label.provider.as_str()),
                    polish_output
                        .llm_call_label
                        .as_ref()
                        .map_or("", |label| label.model.as_str()),
                );
                if !findings.is_empty() {
                    log::warn!(
                        "[polish-diagnostics] adjacent near-duplicate output detected for session_id={} count={}",
                        session_id,
                        findings.len()
                    );
                }
            }

            remove_session(&sessions, session_id, &session);
            Ok(EngineResult {
                raw_text: transcript.text,
                asr_transcript,
                polished_text: polish_output.text,
                polish_source: polish_output.source_text,
                duration_ms: transcript.duration_ms,
                polish_failed,
                asr_ms,
                polish_ms,
                has_audio_recording,
                asr_call_label,
                llm_call_label: polish_output.llm_call_label,
            })
        })
    }

    fn update_context(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self
            .sessions
            .lock()
            .expect("pipeline session lock poisoned")
            .get(&session_id)
            .cloned();
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "dictation pipeline session is not active",
                )
            })?;
            session.update_context(context)
        })
    }

    fn feed_audio(&self, session_id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        self.recorder.feed_pcm(session_id, pcm)
    }

    fn retain_failed_recording(&self, session_id: SessionId) -> Option<Arc<dyn RecordingArchive>> {
        let session = self
            .sessions
            .lock()
            .expect("pipeline session lock poisoned")
            .get(&session_id)
            .cloned()?;
        if session.context().output_target == DictationOutputTarget::CloudNote {
            return None;
        }
        session
            .retain_failed_recording
            .store(true, Ordering::Release);
        let resources = session
            .resources
            .lock()
            .expect("pipeline resource lock poisoned");
        resources
            .recording
            .as_ref()
            .and_then(PipelineRecording::archive)
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let sessions = Arc::clone(&self.sessions);
        let polisher = Arc::clone(&self.polisher);
        let pending_captures = Arc::clone(&self.pending_captures);
        Box::pin(async move {
            let Some(session) = sessions
                .lock()
                .expect("pipeline session lock poisoned")
                .get(&session_id)
                .cloned()
            else {
                return Ok(());
            };
            if session.cancelled.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            let preserve_quick_note_archive = session.context().output_target
                == crate::dictation_context::DictationOutputTarget::QuickNote;
            let capture_generation = session.context().recording.capture_generation;

            let (recording, transcription, buffered) = {
                let mut resources = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned");
                (
                    resources.recording.take(),
                    resources.transcription.clone(),
                    resources.buffered.take(),
                )
            };
            let mut first_error = None;
            if let Some(generation) = capture_generation {
                retain_first_error(
                    &mut first_error,
                    discard_pending_capture(&pending_captures, generation).await,
                );
            }
            if let Some(recording) = recording {
                let archive = recording.archive();
                retain_first_error(&mut first_error, recording.stop().await);
                if session.retain_failed_recording.load(Ordering::Acquire) {
                    let mut available = None;
                    demote_failed_archive(
                        archive.as_ref(),
                        session.context().output_target,
                        &mut available,
                    )
                    .await;
                } else if !preserve_quick_note_archive {
                    if let Some(archive) = archive {
                        retain_first_error(&mut first_error, archive.discard().await);
                    }
                }
            }
            if let Some(buffered) = buffered {
                retain_first_error(
                    &mut first_error,
                    buffered.cancel_without_waiting_for_start().await,
                );
            } else if let Some(transcription) = transcription {
                retain_first_error(
                    &mut first_error,
                    cancel_transcription_once(&session, transcription).await,
                );
            }
            if session.polishing.load(Ordering::Acquire) {
                retain_first_error(
                    &mut first_error,
                    cancel_polisher_once(&session, &polisher, session_id).await,
                );
            }
            remove_session(&sessions, session_id, &session);
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
    }
}

async fn discard_ephemeral_archive(
    archive: Option<&Arc<dyn RecordingArchive>>,
    has_audio_recording: &mut Option<bool>,
) {
    let Some(archive) = archive else {
        return;
    };
    if archive.is_available() {
        if let Err(error) = archive.discard().await {
            log::warn!("[recording] failed to discard ephemeral archive: {error}");
        }
    }
    *has_audio_recording = Some(archive.is_available());
}

async fn demote_failed_archive(
    archive: Option<&Arc<dyn RecordingArchive>>,
    output_target: DictationOutputTarget,
    has_audio_recording: &mut Option<bool>,
) {
    if output_target == DictationOutputTarget::CloudNote {
        discard_ephemeral_archive(archive, has_audio_recording).await;
        return;
    }
    if output_target == DictationOutputTarget::QuickNote {
        return;
    }
    let Some(archive) = archive else {
        return;
    };
    if archive.is_available() {
        if let Err(error) = archive.demote_to_ordinary_recording().await {
            log::warn!("[recording] failed to demote failed archive: {error}");
        }
    }
    *has_audio_recording = Some(archive.is_available());
}

fn find_session(
    sessions: &Arc<Mutex<HashMap<SessionId, Arc<PipelineSession>>>>,
    session_id: SessionId,
) -> Result<Arc<PipelineSession>, BackendError> {
    sessions
        .lock()
        .expect("pipeline session lock poisoned")
        .get(&session_id)
        .cloned()
        .ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "dictation pipeline session is not active",
            )
        })
}

async fn discard_pending_capture(
    pending_captures: &Arc<Mutex<HashMap<u64, Box<dyn PendingAudioCapture>>>>,
    generation: u64,
) -> Result<(), BackendError> {
    let capture = pending_captures
        .lock()
        .expect("pending capture lock poisoned")
        .remove(&generation);
    match capture {
        Some(capture) => capture.discard().await,
        None => Ok(()),
    }
}

fn remove_session(
    sessions: &Arc<Mutex<HashMap<SessionId, Arc<PipelineSession>>>>,
    session_id: SessionId,
    expected: &Arc<PipelineSession>,
) {
    let mut sessions = sessions.lock().expect("pipeline session lock poisoned");
    if sessions
        .get(&session_id)
        .is_some_and(|current| Arc::ptr_eq(current, expected))
    {
        sessions.remove(&session_id);
    }
}

/// How a recorded-audio replay is attempted: the cloud retry backs off and
/// tries twice.
#[derive(Clone, Copy)]
struct ReplayPlan {
    attempts: u64,
    backoff: bool,
}

const CLOUD_RETRY: ReplayPlan = ReplayPlan {
    attempts: 2,
    backoff: true,
};

type AsrAttempt = (
    crate::ports::TranscriptOutput,
    Option<crate::auxiliary::AsrCallLabel>,
);

/// The outcome of racing the cloud ASR against the local standby.
enum AsrRace {
    /// The cloud answered first (or the local model had nothing usable).
    /// `standby` is the local transcription still pending, if any, for the
    /// caller to use after a cloud error or to abandon.
    Cloud {
        result: Result<crate::ports::TranscriptOutput, BackendError>,
        standby: Option<LocalStandby>,
    },
    /// The local model finished first with a usable transcript.
    Local(AsrAttempt),
}

/// Runs the cloud ASR and the local standby side by side once recording has
/// stopped; whichever returns a transcript first is used. The cloud wins a tie,
/// and a failed local run leaves the cloud to finish on its own.
async fn race_cloud_with_local_standby(
    mut cloud: BoxFuture<'static, Result<crate::ports::TranscriptOutput, BackendError>>,
    mut standby: LocalStandby,
) -> AsrRace {
    tokio::select! {
        biased;
        result = &mut cloud => AsrRace::Cloud {
            result,
            standby: Some(standby),
        },
        local = &mut standby.run => match local {
            Some(local) => AsrRace::Local(local),
            None => AsrRace::Cloud {
                result: cloud.await,
                standby: None,
            },
        },
    }
}

/// The session's local fallback model transcribing the frozen recording,
/// outside the pipeline's registered resources so that it can run alongside
/// the cloud call. Yields None when there is no usable result — the caller then
/// carries on with the cloud's outcome, so a missing or broken local model never
/// replaces the error the user's provider actually gave.
struct LocalStandby {
    run: BoxFuture<'static, Option<AsrAttempt>>,
    /// The started local session until it finishes, so that an abandoned
    /// standby can still release its model.
    started: Arc<Mutex<Option<Arc<dyn TranscriptionSession>>>>,
}

impl LocalStandby {
    fn new(
        engine: Arc<dyn TranscriptionEngine>,
        context: Arc<DictationContext>,
        session: Arc<PipelineSession>,
        session_id: SessionId,
        archive: Arc<dyn RecordingArchive>,
    ) -> Self {
        let started: Arc<Mutex<Option<Arc<dyn TranscriptionSession>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&started);
        let run = Box::pin(async move {
            let transcribe = async {
                let fallback = context.asr_fallback.clone()?;
                let pcm = archive.read_pcm().await.ok().filter(|pcm| !pcm.is_empty())?;
                let mut local = (*context).clone();
                local.asr = fallback;
                local.asr_fallback = None;
                let prepared = match engine.prepare(session_id, Arc::new(local)).await {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        log::warn!("[dictation] local ASR standby unavailable: {}", error.message);
                        return None;
                    }
                };
                // Partials stay private: the cloud may still win.
                let transcription = match prepared.start(Arc::new(DiscardedTextStream)).await {
                    Ok(transcription) => transcription,
                    Err(error) => {
                        log::warn!("[dictation] local ASR standby failed to start: {}", error.message);
                        return None;
                    }
                };
                *slot.lock().expect("local standby lock poisoned") = Some(Arc::clone(&transcription));
                let label = transcription.asr_call_label();
                transcription.consume_pcm_chunk(&pcm);
                let result = transcription.finish().await;
                slot.lock().expect("local standby lock poisoned").take();
                match result {
                    Ok(output) if output.text.trim().is_empty() => {
                        log::warn!("[dictation] local ASR standby returned an empty transcript");
                        None
                    }
                    Ok(output) => Some((output, label)),
                    Err(error) => {
                        log::warn!("[dictation] local ASR standby failed: {}", error.message);
                        None
                    }
                }
            };
            tokio::select! {
                result = transcribe => result,
                _ = wait_until_cancelled(&session) => {
                    let started = slot.lock().expect("local standby lock poisoned").take();
                    if let Some(transcription) = started {
                        let _ = transcription.cancel().await;
                    }
                    None
                }
            }
        });
        Self { run, started }
    }

    async fn result(self) -> Option<AsrAttempt> {
        self.run.await
    }

    /// Stops a standby whose result is no longer needed and releases its model.
    async fn abandon(self) {
        drop(self.run);
        let started = self.started.lock().expect("local standby lock poisoned").take();
        if let Some(transcription) = started {
            let _ = transcription.cancel().await;
        }
    }
}

async fn wait_until_cancelled(session: &PipelineSession) {
    while !session.cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

struct DiscardedTextStream;

impl TextStreamSink for DiscardedTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

async fn retry_transcription(
    prepared: Arc<dyn PreparedTranscription>,
    session: Arc<PipelineSession>,
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
    pcm: Vec<u8>,
    plan: ReplayPlan,
) -> Result<
    (
        crate::ports::TranscriptOutput,
        Option<crate::auxiliary::AsrCallLabel>,
    ),
    (BackendError, Option<crate::auxiliary::AsrCallLabel>),
> {
    let mut last_label = None;
    for attempt in 1..=plan.attempts {
        if plan.backoff {
            if let Err(error) =
                cancellable_backoff(&session, std::time::Duration::from_millis(500 * attempt)).await
            {
                return Err((error, last_label));
            }
        }
        let partials: Arc<dyn TextStreamSink> = Arc::new(TranscriptProgressForwarder {
            session_id,
            progress: Arc::clone(&progress),
        });
        let transcription = match prepared.start(partials).await {
            Ok(transcription) => transcription,
            Err(_) if session.cancelled.load(Ordering::Acquire) => {
                return Err((
                    cancelled_error("dictation was cancelled while retry ASR was starting"),
                    last_label,
                ));
            }
            Err(error) if error.retryable && attempt < plan.attempts => continue,
            Err(error) => return Err((error, last_label)),
        };
        last_label = transcription.asr_call_label();
        let registered = {
            let mut resources = session
                .resources
                .lock()
                .expect("pipeline resource lock poisoned");
            // Cancellation may have removed the pipeline while the provider
            // was creating this retry. Publish the new resource and reset its
            // once-only flags under the same lock used by cancel(), so that
            // cancel either owns this retry or the late-start path cleans it.
            if session.cancelled.load(Ordering::Acquire) {
                false
            } else {
                session
                    .transcription_cancelled
                    .store(false, Ordering::Release);
                session
                    .transcription_finished
                    .store(false, Ordering::Release);
                resources.transcription = Some(Arc::clone(&transcription));
                resources.buffered = None;
                true
            }
        };
        if !registered {
            // The global once flag belongs to the previous attempt. This
            // unregistered resource must be cancelled directly, never fed.
            let _ = transcription.cancel().await;
            return Err((
                cancelled_error("dictation was cancelled while retry ASR was starting"),
                last_label,
            ));
        }
        if session.cancelled.load(Ordering::Acquire) {
            let _ = cancel_transcription_once(&session, transcription).await;
            return Err((
                cancelled_error("dictation was cancelled before retry audio replay"),
                last_label,
            ));
        }
        transcription.consume_pcm_chunk(&pcm);
        let transcription_result = transcription.finish().await;
        for notification in transcription.take_progress_notifications() {
            if let Err(error) = publish_progress(
                &session,
                session_id,
                &progress,
                EngineProgress::Notification(notification),
            ) {
                return Err((error, last_label));
            }
        }
        match transcription_result {
            Ok(output) => {
                session
                    .transcription_finished
                    .store(true, Ordering::Release);
                return Ok((output, last_label));
            }
            Err(error) => {
                let _ = cancel_transcription_once(&session, transcription).await;
                if !error.retryable || attempt == plan.attempts {
                    return Err((error, last_label));
                }
            }
        }
    }
    unreachable!("retry loop always returns")
}

async fn cancellable_backoff(
    session: &PipelineSession,
    duration: std::time::Duration,
) -> Result<(), BackendError> {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        if session.cancelled.load(Ordering::Acquire) {
            return Err(cancelled_error(
                "dictation was cancelled during ASR retry backoff",
            ));
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        tokio::time::sleep(remaining.min(std::time::Duration::from_millis(25))).await;
    }
}

async fn cancel_transcription_once<T>(
    session: &Arc<PipelineSession>,
    transcription: Arc<T>,
) -> Result<(), BackendError>
where
    T: TranscriptionSession + ?Sized,
{
    if session.transcription_finished.load(Ordering::Acquire)
        || session.transcription_cancelled.swap(true, Ordering::AcqRel)
    {
        return Ok(());
    }
    transcription.cancel().await
}

async fn cancel_polisher_once(
    session: &Arc<PipelineSession>,
    polisher: &Arc<dyn TextPolisher>,
    session_id: SessionId,
) -> Result<(), BackendError> {
    if session.polisher_cancelled.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    polisher.cancel(session_id).await
}

fn publish_progress(
    session: &Arc<PipelineSession>,
    session_id: SessionId,
    progress: &Arc<dyn EngineProgressSink>,
    event: EngineProgress,
) -> Result<(), BackendError> {
    if session.cancelled.load(Ordering::Acquire) {
        return Err(cancelled_error(
            "dictation progress arrived after cancellation",
        ));
    }
    progress.publish(session_id, event)
}

fn can_fallback_to_raw(policy: PolishFailurePolicy, error: &BackendError) -> bool {
    policy == PolishFailurePolicy::UseRawText
        && matches!(
            error.code,
            BackendErrorCode::Provider | BackendErrorCode::Unsupported
        )
}

fn cancelled_error(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, message)
}

fn retain_first_error(first_error: &mut Option<BackendError>, result: Result<(), BackendError>) {
    if first_error.is_none() {
        if let Err(error) = result {
            *first_error = Some(error);
        }
    }
}

pub(crate) fn buffered_transcription_session(
    prepared: Arc<dyn PreparedTranscription>,
    context: Arc<DictationContext>,
    partials: Arc<dyn TextStreamSink>,
    progress: Arc<dyn RecordingProgressSink>,
    task_spawner: Arc<dyn crate::TaskSpawner>,
) -> Arc<dyn TranscriptionSession> {
    let partials = if context.recording.transcribe_after_stop {
        Arc::new(DiscardTextStream) as Arc<dyn TextStreamSink>
    } else {
        partials
    };
    let buffered = Arc::new(BufferedTranscriptionSession::new(
        prepared, partials, progress,
    ));
    if !context.recording.transcribe_after_stop {
        buffered.attach_in_background(task_spawner);
    }
    buffered
}

struct BufferedTranscriptionSession {
    inner: Arc<BufferedTranscriptionInner>,
}

struct BufferedTranscriptionInner {
    prepared: Arc<dyn PreparedTranscription>,
    partials: Arc<dyn TextStreamSink>,
    progress: Arc<dyn RecordingProgressSink>,
    limit_notified: AtomicBool,
    has_nonzero_pcm: AtomicBool,
    limit_threshold_bytes: usize,
    state: Mutex<BufferedTranscriptionState>,
}

enum BufferedTranscriptionState {
    Buffering(Vec<u8>),
    Attaching {
        pcm: Vec<u8>,
        waiter: Arc<tokio::sync::Notify>,
    },
    Direct(Arc<dyn TranscriptionSession>),
    Failed(BackendError),
    Cancelled,
}

impl BufferedTranscriptionSession {
    fn new(
        prepared: Arc<dyn PreparedTranscription>,
        partials: Arc<dyn TextStreamSink>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> Self {
        Self::new_with_limit(
            prepared,
            partials,
            progress,
            BUFFERED_TRANSCRIPTION_STOP_THRESHOLD_BYTES,
        )
    }

    fn new_with_limit(
        prepared: Arc<dyn PreparedTranscription>,
        partials: Arc<dyn TextStreamSink>,
        progress: Arc<dyn RecordingProgressSink>,
        limit_threshold_bytes: usize,
    ) -> Self {
        Self {
            inner: Arc::new(BufferedTranscriptionInner {
                prepared,
                partials,
                progress,
                limit_notified: AtomicBool::new(false),
                has_nonzero_pcm: AtomicBool::new(false),
                limit_threshold_bytes,
                state: Mutex::new(BufferedTranscriptionState::Buffering(Vec::new())),
            }),
        }
    }

    fn attach(&self) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let waiter = Arc::new(tokio::sync::Notify::new());
        let start = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("buffered transcription lock poisoned");
            match &mut *state {
                BufferedTranscriptionState::Buffering(pcm) => {
                    let pcm = std::mem::take(pcm);
                    *state = BufferedTranscriptionState::Attaching {
                        pcm: Vec::new(),
                        waiter: Arc::clone(&waiter),
                    };
                    Some(pcm)
                }
                BufferedTranscriptionState::Direct(session) => {
                    let session = Arc::clone(session);
                    return Box::pin(async move { Ok(session) });
                }
                BufferedTranscriptionState::Failed(error) => {
                    let error = error.clone();
                    return Box::pin(async move { Err(error) });
                }
                BufferedTranscriptionState::Cancelled => {
                    return Box::pin(async {
                        Err(cancelled_error("transcription buffer was cancelled"))
                    });
                }
                BufferedTranscriptionState::Attaching { .. } => {
                    let waiter = match &*state {
                        BufferedTranscriptionState::Attaching { waiter, .. } => Arc::clone(waiter),
                        _ => unreachable!("buffer state changed while it was locked"),
                    };
                    let inner = Arc::clone(&self.inner);
                    return Box::pin(async move {
                        waiter.notified().await;
                        match &*inner
                            .state
                            .lock()
                            .expect("buffered transcription lock poisoned")
                        {
                            BufferedTranscriptionState::Direct(session) => Ok(Arc::clone(session)),
                            BufferedTranscriptionState::Failed(error) => Err(error.clone()),
                            BufferedTranscriptionState::Cancelled => {
                                Err(cancelled_error("transcription buffer was cancelled"))
                            }
                            BufferedTranscriptionState::Buffering(_)
                            | BufferedTranscriptionState::Attaching { .. } => {
                                Err(BackendError::new(
                                    BackendErrorCode::Internal,
                                    "transcription attachment did not settle",
                                ))
                            }
                        }
                    });
                }
            }
        };
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            attach_buffered_transcription(inner, start.expect("buffer attach must start"), waiter)
                .await
        })
    }

    fn attach_in_background(&self, task_spawner: Arc<dyn crate::TaskSpawner>) {
        let attaching = self.attach();
        task_spawner.spawn(Box::pin(async move {
            if let Err(error) = attaching.await {
                log::warn!("provider-only transcription startup failed: {error}");
            }
        }));
    }

    fn prepared(&self) -> Arc<dyn PreparedTranscription> {
        Arc::clone(&self.inner.prepared)
    }

    fn downstream(&self) -> Option<Arc<dyn TranscriptionSession>> {
        match &*self
            .inner
            .state
            .lock()
            .expect("buffered transcription lock poisoned")
        {
            BufferedTranscriptionState::Direct(session) => Some(Arc::clone(session)),
            _ => None,
        }
    }
}

fn attach_buffered_transcription(
    inner: Arc<BufferedTranscriptionInner>,
    pcm: Vec<u8>,
    waiter: Arc<tokio::sync::Notify>,
) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
    Box::pin(async move {
        let _notify = NotifyOnDrop { waiter };
        if matches!(
            *inner
                .state
                .lock()
                .expect("buffered transcription lock poisoned"),
            BufferedTranscriptionState::Cancelled
        ) {
            return Err(cancelled_error(
                "transcription cancelled before provider startup",
            ));
        }
        let downstream = match inner.prepared.start(Arc::clone(&inner.partials)).await {
            Ok(session) => session,
            Err(error) => {
                let mut state = inner
                    .state
                    .lock()
                    .expect("buffered transcription lock poisoned");
                if matches!(*state, BufferedTranscriptionState::Cancelled) {
                    return Err(cancelled_error(
                        "transcription buffer was cancelled while ASR was starting",
                    ));
                }
                *state = BufferedTranscriptionState::Failed(error.clone());
                return Err(error);
            }
        };

        let mut first_chunk = Some(pcm);

        loop {
            let chunk = if let Some(chunk) = first_chunk.take().filter(|chunk| !chunk.is_empty()) {
                if matches!(
                    &*inner
                        .state
                        .lock()
                        .expect("buffered transcription lock poisoned"),
                    BufferedTranscriptionState::Cancelled
                ) {
                    Err(cancelled_error(
                        "transcription buffer was cancelled while ASR was starting",
                    ))
                } else {
                    Ok(chunk)
                }
            } else {
                let mut state = inner
                    .state
                    .lock()
                    .expect("buffered transcription lock poisoned");
                match &mut *state {
                    BufferedTranscriptionState::Attaching { pcm, .. } if pcm.is_empty() => {
                        *state = BufferedTranscriptionState::Direct(Arc::clone(&downstream));
                        return Ok(downstream);
                    }
                    BufferedTranscriptionState::Attaching { pcm, .. } => Ok(std::mem::take(pcm)),
                    BufferedTranscriptionState::Cancelled => Err(cancelled_error(
                        "transcription buffer was cancelled while ASR was starting",
                    )),
                    BufferedTranscriptionState::Failed(error) => Err(error.clone()),
                    BufferedTranscriptionState::Direct(session) => return Ok(Arc::clone(session)),
                    BufferedTranscriptionState::Buffering(_) => unreachable!(
                        "buffered transcription cannot return to buffering while attaching"
                    ),
                }
            };
            match chunk {
                Ok(chunk) => {
                    for chunk in chunk.chunks(BUFFERED_TRANSCRIPTION_FORWARD_CHUNK_BYTES) {
                        downstream.consume_pcm_chunk(chunk);
                    }
                }
                Err(error) => {
                    let _ = downstream.cancel().await;
                    return Err(error);
                }
            }
        }
    })
}

struct NotifyOnDrop {
    waiter: Arc<tokio::sync::Notify>,
}

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.waiter.notify_waiters();
        self.waiter.notify_one();
    }
}

impl AudioConsumer for BufferedTranscriptionSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if !self.inner.has_nonzero_pcm.load(Ordering::Relaxed) && pcm.iter().any(|byte| *byte != 0)
        {
            self.inner.has_nonzero_pcm.store(true, Ordering::Release);
        }
        let (downstream, buffer_limit_reached) = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("buffered transcription lock poisoned");
            match &mut *state {
                BufferedTranscriptionState::Buffering(buffer)
                | BufferedTranscriptionState::Attaching { pcm: buffer, .. } => {
                    buffer.extend_from_slice(pcm);
                    (None, buffer.len() >= self.inner.limit_threshold_bytes)
                }
                BufferedTranscriptionState::Direct(session) => (Some(Arc::clone(session)), false),
                BufferedTranscriptionState::Failed(_) | BufferedTranscriptionState::Cancelled => {
                    (None, false)
                }
            }
        };
        if let Some(downstream) = downstream {
            downstream.consume_pcm_chunk(pcm);
        }
        if buffer_limit_reached
            && self
                .inner
                .limit_notified
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            // ponytail: one stop request only; if the host cannot honor it, PCM
            // may exceed 128 MiB until the caller stops. Never truncate the tail;
            // add a hard cap only with a product-level overflow policy.
            if let Err(error) = self
                .inner
                .progress
                .publish(crate::ports::RecordingEvent::LimitReached)
            {
                log::warn!("failed to request recording stop at PCM limit: {error}");
            }
        }
    }
}

impl TranscriptionSession for BufferedTranscriptionSession {
    fn asr_call_label(&self) -> Option<crate::auxiliary::AsrCallLabel> {
        self.downstream()
            .and_then(|session| session.asr_call_label())
    }

    fn take_progress_notifications(&self) -> Vec<crate::types::NotificationPayload> {
        self.downstream()
            .map_or_else(Vec::new, |session| session.take_progress_notifications())
    }

    fn finish(&self) -> BoxFuture<'static, Result<crate::ports::TranscriptOutput, BackendError>> {
        // Inspect after the caller stops capture. Cancellation remains independent
        // of audio validity, and quiet non-zero samples do not trip a speech threshold.
        let terminal = matches!(
            &*self
                .inner
                .state
                .lock()
                .expect("buffered transcription lock poisoned"),
            BufferedTranscriptionState::Failed(_) | BufferedTranscriptionState::Cancelled
        );
        if !terminal && !self.inner.has_nonzero_pcm.load(Ordering::Acquire) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "未收到有效音频，请检查麦克风输入设备后重试",
                ))
            });
        }
        let attaching = self.attach();
        Box::pin(async move {
            let downstream = attaching.await?;
            downstream.finish().await
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let (downstream, waiter) = self.take_cancel_state();
        Box::pin(async move {
            if let Some(waiter) = waiter {
                waiter.notified().await;
            }
            match downstream {
                Some(session) => session.cancel().await,
                None => Ok(()),
            }
        })
    }
}

impl BufferedTranscriptionSession {
    fn take_cancel_state(
        &self,
    ) -> (
        Option<Arc<dyn TranscriptionSession>>,
        Option<Arc<tokio::sync::Notify>>,
    ) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("buffered transcription lock poisoned");
        let waiter = match &*state {
            BufferedTranscriptionState::Attaching { waiter, .. } => Some(Arc::clone(waiter)),
            _ => None,
        };
        let downstream = match std::mem::replace(&mut *state, BufferedTranscriptionState::Cancelled)
        {
            BufferedTranscriptionState::Direct(session) => Some(session),
            _ => None,
        };
        (downstream, waiter)
    }

    fn cancel_without_waiting_for_start(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let (downstream, _) = self.take_cancel_state();
        Box::pin(async move {
            match downstream {
                Some(session) => session.cancel().await,
                None => Ok(()),
            }
        })
    }
}

struct DiscardTextStream;

impl TextStreamSink for DiscardTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

struct RecordingProgressForwarder {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
}

impl RecordingProgressSink for RecordingProgressForwarder {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        self.progress.publish(
            self.session_id,
            EngineProgress::RecordingLevel {
                elapsed_ms,
                level: level.clamp(0.0, 1.0),
            },
        )
    }

    fn publish(&self, event: crate::ports::RecordingEvent) -> Result<(), BackendError> {
        match event {
            crate::ports::RecordingEvent::Level { elapsed_ms, level } => {
                self.publish_level(elapsed_ms, level)
            }
            crate::ports::RecordingEvent::LimitReached => self
                .progress
                .publish(self.session_id, EngineProgress::RecordingLimitReached),
            crate::ports::RecordingEvent::Fatal(error) => self
                .progress
                .publish(self.session_id, EngineProgress::RecordingFault(error)),
        }
    }
}

struct TranscriptProgressForwarder {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
}

impl TextStreamSink for TranscriptProgressForwarder {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        log::info!(
            "[transcript] partial emitted session={} offset={} chars={}",
            self.session_id,
            chunk.offset,
            chunk.text.chars().count(),
        );
        self.progress.publish(
            self.session_id,
            EngineProgress::TranscriptDelta(TranscriptDelta {
                text: chunk.text,
                offset: chunk.offset,
                is_final: false,
            }),
        )
    }
}

struct PolishProgressForwarder {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
}

impl TextStreamSink for PolishProgressForwarder {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        self.progress.publish(
            self.session_id,
            EngineProgress::PolishDelta(PolishDelta {
                text: chunk.text,
                offset: chunk.offset,
                is_final: false,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::ports::{RecordingArchive, TranscriptOutput};

    #[derive(Default)]
    struct RecordingProgress {
        events: Mutex<Vec<EngineProgress>>,
    }

    impl EngineProgressSink for RecordingProgress {
        fn publish(
            &self,
            _session_id: SessionId,
            progress: EngineProgress,
        ) -> Result<(), BackendError> {
            self.events.lock().unwrap().push(progress);
            Ok(())
        }
    }

    struct NoopRecordingProgress;

    impl RecordingProgressSink for NoopRecordingProgress {
        fn publish_level(&self, _elapsed_ms: u64, _level: f32) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn early_recording_stop_shares_cleanup_failure_and_preserves_the_archive() {
        struct BlockingStop {
            stops: Arc<AtomicUsize>,
            release: Arc<tokio::sync::Notify>,
            archive: Arc<FixtureArchive>,
        }
        impl ActiveRecording for BlockingStop {
            fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
                Some(self.archive.clone())
            }
            fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
                self.stops.fetch_add(1, Ordering::AcqRel);
                Box::pin(async move {
                    self.release.notified().await;
                    Err(BackendError::new(
                        BackendErrorCode::Platform,
                        "fixture stop failure",
                    ))
                })
            }
        }
        let stops = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let archive = Arc::new(FixtureArchive {
            available: AtomicBool::new(true),
            discards: Arc::new(AtomicUsize::new(0)),
            pcm: vec![1, 0],
        });
        let mut recording = PipelineRecording::new(Box::new(BlockingStop {
            stops: stops.clone(),
            release: release.clone(),
            archive: archive.clone(),
        }));
        let retained = recording.archive().unwrap();
        let mut early_stop = Box::pin(recording.stop_recording());
        let mut terminal_stop = Box::pin(recording.stop());
        assert!(futures_util::poll!(early_stop.as_mut()).is_pending());
        assert!(futures_util::poll!(terminal_stop.as_mut()).is_pending());
        assert_eq!(stops.load(Ordering::Acquire), 1);
        release.notify_one();
        let (early, terminal) = tokio::join!(early_stop, terminal_stop);
        assert_eq!(early.unwrap_err().message, "fixture stop failure");
        assert_eq!(terminal.unwrap_err().message, "fixture stop failure");
        assert!(retained.is_available());
        assert_eq!(retained.read_pcm().await.unwrap(), vec![1, 0]);
        assert_eq!(archive.discards.load(Ordering::Acquire), 0);
    }

    #[derive(Default)]
    struct LimitRecordingProgress {
        limits: AtomicUsize,
    }

    impl RecordingProgressSink for LimitRecordingProgress {
        fn publish_level(&self, _elapsed_ms: u64, _level: f32) -> Result<(), BackendError> {
            Ok(())
        }

        fn publish(&self, event: crate::ports::RecordingEvent) -> Result<(), BackendError> {
            if matches!(event, crate::ports::RecordingEvent::LimitReached) {
                self.limits.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    struct FixtureRecording {
        stops: Arc<AtomicUsize>,
        archive: Arc<FixtureArchive>,
    }

    struct FixtureArchive {
        available: AtomicBool,
        discards: Arc<AtomicUsize>,
        pcm: Vec<u8>,
    }

    impl RecordingArchive for FixtureArchive {
        fn is_available(&self) -> bool {
            self.available.load(Ordering::Acquire)
        }

        fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.discards.fetch_add(1, Ordering::AcqRel);
            self.available.store(false, Ordering::Release);
            Box::pin(async { Ok(()) })
        }

        fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
            let pcm = self.pcm.clone();
            Box::pin(async move { Ok(pcm) })
        }
    }

    impl ActiveRecording for FixtureRecording {
        fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
            Some(self.archive.clone())
        }

        fn stop(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    struct FixtureRecorder {
        stops: Arc<AtomicUsize>,
        archive: Arc<FixtureArchive>,
        fail: bool,
    }

    impl AudioRecorder for FixtureRecorder {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            consumer: Arc<dyn AudioConsumer>,
            progress: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            let stops = Arc::clone(&self.stops);
            let archive = Arc::clone(&self.archive);
            let fail = self.fail;
            Box::pin(async move {
                if fail {
                    return Err(BackendError::new(
                        BackendErrorCode::Platform,
                        "fixture recorder failed",
                    ));
                }
                consumer.consume_pcm_chunk(&[1, 0, 2, 0]);
                progress.publish_level(25, 1.5)?;
                Ok(Box::new(FixtureRecording { stops, archive }) as Box<dyn ActiveRecording>)
            })
        }
    }

    struct ExposedRecorder {
        consumer: Arc<Mutex<Option<Arc<dyn AudioConsumer>>>>,
        stops: Arc<AtomicUsize>,
    }

    impl AudioRecorder for ExposedRecorder {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            consumer: Arc<dyn AudioConsumer>,
            _progress: Arc<dyn RecordingProgressSink>,
        ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
            consumer.consume_pcm_chunk(&[1, 0]);
            *self.consumer.lock().unwrap() = Some(consumer);
            let stops = Arc::clone(&self.stops);
            Box::pin(async move {
                Ok(Box::new(FixtureRecording {
                    stops,
                    archive: Arc::new(FixtureArchive {
                        available: AtomicBool::new(false),
                        discards: Arc::new(AtomicUsize::new(0)),
                        pcm: Vec::new(),
                    }),
                }) as Box<dyn ActiveRecording>)
            })
        }
    }

    struct FixtureTranscriptionSession {
        pcm: Arc<Mutex<Vec<u8>>>,
        cancels: Arc<AtomicUsize>,
        finish_entered: Option<Arc<tokio::sync::Notify>>,
        finish_release: Option<Arc<tokio::sync::Notify>>,
    }

    impl AudioConsumer for FixtureTranscriptionSession {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().extend_from_slice(pcm);
        }
    }

    impl TranscriptionSession for FixtureTranscriptionSession {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let entered = self.finish_entered.clone();
            let release = self.finish_release.clone();
            Box::pin(async move {
                if let Some(entered) = entered {
                    entered.notify_one();
                }
                if let Some(release) = release {
                    release.notified().await;
                }
                Ok(TranscriptOutput {
                    text: "raw text".to_string(),
                    duration_ms: 25,
                })
            })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancels.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    struct FixtureTranscriber {
        session: Arc<FixtureTranscriptionSession>,
        starts: Arc<AtomicUsize>,
    }

    impl TranscriptionEngine for FixtureTranscriber {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            let session = Arc::clone(&self.session);
            Box::pin(async move {
                partials.publish(TextStreamChunk {
                    text: "raw".to_string(),
                    offset: 0,
                })?;
                Ok(session as Arc<dyn TranscriptionSession>)
            })
        }
    }

    struct DelayedTranscriber {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        session: Arc<FixtureTranscriptionSession>,
    }

    impl TranscriptionEngine for DelayedTranscriber {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let entered = Arc::clone(&self.entered);
            let release = Arc::clone(&self.release);
            let session = Arc::clone(&self.session);
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                Ok(session as Arc<dyn TranscriptionSession>)
            })
        }
    }

    struct FailingTranscriber;

    impl TranscriptionEngine for FailingTranscriber {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture ASR start failed",
                ))
            })
        }
    }

    struct FailingPreparationTranscriber;

    impl TranscriptionEngine for FailingPreparationTranscriber {
        fn prepare(
            self: Arc<Self>,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
        ) -> BoxFuture<'static, Result<Arc<dyn crate::ports::PreparedTranscription>, BackendError>>
        {
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "fixture ASR preparation failed",
                ))
            })
        }

        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            Box::pin(async { unreachable!("preparation failure must precede provider start") })
        }
    }

    struct RetryTranscriber {
        outputs: Arc<Mutex<VecDeque<Result<TranscriptOutput, BackendError>>>>,
        starts: Arc<AtomicUsize>,
        pcm: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    struct RetryTranscriptionSession {
        output: Result<TranscriptOutput, BackendError>,
        pcm: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl AudioConsumer for RetryTranscriptionSession {
        fn consume_pcm_chunk(&self, pcm: &[u8]) {
            self.pcm.lock().unwrap().push(pcm.to_vec());
        }
    }

    impl TranscriptionSession for RetryTranscriptionSession {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let output = self.output.clone();
            Box::pin(async move { output })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }

    impl TranscriptionEngine for RetryTranscriber {
        fn start(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            let output = self
                .outputs
                .lock()
                .unwrap()
                .pop_front()
                .expect("retry fixture output");
            let pcm = Arc::clone(&self.pcm);
            Box::pin(async move {
                Ok(Arc::new(RetryTranscriptionSession { output, pcm })
                    as Arc<dyn TranscriptionSession>)
            })
        }
    }

    struct FixturePolisher {
        result: Result<crate::ports::PolishOutput, BackendError>,
        calls: Arc<AtomicUsize>,
        cancels: Arc<AtomicUsize>,
        contexts: Arc<Mutex<Vec<Arc<DictationContext>>>>,
    }

    impl TextPolisher for FixturePolisher {
        fn polish(
            &self,
            _session_id: SessionId,
            context: Arc<DictationContext>,
            _raw_text: String,
            partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<crate::ports::PolishOutput, BackendError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(context);
            let result = self.result.clone();
            Box::pin(async move {
                partials.publish(TextStreamChunk {
                    text: "polished".to_string(),
                    offset: 0,
                })?;
                result
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancels.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    fn noop_polisher() -> Arc<dyn TextPolisher> {
        Arc::new(FixturePolisher {
            result: Ok(crate::ports::PolishOutput::text("unused")),
            calls: Arc::new(AtomicUsize::new(0)),
            cancels: Arc::new(AtomicUsize::new(0)),
            contexts: Arc::new(Mutex::new(Vec::new())),
        })
    }

    struct FixtureParts {
        engine: PipelineDictationEngine,
        progress: Arc<RecordingProgress>,
        pcm: Arc<Mutex<Vec<u8>>>,
        recorder_stops: Arc<AtomicUsize>,
        archive_discards: Arc<AtomicUsize>,
        transcription_cancels: Arc<AtomicUsize>,
        transcription_starts: Arc<AtomicUsize>,
        polish_calls: Arc<AtomicUsize>,
        polish_contexts: Arc<Mutex<Vec<Arc<DictationContext>>>>,
    }

    fn fixture_engine(
        recorder_fails: bool,
        polish_result: Result<crate::ports::PolishOutput, BackendError>,
        finish_entered: Option<Arc<tokio::sync::Notify>>,
        finish_release: Option<Arc<tokio::sync::Notify>>,
    ) -> FixtureParts {
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let recorder_stops = Arc::new(AtomicUsize::new(0));
        let archive_discards = Arc::new(AtomicUsize::new(0));
        let archive = Arc::new(FixtureArchive {
            available: AtomicBool::new(true),
            discards: Arc::clone(&archive_discards),
            pcm: vec![1, 0, 2, 0],
        });
        let transcription_cancels = Arc::new(AtomicUsize::new(0));
        let transcription_starts = Arc::new(AtomicUsize::new(0));
        let polish_calls = Arc::new(AtomicUsize::new(0));
        let polish_contexts = Arc::new(Mutex::new(Vec::new()));
        let transcriber = Arc::new(FixtureTranscriber {
            session: Arc::new(FixtureTranscriptionSession {
                pcm: Arc::clone(&pcm),
                cancels: Arc::clone(&transcription_cancels),
                finish_entered,
                finish_release,
            }),
            starts: Arc::clone(&transcription_starts),
        });
        let engine = PipelineDictationEngine::new(
            Arc::new(FixtureRecorder {
                stops: Arc::clone(&recorder_stops),
                archive,
                fail: recorder_fails,
            }),
            transcriber,
            Arc::new(FixturePolisher {
                result: polish_result,
                calls: Arc::clone(&polish_calls),
                cancels: Arc::new(AtomicUsize::new(0)),
                contexts: Arc::clone(&polish_contexts),
            }),
        );
        FixtureParts {
            engine,
            progress: Arc::new(RecordingProgress::default()),
            pcm,
            recorder_stops,
            archive_discards,
            transcription_cancels,
            transcription_starts,
            polish_calls,
            polish_contexts,
        }
    }

    struct GatedPolisher {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicUsize>,
    }

    impl TextPolisher for GatedPolisher {
        fn polish(
            &self,
            _session_id: SessionId,
            _context: Arc<DictationContext>,
            _raw_text: String,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<crate::ports::PolishOutput, BackendError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let entered = Arc::clone(&self.entered);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                Ok(crate::ports::PolishOutput::text("polished final"))
            })
        }

        fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn pipeline_finish_waits_for_polish_and_returns_distinct_final_text() {
        let polish_entered = Arc::new(tokio::sync::Notify::new());
        let polish_release = Arc::new(tokio::sync::Notify::new());
        let polish_calls = Arc::new(AtomicUsize::new(0));
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let recorder_stops = Arc::new(AtomicUsize::new(0));
        let archive_discards = Arc::new(AtomicUsize::new(0));
        let transcriber = Arc::new(FixtureTranscriber {
            session: Arc::new(FixtureTranscriptionSession {
                pcm,
                cancels: Arc::new(AtomicUsize::new(0)),
                finish_entered: None,
                finish_release: None,
            }),
            starts: Arc::new(AtomicUsize::new(0)),
        });
        let engine = Arc::new(PipelineDictationEngine::new(
            Arc::new(FixtureRecorder {
                stops: recorder_stops,
                archive: Arc::new(FixtureArchive {
                    available: AtomicBool::new(true),
                    discards: archive_discards,
                    pcm: vec![1, 0, 2, 0],
                }),
                fail: false,
            }),
            transcriber,
            Arc::new(GatedPolisher {
                entered: Arc::clone(&polish_entered),
                release: Arc::clone(&polish_release),
                calls: Arc::clone(&polish_calls),
            }),
        ));
        let progress = Arc::new(RecordingProgress::default());
        let session_id = SessionId::new();
        let mut context = (*raw_dictation_context()).clone();
        context.polish.mode = crate::types::PolishMode::Formal;
        context.polish.style_system_prompt =
            crate::style_packs::default_style_system_prompt_for_mode(
                crate::types::PolishMode::Formal,
            );
        engine
            .start(session_id, Arc::new(context), progress.clone())
            .await
            .unwrap();

        let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
        let finishing = Arc::clone(&engine);
        tokio::spawn(async move {
            let _ = result_tx.send(finishing.finish(session_id, progress).await);
        });
        polish_entered.notified().await;
        assert_eq!(polish_calls.load(Ordering::Acquire), 1);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut result_rx)
                .await
                .is_err()
        );

        polish_release.notify_one();
        let result = result_rx.await.expect("finish task must return").unwrap();
        assert_eq!(result.raw_text, "raw text");
        assert_eq!(result.polished_text, "polished final");
    }

    #[tokio::test]
    async fn asr_diagnostics_distinguish_correction_introduced_repetition() {
        crate::testing::logging::start();
        let corrected =
            "Please submit the final report today. Please submit the final report today.";
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text(corrected)),
            None,
            None,
        );
        let mut context = DictationContext::default();
        context.correction_rules.push(crate::types::CorrectionRule {
            id: "diagnostic-fixture".into(),
            pattern: "raw text".into(),
            replacement: corrected.into(),
            enabled: true,
            created_at: String::new(),
            source: Default::default(),
        });
        let session_id = SessionId::new();
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();
        assert_eq!(result.asr_transcript.as_deref(), Some("raw text"));
        assert_eq!(result.raw_text, corrected);
        let logs = crate::testing::logging::matching(&format!(
            "[asr-diagnostics] session_id={session_id} "
        ));
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains(&format!(
            "asr_chars=8 asr_hash={} asr_near_duplicates=0",
            crate::output_cleaning::privacy_text_hash("raw text")
        )));
        assert!(logs[0].contains(&format!(
            "corrected_chars={} corrected_hash={} corrected_near_duplicates=1",
            corrected.chars().count(),
            crate::output_cleaning::privacy_text_hash(corrected)
        )));
        assert!(!logs[0].contains("raw text"));
        assert!(!logs[0].contains("Please submit"));
    }

    #[tokio::test]
    async fn provider_only_capture_uses_host_spawner_and_stable_mode_defers_start() {
        #[derive(Default)]
        struct QueuedSpawner(Mutex<Vec<BoxFuture<'static, ()>>>);
        impl crate::TaskSpawner for QueuedSpawner {
            fn spawn(&self, task: BoxFuture<'static, ()>) {
                self.0.lock().unwrap().push(task);
            }
        }
        for stable in [false, true] {
            let fixture = fixture_engine(
                false,
                Ok(crate::ports::PolishOutput::text("unused")),
                None,
                None,
            );
            let spawner = Arc::new(QueuedSpawner::default());
            let mut context = DictationContext::default();
            context.recording.transcribe_after_stop = stable;
            let session = Arc::new(fixture.engine)
                .start_transcription_with_progress(
                    spawner.clone(),
                    SessionId::new(),
                    Arc::new(context),
                    Arc::new(DiscardTextStream),
                    Arc::new(NoopRecordingProgress),
                )
                .await
                .unwrap();
            session.consume_pcm_chunk(&[1, 0, 2, 0]);
            assert_eq!(fixture.transcription_starts.load(Ordering::SeqCst), 0);
            assert!(fixture.pcm.lock().unwrap().is_empty());
            let tasks = std::mem::take(&mut *spawner.0.lock().unwrap());
            assert_eq!(tasks.len(), usize::from(!stable));
            for task in tasks {
                task.await;
            }
            if !stable {
                assert_eq!(fixture.transcription_starts.load(Ordering::SeqCst), 1);
                assert_eq!(*fixture.pcm.lock().unwrap(), vec![1, 0, 2, 0]);
            }
            assert_eq!(session.finish().await.unwrap().text, "raw text");
            assert_eq!(fixture.transcription_starts.load(Ordering::SeqCst), 1);
            assert_eq!(*fixture.pcm.lock().unwrap(), vec![1, 0, 2, 0]);
        }
    }

    #[tokio::test]
    async fn pipeline_streams_pcm_progress_and_terminal_deltas() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(&*fixture.pcm.lock().unwrap(), &[1, 0, 2, 0]);
        assert_eq!(fixture.recorder_stops.load(Ordering::SeqCst), 1);
        assert_eq!(result.raw_text, "raw text");
        assert_eq!(result.polished_text, "polished text");
        let events = fixture.progress.events.lock().unwrap();
        assert!(events.contains(&EngineProgress::RecordingLevel {
            elapsed_ms: 25,
            level: 1.0,
        }));
        assert!(
            events.contains(&EngineProgress::TranscriptDelta(TranscriptDelta {
                text: "raw text".to_string(),
                offset: 0,
                is_final: true,
            }))
        );
        assert!(events.contains(&EngineProgress::PolishDelta(PolishDelta {
            text: "polished text".to_string(),
            offset: 0,
            is_final: true,
        })));
    }

    #[tokio::test]
    async fn stable_pipeline_starts_asr_only_after_stop_and_emits_only_final_transcript() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("unused")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let mut context = (*raw_dictation_context()).clone();
        context.recording.transcribe_after_stop = true;

        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(fixture.transcription_starts.load(Ordering::Acquire), 0);
        assert!(fixture.pcm.lock().unwrap().is_empty());

        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(result.raw_text, "raw text");
        assert_eq!(fixture.transcription_starts.load(Ordering::Acquire), 1);
        assert_eq!(&*fixture.pcm.lock().unwrap(), &[1, 0, 2, 0]);
        let transcript_events: Vec<_> = fixture
            .progress
            .events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                EngineProgress::TranscriptDelta(delta) => Some(delta.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            transcript_events,
            vec![TranscriptDelta {
                text: "raw text".into(),
                offset: 0,
                is_final: true,
            }]
        );
    }

    #[tokio::test]
    async fn stable_pipeline_cancel_discards_pcm_without_starting_asr() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("unused")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let mut context = DictationContext::default();
        context.recording.transcribe_after_stop = true;
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();

        fixture.engine.cancel(session_id).await.unwrap();

        assert_eq!(fixture.transcription_starts.load(Ordering::Acquire), 0);
        assert_eq!(fixture.transcription_cancels.load(Ordering::Acquire), 0);
        assert_eq!(fixture.recorder_stops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn stable_voice_capture_defers_asr_until_the_shared_finish_handle() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("unused")),
            None,
            None,
        );
        let mut context = DictationContext::default();
        context.recording.transcribe_after_stop = true;
        let capture = fixture
            .engine
            .start_voice_capture(
                SessionId::new(),
                Arc::new(context),
                Arc::new(DiscardTextStream),
                Arc::new(NoopRecordingProgress),
                crate::CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(fixture.transcription_starts.load(Ordering::Acquire), 0);
        capture.recording.stop().await.unwrap();
        let output = capture.transcription.finish().await.unwrap();

        assert_eq!(output.text, "raw text");
        assert_eq!(fixture.transcription_starts.load(Ordering::Acquire), 1);
        assert_eq!(&*fixture.pcm.lock().unwrap(), &[1, 0, 2, 0]);
    }

    #[tokio::test]
    async fn empty_or_zero_pcm_never_finalizes_asr_and_can_still_be_cancelled() {
        for (chunk, attached) in [
            (Vec::new(), false),
            (vec![0; 640], false),
            (Vec::new(), true),
            (vec![0; 640], true),
        ] {
            let pcm = Arc::new(Mutex::new(Vec::new()));
            let starts = Arc::new(AtomicUsize::new(0));
            let cancels = Arc::new(AtomicUsize::new(0));
            let transcriber = Arc::new(FixtureTranscriber {
                session: Arc::new(FixtureTranscriptionSession {
                    pcm: pcm.clone(),
                    cancels: cancels.clone(),
                    finish_entered: None,
                    finish_release: None,
                }),
                starts: starts.clone(),
            });
            let prepared = transcriber
                .prepare(SessionId::new(), raw_dictation_context())
                .await
                .unwrap();
            let buffered = BufferedTranscriptionSession::new(
                prepared,
                Arc::new(DiscardTextStream),
                Arc::new(LimitRecordingProgress::default()),
            );
            if attached {
                buffered.attach().await.unwrap();
            }
            buffered.consume_pcm_chunk(&chunk);
            let error = buffered.finish().await.unwrap_err();
            assert_eq!(error.code, BackendErrorCode::InvalidArgument);
            assert_eq!(starts.load(Ordering::Acquire), usize::from(attached));
            buffered.cancel().await.unwrap();
            assert_eq!(cancels.load(Ordering::Acquire), usize::from(attached));
            assert_eq!(
                buffered.finish().await.unwrap_err().code,
                BackendErrorCode::Cancelled
            );
        }
    }

    #[tokio::test]
    async fn quiet_nonzero_pcm_is_not_rejected_as_silence() {
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let starts = Arc::new(AtomicUsize::new(0));
        let transcriber = Arc::new(FixtureTranscriber {
            session: Arc::new(FixtureTranscriptionSession {
                pcm: pcm.clone(),
                cancels: Arc::new(AtomicUsize::new(0)),
                finish_entered: None,
                finish_release: None,
            }),
            starts: starts.clone(),
        });
        let prepared = transcriber
            .prepare(SessionId::new(), raw_dictation_context())
            .await
            .unwrap();
        let buffered = BufferedTranscriptionSession::new(
            prepared,
            Arc::new(DiscardTextStream),
            Arc::new(LimitRecordingProgress::default()),
        );
        buffered.consume_pcm_chunk(&[1, 0]);
        assert_eq!(buffered.finish().await.unwrap().text, "raw text");
        assert_eq!(starts.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn buffered_pcm_limit_requests_one_stop_and_preserves_the_tail() {
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let starts = Arc::new(AtomicUsize::new(0));
        let progress = Arc::new(LimitRecordingProgress::default());
        let transcriber = Arc::new(FixtureTranscriber {
            session: Arc::new(FixtureTranscriptionSession {
                pcm: Arc::clone(&pcm),
                cancels: Arc::new(AtomicUsize::new(0)),
                finish_entered: None,
                finish_release: None,
            }),
            starts: Arc::clone(&starts),
        });
        let prepared = transcriber
            .prepare(SessionId::new(), raw_dictation_context())
            .await
            .unwrap();
        let buffered = BufferedTranscriptionSession::new_with_limit(
            prepared,
            Arc::new(DiscardTextStream),
            progress.clone(),
            4,
        );

        buffered.consume_pcm_chunk(&[1, 0]);
        buffered.consume_pcm_chunk(&[2, 0, 3, 0]);
        buffered.consume_pcm_chunk(&[4, 0]);

        assert_eq!(progress.limits.load(Ordering::SeqCst), 1);
        buffered.attach().await.unwrap().finish().await.unwrap();
        assert_eq!(&*pcm.lock().unwrap(), &[1, 0, 2, 0, 3, 0, 4, 0]);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pipeline_uses_the_updated_context_when_finalizing() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("translated text")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let initial = Arc::new(DictationContext::default());
        fixture
            .engine
            .start(session_id, initial.clone(), fixture.progress.clone())
            .await
            .unwrap();
        let mut updated = (*initial).clone();
        updated.polish.translation_active = true;
        let updated = Arc::new(updated);

        fixture
            .engine
            .update_context(session_id, updated.clone())
            .await
            .unwrap();
        fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        let contexts = fixture.polish_contexts.lock().unwrap();
        assert_eq!(contexts.as_slice(), &[updated]);
    }

    #[tokio::test]
    async fn pipeline_rejects_context_updates_after_finalization_starts() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            Some(Arc::clone(&entered)),
            Some(Arc::clone(&release)),
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();
        let finish = fixture.engine.finish(session_id, fixture.progress.clone());
        let finish_task = tokio::spawn(finish);
        entered.notified().await;

        let error = fixture
            .engine
            .update_context(session_id, Arc::new(DictationContext::default()))
            .await
            .expect_err("context must be frozen after finish starts");
        assert_eq!(error.code, BackendErrorCode::InvalidState);

        release.notify_one();
        finish_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn successful_transcription_discards_archive_when_debug_recording_is_disabled() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();

        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(fixture.archive_discards.load(Ordering::Acquire), 1);
        assert_eq!(result.has_audio_recording, Some(false));
    }

    #[tokio::test]
    async fn successful_transcription_preserves_archive_when_debug_recording_is_enabled() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let mut context = DictationContext::default();
        context.recording.archive_successful_recording = true;
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();

        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(fixture.archive_discards.load(Ordering::Acquire), 0);
        assert_eq!(result.has_audio_recording, Some(true));
    }

    #[tokio::test]
    async fn recorder_start_failure_never_starts_transcription() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("unused")),
            None,
            None,
        );
        let failing = fixture_engine(
            true,
            Ok(crate::ports::PolishOutput::text("unused")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let error = failing
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                failing.progress.clone(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Platform);
        assert_eq!(failing.transcription_starts.load(Ordering::SeqCst), 0);
        assert_eq!(failing.transcription_cancels.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.transcription_cancels.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn realtime_pipeline_preserves_pcm_before_during_and_after_asr_start() {
        let consumer = Arc::new(Mutex::new(None));
        let stops = Arc::new(AtomicUsize::new(0));
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let engine = Arc::new(PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::clone(&consumer),
                stops: Arc::clone(&stops),
            }),
            Arc::new(DelayedTranscriber {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                session: Arc::new(FixtureTranscriptionSession {
                    pcm: Arc::clone(&pcm),
                    cancels: Arc::new(AtomicUsize::new(0)),
                    finish_entered: None,
                    finish_release: None,
                }),
            }),
            noop_polisher(),
        ));
        let progress = Arc::new(RecordingProgress::default());
        let session_id = SessionId::new();
        let starting = tokio::spawn({
            let engine = Arc::clone(&engine);
            let progress = Arc::clone(&progress);
            async move {
                engine
                    .start(session_id, raw_dictation_context(), progress)
                    .await
            }
        });
        entered.notified().await;

        assert!(pcm.lock().unwrap().is_empty());
        consumer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .consume_pcm_chunk(&[2, 0]);
        release.notify_one();
        starting.await.unwrap().unwrap();
        consumer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .consume_pcm_chunk(&[3, 0]);

        engine.finish(session_id, progress).await.unwrap();

        assert_eq!(&*pcm.lock().unwrap(), &[1, 0, 2, 0, 3, 0]);
        assert_eq!(stops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn realtime_asr_start_failure_stops_recorder_and_removes_session() {
        let stops = Arc::new(AtomicUsize::new(0));
        let engine = PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::new(Mutex::new(None)),
                stops: Arc::clone(&stops),
            }),
            Arc::new(FailingTranscriber),
            noop_polisher(),
        );
        let error = engine
            .start(
                SessionId::new(),
                raw_dictation_context(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Provider);
        assert_eq!(stops.load(Ordering::Acquire), 1);
        assert!(engine.sessions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn provider_preparation_failure_stops_capture_and_removes_the_session() {
        let stops = Arc::new(AtomicUsize::new(0));
        let engine = PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::new(Mutex::new(None)),
                stops: Arc::clone(&stops),
            }),
            Arc::new(FailingPreparationTranscriber),
            noop_polisher(),
        );
        let error = engine
            .start(
                SessionId::new(),
                raw_dictation_context(),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Provider);
        assert_eq!(stops.load(Ordering::Acquire), 1);
        assert!(engine.sessions.lock().unwrap().is_empty());
    }

    struct ReadyFixturePreparation(Arc<FixtureTranscriptionSession>);

    impl PreparedTranscription for ReadyFixturePreparation {
        fn start(
            &self,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let session = self.0.clone();
            Box::pin(async move { Ok(session as Arc<dyn TranscriptionSession>) })
        }
    }

    struct BlockedPreparationTranscriber {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicUsize>,
        session: Arc<FixtureTranscriptionSession>,
    }

    impl TranscriptionEngine for BlockedPreparationTranscriber {
        fn prepare(
            self: Arc<Self>,
            _id: SessionId,
            _context: Arc<DictationContext>,
        ) -> BoxFuture<'static, Result<Arc<dyn PreparedTranscription>, BackendError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                self.entered.notify_one();
                self.release.notified().await;
                Ok(Arc::new(ReadyFixturePreparation(self.session.clone()))
                    as Arc<dyn PreparedTranscription>)
            })
        }

        fn start(
            &self,
            _id: SessionId,
            _context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            Box::pin(async { unreachable!("must use the prepared provider") })
        }
    }

    #[tokio::test]
    async fn capture_starts_before_slow_preparation_and_cancellation_releases_it_promptly() {
        for cancel in [false, true] {
            let consumer = Arc::new(Mutex::new(None));
            let stops = Arc::new(AtomicUsize::new(0));
            let pcm = Arc::new(Mutex::new(Vec::new()));
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let engine = Arc::new(PipelineDictationEngine::new(
                Arc::new(ExposedRecorder {
                    consumer: consumer.clone(),
                    stops: stops.clone(),
                }),
                Arc::new(BlockedPreparationTranscriber {
                    entered: entered.clone(),
                    release: release.clone(),
                    calls: calls.clone(),
                    session: Arc::new(FixtureTranscriptionSession {
                        pcm: pcm.clone(),
                        cancels: Arc::new(AtomicUsize::new(0)),
                        finish_entered: None,
                        finish_release: None,
                    }),
                }),
                noop_polisher(),
            ));
            let id = SessionId::new();
            let progress = Arc::new(RecordingProgress::default());
            let starting = tokio::spawn({
                let engine = engine.clone();
                let progress = progress.clone();
                async move { engine.start(id, raw_dictation_context(), progress).await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            assert!(
                consumer.lock().unwrap().is_some(),
                "microphone must precede prepare"
            );
            assert!(pcm.lock().unwrap().is_empty());
            consumer
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .consume_pcm_chunk(&[2, 0]);
            if cancel {
                tokio::time::timeout(std::time::Duration::from_secs(2), engine.cancel(id))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    stops.load(Ordering::Acquire),
                    1,
                    "cancel waited for provider preparation"
                );
            }
            release.notify_one();
            let result = starting.await.unwrap();
            if cancel {
                assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);
                assert!(pcm.lock().unwrap().is_empty());
            } else {
                result.unwrap();
                engine.finish(id, progress).await.unwrap();
                assert_eq!(&*pcm.lock().unwrap(), &[1, 0, 2, 0]);
            }
            assert_eq!(calls.load(Ordering::Acquire), 1);
            assert_eq!(stops.load(Ordering::Acquire), 1);
            assert!(engine.sessions.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn cancel_while_realtime_asr_starts_stops_recorder_and_cancels_late_session() {
        let consumer = Arc::new(Mutex::new(None));
        let stops = Arc::new(AtomicUsize::new(0));
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let cancels = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let engine = Arc::new(PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::clone(&consumer),
                stops: Arc::clone(&stops),
            }),
            Arc::new(DelayedTranscriber {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                session: Arc::new(FixtureTranscriptionSession {
                    pcm: Arc::clone(&pcm),
                    cancels: Arc::clone(&cancels),
                    finish_entered: None,
                    finish_release: None,
                }),
            }),
            noop_polisher(),
        ));
        let session_id = SessionId::new();
        let starting = tokio::spawn({
            let engine = Arc::clone(&engine);
            async move {
                engine
                    .start(
                        session_id,
                        raw_dictation_context(),
                        Arc::new(RecordingProgress::default()),
                    )
                    .await
            }
        });
        entered.notified().await;

        engine.cancel(session_id).await.unwrap();
        assert_eq!(stops.load(Ordering::Acquire), 1);
        release.notify_one();

        let error = starting.await.unwrap().unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert_eq!(cancels.load(Ordering::Acquire), 1);
        assert!(pcm.lock().unwrap().is_empty());
        assert!(engine.sessions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_model_loading_never_blocks_stop_or_cancel_of_the_microphone() {
        use crate::experimental_asr::ExperimentalAsrModel;
        for model in ExperimentalAsrModel::ALL {
            for cancel in [false, true] {
                let stops = Arc::new(AtomicUsize::new(0));
                let pcm = Arc::new(Mutex::new(Vec::new()));
                let cancels = Arc::new(AtomicUsize::new(0));
                let entered = Arc::new(tokio::sync::Notify::new());
                let release = Arc::new(tokio::sync::Notify::new());
                let engine = Arc::new(
                    PipelineDictationEngine::new(
                        Arc::new(ExposedRecorder {
                            consumer: Arc::new(Mutex::new(None)),
                            stops: Arc::clone(&stops),
                        }),
                        Arc::new(DelayedTranscriber {
                            entered: Arc::clone(&entered),
                            release: Arc::clone(&release),
                            session: Arc::new(FixtureTranscriptionSession {
                                pcm: Arc::clone(&pcm),
                                cancels: Arc::clone(&cancels),
                                finish_entered: None,
                                finish_release: None,
                            }),
                        }),
                        noop_polisher(),
                    )
                    .with_local_startup_spawner(Arc::new(crate::TokioTaskSpawner)),
                );
                let mut context = (*raw_dictation_context()).clone();
                context.asr.provider_type = model.provider_type().into();
                let id = SessionId::new();
                let progress = Arc::new(RecordingProgress::default());
                tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    engine.start(id, Arc::new(context), progress.clone()),
                )
                .await
                .expect("capture must be ready before model loading")
                .unwrap();
                entered.notified().await;
                if cancel {
                    engine.cancel(id).await.unwrap();
                    assert_eq!(stops.load(Ordering::Acquire), 1);
                    release.notify_one();
                    tokio::time::timeout(std::time::Duration::from_secs(1), async {
                        while cancels.load(Ordering::Acquire) == 0 {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    assert!(
                        pcm.lock().unwrap().is_empty(),
                        "cancelled capture must never be replayed"
                    );
                } else {
                    let finishing = tokio::spawn({
                        let engine = engine.clone();
                        async move { engine.finish(id, progress).await }
                    });
                    tokio::time::timeout(std::time::Duration::from_secs(1), async {
                        while stops.load(Ordering::Acquire) == 0 {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .expect("stop must release capture while model is still loading");
                    assert!(
                        !finishing.is_finished(),
                        "result still waits for real recognition"
                    );
                    release.notify_one();
                    assert_eq!(finishing.await.unwrap().unwrap().raw_text, "raw text");
                    assert_eq!(*pcm.lock().unwrap(), vec![1, 0]);
                }
            }
        }
    }

    #[tokio::test]
    async fn cancelled_queued_local_start_never_loads_the_model() {
        #[derive(Default)]
        struct Queue(Mutex<Vec<BoxFuture<'static, ()>>>);
        impl crate::TaskSpawner for Queue {
            fn spawn(&self, task: BoxFuture<'static, ()>) {
                self.0.lock().unwrap().push(task);
            }
        }
        let spawner = Arc::new(Queue::default());
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let engine = PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::new(Mutex::new(None)),
                stops: stops.clone(),
            }),
            Arc::new(FixtureTranscriber {
                starts: starts.clone(),
                session: Arc::new(FixtureTranscriptionSession {
                    pcm: Arc::new(Mutex::new(Vec::new())),
                    cancels: Arc::new(AtomicUsize::new(0)),
                    finish_entered: None,
                    finish_release: None,
                }),
            }),
            noop_polisher(),
        )
        .with_local_startup_spawner(spawner.clone());
        let mut context = (*raw_dictation_context()).clone();
        context.asr.provider_type = crate::experimental_asr::ExperimentalAsrModel::TextStream
            .provider_type()
            .into();
        let id = SessionId::new();
        engine
            .start(
                id,
                Arc::new(context),
                Arc::new(RecordingProgress::default()),
            )
            .await
            .unwrap();
        engine.cancel(id).await.unwrap();
        let tasks = std::mem::take(&mut *spawner.0.lock().unwrap());
        assert_eq!(tasks.len(), 1);
        for task in tasks {
            task.await;
        }
        assert_eq!(starts.load(Ordering::Acquire), 0);
        assert_eq!(stops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn background_local_start_failure_is_reported_not_silently_recorded_forever() {
        let engine = PipelineDictationEngine::new(
            Arc::new(ExposedRecorder {
                consumer: Arc::new(Mutex::new(None)),
                stops: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(FailingTranscriber),
            noop_polisher(),
        )
        .with_local_startup_spawner(Arc::new(crate::TokioTaskSpawner));
        let mut context = (*raw_dictation_context()).clone();
        context.asr.provider_type = crate::experimental_asr::ExperimentalAsrModel::SenseVoice
            .provider_type()
            .into();
        let id = SessionId::new();
        let progress = Arc::new(RecordingProgress::default());
        engine
            .start(id, Arc::new(context), progress.clone())
            .await
            .unwrap();
        // finish must expose the original provider failure, not a fabricated empty result.
        let error = engine.finish(id, progress).await.unwrap_err();
        assert_eq!(error.error.code, BackendErrorCode::Provider);
    }

    #[tokio::test]
    async fn provider_polish_failure_uses_raw_text_fallback() {
        let fixture = fixture_engine(
            false,
            Err(BackendError::new(
                BackendErrorCode::Provider,
                "fixture polish failure",
            )),
            None,
            None,
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();
        assert_eq!(result.raw_text, "raw text");
        assert_eq!(result.polished_text, "raw text");
    }

    #[tokio::test]
    async fn voice_activation_commands_are_cleaned_before_correction_and_polishing() {
        use crate::testing::{
            FixtureAudioRecorder, FixtureTextPolisher, FixtureTranscriptionEngine,
        };
        for (input, voice, finish, expected, original) in [
            (
                "开始，文字录入。 错字 完成，文字录入。",
                true,
                true,
                "正文",
                Some("错字"),
            ),
            (
                "开始文字录入 正文完成文字录入之前 完成文字录入",
                true,
                true,
                "正文完成文字录入之前",
                None,
            ),
            (
                "开始文字录入 正文完成文字录入",
                true,
                false,
                "正文完成文字录入",
                None,
            ),
            ("开始文字录入 完成文字录入", true, true, "", None),
            (
                "开始文字录入 正文 完成文字录入",
                false,
                true,
                "开始文字录入 正文 完成文字录入",
                None,
            ),
        ] {
            let polisher = Arc::new(FixtureTextPolisher::successful("润色结果"));
            let engine = PipelineDictationEngine::new(
                Arc::new(FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])),
                Arc::new(FixtureTranscriptionEngine::successful(input, 1_000)),
                polisher.clone(),
            );
            let mut context = DictationContext::default();
            context.polish.polish_skip_max_chars = 0;
            context.voice_activation =
                voice.then(|| crate::voice_activation::VoiceActivationTextPolicy {
                    wake_phrase: "开始文字录入".into(),
                    finish_phrase: finish.then(|| "完成文字录入".into()),
                });
            context.correction_rules.push(crate::types::CorrectionRule {
                id: "voice-cleanup".into(),
                pattern: "错字".into(),
                replacement: "正文".into(),
                enabled: true,
                created_at: String::new(),
                source: Default::default(),
            });
            let session_id = SessionId::new();
            let progress = Arc::new(RecordingProgress::default());
            engine
                .start(session_id, Arc::new(context), progress.clone())
                .await
                .unwrap();
            let result = engine.finish(session_id, progress.clone()).await.unwrap();
            assert_eq!(result.raw_text, expected);
            assert_eq!(result.asr_transcript.as_deref(), original);
            if expected.is_empty() {
                assert!(polisher.inputs().is_empty());
                assert!(result.polished_text.is_empty());
            } else {
                assert_eq!(polisher.inputs(), vec![expected]);
                assert_eq!(result.polished_text, "润色结果");
            }
            assert!(progress.events.lock().unwrap().iter().any(|event| matches!(event,
                EngineProgress::TranscriptDelta(delta) if delta.is_final && delta.text == expected)));
        }
    }

    #[tokio::test]
    async fn voice_activation_polish_failure_retains_only_spoken_body() {
        use crate::testing::{
            FixtureAudioRecorder, FixtureTextPolisher, FixtureTranscriptionEngine,
        };
        for policy in [PolishFailurePolicy::Fail, PolishFailurePolicy::UseRawText] {
            let polisher = Arc::new(FixtureTextPolisher::failing(BackendError::new(
                BackendErrorCode::Provider,
                "fixture",
            )));
            let engine = PipelineDictationEngine::new(
                Arc::new(FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])),
                Arc::new(FixtureTranscriptionEngine::successful(
                    "开始语音录入 正文 完成语音录入",
                    1000,
                )),
                polisher.clone(),
            )
            .with_polish_failure_policy(policy);
            let context = DictationContext {
                voice_activation: Some(crate::voice_activation::VoiceActivationTextPolicy {
                    wake_phrase: "开始语音录入".into(),
                    finish_phrase: Some("完成语音录入".into()),
                }),
                ..Default::default()
            };
            let id = SessionId::new();
            let progress = Arc::new(RecordingProgress::default());
            engine
                .start(id, Arc::new(context), progress.clone())
                .await
                .unwrap();
            let result = engine.finish(id, progress).await;
            assert_eq!(polisher.inputs(), vec!["正文"]);
            match result {
                Ok(output) => {
                    assert_eq!(output.raw_text, "正文");
                    assert_eq!(output.polished_text, "正文");
                }
                Err(failure) => assert_eq!(failure.raw_text.as_deref(), Some("正文")),
            }
        }
    }

    #[tokio::test]
    async fn short_transcript_under_threshold_skips_polish() {
        // Issue #1032: "raw text" has 7 non-whitespace characters, within a limit of 8.
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let mut context = DictationContext::default();
        context.polish.polish_skip_max_chars = 8;
        let session_id = SessionId::new();
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();
        assert_eq!(fixture.polish_calls.load(Ordering::SeqCst), 0);
        assert_eq!(result.polished_text, "raw text");
        assert!(!result.polish_failed);
    }

    #[tokio::test]
    async fn transcript_over_threshold_still_polishes() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let mut context = DictationContext::default();
        // "raw text" has 7 non-whitespace characters, above a limit of 6.
        context.polish.polish_skip_max_chars = 6;
        let session_id = SessionId::new();
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();
        assert_eq!(fixture.polish_calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.polished_text, "polished text");
    }

    #[tokio::test]
    async fn duplicate_start_keeps_the_original_pipeline_session() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            None,
            None,
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();

        let error = fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .expect_err("duplicate session must be rejected");
        assert_eq!(error.code, BackendErrorCode::Busy);

        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .expect("the original pipeline session must remain active");
        assert_eq!(result.polished_text, "polished text");
    }

    #[tokio::test]
    async fn qa_handoff_preserves_question_without_calling_the_dictation_polisher() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("must not be used")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let context = Arc::new(DictationContext::default());
        fixture
            .engine
            .start(session_id, Arc::clone(&context), fixture.progress.clone())
            .await
            .unwrap();
        fixture
            .engine
            .update_context(
                session_id,
                Arc::new(context.with_output_target(DictationOutputTarget::Qa)),
            )
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();
        assert_eq!(result.polished_text, "raw text");
        assert_eq!(fixture.polish_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn builtin_raw_mode_bypasses_the_polisher() {
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("must not be used")),
            None,
            None,
        );
        let session_id = SessionId::new();
        let mut context = DictationContext::default();
        context.polish.mode = crate::types::PolishMode::Raw;
        context.polish.style_system_prompt =
            crate::style_packs::default_style_system_prompt_for_mode(crate::types::PolishMode::Raw);
        fixture
            .engine
            .start(session_id, Arc::new(context), fixture.progress.clone())
            .await
            .unwrap();
        let result = fixture
            .engine
            .finish(session_id, fixture.progress.clone())
            .await
            .unwrap();

        assert_eq!(result.polished_text, "raw text");
        assert_eq!(fixture.polish_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_while_asr_finishes_rejects_late_terminal_progress() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let fixture = fixture_engine(
            false,
            Ok(crate::ports::PolishOutput::text("polished text")),
            Some(Arc::clone(&entered)),
            Some(Arc::clone(&release)),
        );
        let session_id = SessionId::new();
        fixture
            .engine
            .start(
                session_id,
                Arc::new(DictationContext::default()),
                fixture.progress.clone(),
            )
            .await
            .unwrap();
        let finish = fixture.engine.finish(session_id, fixture.progress.clone());
        let finish_task = tokio::spawn(finish);
        entered.notified().await;
        fixture.engine.cancel(session_id).await.unwrap();
        release.notify_one();

        let error = finish_task.await.unwrap().unwrap_err();
        assert_eq!(error.error.code, BackendErrorCode::Cancelled);
        assert_eq!(fixture.transcription_cancels.load(Ordering::SeqCst), 1);
        let events = fixture.progress.events.lock().unwrap();
        assert!(!events.iter().any(|event| matches!(
            event,
            EngineProgress::TranscriptDelta(TranscriptDelta { is_final: true, .. })
        )));
    }

    #[tokio::test]
    async fn retryable_asr_failure_reuses_the_frozen_archive_once() {
        let archive = Arc::new(FixtureArchive {
            available: AtomicBool::new(true),
            discards: Arc::new(AtomicUsize::new(0)),
            pcm: vec![1, 0, 2, 0],
        });
        let starts = Arc::new(AtomicUsize::new(0));
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let transcriber = Arc::new(RetryTranscriber {
            outputs: Arc::new(Mutex::new(VecDeque::from([
                Err(BackendError::new(BackendErrorCode::Provider, "temporary").retryable(true)),
                Ok(TranscriptOutput {
                    text: "retry success".into(),
                    duration_ms: 25,
                }),
            ]))),
            starts: Arc::clone(&starts),
            pcm: Arc::clone(&pcm),
        });
        let engine = PipelineDictationEngine::new(
            Arc::new(FixtureRecorder {
                stops: Arc::new(AtomicUsize::new(0)),
                archive,
                fail: false,
            }),
            transcriber,
            Arc::new(FixturePolisher {
                result: Ok(crate::ports::PolishOutput::text("unused")),
                calls: Arc::new(AtomicUsize::new(0)),
                cancels: Arc::new(AtomicUsize::new(0)),
                contexts: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        let progress = Arc::new(RecordingProgress::default());
        let session_id = SessionId::new();
        let mut context = DictationContext::default();
        context.polish.mode = crate::types::PolishMode::Raw;
        context.polish.style_system_prompt =
            crate::style_packs::default_style_system_prompt_for_mode(crate::types::PolishMode::Raw);
        context.recording.transcribe_after_stop = true;
        engine
            .start(session_id, Arc::new(context), progress.clone())
            .await
            .unwrap();
        assert_eq!(starts.load(Ordering::Acquire), 0);

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "retry success");
        assert_eq!(starts.load(Ordering::Acquire), 2);
        assert_eq!(
            pcm.lock().unwrap().as_slice(),
            &[vec![1, 0, 2, 0], vec![1, 0, 2, 0]]
        );
    }

    fn retry_test_engine(
        outputs: Vec<Result<TranscriptOutput, BackendError>>,
        archive_available: bool,
    ) -> (
        PipelineDictationEngine,
        Arc<RecordingProgress>,
        Arc<AtomicUsize>,
    ) {
        let archive = Arc::new(FixtureArchive {
            available: AtomicBool::new(archive_available),
            discards: Arc::new(AtomicUsize::new(0)),
            pcm: vec![1, 0, 2, 0],
        });
        let starts = Arc::new(AtomicUsize::new(0));
        let engine = PipelineDictationEngine::new(
            Arc::new(FixtureRecorder {
                stops: Arc::new(AtomicUsize::new(0)),
                archive,
                fail: false,
            }),
            Arc::new(RetryTranscriber {
                outputs: Arc::new(Mutex::new(outputs.into())),
                starts: Arc::clone(&starts),
                pcm: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(FixturePolisher {
                result: Ok(crate::ports::PolishOutput::text("unused")),
                calls: Arc::new(AtomicUsize::new(0)),
                cancels: Arc::new(AtomicUsize::new(0)),
                contexts: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        (engine, Arc::new(RecordingProgress::default()), starts)
    }

    fn raw_dictation_context() -> Arc<DictationContext> {
        let mut context = DictationContext::default();
        context.polish.mode = crate::types::PolishMode::Raw;
        context.polish.style_system_prompt =
            crate::style_packs::default_style_system_prompt_for_mode(crate::types::PolishMode::Raw);
        Arc::new(context)
    }

    /// Answers each start from the queue of the provider the session was
    /// started with, and records that provider — so a test can tell the cloud
    /// call, its retries and the local fallback apart.
    struct RoutedTranscriber {
        outputs: Mutex<
            std::collections::HashMap<String, VecDeque<Result<TranscriptOutput, BackendError>>>,
        >,
        started: Arc<Mutex<Vec<String>>>,
        /// How long each provider's finish takes; providers not listed answer at once.
        delays: std::collections::HashMap<String, std::time::Duration>,
    }

    struct DelayedTranscriptionSession {
        output: Result<TranscriptOutput, BackendError>,
        delay: std::time::Duration,
        cancels: Arc<AtomicUsize>,
    }

    impl AudioConsumer for DelayedTranscriptionSession {
        fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
    }

    impl TranscriptionSession for DelayedTranscriptionSession {
        fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
            let output = self.output.clone();
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                output
            })
        }

        fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
            self.cancels.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    impl TranscriptionEngine for RoutedTranscriber {
        fn start(
            &self,
            _session_id: SessionId,
            context: Arc<DictationContext>,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
            let provider = context.asr.provider_type.clone();
            self.started.lock().unwrap().push(provider.clone());
            let output = self
                .outputs
                .lock()
                .unwrap()
                .get_mut(&provider)
                .and_then(VecDeque::pop_front)
                .unwrap_or_else(|| panic!("no fixture output left for {provider}"));
            if let Some(delay) = self.delays.get(&provider).copied() {
                return Box::pin(async move {
                    Ok(Arc::new(DelayedTranscriptionSession {
                        output,
                        delay,
                        cancels: Arc::new(AtomicUsize::new(0)),
                    }) as Arc<dyn TranscriptionSession>)
                });
            }
            Box::pin(async move {
                Ok(Arc::new(RetryTranscriptionSession {
                    output,
                    pcm: Arc::new(Mutex::new(Vec::new())),
                }) as Arc<dyn TranscriptionSession>)
            })
        }
    }

    const CLOUD: &str = "volcengine";
    const LOCAL: &str = "sherpa-onnx-local";

    /// A raw-mode session on CLOUD whose fallback (when set) is LOCAL.
    fn fallback_test_engine(
        cloud: Vec<Result<TranscriptOutput, BackendError>>,
        local: Vec<Result<TranscriptOutput, BackendError>>,
    ) -> (
        PipelineDictationEngine,
        Arc<RecordingProgress>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let (engine, progress, started, _) = fallback_polish_test_engine(
            cloud,
            local,
            Ok(crate::ports::PolishOutput::text("unused")),
        );
        (engine, progress, started)
    }

    /// The engine, its progress sink, the providers started in order and the
    /// polisher's call count.
    type FallbackTestEngine = (
        PipelineDictationEngine,
        Arc<RecordingProgress>,
        Arc<Mutex<Vec<String>>>,
        Arc<AtomicUsize>,
    );

    /// fallback_test_engine() with the polisher's result chosen, also
    /// returning how many times it was called.
    fn fallback_polish_test_engine(
        cloud: Vec<Result<TranscriptOutput, BackendError>>,
        local: Vec<Result<TranscriptOutput, BackendError>>,
        polish: Result<crate::ports::PolishOutput, BackendError>,
    ) -> FallbackTestEngine {
        racing_test_engine(&[], cloud, local, polish)
    }

    /// fallback_polish_test_engine() with each listed provider taking the
    /// given time to finish.
    fn racing_test_engine(
        delays: &[(&str, u64)],
        cloud: Vec<Result<TranscriptOutput, BackendError>>,
        local: Vec<Result<TranscriptOutput, BackendError>>,
        polish: Result<crate::ports::PolishOutput, BackendError>,
    ) -> FallbackTestEngine {
        let started = Arc::new(Mutex::new(Vec::new()));
        let polish_calls = Arc::new(AtomicUsize::new(0));
        let engine = PipelineDictationEngine::new(
            Arc::new(FixtureRecorder {
                stops: Arc::new(AtomicUsize::new(0)),
                archive: Arc::new(FixtureArchive {
                    available: AtomicBool::new(true),
                    discards: Arc::new(AtomicUsize::new(0)),
                    pcm: vec![1, 0, 2, 0],
                }),
                fail: false,
            }),
            Arc::new(RoutedTranscriber {
                outputs: Mutex::new(std::collections::HashMap::from([
                    (CLOUD.to_string(), cloud.into()),
                    (LOCAL.to_string(), local.into()),
                ])),
                started: Arc::clone(&started),
                delays: delays
                    .iter()
                    .map(|(provider, ms)| {
                        (provider.to_string(), std::time::Duration::from_millis(*ms))
                    })
                    .collect(),
            }),
            Arc::new(FixturePolisher {
                result: polish,
                calls: Arc::clone(&polish_calls),
                cancels: Arc::new(AtomicUsize::new(0)),
                contexts: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        (
            engine,
            Arc::new(RecordingProgress::default()),
            started,
            polish_calls,
        )
    }

    /// A polishing session (not raw mode) on CLOUD with the LOCAL fallback.
    fn polishing_fallback_context() -> Arc<DictationContext> {
        let mut context = DictationContext {
            asr: crate::dictation_context::ProviderInvocation::new(CLOUD, CLOUD),
            asr_fallback: Some(crate::dictation_context::ProviderInvocation::new(
                LOCAL, LOCAL,
            )),
            ..DictationContext::default()
        };
        assert!(context.uses_llm_polisher(), "fixture must exercise the polisher");
        context.polish.polish_skip_max_chars = 0;
        Arc::new(context)
    }

    #[tokio::test]
    async fn realtime_cloud_start_failure_preserves_local_fallback_and_original_error() {
        struct FailingCloudStart(Arc<dyn TranscriptionEngine>);
        impl TranscriptionEngine for FailingCloudStart {
            fn start(
                &self,
                id: SessionId,
                context: Arc<DictationContext>,
                partials: Arc<dyn TextStreamSink>,
            ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>>
            {
                if context.asr.provider_type == CLOUD {
                    Box::pin(async {
                        Err(BackendError::new(
                            BackendErrorCode::Provider,
                            "cloud connection failed",
                        ))
                    })
                } else {
                    self.0.start(id, context, partials)
                }
            }
        }
        for local_succeeds in [true, false] {
            let local = if local_succeeds {
                transcript("local transcript")
            } else {
                Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "model missing",
                ))
            };
            let (mut engine, progress, _) = fallback_test_engine(vec![], vec![local]);
            engine.transcription = Arc::new(FailingCloudStart(Arc::clone(&engine.transcription)));
            let id = SessionId::new();
            engine
                .start(id, fallback_context(), progress.clone())
                .await
                .unwrap();
            let result = engine.finish(id, progress).await;
            if local_succeeds {
                assert_eq!(result.unwrap().raw_text, "local transcript");
            } else {
                assert_eq!(result.unwrap_err().error.message, "cloud connection failed");
            }
            assert!(engine.sessions.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn local_recognition_preserves_translation_and_reports_translation_failure() {
        for fallback in [false, true] {
            for polish_succeeds in [false, true] {
                let polish = if polish_succeeds {
                    Ok(crate::ports::PolishOutput::text("Hello"))
                } else {
                    Err(BackendError::new(BackendErrorCode::Provider, "LLM offline")
                        .retryable(true))
                };
                let (engine, progress, _, calls) = fallback_polish_test_engine(
                    if fallback {
                        vec![Err(BackendError::new(
                            BackendErrorCode::Provider,
                            "ASR quota exhausted",
                        ))]
                    } else {
                        vec![]
                    },
                    vec![transcript("你好")],
                    polish,
                );
                let mut context = if fallback {
                    (*polishing_fallback_context()).clone()
                } else {
                    (*local_polishing_context()).clone()
                };
                context.polish.translation_active = true;
                context.polish.translation_target_language = "English".into();
                let id = SessionId::new();
                engine
                    .start(id, Arc::new(context), progress.clone())
                    .await
                    .unwrap();
                let result = engine.finish(id, progress).await.unwrap();
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                assert_eq!(
                    result.polished_text,
                    if polish_succeeds { "Hello" } else { "你好" }
                );
                assert_eq!(result.polish_failed, !polish_succeeds);
            }
        }
    }

    #[tokio::test]
    async fn a_local_fallback_skips_polish_and_reports_no_failure() {
        let (engine, progress, started, polish_calls) = fallback_polish_test_engine(
            vec![Err(BackendError::new(
                BackendErrorCode::Provider,
                "offline",
            )
            .retryable(true))],
            vec![transcript("本地识别")],
            Err(BackendError::new(BackendErrorCode::Provider, "offline")),
        );
        let session_id = SessionId::new();
        engine.start(session_id, polishing_fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "本地识别");
        assert_eq!(result.polished_text, "本地识别");
        assert!(!result.polish_failed);
        // The cloud was unreachable a moment ago: no polish request at all.
        assert_eq!(polish_calls.load(Ordering::SeqCst), 0);
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    fn local_polishing_context() -> Arc<DictationContext> {
        let mut context = (*polishing_fallback_context()).clone();
        context.asr = crate::dictation_context::ProviderInvocation::new(LOCAL, LOCAL);
        context.asr_fallback = None;
        Arc::new(context)
    }

    #[tokio::test]
    async fn a_rejected_polish_after_local_recognition_is_still_reported() {
        let (engine, progress, _, polish_calls) = fallback_polish_test_engine(
            vec![],
            vec![transcript("本地识别")],
            Err(BackendError::new(
                BackendErrorCode::Provider,
                "invalid api key",
            )),
        );
        let session_id = SessionId::new();
        engine.start(session_id, local_polishing_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.polished_text, "本地识别");
        assert!(result.polish_failed, "a rejected key must not be hidden as offline");
        assert_eq!(polish_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_offline_polish_after_local_recognition_is_raw_output_not_a_failure() {
        let (engine, progress, started, polish_calls) = fallback_polish_test_engine(
            vec![],
            vec![transcript("本地识别")],
            Err(BackendError::new(BackendErrorCode::Provider, "offline").retryable(true)),
        );
        let mut context = (*polishing_fallback_context()).clone();
        context.asr = crate::dictation_context::ProviderInvocation::new(LOCAL, LOCAL);
        context.asr_fallback = None;
        let session_id = SessionId::new();
        engine.start(session_id, Arc::new(context), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.polished_text, "本地识别");
        assert!(!result.polish_failed);
        assert_eq!(polish_calls.load(Ordering::SeqCst), 1);
        assert_eq!(started.lock().unwrap().as_slice(), &[LOCAL]);
    }

    #[tokio::test]
    async fn a_failed_polish_after_cloud_recognition_is_still_reported() {
        let (engine, progress, _, polish_calls) = fallback_polish_test_engine(
            vec![transcript("云端识别")],
            vec![],
            Err(BackendError::new(BackendErrorCode::Provider, "offline")),
        );
        let session_id = SessionId::new();
        engine.start(session_id, polishing_fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.polished_text, "云端识别");
        assert!(result.polish_failed);
        assert_eq!(polish_calls.load(Ordering::SeqCst), 1);
    }

    fn fallback_context() -> Arc<DictationContext> {
        let mut context = (*raw_dictation_context()).clone();
        context.asr = crate::dictation_context::ProviderInvocation::new(CLOUD, CLOUD);
        context.asr_fallback = Some(crate::dictation_context::ProviderInvocation::new(
            LOCAL, LOCAL,
        ));
        Arc::new(context)
    }

    fn transcript(text: &str) -> Result<TranscriptOutput, BackendError> {
        Ok(TranscriptOutput {
            text: text.into(),
            duration_ms: 25,
        })
    }

    #[tokio::test]
    async fn failed_cloud_asr_goes_straight_to_the_local_fallback() {
        let (engine, progress, started) = fallback_test_engine(
            vec![Err(BackendError::new(
                BackendErrorCode::Provider,
                "offline",
            )
            .retryable(true))],
            vec![transcript("本地识别")],
        );
        let session_id = SessionId::new();
        engine.start(session_id, fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "本地识别");
        // No cloud retry (and no backoff) before the local model runs.
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    #[tokio::test]
    async fn a_non_retryable_cloud_error_also_falls_back() {
        let (engine, progress, started) = fallback_test_engine(
            vec![Err(BackendError::new(
                BackendErrorCode::Provider,
                "quota exhausted",
            ))],
            vec![transcript("本地识别")],
        );
        let session_id = SessionId::new();
        engine.start(session_id, fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "本地识别");
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    #[tokio::test]
    async fn a_failed_local_fallback_keeps_the_cloud_retry_and_the_cloud_error() {
        let temporary =
            || Err(BackendError::new(BackendErrorCode::Provider, "offline").retryable(true));
        let (engine, progress, started) = fallback_test_engine(
            vec![temporary(), temporary(), temporary()],
            vec![Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "model not downloaded",
            ))],
        );
        let session_id = SessionId::new();
        engine.start(session_id, fallback_context(), progress.clone()).await.unwrap();

        let failure = engine.finish(session_id, progress).await.unwrap_err();

        // The cloud's own error, not the local model's.
        assert_eq!(failure.error.code, BackendErrorCode::Provider);
        assert_eq!(failure.error.message, "offline");
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL, CLOUD, CLOUD]);
    }

    #[tokio::test]
    async fn without_a_fallback_the_cloud_retry_is_unchanged() {
        let temporary =
            || Err(BackendError::new(BackendErrorCode::Provider, "offline").retryable(true));
        let (engine, progress, started) =
            fallback_test_engine(vec![temporary(), transcript("cloud retry")], vec![]);
        let mut context = (*fallback_context()).clone();
        context.asr_fallback = None;
        let session_id = SessionId::new();
        engine.start(session_id, Arc::new(context), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "cloud retry");
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, CLOUD]);
    }

    #[tokio::test]
    async fn the_local_standby_wins_when_it_answers_before_a_slow_cloud() {
        let (engine, progress, started, polish_calls) = racing_test_engine(
            &[(CLOUD, 2_000), (LOCAL, 20)],
            vec![transcript("云端识别")],
            vec![transcript("本地识别")],
            Ok(crate::ports::PolishOutput::text("润色后")),
        );
        let session_id = SessionId::new();
        engine.start(session_id, polishing_fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "本地识别");
        // Only slower, not unreachable: the transcript is still polished.
        assert_eq!(result.polished_text, "润色后");
        assert_eq!(polish_calls.load(Ordering::SeqCst), 1);
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    #[tokio::test]
    async fn the_cloud_wins_when_it_answers_before_the_local_standby() {
        let (engine, progress, _, _) = racing_test_engine(
            &[(CLOUD, 20), (LOCAL, 2_000)],
            vec![transcript("云端识别")],
            vec![transcript("本地识别")],
            Ok(crate::ports::PolishOutput::text("unused")),
        );
        let session_id = SessionId::new();
        engine.start(session_id, fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "云端识别");
    }

    #[tokio::test]
    async fn a_failed_local_standby_leaves_a_slow_cloud_to_finish() {
        let (engine, progress, _, _) = racing_test_engine(
            &[(CLOUD, 200)],
            vec![transcript("云端识别")],
            vec![Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "model not downloaded",
            ))],
            Ok(crate::ports::PolishOutput::text("unused")),
        );
        let session_id = SessionId::new();
        engine.start(session_id, fallback_context(), progress.clone()).await.unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "云端识别");
    }

    #[tokio::test]
    async fn an_empty_cloud_result_uses_a_valid_local_standby() {
        let (engine, progress, started, polish_calls) = racing_test_engine(
            &[(CLOUD, 20), (LOCAL, 200)],
            vec![transcript(" \n\t")],
            vec![transcript("本地识别")],
            Ok(crate::ports::PolishOutput::text("unused")),
        );
        let session_id = SessionId::new();
        engine
            .start(session_id, polishing_fallback_context(), progress.clone())
            .await
            .unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "本地识别");
        assert_eq!(polish_calls.load(Ordering::SeqCst), 0);
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    #[tokio::test]
    async fn an_empty_local_standby_leaves_a_slow_cloud_to_finish() {
        let (engine, progress, started, _) = racing_test_engine(
            &[(CLOUD, 200), (LOCAL, 20)],
            vec![transcript("云端识别")],
            vec![transcript(" \n\t")],
            Ok(crate::ports::PolishOutput::text("unused")),
        );
        let session_id = SessionId::new();
        engine
            .start(session_id, fallback_context(), progress.clone())
            .await
            .unwrap();

        let result = engine.finish(session_id, progress).await.unwrap();

        assert_eq!(result.raw_text, "云端识别");
        assert_eq!(started.lock().unwrap().as_slice(), &[CLOUD, LOCAL]);
    }

    #[tokio::test]
    async fn retryable_asr_failure_stops_after_two_retries() {
        let temporary =
            || Err(BackendError::new(BackendErrorCode::Provider, "temporary").retryable(true));
        let (engine, progress, starts) =
            retry_test_engine(vec![temporary(), temporary(), temporary()], true);
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();

        let failure = engine.finish(session_id, progress).await.unwrap_err();

        assert_eq!(failure.error.code, BackendErrorCode::Provider);
        assert_eq!(starts.load(Ordering::Acquire), 3);
    }

    #[tokio::test]
    async fn terminal_empty_and_missing_archive_never_retry() {
        let terminal = Err(BackendError::new(BackendErrorCode::Provider, "terminal"));
        let (engine, progress, starts) = retry_test_engine(vec![terminal], true);
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();
        assert_eq!(
            engine
                .finish(session_id, progress)
                .await
                .unwrap_err()
                .error
                .code,
            BackendErrorCode::Provider
        );
        assert_eq!(starts.load(Ordering::Acquire), 1);

        let (engine, progress, starts) = retry_test_engine(
            vec![Ok(TranscriptOutput {
                text: String::new(),
                duration_ms: 25,
            })],
            true,
        );
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();
        assert!(engine
            .finish(session_id, progress)
            .await
            .unwrap()
            .raw_text
            .is_empty());
        assert_eq!(starts.load(Ordering::Acquire), 1);

        let retryable =
            Err(BackendError::new(BackendErrorCode::Provider, "temporary").retryable(true));
        let (engine, progress, starts) = retry_test_engine(vec![retryable], false);
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();
        assert_eq!(
            engine
                .finish(session_id, progress)
                .await
                .unwrap_err()
                .error
                .code,
            BackendErrorCode::Provider
        );
        assert_eq!(starts.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cancellation_during_asr_retry_backoff_starts_no_retry_session() {
        let retryable =
            Err(BackendError::new(BackendErrorCode::Provider, "temporary").retryable(true));
        let (engine, progress, starts) = retry_test_engine(vec![retryable], true);
        let engine = Arc::new(engine);
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();
        let finishing = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move { engine.finish(session_id, progress).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        engine.cancel(session_id).await.unwrap();

        let failure = finishing.await.unwrap().unwrap_err();
        assert_eq!(failure.error.code, BackendErrorCode::Cancelled);
        assert_eq!(starts.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cancellation_while_retry_asr_starts_cancels_the_late_resource_without_feeding_it() {
        struct RetryThenDelayedStart {
            starts: Arc<AtomicUsize>,
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            session: Arc<FixtureTranscriptionSession>,
            pcm: Arc<Mutex<Vec<Vec<u8>>>>,
        }
        impl TranscriptionEngine for RetryThenDelayedStart {
            fn start(
                &self,
                _: SessionId,
                _: Arc<DictationContext>,
                _: Arc<dyn TextStreamSink>,
            ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>>
            {
                if self.starts.fetch_add(1, Ordering::AcqRel) == 0 {
                    let pcm = Arc::clone(&self.pcm);
                    return Box::pin(async move {
                        Ok(Arc::new(RetryTranscriptionSession {
                            output: Err(BackendError::new(BackendErrorCode::Provider, "temporary")
                                .retryable(true)),
                            pcm,
                        }) as Arc<dyn TranscriptionSession>)
                    });
                }
                let entered = self.entered.clone();
                let release = self.release.clone();
                let session = self.session.clone();
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(session as Arc<dyn TranscriptionSession>)
                })
            }
        }
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let late = Arc::new(FixtureTranscriptionSession {
            pcm: Arc::new(Mutex::new(Vec::new())),
            cancels: Arc::new(AtomicUsize::new(0)),
            finish_entered: None,
            finish_release: None,
        });
        let (mut engine, progress, _) = retry_test_engine(Vec::new(), true);
        let pcm = Arc::new(Mutex::new(Vec::new()));
        engine.transcription = Arc::new(RetryThenDelayedStart {
            starts: Arc::new(AtomicUsize::new(0)),
            entered: entered.clone(),
            release: release.clone(),
            session: late.clone(),
            pcm,
        });
        let session_id = SessionId::new();
        engine
            .start(session_id, raw_dictation_context(), progress.clone())
            .await
            .unwrap();
        let engine = Arc::new(engine);
        let finishing = tokio::spawn({
            let engine = engine.clone();
            async move { engine.finish(session_id, progress).await }
        });
        entered.notified().await;
        engine.cancel(session_id).await.unwrap();
        release.notify_one();
        let failure = finishing.await.unwrap().unwrap_err();
        assert_eq!(failure.error.code, BackendErrorCode::Cancelled);
        assert_eq!(
            late.cancels.load(Ordering::Acquire),
            1,
            "late ASR resource needs its own cleanup"
        );
        assert!(
            late.pcm.lock().unwrap().is_empty(),
            "cancelled speech must not be replayed to a new ASR"
        );
        assert!(engine.sessions.lock().unwrap().is_empty());
    }
}
