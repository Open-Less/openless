//! Standalone Ask search: confirmed public input, progress without page bodies, and bounded reads.

use crate::web_search::{self, PageReadError, RequestPolicy, SearchProvider, WebSearchConfig};
use crate::{CancellationToken, CredentialStore};
use chrono::{SecondsFormat, Utc};
use futures_util::future::{join_all, BoxFuture};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub const MAX_PUBLIC_QUERY_CHARS: usize = 400;
const SEARCH_DEADLINE: Duration = Duration::from_secs(15);
const MAX_PAGE_READS: usize = 3;
const MAX_SOURCE_URL_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PublicQueryError {
    #[error("Confirm a non-empty public search query")]
    Empty,
    #[error("Public search query exceeds 400 characters")]
    TooLong,
}

/// Construct once from the accepted Ask submission, never from the prompt,
/// model tool arguments, attachment, or conversation history.
pub struct SearchTurn {
    conversation_id: Uuid,
    turn_id: Uuid,
    query: Option<String>,
}

impl SearchTurn {
    pub fn new(
        conversation_id: Uuid,
        turn_id: Uuid,
        ask_web_search: bool,
        confirmed_public_query: Option<&str>,
    ) -> Result<Self, PublicQueryError> {
        let query = if let Some(raw) = confirmed_public_query.filter(|_| ask_web_search) {
            let query = raw.trim();
            if query.is_empty() {
                return Err(PublicQueryError::Empty);
            }
            if query.chars().count() > MAX_PUBLIC_QUERY_CHARS {
                return Err(PublicQueryError::TooLong);
            }
            Some(query.to_owned())
        } else {
            None
        };
        Ok(Self {
            conversation_id,
            turn_id,
            query,
        })
    }

    pub fn take_attempt(&mut self, tool_call_id: Uuid) -> Option<SearchAttempt> {
        Some(SearchAttempt {
            scope: SearchScope {
                conversation_id: self.conversation_id,
                turn_id: self.turn_id,
                tool_call_id,
            },
            public_query: self.query.take()?,
        })
    }
}

/// A single-use capability. The tool consumer cannot replace the approved query.
pub struct SearchAttempt {
    scope: SearchScope,
    public_query: String,
}

impl SearchAttempt {
    pub fn scope(&self) -> SearchScope {
        self.scope
    }

