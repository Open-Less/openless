use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use crate::errors::{BackendError, BackendErrorCode};
use crate::usage_ledger::{
    sanitized_identifier, CacheInputAccounting, ReportedTokens, ServiceKind, ServiceOutcome,
    ServicePurpose, UsageRecord, UsageRecorder,
};
use crate::{Clock, TaskSpawner};

mod calculator;
mod entry;
mod model;
mod prompt;
#[cfg(test)]
mod tests;

pub use entry::*;
pub use model::ConfiguredAskModels;

const MAX_ATTACHMENTS: usize = 4;
const WEB_SEARCH_GUIDANCE: &str = "Consider web_search for every question. Prefer to verify time-sensitive or changeable public facts before answering, including news, weather, prices, schedules, rules, and current software versions or documentation. Also use it to verify relevant public facts or find sources when useful, even if you think you know the answer. Skip greetings, purely creative requests, and questions with no useful public query. Send only a short public query, never private data. If you also need a calculation based on current information, verify the inputs first.";
const CALCULATOR_GUIDANCE: &str = "Use calculator for precise arithmetic, including large numbers, fractions, percentages (write them as /100), and integer powers. Call one tool at a time. Use the exact result where useful and label rounded decimals as approximate. If the tool reports an error, correct the expression or explain the limitation; do not invent a result.";
const MAX_CALCULATIONS: usize = 3;
const MAX_REQUEST_ID_BYTES: usize = 128;
const EVENT_LIMIT: usize = 512;
const EVENT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct AskCancellation(watch::Sender<bool>);

impl Default for AskCancellation {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}

