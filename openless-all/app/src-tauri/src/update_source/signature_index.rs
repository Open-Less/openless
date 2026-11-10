//! Release metadata only. The embedded minisign signature must still verify the payload.
pub(super) const NAME: &str = "updater-signatures.json";
pub(super) const LIMIT: usize = 128 * 1024;

pub(super) fn extract(body: &str, tag: &str, artifact: &str, indexed: bool) -> Result<String, String> {
    if body.len() > LIMIT { return Err("signature metadata exceeds size limit".into()); }
    if !indexed { return Ok(body.trim().to_string()); }
    let index: serde_json::Value = serde_json::from_str(body).map_err(|_| "invalid signature index")?;
    if index["schema"].as_u64() != Some(1) || index["release_tag"].as_str() != Some(tag) {
        return Err("signature index schema/tag mismatch".into());
    }
    index["artifacts"][artifact]["signature"].as_str().map(str::trim)
        .filter(|value| !value.is_empty()).map(str::to_owned)
        .ok_or_else(|| "artifact missing from signature index".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_and_legacy_signatures() {
        let body = r#"{"schema":1,"release_tag":"v2-tauri","artifacts":{"app.exe":{"signature":" signed "}}}"#;
        assert_eq!(extract(body, "v2-tauri", "app.exe", true).unwrap(), "signed");
        assert!(extract(body, "v3-tauri", "app.exe", true).is_err());
        assert!(extract(body, "v2-tauri", "other.exe", true).is_err());
        assert!(extract("{}", "v2-tauri", "app.exe", true).is_err());
        assert!(extract(&"x".repeat(LIMIT + 1), "v2-tauri", "app.exe", true).is_err());
        assert_eq!(extract(" old-signature\n", "v1", "app", false).unwrap(), "old-signature");
    }
}