    pub fn public_query(&self) -> &str {
        &self.public_query
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchScope {
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
    pub tool_call_id: Uuid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchStatus {
    #[default]
    Idle,
    Searching,
    Reading,
    Done,
    NoResults,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadStatus {
    NotAttempted,
    Reading,
    Read,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSource {
    pub source_id: Uuid,
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub read_status: ReadStatus,
}

impl SearchSource {
    /// Only call after resolving source_id in this conversation and turn's
    /// actual search records. Never pass a URL from an IPC request here.
    pub fn browser_url(&self) -> Option<url::Url> {
        let url = url::Url::parse(&self.url).ok()?;
        (matches!(url.scheme(), "http" | "https")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none())
        .then_some(url)
    }
}

pub struct SearchProgress {
    pub scope: SearchScope,
    pub update: SearchUpdate,
}

pub enum SearchUpdate {
    Status(SearchStatus),
    Source(SearchSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchErrorCode {
    Configuration,
    Provider,
    Timeout,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchError {
    pub code: SearchErrorCode,
    pub message: String,
}

/// Model-only content retrieved by the tool. `retrieved_at` is not the
/// publication, observation, or forecast time. No bodies in events or history.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEvidence {
    pub source: SearchSource,
    pub retrieved_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

pub struct SearchOutcome {
    pub status: SearchStatus,
    pub error: Option<SearchError>,
    pub sources: Vec<SearchSource>,
    pub evidence: Vec<SearchEvidence>,
}

pub trait SearchTool: Send + Sync {
    fn search<'a>(
        &'a self,
        attempt: SearchAttempt,
        cancel: CancellationToken,
        on_progress: &'a (dyn Fn(SearchProgress) + Sync),
    ) -> BoxFuture<'a, SearchOutcome>;
}

pub struct WebSearchTool {
    credentials: Arc<dyn CredentialStore>,
}

impl WebSearchTool {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self { credentials }
    }
}

impl SearchTool for WebSearchTool {
    fn search<'a>(
        &'a self,
        attempt: SearchAttempt,
        cancel: CancellationToken,
        on_progress: &'a (dyn Fn(SearchProgress) + Sync),
    ) -> BoxFuture<'a, SearchOutcome> {
        Box::pin(async move {
            let run = SearchRun {
                attempt,
                cancel,
                on_progress,
                state: Mutex::default(),
            };
            let disposition = tokio::select! {
                biased;
                _ = run.cancel.cancelled() => Ok(SearchStatus::Cancelled),
                result = tokio::time::timeout(SEARCH_DEADLINE, run.execute(self.credentials.as_ref())) => {
                    result.unwrap_or_else(|_| Err(SearchError {
                        code: SearchErrorCode::Timeout,
                        message: "Web Search timed out".into(),
                    }))
                }
            };
            run.finish(disposition)
        })
    }
}

#[derive(Default)]
struct RunState {
    status: SearchStatus,
    evidence: Vec<SearchEvidence>,
}

struct SearchRun<'a> {
    attempt: SearchAttempt,
    cancel: CancellationToken,
    on_progress: &'a (dyn Fn(SearchProgress) + Sync),
    state: Mutex<RunState>,
}

impl SearchRun<'_> {
    fn emit(&self, update: SearchUpdate) {
        (self.on_progress)(SearchProgress {
            scope: self.attempt.scope,
            update,
        });
    }

    fn set_status(&self, status: SearchStatus) {
        let previous = std::mem::replace(&mut self.state.lock().status, status);
        if previous != status {
            self.emit(SearchUpdate::Status(status));
        }
    }

    async fn execute(
        &self,
        credentials: &dyn CredentialStore,
    ) -> Result<SearchStatus, SearchError> {
        let config = WebSearchConfig::load(credentials)
            .await
            .map_err(|_| SearchError {
                code: SearchErrorCode::Configuration,
                message: "Could not load Web Search configuration".into(),
            })?;
        match config.provider {
            SearchProvider::Off => return Ok(SearchStatus::Idle),
            SearchProvider::Tavily | SearchProvider::Brave if config.api_key.trim().is_empty() => {
                return Err(SearchError {
                    code: SearchErrorCode::Configuration,
                    message: "The configured Web Search provider needs an API key".into(),
                });
            }
            SearchProvider::Searxng if config.endpoint.trim().is_empty() => {
                return Err(SearchError {
                    code: SearchErrorCode::Configuration,
                    message: "The configured Web Search provider needs a SearXNG endpoint".into(),
                });
            }
            _ => {}
        }
        let results = web_search::query_sources(
            &config,
            self.attempt.public_query(),
            RequestPolicy::Ask,
            || {
                if self.cancel.is_cancelled() {
                    return false;
                }
                self.set_status(SearchStatus::Searching);
                !self.cancel.is_cancelled()
            },
        )
        .await
        .map_err(|message| SearchError {
            code: SearchErrorCode::Provider,
            message,
        })?;
        let retrieved_at = timestamp();
        let mut pages = Vec::new();
        for result in results {
            if self.cancel.is_cancelled() {
                break;
            }
            if result.url.len() > MAX_SOURCE_URL_BYTES {
                continue;
            }
            let source = SearchSource {
                source_id: Uuid::new_v4(),
                url: result.url,
                title: result.title,
                snippet: (!result.snippet.trim().is_empty()).then_some(result.snippet),
                read_status: ReadStatus::NotAttempted,
            };
            let mut state = self.state.lock();
            if state
                .evidence
                .iter()
                .any(|item| item.source.url == source.url)
            {
                continue;
            }
            if pages.len() < MAX_PAGE_READS {
                pages.push((state.evidence.len(), source.url.clone()));
            }
            state.evidence.push(SearchEvidence {
                source: source.clone(),
                retrieved_at: retrieved_at.clone(),
                excerpt: None,
            });
            drop(state);
            self.emit(SearchUpdate::Source(source));
        }
        if pages.is_empty() {
            return Ok(SearchStatus::NoResults);
        }
        join_all(pages.into_iter().map(|(index, url)| self.read(index, url))).await;
        Ok(SearchStatus::Done)
    }

    async fn read(&self, index: usize, url: String) {
        if self.cancel.is_cancelled() {
            return;
        }
        let result = web_search::fetch_public_page(&url, RequestPolicy::Ask, || {
            if self.cancel.is_cancelled() {
                return false;
            }
            self.set_status(SearchStatus::Reading);
            self.update_read(index, ReadStatus::Reading, None);
            !self.cancel.is_cancelled()
        })
        .await;
        if self.cancel.is_cancelled() {
            return;
        }
        match result {
            Ok(excerpt) => {
                self.update_read(index, ReadStatus::Read, Some(excerpt));
            }
            Err(PageReadError::Blocked) => self.update_read(index, ReadStatus::Blocked, None),
            _ => self.update_read(index, ReadStatus::Failed, None),
        }
    }

    fn update_read(&self, index: usize, status: ReadStatus, excerpt: Option<String>) {
        let mut state = self.state.lock();
        let item = &mut state.evidence[index];
        item.source.read_status = status;
        if excerpt.is_some() {
            item.retrieved_at = timestamp();
            item.excerpt = excerpt;
        }
        let source = item.source.clone();
        drop(state);
        self.emit(SearchUpdate::Source(source));
    }

    fn finish(self, disposition: Result<SearchStatus, SearchError>) -> SearchOutcome {
        let (status, error) = if self.cancel.is_cancelled() {
            (SearchStatus::Cancelled, None)
        } else {
            match disposition {
                Ok(status) => (status, None),
                Err(error) => (SearchStatus::Error, Some(error)),
            }
        };
        let unfinished_status = if status == SearchStatus::Cancelled {
            ReadStatus::Cancelled
        } else {
            ReadStatus::Failed
        };
        let unfinished: Vec<_> = self
            .state
            .lock()
            .evidence
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                (item.source.read_status == ReadStatus::Reading).then_some(index)
            })
            .collect();
        for index in unfinished {
            self.update_read(index, unfinished_status, None);
        }
        self.set_status(status);
        let evidence = self.state.into_inner().evidence;
        let sources = evidence.iter().map(|item| item.source.clone()).collect();
        SearchOutcome {
            status,
            error,
            sources,
            evidence: if status == SearchStatus::Cancelled {
                Vec::new()
            } else {
                evidence
            },
        }
    }
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl crate::ask::SearchTool for WebSearchTool {
    fn search(
        &self,
        request: crate::ask::AskSearchRequest,
        cancellation: crate::ask::AskCancellation,
        progress: crate::ask::AskToolSink,
    ) -> BoxFuture<'_, Result<crate::ask::AskSearchResult, crate::BackendError>> {
        use crate::ask::{
            AskReadStatus, AskSearchResult, AskSearchStatus, AskSource, AskToolProgress,
        };
        use crate::{BackendError, BackendErrorCode};

        Box::pin(async move {
            let key = request.key;
            let attempt = SearchTurn::new(
                key.conversation_id,
                key.turn_id,
                true,
                Some(&request.public_query),
            )
            .map_err(|error| {
                BackendError::new(BackendErrorCode::InvalidArgument, error.to_string())
            })?
            .take_attempt(key.tool_call_id);
            let Some(attempt) = attempt else {
                return Ok(AskSearchResult::Disabled);
            };
            let cancel = CancellationToken::new();
            let notify = |update: SearchProgress| {
                let scope = update.scope;
                if cancellation.is_cancelled()
                    || scope.conversation_id != key.conversation_id
                    || scope.turn_id != key.turn_id
                    || scope.tool_call_id != key.tool_call_id
                {
                    return;
                }
                progress(match update.update {
                    SearchUpdate::Status(SearchStatus::Searching) => {
                        AskToolProgress::Status(AskSearchStatus::Searching)
                    }
                    SearchUpdate::Status(SearchStatus::Reading) => {
                        AskToolProgress::Status(AskSearchStatus::Reading)
                    }
                    SearchUpdate::Status(_) => return,
                    SearchUpdate::Source(source) => AskToolProgress::Source(AskSource {
                        source_id: source.source_id.to_string(),
                        url: source.url,
                        title: source.title,
                        snippet: source.snippet,
                        read_status: match source.read_status {
                            ReadStatus::NotAttempted => AskReadStatus::Discovered,
                            ReadStatus::Reading => AskReadStatus::Reading,
                            ReadStatus::Read => AskReadStatus::Read,
                            ReadStatus::Blocked => AskReadStatus::Blocked,
                            ReadStatus::Failed => AskReadStatus::Error,
                            ReadStatus::Cancelled => AskReadStatus::Cancelled,
                        },
                    }),
                });
            };
            let outcome = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    cancel.cancel();
                    return Err(BackendError::new(BackendErrorCode::Cancelled, "Search cancelled"));
                }
                outcome = SearchTool::search(self, attempt, cancel.clone(), &notify) => outcome,
            };
            match outcome.status {
                SearchStatus::Idle => Ok(AskSearchResult::Disabled),
                SearchStatus::NoResults => Ok(AskSearchResult::NoResults),
                SearchStatus::Cancelled => Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Search cancelled",
                )),
                SearchStatus::Done => Ok(AskSearchResult::Done {
                    evidence: budgeted_evidence(outcome.evidence, request.evidence_token_budget),
                }),
                _ => Err(BackendError::new(
                    BackendErrorCode::Provider,
                    outcome
                        .error
                        .map_or_else(|| "Web search failed".into(), |error| error.message),
                )),
            }
        })
    }
}

