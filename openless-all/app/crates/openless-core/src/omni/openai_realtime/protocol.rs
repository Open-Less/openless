//! GA Realtime wire messages. Server errors are classified without echoing payloads.

use base64::Engine;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::super::OmniConfig;
use crate::polish::LLMError;

pub(super) fn setup(config: &OmniConfig, instructions: &str) -> Message {
    let mut session = json!({
        "type": "realtime",
        "output_modalities": ["text"],
        "instructions": instructions,
        "max_output_tokens": 4096,
        "tools": [],
        "audio": {"input": {"format": {"type": "audio/pcm", "rate": 24000}, "turn_detection": null}},
    });
    // The 2.1 family exposes reasoning effort; older GA Realtime models do not.
    if config.model.trim().starts_with("gpt-realtime-2") {
        // Dictation favors the lowest supported effort. Do not inherit an unrelated
        // text model's thinking toggle or misrepresent low as disabled reasoning.
        session["reasoning"] = json!({"effort": "low"});
    }
    Message::text(json!({"type": "session.update", "session": session}).to_string())
}

pub(super) fn instructions(value: &str) -> Message {
    Message::text(
        json!({"type":"session.update", "session":{"type":"realtime", "instructions":value}})
            .to_string(),
    )
}

pub(super) fn user_text(value: &str) -> Message {
    Message::text(
        json!({"type":"conversation.item.create", "item":{
            "type":"message", "role":"user", "content":[{"type":"input_text", "text":value}]
        }})
        .to_string(),
    )
}

pub(super) fn audio(pcm24: &[u8]) -> Message {
    Message::text(
        json!({"type":"input_audio_buffer.append", "audio":
        base64::engine::general_purpose::STANDARD.encode(pcm24)})
        .to_string(),
    )
}

pub(super) fn commit() -> Message {
    Message::text(r#"{"type":"input_audio_buffer.commit"}"#)
}
pub(super) fn response() -> Message {
    Message::text(r#"{"type":"response.create","response":{"output_modalities":["text"]}}"#)
}

pub(super) fn decode(message: Message) -> Result<Option<Value>, LLMError> {
    let value: Value = match message {
        Message::Text(text) => serde_json::from_str(&text),
        Message::Binary(bytes) => serde_json::from_slice(&bytes),
        Message::Close(_) => {
            return Err(LLMError::Network(
                "OpenAI Realtime closed before the complete response".into(),
            ))
        }
        _ => return Ok(None),
    }
    .map_err(|_| LLMError::ParseError("Invalid OpenAI Realtime event".into()))?;
    if value["type"] == "error" {
        return Err(server_error(&value["error"]));
    }
    Ok(Some(value))
}

pub(super) fn server_error(error: &Value) -> LLMError {
    let code = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .unwrap_or("");
    let (status, message) = match code {
        "invalid_api_key" | "authentication_error" | "permission_denied" => {
            (401, "OpenAI Realtime authentication or model access failed")
        }
        "insufficient_quota" | "credit_balance_exhausted" | "billing_hard_limit_reached" => {
            (429, "OpenAI Realtime API quota is exhausted")
        }
        "rate_limit_exceeded" | "rate_limit_error" => (429, "OpenAI Realtime rate limit reached"),
        _ => (
            400,
            "OpenAI Realtime rejected the request; check model and session configuration",
        ),
    };
    LLMError::InvalidResponse {
        status,
        body: message.into(),
    }
}

pub(super) fn final_text(response: &Value, accumulated: &str) -> Result<String, LLMError> {
    if response["status"] != "completed" {
        if response["status"] == "failed" && response["status_details"]["error"].is_object() {
            return Err(server_error(&response["status_details"]["error"]));
        }
        let message = if response["status_details"]["reason"] == "max_output_tokens" {
            "OpenAI Realtime reached its output limit; partial text was not accepted"
        } else {
            "OpenAI Realtime returned an incomplete response; partial text was not accepted"
        };
        return Err(LLMError::InvalidResponse {
            status: 200,
            body: message.into(),
        });
    }
    let provided = response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter(|part| matches!(part["type"].as_str(), Some("output_text" | "text")))
        .filter_map(|part| part["text"].as_str())
        .collect::<String>();
    if !provided.trim().is_empty()
        && !accumulated.trim().is_empty()
        && provided.trim() != accumulated.trim()
    {
        return Err(LLMError::ParseError(
            "OpenAI Realtime final text did not match the streamed response".into(),
        ));
    }
    let text = if provided.trim().is_empty() {
        accumulated.to_owned()
    } else {
        provided
    };
    if text.trim().is_empty() {
        return Err(LLMError::InvalidResponse {
            status: 200,
            body: "OpenAI Realtime returned no text".into(),
        });
    }
    Ok(text.trim().to_owned())
}
