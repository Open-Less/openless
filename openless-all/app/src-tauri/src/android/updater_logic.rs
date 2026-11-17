//! Pure Android updater helpers (version compare, minisign verify). Testable on all targets.

/// Must match `plugins.updater.pubkey` in `tauri.conf.json` (TAURI_SIGNING_PRIVATE_KEY pair).
pub const UPDATER_PUBKEY_B64: &str =
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDFERUFBODAzNTY0QzMyM0YKUldRL01reFdBNmpxSGE1K0JadlpONXNWTzhJcGZCRGxjUVdIWExNNFJpeUNsSGZwazdlQThhemkK";

/// Returned when Kotlin `installApk` cannot open the system installer (e.g. missing install permission).
pub const INSTALLER_NOT_OPENED_MSG: &str =
    "无法打开系统安装器：请先在系统设置中允许 OpenLess 安装未知应用，然后重新点击更新";

pub fn map_abi_to_arch(abi: &str) -> &'static str {
    match abi {
        "arm64-v8a" => "aarch64",
        "armeabi-v7a" => "armv7",
        "x86" => "i686",
        "x86_64" => "x86_64",
        _ => "aarch64",
    }
}

pub fn version_is_newer(remote: &str, current: &str) -> bool {
    let (Ok(remote), Ok(current)) = (
        semver::Version::parse(remote),
        semver::Version::parse(current),
    ) else {
        return false;
    };
    remote.cmp_precedence(&current).is_gt()
}

/// Human-readable manifest fetch failure for UI tooltips.
pub fn format_manifest_error(status: u16, url: &str) -> String {
    if status == 404 {
        format!("更新清单不存在 (404): {url}")
    } else {
        format!("无法获取更新清单 (HTTP {status}): {url}")
    }
}

/// Unwrap a Tauri updater blob into minisign text.
///
/// `plugins.updater.pubkey` and `.sig` files store the minisign file as
/// standard Base64. Desktop `tauri-plugin-updater` base64-decodes first, then
/// calls `PublicKey::decode` / `Signature::decode`. Passing the wrapped pubkey
/// to `PublicKey::from_base64` fails with "Invalid encoding in minisign data"
/// because that API expects the 42-byte key line, not the 114-byte file.
pub fn decode_tauri_minisign_text(encoded: &str) -> Result<String, String> {
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Err("empty minisign blob".to_string());
    }
    if trimmed.starts_with("untrusted comment:") {
        return Ok(trimmed.to_string());
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|e| format!("decode minisign blob: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("minisign blob is not UTF-8: {e}"))
}

pub fn parse_updater_public_key(pubkey_b64: &str) -> Result<minisign_verify::PublicKey, String> {
    let text = decode_tauri_minisign_text(pubkey_b64)?;
    minisign_verify::PublicKey::decode(&text).map_err(|e| format!("parse updater pubkey: {e}"))
}

pub fn parse_updater_signature(signature: &str) -> Result<minisign_verify::Signature, String> {
    let text = decode_tauri_minisign_text(signature)?;
    minisign_verify::Signature::decode(&text).map_err(|e| format!("decode signature: {e}"))
}

/// Verify APK bytes against a Tauri updater signature (prehashed, same as desktop).
pub fn verify_updater_signature(
    data: &[u8],
    signature: &str,
    pubkey_b64: &str,
) -> Result<(), String> {
    let public_key = parse_updater_public_key(pubkey_b64)?;
    let signature = parse_updater_signature(signature)?;
    public_key
        .verify(data, &signature, true)
        .map_err(|e| format!("signature verify failed: {e}"))?;
    Ok(())
}

/// Every fallback retains the matching URL, version and detached signature as one unit.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UpdateCandidate {
    pub version: String,
    pub url: String,
    pub signature: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

fn freshest_full_version<'a>(
    candidates: impl Iterator<Item = &'a UpdateCandidate>,
) -> Option<String> {
    let mut selected: Option<(&UpdateCandidate, semver::Version)> = None;
    let github = |candidate: &UpdateCandidate| {
        url::Url::parse(&candidate.url)
            .ok()
            .is_some_and(|url| url.host_str() == Some("github.com"))
    };
    for candidate in candidates {
        let Ok(version) = semver::Version::parse(&candidate.version) else {
            continue;
        };
        let replace = selected.as_ref().is_none_or(|(current, current_version)| {
            version.cmp_precedence(current_version).is_gt()
                || version.cmp_precedence(current_version).is_eq()
                    && github(candidate)
                    && !github(current)
        });
        if replace {
            selected = Some((candidate, version));
        }
    }
    selected.map(|(candidate, _)| candidate.version.clone())
}

