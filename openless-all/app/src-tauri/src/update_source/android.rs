//! Android GitHub discovery uses official versioned APKs and their detached signatures.
use crate::types::UpdateChannel;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type KnownCandidate = Arc<Mutex<Option<String>>>;
use std::time::Duration;
use url::Url;

const DISCOVERY_BUDGET: Duration = Duration::from_secs(24);
const METADATA_REQUEST_BUDGET: Duration = Duration::from_secs(8);
const PROBE_BUDGET: Duration = Duration::from_secs(4);
const METADATA_LIMIT: usize = 2 * 1024 * 1024;
const API_METADATA_LIMIT: usize = 8 * 1024 * 1024;
const SIGNATURE_LIMIT: usize = super::signature_index::LIMIT;

fn github_url(url: &Url) -> bool {
    url.host_str() == Some("github.com")
        && url
            .path()
            .starts_with("/Open-Less/openless/releases/download/")
}

fn legacy_github_mirror_url(url: &Url) -> bool {
    url.host_str() == Some("fastgit.cc")
        && url
            .path()
            .starts_with("/https://github.com/Open-Less/openless/releases/")
}

pub(crate) fn allowed_update_download_url(url: &Url) -> bool {
    safe_https(url)
        && (url.host_str() == Some("github.com")
            && url.path().starts_with("/Open-Less/openless/releases/")
            || legacy_github_mirror_url(url)
            || url.host_str() == Some("dl.aeoform.com") && url.path().starts_with("/releases/"))
}

fn safe_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
}

fn allowed_request(url: &Url, redirected: bool) -> bool {
    if !safe_https(url) {
        return false;
    }
    match url.host_str() {
        Some("github.com") => url.path().starts_with("/Open-Less/openless/releases"),
        Some("api.github.com") => url.path().starts_with("/repos/Open-Less/openless/releases"),
        Some("fastgit.cc") => legacy_github_mirror_url(url),
        Some("dl.aeoform.com") => {
            url.path().starts_with("/releases/") || url.path().starts_with("/updates/")
        }
        Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com") => {
            redirected
        }
        _ => false,
    }
}

/// Preserve the shared proxy/TLS policy while checking every redirect before sending it.
async fn update_get(
    mut url: Url,
    budget: Duration,
    range: bool,
) -> Result<reqwest::Response, String> {
    tokio::time::timeout(budget, async {
        for hop in 0..=5 {
            if !allowed_request(&url, hop > 0) {
                return Err("update URL is not an allowed HTTPS origin".into());
            }
            let mut request = crate::net::anonymous_no_redirect_http()
                .get(url.clone())
                .header(reqwest::header::USER_AGENT, "OpenLess-Updater")
                .timeout(budget);
            if range {
                request = request.header(reqwest::header::RANGE, "bytes=0-65535");
            }
            let response = request.send().await.map_err(|e| e.to_string())?;
            if response.status().is_redirection() {
                let next = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or("redirect missing location")?;
                url = url.join(next).map_err(|e| e.to_string())?;
            } else {
                return if response.status().is_success() {
                    Ok(response)
                } else {
                    Err(format!("update HTTP status {}", response.status().as_u16()))
                };
            }
        }
        Err("too many update redirects".into())
    })
    .await
    .map_err(|_| "update request timed out".to_string())?
}

pub(crate) async fn bounded_update_get(
    url: Url,
    budget: Duration,
) -> Result<reqwest::Response, String> {
    update_get(url, budget, false).await
}

async fn bounded_text(response: reqwest::Response, limit: usize) -> Result<String, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("update metadata exceeds size limit".into());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("update metadata exceeds size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|e| e.to_string())
}

async fn metadata_response_text(
    response: reqwest::Response,
    limit: usize,
    budget: Duration,
) -> Result<String, String> {
    tokio::time::timeout(budget, bounded_text(response, limit))
        .await
        .map_err(|_| "update metadata body timed out".to_string())?
}

async fn text(url: &str, limit: usize) -> Result<String, String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    tokio::time::timeout(METADATA_REQUEST_BUDGET, async {
        metadata_response_text(
            bounded_update_get(url, METADATA_REQUEST_BUDGET).await?,
            limit,
            METADATA_REQUEST_BUDGET,
        )
        .await
    })
    .await
    .map_err(|_| "update metadata timed out".to_string())?
}

