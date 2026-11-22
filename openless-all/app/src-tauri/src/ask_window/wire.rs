//! Translate canonical Core events without dropping sequence numbers or reading ahead.

use openless_core::ask::{
    AskActivity, AskEvent, AskEventKind, AskMessage, AskSnapshot, AskSubmission, AskTurn,
    AskToolState, AskTurnStatus,
};
use openless_core::{BackendError, BackendErrorCode};
use serde_json::{json, Value};

pub(super) fn error(error: BackendError) -> Value {
    let public_message = match error.message.as_str() {
        "askTurnInProgress" => {
            "Wait for the current answer or stop it before sending another message."
        }
        "askDeepThinkingUnsupported" => "The configured model does not support deep thinking.",
        "askImageInputUnsupported" => "The configured model does not support images in Ask.",
        "askAnswerUnusable" | "askEmptyAnswer" | "askEmptyModelOutput" => "The model did not produce a usable answer.",
        "askAnswerReviewUnavailable" => "The answer could not be verified.",
        "askCancelled" => "This request was cancelled.",
        "askSessionChanged" => "This request is no longer active.",
        "askContextWindowUnknown" => "The configured model's context limit is unknown.",
        "askContextLimit" | "askCompactNotSmaller" => "The request exceeds the model's context limit.",
        "askContextDenied" => "Permission to read this context was not granted.",
        "askContextUnavailable" | "askContextFailed" => "The requested context is unavailable.",
        "askToolTimeout" => "The tool request took too long.",
        "askToolLimit" => "The tool request limit was reached.",
        "askToolFailed" => "The tool request failed.",
        "askUnexpectedToolCall" | "askInvalidToolCall" | "askModelRejectedTools" => "The model's tool request is not supported.",
        "askCalculationInvalid" | "askCalculatorFailed" => "The calculation could not be completed.",
        "askSearchDisabled" => "Web search is disabled.",
        "askPageBlocked" | "askPageReadFailed" => "The requested page could not be read.",
        "askReleaseInvalidRepository" | "askReleaseInvalidResponse" | "askReleaseNetworkError"
            | "askReleaseNotFound" | "askReleaseRateLimited" | "askReleaseRequestFailed"
            | "askReleaseTooLarge" => "Release information could not be retrieved.",
        "askImageUnsupported" => "This model and endpoint cannot accept the image. The draft and image were kept.",
        "askDraftUnusable" => "The model did not produce usable text. Nothing was written.",
        "askDraftReviewUnavailable" => "The generated text could not be reviewed and was not written.",
        "askDraftSearchUnavailable" => "No verifiable search evidence was available. Nothing was written.",
        "askSelectionUnavailable" => "The selection could not be verified. The original text was not changed.",
        "askSelectionRequired" => "No verified selection was available. The original text was not changed.",
        "askRequestConflict" => "This request ID belongs to a different message.",
        "askConversationUnavailable" => "This conversation is no longer available.",
        "askIntentInvalid" => {
            "Could not determine what to do with this request. Try rephrasing it."
        }
        "askIntentClarificationRequired" => "Clarify the requested destination or reselect the text to change.",
        "askIntentToolUnsupported" => {
            "The configured model does not support the selected intent tool."
        }
        "askIntentTimeout" => "Intent detection took too long. Please try again.",
        "askEntrySettingsUnavailable" => {
            "Could not read Ask settings. Check Settings and try again."
        }
        _ => "",
    };
    let (reason, message) = if public_message.is_empty() {
        // Provider messages/details may include URLs, credentials or private
        // tool evidence. Only the typed code crosses this public boundary.
        let message = match error.code {
            BackendErrorCode::InvalidArgument => "This request is invalid.",
            BackendErrorCode::InvalidState => "This request is no longer available.",
            BackendErrorCode::Busy => "Another request is in progress.",
            BackendErrorCode::Cancelled => "This request was cancelled.",
            BackendErrorCode::PermissionDenied => "Permission was not granted.",
            BackendErrorCode::Unsupported => "This operation is not supported.",
            BackendErrorCode::Provider => "The model or tool request failed.",
            BackendErrorCode::Persistence => "The result could not be saved.",
            BackendErrorCode::Platform => "The native operation failed.",
            BackendErrorCode::OutcomeUnknown => "The operation could not be confirmed.",
            BackendErrorCode::Internal => "The request could not be completed.",
        };
        (json!(error.code), message)
    } else {
        (json!(error.message), public_message)
    };
    json!({"code": error.code, "reason": reason, "message": message})
}

fn working(status: Option<AskActivity>) -> &'static str {
    match status {
        Some(AskActivity::Thinking) => "thinking",
        Some(AskActivity::Compacting) => "compacting",
        Some(AskActivity::Generating) => "thinking",
        Some(AskActivity::Polishing) => "polishing",
        Some(AskActivity::Preparing) | None => "queued",
    }
}

