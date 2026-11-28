use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{
    parse_results, response_bytes, RequestError, SearchProvider, SearchResult, WebSearchConfig,
    MAX_RESULTS,
};

const SOURCE_TIMEOUT: Duration = Duration::from_secs(8);
const PUBLIC_REQUEST_SPACING: Duration = Duration::from_millis(500);
const DEEPSEEK_QUERY_LIMIT: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    FreeSerp,
    Keenable,
    DeepSeek,
}

// Share rate-limit cooldowns across local conversations.
static FREE_SERP_PAUSE: Mutex<Option<Instant>> = Mutex::new(None);
static KEENABLE_PAUSE: Mutex<Option<Instant>> = Mutex::new(None);

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::FreeSerp => "FreeSerp",
            Self::Keenable => "Keenable",
            Self::DeepSeek => "DeepSeek",
        }
    }

    fn pause(self) -> Option<&'static Mutex<Option<Instant>>> {
        match self {
            Self::FreeSerp => Some(&FREE_SERP_PAUSE),
            Self::Keenable => Some(&KEENABLE_PAUSE),
            Self::DeepSeek => None,
        }
    }

    fn request(
        self,
        client: &reqwest::Client,
        query: &str,
        deepseek_key: &str,
    ) -> Result<reqwest::Request, RequestError> {
        let request = match self {
            Self::FreeSerp => client.get("https://freeserp.ai/api.php").query(&[
                ("index", "web"),
                ("q", query),
                ("size", "5"),
            ]),
            Self::Keenable => client
                .post("https://api.keenable.ai/v1/search/public")
                .header("X-Keenable-Title", "OpenLess")
                .json(&json!({"query":query,"max_results":5,"snippet_max_length":220})),
            Self::DeepSeek => {
                if deepseek_key.is_empty() {
                    return Err("DeepSeek Search API key is missing".into());
                }
                if query.chars().count() > DEEPSEEK_QUERY_LIMIT {
                    return Err("Search query exceeds the DeepSeek limit".into());
                }
                client
                    .post("https://api.deepseek.com/v1/tools/search")
                    .bearer_auth(deepseek_key)
                    .json(&json!({"query":query,"count":5,"need_content":false}))
            }
        };
        request.build().map_err(|_| "Invalid search request".into())
    }

    fn parse(self, bytes: &[u8]) -> Result<Vec<SearchResult>, RequestError> {
        let body: Value = serde_json::from_slice(bytes).map_err(|_| "Invalid search response")?;
        if (self == Self::FreeSerp && body["ok"] != true)
            || body.get("error").is_some_and(|error| !error.is_null())
        {
            return Err("Search provider rejected the request".into());
        }
        if self == Self::FreeSerp && body["engine_fallback"] == true {
            return Err("The FreeSerp web index is unavailable".into());
        }
        let mut results = parse_results(&body["results"], &["snippet", "summary"])
            .map_err(|_| RequestError::from("Invalid search results"))?;
        for result in &mut results {
            result.providers.push(self.name().into());
        }
        Ok(results)
    }

    fn claim(self) -> Result<(), String> {
        if let Some(pause) = self.pause() {
            let now = Instant::now();
            let mut next = pause.lock();
            if next.is_some_and(|until| until > now) {
                return Err(format!("{} rate limited", self.name()));
            }
            *next = Some(now + PUBLIC_REQUEST_SPACING);
        }
        Ok(())
    }

    async fn query(
        self,
        client: &reqwest::Client,
        request: reqwest::Request,
    ) -> Result<Vec<SearchResult>, String> {
        let result = tokio::time::timeout(SOURCE_TIMEOUT, async {
            let bytes = response_bytes(client, request).await?;
            self.parse(&bytes)
        })
        .await
        .unwrap_or_else(|_| Err("Search provider timed out".into()));
        result.map_err(|error| {
            if let (Some(pause), Some(after)) = (self.pause(), error.retry_after) {
                let until = Instant::now() + after;
                let mut saved = pause.lock();
                *saved = Some(saved.map_or(until, |previous| previous.max(until)));
            }
            format!("{}: {}", self.name(), error.message)
        })
    }
}

