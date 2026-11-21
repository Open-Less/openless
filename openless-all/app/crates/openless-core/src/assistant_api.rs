//! The personal assistant boundary. Native focus, routing and text delivery stay in the host.

use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ask::AskCallUsage;
use crate::ask::{AskAttachment, AskCancellation, AskOptions, AskSource, AskTurnKey};
use crate::BackendError;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub role: AssistantRole,
    pub text: String,
}

/// A quotation supplied for this question. It never grants permission to replace text.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSelection {
    pub text: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub source_app: Option<String>,
    pub source_url: Option<String>,
}

pub struct AssistantRequest {
    pub key: AskTurnKey,
    pub client_request_id: String,
    pub text: String,
    /// Live conversation context, independent of the persistent-history preference.
    pub messages: Vec<AssistantMessage>,
    pub selection: Option<AssistantSelection>,
    pub attachments: Vec<AskAttachment>,
    pub options: AskOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantContextQuery {
    CurrentTime,
    System,
    ForegroundApp,
    BrowserTab,
    VoiceHistory { query: String, limit: usize },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AssistantContextResult {
    Available {
        data: serde_json::Value,
        source: String,
        as_of: String,
    },
    Unavailable {
        reason: String,
    },
    Denied,
}

/// Read-only, authorized data. Implementations retain native identities locally.
pub trait AssistantContextProvider: Send + Sync {
    fn read(
        &self,
        key: AskTurnKey,
        query: AssistantContextQuery,
        cancellation: AskCancellation,
    ) -> BoxFuture<'_, Result<AssistantContextResult, BackendError>>;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantActivity {
    Thinking,
    Writing,
    Compacting,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantToolStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

pub enum AssistantEvent {
    TextDelta(String),
    Activity(AssistantActivity),
    Tool {
        tool_call_id: Uuid,
        name: String,
        status: AssistantToolStatus,
        error: Option<BackendError>,
    },
    Source {
        tool_call_id: Uuid,
        source: AskSource,
    },
    Usage(AskCallUsage),
}

/// The sink is bound to request.key by the caller; it must reject late events.
pub type AssistantEventSink = Arc<dyn Fn(AssistantEvent) + Send + Sync>;

pub struct AssistantResponse {
    /// Complete final answer. A successful return is terminal, with no second review call.
    pub text: String,
}

/// Exactly one request: the implementation may call tools before returning a final answer.
/// Cancellation returns BackendErrorCode::Cancelled, never an answer containing "Esc".
pub trait AssistantRuntime: Send + Sync {
    fn run(
        &self,
        request: AssistantRequest,
        cancellation: AskCancellation,
        events: AssistantEventSink,
    ) -> BoxFuture<'_, Result<AssistantResponse, BackendError>>;
}
