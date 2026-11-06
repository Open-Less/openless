//! Where the in-app updater looks for release manifests.
//!
//! # Background
//!
//! The updater used to read its manifest from the GitHub Release assets
//! (`releases/latest/download/latest-{target}-{arch}.json`, and a `-beta` twin per tag). That
//! forced every release to carry fourteen `latest-*.json` files — seven target/arch entries
//! times a `-mirror` twin — all repeating the same three fields (`version`, `url`, `signature`),
//! and every new release re-uploaded the same file names because the Stable endpoint was a
//! rolling pointer.
//!
//! The manifests now live on the project's own CDN:
//!
//! ```text
//! https://dl.aeoform.com/updates/stable/latest-{target}-{arch}.json
//! https://dl.aeoform.com/updates/beta/latest-{target}-{arch}-beta.json
//! ```
//!
//! Two properties matter:
//!
//! - **One rolling file per platform.** The CDN name is stable per channel, so a release never
//!   accumulates manifest copies; publishing overwrites in place.
//! - **Channel separation is a path, not a filename suffix.** `/updates/stable/` and
//!   `/updates/beta/` are distinct locations, so a Stable client cannot walk into a Beta
//!   manifest by accident.
//!
//! The manifest body is unchanged (`version`, `pub_date`, `url`, `signature`), so
//! `tauri-plugin-updater` keeps owning download, minisign verification and platform install.
//!
//! # Aliyun artifacts, then GitHub
//!
//! Maintainers upload the binary artifacts to Aliyun at
//! `https://dl.aeoform.com/releases/{tag}/{asset}`. The manifest `url` always names that
//! Aliyun object. At download time the client asks Aliyun first: a successful fetch stays
//! on Aliyun because that path is faster. When Aliyun does not serve the object (missing,
//! non-success, or the probe fails), the same `{tag}/{asset}` is downloaded from
//! `https://github.com/Open-Less/openless/releases/download/{tag}/{asset}`.
//! The rolling JSON itself has no GitHub copy. A client can only discover a new version
//! after that JSON is overwritten on Aliyun.

use std::time::Duration;

use crate::types::UpdateChannel;

/// CDN that serves both the update manifests and the mirrored release artifacts.
pub(crate) const CDN_BASE: &str = "https://dl.aeoform.com";

/// Host that must appear on an Aliyun artifact URL before it can fall back to GitHub.
const CDN_HOST: &str = "dl.aeoform.com";

/// GitHub Release download prefix for the same `{tag}/{asset}` Aliyun stores.
const GITHUB_RELEASE_DOWNLOAD_BASE: &str =
    "https://github.com/Open-Less/openless/releases/download";

/// How long the Aliyun existence probe may wait before the download uses GitHub.
const ARTIFACT_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Manifest URL for `channel`, with the `{{target}}` / `{{arch}}` tokens left in place for
/// `tauri-plugin-updater` to substitute at check time.
///
/// `format!` doubles the braces, so `{{{{target}}}}` renders as the literal `{{target}}`.
pub fn endpoint_template(channel: UpdateChannel) -> String {
    let (path, suffix) = match channel {
        UpdateChannel::Stable => ("stable", ""),
        UpdateChannel::Beta => ("beta", "-beta"),
    };
    format!("{CDN_BASE}/updates/{path}/latest-{{{{target}}}}-{{{{arch}}}}{suffix}.json")
}

/// Concrete manifest URL for clients that fetch the manifest themselves (Android).
///
/// `target`/`arch` use the updater token convention (`android` + `aarch64`, `darwin` +
/// `x86_64`, …), i.e. the same values the plugin would substitute into
/// [`endpoint_template`].
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn manifest_url(channel: UpdateChannel, target: &str, arch: &str) -> String {
    endpoint_template(channel)
        .replace("{{target}}", target)
        .replace("{{arch}}", arch)
}

