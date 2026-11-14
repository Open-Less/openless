//! Live DeepSeek ASR check using the settings probe and production transcription engine.
//! DEEPSEEK_API_KEY=... cargo run -p openless-core --example deepseek_asr_smoke -- speech.pcm
//! Input: signed 16-bit little-endian PCM, 16 kHz, mono. Credentials remain in memory.

use std::sync::Arc;

use anyhow::{Context, Result};
use openless_core::credentials::{
    ChannelKind, ChannelMutation, ChannelMutationResult, CredentialKey, CredentialNamespace,
    CredentialStore, InMemoryCredentialStore, SecretValue, ASR_API_KEY_ACCOUNT,
};
use openless_core::ports::{TextStreamChunk, TextStreamSink, TranscriptionEngine};
use openless_core::{
    BackendError, DictationContext, ProviderApi, ProviderInvocation, ProviderKind, ProviderRequest,
    ProviderService, SessionId, SharedCloudTranscriptionEngine, TokioTaskSpawner,
};

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("expected a 16 kHz mono s16le PCM file")?;
    let pcm = std::fs::read(path)?;
    anyhow::ensure!(!pcm.is_empty() && pcm.len() % 2 == 0, "invalid PCM length");
    let key = std::env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY is required")?;
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let ChannelMutationResult::Created(channel) = credentials
        .mutate_channel(ChannelMutation::Create {
            kind: ChannelKind::Asr,
            provider_type: "deepseek".into(),
            name: "DeepSeek ASR smoke test".into(),
        })
        .await?
    else {
        anyhow::bail!("channel creation failed")
    };
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Asr,
                Some(channel.clone()),
                ASR_API_KEY_ACCOUNT,
            )?,
            SecretValue::new(key),
        )
        .await?;
    let service = ProviderService::new(credentials.clone(), Arc::new(TokioTaskSpawner));
    let check = service
        .validate(ProviderRequest {
            kind: ProviderKind::Asr,
            channel_id: Some(channel.clone()),
            thinking_enabled: false,
        })
        .await?;
    anyhow::ensure!(check.ok, "DeepSeek ASR connection check failed");
    println!("DeepSeek ASR settings validation succeeded.");
    let mut context = DictationContext::default();
    context.asr = ProviderInvocation::new(channel, "deepseek");
    let engine = SharedCloudTranscriptionEngine::new(credentials);
    let session = engine
        .start(SessionId::new(), Arc::new(context), Arc::new(DiscardText))
        .await?;
    session.consume_pcm_chunk(&pcm);
    let transcript = session.finish().await?;
    anyhow::ensure!(!transcript.text.trim().is_empty(), "empty transcription");
    println!("{}", transcript.text);
    println!("Duration: {} ms", transcript.duration_ms);
    Ok(())
}

struct DiscardText;
impl TextStreamSink for DiscardText {
    fn publish(&self, _: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}
