//! HarmonyOS update channel and GitHub Release Atom feed update checker.
use crate::Host;
use openless_core::shared_types::UpdateChannel;
use serde_json::{json, Value};

const RELEASES_ATOM_URL: &str = "https://github.com/Open-Less/openless/releases.atom";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEntry {
    pub tag_name: String,
    pub version: String,
    pub html_url: String,
    pub published_at: String,
    pub is_prerelease: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<Vec<PreIdent>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreIdent {
    Num(u64),
    Alpha(String),
}

impl Ord for PreIdent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (PreIdent::Num(a), PreIdent::Num(b)) => a.cmp(b),
            (PreIdent::Num(_), PreIdent::Alpha(_)) => std::cmp::Ordering::Less,
            (PreIdent::Alpha(_), PreIdent::Num(_)) => std::cmp::Ordering::Greater,
            (PreIdent::Alpha(a), PreIdent::Alpha(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PreIdent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version(raw: &str) -> Option<ParsedVersion> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let without_build = trimmed.split('+').next().unwrap_or(trimmed);
    let (core, pre) = match without_build.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre)),
        _ => (without_build, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let prerelease = pre.map(|value| {
        value
            .split('.')
            .map(|part| {
                if let Ok(number) = part.parse::<u64>() {
                    PreIdent::Num(number)
                } else {
                    PreIdent::Alpha(part.to_ascii_lowercase())
                }
            })
            .collect()
    });
    Some(ParsedVersion {
        major,
        minor,
        patch,
        prerelease,
    })
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    let to = rest.find(end)?;
    Some(rest[..to].trim())
}

pub fn parse_releases_from_atom(body: &str) -> Vec<ReleaseEntry> {
    let mut releases = Vec::new();
    for entry in body.split("<entry>").skip(1) {
        let entry_body = entry
            .split_once("</entry>")
            .map(|(b, _)| b)
            .unwrap_or(entry);
        let needle = "/releases/tag/";
        let Some(tag_start) = entry_body.find(needle).map(|i| i + needle.len()) else {
            continue;
        };
        let tag_after = &entry_body[tag_start..];
        let tag_end = tag_after
            .find(|c: char| c == '"' || c == '<' || c == ' ' || c == '/')
            .unwrap_or(tag_after.len());
        let tag_name = tag_after[..tag_end].trim().to_string();
        if tag_name.is_empty() {
            continue;
        }
        let version = tag_name.trim_start_matches(['v', 'V']).to_string();
        if parse_version(&version).is_none() {
            continue;
        }
        let is_prerelease = version.contains('-');
        let published_at = extract_between(entry_body, "<updated>", "</updated>")
            .unwrap_or("")
            .to_string();
        releases.push(ReleaseEntry {
            html_url: format!("https://github.com/Open-Less/openless/releases/tag/{tag_name}"),
            tag_name,
            version,
            published_at,
            is_prerelease,
        });
    }
    releases
}

pub fn select_latest_release(
    releases: &[ReleaseEntry],
    channel: UpdateChannel,
) -> Option<ReleaseEntry> {
    releases
        .iter()
        .filter(|entry| match channel {
            UpdateChannel::Stable => !entry.is_prerelease,
            UpdateChannel::Beta => true,
        })
        .max_by(|left, right| {
            parse_version(&left.version)
                .cmp(&parse_version(&right.version))
                .then_with(|| left.published_at.cmp(&right.published_at))
        })
        .cloned()
}

pub fn effective_channel(host: &Host, requested: Option<UpdateChannel>) -> UpdateChannel {
    if let Some(channel) = requested {
        return channel;
    }
    let prefs = host.backend.get_preferences();
    if prefs.update_channel_explicit {
        prefs.update_channel
    } else if env!("CARGO_PKG_VERSION").contains('-') {
        UpdateChannel::Beta
    } else {
        UpdateChannel::Stable
    }
}

pub fn get_channel(host: &Host) -> Value {
    json!(effective_channel(host, None))
}

pub fn set_channel(host: &Host, args: &Value) -> Result<Value, String> {
    let channel: UpdateChannel = serde_json::from_value(args["channel"].clone())
        .map_err(|_| "channel must be 'stable' or 'beta'".to_string())?;
    let mut prefs = host.backend.get_preferences();
    prefs.update_channel = channel;
    prefs.update_channel_explicit = true;
    let _entered = host.runtime.enter();
    host.backend
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &openless_core::settings::NoopSettingsRuntime,
        )
        .map_err(|e| e.to_string())?;
    Ok(json!(channel))
}

