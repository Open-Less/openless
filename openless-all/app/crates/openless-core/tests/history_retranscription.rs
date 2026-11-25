use openless_core::testing::{FixtureTextPolisher, FixtureTranscriptionEngine};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, DictationSession,
    InMemoryCredentialStore, OpenLessBackend,
};
use std::sync::Arc;

// The same command implementation used by Tauri, with only its archive path supplied locally.
#[path = "../../../src-tauri/src/commands/history_retranscription.rs"]
mod command;

#[tokio::test]
async fn history_command_decodes_audio_repairs_legacy_empty_failure_and_preserves_errors() {
    let root = std::env::temp_dir().join(format!("openless-retranscribe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    for outcome in [
        "success",
        "empty",
        "provider",
        "corrupt",
        "missing",
        "completed",
    ] {
        let dir = root.join(outcome);
        let transcription = Arc::new(if outcome == "provider" {
            FixtureTranscriptionEngine::failing(BackendError::new(
                BackendErrorCode::Provider,
                "untrusted provider body: secret-marker",
            ))
        } else {
            FixtureTranscriptionEngine::successful(
                if outcome == "empty" { "" } else { "recovered" },
                938,
            )
        });
        let polisher = Arc::new(FixtureTextPolisher::successful("must not polish"));
        let mut deps = BackendDependencies::unsupported();
        deps.credential_store = Arc::new(InMemoryCredentialStore::default());
        deps.services
            .configure_auxiliary_runtime(polisher.clone(), transcription.clone());
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: dir.clone(),
                ..Default::default()
            },
            deps,
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut entry: DictationSession = serde_json::from_value(serde_json::json!({
            "id": id, "createdAt": "2026-11-23T04:28:16Z", "rawTranscript": "",
            "finalText": "", "mode": "structured", "insertStatus": "failed",
            "errorCode": "polishFailed", "durationMs": 720, "hasAudioRecording": true,
            "asrProvider": "volcengine", "llmProvider": "ark", "llmModel": "deepseek-flash",
            "polishMs": 0, "pipelineMode": "traditional"
        }))
        .unwrap();
        if outcome == "completed" {
            entry.raw_transcript = "original raw".into();
            entry.final_text = "original final".into();
            entry.error_code = None;
        }
        backend.append_history(entry.clone(), 0, None).unwrap();
        let pcm = vec![1, 1, 1, 2, 1, 3];
        let mut wav = openless_core::audio::encode_dictation_wav(&pcm).unwrap();
        // A legal metadata chunk must not be passed to ASR as sound.
        wav.splice(36..36, *b"JUNK\x02\0\0\0xx");
        let size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&size.to_le_bytes());
        if outcome == "corrupt" {
            wav.truncate(45);
        }
        let path = dir.join("recording.wav");
        if outcome != "missing" {
            std::fs::write(&path, &wav).unwrap();
        }
        let result =
            command::retranscribe_recording_with_path(&backend, id.clone(), |_| Ok(path.clone()))
                .await;
        match outcome {
            "success" => {
                let result = result.unwrap();
                assert_eq!(result.text, "recovered");
                let updated = result.updated_entry.unwrap();
                assert_eq!(updated.id, id);
                assert_eq!(updated.raw_transcript, "recovered");
                assert!(updated.error_code.is_none());
                assert!(updated.llm_provider.is_none());
                assert_eq!(backend.list_history().unwrap(), [updated]);
            }
            "completed" => {
                let result = result.unwrap();
                assert_eq!(result.text, "recovered");
                assert!(result.updated_entry.is_none());
                assert_eq!(backend.list_history().unwrap(), [entry]);
            }
            _ => {
                let expected = match outcome {
                    "empty" => "historyRetranscribeNoSpeech",
                    "provider" => "historyRetranscribeProvider",
                    "corrupt" => "historyRetranscribeInvalidAudio",
                    "missing" => "historyRetranscribeMissing",
                    _ => unreachable!(),
                };
                assert_eq!(result.err().unwrap(), expected);
                assert_eq!(backend.list_history().unwrap(), [entry]);
            }
        }
        assert!(polisher.inputs().is_empty());
        assert_eq!(
            transcription.pcm(),
            if matches!(outcome, "corrupt" | "missing") {
                vec![]
            } else {
                pcm
            }
        );
        if outcome != "missing" {
            assert_eq!(std::fs::read(&path).unwrap(), wav);
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
