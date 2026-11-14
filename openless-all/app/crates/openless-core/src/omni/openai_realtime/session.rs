//! Ordered manual-turn protocol and authoritative finalization, with one socket owner.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use super::{pcm::Resampler, protocol, transport, Command, FINISH_TIMEOUT, WRITE_TIMEOUT};
use super::super::OmniConfig;
use crate::asr::RawTranscript;
use crate::polish::LLMError;
use crate::ports::{TextStreamChunk, TextStreamSink};

pub(super) async fn connect(config: &OmniConfig, instructions: &str, user_text: &str) -> Result<transport::Socket, LLMError> {
    let mut socket = transport::connect(config).await?;
    loop {
        if let Some(value) = receive(&mut socket).await? {
            if value["type"] == "session.created" { break; }
        }
    }
    send(&mut socket, protocol::setup(config, instructions)).await?;
    loop {
        if let Some(value) = receive(&mut socket).await? {
            if value["type"] == "session.updated" { break; }
        }
    }
    if !user_text.trim().is_empty() { send(&mut socket, protocol::user_text(user_text)).await?; }
    Ok(socket)
}

pub(super) async fn run(
    socket: &mut transport::Socket,
    commands: mpsc::Receiver<Command>,
    mut cancelled: watch::Receiver<Option<&'static str>>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript, LLMError> {
    tokio::select! {
        biased;
        reason = cancellation(&mut cancelled) => Err(LLMError::Network(reason.into())),
        result = compose(socket, commands, partials) => result,
    }
}

pub(super) async fn cancellation(cancelled: &mut watch::Receiver<Option<&'static str>>) -> &'static str {
    loop {
        if let Some(reason) = *cancelled.borrow_and_update() { return reason; }
        if cancelled.changed().await.is_err() { return "OpenAI Realtime session dropped"; }
    }
}

async fn compose(
    socket: &mut transport::Socket,
    mut commands: mpsc::Receiver<Command>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript, LLMError> {
    let mut pcm = Resampler::default();
    let mut text = String::new();
    let mut offset = 0;
    let mut finishing = None;
    let mut deadline = Instant::now() + Duration::from_secs(600) + FINISH_TIMEOUT;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err(LLMError::Timeout),
            command = commands.recv(), if finishing.is_none() => {
                match command {
                    Some(Command::Audio(audio)) => {
                        let converted = pcm.push(&audio);
                        if !converted.is_empty() { send(socket, protocol::audio(&converted)).await?; }
                    }
                    Some(Command::Finish { duration_ms, instructions, has_audio }) => {
                        let tail = pcm.finish();
                        if !tail.is_empty() { send(socket, protocol::audio(&tail)).await?; }
                        // Preserve in-recording style/translation updates using the final context.
                        // This updates instructions only, never recaptures the host document.
                        send(socket, protocol::instructions(&instructions)).await?;
                        if has_audio { send(socket, protocol::commit()).await?; }
                        send(socket, protocol::response()).await?;
                        finishing = Some(duration_ms);
                        deadline = Instant::now() + FINISH_TIMEOUT;
                    }
                    None => return Err(LLMError::Network("OpenAI Realtime audio source closed before finalization".into())),
                }
            }
            value = receive(socket) => {
                let Some(value) = value? else { continue; };
                match value["type"].as_str() {
                    Some("response.output_text.delta" | "response.text.delta") if finishing.is_some() => {
                        if let Some(delta) = value["delta"].as_str().filter(|text| !text.is_empty()) {
                            if text.len().saturating_add(delta.len()) > 1_000_000 {
                                return Err(LLMError::ParseError("OpenAI Realtime text exceeded the bounded response limit".into()));
                            }
                            text.push_str(delta);
                            partials.publish(TextStreamChunk { text: delta.to_owned(), offset })
                                .map_err(|_| LLMError::Network("OpenAI Realtime preview delivery failed".into()))?;
                            offset += delta.chars().count() as u64;
                        }
                    }
                    Some("response.done") => {
                        let duration_ms = finishing.ok_or_else(|| LLMError::ParseError("Unexpected OpenAI Realtime response before stop".into()))?;
                        let final_text = protocol::final_text(&value["response"], &text)?;
                        if text.is_empty() {
                            partials.publish(TextStreamChunk { text: final_text.clone(), offset: 0 })
                                .map_err(|_| LLMError::Network("OpenAI Realtime preview delivery failed".into()))?;
                        }
                        return Ok(RawTranscript { text: final_text, duration_ms });
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn receive(socket: &mut transport::Socket) -> Result<Option<serde_json::Value>, LLMError> {
    let message = socket.next().await
        .ok_or_else(|| LLMError::Network("OpenAI Realtime disconnected before final response".into()))?
        .map_err(|_| LLMError::Network("OpenAI Realtime receive failed".into()))?;
    let value = protocol::decode(message)?;
    if value.is_none() {
        tokio::time::timeout(WRITE_TIMEOUT, socket.flush()).await
            .map_err(|_| LLMError::Timeout)?
            .map_err(|_| LLMError::Network("OpenAI Realtime socket flush failed".into()))?;
    }
    Ok(value)
}

async fn send(socket: &mut transport::Socket, message: tokio_tungstenite::tungstenite::Message) -> Result<(), LLMError> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message)).await
        .map_err(|_| LLMError::Timeout)?
        .map_err(|_| LLMError::Network("OpenAI Realtime send failed".into()))
}
