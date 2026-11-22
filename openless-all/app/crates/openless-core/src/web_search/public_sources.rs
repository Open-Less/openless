use std::collections::HashSet;
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
        parse_results(&body["results"], &["snippet", "summary"])
            .map_err(|_| "Invalid search results".into())
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
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    // Interleave by rank so one provider cannot occupy the whole result budget.
    loop {
        let mut advanced = false;
        for items in &mut remaining {
            if let Some(item) = items.next() {
                advanced = true;
                let key = item.url.split('#').next().unwrap_or(&item.url);
                if seen.insert(key.to_owned()) {
                    results.push(item);
                    if results.len() == MAX_RESULTS {
                        return Ok(results);
                    }
                }
            }
        }
        if !advanced {
            return Ok(results);
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
        let rows = query(
            &WebSearchConfig::default(),
            "Rust language documentation",
            || true,
        )
        .await
        .unwrap();
        assert!(!rows.is_empty() && rows.len() <= MAX_RESULTS);
    }
}
