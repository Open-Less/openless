use super::tests::{audio, event, next, reply, send};
use super::*;
use credentials::{IosCredentialStore, MemoryVault};
use openless_core::credentials::{CredentialKey, CredentialNamespace, SecretValue};

#[test]
fn audio_settings_are_available_without_developer_mode_and_persist() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(MemoryVault::default());
    let (handle, context) = open_at(directory.path(), false, storage.clone());
    let initial = result(control(handle, context, json!({"action":"audioSettings"})));
    assert_eq!(initial["enabled"], false);
    let saved = result(control(handle, context, json!({"action":"updateAudioSettings","enabled":true,"dsp":{"noiseReduction":"off","gainDb":100,"androidAecEnabled":true}})));
    assert_eq!(saved["enabled"], true);
    assert_eq!(saved["dsp"]["gainDb"], 50);
    assert_eq!(saved["dsp"]["androidAecEnabled"], false);
    ol_ios_destroy(handle);
    let (handle, context) = open_at(directory.path(), false, storage);
    assert_eq!(result(control(handle, context, json!({"action":"audioSettings"}))), saved);
    ol_ios_destroy(handle);
}

#[test]
fn native_capture_enhancement_and_stop_flush_match_shared_processor() {
    #[derive(Default)]
    struct Sink(Mutex<Vec<u8>>);
    impl AudioConsumer for Sink {
        fn consume_pcm_chunk(&self, pcm: &[u8]) { self.0.lock().unwrap().extend_from_slice(pcm); }
    }
    impl RecordingProgressSink for Sink {
        fn publish_level(&self, _: u64, _: f32) -> Result<(), BackendError> { Ok(()) }
    }
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), false, Arc::new(MemoryVault::default()));
    let owner = host(handle).unwrap();
    let id = SessionId::new();
    let config = openless_core::AudioProcessingConfig {
        enhancement_enabled: true,
        echo_cancellation_enabled: false,
        dsp: openless_core::AudioDspConfig { noise_reduction: openless_core::NoiseReduction::Off, gain_db: 6, agc_enabled: false, ..Default::default() },
    };
    let sink = Arc::new(Sink::default());
    owner.broker.captures.lock().unwrap().insert(id, Arc::new(Capture {
        processing: Some(Mutex::new((openless_core::AudioProcessor::new(config), Vec::new()))),
        consumer: sink.clone(), progress: sink.clone(), bytes: AtomicU64::new(0), archive: None,
    }));
    let mut reference = openless_core::AudioProcessor::new(config);
    let mut expected = Vec::new();
    let pcm: Vec<u8> = (0..1001).flat_map(|i| (((i as f32 * 0.07).sin() * 3000.0) as i16).to_le_bytes()).collect();
    let session = CString::new(id.to_string()).unwrap();
    for bytes in pcm.chunks(640) {
        assert_eq!(unsafe { ol_ios_feed_pcm(handle, session.as_ptr(), bytes.as_ptr(), bytes.len()) }, 0);
        let samples: Vec<f32> = bytes.chunks_exact(2).map(|p| i16::from_le_bytes([p[0],p[1]]) as f32 / 32768.0).collect();
        if let Some(chunk) = reference.process(&samples, 1, 16_000) { expected.extend(chunk.pcm_i16_le); }
    }
    if let Some(chunk) = reference.finish() { expected.extend(chunk.pcm_i16_le); }
    let task = owner.executor.spawn(Box::new(Recording { broker: owner.broker.clone(), id, archive: None }).stop());
    loop {
        let request = next(context);
        if request["kind"] == "nativeRequest" {
            assert_eq!(request["operation"], "captureStop");
            reply(handle, &request, Value::Null);
            break;
        }
    }
    owner.executor.block_on(task).unwrap().unwrap();
    assert_eq!(*sink.0.lock().unwrap(), expected);
    assert_eq!(expected.len(), pcm.len());
    assert_ne!(expected, pcm);
    ol_ios_destroy(handle);
}