fn budgeted_evidence(entries: Vec<SearchEvidence>, budget: usize) -> String {
    let mut included = Vec::new();
    for mut entry in entries {
        // Retain the returned URL and retrieval time. Bound excerpts separately so
        // an extensive page can't crowd out all the other sources.
        if let Some(text) = &mut entry.excerpt {
            *text = text.chars().take(420).collect();
        }
        if let Some(text) = &mut entry.source.snippet {
            *text = text.chars().take(220).collect();
        }
        included.push(entry);
        let encoded = serde_json::to_string(&included).unwrap_or_default();
        if serde_json::json!({"webEvidence": encoded})
            .to_string()
            .len()
            > budget
        {
            included.pop();
        }
    }
    if included.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&included).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CredentialKey, CredentialNamespace, InMemoryCredentialStore, SecretValue};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn turn(enabled: bool, query: Option<&str>) -> SearchTurn {
        SearchTurn::new(Uuid::new_v4(), Uuid::new_v4(), enabled, query).unwrap()
    }

    fn attempt() -> SearchAttempt {
        turn(true, Some("Madrid weather"))
            .take_attempt(Uuid::new_v4())
            .unwrap()
    }

    fn tool(config: &WebSearchConfig) -> WebSearchTool {
        let key = CredentialKey::new(
            CredentialNamespace::Application,
            None,
            web_search::CONFIG_ACCOUNT,
        )
        .unwrap();
        let secret = SecretValue::new(serde_json::to_string(config).unwrap());
        WebSearchTool::new(Arc::new(
            InMemoryCredentialStore::default().with_secret(key, secret),
        ))
    }

    #[test]
    fn only_one_explicit_query_is_available_per_turn() {
        assert!(turn(false, Some("Madrid weather"))
            .take_attempt(Uuid::new_v4())
            .is_none());
        assert!(turn(true, None).take_attempt(Uuid::new_v4()).is_none());
        let mut approved = turn(true, Some("  Madrid weather "));
        assert_eq!(
            approved
                .take_attempt(Uuid::new_v4())
                .unwrap()
                .public_query(),
            "Madrid weather"
        );
        assert!(approved.take_attempt(Uuid::new_v4()).is_none());
        assert!(matches!(
            SearchTurn::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                true,
                Some(&"雨".repeat(401))
            ),
            Err(PublicQueryError::TooLong)
        ));
    }

    #[tokio::test]
    async fn global_off_does_not_start_search() {
        let outcome = tool(&WebSearchConfig {
            provider: SearchProvider::Off,
            ..Default::default()
        })
        .search(attempt(), CancellationToken::new(), &|_| {
            panic!("unexpected progress")
        })
        .await;
        assert_eq!(outcome.status, SearchStatus::Idle);
    }

    #[tokio::test]
    async fn source_discovery_does_not_claim_blocked_reads_or_expose_page_bodies() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/search?ignored=yes",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let count = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /search?q=Madrid+weather&format=json "));
            assert!(!request.contains("not-for-searxng"));
            let body = r#"{"results":[{"title":"Result","url":"http://127.0.0.1/","content":"provider snippet"},{"url":"javascript:alert(1)"}]}"#;
            socket.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ).as_bytes()).await.unwrap();
        });
        let progress = Mutex::new(Vec::new());
        let outcome = tool(&WebSearchConfig {
            provider: SearchProvider::Searxng,
            endpoint,
            api_key: "not-for-searxng".into(),
        })
        .search(attempt(), CancellationToken::new(), &|event| {
            progress.lock().push(event.update)
        })
        .await;
        server.await.unwrap();
        assert!(matches!(
            progress.into_inner().as_slice(),
            [
                SearchUpdate::Status(SearchStatus::Searching),
                SearchUpdate::Source(SearchSource {
                    read_status: ReadStatus::NotAttempted,
                    ..
                }),
                SearchUpdate::Source(SearchSource {
                    read_status: ReadStatus::Blocked,
                    ..
                }),
                SearchUpdate::Status(SearchStatus::Done),
            ]
        ));
        assert_eq!(
            outcome.sources[0].snippet.as_deref(),
            Some("provider snippet")
        );
        assert!(outcome.evidence[0].excerpt.is_none());
    }

    #[tokio::test]
    async fn cancellation_drops_a_request_waiting_for_its_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/search", listener.local_addr().unwrap());
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 8192];
            assert_ne!(socket.read(&mut buffer).await.unwrap(), 0);
            server_cancel.cancel();
            assert_eq!(socket.read(&mut buffer).await.unwrap(), 0);
        });
        let progress = Mutex::new(Vec::new());
        let outcome = tokio::time::timeout(Duration::from_secs(2), async {
            let outcome = tool(&WebSearchConfig {
                provider: SearchProvider::Searxng,
                endpoint,
                api_key: String::new(),
            })
            .search(attempt(), cancel, &|event| {
                progress.lock().push(event.update)
            })
            .await;
            server.await.unwrap();
            outcome
        })
        .await
        .unwrap();
        assert_eq!(outcome.status, SearchStatus::Cancelled);
        assert!(matches!(
            progress.into_inner().as_slice(),
            [
                SearchUpdate::Status(SearchStatus::Searching),
                SearchUpdate::Status(SearchStatus::Cancelled),
            ]
        ));
    }
}
