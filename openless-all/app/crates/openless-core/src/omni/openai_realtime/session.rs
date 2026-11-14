//! Ordered manual-turn protocol and authoritative finalization, with one socket owner.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use super::super::OmniConfig;
use super::{
    pcm::Resampler, protocol, transport, wire::Profile, Command, FINISH_TIMEOUT, WRITE_TIMEOUT,
};
use crate::asr::RawTranscript;
use crate::omni::realtime_options::RealtimeReasoningEffort;
use crate::polish::LLMError;
use crate::ports::{TextStreamChunk, TextStreamSink};

pub(super) async fn connect(
    config: &OmniConfig,
    instructions: &str,
    user_text: &str,
) -> Result<transport::Socket, LLMError> {
    let profile = Profile::from_provider(&config.provider_id)?;
    if !user_text.trim().is_empty() && !profile.supports_text_input() {
        return Err(LLMError::ParseError(
            "This Realtime protocol does not support auxiliary text input".into(),
        ));
    }
    let mut socket = transport::connect(config).await?;
    loop {
        if let Some(value) = receive(&mut socket).await? {
            if value["type"] == "session.created" {
                break;
            }
        }
    }
    send(&mut socket, profile.setup(config, instructions)).await?;
    let mut may_fallback = profile == Profile::OpenAiGa
        && protocol::reasoning_effort(config) == Some("none")
        && config.realtime_options.reasoning_fallback;
    loop {
        if let Some(value) = receive_raw(&mut socket).await? {
            if value["type"] == "error" {
                if may_fallback && protocol::rejected_experimental_effort(&value["error"]) {
                    may_fallback = false;
                    let mut fallback = config.clone();
                    fallback.realtime_options.reasoning_effort = RealtimeReasoningEffort::Low;
                    log::info!("[realtime] Experimental none effort rejected; retrying initial setup once with low");
                    send(&mut socket, profile.setup(&fallback, instructions)).await?;
                    continue;
                }
                return Err(protocol::server_error(&value["error"]));
            }
            if value["type"] == "session.updated" {
                break;
            }
        }
    }
    if !user_text.trim().is_empty() {
        send(&mut socket, protocol::user_text(user_text)).await?;
    }
    Ok(socket)
}

pub(super) async fn run(
    socket: &mut transport::Socket,
    profile: Profile,
    commands: mpsc::Receiver<Command>,
    mut cancelled: watch::Receiver<Option<&'static str>>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript, LLMError> {
    tokio::select! {
        biased;
        reason = cancellation(&mut cancelled) => Err(LLMError::Network(reason.into())),
        result = compose(socket, profile, commands, partials) => result,
    }
}

pub(super) async fn cancellation(
    cancelled: &mut watch::Receiver<Option<&'static str>>,
) -> &'static str {
    loop {
        if let Some(reason) = *cancelled.borrow_and_update() {
            return reason;
        }
        if cancelled.changed().await.is_err() {
            return "Realtime session dropped";
        }
    }
}

