//! Translate canonical Core events without dropping sequence numbers or reading ahead.

use openless_core::ask::{
    AskActivity, AskEvent, AskEventKind, AskMessage, AskSnapshot, AskSubmission, AskTurn,
    AskTurnStatus,
};
use openless_core::BackendError;
use serde_json::{json, Value};

pub(super) fn error(error: BackendError) -> Value {
    let message = match error.message.as_str() {
        "askTurnInProgress" => {
            "Wait for the current answer or stop it before sending another message."
        }
        "askDeepThinkingUnsupported" => "The configured model does not support deep thinking.",
        "askImageInputUnsupported" => "The configured model does not support images in Ask.",
        "askRequestConflict" => "This request ID belongs to a different message.",
        "askConversationUnavailable" => "This conversation is no longer available.",
        "askIntentInvalid" => {
            "Could not determine what to do with this request. Try rephrasing it."
        }
        "askIntentToolUnsupported" => {
            "The configured model does not support the selected intent tool."
        }
        "askIntentTimeout" => "Intent detection took too long. Please try again.",
        "askEntrySettingsUnavailable" => {
            "Could not read Ask settings. Check Settings and try again."
        }
        _ => &error.message,
    };
    let mut value = json!({"code": error.code, "message": message});
    if matches!(
        error.message.as_str(),
        "askIntentInvalid"
            | "askIntentToolUnsupported"
            | "askIntentTimeout"
            | "askEntrySettingsUnavailable"
    ) {
        value["reason"] = json!(error.message);
    }
    value
}

fn working(status: Option<AskActivity>) -> &'static str {
    match status {
        Some(AskActivity::Thinking) => "thinking",
        Some(AskActivity::Compacting) => "compacting",
        Some(AskActivity::Generating) => "thinking",
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

fn turn(turn: &AskTurn, user: &AskMessage, assistant: &AskMessage) -> Value {
    json!({
        "turnId":turn.turn_id,
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
        "error":turn.error.clone().map(error),
    })
}

fn submission(value: &AskSubmission) -> Value {
    json!({
        "currentTurnId": value.current_turn_id,
        "route": value.route,
        "status": status(value.status, value.activity),
        "activity": value.activity,
        "searchStatus": value.search_status,
        "searchError": value.search_error.clone().map(error),
        "toolCallId": value.tool_call_id,
        "sources": value.sources,
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
        } => {
            json!({"type":"turnFinished","status":status(*terminal,None), "error":cause.clone().map(error)})
        }
    };
    packet["conversationId"] = json!(event.conversation_id);
    packet["sequence"] = json!(event.sequence);
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
        assert_eq!(packet["turnId"], json!(turn));
        assert_eq!(packet["toolCallId"], json!(tool));
        assert_eq!(packet["sequence"], 7);
        assert_eq!(packet["type"], "sourceChanged");
        assert_eq!(packet["source"]["readStatus"], "blocked");
        assert!(packet.get("evidence").is_none());
    }

    #[test]
    fn intent_error_can_be_localized_without_using_its_display_message() {
        let result = error(BackendError::new(
            openless_core::BackendErrorCode::Unsupported,
            "askIntentToolUnsupported",
        ));
        assert_eq!(result["reason"], "askIntentToolUnsupported");
        assert_ne!(result["message"], result["reason"]);
    }
}
