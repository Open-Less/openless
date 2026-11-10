//! Adapted from PR #1399: prefer the Aliyun binary, retain the same signed bytes on GitHub.
use std::time::Duration;
const BUDGET: Duration = Duration::from_secs(8);

fn decode(value: &str) -> Option<String> {
    super::android::decode_asset_name(value)
}
fn segment(value: &str) -> bool {
    !value.is_empty() && !value.contains("..") && !value.contains(['/', '\\', '\0'])
}
fn encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
fn parts(raw: &str, github: bool) -> Option<(String, String)> {
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let prefix = if github {
        if url.host_str()? != "github.com" {
            return None;
        }
        "/Open-Less/openless/releases/download/"
    } else {
        if url.host_str()? != "dl.aeoform.com" {
            return None;
        }
        "/releases/"
    };
    let mut path = url.path().strip_prefix(prefix)?.split('/');
    let tag = decode(path.next()?)?;
    let asset = decode(path.next()?)?;
    if path.next().is_some() || !segment(&tag) || !segment(&asset) {
        return None;
    }
    Some((tag, asset))
}
pub(crate) fn github_fallback_for_cdn_artifact(raw: &str) -> Option<String> {
    let (tag, asset) = parts(raw, false)?;
    Some(format!(
        "https://github.com/Open-Less/openless/releases/download/{}/{}",
        encode(&tag),
        encode(&asset)
    ))
}
fn aliyun_for_github(raw: &str) -> Option<String> {
    let (tag, asset) = parts(raw, true)?;
    Some(format!(
        "https://dl.aeoform.com/releases/{}/{}",
        encode(&tag),
        encode(&asset)
    ))
}
/// A failed/forbidden HEAD is confirmed with a bounded range request. Redirects
/// use the updater's existing origin allowlist; neither probe buffers the payload.
async fn available(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    let client = crate::net::anonymous_no_redirect_http();
    let status = match client.head(url.clone()).timeout(BUDGET).send().await {
        Ok(response) => response.status(),
        Err(_) => return false,
    };
    if status.is_success() {
        return true;
    }
    if !status.is_redirection()
        && status != reqwest::StatusCode::FORBIDDEN
        && status != reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        return false;
    }
    tokio::time::timeout(BUDGET, super::android::probe_android_download(raw))
        .await
        .ok()
        .flatten()
        .is_some()
}
pub(crate) async fn resolve_artifact_download_url(raw: &str) -> String {
    let cdn = if parts(raw, false).is_some() {
        raw.to_string()
    } else if let Some(cdn) = aliyun_for_github(raw) {
        cdn
    } else {
        return raw.to_string();
    };
    if available(&cdn).await {
        cdn
    } else {
        github_fallback_for_cdn_artifact(&cdn).unwrap_or_else(|| raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_asset_mapping_and_rejection() {
        let cdn =
            "https://dl.aeoform.com/releases/v2-Beta.1-tauri/OpenLess_2%2Bbuild_x64-setup.exe";
        let github = github_fallback_for_cdn_artifact(cdn).unwrap();
        assert_eq!(aliyun_for_github(&github).unwrap(), cdn);
        assert_eq!(
            github_fallback_for_cdn_artifact(&cdn.replace("%2B", "+")).unwrap(),
            github
        );
        for invalid in [
            "http://dl.aeoform.com/releases/v2/a",
            "https://dl.aeoform.com.attacker.invalid/releases/v2/a",
            "https://u@dl.aeoform.com/releases/v2/a",
            "https://dl.aeoform.com/releases/v2/a%2Fb",
            "https://dl.aeoform.com/releases/v2/a?x=1",
            "https://dl.aeoform.com/releases/v2/a/b",
            "https://dl.aeoform.com/releases/v2/%2e%2e",
            "https://dl.aeoform.com/updates/beta/latest.json",
        ] {
            assert!(
                github_fallback_for_cdn_artifact(invalid).is_none(),
                "{invalid}"
            );
        }
    }
}