pub(super) async fn query(
    config: &WebSearchConfig,
    query: &str,
    on_request: impl FnOnce() -> bool,
) -> Result<Vec<SearchResult>, String> {
    let sources: &[Source] = match config.provider {
        SearchProvider::FreeSerp => &[Source::FreeSerp],
        SearchProvider::Hybrid => &[Source::FreeSerp, Source::Keenable, Source::DeepSeek],
        _ => &[Source::FreeSerp, Source::Keenable],
    };
    // Requests originate on the device and respect its proxy policy, without an app relay.
    let client = crate::net::anonymous_no_redirect_http();
    let mut replies = Vec::new();
    let mut requests = Vec::new();
    for &source in sources {
        match source
            .request(&client, query, config.api_key.trim())
            .map_err(String::from)
            .and_then(|request| {
                source.claim()?;
                Ok(request)
            }) {
            Ok(request) => requests.push(source.query(&client, request)),
            Err(error) => replies.push(Err(error)),
        }
    }
    if !requests.is_empty() {
        if !on_request() {
            return Err("Web Search cancelled".into());
        }
        replies.extend(join_all(requests).await);
    }
    merge(replies)
}

fn merge(replies: Vec<Result<Vec<SearchResult>, String>>) -> Result<Vec<SearchResult>, String> {
    if replies.iter().all(Result::is_err) {
        return Err(replies
            .into_iter()
            .filter_map(Result::err)
            .collect::<Vec<_>>()
            .join("; "));
    }
    let mut remaining: Vec<_> = replies
        .into_iter()
        .filter_map(Result::ok)
        .map(Vec::into_iter)
        .collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut results: Vec<SearchResult> = Vec::new();
    // Interleave by rank so one provider cannot occupy the whole result budget.
    loop {
        let mut advanced = false;
        for items in &mut remaining {
            if let Some(item) = items.next() {
                advanced = true;
                let key = item.url.split('#').next().unwrap_or(&item.url);
                if let Some(&index) = seen.get(key) {
                    for provider in item.providers {
                        if !results[index].providers.contains(&provider) {
                            results[index].providers.push(provider);
                        }
                    }
                } else {
                    seen.insert(key.to_owned(), results.len());
                    results.push(item);
                }
            }
        }
        if !advanced {
            results.truncate(MAX_RESULTS);
            return if results.is_empty() {
                Err("Web Search returned no usable sources".into())
            } else {
                Ok(results)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_routes_send_no_credentials_and_deepseek_only_receives_the_query() {
        let client = reqwest::Client::new();
        let free = Source::FreeSerp
            .request(&client, "rust + 文档", "secret")
            .ok()
            .unwrap();
        assert_eq!(free.url().host_str(), Some("freeserp.ai"));
        assert_eq!(
            free.url()
                .query_pairs()
                .find(|(name, _)| name == "q")
                .unwrap()
                .1,
            "rust + 文档"
        );
        let keenable = Source::Keenable
            .request(&client, "rust", "secret")
            .ok()
            .unwrap();
        assert_eq!(keenable.url().path(), "/v1/search/public");
        assert_eq!(keenable.headers()["X-Keenable-Title"], "OpenLess");
        for request in [&free, &keenable] {
            assert!(request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .is_none());
        }
        let deepseek = Source::DeepSeek
            .request(&client, "rust", "secret")
            .ok()
            .unwrap();
        assert_eq!(
            deepseek.url().as_str(),
            "https://api.deepseek.com/v1/tools/search"
        );
        assert_eq!(
            deepseek.headers()[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
        let body: Value =
            serde_json::from_slice(deepseek.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body, json!({"query":"rust","count":5,"need_content":false}));
        assert!(Source::DeepSeek
            .request(&client, &"文".repeat(129), "secret")
            .is_err());
        assert!(Source::DeepSeek
            .request(&client, &"文".repeat(128), "secret")
            .is_ok());
    }

    #[test]
    fn merging_is_bounded_and_tolerates_a_source_failure() {
        let fixture =
            |engine: Source, names: &[&str]| {
                let results: Vec<_> = names.iter().map(|name| json!({
                "title":name,"url":format!("https://example.com/{name}"),"snippet":"public"
            })).collect();
                engine
                    .parse(&serde_json::to_vec(&json!({"ok":true,"results":results})).unwrap())
                    .map_err(String::from)
            };
        let merged = merge(vec![
            fixture(Source::FreeSerp, &["a", "b", "c"]),
            fixture(Source::Keenable, &["a#section", "d", "e"]),
            fixture(Source::DeepSeek, &["f", "g", "h"]),
        ])
        .unwrap();
        assert_eq!(
            merged
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "f", "b", "d", "g"]
        );
        assert_eq!(merged[0].providers, ["FreeSerp", "Keenable"]);
        assert_eq!(
            merge(vec![
                Err("limited".into()),
                fixture(Source::Keenable, &["a"])
            ])
            .unwrap()
            .len(),
            1
        );
        assert!(merge(vec![Err("limited".into()), Err("offline".into())]).is_err());
        assert!(merge(vec![Ok(vec![]), Ok(vec![])]).is_err());
        assert!(merge(vec![Err("blocked".into()), Ok(vec![])]).is_err());
        assert_eq!(
            merge(vec![
                Err("HTTP 403".into()),
                Err("timeout".into()),
                fixture(Source::DeepSeek, &["only"]),
            ])
            .unwrap()[0]
                .providers,
            ["DeepSeek"]
        );
        assert!(Source::Keenable.parse(b"<html>CAPTCHA</html>").is_err());
        assert!(Source::DeepSeek
            .request(&reqwest::Client::new(), "rust", "")
            .is_err());
        assert!(Source::FreeSerp
            .parse(br#"{"ok":false,"results":[]}"#)
            .is_err());
        assert!(Source::FreeSerp
            .parse(br#"{"ok":true,"engine_fallback":true,"results":[]}"#)
            .is_err());
    }

    #[tokio::test]
    #[ignore = "requires live public search endpoints"]
    async fn public_search_smoke() {
        let rows = super::super::search(&WebSearchConfig::default(), "Rust language documentation")
            .await
            .unwrap();
        assert!(!rows.is_empty() && rows.len() <= MAX_RESULTS);
        println!("{}", serde_json::to_string(&rows).unwrap());
    }

    #[tokio::test]
    #[ignore = "live free endpoints; no DeepSeek request without a key"]
    async fn hybrid_missing_key_preserves_live_free_sources() {
        let rows = super::super::search(
            &WebSearchConfig {
                provider: SearchProvider::Hybrid,
                ..Default::default()
            },
            "Rust programming official documentation",
        )
        .await
        .unwrap();
        assert!(!rows.is_empty());
        assert!(rows
            .iter()
            .all(|row| !row.providers.iter().any(|provider| provider == "DeepSeek")));
        println!("{}", serde_json::to_string(&rows).unwrap());
    }

    #[tokio::test]
    #[ignore = "opt-in paid official DeepSeek API; reads DEEPSEEK_API_KEY only in process"]
    async fn hybrid_live_official_deepseek() {
        let api_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY is required");
        let rows = super::super::search(
            &WebSearchConfig {
                provider: SearchProvider::Hybrid,
                api_key,
                ..Default::default()
            },
            "Rust programming official documentation",
        )
        .await
        .unwrap();
        assert!(
            rows.iter()
                .any(|row| row.providers.iter().any(|provider| provider == "DeepSeek")),
            "free fallback alone does not verify DeepSeek success"
        );
        println!(
            "Official Search API: $0.75/1,000 nonempty successful requests (2026-11-23); {}",
            serde_json::to_string(&rows).unwrap()
        );
    }

    #[tokio::test]
    async fn http_source_failure_does_not_hide_other_sources() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 8192];
                let count = socket.read(&mut bytes).await.unwrap();
                let body = if String::from_utf8_lossy(&bytes[..count]).starts_with("GET /blocked ")
                {
                    "<html>CAPTCHA</html>"
                } else {
                    r#"{"results":[{"title":"Docs","url":"https://doc.rust-lang.org/","snippet":"Rust"}]}"#
                };
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let client = reqwest::Client::new();
        let replies = join_all([Source::FreeSerp, Source::Keenable].map(|source| {
            source.query(
                &client,
                client
                    .get(format!(
                        "{endpoint}/{}",
                        if source == Source::FreeSerp {
                            "blocked"
                        } else {
                            "valid"
                        }
                    ))
                    .build()
                    .unwrap(),
            )
        }))
        .await;
        let rows = merge(replies).unwrap();
        assert_eq!(rows[0].url, "https://doc.rust-lang.org/");
        assert_eq!(rows[0].providers, ["Keenable"]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn timed_out_source_preserves_other_results() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 8192];
            assert!(socket.read(&mut bytes).await.unwrap() > 0);
            // The client must close its pending response when the source times out.
            assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
        });
        let client = reqwest::Client::new();
        let slow = Source::FreeSerp
            .query(&client, client.get(endpoint).build().unwrap())
            .await;
        assert!(slow.as_ref().unwrap_err().contains("timed out"));
        let fast = Source::Keenable
            .parse(br#"{"results":[{"title":"Rust","url":"https://doc.rust-lang.org/"}]}"#)
            .map_err(String::from)
            .unwrap();
        assert_eq!(
            merge(vec![slow, Err("DeepSeek: HTTP 402".into()), Ok(fast)])
                .unwrap()
                .len(),
            1
        );
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
    }
}
