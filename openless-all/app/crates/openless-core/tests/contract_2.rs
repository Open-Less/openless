use openless_core::{
    require_backend_contract_version, BackendEvent, BackendSnapshot, CredentialsStatus,
    DictationInsertStatus, DictationStateSnapshot, RemoteAuthResult, StartupSnapshot,
    BACKEND_CONTRACT_VERSION,
};

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../contract/backend-2.0.json"))
        .expect("canonical backend contract fixture must be valid JSON")
}

#[test]
fn startup_and_enum_wire_shapes_match_the_canonical_contract() {
    let fixture = fixture();
    assert_eq!(fixture["contractVersion"], BACKEND_CONTRACT_VERSION);
    assert_eq!(
        serde_json::to_value(StartupSnapshot {
            contract_version: BACKEND_CONTRACT_VERSION.to_string(),
            backend: BackendSnapshot {
                running: true,
                dictation: DictationStateSnapshot::default(),
                vocabulary_revision: 0,
                history_revision: 0,
                style_pack_revision: 0,
                preferences_revision: 0,
                credentials: CredentialsStatus::default(),
            },
        })
        .unwrap()["contractVersion"],
        fixture["startupSnapshot"]["sample"]["contractVersion"]
    );
    let startup: StartupSnapshot =
        serde_json::from_value(fixture["startupSnapshot"]["sample"].clone()).unwrap();
    assert_eq!(startup.contract_version, BACKEND_CONTRACT_VERSION);
    assert_eq!(
        serde_json::to_value(startup).unwrap(),
        fixture["startupSnapshot"]["sample"],
        "startup fixtures must include every serialized snapshot field"
    );
    assert_eq!(
        fixture["androidJni"]["sample"]["payload"], fixture["startupSnapshot"]["sample"]["backend"],
        "JNI and startup must use the same complete backend snapshot"
    );
    assert_eq!(
        serde_json::to_value([
            DictationInsertStatus::Inserted,
            DictationInsertStatus::PasteSent,
            DictationInsertStatus::CopiedFallback,
            DictationInsertStatus::NotRequested,
        ])
        .unwrap(),
        fixture["enums"]["insertStatus"]
    );
    assert_eq!(
        serde_json::to_value([
            RemoteAuthResult::Ok,
            RemoteAuthResult::BadPin,
            RemoteAuthResult::Locked,
        ])
        .unwrap(),
        fixture["enums"]["remoteAuthResult"]
    );
}

#[test]
fn runtime_wire_rejects_non_2_contracts() {
    require_backend_contract_version(BACKEND_CONTRACT_VERSION).unwrap();
    assert!(require_backend_contract_version("1.0.0").is_err());
    assert!(require_backend_contract_version("2.1.0").is_err());
}

#[test]
fn less_computer_voice_feedback_matches_the_shared_wire_contract() {
    use openless_core::{LessComputerEvent, LessComputerEventKind, LessComputerVoicePhase};
    let fixture = fixture();
    let event: LessComputerEvent =
        serde_json::from_value(fixture["lessComputerVoice"]["sample"].clone()).unwrap();
    assert!(matches!(
        event.kind,
        LessComputerEventKind::VoiceState {
            phase: LessComputerVoicePhase::Recording,
            level: 0.5,
            elapsed_ms: 120,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        fixture["lessComputerVoice"]["sample"]
    );
    assert_eq!(
        serde_json::to_value([
            LessComputerVoicePhase::Starting,
            LessComputerVoicePhase::Recording,
            LessComputerVoicePhase::Transcribing,
            LessComputerVoicePhase::Idle,
        ])
        .unwrap(),
        fixture["lessComputerVoice"]["phases"]
    );
}

#[test]
fn every_core_event_has_a_canonical_camel_case_round_trip_fixture() {
    let fixture = fixture();
    let expected = fixture["backendEvent"]["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|kind| kind.as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let samples = fixture["backendEvent"]["samples"].as_object().unwrap();
    let actual = samples
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected);

    for (kind, sample) in samples {
        assert_camel_case_fields(sample);
        let event: BackendEvent = serde_json::from_value(sample.clone())
            .unwrap_or_else(|error| panic!("invalid {kind} event fixture: {error}"));
        assert_eq!(serde_json::to_value(event).unwrap(), *sample, "{kind}");
    }
}

fn assert_camel_case_fields(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                assert!(
                    !key.contains('_') && key.chars().next().is_some_and(char::is_lowercase),
                    "contract field is not camelCase: {key}"
                );
                assert_camel_case_fields(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                assert_camel_case_fields(value);
            }
        }
        _ => {}
    }
}

#[test]
fn linux_facade_fields_follow_the_production_core_dtos() {
    let fixture = fixture();
    let startup_fields = fixture["startupSnapshot"]["sample"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let linux_startup_fields = fixture["linuxFacade"]["startupFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field.as_str().unwrap().to_string())
        .collect();
    assert_eq!(startup_fields, linux_startup_fields);
    let event_fields = fixture["backendEvent"]["samples"]["backend_started"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let linux_event_fields = fixture["linuxFacade"]["eventFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field.as_str().unwrap().to_string())
        .collect();
    assert_eq!(event_fields, linux_event_fields);
}

#[tokio::test]
async fn core_streams_polish_deltas_through_one_insertion_session() {
    use std::sync::Arc;

    use openless_core::shared_types::WindowsInsertionMode;
    use openless_core::testing::{
        FixtureDictationEngine, FixtureInsertionAction, FixtureTextInserter,
    };
    use openless_core::{
        BackendConfig, BackendDependencies, InMemoryCredentialStore, InsertOutcome,
        NoopSettingsRuntime, PolishDelta, SettingsUpdateOptions, TokioTaskSpawner,
    };

    let data_dir = std::env::temp_dir().join(format!(
        "openless-contract-streaming-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let inserter = FixtureTextInserter::with_outcome(InsertOutcome::Inserted);
    let backend = openless_core::OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        BackendDependencies {
            text_inserter: Arc::new(inserter.clone()),
            dictation_engine: Arc::new(
                FixtureDictationEngine::successful("你好", "你好").with_polish_deltas(vec![
                    PolishDelta {
                        text: "你".to_string(),
                        offset: 0,
                        is_final: false,
                    },
                    PolishDelta {
                        text: "好".to_string(),
                        offset: 1,
                        is_final: false,
                    },
                ]),
            ),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(InMemoryCredentialStore::default()),
            ..BackendDependencies::unsupported()
        },
    )
    .unwrap();
    let mut preferences = backend.get_preferences();
    preferences.windows_insertion_mode = WindowsInsertionMode::SendInput;
    backend
        .update_settings(
            preferences,
            SettingsUpdateOptions::STRICT,
            &NoopSettingsRuntime,
        )
        .unwrap();
    backend.start().await.unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();

    assert!(inserter.actions().iter().any(|action| matches!(
        action,
        FixtureInsertionAction::Write { text, .. } if text == "你好"
    )));
    assert!(inserter.actions().iter().any(|action| matches!(
        action,
        FixtureInsertionAction::Insert { text, .. } if text.is_empty()
    )));
    let _ = std::fs::remove_dir_all(data_dir);
}
