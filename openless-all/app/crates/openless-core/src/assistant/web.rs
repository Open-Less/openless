//! Shared, read-only public web capabilities. These can also be called by explicit voice
//! questions without putting ordinary dictation through the assistant loop.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::ask::{AskCancellation, AskReadStatus, AskSource};
use crate::{BackendError, BackendErrorCode};

pub struct PublicEvidence {
    pub source: AskSource,
    pub evidence: Value,
}

pub async fn read_public_page(
    url: &str,
    cancellation: AskCancellation,
) -> Result<PublicEvidence, BackendError> {
    super::check_cancelled(&cancellation)?;
    let page = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(super::cancelled()),
        page = crate::web_search::fetch_public_page(url, crate::web_search::RequestPolicy::Ask, || !cancellation.is_cancelled()) => page,
    }.map_err(|error| match error {
        crate::web_search::PageReadError::Blocked => super::error(BackendErrorCode::PermissionDenied, "askPageBlocked"),
        crate::web_search::PageReadError::Failed => super::error(BackendErrorCode::Provider, "askPageReadFailed"),
    })?;
    super::check_cancelled(&cancellation)?;
    let source = source(url.to_owned(), url.to_owned());
    Ok(PublicEvidence {
        evidence: json!({"status":"available","source":source,"retrievedAt":chrono::Utc::now().to_rfc3339(),"excerpt":page}),
        source,
    })
}

pub(crate) fn release_url(repository: &str, tag: Option<&str>) -> Result<url::Url, BackendError> {
    let parts: Vec<_> = repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 100
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
        || tag.is_some_and(|tag| {
            tag.is_empty() || tag.len() > 200 || tag.chars().any(char::is_control)
        })
    {
        return Err(super::error(
            BackendErrorCode::InvalidArgument,
            "askReleaseInvalidRepository",
        ));
    }
    let mut url = url::Url::parse("https://api.github.com").expect("static GitHub URL");
    {
        let mut path = url
            .path_segments_mut()
            .expect("HTTPS URL has path segments");
        path.extend(["repos", parts[0], parts[1], "releases"]);
        if let Some(tag) = tag {
            path.extend(["tags", tag]);
        } else {
            path.push("latest");
        }
    }
    Ok(url)
}

pub async fn github_release(
    repository: &str,
    tag: Option<&str>,
    cancellation: AskCancellation,
) -> Result<PublicEvidence, BackendError> {
    let url = release_url(repository, tag)?;
    super::check_cancelled(&cancellation)?;
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(super::cancelled()),
        result = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            // The host is fixed and redirects are refused; no model-controlled endpoint,
            // ambient cookies or credentials are sent, and no asset bodies are fetched.
            let response = crate::net::anonymous_no_redirect_http().get(url.clone())
                .header(reqwest::header::USER_AGENT, "OpenLess-Assistant")
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .send().await.map_err(|_| super::error(BackendErrorCode::Provider, "askReleaseNetworkError"))?;
            release_response(response).await
        }) => result.map_err(|_| super::error(BackendErrorCode::Provider, "askToolTimeout"))?,
    }?;
    super::check_cancelled(&cancellation)?;
    release_evidence(result, url.as_str())
}