/// Map one Aliyun release object to the GitHub Release asset of the same name.
///
/// Accepts only `https://dl.aeoform.com/releases/{tag}/{asset}` with a single tag segment
/// and a single asset segment. Any other host, scheme, userinfo, port, query, extra slash,
/// or `..` segment returns `None` so a manifest cannot redirect the download elsewhere.
pub fn github_fallback_for_cdn_artifact(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(CDN_HOST)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }

    let mut segments = parsed.path_segments()?;
    if segments.next()? != "releases" {
        return None;
    }
    // `path_segments` keeps percent-encoding. Decode once so `%2B` is the same asset as `+`,
    // then reject a decoded slash or `..` before encoding the GitHub path.
    let tag = decode_path_segment(segments.next()?)?;
    let asset = decode_path_segment(segments.next()?)?;
    if segments.next().is_some() || !is_single_path_segment(&tag) || !is_single_path_segment(&asset)
    {
        return None;
    }

    Some(format!(
        "{GITHUB_RELEASE_DOWNLOAD_BASE}/{}/{}",
        encode_path_segment(&tag),
        encode_path_segment(&asset)
    ))
}

/// Keep `cdn_url` when Aliyun served the object. Otherwise use the GitHub asset URL when
/// the input is an Aliyun release object, and leave every other URL unchanged.
pub fn select_artifact_url(cdn_url: &str, cdn_available: bool) -> String {
    if cdn_available {
        return cdn_url.to_string();
    }
    github_fallback_for_cdn_artifact(cdn_url).unwrap_or_else(|| cdn_url.to_string())
}

/// Probe the Aliyun object and return the URL the downloader should GET.
///
/// A non-Aliyun URL is returned unchanged. Aliyun wins on HTTP 2xx. HEAD 403/405 is confirmed
/// with a one-byte range so a CDN that refuses HEAD can still be used. 404, any other
/// non-success, and a network or timeout failure all count as "Aliyun did not serve the
/// file" and select GitHub. The probe never reads the artifact body.
pub async fn resolve_artifact_download_url(url: &str) -> String {
    if github_fallback_for_cdn_artifact(url).is_none() {
        return url.to_string();
    }
    let available = aliyun_artifact_reachable(url).await;
    let chosen = select_artifact_url(url, available);
    if !available {
        log::info!("[updater] Aliyun artifact was not fetched; downloading from GitHub");
    }
    chosen
}

fn decode_path_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let value = u8::from_str_radix(hex, 16).ok()?;
            decoded.push(value);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).ok()
}

fn is_single_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains("..")
        && !segment.contains(['/', '\\', '\0'])
}

fn encode_path_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

async fn aliyun_artifact_reachable(url: &str) -> bool {
    let client = crate::net::http();
    match client
        .head(url)
        .timeout(ARTIFACT_PROBE_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            // Drop without reading. A follow-up range probe must not buffer the package.
            drop(response);
            if status.is_success() {
                return true;
            }
            if status == reqwest::StatusCode::METHOD_NOT_ALLOWED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return aliyun_range_probe_ok(&client, url).await;
            }
            false
        }
        Err(error) => {
            log::info!(
                "[updater] Aliyun artifact probe failed ({})",
                crate::net::request_error_kind(&error)
            );
            false
        }
    }
}