pub(super) fn open_at(directory: &std::path::Path, developer: bool, storage: Arc<MemoryVault>) -> (u64, u64) {
    static CONTEXT: AtomicU64 = AtomicU64::new(10_000);
    let context = CONTEXT.fetch_add(1, Ordering::Relaxed);
    let handle = create_with_store(
        Config {
            data_dir: directory.into(),
            developer_mode: developer,
        },
        Callbacks {
            event,
            audio,
            context,
        },
        Arc::new(IosCredentialStore::open(storage).unwrap()),
    )
    .unwrap();
    (handle, context)
}
pub(super) fn control(handle: u64, context: u64, action: Value) -> Value {
    let mut command = action;
    command["operation"] = json!("control");
    command["requestID"] = json!("test-request");
    assert_eq!(send(handle, command), 0);
    loop {
        let value = next(context);
        if value["kind"] == "controlResult" {
            assert_eq!(value["requestID"], "test-request");
            return value;
        }
    }
}
pub(super) fn result(value: Value) -> Value {
    assert_eq!(value["ok"], true, "{value}");
    serde_json::from_str(value["json"].as_str().unwrap()).unwrap()
}
fn save(handle: u64, context: u64, kind: &str, provider: &str, key: &str) -> String {
    result(control(handle,context,json!({"action":"saveChannel","kind":kind,"providerType":provider,"name":"Test channel","endpoint":"https://invalid.example/v1","model":"test-model","apiKey":key})))["channelID"].as_str().unwrap().into()
}

#[test]
fn config_defaults_and_control_unknown_action_are_correlated() {
    let config: Config = serde_json::from_value(json!({"dataDir":"/tmp/unused"})).unwrap();
    assert!(!config.developer_mode);
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    let initial = result(control(handle, context, json!({"action":"snapshot"})));
    assert_eq!(initial["polishEnabled"], false);
    assert_eq!(initial["preferences"]["pipelineMode"], "traditional");
    assert!(initial["channels"]["llm"].as_array().unwrap().is_empty());
    let value = control(handle, context, json!({"action":"notAnAction"}));
    assert_eq!(value["ok"], false);
    assert_eq!(value["message"], "unknown control action");
    ol_ios_destroy(handle);
}

#[test]
fn normal_mode_rejects_management_and_does_not_disclose_developer_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(MemoryVault::default());
    let (handle, context) = open_at(directory.path(), true, storage.clone());
    let channel = save(handle, context, "llm", "custom", "never-return-this-key");
    result(control(
        handle,
        context,
        json!({"action":"setActive","kind":"llm","channelID":channel}),
    ));
    save(handle, context, "omni", "custom", "omni-key");
    result(control(
        handle,
        context,
        json!({"action":"setActive","kind":"omni","channelID":"custom"}),
    ));
    result(control(
        handle,
        context,
        json!({"action":"updateSettings","pipelineMode":"multimodal","polishEnabled":true}),
    ));
    ol_ios_destroy(handle);
    // Even contaminated prefs/vault cannot enable LLM/Omni in ordinary mode.
    let (handle, context) = open_at(directory.path(), false, storage);
    let snapshot = result(control(handle, context, json!({"action":"snapshot"})));
    assert_eq!(snapshot["asr"]["providerType"], "ios-sensevoice");
    assert_eq!(snapshot["polishEnabled"], false);
    assert!(snapshot.get("preferences").is_none());
    assert!(snapshot.get("channels").is_none());
    for action in [
        "saveChannel",
        "setActive",
        "updateSettings",
        "listModels",
        "validate",
        "deleteChannel",
        "rewrite",
    ] {
        let value = control(
            handle,
            context,
            json!({"action":action,"kind":"llm","channelID":"custom","providerType":"custom","name":"X","endpoint":"","model":"","pipelineMode":"multimodal","polishEnabled":true,"text":"hi","instruction":"edit"}),
        );
        assert_eq!(value["ok"], false, "{action}");
        assert_eq!(value["message"], "developer mode required");
    }
    assert_eq!(
        host(handle)
            .unwrap()
            .backend
            .get_preferences()
            .pipeline_mode,
        openless_core::shared_types::PipelineMode::Traditional
    );
    assert_eq!(
        host(handle)
            .unwrap()
            .backend
            .get_preferences()
            .active_style_pack_id,
        "builtin.raw"
    );
    ol_ios_destroy(handle);
}