pub(crate) async fn probe_android_download(url: &str) -> Option<(f64, f64)> {
    let url = Url::parse(url).ok()?;
    if !allowed_update_download_url(&url) {
        return None;
    }
    tokio::time::timeout(PROBE_BUDGET, async {
        let started = std::time::Instant::now();
        let response = update_get(url, PROBE_BUDGET, true).await.ok()?;
        let transfer = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        let first = stream.next().await?.ok()?;
        let latency = started.elapsed().as_secs_f64();
        let mut bytes = first.len().min(65536);
        while bytes < 65536 {
            match stream.next().await {
                Some(Ok(chunk)) => bytes = bytes.saturating_add(chunk.len()).min(65536),
                Some(Err(_)) => return None,
                None => break,
            }
        }
        (bytes > 0).then(|| {
            (
                latency,
                bytes as f64 / transfer.elapsed().as_secs_f64().max(0.001),
            )
        })
    })
    .await
    .ok()
    .flatten()
}

fn abi(arch: &str) -> Option<&'static str> {
    match arch {
        "aarch64" => Some("arm64-v8a"),
        "armv7" => Some("armeabi-v7a"),
        "i686" => Some("x86"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
}

fn artifact_version(name: &str, arch: &str) -> Option<semver::Version> {
    let suffix = format!("_{}.apk", abi(arch)?);
    let value = name.strip_prefix("OpenLess_")?.strip_suffix(&suffix)?;
    semver::Version::parse(value).ok()
}

fn tag_version(tag: &str, channel: UpdateChannel) -> Option<semver::Version> {
    let version = semver::Version::parse(tag.strip_prefix('v')?.strip_suffix("-tauri")?).ok()?;
    let matches = match channel {
        UpdateChannel::Stable => version.pre.is_empty(),
        UpdateChannel::Beta => version
            .pre
            .as_str()
            .to_ascii_lowercase()
            .starts_with("beta."),
    };
    matches.then_some(version)
}

/// Filename metadata is authenticated only when the APK's minisign verification succeeds.
pub(crate) fn android_signature_version(
    signature: &str,
    artifact_name: &str,
    tag: &str,
    arch: &str,
) -> Result<String, String> {
    let decoded = crate::android::updater_logic::decode_tauri_minisign_text(signature)?;
    let trusted = decoded
        .lines()
        .find_map(|line| line.strip_prefix("trusted comment: "))
        .ok_or("missing APK trusted comment")?;
    let signed_file = trusted
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"))
        .ok_or("missing signed APK filename")?;
    if signed_file != artifact_name {
        return Err("signed APK filename differs from release asset".into());
    }
    let signed = artifact_version(signed_file, arch).ok_or("invalid signed APK version or ABI")?;
    let tag = semver::Version::parse(tag).map_err(|e| e.to_string())?;
    if signed.major != tag.major
        || signed.minor != tag.minor
        || signed.patch != tag.patch
        || signed.pre != tag.pre
    {
        return Err("signed APK version differs from release tag".into());
    }
    Ok(signed.to_string())
}

fn ordered_releases(value: &Value, channel: UpdateChannel) -> Vec<&Value> {
    let values: Vec<&Value> = match value.as_array() {
        Some(values) => values.iter().collect(),
        None => vec![value],
    };
    let mut releases: Vec<_> = values
        .into_iter()
        .filter_map(|release| {
            if release["draft"] != false
                || release["prerelease"] != matches!(channel, UpdateChannel::Beta)
            {
                return None;
            }
            let version = tag_version(release["tag_name"].as_str()?, channel)?;
            Some((version, release))
        })
        .collect();
    releases.sort_by(|a, b| b.0.cmp_precedence(&a.0));
    releases.into_iter().map(|(_, release)| release).collect()
}

#[cfg(test)]
fn latest_release(value: &Value, channel: UpdateChannel) -> Option<&Value> {
    ordered_releases(value, channel).into_iter().next()
}

fn latest_android_release<'a>(
    value: &'a Value,
    channel: UpdateChannel,
    arch: &str,
) -> Option<&'a Value> {
    ordered_releases(value, channel)
        .into_iter()
        .find(|release| api_artifact(release, arch).is_some())
}

