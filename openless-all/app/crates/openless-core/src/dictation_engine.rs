//! Shared dictation pipeline orchestration.
//!
//! The pipeline owns provider/recorder ordering, terminal progress events and
//! cancellation guards. Native hosts only implement the narrow ports from
//! [`crate::ports`]; they never duplicate session or fallback decisions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use futures_util::future::BoxFuture;

use crate::dictation_context::{DictationContext, DictationOutputTarget};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    ActiveRecording, AudioCapture, AudioConsumer, AudioRecorder, CapturedPcm, DictationEngine,
    EngineFailure, EngineFailureStage, EngineProgress, EngineProgressSink, EngineResult,
    EngineStage, PreparedTranscription, RecordingArchive, RecordingProgressSink, TextPolisher,
    TextStreamChunk, TextStreamSink, TranscriptionEngine, TranscriptionSession, VoiceCapture,
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
    sessions: Arc<Mutex<HashMap<SessionId, Arc<PipelineSession>>>>,
}

struct PipelineSession {
    context: RwLock<Arc<DictationContext>>,
    cancelled: AtomicBool,
    finishing: AtomicBool,
    transcription_finished: AtomicBool,
    transcription_cancelled: AtomicBool,
    polishing: AtomicBool,
    polisher_cancelled: AtomicBool,
    recording_fault: Mutex<Option<BackendError>>,
    resources: Mutex<PipelineResources>,
}

#[derive(Default)]
struct PipelineResources {
    recording: Option<Box<dyn ActiveRecording>>,
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
            recording_fault: Mutex::new(None),
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
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_polish_failure_policy(mut self, policy: PolishFailurePolicy) -> Self {
        self.polish_failure_policy = policy;
        self
    }
}

impl DictationEngine for PipelineDictationEngine {
    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let recorder = Arc::clone(&self.recorder);
        let transcription_engine = Arc::clone(&self.transcription);
        let sessions = Arc::clone(&self.sessions);
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

            let prepared = match transcription_engine
                .prepare(session_id, Arc::clone(&context))
                .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    remove_session(&sessions, session_id, &session);
                    return Err(if session.cancelled.load(Ordering::Acquire) {
                        cancelled_error("dictation cancelled while preparing the provider")
                    } else {
                        error
                    });
                }
            };
            if session.cancelled.load(Ordering::Acquire) {
                remove_session(&sessions, session_id, &session);
                return Err(cancelled_error(
                    "dictation cancelled while preparing the provider",
                ));
            }
            let stable = context.recording.transcribe_after_stop;
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
                    session: Arc::downgrade(&session),
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
            let recording = match recorder
                .start(session_id, context, audio_consumer, recording_progress)
                .await
            {
                Ok(recording) => recording,
                Err(error) => {
                    let _ = cancel_transcription_once(&session, Arc::clone(&buffered)).await;
                    remove_session(&sessions, session_id, &session);
                    return Err(error);
                }
            };

            let mut recording = Some(recording);
            {
                let mut resources = session
                    .resources
                    .lock()
                    .expect("pipeline resource lock poisoned");
                if !session.cancelled.load(Ordering::Acquire) {
                    resources.recording = recording.take();
                }
            }
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
            if !stable {
                if let Err(error) = buffered.attach().await {
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
            let transcription_result = transcription.finish().await;
            asr_call_label = transcription.asr_call_label().or(asr_call_label);
            for notification in transcription.take_progress_notifications() {
                publish_progress(
                    &session,
                    session_id,
                    &progress,
                    EngineProgress::Notification(notification),
                )?;
            }
            let mut transcript = match transcription_result {
                Ok(transcript) => {
                    session
                        .transcription_finished
                        .store(true, Ordering::Release);
                    transcript
                }
                Err(first_error) => {
                    let cancelled = session.cancelled.load(Ordering::Acquire);
                    let prepared = prepared.ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Internal,
                            "transcription provider preparation is unavailable",
                        )
                    })?;
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
                    match retry_pcm {
                        Some(pcm) => match retry_transcription(
                            prepared,
                            Arc::clone(&session),
                            session_id,
                            Arc::clone(&progress),
                            pcm,
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
            let original_asr_text = transcript.text.clone();
            transcript.text = crate::correction::apply_correction_rules(
                &transcript.text,
                &context.correction_rules,
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
            let uses_polisher = context.uses_llm_polisher();
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
                    Err(error) if can_fallback_to_raw(policy, &error) => (
                        crate::ports::PolishOutput::text(transcript.text.clone()),
                        true,
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

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let sessions = Arc::clone(&self.sessions);
        let polisher = Arc::clone(&self.polisher);
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
            if let Some(recording) = recording {
                let archive = recording.archive();
                retain_first_error(&mut first_error, recording.stop().await);
                if !preserve_quick_note_archive {
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

async fn retry_transcription(
    prepared: Arc<dyn PreparedTranscription>,
    session: Arc<PipelineSession>,
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
    pcm: Vec<u8>,
) -> Result<
    (
        crate::ports::TranscriptOutput,
        Option<crate::auxiliary::AsrCallLabel>,
    ),
    (BackendError, Option<crate::auxiliary::AsrCallLabel>),
> {
    let mut last_label = None;
    for attempt in 1..=2_u64 {
        if let Err(error) =
            cancellable_backoff(&session, std::time::Duration::from_millis(500 * attempt)).await
        {
            return Err((error, last_label));
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
            Err(error) if error.retryable && attempt < 2 => continue,
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
                if !error.retryable || attempt == 2 {
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
    session: std::sync::Weak<PipelineSession>,
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
            crate::ports::RecordingEvent::Fatal(error) => {
                if let Some(session) = self.session.upgrade() {
                    *session
                        .recording_fault
                        .lock()
                        .expect("recording fault lock poisoned") = Some(error.clone());
                }
                self.progress
                    .publish(self.session_id, EngineProgress::RecordingFault(error))
            }
        }
    }
}

struct TranscriptProgressForwarder {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
}

impl TextStreamSink for TranscriptProgressForwarder {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
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

    #[test]
    fn recording_progress_forwarder_does_not_keep_pipeline_session_alive() {
        let session = Arc::new(PipelineSession::new(raw_dictation_context()));
        let weak = Arc::downgrade(&session);
        let _forwarder = RecordingProgressForwarder {
            session: Arc::downgrade(&session),
            session_id: SessionId::new(),
            progress: Arc::new(RecordingProgress::default()),
        };

        drop(session);
        assert!(weak.upgrade().is_none());
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
    async fn provider_preparation_failure_does_not_start_the_recorder() {
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
        assert_eq!(stops.load(Ordering::Acquire), 0);
        assert!(engine.sessions.lock().unwrap().is_empty());
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
