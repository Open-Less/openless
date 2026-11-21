//! Live Gemini transcription smoke test with in-memory credentials for either pipeline.
//!
//! Run from openless-all/app:
//! cargo run -p openless-core --example gemini_transcribe_smoke -- [--live] [--smart] speech.pcm [base-url]
//! Input must be raw signed 16-bit little-endian, 16 kHz, mono PCM.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use openless_core::asr::gemini::{GeminiBatchASR, DEFAULT_ENDPOINT, DEFAULT_MODEL, PROVIDER_ID};
use openless_core::credentials::{
    ChannelKind, ChannelMutation, ChannelMutationResult, CredentialKey, CredentialNamespace,
    CredentialStore, InMemoryCredentialStore, SecretValue, ASR_API_KEY_ACCOUNT,
    ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT,
    OMNI_MODEL_ACCOUNT,
};
use openless_core::ports::{AudioConsumer, TextStreamChunk, TextStreamSink, TranscriptionEngine};
use openless_core::{
    BackendError, DictationAudioSource, DictationContext, DictationEngine, EngineProgress,
    EngineProgressSink, ProviderApi, ProviderInvocation, ProviderKind, ProviderRequest,
    ProviderService, ProviderSlot, SessionId, SharedOmniDictationEngine, TokioTaskSpawner,
};

/// Exercise both the settings silence probe and the production transcription client.
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    let mut smart = false;
    let mut live = false;
    while args.peek().is_some_and(|arg| arg.starts_with("--")) {
        match args.next().as_deref() {
            Some("--smart") => smart = true,
            Some("--live") => live = true,
            _ => anyhow::bail!("supported flags: --live, --smart"),
        }
    }
    let model = if live {
        openless_core::asr::gemini_live::MODEL
    } else {
        DEFAULT_MODEL
    };
    let path = args
        .next()
        .context("expected a 16 kHz mono s16le PCM file")?;
    let endpoint = args.next().unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
    let key = std::env::var("GEMINI_API_KEY").context("GEMINI_API_KEY is required")?;
    let pcm = std::fs::read(path)?;
    anyhow::ensure!(!pcm.is_empty() && pcm.len() % 2 == 0, "invalid PCM length");
    if smart {
        return smoke_multimodal(pcm, key, endpoint, model, live).await;
    }

    let credentials = Arc::new(InMemoryCredentialStore::default());
    let ChannelMutationResult::Created(channel) = credentials
        .mutate_channel(ChannelMutation::Create {
            kind: ChannelKind::Asr,
            provider_type: PROVIDER_ID.to_owned(),
            name: "Gemini smoke test".to_owned(),
        })
        .await?
    else {
        anyhow::bail!("channel creation failed");
    };
    for (account, value) in [
        (ASR_API_KEY_ACCOUNT, key.as_str()),
        (ASR_ENDPOINT_ACCOUNT, endpoint.as_str()),
        (ASR_MODEL_ACCOUNT, model),
    ] {
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Asr, Some(channel.clone()), account)?,
                SecretValue::new(value),
            )
            .await?;
    }
    let service = ProviderService::new(credentials.clone(), Arc::new(TokioTaskSpawner));
    let check = service
        .validate(ProviderRequest {
            kind: ProviderKind::Asr,
            channel_id: Some(channel.clone()),
            thinking_enabled: false,
        })
        .await?;
    anyhow::ensure!(check.ok, "Gemini ASR Verify failed");
    println!("Gemini ASR Verify succeeded (500 ms silence).");

    if live {
        let engine = openless_core::SharedCloudTranscriptionEngine::new(credentials);
        let mut context = DictationContext::default();
        context.asr = ProviderInvocation::for_provider(PROVIDER_ID);
        context.asr.provider_id = channel;
        context.asr.model = Some(model.into());
        context.polish.hotwords = vec!["OpenLess".into(), "Gemini".into()];
        let preview = Arc::new(LivePreview::default());
        let session = engine
            .start(SessionId::new(), Arc::new(context), preview.clone())
            .await?;
        for chunk in pcm.chunks(3_200) {
            session.consume_pcm_chunk(chunk);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let previews_before_stop = preview.0.load(Ordering::Acquire);
        let transcript = session.finish().await?;
        anyhow::ensure!(
            previews_before_stop > 0,
            "no live preview arrived before stopping"
        );
        anyhow::ensure!(
            !transcript.text.trim().is_empty(),
            "no speech was transcribed"
        );
        println!(
            "Live transcript ({} ms, {previews_before_stop} previews before stop): {}",
            transcript.duration_ms, transcript.text
        );
        return Ok(());
    }

    let provider = GeminiBatchASR::new(
        key,
        endpoint,
        DEFAULT_MODEL.to_owned(),
        None,
        vec!["OpenLess".to_owned(), "Gemini".to_owned()],
    );
    provider.consume_pcm_chunk(&pcm);
    let transcript = provider.transcribe().await?;
    anyhow::ensure!(
        !transcript.text.trim().is_empty(),
        "no speech was transcribed"
    );
    println!(
        "Transcript ({} ms): {}",
        transcript.duration_ms, transcript.text
    );
    Ok(())
}

