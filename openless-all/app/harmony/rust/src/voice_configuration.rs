//! The IME has its own sandbox. Exchange only voice settings through the
//! app-scoped secure vault; never assume identical sandbox paths share files.
use super::*;

const FIELDS: &[&str] = &[
    "pipelineMode",
    "multimodalPipelineEnabled",
    "activeAsrProvider",
    "activeLlmProvider",
    "activeOmniProvider",
    "microphoneDeviceName",
    "defaultMode",
    "activeStylePackId",
    "workingLanguages",
    "translationTargetLanguage",
    "chineseScriptPreference",
    "outputLanguagePreference",
    "llmThinkingEnabled",
    "polishSkipMaxChars",
    "stableTranscriptionEnabled",
    "audioEnhancementEnabled",
    "audioDsp",
    "recordAudioForDebug",
];

pub(super) fn export(host: &Host) -> Result<Value, String> {
    let all = json!(host.backend.get_preferences());
    let preferences: serde_json::Map<String, Value> = FIELDS
        .iter()
        .filter_map(|key| all.get(*key).map(|value| ((*key).into(), value.clone())))
        .collect();
    let path = host.backend.config().data_dir.join("channels.json");
    let channels: openless_core::credentials::CredentialMetadata = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(error.to_string()),
    };
    Ok(json!({"version":1,"preferences":preferences,"channels":channels}))
}

pub(super) fn import(host: &Host, snapshot: Value) -> Result<Value, String> {
    if !matches!(
        host.backend.snapshot().dictation.phase,
        openless_core::DictationPhase::Idle
            | openless_core::DictationPhase::Completed
            | openless_core::DictationPhase::Cancelled
            | openless_core::DictationPhase::Failed
    ) {
        return Err("voice backend is busy".into());
    }
    if snapshot["version"] != 1 {
        return Err("unsupported voice configuration version".into());
    }
    let fields = snapshot["preferences"]
        .as_object()
        .ok_or("voice preferences required")?;
    if fields.keys().any(|key| !FIELDS.contains(&key.as_str())) {
        return Err("unsupported voice preference".into());
    }
    let channels: openless_core::credentials::CredentialMetadata =
        serde_json::from_value(snapshot["channels"].clone()).map_err(|e| e.to_string())?;
    let directory = &host.backend.config().data_dir;
    let marker = directory.join("ime-voice-configuration.json");
    if std::fs::read(&marker)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .as_ref()
        == Some(&snapshot)
    {
        return Ok(json!(false));
    }
    let mut preferences = json!(host.backend.get_preferences());
    for (key, value) in fields {
        preferences[key] = value.clone();
    }
    let preferences = serde_json::from_value(preferences).map_err(|e| e.to_string())?;
    match std::fs::remove_file(&marker) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    // Each file is atomically replaced; the marker is committed last so an
    // interrupted import is retried before any recording can start.
    let temporary = directory.join("ime-voice-channels.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec(&channels).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, directory.join("channels.json")).map_err(|e| e.to_string())?;
    let _entered = host.runtime.enter();
    host.backend
        .update_settings(
            preferences,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &openless_core::settings::NoopSettingsRuntime,
        )
        .map_err(|e| e.to_string())?;
    let temporary = directory.join("ime-voice-configuration.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, marker).map_err(|e| e.to_string())?;
    Ok(json!(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_configuration_crosses_independent_sandboxes_before_recording() {
        let root = std::env::temp_dir().join(format!(
            "openless-voice-sandboxes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let main = build_host(root.join("main"), root.join("main-cache"), runtime.clone()).unwrap();
        let ime = build_host(root.join("ime"), root.join("ime-cache"), runtime.clone()).unwrap();
        dispatch(
            &main,
            "set_active_omni_provider",
            json!({"provider":"dashscope-omni"}),
        )
        .unwrap();
        let mut prefs = json!(main.backend.get_preferences());
        prefs["pipelineMode"] = json!("multimodal");
        prefs["multimodalPipelineEnabled"] = json!(true);
        dispatch(&main, "set_settings", json!({"prefs":prefs})).unwrap();
        assert_eq!(
            dispatch(&ime, "get_credentials", json!({})).unwrap()["pipelineMode"],
            "traditional"
        );
        let snapshot = export(&main).unwrap();
        assert!(snapshot["preferences"]
            .get("syncCustomServerOrigin")
            .is_none());
        assert_eq!(import(&ime, snapshot.clone()).unwrap(), true);
        assert_eq!(import(&ime, snapshot.clone()).unwrap(), false);
        // Reload just like VoiceClient, then restore legacy vault aliases. The
        // main app's provider metadata must already be present at this point.
        let restored = build_host(root.join("ime"), root.join("ime-cache"), runtime).unwrap();
        dispatch(
            &restored,
            "__hydrate_credential",
            json!({"account":"omni.api_key","value":"fixture-only"}),
        )
        .unwrap();
        let status = dispatch(&restored, "get_credentials", json!({})).unwrap();
        assert_eq!(status["pipelineMode"], "multimodal");
        assert_eq!(status["omniConfigured"], true);
        let session = dispatch(&restored, "start_dictation", json!({})).unwrap();
        dispatch(&restored, "cancel_dictation", json!({"session":session})).unwrap();
        dispatch(&ime, "set_default_polish_mode", json!({"mode":"raw"})).unwrap();
        assert_eq!(
            import(&ime, snapshot.clone()).unwrap(),
            false,
            "unchanged main settings preserve keyboard mode"
        );
        assert_eq!(json!(ime.backend.get_preferences())["defaultMode"], "raw");
        let mut invalid = snapshot;
        invalid["preferences"]["syncAutoSync"] = json!(true);
        assert!(import(&ime, invalid).is_err());
        for host in [main, ime, restored] {
            host.runtime.block_on(host.backend.shutdown()).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