async fn aliyun_range_probe_ok(client: &reqwest::Client, url: &str) -> bool {
    match client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .timeout(ARTIFACT_PROBE_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            drop(response);
            status == reqwest::StatusCode::OK || status == reqwest::StatusCode::PARTIAL_CONTENT
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_endpoint_has_no_beta_marker() {
        let url = endpoint_template(UpdateChannel::Stable);
        assert!(
            !url.ends_with('/'),
            "endpoint must not end with a slash: {url}"
        );
        assert!(url.starts_with("https://dl.aeoform.com/updates/stable/"));
        assert!(url.contains("{{target}}") && url.contains("{{arch}}"));
        assert!(!url.contains("beta"));
    }

    #[test]
    fn beta_endpoint_is_a_separate_path_with_suffix() {
        let url = endpoint_template(UpdateChannel::Beta);
        assert!(url.starts_with("https://dl.aeoform.com/updates/beta/"));
        assert!(url.ends_with("-{{target}}-{{arch}}-beta.json"));
    }

    #[test]
    fn manifest_url_substitutes_every_token() {
        for (channel, expected) in [
            (
                UpdateChannel::Stable,
                "https://dl.aeoform.com/updates/stable/latest-darwin-aarch64.json",
            ),
            (
                UpdateChannel::Beta,
                "https://dl.aeoform.com/updates/beta/latest-darwin-aarch64-beta.json",
            ),
        ] {
            let url = manifest_url(channel, "darwin", "aarch64");
            assert_eq!(url, expected);
            assert!(!url.contains("{{"), "unsubstituted token in {url}");
        }
    }

    #[test]
    fn manifest_url_covers_every_published_target() {
        for (target, arch, expected) in [
            ("darwin", "aarch64", "latest-darwin-aarch64.json"),
            ("darwin", "x86_64", "latest-darwin-x86_64.json"),
            ("windows", "x86_64", "latest-windows-x86_64.json"),
            ("android", "aarch64", "latest-android-aarch64.json"),
            ("android", "armv7", "latest-android-armv7.json"),
            ("android", "x86_64", "latest-android-x86_64.json"),
            ("android", "i686", "latest-android-i686.json"),
        ] {
            assert!(
                manifest_url(UpdateChannel::Stable, target, arch).ends_with(expected),
                "{target}/{arch} should resolve to {expected}"
            );
            assert!(
                manifest_url(UpdateChannel::Beta, target, arch)
                    .ends_with(&expected.replace(".json", "-beta.json")),
                "{target}/{arch} beta should carry the -beta suffix"
            );
        }
    }

    #[test]
    fn cdn_base_has_no_trailing_slash() {
        assert!(!CDN_BASE.ends_with('/'));
        assert!(endpoint_template(UpdateChannel::Stable).starts_with(CDN_BASE));
    }

    #[test]
    fn aliyun_artifact_maps_to_the_github_release_asset() {
        let cdn = "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess_aarch64.app.tar.gz";
        assert_eq!(
            github_fallback_for_cdn_artifact(cdn).as_deref(),
            Some(
                "https://github.com/Open-Less/openless/releases/download/v2.0.2-Beta.4-tauri/OpenLess_aarch64.app.tar.gz"
            )
        );
    }

    #[test]
    fn percent_encoded_plus_stays_a_single_github_asset_segment() {
        let encoded = "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess_2.0.2-Beta.4%2Bbuild.20261106-16d927ba_x64-setup.exe";
        let literal_plus = "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess_2.0.2-Beta.4+build.20261106-16d927ba_x64-setup.exe";
        let expected = "https://github.com/Open-Less/openless/releases/download/v2.0.2-Beta.4-tauri/OpenLess_2.0.2-Beta.4%2Bbuild.20261106-16d927ba_x64-setup.exe";
        assert_eq!(
            github_fallback_for_cdn_artifact(encoded).as_deref(),
            Some(expected)
        );
        assert_eq!(
            github_fallback_for_cdn_artifact(literal_plus).as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn github_fallback_rejects_anything_except_one_aliyun_release_object() {
        for rejected in [
            "https://github.com/Open-Less/openless/releases/download/v2.0.2-Beta.4-tauri/OpenLess.dmg",
            "https://dl.aeoform.com/updates/beta/latest-darwin-aarch64-beta.json",
            "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/dir/OpenLess.dmg",
            "https://dl.aeoform.com/releases/../OpenLess.dmg",
            "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/../OpenLess.dmg",
            "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess%2Fevil.dmg",
            "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess%.dmg",
            "http://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess.dmg",
            "https://dl.aeoform.com.evil.example/releases/v2.0.2-Beta.4-tauri/OpenLess.dmg",
            "https://user:pass@dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess.dmg",
            "https://dl.aeoform.com:8443/releases/v2.0.2-Beta.4-tauri/OpenLess.dmg",
            "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess.dmg?redirect=1",
        ] {
            assert_eq!(
                github_fallback_for_cdn_artifact(rejected),
                None,
                "{rejected}"
            );
        }
    }

    #[test]
    fn select_artifact_url_prefers_aliyun_and_falls_back_only_when_missing() {
        let cdn = "https://dl.aeoform.com/releases/v2.0.2-Beta.4-tauri/OpenLess_aarch64.app.tar.gz";
        let github = github_fallback_for_cdn_artifact(cdn).expect("cdn url");
        assert_eq!(select_artifact_url(cdn, true), cdn);
        assert_eq!(select_artifact_url(cdn, false), github);
        let already_github = github.as_str();
        assert_eq!(select_artifact_url(already_github, false), already_github);
    }
}
