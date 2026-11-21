//! Socket ownership, setup gating, and the recording's finalization boundary.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{protocol::frame::coding::CloseCode, Message};

use super::{
    protocol, transport, Command, GeminiLiveConfig, RawTranscript, FINISH_TIMEOUT, WRITE_TIMEOUT,
};
use crate::ports::{TextStreamChunk, TextStreamSink};

// shortcut: input transcription has no end marker; replace the 500 ms drain when the API provides one.
const TRANSCRIPT_QUIET_PERIOD: Duration = Duration::from_millis(500);

/// Wait for setupComplete before starting the manually delimited activity.
pub(super) async fn connect(config: &GeminiLiveConfig) -> Result<transport::Socket> {
    let mut socket = transport::connect(config).await?;
    send(&mut socket, protocol::setup(config)).await?;
    loop {
        let message = socket
            .next()
            .await
            .context("Gemini Live disconnected during setup")?
            .map_err(|_| anyhow::anyhow!("Gemini Live setup receive failed"))?;
        if let Some(value) = protocol::decode(message)? {
            if value.get("setupComplete").is_some() {
                break;
            }
        }
    }
    send(&mut socket, protocol::activity(true)).await?;
    Ok(socket)
}

/// The worker never owns the public session, preventing a task/session reference cycle.
pub(super) async fn run(
    mut socket: transport::Socket,
    commands: mpsc::Receiver<Command>,
    mut cancelled: watch::Receiver<Option<&'static str>>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript> {
    let cancellation = async {
        loop {
            if let Some(reason) = *cancelled.borrow_and_update() {
                break reason;
            }
            if cancelled.changed().await.is_err() {
                break "Gemini Live session dropped";
            }
        }
    };
    let result = tokio::select! {
        biased;
        reason = cancellation => Err(anyhow::anyhow!(reason)),
        result = transcribe(&mut socket, commands, partials) => result,
    };
    // Best-effort protocol close is bounded; dropping the socket always releases transport.
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    result
}

async fn transcribe(
    socket: &mut transport::Socket,
    mut commands: mpsc::Receiver<Command>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<RawTranscript> {
    let mut transcript = protocol::Transcript::default();
    let mut finishing = None;
    let mut quiet_deadline = None;
    let mut deadline = Instant::now() + Duration::from_secs(600) + FINISH_TIMEOUT;
    loop {
        tokio::select! {
            // Drain ready frames before quiet expiry, but never past the hard deadline.
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                bail!("Gemini Live transcription timed out before final text arrived");
            }
            message = socket.next() => {
                let message = message.context("Gemini Live disconnected before finalization")?
                    .map_err(|_| anyhow::anyhow!("Gemini Live receive failed"))?;
                // A normal close is an ordered end-of-stream, unlike generation/activity markers.
                if let Message::Close(frame) = &message {
                    if frame.as_ref().is_none_or(|frame| frame.code == CloseCode::Normal) {
                        if let Some((duration_ms, silence)) = finishing {
                            if transcript.can_finish(silence) {
                                return Ok(RawTranscript { text: transcript.final_text(), duration_ms });
                            }
                        }
                    }
                }
                let Some(value) = protocol::decode(message)? else {
                    // Flush automatic pong replies, including while waiting for final text.
                    tokio::time::timeout(WRITE_TIMEOUT, socket.flush()).await
                        .context("Gemini Live write timed out")?
                        .map_err(|_| anyhow::anyhow!("Gemini Live write failed"))?;
                    continue;
                };
                if let Some(text) = transcript.update(&value, finishing.is_some()) {
                    partials.publish(TextStreamChunk { text, offset: 0 })
                        .map_err(|_| anyhow::anyhow!("Gemini Live preview delivery failed"))?;
                }
                if let Some((_, silence)) = finishing {
                    let content = &value["serverContent"];
                    let transcription = content["inputTranscription"]["text"].is_string()
                        || content["interimInputTranscription"]["text"].is_string();
                    if !transcript.can_finish(silence) {
                        quiet_deadline = None;
                    } else if quiet_deadline.is_none() || transcription {
                        quiet_deadline = Some(Instant::now() + TRANSCRIPT_QUIET_PERIOD);
                    }
                }
            }
            _ = tokio::time::sleep_until(quiet_deadline.unwrap_or(deadline)), if quiet_deadline.is_some() => {
                let (duration_ms, _) = finishing.expect("only stopped recordings can drain");
                return Ok(RawTranscript { text: transcript.final_text(), duration_ms });
            }
            command = commands.recv(), if finishing.is_none() => {
                match command {
                    Some(Command::Audio(pcm)) => send(socket, protocol::audio(&pcm)).await?,
                    Some(Command::Finish { duration_ms, silence }) => {
                        transcript.begin_finish();
                        send(socket, protocol::activity(false)).await?;
                        finishing = Some((duration_ms, silence));
                        deadline = Instant::now() + FINISH_TIMEOUT;
                    }
                    None => bail!("Gemini Live audio source closed before finalization"),
                }
            }
        }
    }
}

async fn send(socket: &mut transport::Socket, message: Message) -> Result<()> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .context("Gemini Live write timed out")?
        .map_err(|_| anyhow::anyhow!("Gemini Live write failed"))
}