fn status(status: AskTurnStatus, activity: Option<AskActivity>) -> &'static str {
    match status {
        AskTurnStatus::Running => working(activity),
        AskTurnStatus::Completed => "completed",
        AskTurnStatus::Cancelled => "cancelled",
        AskTurnStatus::Failed => "failed",
    }
}

fn tool(tool: &AskToolState) -> Value {
    json!({
        "toolCallId": tool.tool_call_id,
        "name": tool.name,
        "status": tool.status,
        "error": tool.error.clone().map(error),
    })
}

fn turn(turn: &AskTurn, user: &AskMessage, assistant: &AskMessage) -> Value {
    json!({
        "turnId":turn.turn_id,
        "clientRequestId":turn.client_request_id,
        "userMessage":{"messageId": user.message_id, "text":user.text},
        "assistantMessage":{"messageId":assistant.message_id, "text":assistant.text},
        "attachments":turn.attachments,
        "deepThinking":turn.deep_thinking,
        "status":if turn.status == AskTurnStatus::Running && !assistant.text.is_empty() {
            "writing"
        } else {
            status(turn.status, turn.activity)
        },
        "searchStatus":turn.search_status,
        "searchError":turn.search_error.clone().map(error),
        "sources":turn.sources,
        "tools":turn.tools.iter().map(tool).collect::<Vec<_>>(),
        "error":turn.error.clone().map(error),
    })
}

fn submission(value: &AskSubmission) -> Value {
    json!({
        "currentTurnId": value.current_turn_id,
        "clientRequestId": value.client_request_id,
        "route": value.route,
        "status": status(value.status, value.activity),
        "activity": value.activity,
        "searchStatus": value.search_status,
        "searchError": value.search_error.clone().map(error),
        "toolCallId": value.tool_call_id,
        "sources": value.sources,
        "tools": value.tools.iter().map(tool).collect::<Vec<_>>(),
        "resultText": value.result_text,
        "delivery": value.delivery,
        "copyStatus": value.copy_status,
        "error": value.error.clone().map(error),
    })
}

pub(super) fn snapshot(snapshot: AskSnapshot) -> Value {
    let messages: std::collections::HashMap<_, _> = snapshot
        .messages
        .iter()
        .map(|m| (m.message_id, m))
        .collect();
    let turns = snapshot
        .turns
        .iter()
        .filter_map(|view| {
            Some(turn(
                view,
                messages.get(&view.user_message_id)?,
                messages.get(&view.assistant_message_id)?,
            ))
        })
        .collect::<Vec<_>>();
    json!({
        "conversationId": snapshot.conversation_id,
        "sequence": snapshot.sequence,
        "activeTurnId": snapshot.current_turn_id,
        "turns": turns,
        "attachments": snapshot.attachments,
        "cache": snapshot.cache,
        "submission": snapshot.submission.as_ref().map(submission),
    })
}