pub(super) fn decode_asset_name(encoded: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut chars = encoded.as_bytes().iter().copied();
    while let Some(ch) = chars.next() {
        bytes.push(if ch == b'%' {
            let high = (chars.next()? as char).to_digit(16)?;
            let low = (chars.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            ch
        });
    }
    let result = String::from_utf8(bytes).ok()?;
    (!result.contains(['/', '\\', '\0'])).then_some(result)
}

fn api_artifact<'a>(
    release: &'a Value,
    arch: &str,
) -> Option<(&'a str, &'a str, &'a str, Option<u64>)> {
    let tag = release["tag_name"].as_str()?;
    let assets = release["assets"].as_array()?;
    let artifact = assets
        .iter()
        .filter_map(|a| Some((artifact_version(a["name"].as_str()?, arch)?, a)))
        .max_by(|a, b| a.0.cmp(&b.0))?
        .1;
    let name = artifact["name"].as_str()?;
    let signature = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(super::signature_index::NAME))
        .or_else(|| assets.iter().find(|a| a["name"].as_str() == Some(&format!("{name}.sig"))))?;
    let artifact_url = artifact["browser_download_url"].as_str()?;
    let signature_url = signature["browser_download_url"].as_str()?;
    let expected = format!("/Open-Less/openless/releases/download/{tag}/");
    for (value, filename) in [
        (artifact_url, name),
        (signature_url, signature["name"].as_str()?),
    ] {
        let url = Url::parse(value).ok()?;
        if !safe_https(&url) || !github_url(&url) || !url.path().starts_with(&expected) {
            return None;
        }
        let decoded = decode_asset_name(url.path().strip_prefix(&expected)?)?;
        if decoded != filename {
            return None;
        }
    }
    Some((name, artifact_url, signature_url, artifact["size"].as_u64()))
}

enum ApiDiscoveryError {
    Unavailable(String),
    Candidate(String),
}
impl From<String> for ApiDiscoveryError {
    fn from(value: String) -> Self {
        Self::Unavailable(value)
    }
}
impl From<&str> for ApiDiscoveryError {
    fn from(value: &str) -> Self {
        Self::Unavailable(value.to_string())
    }
}

async fn api_manifest(
    channel: UpdateChannel,
    arch: &str,
    candidate_seen: &KnownCandidate,
) -> Result<Value, ApiDiscoveryError> {
    let endpoint = match channel {
        UpdateChannel::Stable => "https://api.github.com/repos/Open-Less/openless/releases/latest",
        UpdateChannel::Beta => {
            "https://api.github.com/repos/Open-Less/openless/releases?per_page=30"
        }
    };
    let mut value: Value = serde_json::from_str(&text(endpoint, API_METADATA_LIMIT).await?)
        .map_err(|e| e.to_string())?;
    if matches!(channel, UpdateChannel::Stable)
        && latest_android_release(&value, channel, arch).is_none()
    {
        value = serde_json::from_str(
            &text(
                "https://api.github.com/repos/Open-Less/openless/releases?per_page=30",
                API_METADATA_LIMIT,
            )
            .await?,
        )
        .map_err(|e| e.to_string())?;
    }
    let release = latest_android_release(&value, channel, arch)
        .ok_or("no published signed APK for device ABI")?;
    let tag = release["tag_name"].as_str().ok_or("missing release tag")?;
    let version = tag_version(tag, channel).ok_or("invalid release tag")?;
    let (name, url, sig_url, size) =
        api_artifact(release, arch).ok_or("no versioned APK and signature for device ABI")?;
    let full_version = artifact_version(name, arch)
        .ok_or("invalid APK asset version")?
        .to_string();
    *candidate_seen
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(full_version.clone());
    let body = text(sig_url, SIGNATURE_LIMIT)
        .await
        .map_err(|error| ApiDiscoveryError::Candidate(candidate_error(&full_version, &error)))?;
    let signature = super::signature_index::extract(&body, tag, name, sig_url.ends_with(super::signature_index::NAME))
        .map_err(|error| ApiDiscoveryError::Candidate(candidate_error(&full_version, &error)))?;
    let version = android_signature_version(signature.trim(), name, &version.to_string(), arch)
        .map_err(|error| ApiDiscoveryError::Candidate(candidate_error(&full_version, &error)))?;
    Ok(
        json!({"version":version,"url":url,"signature":signature.trim(),"pub_date":release["published_at"],"notes":release["body"],"size":size}),
    )
}