async fn fetch_atom_feed(timeout_ms: u64) -> Result<String, String> {
    let clamped_ms = timeout_ms.clamp(3_000, 20_000);
    let resp = openless_core::net::http()
        .get(RELEASES_ATOM_URL)
        .timeout(std::time::Duration::from_millis(clamped_ms))
        .send()
        .await
        .map_err(|e| format!("无法连接更新服务：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("更新服务返回异常状态 {}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("读取更新清单失败：{e}"))
}

pub fn fetch_latest_beta(host: &Host) -> Result<Value, String> {
    let body = host.runtime.block_on(fetch_atom_feed(12_000))?;
    let releases = parse_releases_from_atom(&body);
    let latest = releases
        .into_iter()
        .filter(|entry| entry.is_prerelease)
        .max_by(|left, right| parse_version(&left.version).cmp(&parse_version(&right.version)));
    Ok(match latest {
        Some(entry) => json!({
            "tagName": entry.tag_name,
            "version": entry.version,
            "htmlUrl": entry.html_url,
            "publishedAt": entry.published_at,
        }),
        None => Value::Null,
    })
}

pub fn check_update(host: &Host, args: &Value) -> Result<Value, String> {
    let requested: Option<UpdateChannel> = if args["channel"].is_null() {
        None
    } else {
        Some(
            serde_json::from_value(args["channel"].clone())
                .map_err(|_| "channel must be 'stable' or 'beta'".to_string())?,
        )
    };
    let channel = effective_channel(host, requested);
    let current_version = args["currentVersion"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string();
    let timeout_ms = args["timeoutMs"].as_u64().unwrap_or(12_000);
    let body = host.runtime.block_on(fetch_atom_feed(timeout_ms))?;
    let releases = parse_releases_from_atom(&body);
    let Some(latest) = select_latest_release(&releases, channel) else {
        return Ok(json!({
            "currentVersion": current_version,
            "latestVersion": current_version,
            "tagName": "",
            "channel": channel,
            "hasUpdate": false,
            "publishedAt": "",
            "releaseUrl": "https://github.com/Open-Less/openless/releases"
        }));
    };
    let has_update = is_newer_version(&latest.version, &current_version);
    Ok(json!({
        "currentVersion": current_version,
        "latestVersion": latest.version,
        "tagName": latest.tag_name,
        "channel": channel,
        "hasUpdate": has_update,
        "publishedAt": latest.published_at,
        "releaseUrl": latest.html_url
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const SAMPLE_ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.3-beta.2"/>
    <updated>2026-11-27T10:00:00Z</updated>
  </entry>
  <entry>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.2"/>
    <updated>2026-11-20T10:00:00Z</updated>
  </entry>
  <entry>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.2-beta.7"/>
    <updated>2026-11-18T10:00:00Z</updated>
  </entry>
</feed>"#;

    #[test]
    fn parses_atom_and_compares_stable_and_beta_versions() {
        let releases = parse_releases_from_atom(SAMPLE_ATOM);
        assert_eq!(releases.len(), 3);
        let stable = select_latest_release(&releases, UpdateChannel::Stable).unwrap();
        assert_eq!(stable.version, "2.0.2");
        assert!(!stable.is_prerelease);
        let beta = select_latest_release(&releases, UpdateChannel::Beta).unwrap();
        assert_eq!(beta.version, "2.0.3-beta.2");
        assert!(beta.is_prerelease);

        assert!(is_newer_version("2.0.2", "2.0.2-beta.7"));
        assert!(is_newer_version("2.0.3-beta.2", "2.0.2"));
        assert!(is_newer_version("2.0.3-beta.10", "2.0.3-beta.2"));
        assert!(!is_newer_version("2.0.2", "2.0.2"));
        assert!(!is_newer_version("2.0.2-beta.7", "2.0.2"));
    }

    #[test]
    fn update_channel_roundtrips_through_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-update-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let host = crate::build_host(root.clone(), root.join("cache"), runtime).unwrap();
        assert_eq!(
            crate::dispatch(&host, "set_update_channel", json!({"channel": "beta"})).unwrap(),
            json!("beta")
        );
        assert_eq!(
            crate::dispatch(&host, "get_update_channel", json!({})).unwrap(),
            json!("beta")
        );
        assert_eq!(
            crate::dispatch(&host, "set_update_channel", json!({"channel": "stable"})).unwrap(),
            json!("stable")
        );
        assert_eq!(
            crate::dispatch(&host, "get_update_channel", json!({})).unwrap(),
            json!("stable")
        );
        host.runtime.block_on(host.backend.shutdown()).unwrap();
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }
}