pub(super) fn event(event: &AskEvent) -> Value {
    let mut packet = match &event.kind {
        AskEventKind::Submitted { submission: value } => {
            json!({"type":"submitted","submission":submission(value)})
        }
        AskEventKind::RouteResolved { route } => json!({"type":"routeResolved","route":route}),
        AskEventKind::Delivery { submission: value } => {
            json!({"type":"delivery","submission":submission(value)})
        }
        AskEventKind::Accepted {
            user,
            assistant,
            turn: view,
        } => json!({"type":"turnAccepted","turn":turn(view,user,assistant)}),
        AskEventKind::AnswerDelta { message_id, delta } => {
            json!({"type":"answerDelta","messageId":message_id,"text":delta})
        }
        AskEventKind::Activity { activity } => {
            json!({"type":"turnStatus","status":working(Some(*activity))})
        }
        AskEventKind::Search {
            tool_call_id,
            status,
            error: cause,
        } => {
            json!({"type":"searchChanged","toolCallId":tool_call_id,"status":status,"error":cause.clone().map(error)})
        }
        AskEventKind::Source {
            tool_call_id,
            source,
        } => json!({"type":"sourceChanged","toolCallId":tool_call_id,"source":source}),
        AskEventKind::Tool { tool_call_id, name, status, error: cause } => {
            json!({"type":"toolChanged","toolCallId":tool_call_id,"name":name,"status":status,"error":cause.clone().map(error)})
        }
        AskEventKind::Usage { .. } => json!({"type":"usageChanged"}),
        AskEventKind::Attachment { attachment } => {
            json!({"type":"attachmentChanged","attachment":attachment})
        }
        AskEventKind::AttachmentRemoved { attachment_id } => {
            json!({"type":"attachmentRemoved","attachmentId":attachment_id})
        }
        AskEventKind::Terminal {
            status: terminal,
            error: cause,
            text,
        } => {
            json!({"type":"turnFinished","status":status(*terminal,None), "error":cause.clone().map(error), "text":text})
        }
    };
    packet["conversationId"] = json!(event.conversation_id);
    packet["sequence"] = json!(event.sequence);
    if let Some(id) = &event.client_request_id {
        packet["clientRequestId"] = json!(id);
    }
    if let Some(id) = event.turn_id {
        packet["turnId"] = json!(id);
    }
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::ask::{AskReadStatus, AskSource};
    use uuid::Uuid;

    #[test]
    fn search_source_updates_keep_the_scoped_sequence_and_actual_read_status() {
        let conversation = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let tool = Uuid::new_v4();
        let packet = event(&AskEvent {
            conversation_id: conversation,
            client_request_id: Some("request-a".into()),
            sequence: 7,
            turn_id: Some(turn),
            kind: AskEventKind::Source {
                tool_call_id: tool,
                source: AskSource {
                    source_id: "result-1".into(),
                    url: "https://example.com/".into(),
                    title: "Result".into(),
                    snippet: None,
                    read_status: AskReadStatus::Blocked,
                },
            },
        });
        assert_eq!(packet["conversationId"], json!(conversation));
        assert_eq!(packet["clientRequestId"], "request-a");
        assert_eq!(packet["turnId"], json!(turn));
        assert_eq!(packet["toolCallId"], json!(tool));
        assert_eq!(packet["sequence"], 7);
        assert_eq!(packet["type"], "sourceChanged");
        assert_eq!(packet["source"]["readStatus"], "blocked");
        assert!(packet.get("evidence").is_none());
    }

    #[test]
    fn intent_error_can_be_localized_without_using_its_display_message() {
        for reason in ["askIntentToolUnsupported", "askSelectionUnavailable", "askDraftReviewUnavailable", "askImageUnsupported"] {
            let result = error(BackendError::new(openless_core::BackendErrorCode::Unsupported, reason));
            assert_eq!(result["reason"], reason);
            assert_ne!(result["message"], result["reason"]);
        }
    }

    #[test]
    fn unknown_provider_errors_expose_a_stable_reason_without_private_evidence() {
        let mut cause = BackendError::new(BackendErrorCode::Provider, "secret provider URL and prompt");
        cause.details = Some(json!({"evidence":"private document", "token":"secret"}));
        let result = error(cause);
        assert_eq!(result["reason"], "provider");
        assert_eq!(result["code"], "provider");
        assert!(!result.to_string().contains("secret"));
        assert!(result.get("details").is_none());
    }

    #[test]
    fn known_request_errors_always_have_symbolic_reasons() {
        for reason in ["askTurnInProgress", "askRequestConflict", "askConversationUnavailable"] {
            assert_eq!(error(BackendError::new(BackendErrorCode::InvalidState, reason))["reason"], reason);
        }
    }

    #[test]
    fn generic_tool_events_and_replay_share_public_status_and_redacted_errors() {
        use openless_core::assistant_api::AssistantToolStatus;
        for status in [
            AssistantToolStatus::Running, AssistantToolStatus::Completed,
            AssistantToolStatus::Failed, AssistantToolStatus::Cancelled,
        ] {
            let state = AskToolState {
                tool_call_id: Uuid::new_v4(),
                name: "current_time".into(),
                status,
                error: Some(BackendError::new(BackendErrorCode::Provider, "private evidence")),
            };
            let conversation = Uuid::new_v4();
            let turn = Uuid::new_v4();
            let packet = event(&AskEvent {
                conversation_id: conversation,
                client_request_id: Some("request-tool".into()),
                sequence: 9,
                turn_id: Some(turn),
                kind: AskEventKind::Tool {
                    tool_call_id: state.tool_call_id, name: state.name.clone(),
                    status, error: state.error.clone(),
                },
            });
            assert_eq!(packet["type"], "toolChanged");
            assert_eq!(packet["conversationId"], json!(conversation));
            assert_eq!(packet["clientRequestId"], "request-tool");
            assert_eq!(packet["turnId"], json!(turn));
            assert_eq!(packet["sequence"], 9);
            let replay = tool(&state);
            for field in ["toolCallId", "name", "status", "error"] {
                assert_eq!(packet[field], replay[field]);
            }
            assert_eq!(packet["error"]["reason"], "provider");
            assert!(!packet.to_string().contains("private evidence"));
        }
    }

    #[test]
    fn terminal_event_carries_the_authoritative_answer_and_request_identity() {
        let packet = event(&AskEvent {
            conversation_id: Uuid::new_v4(),
            turn_id: Some(Uuid::new_v4()),
            client_request_id: Some("request-final".into()),
            sequence: 11,
            kind: AskEventKind::Terminal {
                status: AskTurnStatus::Completed,
                error: None,
                text: Some("complete final answer".into()),
            },
        });
        assert_eq!(packet["type"], "turnFinished");
        assert_eq!(packet["text"], "complete final answer");
        assert_eq!(packet["clientRequestId"], "request-final");
        assert_eq!(packet["sequence"], 11);
    }
}