fn web_artifact(page: &str, tag: &str, arch: &str) -> Option<(String, String, String)> {
    let prefix = format!("/Open-Less/openless/releases/download/{tag}/");
    let paths: Vec<_> = page
        .split("href=\"")
        .skip(1)
        .filter_map(|p| {
            let path = p.split('"').next()?;
            let name = decode_asset_name(path.strip_prefix(&prefix)?)?;
            (!name.contains('/')).then_some((path, name))
        })
        .collect();
    let (path, name) = paths
        .iter()
        .filter_map(|(p, n)| Some((artifact_version(n, arch)?, p, n)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, p, n)| (p, n))?;
    let signature = paths.iter().find(|(_, n)| *n == super::signature_index::NAME)
        .or_else(|| paths.iter().find(|(_, n)| *n == format!("{name}.sig")))?.0;
    Some((
        name.clone(),
        format!("https://github.com{path}"),
        format!("https://github.com{signature}"),
    ))
}

async fn web_manifest_from_feed<F, Fut>(
    feed: &str,
    channel: UpdateChannel,
    arch: &str,
    candidate_seen: &KnownCandidate,
    mut fetch: F,
) -> Result<Value, String>
where
    F: FnMut(String, usize) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    for tag in super::github_feed_tags(feed, channel).into_iter().take(3) {
        let version = tag_version(tag, channel).ok_or("invalid public release tag")?;
        let page = match fetch(
            format!("https://github.com/Open-Less/openless/releases/expanded_assets/{tag}"),
            METADATA_LIMIT,
        )
        .await
        {
            Ok(page) => page,
            Err(error) if error == "update HTTP status 404" => continue,
            Err(error) => return Err(format!("public release assets: {error}")),
        };
        let Some((name, url, sig_url)) = web_artifact(&page, tag, arch) else {
            continue;
        };
        // Once a matching pair exists, a transport or metadata failure cannot downgrade it.
        let full_version = artifact_version(&name, arch)
            .ok_or("invalid APK asset version")?
            .to_string();
        *candidate_seen
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(full_version.clone());
        let indexed = sig_url.ends_with(super::signature_index::NAME);
        let body = fetch(sig_url, SIGNATURE_LIMIT).await.map_err(|error| {
            candidate_error(&full_version, &format!("public signature: {error}"))
        })?;
        let signature = super::signature_index::extract(&body, tag, &name, indexed)
            .map_err(|error| candidate_error(&full_version, &error))?;
        let version =
            android_signature_version(signature.trim(), &name, &version.to_string(), arch)
                .map_err(|error| candidate_error(&full_version, &error))?;
        return Ok(
            json!({"version":version,"url":url,"signature":signature.trim(),"notes":format!("https://github.com/Open-Less/openless/releases/tag/{tag}")}),
        );
    }
    Err("public releases have no matching signed APK in the newest channel tags".into())
}

async fn web_manifest(
    channel: UpdateChannel,
    arch: &str,
    candidate_seen: &KnownCandidate,
) -> Result<Value, String> {
    let feed = text(
        "https://github.com/Open-Less/openless/releases.atom",
        METADATA_LIMIT,
    )
    .await
    .map_err(|e| format!("public feed: {e}"))?;
    web_manifest_from_feed(
        &feed,
        channel,
        arch,
        candidate_seen,
        |url, limit| async move { text(&url, limit).await },
    )
    .await
}

fn candidate_error(version: &str, reason: &str) -> String {
    format!("GitHub release candidate: {version}: {reason}")
}

fn combine_discovery_errors(api: String, web: String) -> String {
    if web.starts_with("GitHub release candidate:") {
        web
    } else {
        format!("GitHub API: {api}; public releases: {web}")
    }
}

async fn discovery_with_deadline<F>(
    budget: Duration,
    candidate_seen: &KnownCandidate,
    discovery: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    tokio::time::timeout(budget, discovery).await.map_err(|_| {
        if let Some(version) = candidate_seen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            candidate_error(version, "discovery timed out")
        } else {
            "GitHub Android discovery timed out".to_string()
        }
    })?
}

