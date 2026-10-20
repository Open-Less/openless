//! Optional cloud candidates, using the GoogleCN protocol used by Fcitx5.
//! Callers enforce editor privacy and discard replies for obsolete compositions.
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

pub fn valid_query(query: &str) -> bool {
    (2..=64).contains(&query.len())
        && query.bytes().all(|c| c.is_ascii_lowercase() || c == b'\'')
        && query.bytes().any(|c| c.is_ascii_lowercase())
}

fn candidates(value: &Value, query: &str) -> Vec<String> {
    if value[0] != "SUCCESS" || value[1][0][0] != query {
        return vec![];
    }
    let mut result = Vec::new();
    if let Some(rows) = value[1][0][1].as_array() {
        for text in rows.iter().filter_map(Value::as_str) {
            if !text.is_empty()
                && text.chars().count() <= 128
                && !text.chars().any(char::is_control)
                && !result.iter().any(|item| item == text)
            {
                result.push(text.to_owned());
                if result.len() == 5 {
                    break;
                }
            }
        }
    }
    result
}

pub async fn query(query: &str) -> Vec<String> {
    if !valid_query(query) {
        return vec![];
    }
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    let Some(client) = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_millis(1200))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()
    }) else {
        return vec![];
    };
    // Never include URLs, queries or server response text in diagnostics.
    let Ok(mut response) = client
        .get("https://www.google.cn/inputtools/request")
        .query(&[("ime", "pinyin"), ("text", query)])
        .send()
        .await
    else {
        return vec![];
    };
    if !response.status().is_success() {
        return vec![];
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= 65536 => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(None) => break,
            _ => return vec![],
        }
    }
    serde_json::from_slice(&bytes)
        .map(|value| candidates(&value, query))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    #[ignore = "explicit live provider smoke test; never sends user input"]
    async fn live_known_pinyin() {
        assert!(query("nihao").await.iter().any(|text| text == "你好"));
    }
    #[test]
    fn query_boundary() {
        assert!(valid_query("ni'hao"));
        for invalid in ["a", "hello world", "秘密", "https://x", "\nnihao"] {
            assert!(!valid_query(invalid));
        }
        assert!(!valid_query(&"a".repeat(65)));
    }
    #[test]
    fn validate_response_identity_and_candidates() {
        let reply = json!([
            "SUCCESS",
            [["nihao", ["你好", "你好", "你号", "bad\ntext"]]]
        ]);
        assert_eq!(candidates(&reply, "nihao"), vec!["你好", "你号"]);
        assert!(candidates(&reply, "nihaoma").is_empty());
        assert!(candidates(&json!(["ERROR"]), "nihao").is_empty());
    }
}
