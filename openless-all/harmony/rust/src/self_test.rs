//! Exercises the real wire dispatcher against an isolated, disposable sandbox.
use crate::{credentials::HarmonyCredentials, dispatch, Host};
use openless_core::{BackendConfig, BackendDependencies, OpenLessBackend};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn styles(owner: &Host) -> Result<Value, String> {
    let root = owner.backend.config().cache_dir.join(format!(
        "native-selftest-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&root).map_err(|e| e.to_string())?;
    let result = (|| {
        let data = root.join("data");
        std::fs::create_dir(&data).map_err(|e| e.to_string())?;
        let credentials = Arc::new(HarmonyCredentials::new(data.join("channels.json")));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        let config = BackendConfig {
            data_dir: data,
            cache_dir: root.clone(),
            platform: crate::capabilities(),
            ..Default::default()
        };
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let backend =
            OpenLessBackend::new(config.clone(), dependencies).map_err(|e| e.to_string())?;
        runtime
            .block_on(backend.start())
            .map_err(|e| e.to_string())?;
        let test = Host {
            runtime: Arc::new(runtime),
            backend: Arc::new(backend),
            credentials,
        };
        let channel = dispatch(
            &test,
            "create_channel",
            json!({"kind":"asr","providerType":"openai","name":"isolated test"}),
        )?;
        let channel_id = channel.as_str().ok_or("missing channel id")?;
        dispatch(
            &test,
            "record_channel_test",
            json!({"kind":"asr","id":channel_id,"ok":false,"latencyMs":23,"error":"fixture failure"}),
        )?;
        let channels = dispatch(&test, "list_channels", json!({"kind":"asr"}))?;
        if !channels.as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["id"] == channel_id
                    && item["lastTest"]["ok"] == false
                    && item["lastTest"]["latencyMs"] == 23
            })
        }) {
            return Err("channel test result was not persisted".into());
        }
        // Exercise shared IPC argument names and persistence without touching user data.
        dispatch(&test, "add_learned_vocab", json!({"phrase":"鸿蒙接线测试"}))?;
        dispatch(&test, "add_learned_vocab", json!({"phrase":"鸿蒙接线测试"}))?;
        let vocab = dispatch(&test, "list_vocab", json!({}))?;
        if vocab.as_array().ok_or("invalid vocab list")?.len() != 1 {
            return Err("learned vocabulary did not deduplicate".into());
        }
        let rule = dispatch(
            &test,
            "add_correction_rule",
            json!({"pattern":"测试错词","replacement":"测试正词"}),
        )?;
        let rule_id = rule["id"].as_str().ok_or("missing rule id")?;
        dispatch(
            &test,
            "set_correction_rule_enabled",
            json!({"id":rule_id,"enabled":false}),
        )?;
        let rules = dispatch(&test, "list_correction_rules", json!({}))?;
        if rules[0]["enabled"] != json!(false) {
            return Err("correction rule disable did not persist".into());
        }
        dispatch(&test, "remove_correction_rule", json!({"id":rule_id}))?;
        if dispatch(&test, "list_correction_rules", json!({}))? != json!([]) {
            return Err("correction rule delete failed".into());
        }
        let mut presets = dispatch(&test, "list_vocab_presets", json!({}))?;
        presets["disabledBuiltinPresetIds"] = json!(["harmony-selftest"]);
        dispatch(&test, "save_vocab_presets", json!({"store":presets}))?;
        if dispatch(&test, "list_vocab_presets", json!({}))? != presets {
            return Err("vocabulary presets roundtrip failed".into());
        }
        let list = dispatch(&test, "list_style_packs", json!({}))?;
        let mut template = list
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or("no builtin styles")?;
        template["name"] = json!("Harmony isolated self-test");
        let mut created = dispatch(
            &test,
            "create_style_pack_from_template",
            json!({"template":template}),
        )?;
        let id = created["id"].as_str().ok_or("missing style id")?.to_owned();
        created["prompt"] = json!("Harmony persistence and ZIP roundtrip test.");
        dispatch(&test, "save_style_pack", json!({"stylePack":created}))?;
        dispatch(
            &test,
            "preview_style_pack_runtime",
            json!({"stylePack":created}),
        )?;
        dispatch(&test, "set_active_style_pack", json!({"id":id}))?;
        if test.backend.get_preferences().active_style_pack_id != id {
            return Err("activation did not update preferences".into());
        }
        dispatch(
            &test,
            "set_style_pack_enabled",
            json!({"id":id,"enabled":false}),
        )?;
        let zip = root.join("test.zip");
        dispatch(
            &test,
            "export_style_pack_to_zip",
            json!({"id":id,"targetPath":zip}),
        )?;
        let imported = dispatch(&test, "import_style_pack_from_zip", json!({"zipPath":zip}))?;
        if imported["prompt"] != created["prompt"] {
            return Err("ZIP roundtrip changed prompt".into());
        }
        let imported_id = imported["id"]
            .as_str()
            .ok_or("missing imported id")?
            .to_owned();
        dispatch(&test, "delete_style_pack", json!({"id":id}))?;
        if test.backend.get_style_pack(&id).is_ok() {
            return Err("deleted style still present".into());
        }
        let reopened = OpenLessBackend::new(config, BackendDependencies::unsupported())
            .map_err(|e| e.to_string())?;
        if reopened.list_vocabulary().map_err(|e| e.to_string())?.len() != 1
            || json!(reopened
                .list_vocabulary_presets()
                .map_err(|e| e.to_string())?)
                != presets
        {
            return Err("vocabulary data did not survive repository reopen".into());
        }
        if reopened
            .get_style_pack(&imported_id)
            .map_err(|e| e.to_string())?
            .prompt
            != created["prompt"].as_str().ok_or("missing prompt")?
        {
            return Err("saved prompt did not survive repository reopen".into());
        }
        Ok(
            json!({"create":true,"save":true,"preview":true,"activate":true,"enabled":true,"export":true,"import":true,"delete":true,"persistence":true,"learnedVocabulary":true,"correctionRules":true,"vocabularyPresets":true}),
        )
    })();
    if result.is_ok() {
        std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
    }
    result
}

