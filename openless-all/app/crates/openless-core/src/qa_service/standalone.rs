use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, watch};
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
const WEB_SEARCH_GUIDANCE: &str = "用户明确要求联网查询，或问题依赖时效性公开事实时，先调用 web_search。\
    查询保留本轮公开主题、日期和限定条件，不夹带私人内容。根据工具实际返回的证据作答，不虚构来源。";
const WEB_SEARCH_EVIDENCE: &str = "本轮搜索已由应用处理。以下是搜索状态和参考资料；仅据实际可核实的内容作答。\
    区分已读正文与未核实摘要；抓取时间不是发布日期。无法读取、无关或检索失败时说明限制，不把链接数当作证据。";
const NO_WEB_EVIDENCE: &str = "本轮没有联网证据；不声称已搜索，不虚构时效性事实。";
#[cfg(not(test))]
const DELIVERY_REVIEW_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
#[cfg(test)]
const DELIVERY_REVIEW_WAIT: std::time::Duration = std::time::Duration::from_millis(250);
const CALCULATOR_GUIDANCE: &str = "Use calculator for precise arithmetic, including large numbers, fractions, percentages (write them as /100), and integer powers. Call one tool at a time. Use the exact result where useful and label rounded decimals as approximate. If the tool reports an error, correct the expression or explain the limitation; do not invent a result.";
const MAX_CALCULATIONS: usize = 3;
const MAX_REQUEST_ID_BYTES: usize = 128;
const EVENT_LIMIT: usize = 512;
const EVENT_BYTES: usize = 2 * 1024 * 1024;

fn includes_any(text: &str, cues: &[&str]) -> bool {
    cues.iter().any(|cue| text.contains(cue))
}

fn explicit_app_reference(instruction: &str) -> bool {
    includes_any(&instruction.to_lowercase(), &[
        "this app", "current app", "this page", "current page", "当前应用", "这个应用",
        "當前應用", "这个页面", "這個頁面", "当前网页", "这里的页面",
    ])
}

fn calculation_requested(instruction: &str) -> bool {
    let text = instruction.to_lowercase();
    includes_any(&text, &["calculate", "compute", "算一下", "计算", "算出", "多少钱"])
        || (text.chars().any(|character| character.is_ascii_digit())
            && text.contains(['+', '×', '÷', '=', '/', '*']))
}

fn search_payload(status: &AskSearchStatus, evidence: &str) -> String {
    let evidence = serde_json::from_str::<serde_json::Value>(evidence)
        .unwrap_or_else(|_| serde_json::json!(evidence));
    serde_json::json!({"status":status, "evidence":evidence}).to_string()
}

#[derive(Clone)]
pub struct AskCancellation(
    watch::Sender<bool>,
    Option<watch::Sender<bool>>,
    Option<Arc<dyn Fn() -> bool + Send + Sync>>,
);

impl Default for AskCancellation {
    fn default() -> Self {
        Self(watch::channel(false).0, None, None)
    }
}

impl AskCancellation {
    fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow() || self.1.as_ref().is_some_and(|scope| *scope.borrow())
            || self.2.as_ref().is_some_and(|permitted| !permitted())
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() { return; }
        let mut receiver = self.0.subscribe();
        if let Some(scope) = &self.1 {
            let mut scope = scope.subscribe();
            tokio::select! {
                _ = receiver.wait_for(|cancelled| *cancelled) => {}
                _ = scope.wait_for(|revoked| *revoked) => {}
            }
        } else {
            let _ = receiver.wait_for(|cancelled| *cancelled).await;
        }
    }

    pub fn requiring(mut self, permitted: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.2 = Some(Arc::new(permitted));
        self
    }
}

#[derive(Default)]
pub struct AskSessionFence(Mutex<AskCancellation>);

impl AskSessionFence {
    pub fn issue(&self) -> AskCancellation {
        AskCancellation(watch::channel(false).0, Some(self.0.lock().0.clone()), None)
    }

