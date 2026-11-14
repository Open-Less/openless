//! Source discovery and authenticated APK transfer, shared with host-side tests.

use std::time::Duration;

use super::updater_logic::{self, UpdateCandidate};
use crate::{types::UpdateChannel, update_source};

const MAX_APK_BYTES: u64 = 200 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

fn validate(
    candidate: UpdateCandidate,
    channel: UpdateChannel,
    arch: &str,
) -> Result<UpdateCandidate, String> {
    let version =
        semver::Version::parse(&candidate.version).map_err(|_| "invalid update version")?;
    let matches_channel = match channel {
        UpdateChannel::Stable => version.pre.is_empty(),
        UpdateChannel::Beta => version
            .pre
            .as_str()
            .to_ascii_lowercase()
            .starts_with("beta."),
    };
    if !matches_channel {
        return Err("update version does not match selected channel".into());
    }
    let url = url::Url::parse(&candidate.url).map_err(|_| "invalid update URL")?;
    if !update_source::allowed_update_download_url(&url) {
        return Err("untrusted APK source".into());
    }
    let abi = match arch {
        "aarch64" => "arm64-v8a",
        "armv7" => "armeabi-v7a",
        "i686" => "x86",
        "x86_64" => "x86_64",
        _ => return Err("unsupported update architecture".into()),
    };
    let filename = format!("OpenLess_{}_{abi}.apk", candidate.version);
    let tag_version = format!(
        "{}.{}.{}{}",
        version.major,
        version.minor,
        version.patch,
        if version.pre.is_empty() {
            String::new()
        } else {
            format!("-{}", version.pre)
        }
    );
    let signed = update_source::android_signature_version(
        &candidate.signature,
        &filename,
        &tag_version,
        arch,
    )?;
    if signed != candidate.version {
        return Err("APK signature version does not match update metadata".into());
    }
    Ok(candidate)
}

async fn cdn_manifest(channel: UpdateChannel, arch: &str) -> Result<UpdateCandidate, String> {
    let url = url::Url::parse(&update_source::manifest_url(channel, "android", arch))
        .map_err(|_| "invalid CDN manifest URL")?;
    let response = update_source::bounded_update_get(url, Duration::from_secs(8))
        .await
        .map_err(|_| "CDN update manifest unavailable")?;
    let length = response.content_length();
    let bytes = manifest_bytes(response.bytes_stream(), length).await?;
    let candidate = serde_json::from_slice::<UpdateCandidate>(&bytes)
        .map_err(|_| "invalid CDN update manifest")?;
    validate(candidate, channel, arch)
}

