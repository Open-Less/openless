//! DeepSeek streaming speech: PCM frames are usable only until a successful finish.
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub async fn stream<F>(api_key: &str, text: &str, voice: &str, audio: F) -> Result<(), String>
where
    F: FnMut(Vec<u8>) -> Result<(), String>,
{
    stream_url(
        api_key,
        text,
        voice,
        "wss://api.deepseek.com/v1/tools/tts",
        audio,
    )
    .await
}

async fn stream_url<F>(
    api_key: &str,
    text: &str,
    voice: &str,
    endpoint: &str,
    mut audio: F,
) -> Result<(), String>
where
    F: FnMut(Vec<u8>) -> Result<(), String>,
{
    let billable: usize = text
        .chars()
        .map(|c| {
            if ('\u{3400}'..='\u{9fff}').contains(&c) {
                2
            } else {
                1
            }
        })
        .sum();
    if text.trim().is_empty() || billable > 10_000 || text.len() > 60_000 {
        return Err("DeepSeek TTS text must contain 1–10,000 billing characters".into());
    }
    if !matches!(voice, "stella" | "tide") {
        return Err("Unknown DeepSeek voice".into());
    }
    let mut request = format!("{endpoint}?voice_id={voice}&format=pcm")
        .into_client_request()
        .map_err(|_| "Invalid TTS endpoint")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {api_key}")
            .parse()
            .map_err(|_| "Invalid API key")?,
    );
    let (mut ws, _) = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| "TTS connection timed out")?
    .map_err(|_| "DeepSeek TTS connection failed")?;
    ws.send(Message::Text(
        json!({"event":"text","text":text}).to_string().into(),
    ))
    .await
    .map_err(|_| "TTS send failed")?;
    ws.send(Message::Text(
        json!({"event":"finish_text"}).to_string().into(),
    ))
    .await
    .map_err(|_| "TTS send failed")?;
    let mut size = 0usize;
    while let Some(message) = tokio::time::timeout(std::time::Duration::from_secs(60), ws.next())
        .await
        .map_err(|_| "TTS timed out")?
    {
        match message.map_err(|_| "TTS stream interrupted")? {
            Message::Binary(bytes) => {
                size += bytes.len();
                if size > 86_400_000 {
                    return Err("TTS audio limit exceeded".into());
                }
                audio(bytes.to_vec())?;
            }
            Message::Text(raw) => {
                let event: serde_json::Value =
                    serde_json::from_str(&raw).map_err(|_| "Invalid TTS event")?;
                if event["event"] == "finish" {
                    return if event["code"] == "success" && size > 0 {
                        Ok(())
                    } else {
                        Err("DeepSeek TTS synthesis failed".into())
                    };
                }
            }
            Message::Ping(bytes) => ws
                .send(Message::Pong(bytes))
                .await
                .map_err(|_| "TTS connection lost")?,
            Message::Close(_) => break,
            _ => {}
        }
    }
    Err("DeepSeek TTS stream ended before successful completion".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[allow(clippy::result_large_err)]
    async fn websocket_frames_require_successful_finish() {
        for finish in [Some("success"), Some("service_error"), None] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("ws://{}/v1/tools/tts", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_hdr_async(
                    socket,
                    |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                     response| {
                        assert_eq!(request.headers()["authorization"], "Bearer test-key");
                        assert_eq!(request.uri().path(), "/v1/tools/tts");
                        Ok(response)
                    },
                )
                .await
                .unwrap();
                let first = ws.next().await.unwrap().unwrap().into_text().unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&first).unwrap()["event"],
                    "text"
                );
                let second = ws.next().await.unwrap().unwrap().into_text().unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&second).unwrap()["event"],
                    "finish_text"
                );
                ws.send(Message::Binary(vec![0, 1, 2, 3].into()))
                    .await
                    .unwrap();
                if let Some(code) = finish {
                    ws.send(Message::Text(
                        json!({"event":"finish","code":code}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                } else {
                    ws.close(None).await.unwrap();
                }
            });
            let mut bytes = Vec::new();
            let outcome = stream_url("test-key", "hello", "stella", &endpoint, |data| {
                bytes.extend(data);
                Ok(())
            })
            .await;
            assert_eq!(outcome.is_ok(), finish == Some("success"));
            assert_eq!(bytes, vec![0, 1, 2, 3]);
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn invalid_text_is_rejected_before_network_access() {
        for text in [String::new(), "中".repeat(5001)] {
            assert!(stream("key", &text, "stella", |_| Ok(())).await.is_err());
        }
    }
}