#[test]
fn channels_credentials_active_slots_and_settings_survive_store_and_host_reload() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(MemoryVault::default());
    let (handle, context) = open_at(directory.path(), true, storage.clone());
    let asr = save(handle, context, "asr", "openai-compatible", "asr-secret");
    let llm = save(handle, context, "llm", "custom", "llm-secret");
    let second_llm = save(handle, context, "llm", "custom", "second-secret");
    assert_ne!(llm, second_llm);
    save(handle, context, "omni", "custom", "omni-secret");
    for (kind, id) in [
        ("asr", asr.as_str()),
        ("llm", llm.as_str()),
        ("omni", "custom"),
    ] {
        result(control(
            handle,
            context,
            json!({"action":"setActive","kind":kind,"channelID":id}),
        ));
    }
    // Blank API key preserves the existing secret, endpoint/model are patched.
    result(control(
        handle,
        context,
        json!({"action":"saveChannel","kind":"llm","channelID":llm,"providerType":"custom","name":"Renamed","endpoint":"https://invalid.example/v2","model":"updated","apiKey":""}),
    ));
    result(control(
        handle,
        context,
        json!({"action":"updateSettings","pipelineMode":"multimodal","polishEnabled":true}),
    ));
    ol_ios_destroy(handle);
    let (handle, context) = open_at(directory.path(), true, storage);
    let value = control(handle, context, json!({"action":"snapshot"}));
    assert!(!value.to_string().contains("llm-secret"));
    assert!(!value.to_string().contains("omni-secret"));
    let snapshot = result(value);
    assert_eq!(
        snapshot["active"],
        json!({"asr":asr,"llm":llm,"omni":"custom"})
    );
    assert_eq!(snapshot["preferences"]["pipelineMode"], "multimodal");
    assert_eq!(
        snapshot["preferences"]["activeStylePackId"],
        "builtin.light"
    );
    assert_eq!(snapshot["channels"]["llm"][0]["model"], "updated");
    assert_eq!(snapshot["channels"]["llm"][0]["name"], "Renamed");
    assert_eq!(snapshot["channels"]["llm"][0]["hasAPIKey"], true);
    assert_eq!(snapshot["credentialsStatus"]["llmConfigured"], true);
    assert_eq!(snapshot["credentialsStatus"]["omniConfigured"], true);
    let owner = host(handle).unwrap();
    let secret = owner
        .executor
        .block_on(owner.backend.read_credential(
            CredentialKey::new(CredentialNamespace::Llm, Some(llm.clone()), "ark.api_key").unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(secret.expose_secret(), "llm-secret");
    let context_snapshot = owner
        .executor
        .block_on(
            owner
                .backend
                .capture_host_dictation_context(DictationStartOptions::default()),
        )
        .unwrap();
    assert_eq!(context_snapshot.omni.model.as_deref(), Some("test-model"));
    result(control(
        handle,
        context,
        json!({"action":"deleteChannel","kind":"llm","channelID":llm}),
    ));
    assert!(owner
        .executor
        .block_on(owner.backend.read_credential(
            CredentialKey::new(CredentialNamespace::Llm, Some(llm), "ark.api_key").unwrap()
        ))
        .unwrap()
        .is_none());
    assert!(
        !result(control(handle, context, json!({"action":"snapshot"})))["channels"]["llm"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    ol_ios_destroy(handle);
}

#[test]
fn management_is_rejected_during_start_admission_and_rewrite_requires_llm() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    let value = control(
        handle,
        context,
        json!({"action":"rewrite","text":"hello","instruction":"shorten"}),
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["message"], "configured LLM required");
    result(control(
        handle,
        context,
        json!({"action":"updateSettings","pipelineMode":"traditional","polishEnabled":false}),
    ));
    let client = Uuid::new_v4();
    assert_eq!(
        send(
            handle,
            json!({"operation":"start","clientSessionID":client})
        ),
        0
    );
    let request = next(context);
    assert_eq!(request["kind"], "nativeRequest");
    // Recorder and transcription preparation can run concurrently.
    assert!(["captureStart", "speechStart"].contains(&request["operation"].as_str().unwrap()));
    let busy = control(
        handle,
        context,
        json!({"action":"updateSettings","pipelineMode":"multimodal","polishEnabled":true}),
    );
    assert_eq!(busy["ok"], false);
    assert_eq!(busy["message"], "recording or processing is active");
    let audio = result(control(handle, context, json!({"action":"audioSettings"})));
    let busy_audio = control(handle, context, json!({"action":"updateAudioSettings","enabled":true,"dsp":audio["dsp"]}));
    assert_eq!(busy_audio["ok"], false);
    assert_eq!(busy_audio["message"], "recording or processing is active");
    // Snapshot is allowed during capture and doesn't mutate settings.
    result(control(handle, context, json!({"action":"snapshot"})));
    assert_eq!(
        send(
            handle,
            json!({"operation":"cancel","clientSessionID":client})
        ),
        0
    );
    ol_ios_destroy(handle);
}

#[test]
fn provider_service_uses_core_catalog_and_rejects_invalid_omni_selection_without_network() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    // Every model-list transport in tests is an explicit in-process fixture.
    let asr = save(handle, context, "asr", "apple-speech", "");
    let models = result(control(
        handle,
        context,
        json!({"action":"listModels","kind":"asr","channelID":asr}),
    ));
    assert_eq!(models["models"], json!(["fixture-model"]));
    let invalid = control(
        handle,
        context,
        json!({"action":"listModels","kind":"omni","channelID":"gemini"}),
    );
    assert_eq!(invalid["ok"], false);
    let invalid = control(
        handle,
        context,
        json!({"action":"saveChannel","kind":"omni","channelID":"uuid-not-supported","providerType":"custom","name":"","endpoint":"","model":""}),
    );
    assert_eq!(invalid["ok"], false);
    ol_ios_destroy(handle);
}

#[test]
fn native_transcript_and_core_preview_are_separate_stage_scoped_paths() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), false, Arc::new(MemoryVault::default()));
    let client = Uuid::new_v4();
    assert_eq!(
        send(
            handle,
            json!({"operation":"start","clientSessionID":client})
        ),
        0
    );
    let session = loop {
        let event = next(context);
        match event["kind"].as_str() {
            Some("nativeRequest") => reply(handle, &event, Value::Null),
            Some("started") => break event["sessionID"].as_str().unwrap().to_string(),
            Some("failed") => panic!("{event}"),
            _ => {}
        }
    };
    assert_eq!(
        send(
            handle,
            json!({"operation":"transcript","sessionID":session,"text":"原始预览"})
        ),
        0
    );
    let preview = next(context);
    assert_eq!(preview["kind"], "preview");
    assert_eq!(preview["stage"], "asr");
    assert_eq!(preview["text"], "原始预览");
    assert_eq!(preview["sessionID"], session);
    assert_eq!(
        send(
            handle,
            json!({"operation":"cancel","clientSessionID":client})
        ),
        0
    );
    ol_ios_destroy(handle);
}