pub(crate) async fn github_android_manifest(
    channel: UpdateChannel,
    arch: &str,
) -> Result<Value, String> {
    if abi(arch).is_none() {
        return Err("unsupported Android updater architecture".into());
    }
    let candidate_seen: KnownCandidate = Arc::new(Mutex::new(None));
    discovery_with_deadline(DISCOVERY_BUDGET, &candidate_seen, async {
        match api_manifest(channel, arch, &candidate_seen).await {
            Ok(value) => Ok(value),
            Err(ApiDiscoveryError::Candidate(error)) => Err(error),
            Err(ApiDiscoveryError::Unavailable(api)) => {
                web_manifest(channel, arch, &candidate_seen)
                    .await
                    .map_err(|web| combine_discovery_errors(api, web))
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signature(name: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(format!("untrusted comment: fixture\nnot-a-valid-signature\ntrusted comment: timestamp:1\tfile:{name}\nnot-a-valid-global-signature"))
    }
    #[test]
    fn all_abis_use_exact_versioned_artifacts_and_matching_signature_names() {
        let tag = "v2.0.2-Beta.6-tauri";
        for (arch, abi) in [
            ("aarch64", "arm64-v8a"),
            ("armv7", "armeabi-v7a"),
            ("i686", "x86"),
            ("x86_64", "x86_64"),
        ] {
            let name = format!("OpenLess_2.0.2-Beta.6+build.20261109-test_{abi}.apk");
            let release = json!({"tag_name":tag,"assets":[
                {"name":name,"browser_download_url":format!("https://github.com/Open-Less/openless/releases/download/{tag}/{name}"),"size":20},
                {"name":format!("{name}.sig"),"browser_download_url":format!("https://github.com/Open-Less/openless/releases/download/{tag}/{name}.sig")}
            ]});
            let (actual, _, _, size) = api_artifact(&release, arch).unwrap();
            assert_eq!(actual, name);
            assert_eq!(size, Some(20));
            assert_eq!(
                android_signature_version(&signature(&name), &name, "2.0.2-Beta.6", arch).unwrap(),
                "2.0.2-Beta.6+build.20261109-test"
            );
            assert!(
                android_signature_version(&signature(&name), &name, "2.0.2-Beta.5", arch).is_err()
            );
            assert!(android_signature_version(
                &signature("wrong.apk"),
                &name,
                "2.0.2-Beta.6",
                arch
            )
            .is_err());
            let page=format!("<a href=\"/Open-Less/openless/releases/download/{tag}/{name}\"><a href=\"/Open-Less/openless/releases/download/{tag}/{name}.sig\">");
            assert_eq!(web_artifact(&page, tag, arch).unwrap().0, name);
            assert!(web_artifact(&page.replace(".apk.sig", ".apk.unsigned"), tag, arch).is_none());
        }
        assert!(artifact_version("OpenLess_2.0.2-Beta.6_x86_64.apk", "i686").is_none());
        assert!(artifact_version("OpenLess_android_aarch64.apk", "aarch64").is_none());
    }
    #[test]
    fn discovery_excludes_drafts_other_channels_and_uses_semver() {
        let releases = json!([
            {"tag_name":"v2.0.2-Beta.9-tauri","draft":false,"prerelease":true},
            {"tag_name":"v2.0.2-Beta.10-tauri","draft":false,"prerelease":true},
            {"tag_name":"v3.0.0-Beta.1-tauri","draft":true,"prerelease":true},
            {"tag_name":"v9.0.0-RC.1-tauri","draft":false,"prerelease":true},
            {"tag_name":"v4.0.0-tauri","draft":false,"prerelease":false}
        ]);
        assert_eq!(
            latest_release(&releases, UpdateChannel::Beta).unwrap()["tag_name"],
            "v2.0.2-Beta.10-tauri"
        );
        assert_eq!(
            latest_release(&releases, UpdateChannel::Stable).unwrap()["tag_name"],
            "v4.0.0-tauri"
        );
        assert!(latest_release(
            &json!({"tag_name":"v3.0.0-tauri","draft":true,"prerelease":false}),
            UpdateChannel::Stable
        )
        .is_none());
    }
    #[test]
    fn request_origins_are_fixed_and_redirects_cannot_escape_https() {
        for value in [
            "http://github.com/Open-Less/openless/releases/download/tag/file.apk",
            "https://attacker.invalid/releases/file.apk",
            "https://user:password@github.com/Open-Less/openless/releases/download/tag/file.apk",
            "https://github.com:444/Open-Less/openless/releases/download/tag/file.apk",
            "https://github.com/other/repo/releases/download/tag/file.apk",
        ] {
            let url = Url::parse(value).unwrap();
            assert!(!allowed_update_download_url(&url));
            assert!(!allowed_request(&url, true));
        }
        assert!(allowed_update_download_url(
            &Url::parse("https://dl.aeoform.com/releases/tag/app.apk").unwrap()
        ));
        let redirect =
            Url::parse("https://release-assets.githubusercontent.com/path?sig=fixture").unwrap();
        assert!(!allowed_request(&redirect, false));
        assert!(allowed_request(&redirect, true));
        assert!(decode_asset_name("OpenLess_2.0.2%2Bbuild_x86.apk")
            .unwrap()
            .contains('+'));
        assert!(decode_asset_name("%2fetc%2ffile.apk").is_none());
    }
    #[tokio::test]
    async fn unsupported_architecture_and_untrusted_probe_fail_without_network() {
        assert!(github_android_manifest(UpdateChannel::Beta, "mips")
            .await
            .is_err());
        assert!(probe_android_download("http://127.0.0.1/private")
            .await
            .is_none());
        assert!(bounded_update_get(
            Url::parse("http://127.0.0.1/private").unwrap(),
            Duration::from_millis(20)
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn metadata_body_size_limit_rejects_an_oversized_local_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n12345678901234567890").await.unwrap();
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        assert!(bounded_text(response, 10).await.is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn metadata_body_timeout_cancels_a_stalled_local_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let result = metadata_response_text(response, 100, Duration::from_millis(25)).await;
        assert!(result.unwrap_err().contains("timed out"));
        server.abort();
    }
    fn release_fixture(tag: &str, abi: &str, draft: bool) -> Value {
        let version = tag
            .strip_prefix('v')
            .unwrap()
            .strip_suffix("-tauri")
            .unwrap();
        let name = format!("OpenLess_{version}+build.test_{abi}.apk");
        json!({"tag_name":tag,"draft":draft,"prerelease":true,"assets":[
            {"name":name,"browser_download_url":format!("https://github.com/Open-Less/openless/releases/download/{tag}/{name}")},
            {"name":format!("{name}.sig"),"browser_download_url":format!("https://github.com/Open-Less/openless/releases/download/{tag}/{name}.sig")}
        ]})
    }
    fn page_fixture(tag: &str, abi: &str) -> (String, String) {
        let version = tag
            .strip_prefix('v')
            .unwrap()
            .strip_suffix("-tauri")
            .unwrap();
        let name = format!("OpenLess_{version}+build.test_{abi}.apk");
        (format!("<a href=\"/Open-Less/openless/releases/download/{tag}/{name}\"><a href=\"/Open-Less/openless/releases/download/{tag}/{name}.sig\">"),name)
    }
    fn two_beta_feed() -> &'static str {
        r#"<entry><link href="https://github.com/Open-Less/openless/releases/tag/v2.0.2-Beta.4-tauri"/></entry><entry><link href="https://github.com/Open-Less/openless/releases/tag/v2.0.2-Beta.5-tauri"/></entry>"#
    }
    #[test]
    fn api_chooses_latest_published_release_with_a_matching_abi_pair() {
        let release = json!([
            release_fixture("v2.0.2-Beta.7-tauri","arm64-v8a",true),
            release_fixture("v2.0.2-Beta.6-tauri","x86",false),
            {"tag_name":"v2.0.2-Beta.5-tauri","draft":false,"prerelease":true,"assets":[]},
            release_fixture("v2.0.2-Beta.4-tauri","arm64-v8a",false)
        ]);
        assert_eq!(
            latest_android_release(&release, UpdateChannel::Beta, "aarch64").unwrap()["tag_name"],
            "v2.0.2-Beta.4-tauri"
        );
        assert_eq!(
            latest_android_release(&release, UpdateChannel::Beta, "i686").unwrap()["tag_name"],
            "v2.0.2-Beta.6-tauri"
        );
    }
    #[tokio::test]
    async fn public_feed_skips_only_missing_or_assetless_newer_tags() {
        for missing in [false, true] {
            let calls = std::sync::Mutex::new(Vec::new());
            let (page, name) = page_fixture("v2.0.2-Beta.4-tauri", "arm64-v8a");
            let result = web_manifest_from_feed(
                two_beta_feed(),
                UpdateChannel::Beta,
                "aarch64",
                &Arc::new(Mutex::new(None)),
                |url, _| {
                    calls.lock().unwrap().push(url.clone());
                    let result = if url.contains("expanded_assets/v2.0.2-Beta.5") {
                        if missing {
                            Err("update HTTP status 404".into())
                        } else {
                            Ok("Source code archives only".into())
                        }
                    } else if url.contains("expanded_assets/v2.0.2-Beta.4") {
                        Ok(page.clone())
                    } else {
                        Ok(signature(&name))
                    };
                    async move { result }
                },
            )
            .await
            .unwrap();
            assert_eq!(result["version"], "2.0.2-Beta.4+build.test");
            assert!(calls.lock().unwrap()[0].contains("Beta.5"));
        }
    }
    #[tokio::test]
    async fn newer_matching_pair_signature_or_transport_failure_never_uses_older_release() {
        for tampered in [false, true] {
            let calls = std::sync::Mutex::new(Vec::new());
            let (page, _) = page_fixture("v2.0.2-Beta.5-tauri", "arm64-v8a");
            let result = web_manifest_from_feed(
                two_beta_feed(),
                UpdateChannel::Beta,
                "aarch64",
                &Arc::new(Mutex::new(None)),
                |url, _| {
                    calls.lock().unwrap().push(url.clone());
                    let result = if url.contains("expanded_assets/v2.0.2-Beta.5") {
                        Ok(page.clone())
                    } else if tampered {
                        Ok(signature("OpenLess_2.0.2-Beta.4+build.test_arm64-v8a.apk"))
                    } else {
                        Err("update request timed out".into())
                    };
                    async move { result }
                },
            )
            .await;
            assert!(result
                .unwrap_err()
                .starts_with("GitHub release candidate: 2.0.2-Beta.5+build.test: "));
            assert!(calls
                .lock()
                .unwrap()
                .iter()
                .all(|url| !url.contains("Beta.4")));
        }
    }
    #[test]
    fn outer_fallback_preserves_known_candidate_error_category() {
        let candidate =
            "GitHub release candidate: 2.0.2-Beta.6+build.test: signed APK filename differs"
                .to_string();
        assert_eq!(
            combine_discovery_errors("API quota exceeded".into(), candidate.clone()),
            candidate
        );
        assert!(combine_discovery_errors(
            "API quota exceeded".into(),
            "public feed unavailable".into()
        )
        .starts_with("GitHub API:"));
    }

    #[tokio::test]
    async fn discovery_deadline_distinguishes_known_pair_from_unknown_network() {
        for after_pair in [false, true] {
            let candidate_seen: KnownCandidate = Arc::new(Mutex::new(None));
            let (page, _) = page_fixture("v2.0.2-Beta.5-tauri", "arm64-v8a");
            let discovery = web_manifest_from_feed(
                two_beta_feed(),
                UpdateChannel::Beta,
                "aarch64",
                &candidate_seen,
                |url, _| {
                    let page = page.clone();
                    async move {
                        if after_pair && url.contains("expanded_assets") {
                            Ok(page)
                        } else {
                            std::future::pending::<Result<String, String>>().await
                        }
                    }
                },
            );
            let error =
                discovery_with_deadline(Duration::from_millis(20), &candidate_seen, discovery)
                    .await
                    .unwrap_err();
            assert_eq!(candidate_seen.lock().unwrap().is_some(), after_pair);
            if after_pair {
                assert!(error.starts_with("GitHub release candidate: 2.0.2-Beta.5+build.test: "));
            }
            assert_eq!(error.starts_with("GitHub release candidate:"), after_pair);
        }
    }
    #[test]
    fn legacy_release_mirror_urls_keep_exact_origin_and_repository_boundaries() {
        for raw in [
            "https://fastgit.cc/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc/https://github.com/Open-Less/openless/releases/download/v2.0.2-Beta.6-tauri/OpenLess.apk",
            "https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
        ] {
            let url = Url::parse(raw).unwrap();
            assert!(allowed_update_download_url(&url));
            assert!(allowed_request(&url, false));
            assert!(allowed_request(&url, true));
        }
        for raw in [
            "http://fastgit.cc/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc.attacker.invalid/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc@attacker.invalid/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://user:password@fastgit.cc/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc:444/https://github.com/Open-Less/openless/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc/https://github.com/other/repo/releases/latest/download/OpenLess.apk",
            "https://fastgit.cc/https://github.com/Open-Less/openless/releases-evil/download/OpenLess.apk",
            "https://fastgit.cc/https://attacker.invalid/Open-Less/openless/releases/download/OpenLess.apk",
            "https://fastgit.cc/https://github.com/Open-Less/openless/releases/../../other/private.apk",
        ] {
            let url = Url::parse(raw).unwrap();
            assert!(!allowed_update_download_url(&url));
            assert!(!allowed_request(&url, false));
            assert!(!allowed_request(&url, true));
        }
    }
}