/// Compare release precedence first. Equal-precedence conflicting build metadata uses
/// GitHub's canonical version; candidates with another exact full version never mix.
pub fn newest_candidates(candidates: Vec<UpdateCandidate>) -> Vec<UpdateCandidate> {
    let Some(newest) = freshest_full_version(candidates.iter()) else {
        return Vec::new();
    };
    candidates
        .into_iter()
        .filter(|v| v.version == newest)
        .collect()
}

/// Estimated completion = first-byte latency + artifact bytes / sampled throughput.
/// Unknown sizes use the same conservative 100 MiB reference on every source.
/// Invalid/failed samples sort last but remain usable as same-version fallbacks.
pub fn rank_download_candidates(
    candidates: Vec<(UpdateCandidate, Option<(f64, f64)>)>,
) -> Vec<UpdateCandidate> {
    let Some(newest) = freshest_full_version(candidates.iter().map(|(v, _)| v)) else {
        return Vec::new();
    };
    let reference_size = candidates
        .iter()
        .filter(|(v, _)| v.version == newest)
        .filter_map(|(v, _)| v.size)
        .max()
        .unwrap_or(100 * 1024 * 1024)
        .max(1);
    let mut candidates: Vec<_> = candidates
        .into_iter()
        .filter(|(v, _)| v.version == newest)
        .collect();
    let estimate = |sample: Option<(f64, f64)>| match sample {
        Some((latency, throughput))
            if latency.is_finite()
                && latency >= 0.
                && throughput.is_finite()
                && throughput > 0. =>
        {
            latency + reference_size as f64 / throughput
        }
        _ => f64::INFINITY,
    };
    candidates.sort_by(|a, b| estimate(a.1).total_cmp(&estimate(b.1)));
    candidates.into_iter().map(|(v, _)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_newer_detects_patch_bump() {
        assert!(version_is_newer("1.3.9", "1.3.8"));
        assert!(!version_is_newer("1.3.8", "1.3.9"));
        assert!(!version_is_newer("1.3.8", "1.3.8"));
    }

    #[test]
    fn version_is_newer_uses_semver_prerelease_ordering() {
        assert!(version_is_newer("1.3.18", "1.3.18-Beta.7"));
        assert!(version_is_newer("1.3.18-Beta.8", "1.3.18-Beta.7"));
        assert!(!version_is_newer("1.3.18-Beta.7", "1.3.18-Beta.7"));
        assert!(!version_is_newer("1.3.17", "1.3.18-Beta.7"));
        assert!(!version_is_newer("not-a-version", "1.3.18-Beta.7"));
    }

    #[test]
    fn build_metadata_alone_does_not_offer_an_upgrade() {
        assert!(!version_is_newer(
            "2.0.2-Beta.6+build.20261110-new",
            "2.0.2-Beta.6+build.20261109-old",
        ));
        assert!(!version_is_newer(
            "2.0.2-Beta.6+build.20261109-old",
            "2.0.2-Beta.6+build.20261110-new",
        ));
        assert!(version_is_newer(
            "2.0.2-Beta.7+build.20261109-new",
            "2.0.2-Beta.6+build.20261110-old",
        ));
    }

    #[test]
    fn map_abi_to_arch_maps_arm64() {
        assert_eq!(map_abi_to_arch("arm64-v8a"), "aarch64");
    }

    #[test]
    fn updater_pubkey_matches_tauri_conf() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf_path = manifest_dir.join("tauri.conf.json");
        let conf_text = std::fs::read_to_string(&conf_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", conf_path.display()));
        let conf: serde_json::Value =
            serde_json::from_str(&conf_text).expect("parse tauri.conf.json");
        let conf_pubkey = conf
            .get("plugins")
            .and_then(|p| p.get("updater"))
            .and_then(|u| u.get("pubkey"))
            .and_then(|v| v.as_str())
            .expect("plugins.updater.pubkey in tauri.conf.json");
        assert_eq!(conf_pubkey, UPDATER_PUBKEY_B64);
    }

    #[test]
    fn updater_pubkey_decodes_to_minisign_file() {
        let text = decode_tauri_minisign_text(UPDATER_PUBKEY_B64).expect("decode pubkey");
        assert!(
            text.starts_with("untrusted comment: minisign public key:"),
            "decoded={text:?}"
        );
        let key_line = text.lines().nth(1).expect("minisign pubkey has a key line");
        assert!(
            key_line.starts_with("RW"),
            "key line should be the minisign RW blob, got {key_line}"
        );
    }

    #[test]
    fn wrapped_updater_pubkey_is_not_a_raw_42_byte_key() {
        let err = minisign_verify::PublicKey::from_base64(UPDATER_PUBKEY_B64)
            .expect_err("Tauri-wrapped pubkey must not parse as a raw 42-byte key");
        assert!(
            err.to_string().contains("Invalid encoding"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn updater_pubkey_parses_after_tauri_unwrap() {
        parse_updater_public_key(UPDATER_PUBKEY_B64)
            .expect("Tauri-wrapped pubkey must parse after unwrap");
    }

    #[test]
    fn decode_tauri_minisign_text_passthrough_raw_file() {
        let raw = "untrusted comment: minisign public key: ABC\nRWABC";
        assert_eq!(
            decode_tauri_minisign_text(&format!("{raw}\n")).unwrap(),
            raw
        );
        assert_eq!(decode_tauri_minisign_text(raw).unwrap(), raw);
    }

    #[test]
    fn decode_tauri_minisign_text_rejects_garbage() {
        assert!(decode_tauri_minisign_text("%%%not-base64%%%").is_err());
        assert!(decode_tauri_minisign_text("").is_err());
        assert!(decode_tauri_minisign_text("   ").is_err());
    }
}

#[cfg(test)]
mod source_selection_tests {
    use super::*;
    fn candidate(version: &str, source: &str) -> UpdateCandidate {
        UpdateCandidate {
            version: version.into(),
            url: format!("https://{source}"),
            signature: format!("signature-{source}"),
            notes: None,
            pub_date: None,
            size: Some(50_000_000),
        }
    }
    #[test]
    fn a_stale_fast_cdn_cannot_beat_the_new_github_release() {
        let result = rank_download_candidates(vec![
            (candidate("2.0.2-Beta.5", "cdn"), Some((0.01, 1e10))),
            (candidate("2.0.2-Beta.6", "github"), Some((0.5, 1e5))),
        ]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "https://github");
        assert_eq!(result[0].signature, "signature-github");
    }
    #[test]
    fn either_source_can_stand_alone_and_full_versions_stay_separate() {
        for source in ["cdn", "github"] {
            let result = newest_candidates(vec![candidate("2.0.2-Beta.6", source)]);
            assert_eq!(result[0].url, format!("https://{source}"));
        }
        assert_eq!(
            newest_candidates(vec![
                candidate("2.0.2-Beta.6+build.20261109-a", "old"),
                candidate("2.0.2-Beta.6+build.20261110-a", "new")
            ])
            .len(),
            1
        );
        assert!(newest_candidates(vec![candidate("not-semver", "bad")]).is_empty());
    }
    #[test]
    fn equal_precedence_prefers_github_metadata_without_mixing_builds() {
        let mut github = candidate("2.0.2-Beta.6+build.20261109-canonical", "github");
        github.url =
            "https://github.com/Open-Less/openless/releases/download/v2.0.2-Beta.6-tauri/app.apk"
                .into();
        let cdn = candidate("2.0.2-Beta.6+build.99999999-stale", "cdn");
        let result = newest_candidates(vec![cdn.clone(), github.clone()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].version, github.version);
        let result =
            rank_download_candidates(vec![(github.clone(), None), (cdn, Some((0.001, 1e10)))]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].version, github.version);
        let mirror = candidate(&github.version, "cdn");
        assert_eq!(newest_candidates(vec![mirror, github]).len(), 2);
    }

    #[test]
    fn conflicting_latency_and_throughput_use_completion_estimate() {
        let version = "2.0.2-Beta.6";
        let result = rank_download_candidates(vec![
            (candidate(version, "low-latency"), Some((0.01, 1e6))),
            (candidate(version, "fast-transfer"), Some((0.5, 10e6))),
        ]);
        assert_eq!(result[0].url, "https://fast-transfer");
        let result = rank_download_candidates(vec![
            (candidate(version, "failed"), None),
            (candidate(version, "works"), Some((0.5, 1e6))),
        ]);
        assert_eq!(result[0].url, "https://works");
        assert_eq!(result.len(), 2);
    }
}