async fn release_response(mut response: reqwest::Response) -> Result<Value, BackendError> {
    if !response.status().is_success() {
        return Err(super::error(
            BackendErrorCode::Provider,
            match response.status().as_u16() {
                404 => "askReleaseNotFound",
                403 | 429 => "askReleaseRateLimited",
                _ => "askReleaseRequestFailed",
            },
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| super::error(BackendErrorCode::Provider, "askReleaseNetworkError"))?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(super::error(
                BackendErrorCode::Provider,
                "askReleaseTooLarge",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| super::error(BackendErrorCode::Provider, "askReleaseInvalidResponse"))
}

fn release_evidence(value: Value, api_url: &str) -> Result<PublicEvidence, BackendError> {
    let url = value["html_url"]
        .as_str()
        .filter(|url| github_link(url))
        .ok_or_else(|| super::error(BackendErrorCode::Provider, "askReleaseInvalidResponse"))?;
    let tag = value["tag_name"]
        .as_str()
        .ok_or_else(|| super::error(BackendErrorCode::Provider, "askReleaseInvalidResponse"))?;
    let assets = value["assets"]
        .as_array()
        .ok_or_else(|| super::error(BackendErrorCode::Provider, "askReleaseInvalidResponse"))?;
    let projected: Vec<_> = assets.iter().take(100).filter_map(|asset| {
        let url = asset["browser_download_url"].as_str().filter(|url| github_link(url))?;
        Some(json!({"name":asset["name"],"url":url,"size":asset["size"],"contentType":asset["content_type"],"updatedAt":asset["updated_at"]}))
    }).collect();
    let source = source(
        url.to_owned(),
        value["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(tag)
            .to_owned(),
    );
    Ok(PublicEvidence {
        evidence: json!({
            "status":"available","source":source,"apiUrl":api_url,
            "retrievedAt":chrono::Utc::now().to_rfc3339(),"tag":tag,"prerelease":value["prerelease"],
            "publishedAt":value["published_at"],"body":value["body"].as_str().unwrap_or_default().chars().take(6000).collect::<String>(),
            "assets":projected,"assetsTruncated":projected.len() != assets.len(),
        }),
        source,
    })
}

fn github_link(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
    })
}

fn source(url: String, title: String) -> AskSource {
    AskSource {
        source_id: Uuid::new_v4().to_string(),
        url,
        title,
        snippet: None,
        read_status: AskReadStatus::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_preserve_exact_asset_links_and_publication_time() {
        let result = release_evidence(json!({
            "html_url":"https://github.com/Open-Less/openless/releases/tag/v1.2", "tag_name":"v1.2",
            "published_at":"2026-11-21T08:00:00Z", "assets":[{
                "name":"OpenLess_aarch64.dmg", "browser_download_url":"https://github.com/Open-Less/openless/releases/download/v1.2/OpenLess_aarch64.dmg", "size":12345
            }]
        }), "https://api.github.com/repos/Open-Less/openless/releases/latest").unwrap();
        assert_eq!(
            result.evidence["assets"][0]["url"],
            "https://github.com/Open-Less/openless/releases/download/v1.2/OpenLess_aarch64.dmg"
        );
        assert_eq!(result.evidence["publishedAt"], "2026-11-21T08:00:00Z");
        assert_eq!(result.source.read_status, AskReadStatus::Read);
        assert!(result.evidence["retrievedAt"].as_str().is_some());
        assert!(release_url("../../etc", None).is_err());
        assert_eq!(
            release_url("Open-Less/openless", Some("v1/rc1"))
                .unwrap()
                .path(),
            "/repos/Open-Less/openless/releases/tags/v1%2Frc1"
        );
    }

    #[tokio::test]
    async fn private_pages_are_blocked_before_network_access() {
        for url in [
            "file:///etc/passwd",
            "http://127.0.0.1/",
            "http://[::1]/",
            "https://user@example.com/",
        ] {
            assert_eq!(
                read_public_page(url, AskCancellation::default())
                    .await
                    .err()
                    .unwrap()
                    .code,
                BackendErrorCode::PermissionDenied
            );
        }
    }

    #[tokio::test]
    #[ignore = "explicit live public-network acceptance"]
    async fn live_public_page_has_readable_text_and_traceable_source() {
        let result = read_public_page("https://example.com/", AskCancellation::default())
            .await
            .unwrap();
        assert_eq!(result.source.url, "https://example.com/");
        assert_eq!(result.source.read_status, AskReadStatus::Read);
        assert!(result.evidence["excerpt"]
            .as_str()
            .unwrap()
            .contains("Example Domain"));
    }

    #[tokio::test]
    #[ignore = "explicit live public-network acceptance"]
    async fn live_github_release_has_traceable_assets() {
        let result = github_release("Open-Less/openless", None, AskCancellation::default())
            .await
            .unwrap();
        assert!(result
            .source
            .url
            .starts_with("https://github.com/Open-Less/openless/releases/"));
        assert!(!result.evidence["assets"].as_array().unwrap().is_empty());
    }
}