/// Verifies the host's actual dictation dispatcher with fixture providers only.
/// This deliberately has no microphone, network access or user credentials.
pub fn voice(owner: &Host) -> Result<Value, String> {
    use openless_core::{credentials::*, testing::*, SessionId};
    let root = owner.backend.config().cache_dir.join(format!(
        "voice-selftest-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let result = (|| {
        let transcription = Arc::new(FixtureTranscriptionEngine::successful("语音管线原文", 1000));
        let polisher = Arc::new(FixtureTextPolisher::successful("语音管线结果"));
        let mut dependencies = BackendDependencies::unsupported();
        let fixture_credentials = Arc::new(InMemoryCredentialStore::default());
        let runtime = Arc::new(tokio::runtime::Runtime::new().map_err(|e| e.to_string())?);
        runtime
            .block_on(
                fixture_credentials.write(
                    CredentialKey::new(
                        CredentialNamespace::Llm,
                        Some("ark".into()),
                        LLM_API_KEY_ACCOUNT,
                    )
                    .map_err(|e| e.to_string())?,
                    SecretValue::new("isolated-fixture"),
                ),
            )
            .map_err(|e| e.to_string())?;
        dependencies.credential_store = fixture_credentials.clone();
        dependencies.services.configure_auxiliary_runtime(
            Arc::new(openless_core::SharedAuxiliaryTextPolisher::new(
                fixture_credentials,
                polisher.clone(),
            )),
            transcription.clone(),
        );
        dependencies.dictation_engine = Arc::new(openless_core::PipelineDictationEngine::new(
            Arc::new(
                openless_core::ExternalAudioRecorder::with_recordings_directory(
                    root.join("data/recordings"),
                ),
            ),
            transcription.clone(),
            polisher.clone(),
        ));
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: root.join("data"),
                    cache_dir: root.clone(),
                    platform: crate::capabilities(),
                    ..Default::default()
                },
                dependencies,
            )
            .map_err(|e| e.to_string())?,
        );
        runtime
            .block_on(backend.start())
            .map_err(|e| e.to_string())?;
        let mut prefs = backend.get_preferences();
        prefs.record_audio_for_debug = true;
        {
            let _entered = runtime.enter();
            backend
                .update_settings(
                    prefs,
                    openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                    &openless_core::settings::NoopSettingsRuntime,
                )
                .map_err(|e| e.to_string())?;
        }
        let test = Host {
            runtime,
            backend,
            credentials: Arc::new(HarmonyCredentials::new(root.join("channels.json"))),
        };
        let session = dispatch(&test, "start_dictation", json!({}))?;
        let id: SessionId = serde_json::from_value(session.clone()).map_err(|e| e.to_string())?;
        let pcm = vec![1u8; 32000];
        test.backend
            .feed_external_pcm(id, &pcm)
            .map_err(|e| e.to_string())?;
        let result = dispatch(&test, "stop_dictation", json!({"session":session}))?;
        if result["rawText"] != "语音管线原文"
            || result["polishedText"] != "语音管线结果"
            || result["inserted"] != json!("notRequested")
            || transcription.pcm() != pcm
        {
            return Err("voice PCM and preview roundtrip failed".into());
        }
        if test.backend.feed_external_pcm(id, &[1, 0]).is_ok() {
            return Err("completed session accepted stale audio".into());
        }
        dispatch(&test, "cancel_dictation", json!({"session":id.to_string()}))?;
        let audio = dispatch(
            &test,
            "read_audio_recording",
            json!({"sessionId":id.to_string()}),
        )?;
        if !audio
            .as_str()
            .is_some_and(|value| value.starts_with("data:audio/wav;base64,UklGR"))
        {
            return Err("archived audio was not returned as WAV".into());
        }
        if dispatch(
            &test,
            "read_audio_recording",
            json!({"sessionId":"../channels"}),
        )
        .is_ok()
        {
            return Err("recording path escaped session UUID validation".into());
        }
        let history_before = test.backend.list_history().map_err(|e| e.to_string())?;
        let retried = dispatch(
            &test,
            "retranscribe_recording",
            json!({"sessionId":id.to_string()}),
        )?;
        if retried["text"] != "语音管线原文"
            || !retried["updatedEntry"].is_null()
            || json!(history_before)
                != json!(test.backend.list_history().map_err(|e| e.to_string())?)
        {
            return Err("retranscription overwrote completed history".into());
        }
        if dispatch(&test, "repolish", json!({"rawText":"再次润色"}))? != "语音管线结果" {
            return Err("history repolish did not use the shared runtime".into());
        }
        let next = dispatch(&test, "start_dictation", json!({}))?;
        let next_id: SessionId = serde_json::from_value(next.clone()).map_err(|e| e.to_string())?;
        if dispatch(&test, "cancel_dictation", json!({"session":id.to_string()})).is_ok() {
            return Err("stale cancellation targeted a newer session".into());
        }
        if test.backend.feed_external_pcm(id, &[1, 0]).is_ok() {
            return Err("new session accepted previous session audio".into());
        }
        dispatch(&test, "cancel_dictation", json!({"session":next}))?;
        dispatch(&test, "cancel_dictation", json!({"session":next}))?;
        if test.backend.feed_external_pcm(next_id, &[1, 0]).is_ok()
            || transcription.cancel_count() == 0
        {
            return Err("cancelled session still accepts audio".into());
        }
        dispatch(&test, "delete_history_entry", json!({"id":id.to_string()}))?;
        if test
            .backend
            .config()
            .data_dir
            .join("recordings")
            .join(format!("{id}.wav"))
            .exists()
        {
            return Err("history deletion left archived audio behind".into());
        }
        test.runtime
            .block_on(test.backend.shutdown())
            .map_err(|e| e.to_string())?;
        Ok(
            json!({"fixtureProviders":true,"pcm":true,"preview":true,"noAutoInsert":true,
                "cancel":true,"staleAudioRejected":true,"audioRead":true,"retranscribe":true,"repolish":true}),
        )
    })();
    if result.is_ok() {
        std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn offline_voice_dispatch_and_session_boundaries() {
        let root = std::env::temp_dir().join(format!("openless-host-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
        let host = crate::build_host(root.join("data"), root.join("cache"), runtime).unwrap();
        assert_eq!(super::voice(&host).unwrap()["staleAudioRejected"], true);
        assert_eq!(super::styles(&host).unwrap()["persistence"], true);
        host.runtime.block_on(host.backend.shutdown()).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
