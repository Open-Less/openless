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
    remote > current
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
