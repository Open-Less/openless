//! Smart transcription adapter for the single-pass multimodal pipeline.

use std::sync::Arc;
use std::time::Duration;

use super::OmniConfig;
use crate::asr::gemini::{CancelOnDrop, GeminiBatchASR, TranscriptionMode};
use crate::asr::gemini_live::{GeminiLiveASR, GeminiLiveConfig};
use crate::config::TaskSpawner;
use crate::polish::LLMError;
use crate::ports::{AudioConsumer, TextStreamChunk, TextStreamSink};

/// Uses the dedicated audio API instead of Gemini's general-purpose chat API.
pub struct GeminiTranscribeOmni {
    config: OmniConfig,
    languages: Vec<String>,
    hotwords: Vec<String>,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl GeminiTranscribeOmni {
    pub(crate) fn with_task_spawner(
        config: OmniConfig,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            config,
            languages: Vec::new(),
            hotwords: Vec::new(),
            task_spawner,
        }
    }

    pub fn set_hints(&mut self, languages: &[String], hotwords: &[String]) {
        self.languages = languages.to_vec();
        self.hotwords = hotwords.to_vec();
    }

    /// Start a Smart session during recording; batch models have no live session.
    pub async fn start_live(
        &self,
        partials: Arc<dyn TextStreamSink>,
    ) -> Result<Option<Arc<GeminiLiveASR>>, LLMError> {
        if self.config.model.trim() != crate::asr::gemini_live::MODEL {
            return Ok(None);
        }
        let config = GeminiLiveConfig {
            api_key: self.config.api_key.clone(),
            base_url: self.config.base_url.clone(),
            languages: self.languages.clone(),
            hotwords: self.hotwords.clone(),
            mode: TranscriptionMode::Smart,
            extra_headers: self.config.extra_headers.clone(),
        };
        GeminiLiveASR::connect(config, partials, Arc::clone(&self.task_spawner))
            .await
            .map(Some)
            .map_err(|error| LLMError::Network(error.to_string()))
    }

    /// Smart mode supplies its own cleanup rules. The API rejects system instructions.
    /// Reject text tasks locally so a transcript cannot be mistaken for a Q&A answer.
    pub async fn complete(
        &self,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
    ) -> Result<String, LLMError> {
        let wav = wav_bytes.filter(|wav| !wav.is_empty()).ok_or_else(|| {
            LLMError::ParseError(
                "Gemini Transcribe requires audio; choose a general-purpose model for text tasks"
                    .into(),
            )
        })?;
        if !user_text.trim().is_empty() {
            return Err(LLMError::ParseError(
                "Gemini Transcribe supports dictation only; choose a general-purpose model for Q&A"
                    .into(),
            ));
        }
        if self.config.model.trim() == crate::asr::gemini_live::MODEL {
            let pcm = crate::audio::decode_dictation_wav(wav)
                .map_err(|error| LLMError::ParseError(error.to_string()))?;
            if pcm.len() > 32_000 * 600 {
                return Err(LLMError::ParseError(
                    "Gemini Live recordings are limited to 10 minutes".into(),
                ));
            }
            let provider = self
                .start_live(Arc::new(DiscardLivePreview))
                .await?
                .expect("Live model creates a Live session");
            // The bounded queue also accepts a complete recording for Verify and retries.
            provider.consume_pcm_chunk(pcm);
            return provider
                .finish()
                .await
                .map(|result| result.text)
                .map_err(|error| LLMError::Network(error.to_string()));
        }
        let provider = Arc::new(
            GeminiBatchASR::new(
                self.config.api_key.clone(),
                self.config.base_url.clone(),
                self.config.model.trim().to_string(),
                None,
                self.hotwords.clone(),
            )
            .with_mode(TranscriptionMode::Smart)
            .with_language_hints(&self.languages),
        );
        let worker = Arc::clone(&provider);
        let wav = wav.to_vec();
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
        self.task_spawner.spawn(Box::pin(async move {
            let _ = result_sender.send(worker.transcribe_wav(&wav).await);
        }));
        // The engine may drop this future on cancellation. Keep the worker alive
        // long enough to cancel the interaction and delete its temporary file.
        let mut cleanup = CancelOnDrop {
            provider: Some(provider),
            task_spawner: Arc::clone(&self.task_spawner),
        };
        let result = result_receiver.await;
        cleanup.provider = None;
        result
            .map_err(|_| LLMError::Network("Gemini transcription task failed".into()))?
            .map_err(sanitize_transcription_error)
    }

    /// Recorded-audio transcription returns one final chunk after recording stops.
    pub async fn complete_streaming<F, C>(
        &self,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let cancelled = async {
            loop {
                if should_cancel() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let text = tokio::select! {
            biased;
            _ = cancelled => return Err(LLMError::Network("Gemini transcription cancelled".into())),
            result = self.complete(user_text, wav_bytes) => result?,
        };
        if should_cancel() {
            return Err(LLMError::Network("Gemini transcription cancelled".into()));
        }
        if !text.is_empty() {
            on_delta(&text);
        }
        Ok(text)
    }
}

struct DiscardLivePreview;

impl TextStreamSink for DiscardLivePreview {
    fn publish(&self, _: TextStreamChunk) -> Result<(), crate::BackendError> {
        Ok(())
    }
}

fn sanitize_transcription_error(error: anyhow::Error) -> LLMError {
    let message = if let Some(error) = error.downcast_ref::<reqwest::Error>() {
        format!(
            "Gemini transcription request failed ({})",
            crate::net::request_error_kind(error)
        )
    } else if let Some(status) = extract_http_status(&error.to_string()) {
        format!("Gemini transcription request failed (HTTP {status})")
    } else {
        "Gemini transcription request failed".into()
    };
    LLMError::Network(message)
}

fn extract_http_status(message: &str) -> Option<u16> {
    ["HTTP ", "status "].iter().find_map(|marker| {
        let tail = message.split_once(marker)?.1;
        let digits = tail
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        digits
            .parse::<u16>()
            .ok()
            .filter(|status| (100..600).contains(status))
    })
}