impl AskCancellation {
    fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskTurnKey {
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskOptions {
    pub deep_thinking: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSubmit {
    pub conversation_id: Uuid,
    pub client_request_id: String,
    pub text: String,
    pub attachment_ids: Vec<Uuid>,
    pub options: AskOptions,
}

pub struct AskContext {
    pub static_version: String,
    pub static_instructions: String,
    pub facts: Vec<AskContextFact>,
    pub missing: Vec<AskMissingFact>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskContextFact {
    pub name: String,
    pub value: serde_json::Value,
    pub source: String,
    pub as_of: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskMissingFact {
    pub name: String,
    pub reason: String,
}

pub trait ContextProvider: Send + Sync {
    fn context(
        &self,
        turn: AskTurnKey,
        cancellation: AskCancellation,
    ) -> BoxFuture<'_, Result<AskContext, BackendError>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskImageMime {
    Jpeg,
    Png,
}

impl AskImageMime {
    fn media_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

#[derive(Clone)]
pub struct AskImage {
    pub mime: AskImageMime,
    pub bytes: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}

pub struct AskPdfPage {
    pub number: u32,
    pub text: Arc<str>,
}

pub enum AskAttachment {
    Selection { text: Arc<str> },
    Pdf { pages: Vec<AskPdfPage> },
    Image(AskImage),
}

pub trait AttachmentReader: Send + Sync {
    fn acquire(
        &self,
        turn: AskTurnKey,
        attachment_ids: &[Uuid],
    ) -> Result<Vec<AskAttachment>, BackendError>;

    fn end_conversation(&self, conversation_id: Uuid);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskAttachmentKind {
    Selection,
    Pdf,
    Image,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskAttachmentStatus {
    Processing,
    Ready,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AskAttachmentPreview {
    Selection {
        #[serde(rename = "text")]
        excerpt: String,
        has_more: bool,
    },
    Image {
        width: u32,
        height: u32,
        thumbnail_available: bool,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAttachmentMetadata {
    pub attachment_id: Uuid,
    pub kind: AskAttachmentKind,
    pub display_name: String,
    pub preview: Option<AskAttachmentPreview>,
    pub page_count: Option<u32>,
    pub source_app: Option<String>,
    pub state: AskAttachmentStatus,
    pub error: Option<BackendError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskSearchStatus {
    Idle,
    Searching,
    Reading,
    Done,
    NoResults,
    Error,
    Cancelled,
}

impl AskSearchStatus {
    fn active(&self) -> bool {
        matches!(self, Self::Searching | Self::Reading)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskReadStatus {
    #[serde(rename = "not_attempted")]
    Discovered,
    Reading,
    Read,
    Blocked,
    #[serde(rename = "failed")]
    Error,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSource {
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub snippet: Option<String>,
    pub read_status: AskReadStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskToolKey {
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
    pub tool_call_id: Uuid,
}

#[derive(Clone)]
pub enum AskToolCall {
    WebSearch { public_query: String },
    Calculate { expression: String },
    ResolveIntent { route: AskRoute },
}

impl AskToolCall {
    fn name(&self) -> &'static str {
        match self {
            Self::WebSearch { .. } => "web_search",
            Self::Calculate { .. } => "calculator",
            Self::ResolveIntent { .. } => "resolve_intent",
        }
    }
}

pub struct AskSearchRequest {
    pub key: AskToolKey,
    pub public_query: String,
    pub evidence_token_budget: usize,
}

pub enum AskToolProgress {
    Status(AskSearchStatus),
    Source(AskSource),
}

pub enum AskSearchResult {
    Disabled,
    Done { evidence: String },
    NoResults,
}

pub type AskToolSink = Arc<dyn Fn(AskToolProgress) + Send + Sync>;

pub trait SearchTool: Send + Sync {
    fn enabled(&self) -> BoxFuture<'_, bool> {
        Box::pin(async { true })
    }

    fn search(
        &self,
        request: AskSearchRequest,
        cancellation: AskCancellation,
        progress: AskToolSink,
    ) -> BoxFuture<'_, Result<AskSearchResult, BackendError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskModelLabel {
    pub service: String,
    pub channel: String,
    pub model: String,
    pub protocol: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskModelCapabilities {
    pub context_window: Option<usize>,
    pub output_tokens: usize,
    pub image_input: bool,
    pub image_token_reserve: usize,
    pub deep_thinking: bool,
}

#[derive(Clone)]
pub enum AskModelPart {
    Text(String),
    Image(AskImage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskModelRole {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct AskModelMessage {
    pub role: AskModelRole,
    pub parts: Vec<AskModelPart>,
}

#[derive(Clone)]
pub struct AskModelRequest {
    pub instructions: String,
    pub messages: Vec<AskModelMessage>,
    pub output_tokens: usize,
    pub deep_thinking: bool,
    pub tools: AskModelTools,
}

#[derive(Clone, Copy, Default)]
pub struct AskToolSet {
    pub search: bool,
    pub calculator: bool,
    pub resolve_intent: bool,
}

impl AskToolSet {
    fn any(self) -> bool {
        self.search || self.calculator || self.resolve_intent
    }

    fn allows(self, call: &AskToolCall) -> bool {
        match call {
            AskToolCall::WebSearch { .. } => self.search,
            AskToolCall::Calculate { .. } => self.calculator,
            AskToolCall::ResolveIntent { .. } => self.resolve_intent,
        }
    }
}

#[derive(Clone, Default)]
pub struct AskModelTools {
    pub available: AskToolSet,
    pub results: Vec<AskToolResult>,
    pub intent: Option<AskIntentContinuation>,
}

#[derive(Clone)]
pub struct AskToolResult {
    pub call: AskModelToolCall,
    pub output: String,
}

#[derive(Clone)]
pub struct AskModelToolCall {
    pub tool: AskToolCall,
    pub id: Option<String>,
    pub resume: Vec<serde_json::Value>,
}

pub enum AskModelProgress {
    Text(String),
    Thinking,
    Usage(ReportedTokens),
}

pub type AskModelSink = Arc<dyn Fn(AskModelProgress) + Send + Sync>;

pub enum AskModelCompletion {
    Complete,
    ToolCall(AskModelToolCall),
}

enum CallResult {
    Text(String),
    Tool(AskModelToolCall),
}

impl CallResult {
    fn require_text(self) -> Result<String, BackendError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Tool(_) => Err(ask_error(
                BackendErrorCode::Provider,
                "askUnexpectedToolCall",
            )),
        }
    }
}

pub trait AskModel: Send + Sync {
    fn label(&self) -> &AskModelLabel;
    fn capabilities(&self) -> AskModelCapabilities;
    fn supports_tools(&self) -> bool {
        false
    }
    fn supports_intent_tool(&self) -> bool {
        self.supports_tools()
    }
    fn stream(
        &self,
        request: AskModelRequest,
        cancellation: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>>;
}

pub trait AskModelFactory: Send + Sync {
    fn prepare(
        &self,
        deep_thinking: bool,
    ) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>>;
}

pub type AskHistorySink = dyn Fn(&str, &str) + Send + Sync;

pub struct StandaloneQaDependencies {
    pub models: Arc<dyn AskModelFactory>,
    pub context: Arc<dyn ContextProvider>,
    pub search: Arc<dyn SearchTool>,
    pub attachments: Arc<dyn AttachmentReader>,
    pub spawner: Arc<dyn TaskSpawner>,
    pub history: Option<Arc<AskHistorySink>>,
    pub usage: Option<AskUsagePersistence>,
}

pub struct AskUsagePersistence {
    pub recorder: Arc<UsageRecorder>,
    pub clock: Arc<dyn Clock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskTurnStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskActivity {
    Preparing,
    Compacting,
    Generating,
    Thinking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskCallPurpose {
    Answer,
    Compose,
    Intent,
    Compact,
    SearchDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskCallUsage {
    pub call_id: Uuid,
    pub purpose: AskCallPurpose,
    pub model: AskModelLabel,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached_input: Option<u64>,
    pub cache_write_input: Option<u64>,
    pub reasoning: Option<u64>,
    pub input_accounting: Option<CacheInputAccounting>,
}

impl AskCallUsage {
    fn replace_tokens(&mut self, tokens: ReportedTokens) {
        self.input = tokens.input;
        self.output = tokens.output;
        self.cached_input = tokens.cached_input;
        self.cache_write_input = tokens.cache_write_input;
        self.reasoning = tokens.reasoning;
        self.input_accounting = tokens.input_accounting;
    }

    fn comparable(&self) -> Option<(u64, u64)> {
        let read = self.cached_input?;
        let input = self.input?;
        let total = match self.input_accounting? {
            CacheInputAccounting::Inclusive => input,
            CacheInputAccounting::ExcludesCache => input
                .checked_add(read)?
                .checked_add(self.cache_write_input?)?,
        };
        (total > 0 && read <= total).then_some((read, total))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskCacheMetric {
    pub model: AskModelLabel,
    pub comparable_reports: usize,
    pub hit_rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskMessage {
    pub message_id: Uuid,
    pub turn_id: Uuid,
    pub role: &'static str,
    pub text: String,
    pub attachment_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskTurn {
    pub turn_id: Uuid,
    pub user_message_id: Uuid,
    pub assistant_message_id: Uuid,
    pub deep_thinking: bool,
    pub attachments: Vec<AskSentAttachment>,
    pub status: AskTurnStatus,
    pub activity: Option<AskActivity>,
    pub search_status: AskSearchStatus,
    pub search_error: Option<BackendError>,
    pub tool_call_id: Option<Uuid>,
    pub sources: Vec<AskSource>,
    pub error: Option<BackendError>,
    pub usage: Vec<AskCallUsage>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSentAttachment {
    pub attachment_id: Uuid,
    pub kind: AskAttachmentKind,
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSnapshot {
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub current_turn_id: Option<Uuid>,
    pub submission: Option<AskSubmission>,
    pub messages: Vec<AskMessage>,
    pub turns: Vec<AskTurn>,
    pub attachments: Vec<AskAttachmentMetadata>,
    pub cache: Vec<AskCacheMetric>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskEvent {
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub turn_id: Option<Uuid>,
    #[serde(flatten)]
    pub kind: AskEventKind,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AskEventKind {
    Submitted {
        submission: Box<AskSubmission>,
    },
    RouteResolved {
        route: AskRoute,
    },
    Delivery {
        submission: Box<AskSubmission>,
    },
    Accepted {
        user: AskMessage,
        assistant: AskMessage,
        turn: Box<AskTurn>,
    },
    AnswerDelta {
        message_id: Uuid,
        delta: String,
    },
    Activity {
        activity: AskActivity,
    },
    Search {
        tool_call_id: Uuid,
        status: AskSearchStatus,
        error: Option<BackendError>,
    },
    Source {
        tool_call_id: Uuid,
        source: AskSource,
    },
    Usage {
        usage: AskCallUsage,
    },
    Attachment {
        attachment: AskAttachmentMetadata,
    },
    AttachmentRemoved {
        attachment_id: Uuid,
    },
    Terminal {
        status: AskTurnStatus,
        error: Option<BackendError>,
    },
}

impl AskEventKind {
    fn bytes(&self) -> usize {
        match self {
            Self::Accepted { user, .. } => user.text.len(),
            Self::Delivery { submission } => {
                submission.result_text.as_ref().map_or(0, String::len)
                    + submission
                        .sources
                        .iter()
                        .map(|source| {
                            source.url.len()
                                + source.title.len()
                                + source.snippet.as_ref().map_or(0, String::len)
                        })
                        .sum::<usize>()
            }
            Self::AnswerDelta { delta, .. } => delta.len(),
            Self::Source { source, .. } => {
                source.url.len()
                    + source.title.len()
                    + source.snippet.as_ref().map_or(0, String::len)
            }
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskReplay {
    pub events: Vec<AskEvent>,
    pub latest_sequence: u64,
    pub truncated: bool,
}

#[derive(Clone)]
pub struct StandaloneQaService(Arc<Service>);

struct Service {
    deps: StandaloneQaDependencies,
    state: Mutex<State>,
    events: broadcast::Sender<Arc<AskEvent>>,
}

#[derive(Default)]
struct State {
    current: Option<Uuid>,
    conversations: HashMap<Uuid, Conversation>,
}

#[derive(Default)]
struct Conversation {
    admission: Arc<tokio::sync::Mutex<()>>,
    lifetime: AskCancellation,
    sequence: u64,
    event_bytes: usize,
    events: VecDeque<Arc<AskEvent>>,
    turns: Vec<Turn>,
    requests: HashMap<String, usize>,
    active: Option<usize>,
    summary: Option<prompt::Summary>,
    contains_private_context: bool,
    attachments: Vec<AskAttachmentMetadata>,
}

struct Turn {
    request: AskSubmit,
    user: AskMessage,
    assistant: AskMessage,
    snapshot: AskTurn,
    cancellation: AskCancellation,
    search_open: bool,
    route: Option<AskRoute>,
    result_text: Option<String>,
    delivery: Option<AskDeliveryStatus>,
    copy_status: AskCopyStatus,
    effect_claimed: bool,
    pending_body: Option<String>,
}

enum AskEffect {
    Write,
    Copy,
}

impl StandaloneQaService {
    pub fn new(deps: StandaloneQaDependencies) -> Self {
        Self(Arc::new(Service {
            deps,
            state: Mutex::new(State::default()),
            events: broadcast::channel(EVENT_LIMIT).0,
        }))
    }

    pub fn open(&self) -> Uuid {
        let mut state = self.0.state.lock();
        state.current.unwrap_or_else(|| state.new_conversation())
    }

    pub fn current(&self) -> Option<Uuid> {
        self.0.state.lock().current
    }

    pub fn new_conversation(&self) -> Uuid {
        self.0.state.lock().new_conversation()
    }

    pub fn end_conversation(&self, conversation_id: Uuid) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state
            .conversations
            .remove(&conversation_id)
            .ok_or_else(no_conversation)?;
        conversation.lifetime.cancel();
        if let Some(active) = conversation.active {
            conversation.turns[active].cancellation.cancel();
        }
        if state.current == Some(conversation_id) {
            state.current = None;
        }
        drop(state);
        self.0.deps.attachments.end_conversation(conversation_id);
        Ok(())
    }

    pub fn end_all(&self) {
        let mut state = self.0.state.lock();
        state.current = None;
        let ids = state
            .conversations
            .drain()
            .map(|(id, conversation)| {
                conversation.lifetime.cancel();
                if let Some(active) = conversation.active {
                    conversation.turns[active].cancellation.cancel();
                }
                id
            })
            .collect::<Vec<_>>();
        drop(state);
        for id in ids {
            self.0.deps.attachments.end_conversation(id);
        }
    }

    pub fn cancel_active_turns(&self) {
        let active = {
            let state = self.0.state.lock();
            state
                .conversations
                .iter()
                .filter_map(|(&id, conversation)| {
                    let index = conversation.active?;
                    Some(AskTurnKey {
                        conversation_id: id,
                        turn_id: conversation.turns[index].snapshot.turn_id,
                    })
                })
                .collect::<Vec<_>>()
        };
        for key in active {
            let _ = self.cancel(key);
        }
    }

    pub async fn model_capabilities(
        &self,
    ) -> Result<(AskModelLabel, AskModelCapabilities), BackendError> {
        let model = self.0.deps.models.prepare(false).await?;
        Ok((model.label().clone(), model.capabilities()))
    }

    pub fn publish_attachment(
        &self,
        conversation_id: Uuid,
        attachment: AskAttachmentMetadata,
        register: bool,
    ) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state.conversation_mut(conversation_id)?;
        let stored = conversation
            .attachments
            .iter_mut()
            .find(|stored| stored.attachment_id == attachment.attachment_id);
        match (register, stored) {
            (true, None) => conversation.attachments.push(attachment.clone()),
            (false, Some(stored)) => *stored = attachment.clone(),
            _ => return Ok(()),
        }
        conversation.emit(
            &self.0.events,
            conversation_id,
            None,
            AskEventKind::Attachment { attachment },
        );
        Ok(())
    }

    pub fn publish_attachment_removed(
        &self,
        conversation_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state.conversation_mut(conversation_id)?;
        if let Some(index) = conversation
            .attachments
            .iter()
            .position(|current| current.attachment_id == attachment_id)
        {
            conversation.attachments.remove(index);
            conversation.emit(
                &self.0.events,
                conversation_id,
                None,
                AskEventKind::AttachmentRemoved { attachment_id },
            );
        }
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<AskEvent>> {
        self.0.events.subscribe()
    }

    pub fn snapshot(&self, conversation_id: Uuid) -> Result<AskSnapshot, BackendError> {
        let state = self.0.state.lock();
        let conversation = state.conversation(conversation_id)?;
        Ok(AskSnapshot {
            conversation_id,
            sequence: conversation.sequence,
            current_turn_id: conversation
                .active
                .map(|index| conversation.turns[index].snapshot.turn_id),
            submission: conversation.turns.last().map(Turn::submission),
            messages: conversation
                .turns
                .iter()
                .filter(|turn| turn.is_chat())
                .flat_map(|turn| [turn.user.clone(), turn.assistant.clone()])
                .collect(),
            turns: conversation
                .turns
                .iter()
                .filter(|turn| turn.is_chat())
                .map(|turn| turn.snapshot.clone())
                .collect(),
            attachments: conversation.attachments.clone(),
            cache: conversation.cache(),
        })
    }

    pub fn events(
        &self,
        conversation_id: Uuid,
        after_sequence: u64,
    ) -> Result<AskReplay, BackendError> {
        let state = self.0.state.lock();
        let conversation = state.conversation(conversation_id)?;
        let truncated = after_sequence > conversation.sequence
            || (after_sequence < conversation.sequence
                && conversation
                    .events
                    .front()
                    .is_none_or(|first| first.sequence > after_sequence + 1));
        Ok(AskReplay {
            events: conversation
                .events
                .iter()
                .filter(|event| event.sequence > after_sequence)
                .map(|event| (**event).clone())
                .collect(),
            latest_sequence: conversation.sequence,
            truncated,
        })
    }

    pub async fn submit(&self, request: AskSubmit) -> Result<Uuid, BackendError> {
        self.submit_guarded(request, || true).await
    }

    pub async fn submit_guarded(
        &self,
        request: AskSubmit,
        admit: impl FnOnce() -> bool + Send,
    ) -> Result<Uuid, BackendError> {
        self.submit_inner(
            request,
            || async { Ok(AskPreparation::default()) },
            admit,
            true,
        )
        .await
    }

    pub async fn submit_prepared<F, Fut>(
        &self,
        request: AskSubmit,
        prepare: F,
    ) -> Result<Uuid, BackendError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<AskPreparation, BackendError>> + Send,
    {
        self.submit_inner(request, prepare, || true, false).await
    }

    async fn submit_inner<F, Fut>(
        &self,
        request: AskSubmit,
        prepare: F,
        admit: impl FnOnce() -> bool + Send,
        answer_only: bool,
    ) -> Result<Uuid, BackendError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<AskPreparation, BackendError>> + Send,
    {
        let (admission, lifetime) = {
            let state = self.0.state.lock();
            let conversation = state.conversation(request.conversation_id)?;
            (
                conversation.admission.clone(),
                conversation.lifetime.clone(),
            )
        };
        let _admission = tokio::select! {
            biased;
            _ = lifetime.cancelled() => return Err(no_conversation()),
            guard = admission.lock() => guard,
        };
        {
            let state = self.0.state.lock();
            let conversation = state.conversation(request.conversation_id)?;
            if let Some(index) = conversation.requests.get(&request.client_request_id) {
                let original = &conversation.turns[*index];
                return if original.request == request {
                    Ok(original.snapshot.turn_id)
                } else {
                    Err(ask_error(
                        BackendErrorCode::InvalidArgument,
                        "askRequestConflict",
                    ))
                };
            }
            if conversation.active.is_some() {
                return Err(ask_error(BackendErrorCode::Busy, "askTurnInProgress").retryable(true));
            }
        }
        validate_submit(&request)?;
        let key = AskTurnKey {
            conversation_id: request.conversation_id,
            turn_id: Uuid::new_v4(),
        };
        let preparation = tokio::select! {
            biased;
            _ = lifetime.cancelled() => return Err(no_conversation()),
            preparation = async { prepare().await } => preparation?,
        };
        let model = tokio::select! {
            biased;
            _ = lifetime.cancelled() => return Err(no_conversation()),
            model = self.0.deps.models.prepare(request.options.deep_thinking) => model?,
        };
        let caps = model.capabilities();
        if request.options.deep_thinking && !caps.deep_thinking {
            return Err(ask_error(
                BackendErrorCode::Unsupported,
                "askDeepThinkingUnsupported",
            ));
        }
        let attachments = if request.attachment_ids.is_empty() {
            Vec::new()
        } else {
            self.0
                .deps
                .attachments
                .acquire(key, &request.attachment_ids)?
        };
        if attachments.len() != request.attachment_ids.len() {
            return Err(ask_error(
                BackendErrorCode::InvalidState,
                "askAttachmentUnavailable",
            ));
        }
        let material = prompt::Material::new(&request.text, attachments, caps, &request.options)?;
        let cancellation = AskCancellation::default();
        {
            let mut state = self.0.state.lock();
            let conversation = state.conversation_mut(key.conversation_id)?;
            if !admit() {
                return Err(ask_error(
                    BackendErrorCode::Cancelled,
                    "askVoiceTargetChanged",
                ));
            }
            let mut turn = Turn::new(key.turn_id, request, cancellation.clone());
            turn.snapshot.attachments = turn
                .request
                .attachment_ids
                .iter()
                .filter_map(|id| {
                    conversation
                        .attachments
                        .iter()
                        .find(|attachment| attachment.attachment_id == *id)
                        .map(|attachment| AskSentAttachment {
                            attachment_id: *id,
                            kind: attachment.kind,
                            display_name: attachment.display_name.clone(),
                        })
                })
                .collect();
            conversation.attachments.retain(|attachment| {
                !turn
                    .request
                    .attachment_ids
                    .contains(&attachment.attachment_id)
            });
            if let Some(previous) = conversation.turns.last_mut() {
                previous.result_text = None;
            }
            let event = AskEventKind::Submitted {
                submission: Box::new(turn.submission()),
            };
            let index = conversation.turns.len();
            conversation
                .requests
                .insert(turn.request.client_request_id.clone(), index);
            conversation.turns.push(turn);
            conversation.active = Some(index);
            conversation.emit_turn(&self.0.events, key, event);
        }
        let service = self.clone();
        let guard = RunningTurn {
            service: service.clone(),
            key,
        };
        self.0.deps.spawner.spawn(Box::pin(async move {
            let _guard = guard;
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(cancelled()),
                result = service.run(key, model, material, &preparation, answer_only, cancellation.clone()) => result,
            };
            let result = match result {
                Ok(Some((route, body))) => service.deliver(key, route, body, preparation).await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            service.finish(key, result);
        }));
        Ok(key.turn_id)
    }

    pub fn cancel(&self, key: AskTurnKey) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state.conversation_mut(key.conversation_id)?;
        if let Some(turn) = conversation.active_turn(key.turn_id) {
            turn.cancellation.cancel();
            if !turn.effect_claimed {
                conversation.finish(&self.0.events, key, Err(cancelled()));
            }
        } else if !conversation
            .turns
            .iter()
            .any(|turn| turn.snapshot.turn_id == key.turn_id)
        {
            return Err(no_turn());
        }
        Ok(())
    }

    pub fn turn(&self, key: AskTurnKey) -> Result<AskTurn, BackendError> {
        self.0
            .state
            .lock()
            .conversation(key.conversation_id)?
            .turns
            .iter()
            .find(|turn| turn.snapshot.turn_id == key.turn_id)
            .map(|turn| turn.snapshot.clone())
            .ok_or_else(no_turn)
    }

    pub fn source_url(&self, key: AskTurnKey, source_id: &str) -> Result<String, BackendError> {
        self.turn(key)?
            .sources
            .into_iter()
            .find(|source| source.source_id == source_id)
            .and_then(|source| public_url(&source.url))
            .ok_or_else(|| ask_error(BackendErrorCode::InvalidArgument, "askSourceNotFound"))
    }

    fn update(&self, key: AskTurnKey, update: impl FnOnce(&mut Turn) -> Option<AskEventKind>) {
        let mut state = self.0.state.lock();
        if let Ok(conversation) = state.conversation_mut(key.conversation_id) {
            if let Some(event) = conversation.active_turn(key.turn_id).and_then(update) {
                conversation.emit_turn(&self.0.events, key, event);
            }
        }
    }

    fn activity(&self, key: AskTurnKey, activity: AskActivity) {
        self.update(key, |turn| turn.activity(activity));
    }

    fn finish(&self, key: AskTurnKey, result: Result<(), BackendError>) {
        let completed = self
            .0
            .state
            .lock()
            .conversation_mut(key.conversation_id)
            .ok()
            .and_then(|conversation| conversation.finish(&self.0.events, key, result));
        if let (Some(history), Some((question, answer))) = (&self.0.deps.history, completed) {
            history(&question, &answer);
        }
    }

    fn publish_route(&self, key: AskTurnKey, route: AskRoute) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state.conversation_mut(key.conversation_id)?;
        let turn = conversation
            .active_turn(key.turn_id)
            .ok_or_else(cancelled)?;
        turn.route = Some(route);
        let accepted = route.is_chat().then(|| AskEventKind::Accepted {
            user: turn.user.clone(),
            assistant: turn.assistant.clone(),
            turn: Box::new(turn.snapshot.clone()),
        });
        conversation.emit_turn(&self.0.events, key, AskEventKind::RouteResolved { route });
        if let Some(event) = accepted {
            conversation.emit_turn(&self.0.events, key, event);
        }
        Ok(())
    }

    fn claim_effect(&self, key: AskTurnKey, effect: AskEffect) -> bool {
        let mut state = self.0.state.lock();
        let Some(turn) = state
            .conversation_mut(key.conversation_id)
            .ok()
            .and_then(|conversation| conversation.active_turn(key.turn_id))
        else {
            return false;
        };
        let pending = match effect {
            AskEffect::Write => turn.delivery == Some(AskDeliveryStatus::Pending),
            AskEffect::Copy => turn.copy_status == AskCopyStatus::Pending,
        };
        if !pending || turn.cancellation.is_cancelled() || turn.effect_claimed {
            return false;
        }
        turn.effect_claimed = true;
        true
    }

    async fn deliver(
        &self,
        key: AskTurnKey,
        route: AskRoute,
        body: String,
        preparation: AskPreparation,
    ) -> Result<(), BackendError> {
        let operation = match (route, &preparation.selection) {
            (AskRoute::Write, ExternalSelection::None) => Some(AskWriteOperation::Insert),
            (AskRoute::Edit, selection) if selection.verified().is_some() => {
                Some(AskWriteOperation::Replace)
            }
            _ => None,
        };
        let mut active = false;
        self.update(key, |turn| {
            active = true;
            turn.snapshot.activity = None;
            turn.delivery = Some(AskDeliveryStatus::Pending);
            turn.pending_body = Some(body.clone());
            Some(turn.delivery_event())
        });
        if !active {
            return Err(cancelled());
        }
        let outcome = if let (Some(operation), Some(writer)) = (operation, preparation.writer) {
            let service = Arc::downgrade(&self.0);
            let claim: AskWriteClaim = Box::new(move || {
                service
                    .upgrade()
                    .map(StandaloneQaService)
                    .is_some_and(|service| service.claim_effect(key, AskEffect::Write))
            });
            writer.write(operation, body.clone(), claim).await
        } else {
            AskWriteOutcome::NotWritten
        };

        let mut try_copy = false;
        let mut observed = false;
        self.update(key, |turn| {
            observed = true;
            turn.delivery = Some(match outcome {
                AskWriteOutcome::Written if turn.effect_claimed => AskDeliveryStatus::Written,
                AskWriteOutcome::Unknown | AskWriteOutcome::Written => AskDeliveryStatus::Unknown,
                AskWriteOutcome::NotWritten | AskWriteOutcome::Declined => {
                    AskDeliveryStatus::Retained
                }
            });
            let retained = turn.delivery != Some(AskDeliveryStatus::Written);
            let pending = turn.pending_body.take();
            if retained {
                turn.result_text = pending;
                if !turn.cancellation.is_cancelled() {
                    turn.copy_status = if preparation.clipboard.is_some() {
                        try_copy = true;
                        AskCopyStatus::Pending
                    } else {
                        AskCopyStatus::Failed
                    };
                }
            }
            turn.effect_claimed = false;
            Some(turn.delivery_event())
        });
        if !observed {
            return Err(cancelled());
        }
        if let Some(clipboard) = preparation.clipboard.filter(|_| try_copy) {
            if self.claim_effect(key, AskEffect::Copy) {
                let copied = clipboard.copy(body).await.is_ok();
                self.update(key, |turn| {
                    turn.copy_status = if copied {
                        AskCopyStatus::Copied
                    } else {
                        AskCopyStatus::Failed
                    };
                    Some(turn.delivery_event())
                });
            }
        }
        Ok(())
    }

    async fn run(
        &self,
        key: AskTurnKey,
        model: Arc<dyn AskModel>,
        mut material: prompt::Material,
        preparation: &AskPreparation,
        answer_only: bool,
        cancellation: AskCancellation,
    ) -> Result<Option<(AskRoute, String)>, BackendError> {
        let question = material.question().to_owned();
        let Resolved {
            route,
            missing_selection,
            continuation,
        } = if answer_only {
            Resolved {
                route: AskRoute::Answer,
                missing_selection: false,
                continuation: None,
            }
        } else {
            self.resolve_intent(key, model.as_ref(), &question, preparation, &cancellation)
                .await?
        };
        self.publish_route(key, route)?;
        if route == AskRoute::Clarify {
            let text = if let Some(continuation) = continuation {
                let request = AskModelRequest {
                    instructions: format!("{}\n{CLARIFY_RULE}", routing_instructions(true)),
                    messages: Vec::new(),
                    output_tokens: model.capabilities().output_tokens.min(256),
                    deep_thinking: false,
                    tools: AskModelTools {
                        intent: Some(continuation),
                        ..Default::default()
                    },
                };
                prompt::check_tool_budget(&request, model.capabilities())?;
                self.call(
                    key,
                    model.as_ref(),
                    request,
                    AskCallPurpose::Compose,
                    cancellation,
                )
                .await?
                .require_text()?
            } else {
                clarification(&question, missing_selection, &preparation.selection)
            };
            self.update(key, |turn| {
                turn.result_text = Some(text);
                Some(turn.delivery_event())
            });
            return Ok(None);
        }
        let related_selection = matches!(route, AskRoute::SelectionAnswer | AskRoute::Edit)
            .then(|| preparation.selection.verified())
            .flatten();
        if let Some(selection) = related_selection {
            material.add_selection(selection);
        }
        let context = self
            .0
            .deps
            .context
            .context(key, cancellation.clone())
            .await?;
        let search_enabled = !question.trim().is_empty()
            && model.supports_tools()
            && self.0.deps.search.enabled().await;
        let private = related_selection.is_some()
            || material.has_attachments()
            || context.facts.iter().any(|fact| fact.name != "local_time");
        let (history, summary, private) = {
            let mut state = self.0.state.lock();
            let conversation = state.conversation_mut(key.conversation_id)?;
            if conversation
                .active
                .is_none_or(|index| conversation.turns[index].snapshot.turn_id != key.turn_id)
            {
                return Err(cancelled());
            }
            let history = conversation
                .turns
                .iter()
                .filter(|turn| {
                    route.is_chat()
                        && turn.is_chat()
                        && turn.snapshot.status == AskTurnStatus::Completed
                })
                .map(|turn| prompt::CompletedTurn {
                    user: turn.user.text.clone(),
                    assistant: turn.assistant.text.clone(),
                })
                .collect::<Vec<_>>();
            if route.is_chat() {
                conversation.contains_private_context |= private;
            }
            let search_isolated = private
                || (route.is_chat()
                    && (conversation.contains_private_context || !history.is_empty()));
            (
                history,
                if route.is_chat() {
                    conversation.summary.clone()
                } else {
                    None
                },
                search_isolated,
            )
        };
        let tools = AskToolSet {
            search: search_enabled,
            calculator: model.supports_tools(),
            ..Default::default()
        };
        let plan = prompt::Plan::new(material, context, tools, history, summary)
            .for_route(route, continuation);
        let (mut request, replacement) = plan
            .prepare(self, key, model.as_ref(), &cancellation)
            .await?;
        if let Some(replacement) = replacement {
            let mut state = self.0.state.lock();
            let conversation = state.conversation_mut(key.conversation_id)?;
            if conversation.active_turn(key.turn_id).is_none() {
                return Err(cancelled());
            }
            conversation.summary = Some(replacement);
        }
        self.activity(key, AskActivity::Generating);
        let mut search = search_enabled && !private;
        if search_enabled && private {
            let decision = AskModelRequest {
                instructions: format!(
                    "{WEB_SEARCH_GUIDANCE}\nIf no useful public search applies, reply no_search. Do not answer the question in this pass."
                ),
                messages: vec![AskModelMessage {
                    role: AskModelRole::User,
                    parts: vec![AskModelPart::Text(question)],
                }],
                output_tokens: request.output_tokens.min(512),
                deep_thinking: false,
                tools: AskModelTools {
                    available: AskToolSet { search: true, ..Default::default() },
                    ..Default::default()
                },
            };
            match self
                .call(
                    key,
                    model.as_ref(),
                    decision,
                    AskCallPurpose::SearchDecision,
                    cancellation.clone(),
                )
                .await
            {
                Ok(CallResult::Tool(call)) => {
                    let output = self.run_tool(key, &call, &cancellation).await?;
                    if let Some(current) = request.messages.last_mut() {
                        current.parts.push(AskModelPart::Text(
                            serde_json::json!({"publicSearchEvidence": output}).to_string(),
                        ));
                    }
                }
                Err(error) if error.code == BackendErrorCode::Cancelled => return Err(error),
                _ => {}
            }
        }

        let mut calculations = if tools.calculator {
            MAX_CALCULATIONS
        } else {
            0
        };
        let base_instructions = request.instructions.clone();
        let purpose = if route.is_chat() {
            AskCallPurpose::Answer
        } else {
            AskCallPurpose::Compose
        };
        loop {
            request.tools.available = AskToolSet {
                search,
                calculator: calculations > 0,
                ..Default::default()
            };
            request.instructions.clone_from(&base_instructions);
            if search {
                request.instructions.push('\n');
                request.instructions.push_str(WEB_SEARCH_GUIDANCE);
            }
            if calculations > 0 {
                request.instructions.push('\n');
                request.instructions.push_str(CALCULATOR_GUIDANCE);
            }
            prompt::check_tool_budget(&request, model.capabilities())?;
            self.activity(key, AskActivity::Generating);
            match self
                .call(
                    key,
                    model.as_ref(),
                    request.clone(),
                    purpose,
                    cancellation.clone(),
                )
                .await
            {
                Ok(CallResult::Text(text)) => {
                    return Ok((!route.is_chat()).then_some((route, text)));
                }
                Ok(CallResult::Tool(call)) => {
                    match &call.tool {
                        AskToolCall::WebSearch { .. } => search = false,
                        AskToolCall::Calculate { .. } => calculations -= 1,
                        AskToolCall::ResolveIntent { .. } => return Err(invalid_intent()),
                    }
                    let output = self.run_tool(key, &call, &cancellation).await?;
                    request.tools.results.push(AskToolResult { call, output });
                }
                Err(error)
                    if error.message == "askModelRejectedTools"
                        && request.tools.available.any()
                        && request.tools.intent.is_none() =>
                {
                    search = false;
                    calculations = 0;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_tool(
        &self,
        key: AskTurnKey,
        call: &AskModelToolCall,
        cancellation: &AskCancellation,
    ) -> Result<String, BackendError> {
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        match &call.tool {
            AskToolCall::WebSearch { public_query } => {
                self.search_web(key, public_query.clone(), cancellation.clone())
                    .await
            }
            AskToolCall::Calculate { expression } => Ok(calculator::calculate(expression)),
            AskToolCall::ResolveIntent { .. } => Err(invalid_intent()),
        }
    }

    async fn search_web(
        &self,
        key: AskTurnKey,
        public_query: String,
        cancellation: AskCancellation,
    ) -> Result<String, BackendError> {
        let tool_call_id = Uuid::new_v4();
        self.update(key, |turn| {
            turn.search_open = true;
            turn.snapshot.tool_call_id = Some(tool_call_id);
            Some(AskEventKind::Search {
                tool_call_id,
                status: AskSearchStatus::Idle,
                error: None,
            })
        });
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        let service = Arc::downgrade(&self.0);
        let progress: AskToolSink = Arc::new(move |progress| {
            let Some(service) = service.upgrade().map(StandaloneQaService) else {
                return;
            };
            service.update(key, |turn| {
                if !turn.search_open || turn.snapshot.tool_call_id != Some(tool_call_id) {
                    return None;
                }
                match progress {
                    AskToolProgress::Status(status) if status.active() => {
                        if turn.snapshot.search_status == status
                            || (turn.snapshot.search_status == AskSearchStatus::Reading
                                && status == AskSearchStatus::Searching)
                        {
                            return None;
                        }
                        turn.search_event(tool_call_id, status, None)
                    }
                    AskToolProgress::Source(mut source) => {
                        source.url = public_url(&source.url)?;
                        if let Some(existing) = turn
                            .snapshot
                            .sources
                            .iter_mut()
                            .find(|existing| existing.source_id == source.source_id)
                        {
                            *existing = source.clone();
                        } else {
                            turn.snapshot.sources.push(source.clone());
                        }
                        Some(AskEventKind::Source {
                            tool_call_id,
                            source,
                        })
                    }
                    _ => None,
                }
            });
        });
        let result = self
            .0
            .deps
            .search
            .search(
                AskSearchRequest {
                    key: AskToolKey {
                        conversation_id: key.conversation_id,
                        turn_id: key.turn_id,
                        tool_call_id,
                    },
                    public_query,
                    evidence_token_budget: prompt::SEARCH_BUDGET,
                },
                cancellation,
                progress,
            )
            .await;
        let (status, evidence, error) = match result {
            Ok(AskSearchResult::Disabled) => (AskSearchStatus::Idle, String::new(), None),
            Ok(AskSearchResult::Done { evidence }) => (AskSearchStatus::Done, evidence, None),
            Ok(AskSearchResult::NoResults) => (AskSearchStatus::NoResults, String::new(), None),
            Err(error) if error.code == BackendErrorCode::Cancelled => return Err(error),
            Err(error) => (AskSearchStatus::Error, String::new(), Some(error)),
        };
        let mut state = self.0.state.lock();
        if let Ok(conversation) = state.conversation_mut(key.conversation_id) {
            if let Some(turn) = conversation.active_turn(key.turn_id) {
                for event in turn.finish_search(status.clone(), error) {
                    conversation.emit_turn(&self.0.events, key, event);
                }
            }
        }
        if !evidence.is_empty() {
            Ok(evidence)
        } else {
            Ok(serde_json::json!({"status":status, "message":"No verified search evidence was returned."}).to_string())
        }
    }

    async fn call(
        &self,
        key: AskTurnKey,
        model: &dyn AskModel,
        request: AskModelRequest,
        purpose: AskCallPurpose,
        cancellation: AskCancellation,
    ) -> Result<CallResult, BackendError> {
        let available = request.tools.available;
        let call_id = Uuid::new_v4();
        let label = model.label().clone();
        let text = Arc::new(Mutex::new(Some(Ok::<_, BackendError>(String::new()))));
        let service = Arc::downgrade(&self.0);
        let accumulated = text.clone();
        let output_limit = AskCancellation::default();
        let limit_reached = output_limit.clone();
        let limit = request.output_tokens.saturating_mul(16);
        let mut archive = self.0.deps.usage.as_ref().map(|usage| CallArchive {
            usage,
            generation: usage.recorder.generation(),
            occurred_at_ms: usage.clock.now_utc().timestamp_millis(),
            call_id,
            model: label.clone(),
            purpose,
            cancellation: cancellation.clone(),
            observed: Arc::new(Mutex::new(Some(ReportedTokens::default()))),
            outcome: None,
        });
        let observed = archive.as_ref().map(|archive| archive.observed.clone());
        let progress: AskModelSink = Arc::new(move |progress| {
            let Some(service) = service.upgrade().map(StandaloneQaService) else {
                return;
            };
            let mut accumulator = accumulated.lock();
            let Some(Ok(output)) = accumulator.as_mut() else {
                return;
            };
            match progress {
                AskModelProgress::Text(delta) if !delta.is_empty() => {
                    if output.len().saturating_add(delta.len()) > limit {
                        *accumulator = Some(Err(if purpose == AskCallPurpose::Intent {
                            invalid_intent()
                        } else {
                            ask_error(BackendErrorCode::Provider, "askOutputTooLarge")
                        }));
                        limit_reached.cancel();
                        return;
                    }
                    output.push_str(&delta);
                    if purpose == AskCallPurpose::Answer {
                        service.activity(key, AskActivity::Generating);
                        service.update(key, |turn| {
                            turn.assistant.text.push_str(&delta);
                            Some(AskEventKind::AnswerDelta {
                                message_id: turn.assistant.message_id,
                                delta,
                            })
                        });
                    }
                }
                AskModelProgress::Thinking
                    if matches!(purpose, AskCallPurpose::Answer | AskCallPurpose::Compose)
                        && output.is_empty() =>
                {
                    service.activity(key, AskActivity::Thinking);
                }
                AskModelProgress::Usage(tokens) if tokens != ReportedTokens::default() => {
                    if let Some(observed) = &observed {
                        if let Some(latest) = observed.lock().as_mut() {
                            *latest = tokens;
                        }
                    }
                    service.update(key, |turn| {
                        let usage = AskCallUsage {
                            call_id,
                            purpose,
                            model: label.clone(),
                            input: None,
                            output: None,
                            cached_input: None,
                            cache_write_input: None,
                            reasoning: None,
                            input_accounting: None,
                        };
                        let saved = match turn
                            .snapshot
                            .usage
                            .iter()
                            .position(|saved| saved.call_id == call_id)
                        {
                            Some(index) => &mut turn.snapshot.usage[index],
                            None => {
                                turn.snapshot.usage.push(usage);
                                turn.snapshot
                                    .usage
                                    .last_mut()
                                    .expect("usage was just inserted")
                            }
                        };
                        saved.replace_tokens(tokens);
                        Some(AskEventKind::Usage {
                            usage: saved.clone(),
                        })
                    });
                }
                _ => {}
            }
        });
        let completion = tokio::select! {
            biased;
            _ = output_limit.cancelled() => Ok(AskModelCompletion::Complete),
            completion = model.stream(request, cancellation.clone(), progress) => completion,
        };
        let output = text
            .lock()
            .take()
            .expect("model output accumulator is closed once");
        let result = completion.and_then(|completion| {
            if cancellation.is_cancelled() {
                return Err(cancelled());
            }
            let output = output?;
            match completion {
                AskModelCompletion::ToolCall(_)
                    if available.resolve_intent && !output.trim().is_empty() =>
                {
                    Err(invalid_intent())
                }
                AskModelCompletion::ToolCall(call) if available.allows(&call.tool) => {
                    Ok(CallResult::Tool(call))
                }
                AskModelCompletion::ToolCall(_) => Err(ask_error(
                    BackendErrorCode::Provider,
                    "askUnexpectedToolCall",
                )),
                AskModelCompletion::Complete if output.trim().is_empty() => {
                    Err(ask_error(BackendErrorCode::Provider, "askEmptyModelOutput"))
                }
                AskModelCompletion::Complete => Ok(CallResult::Text(output)),
            }
        });
        if let Some(archive) = &mut archive {
            archive.outcome = Some(match &result {
                Ok(_) => ServiceOutcome::Succeeded,
                Err(error) if error.code == BackendErrorCode::Cancelled => {
                    ServiceOutcome::Cancelled
                }
                Err(_) => ServiceOutcome::Failed,
            });
        }
        result
    }
}

struct CallArchive<'a> {
    usage: &'a AskUsagePersistence,
    generation: u64,
    occurred_at_ms: i64,
    call_id: Uuid,
    model: AskModelLabel,
    purpose: AskCallPurpose,
    cancellation: AskCancellation,
    observed: Arc<Mutex<Option<ReportedTokens>>>,
    outcome: Option<ServiceOutcome>,
}

impl Drop for CallArchive<'_> {
    fn drop(&mut self) {
        let Some(tokens) = self.observed.lock().take() else {
            return;
        };
        if tokens == ReportedTokens::default() {
            return;
        }
        let outcome = self.outcome.unwrap_or_else(|| {
            if self.cancellation.is_cancelled() {
                ServiceOutcome::Cancelled
            } else {
                ServiceOutcome::Unknown
            }
        });
        self.usage.recorder.submit(
            self.generation,
            UsageRecord {
                id: self.call_id,
                occurred_at_ms: self.occurred_at_ms,
                kind: ServiceKind::Llm,
                purpose: match self.purpose {
                    AskCallPurpose::Answer | AskCallPurpose::Compose => ServicePurpose::Question,
                    AskCallPurpose::Compact
                    | AskCallPurpose::SearchDecision
                    | AskCallPurpose::Intent => ServicePurpose::Other,
                },
                outcome,
                provider: None,
                service: sanitized_identifier(&self.model.service),
                protocol: sanitized_identifier(&self.model.protocol),
                input_accounting: tokens.input_accounting,
                channel: sanitized_identifier(&self.model.channel),
                model: sanitized_identifier(&self.model.model),
                audio_ms: None,
                input_tokens: tokens.input,
                output_tokens: tokens.output,
                cached_input_tokens: tokens.cached_input,
                cache_write_input_tokens: tokens.cache_write_input,
                reasoning_tokens: tokens.reasoning,
                synthesized_characters: None,
            },
        );
    }
}

struct RunningTurn {
    service: StandaloneQaService,
    key: AskTurnKey,
}

impl Drop for RunningTurn {
    fn drop(&mut self) {
        self.service.finish(
            self.key,
            Err(ask_error(BackendErrorCode::Internal, "askTaskInterrupted")),
        );
    }
}

impl State {
    fn new_conversation(&mut self) -> Uuid {
        let id = Uuid::new_v4();
        self.conversations.insert(id, Conversation::default());
        self.current = Some(id);
        id
    }

    fn conversation(&self, id: Uuid) -> Result<&Conversation, BackendError> {
        self.conversations.get(&id).ok_or_else(no_conversation)
    }

    fn conversation_mut(&mut self, id: Uuid) -> Result<&mut Conversation, BackendError> {
        self.conversations.get_mut(&id).ok_or_else(no_conversation)
    }
}

impl Conversation {
    fn emit_turn(
        &mut self,
        sender: &broadcast::Sender<Arc<AskEvent>>,
        key: AskTurnKey,
        kind: AskEventKind,
    ) {
        self.emit(sender, key.conversation_id, Some(key.turn_id), kind);
    }

    fn emit(
        &mut self,
        sender: &broadcast::Sender<Arc<AskEvent>>,
        conversation_id: Uuid,
        turn_id: Option<Uuid>,
        kind: AskEventKind,
    ) {
        self.sequence += 1;
        self.event_bytes += kind.bytes();
        let event = Arc::new(AskEvent {
            conversation_id,
            sequence: self.sequence,
            turn_id,
            kind,
        });
        self.events.push_back(event.clone());
        while self.events.len() > EVENT_LIMIT || self.event_bytes > EVENT_BYTES {
            if let Some(oldest) = self.events.pop_front() {
                self.event_bytes -= oldest.kind.bytes();
            }
        }
        let _ = sender.send(event);
    }

    fn active_turn(&mut self, id: Uuid) -> Option<&mut Turn> {
        self.active
            .and_then(|index| self.turns.get_mut(index))
            .filter(|turn| turn.snapshot.turn_id == id)
    }

    fn finish(
        &mut self,
        sender: &broadcast::Sender<Arc<AskEvent>>,
        key: AskTurnKey,
        result: Result<(), BackendError>,
    ) -> Option<(String, String)> {
        let turn = self.active_turn(key.turn_id)?;
        let (status, error) = match result {
            Ok(()) => (AskTurnStatus::Completed, None),
            Err(error) if error.code == BackendErrorCode::Cancelled => {
                (AskTurnStatus::Cancelled, Some(error))
            }
            Err(error) => (AskTurnStatus::Failed, Some(error)),
        };
        if status != AskTurnStatus::Completed {
            turn.cancellation.cancel();
        }
        let search_status = if status == AskTurnStatus::Cancelled {
            AskSearchStatus::Cancelled
        } else {
            AskSearchStatus::Error
        };
        let mut events = turn.finish_search(search_status, error.clone());
        turn.snapshot.status = status;
        turn.snapshot.activity = None;
        turn.snapshot.error = error.clone();
        if turn.delivery == Some(AskDeliveryStatus::Pending) {
            if turn.effect_claimed {
                turn.delivery = Some(AskDeliveryStatus::Unknown);
                turn.result_text = turn.pending_body.take();
            } else {
                turn.delivery = None;
                turn.pending_body = None;
            }
            events.push(turn.delivery_event());
        }
        if turn.copy_status == AskCopyStatus::Pending {
            turn.copy_status = if turn.effect_claimed {
                AskCopyStatus::Failed
            } else {
                AskCopyStatus::NotNeeded
            };
            events.push(turn.delivery_event());
        }
        let history = (status == AskTurnStatus::Completed && turn.is_chat())
            .then(|| (turn.user.text.clone(), turn.assistant.text.clone()));
        self.active = None;
        for event in events {
            self.emit_turn(sender, key, event);
        }
        self.emit_turn(sender, key, AskEventKind::Terminal { status, error });
        history
    }

    fn cache(&self) -> Vec<AskCacheMetric> {
        let mut totals = HashMap::<AskModelLabel, (u128, u128, usize)>::new();
        for usage in self.turns.iter().flat_map(|turn| &turn.snapshot.usage) {
            let entry = totals.entry(usage.model.clone()).or_default();
            if let Some((read, input)) = usage.comparable() {
                entry.0 += u128::from(read);
                entry.1 += u128::from(input);
                entry.2 += 1;
            }
        }
        let mut metrics = totals
            .into_iter()
            .map(
                |(model, (read, input, comparable_reports))| AskCacheMetric {
                    model,
                    comparable_reports,
                    hit_rate: (input > 0).then(|| read as f64 / input as f64),
                },
            )
            .collect::<Vec<_>>();
        metrics.sort_by(|a, b| {
            (
                &a.model.service,
                &a.model.channel,
                &a.model.model,
                &a.model.protocol,
            )
                .cmp(&(
                    &b.model.service,
                    &b.model.channel,
                    &b.model.model,
                    &b.model.protocol,
                ))
        });
        metrics
    }
}

impl Turn {
    fn new(id: Uuid, request: AskSubmit, cancellation: AskCancellation) -> Self {
        let user = AskMessage {
            message_id: Uuid::new_v4(),
            turn_id: id,
            role: "user",
            text: request.text.clone(),
            attachment_ids: request.attachment_ids.clone(),
        };
        let assistant = AskMessage {
            message_id: Uuid::new_v4(),
            turn_id: id,
            role: "assistant",
            text: String::new(),
            attachment_ids: Vec::new(),
        };
        let snapshot = AskTurn {
            turn_id: id,
            user_message_id: user.message_id,
            assistant_message_id: assistant.message_id,
            deep_thinking: request.options.deep_thinking,
            attachments: Vec::new(),
            status: AskTurnStatus::Running,
            activity: Some(AskActivity::Preparing),
            search_status: AskSearchStatus::Idle,
            search_error: None,
            tool_call_id: None,
            sources: Vec::new(),
            error: None,
            usage: Vec::new(),
        };
        Self {
            request,
            user,
            assistant,
            snapshot,
            cancellation,
            search_open: false,
            route: None,
            result_text: None,
            delivery: None,
            copy_status: AskCopyStatus::NotNeeded,
            effect_claimed: false,
            pending_body: None,
        }
    }

    fn is_chat(&self) -> bool {
        self.route.is_some_and(AskRoute::is_chat)
    }

    fn submission(&self) -> AskSubmission {
        AskSubmission {
            current_turn_id: self.snapshot.turn_id,
            route: self.route,
            status: self.snapshot.status,
            activity: self.snapshot.activity,
            search_status: self.snapshot.search_status.clone(),
            search_error: self.snapshot.search_error.clone(),
            tool_call_id: self.snapshot.tool_call_id,
            sources: self.snapshot.sources.clone(),
            result_text: self.result_text.clone(),
            delivery: self.delivery,
            copy_status: self.copy_status,
            error: self.snapshot.error.clone(),
        }
    }

    fn delivery_event(&self) -> AskEventKind {
        AskEventKind::Delivery {
            submission: Box::new(self.submission()),
        }
    }

    fn activity(&mut self, activity: AskActivity) -> Option<AskEventKind> {
        if self.snapshot.activity == Some(activity) {
            return None;
        }
        self.snapshot.activity = Some(activity);
        Some(AskEventKind::Activity { activity })
    }

    fn search_event(
        &mut self,
        tool_call_id: Uuid,
        status: AskSearchStatus,
        error: Option<BackendError>,
    ) -> Option<AskEventKind> {
        if self.snapshot.search_status == status && self.snapshot.search_error == error {
            return None;
        }
        self.snapshot.search_status = status.clone();
        self.snapshot.search_error = error.clone();
        Some(AskEventKind::Search {
            tool_call_id,
            status,
            error,
        })
    }

    fn finish_search(
        &mut self,
        status: AskSearchStatus,
        error: Option<BackendError>,
    ) -> Vec<AskEventKind> {
        let mut events = Vec::new();
        if !self.search_open {
            return events;
        }
        self.search_open = false;
        if let Some(tool_call_id) = self.snapshot.tool_call_id {
            for source in &mut self.snapshot.sources {
                if source.read_status == AskReadStatus::Reading {
                    source.read_status = if status == AskSearchStatus::Cancelled {
                        AskReadStatus::Cancelled
                    } else {
                        AskReadStatus::Error
                    };
                    events.push(AskEventKind::Source {
                        tool_call_id,
                        source: source.clone(),
                    });
                }
            }
            events.extend(self.search_event(tool_call_id, status, error));
        }
        events
    }
}

fn validate_submit(request: &AskSubmit) -> Result<(), BackendError> {
    if request.client_request_id.trim().is_empty()
        || request.client_request_id.len() > MAX_REQUEST_ID_BYTES
        || (request.text.trim().is_empty() && request.attachment_ids.is_empty())
    {
        return Err(ask_error(
            BackendErrorCode::InvalidArgument,
            "askInvalidRequest",
        ));
    }
    if request.attachment_ids.len() > MAX_ATTACHMENTS
        || request
            .attachment_ids
            .iter()
            .enumerate()
            .any(|(index, attachment)| request.attachment_ids[..index].contains(attachment))
    {
        return Err(ask_error(
            BackendErrorCode::InvalidArgument,
            "askInvalidAttachments",
        ));
    }
    Ok(())
}

fn public_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

fn ask_error(code: BackendErrorCode, message: &'static str) -> BackendError {
    BackendError::new(code, message)
}

fn cancelled() -> BackendError {
    ask_error(BackendErrorCode::Cancelled, "askCancelled")
}

fn no_conversation() -> BackendError {
    ask_error(BackendErrorCode::InvalidState, "askConversationNotFound")
}

fn no_turn() -> BackendError {
    ask_error(BackendErrorCode::InvalidState, "askTurnNotFound")
}
