//! Exercise the host-facing QA entry with HTTP model fixtures, never user settings.
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use openless_core::{
    answer_qa_with_context,
    web_search::{SearchProvider, WebSearchConfig, CONFIG_ACCOUNT},
    BackendError, CredentialKey, CredentialNamespace, CredentialStore, DictationContext,
    InMemoryCredentialStore, ProviderInvocation, QaMessage, QaProgress, QaProgressSink,
    SecretValue, SessionId,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Progress {
    session: SessionId,
    answer: Mutex<String>,
}
impl QaProgressSink for Progress {
    fn publish(&self, session: SessionId, progress: QaProgress) -> Result<(), BackendError> {
        assert_eq!(session, self.session);
        if let QaProgress::AnswerDelta(text) = progress {
            self.answer.lock().unwrap().push_str(&text);
        }
        Ok(())
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    let header_end = loop {
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 128 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    (headers, bytes[header_end..header_end + length].to_vec())
}

async fn run_turn(provider: SearchProvider) -> Vec<Value> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let off = provider == SearchProvider::Off;
    let local_search = provider == SearchProvider::Searxng;
    let server = tokio::spawn(async move {
        let mut model_requests = Vec::new();
        let mut search_requests = 0;
        let expected_models = if off { 1 } else { 2 };
        while model_requests.len() < expected_models {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, body) = read_request(&mut socket).await;
            let (kind, response) = if headers.starts_with("GET /search?") {
                search_requests += 1;
                assert!(headers.contains("q=Rust+language+documentation&format=json"));
                assert!(!headers.to_lowercase().contains("authorization:"));
                (
                    "application/json",
                    json!({"results":[{
                        "title":"Rust Documentation", "url":"https://doc.rust-lang.org/",
                        "content":"Authoritative Rust docs fixture"
                    }]})
                    .to_string(),
                )
            } else {
                assert!(headers.starts_with("POST /v1/chat/completions "));
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"], "qa-fixture");
                assert!(
                    body.get("tools").is_none(),
                    "ordinary QA must use the selected independent search"
                );
                model_requests.push(body);
                let text = if !off && model_requests.len() == 1 {
                    r#"{"query":"Rust language documentation"}"#
                } else {
                    "fixture final answer"
                };
                (
                    "text/event-stream",
                    format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"delta":{"content":text}}]})
                    ),
                )
            };
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        }
        assert_eq!(search_requests, usize::from(local_search));
        model_requests
    });
    let store = Arc::new(InMemoryCredentialStore::default());
    for (namespace, channel, account, value) in [
        (
            CredentialNamespace::Llm,
            Some("qa-fixture".into()),
            "ark.endpoint",
            format!("{origin}/v1"),
        ),
        (
            CredentialNamespace::Llm,
            Some("qa-fixture".into()),
            "ark.api_key",
            "model-fixture-key".into(),
        ),
        (
            CredentialNamespace::Application,
            None,
            CONFIG_ACCOUNT,
            serde_json::to_string(&WebSearchConfig {
                provider,
                api_key: String::new(),
                endpoint: format!("{origin}/search"),
            })
            .unwrap(),
        ),
    ] {
        store
            .write(
                CredentialKey::new(namespace, channel, account).unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    let mut context = DictationContext::default();
    context.llm = ProviderInvocation::new("qa-fixture", "openai-compatible");
    context.llm.model = Some("qa-fixture".into());
    let session = SessionId::new();
    let progress = Arc::new(Progress {
        session,
        answer: Mutex::new(String::new()),
    });
    let answer = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        answer_qa_with_context(
            store,
            Arc::new(context),
            vec![QaMessage {
                id: "question".into(),
                role: "user".into(),
                content: "Search for Rust language documentation".into(),
                selection_text: None,
            }],
            None,
            session,
            progress.clone(),
            Arc::new(AtomicBool::new(false)),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(answer, "fixture final answer");
    assert_eq!(*progress.answer.lock().unwrap(), answer);
    server.await.unwrap()
}

fn search_rows(request: &Value) -> Vec<Value> {
    let content = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_str())
        .find(|content| content.contains("<WebSearchData "))
        .expect("answer request must contain retrieved sources");
    let rows = content
        .lines()
        .find(|line| line.starts_with("[{"))
        .expect("structured source data");
    serde_json::from_str(rows).unwrap()
}

#[tokio::test]
async fn qa_selected_search_config_reaches_planner_and_final_model_request() {
    let requests = run_turn(SearchProvider::Searxng).await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].to_string().contains("Return ONLY JSON"));
    let rows = search_rows(&requests[1]);
    assert_eq!(rows[0]["url"], "https://doc.rust-lang.org/");
    assert!(requests[1]
        .to_string()
        .contains("Authoritative Rust docs fixture"));
    assert!(requests[1].to_string().contains("用户配置的 SearXNG"));
}

#[tokio::test]
async fn qa_off_skips_planning_and_source_injection() {
    let requests = run_turn(SearchProvider::Off).await;
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].to_string().contains("<WebSearchData date="));
    assert!(!requests[0].to_string().contains("Return ONLY JSON"));
}

async fn live_selected_search(provider: SearchProvider, hybrid: bool) {
    let requests = run_turn(provider).await;
    let rows = search_rows(&requests[1]);
    assert!(!rows.is_empty());
    assert!(rows.iter().any(|row| row["providers"]
        .as_array()
        .is_some_and(|providers| providers
            .iter()
            .any(|provider| provider == "FreeSerp" || provider == "Keenable"))));
    assert!(requests[1].to_string().contains(if hybrid {
        "混合搜索"
    } else {
        "免费聚合搜索"
    }));
    println!(
        "QA model is a local fixture; actual selected search sources: {}",
        serde_json::to_string(
            &rows
                .iter()
                .map(|row| json!({"url":row["url"], "providers":row["providers"]}))
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
}

#[tokio::test]
#[ignore = "real free endpoints, local model fixture; does not enable Ask Anything"]
async fn qa_selected_free_reaches_answer_request_live() {
    live_selected_search(SearchProvider::Free, false).await;
}

#[tokio::test]
#[ignore = "real free fallback without DeepSeek key, local model fixture; not paid hybrid success"]
async fn qa_selected_hybrid_without_key_reaches_answer_request_live() {
    live_selected_search(SearchProvider::Hybrid, true).await;
}