/// Exercise settings Verify and the production Multimodal engine without a microphone.
async fn smoke_multimodal(
    pcm: Vec<u8>,
    key: String,
    endpoint: String,
    model: &str,
    live: bool,
) -> Result<()> {
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .set_active_provider(ProviderSlot::Omni, PROVIDER_ID.to_owned())
        .await?;
    for (account, value) in [
        (OMNI_API_KEY_ACCOUNT, key.as_str()),
        (OMNI_ENDPOINT_ACCOUNT, endpoint.as_str()),
        (OMNI_MODEL_ACCOUNT, model),
    ] {
        credentials
            .write(
                CredentialKey::new(
                    CredentialNamespace::Omni,
                    Some(PROVIDER_ID.to_owned()),
                    account,
                )?,
                SecretValue::new(value),
            )
            .await?;
    }
    let service = ProviderService::new(credentials.clone(), Arc::new(TokioTaskSpawner));
    let check = service
        .validate(ProviderRequest {
            kind: ProviderKind::Omni,
            channel_id: None,
            thinking_enabled: true,
        })
        .await?;
    anyhow::ensure!(check.ok, "Gemini Multimodal Verify failed");
    println!("Gemini Multimodal Verify succeeded (500 ms silence, Smart mode).");
    let engine = SharedOmniDictationEngine::new(
        credentials,
        Arc::new(openless_core::testing::FixtureAudioRecorder::new(
            Vec::new(),
            Vec::new(),
        )),
    );
    let mut context = DictationContext::default();
    context.audio_source = DictationAudioSource::External;
    context.pipeline_mode = openless_core::shared_types::PipelineMode::Multimodal;
    context.omni = ProviderInvocation::for_provider(PROVIDER_ID);
    context.omni.model = Some(model.to_owned());
    context.polish.hotwords = vec!["OpenLess".to_owned(), "Gemini".to_owned()];
    let session = SessionId::new();
    let preview = Arc::new(LivePreview::default());
    engine
        .start(session, Arc::new(context), preview.clone())
        .await?;
    if live {
        for chunk in pcm.chunks(3_200) {
            engine.feed_audio(session, chunk)?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    } else {
        engine.feed_audio(session, &pcm)?;
    }
    let previews_before_stop = preview.0.load(Ordering::Acquire);
    let result = engine
        .finish(session, preview)
        .await
        .map_err(|failure| anyhow::anyhow!("{failure:?}"))?;
    anyhow::ensure!(
        !result.polished_text.trim().is_empty(),
        "no speech was transcribed"
    );
    if live {
        anyhow::ensure!(
            previews_before_stop > 0,
            "no Smart live preview arrived before stopping"
        );
        println!("Smart live previews before stop: {previews_before_stop}");
    }
    println!(
        "Smart transcript ({} ms): {}",
        result.duration_ms, result.polished_text
    );
    Ok(())
}

#[derive(Default)]
struct LivePreview(AtomicUsize);

impl TextStreamSink for LivePreview {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        if !chunk.text.is_empty() {
            self.0.fetch_add(1, Ordering::AcqRel);
            println!("Preview: {}", chunk.text);
        }
        Ok(())
    }
}

impl EngineProgressSink for LivePreview {
    fn publish(&self, _: SessionId, progress: EngineProgress) -> Result<(), BackendError> {
        if let EngineProgress::TranscriptDelta(delta) = progress {
            TextStreamSink::publish(
                self,
                TextStreamChunk {
                    text: delta.text,
                    offset: delta.offset,
                },
            )?;
        }
        Ok(())
    }
}