async fn compose(
    socket: &mut transport::Socket,
    profile: Profile,
    mut commands: mpsc::Receiver<Command>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript, LLMError> {
    let mut pcm = profile.needs_24k().then(Resampler::default);
    let mut text = String::new();
    let mut final_snapshot: Option<String> = None;
    let mut offset = 0;
    let mut finishing = None;
    let mut deadline = Instant::now() + Duration::from_secs(600) + FINISH_TIMEOUT;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err(LLMError::Timeout),
            command = commands.recv(), if finishing.is_none() => {
                match command {
                    Some(Command::Audio(audio)) => {
                        let converted = match pcm.as_mut() {
                            Some(pcm) => pcm.push(&audio),
                            None => audio,
                        };
                        if !converted.is_empty() { send(socket, protocol::audio(&converted)).await?; }
                    }
                    Some(Command::Finish { duration_ms, instructions, has_audio }) => {
                        let tail = pcm.as_mut().map(Resampler::finish).unwrap_or_default();
                        if !tail.is_empty() { send(socket, protocol::audio(&tail)).await?; }
                        // Preserve in-recording style/translation updates using the final context.
                        // This updates instructions only, never recaptures the host document.
                        send(socket, profile.instructions(&instructions)).await?;
                        if has_audio { send(socket, protocol::commit()).await?; }
                        send(socket, profile.response()).await?;
                        finishing = Some(duration_ms);
                        deadline = Instant::now() + FINISH_TIMEOUT;
                    }
                    None => return Err(LLMError::Network("Realtime audio source closed before finalization".into())),
                }
            }
            value = receive(socket) => {
                let Some(value) = value? else { continue; };
                let kind = value["type"].as_str().unwrap_or("");
                if kind.starts_with("response.") && finishing.is_none() {
                    return Err(LLMError::ParseError("Realtime service generated a response before manual stop".into()));
                }
                if matches!(kind, "response.audio.delta" | "response.output_audio.delta") {
                    return Err(LLMError::ParseError("Realtime service generated audio despite text-only output; select a compatible protocol".into()));
                }
                match value["type"].as_str() {
                    Some("response.output_text.delta" | "response.text.delta") if finishing.is_some() => {
                        if let Some(delta) = value["delta"].as_str().filter(|text| !text.is_empty()) {
                            if text.len().saturating_add(delta.len()) > 1_000_000 {
                                return Err(LLMError::ParseError("Realtime text exceeded the bounded response limit".into()));
                            }
                            text.push_str(delta);
                            partials.publish(TextStreamChunk { text: delta.to_owned(), offset })
                                .map_err(|_| LLMError::Network("Realtime preview delivery failed".into()))?;
                            offset += delta.chars().count() as u64;
                        }
                    }
                    Some("response.text.done" | "response.output_text.done") if finishing.is_some() => {
                        if let Some(snapshot) = value["text"].as_str().filter(|value| !value.is_empty()) {
                            if snapshot.len() > 1_000_000 {
                                return Err(LLMError::ParseError("Realtime text exceeded the bounded response limit".into()));
                            }
                            final_snapshot = Some(snapshot.to_owned());
                        }
                    }
                    Some("response.done") => {
                        let duration_ms = finishing.ok_or_else(|| LLMError::ParseError("Unexpected Realtime response before stop".into()))?;
                        if let Some(snapshot) = &final_snapshot {
                            if !text.is_empty() && text.trim() != snapshot.trim() {
                                return Err(LLMError::ParseError("Realtime final snapshot did not match streamed text".into()));
                            }
                        }
                        let accumulated = if text.is_empty() { final_snapshot.as_deref().unwrap_or("") } else { &text };
                        let final_text = protocol::final_text(&value["response"], accumulated)?;
                        if text.is_empty() {
                            partials.publish(TextStreamChunk { text: final_text.clone(), offset: 0 })
                                .map_err(|_| LLMError::Network("Realtime preview delivery failed".into()))?;
                        }
                        return Ok(RawTranscript { text: final_text, duration_ms });
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn receive_raw(
    socket: &mut transport::Socket,
) -> Result<Option<serde_json::Value>, LLMError> {
    let message = socket
        .next()
        .await
        .ok_or_else(|| LLMError::Network("Realtime disconnected before final response".into()))?
        .map_err(|_| LLMError::Network("Realtime receive failed".into()))?;
    let value = protocol::decode_message(message)?;
    if value.is_none() {
        tokio::time::timeout(WRITE_TIMEOUT, socket.flush())
            .await
            .map_err(|_| LLMError::Timeout)?
            .map_err(|_| LLMError::Network("Realtime socket flush failed".into()))?;
    }
    Ok(value)
}

async fn receive(socket: &mut transport::Socket) -> Result<Option<serde_json::Value>, LLMError> {
    let value = receive_raw(socket).await?;
    if let Some(value) = &value {
        if value["type"] == "error" {
            return Err(protocol::server_error(&value["error"]));
        }
    }
    Ok(value)
}

async fn send(
    socket: &mut transport::Socket,
    message: tokio_tungstenite::tungstenite::Message,
) -> Result<(), LLMError> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| LLMError::Timeout)?
        .map_err(|_| LLMError::Network("Realtime send failed".into()))
}