async fn manifest_bytes<S, B, E>(mut stream: S, length: Option<u64>) -> Result<Vec<u8>, String>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
{
    use futures_util::StreamExt;
    if length.is_some_and(|value| value > MAX_MANIFEST_BYTES as u64) {
        return Err("CDN update manifest exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "CDN update manifest interrupted")?;
        let chunk = chunk.as_ref();
        if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
            return Err("CDN update manifest exceeds size limit".into());
        }
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}

async fn source_results(
    channel: UpdateChannel,
    arch: &str,
) -> (
    Result<UpdateCandidate, String>,
    Result<UpdateCandidate, String>,
) {
    let (github, cdn) = tokio::join!(
        async {
            let value = update_source::github_android_manifest(channel, arch).await?;
            validate(
                serde_json::from_value(value).map_err(|_| "invalid GitHub update manifest")?,
                channel,
                arch,
            )
        },
        async {
            tokio::time::timeout(Duration::from_secs(8), cdn_manifest(channel, arch))
                .await
                .map_err(|_| "CDN update check timed out".to_string())?
        }
    );
    (github, cdn)
}

pub(crate) async fn candidates(
    channel: UpdateChannel,
    arch: &str,
) -> Result<Vec<UpdateCandidate>, String> {
    let (github, cdn) = source_results(channel, arch).await;
    merge_sources(github, cdn)
}

fn merge_sources(
    github: Result<UpdateCandidate, String>,
    cdn: Result<UpdateCandidate, String>,
) -> Result<Vec<UpdateCandidate>, String> {
    // A known release with invalid/unavailable signature metadata is not proof
    // that an older mirror is the newest release. Preserve the failure.
    if let Err(error) = &github {
        if error.starts_with("GitHub release candidate:") {
            let known_version = error
                .strip_prefix("GitHub release candidate: ")
                .and_then(|rest| rest.split_once(": "))
                .and_then(|(version, _)| semver::Version::parse(version).ok());
            let safe_mirror = known_version.as_ref().is_some_and(|known| {
                cdn.as_ref().ok().is_some_and(|candidate| {
                    semver::Version::parse(&candidate.version)
                        .ok()
                        .is_some_and(|mirror| {
                            candidate.version == known.to_string()
                                || mirror.cmp_precedence(known).is_gt()
                        })
                })
            });
            if !safe_mirror {
                return Err(error.clone());
            }
        }
    }
    let mut available = Vec::new();
    let mut errors = Vec::new();
    // GitHub is canonical when release precedence is equal but build metadata differs.
    for (source, result) in [("GitHub", github), ("CDN", cdn)] {
        match result {
            Ok(candidate) => available.push(candidate),
            Err(error) => errors.push(format!("{source}: {error}")),
        }
    }
    if available.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(available)
}

fn check_sources(
    github: Result<UpdateCandidate, String>,
    cdn: Result<UpdateCandidate, String>,
    current: &str,
) -> Result<Option<UpdateCandidate>, String> {
    let github_unavailable = github.is_err();
    let update = updater_logic::newest_candidates(merge_sources(github, cdn)?)
        .into_iter()
        .find(|candidate| updater_logic::version_is_newer(&candidate.version, current));
    // A rolling mirror can lag. An unavailable canonical source is not evidence
    // that the installed version is current, even if the mirror is reachable.
    if update.is_none() && github_unavailable {
        return Err("Unable to confirm the latest version from GitHub. Please retry.".into());
    }
    Ok(update)
}

pub(crate) async fn check(
    channel: UpdateChannel,
    arch: &str,
    current: &str,
) -> Result<Option<UpdateCandidate>, String> {
    let (github, cdn) = source_results(channel, arch).await;
    check_sources(github, cdn, current)
}

async fn transfer<P: Fn(u64, Option<u64>)>(
    candidate: &UpdateCandidate,
    progress: &P,
) -> Result<Vec<u8>, String> {
    let chosen = update_source::resolve_artifact_download_url(&candidate.url).await;
    let fallback = update_source::github_fallback_for_cdn_artifact(&chosen);
    let mut errors = Vec::new();
    for url in std::iter::once(chosen).chain(fallback) {
        let seconds = if url.starts_with("https://dl.aeoform.com/") {
            120
        } else {
            600
        };
        let mut attempt = candidate.clone();
        attempt.url = url;
        progress(0, attempt.size);
        let result = tokio::time::timeout(
            Duration::from_secs(seconds),
            transfer_once(&attempt, progress),
        )
        .await
        .map_err(|_| "APK download timed out".to_string())
        .and_then(|value| value)
        .and_then(|bytes| {
            updater_logic::verify_updater_signature(
                &bytes,
                &attempt.signature,
                updater_logic::UPDATER_PUBKEY_B64,
            )?;
            Ok(bytes)
        });
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("; "))
}

async fn transfer_once<P: Fn(u64, Option<u64>)>(
    candidate: &UpdateCandidate,
    progress: &P,
) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;
    let url = url::Url::parse(&candidate.url).map_err(|_| "invalid APK source")?;
    let response = update_source::bounded_update_get(url, Duration::from_secs(600)).await?;
    if !response.status().is_success() {
        return Err(format!(
            "APK download status {}",
            response.status().as_u16()
        ));
    }
    let total = response.content_length();
    if total.is_some_and(|length| length > MAX_APK_BYTES) {
        return Err("APK exceeds size limit".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .map_err(|_| "APK download stalled")?
    {
        let chunk = chunk.map_err(|_| "APK download interrupted")?;
        if bytes.len() as u64 + chunk.len() as u64 > MAX_APK_BYTES {
            return Err("APK exceeds size limit".into());
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    if total.is_some_and(|length| length != bytes.len() as u64) {
        return Err("incomplete APK download".into());
    }
    Ok(bytes)
}

pub(crate) async fn download<P: Fn(u64, Option<u64>)>(
    version: String,
    arch: &str,
    url: String,
    signature: String,
    progress: P,
) -> Result<Vec<u8>, String> {
    let parsed = semver::Version::parse(&version).map_err(|_| "invalid update version")?;
    let channel = if parsed.pre.is_empty() {
        UpdateChannel::Stable
    } else {
        UpdateChannel::Beta
    };
    let original = validate(
        UpdateCandidate {
            version: version.clone(),
            url,
            signature,
            notes: None,
            pub_date: None,
            size: None,
        },
        channel,
        arch,
    )?;
    let progress_ref = &progress;
    download_selected(
        &version,
        original,
        candidates(channel, arch).await,
        &progress,
        |url| async move { update_source::probe_android_download(&url).await },
        |candidate| async move {
            tokio::time::timeout(Duration::from_secs(600), transfer(&candidate, progress_ref))
                .await
                .map_err(|_| "APK download timed out".to_string())?
        },
        |bytes, signature| {
            updater_logic::verify_updater_signature(
                bytes,
                signature,
                updater_logic::UPDATER_PUBKEY_B64,
            )
        },
    )
    .await
}

// Both discovery and the original candidate are validated before entering this transfer loop.
async fn download_selected<Probe, ProbeFuture, Transfer, TransferFuture, Verify, P>(
    version: &str,
    original: UpdateCandidate,
    discovered: Result<Vec<UpdateCandidate>, String>,
    progress: &P,
    probe: Probe,
    transfer: Transfer,
    verify: Verify,
) -> Result<Vec<u8>, String>
where
    Probe: Fn(String) -> ProbeFuture,
    ProbeFuture: std::future::Future<Output = Option<(f64, f64)>>,
    Transfer: Fn(UpdateCandidate) -> TransferFuture,
    TransferFuture: std::future::Future<Output = Result<Vec<u8>, String>>,
    Verify: Fn(&[u8], &str) -> Result<(), String>,
    P: Fn(u64, Option<u64>),
{
    let mut available: Vec<_> = discovered
        .unwrap_or_default()
        .into_iter()
        .filter(|candidate| candidate.version == version)
        .collect();
    if !available
        .iter()
        .any(|candidate| candidate.url == original.url && candidate.signature == original.signature)
    {
        available.push(original);
    }
    // Every fallback is bound to the version the user selected; never install a downgrade.
    available.retain(|candidate| candidate.version == version);
    let known_size = available.iter().find_map(|candidate| candidate.size);
    for candidate in &mut available {
        if candidate.size.is_none() {
            candidate.size = known_size;
        }
    }
    let probes = futures_util::future::join_all(available.into_iter().map(|candidate| {
        let sample = probe(candidate.url.clone());
        async move { (candidate, sample.await) }
    }))
    .await;
    let ranked = updater_logic::rank_download_candidates(probes);
    let mut errors = Vec::new();
    for candidate in ranked {
        progress(0, candidate.size);
        let signature = candidate.signature.clone();
        let result = transfer(candidate).await.and_then(|bytes| {
            verify(&bytes, &signature)?;
            Ok(bytes)
        });
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) => errors.push(error),
        }
    }
    Err(format!("更新包下载或验证失败：{}", errors.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Public minisign-verify 0.3.0 test vector; no production or test private key is used.
    const TEST_PUBKEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    const VERSION: &str = "2.0.2-Beta.6+build.test";

    fn candidate(host: &str, version: &str) -> UpdateCandidate {
        UpdateCandidate {
            version: version.into(),
            url: format!("https://{host}/releases/fixture.apk"),
            signature: TEST_SIGNATURE.into(),
            notes: None,
            pub_date: None,
            size: Some(10_000_000),
        }
    }

    #[test]
    fn a_known_release_failure_cannot_be_replaced_by_an_older_mirror() {
        let old = candidate("dl.aeoform.com", "2.0.2-Beta.4");
        assert!(merge_sources(
            Err("GitHub release candidate: signature unavailable".into()),
            Ok(old.clone()),
        )
        .is_err());
        let available = merge_sources(Err("GitHub unavailable".into()), Ok(old)).unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].version, "2.0.2-Beta.4");
        let failed = "GitHub release candidate: 2.0.2-Beta.6+build.test: signature unavailable";
        assert!(merge_sources(
            Err(failed.into()),
            Ok(candidate("dl.aeoform.com", "2.0.2-Beta.4"))
        )
        .is_err());
        assert_eq!(
            merge_sources(Err(failed.into()), Ok(candidate("dl.aeoform.com", VERSION))).unwrap()[0]
                .version,
            VERSION
        );
        assert!(merge_sources(
            Err(failed.into()),
            Ok(candidate("dl.aeoform.com", "2.0.2-Beta.6+build.other"))
        )
        .is_err());
        assert_eq!(
            merge_sources(
                Err(failed.into()),
                Ok(candidate("dl.aeoform.com", "2.0.2-Beta.7"))
            )
            .unwrap()[0]
                .version,
            "2.0.2-Beta.7"
        );
    }

    #[test]
    fn github_unavailable_with_current_or_older_cdn_does_not_claim_up_to_date() {
        for mirror_version in [VERSION, "2.0.2-Beta.5"] {
            let error = check_sources(
                Err("GitHub temporarily unavailable".into()),
                Ok(candidate("dl.aeoform.com", mirror_version)),
                VERSION,
            )
            .unwrap_err();
            assert!(error.contains("Unable to confirm the latest version"));
        }
    }

    #[test]
    fn github_unavailable_can_still_offer_a_known_newer_cdn_update() {
        let update = check_sources(
            Err("GitHub temporarily unavailable".into()),
            Ok(candidate("dl.aeoform.com", VERSION)),
            "2.0.2-Beta.5",
        )
        .unwrap()
        .unwrap();
        assert_eq!(update.version, VERSION);
    }

    #[test]
    fn successful_canonical_check_can_confirm_the_current_version() {
        assert!(check_sources(
            Ok(candidate("github.com", VERSION)),
            Err("CDN unavailable".into()),
            VERSION,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn known_candidate_failure_only_uses_a_sufficiently_fresh_mirror_for_check() {
        let error = format!("GitHub release candidate: {VERSION}: signature unavailable");
        assert_eq!(
            check_sources(
                Err(error.clone()),
                Ok(candidate("dl.aeoform.com", VERSION)),
                "2.0.2-Beta.5",
            )
            .unwrap()
            .unwrap()
            .version,
            VERSION
        );
        assert!(check_sources(
            Err(error),
            Ok(candidate("dl.aeoform.com", "2.0.2-Beta.5")),
            "2.0.2-Beta.4",
        )
        .is_err());
    }

    fn metadata_candidate(version: &str, abi: &str) -> UpdateCandidate {
        let mut candidate = candidate("dl.aeoform.com", version);
        // Metadata validation precedes cryptographic validation of the downloaded bytes.
        candidate.signature = format!("untrusted comment: fixture\nnot-a-signature\ntrusted comment: timestamp:1\tfile:OpenLess_{version}_{abi}.apk\nnot-a-global-signature");
        candidate
    }

    fn verify_fixture(bytes: &[u8], signature: &str) -> Result<(), String> {
        updater_logic::verify_updater_signature(bytes, signature, TEST_PUBKEY)
    }

    #[test]
    fn metadata_validation_binds_channel_abi_full_signed_version_and_origin() {
        assert!(validate(
            metadata_candidate(VERSION, "arm64-v8a"),
            UpdateChannel::Beta,
            "aarch64"
        )
        .is_ok());
        assert!(validate(
            metadata_candidate("2.0.2", "arm64-v8a"),
            UpdateChannel::Stable,
            "aarch64"
        )
        .is_ok());
        assert!(validate(
            metadata_candidate(VERSION, "arm64-v8a"),
            UpdateChannel::Stable,
            "aarch64"
        )
        .is_err());
        assert!(validate(
            metadata_candidate("2.0.2", "arm64-v8a"),
            UpdateChannel::Beta,
            "aarch64"
        )
        .is_err());
        assert!(validate(
            metadata_candidate("2.0.2-rc.1", "arm64-v8a"),
            UpdateChannel::Beta,
            "aarch64"
        )
        .is_err());
        assert!(validate(
            metadata_candidate(VERSION, "x86_64"),
            UpdateChannel::Beta,
            "aarch64"
        )
        .is_err());
        assert!(validate(
            metadata_candidate(VERSION, "arm64-v8a"),
            UpdateChannel::Beta,
            "unknown"
        )
        .is_err());
        let mut wrong = metadata_candidate(VERSION, "arm64-v8a");
        wrong.version = "2.0.2-Beta.6+build.different".into();
        assert!(validate(wrong, UpdateChannel::Beta, "aarch64").is_err());
        for url in [
            "http://dl.aeoform.com/releases/fixture.apk",
            "https://dl.aeoform.com.attacker.test/releases/fixture.apk",
            "https://github.com/another/repo/releases/download/v1/fixture.apk",
        ] {
            let mut wrong = metadata_candidate(VERSION, "arm64-v8a");
            wrong.url = url.into();
            assert!(validate(wrong, UpdateChannel::Beta, "aarch64").is_err());
        }
    }

    #[tokio::test]
    async fn signature_failure_tries_only_same_version_alternative() {
        let mut fast = candidate("github.com", VERSION);
        fast.signature = TEST_SIGNATURE.replace("file:test", "file:tampered");
        let slow = candidate("dl.aeoform.com", VERSION);
        let older = candidate("older.invalid", "2.0.2-Beta.5");
        let probed = RefCell::new(Vec::new());
        let attempts = RefCell::new(Vec::new());
        let resets = RefCell::new(Vec::new());
        let result = download_selected(
            VERSION,
            fast.clone(),
            Ok(vec![fast, slow.clone(), older]),
            &|done, total| resets.borrow_mut().push((done, total)),
            |url| {
                probed.borrow_mut().push(url.clone());
                std::future::ready(Some(if url.contains("github.com") {
                    (0.01, 100_000_000.0)
                } else {
                    (0.1, 100_000_000.0)
                }))
            },
            |candidate| {
                attempts.borrow_mut().push(candidate.url);
                std::future::ready(Ok(b"test".to_vec()))
            },
            verify_fixture,
        )
        .await
        .unwrap();
        assert_eq!(result, b"test");
        assert_eq!(attempts.borrow().len(), 2);
        assert!(attempts.borrow()[0].contains("github.com"));
        assert_eq!(attempts.borrow()[1], slow.url);
        assert_eq!(probed.borrow().len(), 2);
        assert!(probed
            .borrow()
            .iter()
            .all(|url| !url.contains("older.invalid")));
        assert_eq!(
            *resets.borrow(),
            vec![(0, Some(10_000_000)), (0, Some(10_000_000))]
        );
    }

    #[tokio::test]
    async fn newest_github_candidate_never_probes_stale_cdn() {
        let selected = candidate("github.com", VERSION);
        let old = candidate("dl.aeoform.com", "2.0.2-Beta.4");
        let probed = RefCell::new(Vec::new());
        let attempts = RefCell::new(Vec::new());
        let error = download_selected(
            VERSION,
            selected.clone(),
            Ok(vec![old, selected]),
            &|_, _| {},
            |url| {
                probed.borrow_mut().push(url);
                std::future::ready(Some((0.1, 1000.0)))
            },
            |candidate| {
                attempts.borrow_mut().push(candidate.url);
                std::future::ready(Err("offline".into()))
            },
            verify_fixture,
        )
        .await
        .unwrap_err();
        assert!(error.contains("offline"));
        assert_eq!(probed.borrow().len(), 1);
        assert_eq!(attempts.borrow().len(), 1);
        assert!(probed.borrow()[0].contains("github.com"));
    }

    #[tokio::test]
    async fn source_discovery_errors_preserve_users_original_candidate() {
        let original = candidate("dl.aeoform.com", VERSION);
        let attempts = RefCell::new(Vec::new());
        let result = download_selected(
            VERSION,
            original.clone(),
            Err("both metadata sources offline".into()),
            &|_, _| {},
            |_| std::future::ready(None),
            |candidate| {
                attempts
                    .borrow_mut()
                    .push((candidate.url, candidate.signature));
                std::future::ready(Ok(b"test".to_vec()))
            },
            verify_fixture,
        )
        .await
        .unwrap();
        assert_eq!(result, b"test");
        assert_eq!(*attempts.borrow(), vec![(original.url, original.signature)]);
    }

    #[tokio::test]
    async fn estimated_finish_time_outweighs_first_byte_latency() {
        let low_latency = candidate("github.com", VERSION);
        let high_throughput = candidate("dl.aeoform.com", VERSION);
        let attempts = RefCell::new(Vec::new());
        let result = download_selected(
            VERSION,
            low_latency.clone(),
            Ok(vec![low_latency, high_throughput.clone()]),
            &|_, _| {},
            |url| {
                std::future::ready(Some(if url.contains("github.com") {
                    (0.01, 1000.0)
                } else {
                    (0.8, 20_000_000.0)
                }))
            },
            |candidate| {
                attempts.borrow_mut().push(candidate.url);
                std::future::ready(Ok(b"test".to_vec()))
            },
            verify_fixture,
        )
        .await
        .unwrap();
        assert_eq!(result, b"test");
        assert_eq!(*attempts.borrow(), vec![high_throughput.url]);
    }

    #[tokio::test]
    async fn corrupted_bytes_cannot_escape_when_all_sources_fail_authentication() {
        let original = candidate("github.com", VERSION);
        let alternate = candidate("dl.aeoform.com", VERSION);
        let error = download_selected(
            VERSION,
            original.clone(),
            Ok(vec![original, alternate]),
            &|_, _| {},
            |_| std::future::ready(None),
            |_| std::future::ready(Ok(b"modified".to_vec())),
            verify_fixture,
        )
        .await
        .unwrap_err();
        assert!(error.contains("signature verify failed"));
    }
    #[tokio::test]
    async fn cdn_metadata_reader_bounds_announced_and_chunked_bodies() {
        let untouched = futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<Vec<u8>, &'static str>>> {
                panic!("oversized declared body must not be consumed")
            },
        );
        assert!(
            manifest_bytes(untouched, Some(MAX_MANIFEST_BYTES as u64 + 1))
                .await
                .unwrap_err()
                .contains("size limit")
        );
        let oversized = futures_util::stream::iter([
            Ok::<_, &str>(vec![0; MAX_MANIFEST_BYTES / 2]),
            Ok(vec![0; MAX_MANIFEST_BYTES / 2 + 1]),
        ]);
        assert!(manifest_bytes(oversized, None)
            .await
            .unwrap_err()
            .contains("size limit"));
        let exact = futures_util::stream::iter([Ok::<_, &str>(vec![0; MAX_MANIFEST_BYTES])]);
        assert_eq!(
            manifest_bytes(exact, Some(MAX_MANIFEST_BYTES as u64))
                .await
                .unwrap()
                .len(),
            MAX_MANIFEST_BYTES
        );
        let interrupted = futures_util::stream::iter([
            Ok(b"{".to_vec()),
            Err("secret url query must not escape"),
        ]);
        assert_eq!(
            manifest_bytes(interrupted, None).await.unwrap_err(),
            "CDN update manifest interrupted"
        );
    }
}
