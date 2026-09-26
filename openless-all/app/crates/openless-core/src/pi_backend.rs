//! Wire contract for the private, application-bundled PI runtime.
use crate::coding_agent::CodingAgentRequest;
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::CodingAgentStreamEvent;

pub(crate) fn encode_request(request: &CodingAgentRequest) -> Result<String, BackendError> {
    let value = serde_json::json!({
        "type": "prompt",
        "session_id": request.session_id,
        "prompt": request.prompt,
        "cwd": request.cwd,
        "model": request.model,
        "permission_mode": request.permission_mode.as_cli_arg(),
        "timeout_secs": request.timeout_secs,
        "allowed_tools": request.allowed_tools,
        "disallowed_tools": request.disallowed_tools,
        "extra_system_prompt": request.extra_system_prompt,
        "continue_session": request.continue_session,
        "continuation_context": request.continuation_context,
        // Conversation context belongs to LessComputerService. Never resume a
        // global PI session shared with another window or user operation.
        "session_persistence": false,
    });
    serde_json::to_string(&value)
        .map(|mut line| {
            line.push('\n');
            line
        })
        .map_err(|error| BackendError::new(BackendErrorCode::Internal, error.to_string()))
}

pub(crate) fn parse_stream_line(session_id: &str, line: &str) -> Option<CodingAgentStreamEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let session_id = session_id.to_owned();
    match value.get("type")?.as_str()? {
        "delta" => Some(CodingAgentStreamEvent::Delta {
            session_id,
            text: value.get("text")?.as_str()?.to_owned(),
        }),
        "tool_use" => Some(CodingAgentStreamEvent::ToolUse {
            session_id,
            name: value.get("name")?.as_str()?.to_owned(),
        }),
        "complete" => Some(CodingAgentStreamEvent::Completed {
            session_id,
            text: value.get("text")?.as_str()?.to_owned(),
            cost_usd: value.get("cost_usd").and_then(serde_json::Value::as_f64),
            duration_ms: value.get("duration_ms").and_then(serde_json::Value::as_u64),
        }),
        "error" => Some(CodingAgentStreamEvent::Error {
            session_id,
            message: value.get("message")?.as_str()?.to_owned(),
        }),
        "cancelled" => Some(CodingAgentStreamEvent::Cancelled { session_id }),
        // Started is emitted once by the Core runner; tool results can include
        // screenshots and must never be rendered as assistant response text.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_agent::{build_agent_command, CodingAgentProvider, PromptPayload};

    #[test]
    fn prompt_and_context_are_private_stdin_json_and_not_argv() {
        let mut request = CodingAgentRequest::new("session", "--help\n你好 `cmd` $(secret)");
        request.provider = CodingAgentProvider::PiBundled;
        request.executable = Some("old-global-cli".into());
        request.continue_session = true;
        request.continuation_context = Some("previous turn".into());
        let command = build_agent_command(&request).unwrap();
        assert_eq!(command.executable, "openless-pi");
        assert_eq!(command.argv, ["--request"]);
        let PromptPayload::Stdin(line) = command.prompt else {
            panic!("expected stdin")
        };
        assert_eq!(line.lines().count(), 1);
        let payload: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(payload["prompt"], request.prompt);
        assert_eq!(payload["continuation_context"], "previous turn");
        assert_eq!(payload["session_persistence"], false);
    }

    #[test]
    fn stream_preserves_unicode_and_never_exposes_tool_payloads() {
        assert!(
            matches!(parse_stream_line("local", r#"{"type":"delta","text":"你好"}"#),
            Some(CodingAgentStreamEvent::Delta { session_id, text }) if session_id == "local" && text == "你好")
        );
        assert!(
            matches!(parse_stream_line("local", r#"{"type":"complete","text":"完成","session_id":"remote"}"#),
            Some(CodingAgentStreamEvent::Completed { session_id, text, .. }) if session_id == "local" && text == "完成")
        );
        assert!(parse_stream_line("s", r#"{"type":"tool_result","data":"screenshot"}"#).is_none());
        assert!(parse_stream_line("s", "ordinary stdout").is_none());
        assert!(parse_stream_line("s", r#"{"type":"complete"}"#).is_none());
    }
}