#[test]
fn failed_vault_write_does_not_publish_unpersisted_secret() {
    struct FailingVault;
    impl credentials::VaultStorage for FailingVault {
        fn load(&self) -> Result<Option<Vec<u8>>, BackendError> {
            Ok(None)
        }
        fn save(&self, _: &[u8]) -> Result<(), BackendError> {
            Err(error(BackendErrorCode::Platform, "injected write failure"))
        }
    }
    let store = IosCredentialStore::open(Arc::new(FailingVault)).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let key = CredentialKey::new(
        CredentialNamespace::Llm,
        Some("custom".into()),
        "ark.api_key",
    )
    .unwrap();
    assert!(runtime
        .block_on(store.write(key.clone(), SecretValue::new("not-persisted")))
        .is_err());
    assert!(runtime.block_on(store.read(key)).unwrap().is_none());
}

#[test]
fn multi_field_save_can_commit_endpoint_before_later_field_failure() {
    struct PartialVault(Mutex<Option<Vec<u8>>>);
    impl credentials::VaultStorage for PartialVault {
        fn load(&self) -> Result<Option<Vec<u8>>, BackendError> { Ok(self.0.lock().unwrap().clone()) }
        fn save(&self, bytes: &[u8]) -> Result<(), BackendError> {
            if bytes.windows(b"reject-this-model".len()).any(|part| part == b"reject-this-model") {
                return Err(error(BackendErrorCode::Persistence, "fixture later-field failure"));
            }
            *self.0.lock().unwrap() = Some(bytes.to_vec()); Ok(())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let context = 890_123;
    let vault = Arc::new(PartialVault(Mutex::new(None)));
    let handle = create_with_store(Config { data_dir: directory.path().into(), developer_mode:true },
        Callbacks { event, audio, context }, Arc::new(IosCredentialStore::open(vault).unwrap())).unwrap();
    let id = save(handle, context, "llm", "custom", "fixture-key");
    let response = control(handle,context,json!({"action":"saveChannel","kind":"llm","channelID":id,
        "providerType":"custom","name":"Fixture","endpoint":"https://new.invalid/v1","model":"reject-this-model"}));
    assert_eq!(response["ok"],false);
    let snapshot = result(control(handle,context,json!({"action":"snapshot"})));
    assert_eq!(snapshot["channels"]["llm"][0]["endpoint"],"https://new.invalid/v1");
    assert_eq!(snapshot["channels"]["llm"][0]["model"],"test-model");
    ol_ios_destroy(handle);
}

pub(crate) struct ModelListFixture;
impl openless_core::ProviderTransport for ModelListFixture {
    fn execute(
        &self,
        _request: openless_core::ProviderTransportRequest,
        _cancellation: openless_core::ProviderCancellation,
    ) -> BoxFuture<
        'static,
        Result<openless_core::ProviderTransportResponse, openless_core::ProviderTransportError>,
    > {
        Box::pin(async {
            Ok(openless_core::ProviderTransportResponse {
                status: 200,
                body: br#"{"data":[{"id":"fixture-model"}]}"#.to_vec(),
            })
        })
    }
}

#[test]
fn validate_uses_native_apple_provider_and_request_correlated_result() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    let asr = save(handle, context, "asr", "apple-speech", "");
    assert_eq!(
        send(
            handle,
            json!({"operation":"control","requestID":"native-validation","action":"validate","kind":"asr","channelID":asr})
        ),
        0
    );
    let mut saw_start = false;
    loop {
        let value = next(context);
        match value["kind"].as_str() {
            Some("nativeRequest") => {
                let result = match value["operation"].as_str() {
                    Some("speechStart") => {
                        assert_eq!(value["payload"]["provider"], "apple-speech");
                        saw_start = true;
                        Value::Null
                    }
                    Some("speechFinish") => json!({"text":"","durationMs":500}),
                    _ => panic!("unexpected native validation request: {value}"),
                };
                reply(handle, &value, result);
            }
            Some("controlResult") => {
                assert!(saw_start);
                assert_eq!(value["requestID"], "native-validation");
                assert_eq!(result(value), json!({"ok":true}));
                break;
            }
            _ => {}
        }
    }
    ol_ios_destroy(handle);
}

#[test]
fn rewrite_uses_core_selection_prompt_and_returns_preview_without_insertion() {
    struct CapturingPolisher(Arc<Mutex<Option<Arc<DictationContext>>>>);
    impl TextPolisher for CapturingPolisher {
        fn polish(
            &self,
            _id: SessionId,
            context: Arc<DictationContext>,
            text: String,
            _partials: Arc<dyn TextStreamSink>,
        ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
            assert_eq!(text, "source text");
            *self.0.lock().unwrap() = Some(context);
            Box::pin(async { Ok(PolishOutput::text("fixture rewrite")) })
        }
        fn cancel(&self, _id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            Box::pin(async { Ok(()) })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let store = Arc::new(IosCredentialStore::open(Arc::new(MemoryVault::default())).unwrap());
    let captured = Arc::new(Mutex::new(None));
    let mut deps = BackendDependencies::unsupported();
    deps.credential_store = store.clone();
    deps.selection_runtime = Some(Arc::new(selection::SuppliedTextRuntime));
    deps.selection_polisher = Some(Arc::new(CapturingPolisher(captured.clone())));
    deps.host_actions = Arc::new(selection::SelectionPreviewActions);
    let repos = BackendRepositories::open(directory.path()).unwrap();
    let mut prefs = repos.preferences.get();
    prefs.selection_polish_output_mode =
        openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm;
    repos.preferences.set(prefs).unwrap();
    let backend = Arc::new(
        OpenLessBackend::new_with_repositories(
            BackendConfig {
                data_dir: directory.path().into(),
                cache_dir: directory.path().join("cache"),
                ..Default::default()
            },
            deps,
            repos,
        )
        .unwrap(),
    );
    runtime.block_on(async {
        let id = backend
            .create_channel(ChannelKind::Llm, "custom".into(), "Fixture".into())
            .await
            .unwrap();
        for (account, value) in [
            ("ark.api_key", "fixture-key"),
            ("ark.endpoint", "https://invalid.example/v1"),
            ("ark.model_id", "fixture-model"),
        ] {
            backend
                .set_credential(
                    CredentialKey::new(CredentialNamespace::Llm, Some(id.clone()), account)
                        .unwrap(),
                    SecretValue::new(value),
                )
                .await
                .unwrap();
        }
    });
    let broker = Arc::new(Broker {
        callbacks: Mutex::new(None),
        pending: Mutex::new(HashMap::new()),
        captures: Mutex::new(HashMap::new()),
        partials: Mutex::new(HashMap::new()),
        closed: AtomicBool::new(false),
    });
    let owner = Host {
        account_sync: account_sync::AccountSync::new(backend.services().marketplace.clone(), store),
        backend,
        broker,
        admission: Mutex::new(None),
        executor: runtime.handle().clone(),
        developer_mode: true,
        recordings_directory: PathBuf::new(),
        control_busy: AtomicBool::new(false),
    };
    let result = runtime
        .block_on(owner.control(control::ControlAction::Rewrite {
            text: "source text".into(),
            instruction: "make this concise".into(),
        }))
        .unwrap();
    assert_eq!(result, json!({"text":"fixture rewrite"}));
    let context = captured.lock().unwrap().clone().unwrap();
    assert!(context.polish.selection_input);
    assert!(context
        .polish
        .style_system_prompt
        .contains("make this concise"));
    assert_eq!(
        runtime
            .block_on(owner.backend.services().selection.snapshot())
            .unwrap()
            .phase,
        openless_core::domains::SelectionPhase::Cancelled
    );
    assert!(owner.backend.list_history().unwrap().is_empty());
}

#[test]
fn core_transcript_and_polish_deltas_use_independent_unicode_accumulators() {
    use openless_core::events::BackendEventKind;
    use openless_core::types::{PolishDelta, TranscriptDelta};
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    let owner = host(handle).unwrap();
    let session = SessionId::new();
    let publisher = owner.backend.event_publisher();
    publisher.publish(
        Some(session),
        BackendEventKind::TranscriptDelta(TranscriptDelta {
            text: "语音文本".into(),
            offset: 0,
            is_final: false,
        }),
    );
    publisher.publish(
        Some(session),
        BackendEventKind::PolishDelta(PolishDelta {
            text: "润色".into(),
            offset: 0,
            is_final: false,
        }),
    );
    publisher.publish(
        Some(session),
        BackendEventKind::PolishDelta(PolishDelta {
            text: "完成".into(),
            offset: 2,
            is_final: true,
        }),
    );
    let asr = next(context);
    let llm = next(context);
    let final_llm = next(context);
    assert_eq!(asr["text"], "语音文本");
    assert_eq!(asr["stage"], "asr");
    assert_eq!(llm["text"], "润色");
    assert_eq!(llm["stage"], "llm");
    assert_eq!(final_llm["text"], "润色完成");
    assert_eq!(final_llm["isFinal"], true);
    assert!(owner.broker.partials.lock().unwrap().is_empty());
    ol_ios_destroy(handle);
}

#[test]
fn provider_errors_do_not_echo_secrets_in_failed_events() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(directory.path(), true, Arc::new(MemoryVault::default()));
    let message = "server echoed api_key=SECRET-SENTINEL";
    host(handle).unwrap().failed(Uuid::new_v4(), &error(BackendErrorCode::Provider, message));
    let value = next(context);
    assert_eq!(value["kind"], "failed");
    assert!(!value.to_string().contains("SECRET-SENTINEL"));
    assert_eq!(value["code"], "Provider");
    let snapshot = result(control(handle, context, json!({"action":"snapshot"})));
    let oauth = snapshot["providerDescriptors"].as_array().unwrap().iter()
        .find(|v| v["providerType"] == "codex_oauth").unwrap();
    assert_eq!(oauth["supported"], false);
    ol_ios_destroy(handle);
}
