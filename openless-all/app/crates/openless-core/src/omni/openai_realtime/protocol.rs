//! GA Realtime wire messages. Server errors are classified without echoing payloads.

use base64::Engine;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::super::OmniConfig;
use crate::omni::realtime_options::RealtimeReasoningEffort;
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
    if let Some(effort) = reasoning_effort(config) {
        session["reasoning"] = json!({"effort": effort});
    }
    Message::text(json!({"type": "session.update", "session": session}).to_string())
}

pub(super) fn reasoning_effort(config: &OmniConfig) -> Option<&'static str> {
    let selected = config.realtime_options.reasoning_effort;
    if selected == RealtimeReasoningEffort::Omit {
        return None;
    }
    if selected == RealtimeReasoningEffort::Auto {
        return supports_reasoning(&config.model).then_some("low");
    }
    // Saved settings must not leak 2.x-only options into a dated first-generation
    // model when the user switches models. Explicit overrides are useful for
    // compatible gateway aliases whose capabilities cannot be inferred by name.
    let model = config.model.trim();
    let known_legacy = model == "gpt-realtime"
        || model == "gpt-realtime-mini"
        || model.starts_with("gpt-4o-realtime")
        || (model.starts_with("gpt-realtime-") && !supports_reasoning(model));
    if known_legacy {
        None
    } else {
        selected.explicit_value()
    }
}

pub(super) fn rejected_experimental_effort(error: &Value) -> bool {
    matches!(
        error["code"].as_str(),
        Some(
            "invalid_value"
                | "unsupported_value"
                | "invalid_parameter"
                | "invalid_enum_value"
                | "unsupported_parameter_value"
        )
    ) && matches!(
        error["param"].as_str(),
        Some("reasoning.effort" | "session.reasoning.effort")
    ) && matches!(
        error["type"].as_str(),
        None | Some("invalid_request_error" | "validation_error" | "schema_error")
    )
}

fn supports_reasoning(model: &str) -> bool {
    let Some(version) = model
        .trim()
        .strip_prefix("gpt-realtime-")
        .and_then(|suffix| suffix.split('-').next())
    else {
        return false;
    };
    let mut parts = version.split('.');
    parts.next() == Some("2")
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
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

pub(super) fn decode_message(message: Message) -> Result<Option<Value>, LLMError> {
    let value: Value = match message {
        Message::Text(text) => serde_json::from_str(&text),
        Message::Binary(bytes) => serde_json::from_slice(&bytes),
        Message::Close(_) => {
            return Err(LLMError::Network(
                "Realtime closed before the complete response".into(),
            ))
        }
        _ => return Ok(None),
    }
    .map_err(|_| LLMError::ParseError("Invalid Realtime event".into()))?;
    Ok(Some(value))
}

pub(super) fn decode(message: Message) -> Result<Option<Value>, LLMError> {
    let value = decode_message(message)?;
    if let Some(value) = &value {
        if value["type"] == "error" {
            return Err(server_error(&value["error"]));
        }
    }
    Ok(value)
}

pub(super) fn server_error(error: &Value) -> LLMError {
    let code = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .unwrap_or("");
    let (status, message) = match code {
        "invalid_api_key" | "authentication_error" | "permission_denied" => {
            (401, "Realtime authentication or model access failed")
        }
        "insufficient_quota" | "credit_balance_exhausted" | "billing_hard_limit_reached" => {
            (429, "Realtime API quota is exhausted")
        }
        "rate_limit_exceeded" | "rate_limit_error" => (429, "Realtime rate limit reached"),
        _ => (
            400,
            "Realtime rejected the request; check model and session configuration",
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
            "Realtime reached its output limit; partial text was not accepted"
        } else {
            "Realtime returned an incomplete response; partial text was not accepted"
        };
        return Err(LLMError::InvalidResponse {
            status: 200,
            body: message.into(),
        });
    }
    if response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .any(|part| matches!(part["type"].as_str(), Some("audio" | "output_audio")))
    {
        // A transcript attached to audio is not the requested text-only result.
        // In particular, an earlier text.done must not hide audio-only output.
        return Err(LLMError::InvalidResponse {
            status: 200,
            body: "Realtime returned audio instead of the requested text-only output".into(),
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
            "Realtime final text did not match the streamed response".into(),
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
            body: "Realtime returned no text".into(),
        });
    }
    Ok(text.trim().to_owned())
}