    pub fn revoke(&self) {
        std::mem::take(&mut *self.0.lock()).cancel();
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

    fn consume(&self, _conversation_id: Uuid, _attachment_ids: &[Uuid]) {}

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
    Disabled,
    Unsupported,
    Rejected,
    SnippetOnly,
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
    NotRelevant,
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
    SnippetOnly,
}

pub type AskToolSink = Arc<dyn Fn(AskToolProgress) + Send + Sync>;

pub trait SearchTool: Send + Sync {
    fn enabled(&self) -> BoxFuture<'_, Result<bool, BackendError>> {
        Box::pin(async { Ok(true) })
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
    Polishing,
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
    DeliveryCheck,
}

enum SearchDecision {
    Query(AskModelToolCall),
    NotNeeded,
    MissingTopic,
    Unavailable(AskSearchStatus),
}

enum DeliveryReview {
    Approved,
    Rejected,
    Unavailable,
}

struct ReviewedPrefix {
    text: String,
    verdict: DeliveryReview,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum SearchDecisionReply {
    NotNeeded,
    MissingPublicTopic,
}

fn native_write_operation(route: AskRoute, preparation: &AskPreparation) -> Option<AskWriteOperation> {
    match (route, &preparation.selection) {
        (AskRoute::Write, ExternalSelection::None) => Some(AskWriteOperation::Insert),
        (AskRoute::Write, _) if preparation.writer.as_ref().is_some_and(|writer| writer.has_original_caret()) => {
            Some(AskWriteOperation::Insert)
        }
        (AskRoute::Edit, selection) if selection.verified().is_some() => Some(AskWriteOperation::Replace),
        _ => None,
    }
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
    transient: bool,
    chat_locked: bool,
    public_search_topic: Option<String>,
    admission: Arc<tokio::sync::Mutex<()>>,
    lifetime: AskCancellation,
    sequence: u64,
    event_bytes: usize,
    events: VecDeque<Arc<AskEvent>>,
    turns: Vec<Turn>,
    requests: HashMap<String, usize>,
    active: Option<usize>,
    summary: Option<prompt::Summary>,
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

    pub fn open_transient(&self) -> Uuid {
        self.end_all();
        let mut state = self.0.state.lock();
        let id = state.new_conversation();
        state.conversations.get_mut(&id).unwrap().transient = true;
        id
    }

    pub fn current_is_transient(&self) -> bool {
        let state = self.0.state.lock();
        state.current.and_then(|id| state.conversations.get(&id))
            .is_some_and(|conversation| conversation.transient)
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
            AskCancellation::default(),
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
        self.submit_inner(request, prepare, || true, false, AskCancellation::default()).await
    }

    pub async fn submit_prepared_guarded<F, Fut>(
        &self, request: AskSubmit, prepare: F, admit: impl FnOnce() -> bool + Send,
        cancellation: AskCancellation,
    ) -> Result<Uuid, BackendError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<AskPreparation, BackendError>> + Send,
    {
        self.submit_inner(request, prepare, admit, false, cancellation).await
    }

    async fn submit_inner<F, Fut>(
        &self,
        request: AskSubmit,
        prepare: F,
        admit: impl FnOnce() -> bool + Send,
        answer_only: bool,
        cancellation: AskCancellation,
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
            _ = cancellation.cancelled() => return Err(cancelled()),
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
        let answer_only = answer_only || self.0.state.lock()
            .conversation(request.conversation_id)?.chat_locked;
        let key = AskTurnKey {
            conversation_id: request.conversation_id,
            turn_id: Uuid::new_v4(),
        };
        let preparation = if answer_only {
            AskPreparation::default()
        } else { tokio::select! {
            biased;
            _ = lifetime.cancelled() => return Err(no_conversation()),
            _ = cancellation.cancelled() => return Err(cancelled()),
            preparation = async { prepare().await } => preparation?,
        }};
        let model = tokio::select! {
            biased;
            _ = lifetime.cancelled() => return Err(no_conversation()),
            _ = cancellation.cancelled() => return Err(cancelled()),
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
        {
            let mut state = self.0.state.lock();
            let conversation = state.conversation_mut(key.conversation_id)?;
            if cancellation.is_cancelled() || !admit() {
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
        self.update_scoped(key, false, update);
    }

    fn update_scoped(
        &self, key: AskTurnKey, accept_claimed_effect: bool,
        update: impl FnOnce(&mut Turn) -> Option<AskEventKind>,
    ) {
        let mut state = self.0.state.lock();
        if let Ok(conversation) = state.conversation_mut(key.conversation_id) {
            if let Some(event) = conversation.active_turn(key.turn_id)
                .filter(|turn| !turn.cancellation.is_cancelled() || (accept_claimed_effect && turn.effect_claimed))
                .and_then(update) {
                conversation.emit_turn(&self.0.events, key, event);
            }
        }
    }

    fn activity(&self, key: AskTurnKey, activity: AskActivity) {
        self.update(key, |turn| turn.activity(activity));
    }

    fn publish_answer_text(&self, key: AskTurnKey, text: &str) {
        self.update(key, |turn| {
            if text.is_empty() { return None; }
            turn.assistant.text.push_str(text);
            Some(AskEventKind::AnswerDelta { message_id: turn.assistant.message_id, delta: text.into() })
        });
    }

    fn stage_candidate(&self, key: AskTurnKey, route: AskRoute, body: &str) -> Result<(), BackendError> {
        let mut staged = false;
        self.update(key, |turn| {
            staged = true;
            turn.pending_body = Some(body.trim().to_owned());
            if route.is_chat() {
                return None;
            }
            turn.snapshot.activity = None;
            turn.delivery = Some(AskDeliveryStatus::Pending);
            Some(turn.delivery_event())
        });
        if staged { Ok(()) } else { Err(cancelled()) }
    }

    fn clear_candidate(&self, key: AskTurnKey) {
        self.update(key, |turn| {
            turn.pending_body = None;
            turn.delivery.take().map(|_| turn.delivery_event())
        });
    }

    fn finish(&self, key: AskTurnKey, result: Result<(), BackendError>) {
        let completed = {
            let mut state = self.0.state.lock();
            let Ok(conversation) = state.conversation_mut(key.conversation_id) else { return };
            let permission = conversation.active_turn(key.turn_id).map(|turn| turn.cancellation.clone());
            let consumed = conversation.active_turn(key.turn_id)
                .filter(|turn| result.is_ok() && !turn.cancellation.is_cancelled()
                    && (turn.is_chat() || turn.delivery == Some(AskDeliveryStatus::Written)))
                .map(|turn| turn.request.attachment_ids.clone())
                .unwrap_or_default();
            if !consumed.is_empty() {
                self.0.deps.attachments.consume(key.conversation_id, &consumed);
                conversation.attachments.retain(|attachment| !consumed.contains(&attachment.attachment_id));
                for attachment_id in consumed {
                    conversation.emit_turn(&self.0.events, key, AskEventKind::AttachmentRemoved { attachment_id });
                }
            }
            let cancelled_turn = conversation.active_turn(key.turn_id)
                .is_some_and(|turn| turn.cancellation.is_cancelled() && !turn.effect_claimed
                    && !matches!(turn.delivery, Some(AskDeliveryStatus::Written | AskDeliveryStatus::Unknown)));
            let completed = conversation.finish(&self.0.events, key, if cancelled_turn { Err(cancelled()) } else { result });
            if conversation.transient { None } else { completed.zip(permission) }
        };
        if let (Some(history), Some(((question, answer), permission))) = (&self.0.deps.history, completed) {
            if !permission.is_cancelled() {
                history(&question, &answer);
            }
        }
    }

    fn publish_route(&self, key: AskTurnKey, route: AskRoute) -> Result<(), BackendError> {
        let mut state = self.0.state.lock();
        let conversation = state.conversation_mut(key.conversation_id)?;
        let turn = conversation
            .active_turn(key.turn_id)
            .filter(|turn| !turn.cancellation.is_cancelled())
            .ok_or_else(cancelled)?;
        turn.route = Some(route);
        let accepted = route.is_chat().then(|| AskEventKind::Accepted {
            user: turn.user.clone(),
            assistant: turn.assistant.clone(),
            turn: Box::new(turn.snapshot.clone()),
        });
        conversation.chat_locked |= route.is_chat();
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
        let operation = native_write_operation(route, &preparation);
        let mut ready = false;
        self.update(key, |turn| {
            ready = turn.delivery == Some(AskDeliveryStatus::Pending);
            None
        });
        if !ready { return Err(cancelled()); }
        let outcome = if let (Some(operation), Some(writer)) = (operation, preparation.writer) {
            let service = Arc::downgrade(&self.0);
            let current_service = service.clone();
            let still_current = Arc::new(move || {
                current_service.upgrade().is_some_and(|service| {
                    service.state.lock().conversation_mut(key.conversation_id).ok()
                        .and_then(|conversation| conversation.active_turn(key.turn_id))
                        .is_some_and(|turn| !turn.cancellation.is_cancelled() && !turn.effect_claimed)
                })
            });
            let claim = Box::new(move || {
                service
                    .upgrade()
                    .map(StandaloneQaService)
                    .is_some_and(|service| service.claim_effect(key, AskEffect::Write))
            });
            writer.write(operation, body.clone(), AskWriteClaim { still_current, claim }).await
        } else {
            AskWriteOutcome::NotWritten
        };

        let mut try_copy = false;
        let mut observed = false;
        self.update_scoped(key, true, |turn| {
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
                        AskCopyStatus::NotNeeded
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
            mut route,
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
        if missing_selection {
            self.publish_route(key, AskRoute::Clarify)?;
            return Err(ask_error(BackendErrorCode::InvalidState,
                if matches!(preparation.selection, ExternalSelection::Unavailable) {
                    "askSelectionUnavailable"
                } else {
                    "askSelectionRequired"
                },
            ));
        }
        if route == AskRoute::Clarify { route = AskRoute::Answer; }
        self.publish_route(key, route)?;
        let related_selection = matches!(route, AskRoute::SelectionAnswer | AskRoute::Edit)
            .then(|| preparation.selection.verified())
            .flatten();
        if let Some(selection) = related_selection {
            material.add_selection(selection);
        }
        let mut context = self.0.deps.context.context(key, cancellation.clone()).await?;
        let refers_to_current_app = explicit_app_reference(&question);
        let local_time = context.facts.iter().find(|fact| fact.name == "local_time");
        let prior_topic = route.is_chat().then(|| self.public_search_topic(key)).flatten();
        let search_decision = if route == AskRoute::Edit || question.trim().is_empty() {
            SearchDecision::NotNeeded
        } else {
            match self.0.deps.search.enabled().await {
                Ok(false) => SearchDecision::Unavailable(AskSearchStatus::Disabled),
                Err(_) => SearchDecision::Unavailable(AskSearchStatus::Error),
                Ok(true) if !model.supports_tools() => SearchDecision::Unavailable(AskSearchStatus::Unsupported),
                Ok(true) => self.choose_public_query(
                    key, model.as_ref(), &question, prior_topic.as_deref(), local_time, &cancellation,
                ).await?,
            }
        };
        let search_planned = matches!(&search_decision, SearchDecision::Query(_));
        context.facts.retain(|fact| match fact.name.as_str() {
            "foreground_app" => route == AskRoute::Write || (route.is_chat() && refers_to_current_app),
            "local_time" => search_planned,
            _ => false,
        });
        context.missing.clear();
        let isolated_search = related_selection.is_some() || material.has_attachments()
            || prior_topic.is_some() || explicit_app_reference(&question);
        let (history, summary) = {
            let state = self.0.state.lock();
            let conversation = state.conversation(key.conversation_id)?;
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
            (
                history,
                if route.is_chat() {
                    conversation.summary.clone()
                } else {
                    None
                },
            )
        };
        let isolated_search = isolated_search || !history.is_empty() || continuation.is_some();
        let mut search_result = None;
        let search_status = match search_decision {
            SearchDecision::Query(call) => {
                let AskToolCall::WebSearch { public_query } = &call.tool else { return Err(invalid_intent()); };
                let (status, evidence) = self.search_web(key, public_query.clone(), cancellation.clone()).await?;
                if route.is_chat() {
                    self.set_public_search_topic(key, public_query.clone());
                }
                if status != AskSearchStatus::Done {
                    if route.is_chat() {
                        self.answer_search_unavailable(key, &question, status);
                        return Ok(None);
                    }
                    return Err(ask_error(BackendErrorCode::Provider, "askDraftSearchUnavailable"));
                }
                if isolated_search {
                    material.add_search_evidence(&status, &evidence);
                } else {
                    search_result = Some(AskToolResult { call, output: search_payload(&status, &evidence) });
                }
                AskSearchStatus::Done
            }
            SearchDecision::NotNeeded => AskSearchStatus::Idle,
            SearchDecision::MissingTopic if route.is_chat() => {
                self.answer_search_unavailable(key, &question, AskSearchStatus::Idle);
                return Ok(None);
            }
            SearchDecision::MissingTopic => {
                return Err(ask_error(BackendErrorCode::Provider, "askDraftSearchUnavailable"));
            }
            SearchDecision::Unavailable(status) => {
                material.add_search_evidence(&status, "");
                status
            }
        };
        let tools = AskToolSet {
            calculator: model.supports_tools() && route != AskRoute::Edit && calculation_requested(&question),
            ..Default::default()
        };
        let evidence_rule = if search_status == AskSearchStatus::Done {
            Some(WEB_SEARCH_EVIDENCE)
        } else if route != AskRoute::Edit {
            Some(NO_WEB_EVIDENCE)
        } else {
            None
        };
        let plan = prompt::Plan::new(material, context, tools, history, summary)
            .for_route(route, continuation, evidence_rule, search_result);
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
                search: false,
                calculator: calculations > 0,
                ..Default::default()
            };
            request.instructions.clone_from(&base_instructions);
            if calculations > 0 {
                request.instructions.push('\n');
                request.instructions.push_str(CALCULATOR_GUIDANCE);
            }
            prompt::check_tool_budget(&request, model.capabilities())?;
            self.activity(key, if route == AskRoute::Edit { AskActivity::Polishing } else { AskActivity::Generating });
            let streaming = route.is_chat() && !request.tools.available.any();
            let (generated, leading) = if streaming {
                self.stream_reviewed_answer(
                    key, model.as_ref(), route, &question, request.clone(), &cancellation,
                ).await?
            } else {
                let generated = self.call(
                    key, model.as_ref(), request.clone(), purpose, cancellation.clone(),
                ).await;
                (generated, None)
            };
            match generated {
                Ok(CallResult::Text(text)) => {
                    return match fulfillment(&text) {
                        Fulfillment::Direct(body) if !body.trim().is_empty() => {
                            if !streaming { self.stage_candidate(key, route, body)?; }
                            if route.is_chat() {
                                self.complete_reviewed_answer(key, model.as_ref(), route, &question, body, leading, &cancellation).await?;
                                return Ok(None);
                            }
                            match self.review_delivery(key, model.as_ref(), route, &question, body, None, &cancellation).await? {
                                DeliveryReview::Approved => Ok(Some((route, body.trim().to_owned()))),
                                DeliveryReview::Rejected => {
                                    self.clear_candidate(key);
                                    Err(ask_error(BackendErrorCode::Provider, "askDraftUnusable"))
                                }
                                DeliveryReview::Unavailable => Err(ask_error(BackendErrorCode::Provider, "askDraftReviewUnavailable")),
                            }
                        }
                        Fulfillment::NeedsPublicEvidence if search_status != AskSearchStatus::Done && route != AskRoute::Edit => {
                            if search_status != AskSearchStatus::Idle {
                                self.update(key, |turn| turn.search_event(Uuid::new_v4(), search_status.clone(), None));
                            }
                            if route.is_chat() {
                                self.answer_search_unavailable(key, &question, search_status);
                                Ok(None)
                            } else {
                                Err(ask_error(BackendErrorCode::Provider, "askDraftSearchUnavailable"))
                            }
                        }
                        _ if route.is_chat() => Err(ask_error(BackendErrorCode::Provider, "askAnswerUnusable")),
                        _ => Err(ask_error(BackendErrorCode::Provider, "askDraftUnusable")),
                    };
                }
                Ok(CallResult::Tool(call)) => {
                    match &call.tool {
                        AskToolCall::Calculate { .. } => calculations -= 1,
                        _ => return Err(invalid_intent()),
                    }
                    let output = self.run_tool(key, &call, &cancellation).await?;
                    request.tools.results.push(AskToolResult { call, output });
                }
                Err(error)
                    if error.message == "askModelRejectedTools"
                        && request.tools.available.any()
                        && request.tools.intent.is_none() =>
                {
                    calculations = 0;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn stream_reviewed_answer(
        &self, key: AskTurnKey, model: &dyn AskModel, route: AskRoute, question: &str,
        request: AskModelRequest, cancellation: &AskCancellation,
    ) -> Result<(Result<CallResult, BackendError>, Option<ReviewedPrefix>), BackendError> {
        let (sender, receiver) = oneshot::channel();
        let generate = async {
            let result = self.call_with_preview(
                key, model, request, AskCallPurpose::Answer, cancellation.clone(), Some(sender),
            ).await;
            if let Ok(CallResult::Text(output)) = &result {
                if let Fulfillment::Direct(body) = fulfillment(output) {
                    if !body.trim().is_empty() {
                        self.stage_candidate(key, route, body)?;
                    }
                }
            }
            Ok(result)
        };
        let inspect = async {
            let Ok(text) = receiver.await else { return Ok(None); };
            let verdict = self.review_delivery(key, model, route, question, &text, None, cancellation).await?;
            if matches!(verdict, DeliveryReview::Approved) {
                self.publish_answer_text(key, &text);
            }
            Ok(Some(ReviewedPrefix { text, verdict }))
        };
        tokio::try_join!(generate, inspect)
    }

    async fn complete_reviewed_answer(
        &self, key: AskTurnKey, model: &dyn AskModel, route: AskRoute, question: &str, body: &str,
        leading: Option<ReviewedPrefix>, cancellation: &AskCancellation,
    ) -> Result<(), BackendError> {
        let (remaining, preceding) = match leading.as_ref() {
            Some(leading) => match leading.verdict {
                DeliveryReview::Approved => (
                    body.strip_prefix(&leading.text).ok_or_else(|| ask_error(BackendErrorCode::Provider, "askAnswerReviewUnavailable"))?,
                    Some(leading.text.as_str()),
                ),
                DeliveryReview::Rejected => {
                    self.clear_candidate(key);
                    return Err(ask_error(BackendErrorCode::Provider, "askAnswerUnusable"));
                }
                DeliveryReview::Unavailable => {
                    return Err(ask_error(BackendErrorCode::Provider, "askAnswerReviewUnavailable"));
                }
            },
            None => (body, None),
        };
        let verdict = if remaining.trim().is_empty() {
            DeliveryReview::Approved
        } else {
            self.review_delivery(key, model, route, question, remaining, preceding, cancellation).await?
        };
        match verdict {
            DeliveryReview::Approved => {
                self.publish_answer_text(key, remaining);
                self.clear_candidate(key);
                Ok(())
            }
            DeliveryReview::Rejected => {
                self.clear_candidate(key);
                Err(ask_error(BackendErrorCode::Provider, "askAnswerUnusable"))
            }
            DeliveryReview::Unavailable => Err(ask_error(BackendErrorCode::Provider, "askAnswerReviewUnavailable")),
        }
    }

    async fn review_delivery(
        &self, key: AskTurnKey, model: &dyn AskModel, route: AskRoute, question: &str, body: &str,
        preceding: Option<&str>, cancellation: &AskCancellation,
    ) -> Result<DeliveryReview, BackendError> {
        let request = AskModelRequest {
            instructions: DELIVERY_REVIEW_RULE.to_owned(),
            messages: vec![AskModelMessage {
                role: AskModelRole::User,
                parts: vec![AskModelPart::Text(serde_json::json!({
                    "route": route, "currentInstruction": question, "candidateBody": body, "approvedPrefix": preceding,
                }).to_string())],
            }],
            output_tokens: model.capabilities().output_tokens.min(512),
            deep_thinking: false,
            tools: AskModelTools::default(),
        };
        if prompt::check_tool_budget(&request, model.capabilities()).is_err() {
            return Ok(DeliveryReview::Unavailable);
        }
        match tokio::time::timeout(
            DELIVERY_REVIEW_WAIT,
            self.call(key, model, request, AskCallPurpose::DeliveryCheck, cancellation.clone()),
        ).await {
            Ok(Ok(CallResult::Text(verdict))) if verdict.trim() == "deliver" => Ok(DeliveryReview::Approved),
            Ok(Ok(CallResult::Text(verdict))) if verdict.trim() == "reject" => Ok(DeliveryReview::Rejected),
            Ok(Err(error)) if error.code == BackendErrorCode::Cancelled => Err(error),
            _ => Ok(DeliveryReview::Unavailable),
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
                let (status, evidence) = self.search_web(key, public_query.clone(), cancellation.clone()).await?;
                if evidence.is_empty() {
                    Ok(serde_json::json!({"status":status, "message":"No verified search evidence was returned."}).to_string())
                } else {
                    Ok(evidence)
                }
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
    ) -> Result<(AskSearchStatus, String), BackendError> {
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
            Ok(AskSearchResult::Disabled) => (AskSearchStatus::Disabled, String::new(), None),
            Ok(AskSearchResult::Done { evidence }) if !matches!(evidence.trim(), "" | "[]") => {
                (AskSearchStatus::Done, evidence, None)
            }
            Ok(AskSearchResult::Done { .. }) => (AskSearchStatus::NoResults, String::new(), None),
            Ok(AskSearchResult::NoResults) => (AskSearchStatus::NoResults, String::new(), None),
            Ok(AskSearchResult::SnippetOnly) => (AskSearchStatus::SnippetOnly, String::new(), None),
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
        Ok((status, evidence))
    }

    async fn choose_public_query(
        &self, key: AskTurnKey, model: &dyn AskModel, question: &str, prior: Option<&str>,
        local_time: Option<&AskContextFact>, cancellation: &AskCancellation,
    ) -> Result<SearchDecision, BackendError> {
        let request = AskModelRequest {
            instructions: format!(
                "{WEB_SEARCH_GUIDANCE}\n只判定本轮是否真的需要公开检索，不回答本轮任务。\
                 引述搜索词、例句、介绍搜索功能、无需查证的任务或明确不联网时返回 JSON 字符串 \"notNeeded\"，不要调用工具。\
                 只有本轮确实指向同次开窗的公开主题才继续用它，保留地点、时间及限定；已有明确新主题就忽略旧主题。\
                 需要查询却没有可辨认的公开主题时返回 JSON 字符串 \"missingPublicTopic\"。\
                 不把附件、选区、私人指代或联系方式当作公开主题。仅输出一个工具调用或上述 JSON 字符串。"
            ),
            messages: vec![AskModelMessage {
                role: AskModelRole::User,
                parts: vec![AskModelPart::Text(serde_json::json!({
                    "currentInstruction": question, "precedingPublicQuery": prior, "localTime": local_time,
                }).to_string())],
            }],
            output_tokens: model.capabilities().output_tokens.min(512),
            deep_thinking: false,
            tools: AskModelTools {
                available: AskToolSet { search: true, ..Default::default() },
                ..Default::default()
            },
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.call(key, model, request, AskCallPurpose::SearchDecision, cancellation.clone()),
        ).await;
        match result {
            Ok(Ok(CallResult::Tool(call))) if matches!(&call.tool, AskToolCall::WebSearch { .. }) => Ok(SearchDecision::Query(call)),
            Ok(Ok(CallResult::Text(reply))) => Ok(match serde_json::from_str(reply.trim()) {
                Ok(SearchDecisionReply::NotNeeded) => SearchDecision::NotNeeded,
                Ok(SearchDecisionReply::MissingPublicTopic) => SearchDecision::MissingTopic,
                Err(_) => SearchDecision::Unavailable(AskSearchStatus::Rejected),
            }),
            Ok(Err(error)) if error.code == BackendErrorCode::Cancelled => Err(error),
            Ok(Err(error)) if !matches!(error.message.as_str(),
                "askModelRejectedTools" | "askInvalidToolCall" | "askUnexpectedToolCall" | "askEmptyModelOutput") =>
                Ok(SearchDecision::Unavailable(AskSearchStatus::Error)),
            Err(_) => Ok(SearchDecision::Unavailable(AskSearchStatus::Error)),
            _ => Ok(SearchDecision::Unavailable(AskSearchStatus::Rejected)),
        }
    }

    fn public_search_topic(&self, key: AskTurnKey) -> Option<String> {
        self.0.state.lock().conversation(key.conversation_id).ok()?.public_search_topic.clone()
    }

    fn set_public_search_topic(&self, key: AskTurnKey, topic: String) {
        if let Ok(conversation) = self.0.state.lock().conversation_mut(key.conversation_id) {
            if conversation.active_turn(key.turn_id).is_some_and(|turn| !turn.cancellation.is_cancelled()) {
                conversation.public_search_topic = Some(topic);
            }
        }
    }

    fn answer_search_unavailable(&self, key: AskTurnKey, question: &str, status: AskSearchStatus) {
        let chinese = question.chars().any(|character| matches!(character, '\u{3400}'..='\u{9fff}'));
        let message = match (chinese, status) {
            (true, AskSearchStatus::Idle) => "本次开窗没有可继续核查的公开主题，无法确定所指的更新。",
            (false, AskSearchStatus::Idle) => "There is no public topic from this opening to check for updates.",
            (true, AskSearchStatus::Unsupported) => "当前模型接口不能调用网页搜索，无法核实实时消息。",
            (false, AskSearchStatus::Unsupported) => "The current model endpoint cannot call Web Search, so I couldn’t verify current information.",
            (true, AskSearchStatus::Rejected) => "本次查询未能形成有效的公开搜索请求，没有核实结果。",
            (false, AskSearchStatus::Rejected) => "The search call was not accepted. There are no verified results from this turn.",
            (true, AskSearchStatus::SnippetOnly) => "本次搜索只返回了未核实的摘要，无法确认实际报道。",
            (false, AskSearchStatus::SnippetOnly) => "Search returned only unverified snippets, not readable evidence.",
            (true, AskSearchStatus::Disabled) => "网页搜索目前已关闭，无法核实实时消息。可以在设置里开启后重试。",
            (false, AskSearchStatus::Disabled) => "Web Search is off, so I can’t verify current news. Enable it in Settings and retry.",
            (true, AskSearchStatus::Error) => "这次网页搜索未能完成，没有可核实的结果。",
            (false, AskSearchStatus::Error) => "Web Search could not complete. There are no verified results from this turn.",
            (true, _) => "网页搜索已运行，但没有找到可核实的结果。我不会把未经核实的消息当作事实。",
            (false, _) => "Web Search ran but returned no verifiable results. I won’t present unverified news as fact.",
        };
        self.update(key, |turn| {
            turn.assistant.text.push_str(message);
            Some(AskEventKind::AnswerDelta {
                message_id: turn.assistant.message_id, delta: message.into(),
            })
        });
    }

    async fn call(
        &self,
        key: AskTurnKey,
        model: &dyn AskModel,
        request: AskModelRequest,
        purpose: AskCallPurpose,
        cancellation: AskCancellation,
    ) -> Result<CallResult, BackendError> {
        self.call_with_preview(key, model, request, purpose, cancellation, None).await
    }

    async fn call_with_preview(
        &self, key: AskTurnKey, model: &dyn AskModel, request: AskModelRequest,
        purpose: AskCallPurpose, cancellation: AskCancellation, preview: Option<oneshot::Sender<String>>,
    ) -> Result<CallResult, BackendError> {
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
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
        let callback_lifetime = cancellation.clone();
        let preview = Arc::new(Mutex::new(preview));
        let first_boundary = preview.clone();
        let progress: AskModelSink = Arc::new(move |progress| {
            if callback_lifetime.is_cancelled() { return; }
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
                    if purpose == AskCallPurpose::Answer && !available.any() {
                        service.activity(key, AskActivity::Generating);
                        let mut sender = first_boundary.lock();
                        if sender.is_some() {
                            if let Fulfillment::Direct(body) = fulfillment(output) {
                                if let Some((paragraph, _)) = body.split_once("\n\n") {
                                    if !paragraph.trim().is_empty() && paragraph.len() <= 4_096 {
                                        if let Some(sender) = sender.take() {
                                            let _ = sender.send(format!("{paragraph}\n\n"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                AskModelProgress::Thinking
                    if purpose == AskCallPurpose::Answer && output.is_empty() =>
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
            _ = cancellation.cancelled() => Err(cancelled()),
            _ = output_limit.cancelled() => Ok(AskModelCompletion::Complete),
            completion = model.stream(request, cancellation.clone(), progress) => completion,
        };
        preview.lock().take();
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
                    | AskCallPurpose::DeliveryCheck
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
            } else if let Some(body) = turn.pending_body.take() {
                turn.delivery = Some(AskDeliveryStatus::Retained);
                turn.result_text = Some(body);
            } else {
                turn.delivery = None;
            }
            events.push(turn.delivery_event());
        } else if status != AskTurnStatus::Completed && turn.is_chat() {
            if let Some(body) = turn.pending_body.take() {
                turn.result_text = Some(body);
                events.push(turn.delivery_event());
            }
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
