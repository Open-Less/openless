//! Exercise the Qwen text fallback through the public QA and selection APIs.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use openless_core::credentials::{
    LLM_API_KEY_ACCOUNT, LLM_ENDPOINT_ACCOUNT, LLM_MODEL_ACCOUNT, OMNI_MODEL_ACCOUNT,
};
use openless_core::shared_types::PipelineMode;
use openless_core::testing::FixtureSelectionRuntime;
use openless_core::{
    answer_qa_with_context, BackendConfig, BackendDependencies, BackendError, BackendRepositories,
    ChannelKind, ChannelMutation, ChannelMutationResult, CredentialDirectory, CredentialKey,
    CredentialNamespace, CredentialStore, DictationStartOptions, InMemoryCredentialStore,
    InsertOutcome, OpenLessBackend, PolishMode, ProviderSlot, QaMessage, QaProgress,
    QaProgressSink, SecretValue, SelectionCapture, SelectionPolishOutputMode,
    SelectionPolishRequest, SelectionVoiceIntentMode, SelectionVoiceManualIntent,
    SelectionVoiceRoute, SessionId, SharedCloudTextPolisher, UserPreferences,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const QWEN_PROFILES: [&str; 2] = ["dashscope-realtime", "dashscope-realtime-legacy"];
const MODEL: &str = "saved-custom-text-model";
const KEY: &str = "synthetic-text-key";

struct DiscardQaProgress;
impl QaProgressSink for DiscardQaProgress {
    fn publish(&self, _: SessionId, _: QaProgress) -> Result<(), BackendError> {
        Ok(())
    }
}

async fn text_server(
    responses: Vec<&'static str>,
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
                .await
                .expect("text fallback never reached the configured LLM")
                .unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
                    assert!(headers.to_ascii_lowercase().contains(&format!(
                        "authorization: bearer {}",
                        KEY.to_ascii_lowercase()
                    )));
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request: Value =
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
            assert_eq!(request["model"], MODEL);
            assert_eq!(request["stream"], true);
            requests.push(request);
            let event = json!({"choices":[{"delta":{"content":response}}]});
            let body = format!("data: {event}\n\ndata: [DONE]\n\n");
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        }
        requests
    });
    (endpoint, server)
}

async fn backend(
    profile: &str,
    endpoint: &str,
    legacy_llm: bool,
    output: SelectionPolishOutputMode,
) -> (
    OpenLessBackend,
    Arc<InMemoryCredentialStore>,
    String,
    std::path::PathBuf,
) {
    let store = Arc::new(InMemoryCredentialStore::default());
    let directory = CredentialDirectory::new(store.clone());
    let llm_id = if legacy_llm {
        directory
            .set_active_provider(ProviderSlot::Llm, "ark".into())
            .await
            .unwrap();
        "ark".to_string()
    } else {
        let ChannelMutationResult::Created(id) = directory
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "openai-compatible".into(),
                name: "Fixture text channel".into(),
            })
            .await
            .unwrap()
        else {
            panic!("expected a new LLM channel");
        };
        id
    };
    for (account, value) in [
        (LLM_API_KEY_ACCOUNT, KEY),
        (LLM_MODEL_ACCOUNT, MODEL),
        (LLM_ENDPOINT_ACCOUNT, endpoint),
    ] {
        store
            .write(
                CredentialKey::new(CredentialNamespace::Llm, Some(llm_id.clone()), account)
                    .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    directory
        .set_active_provider(ProviderSlot::Omni, profile.into())
        .await
        .unwrap();
    store
        .write(
            CredentialKey::new(
                CredentialNamespace::Omni,
                Some(profile.into()),
                OMNI_MODEL_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("saved-qwen-audio-model"),
        )
        .await
        .unwrap();
    // No Omni API key or endpoint: auxiliary text must use the text channel.
    let data_dir = std::env::temp_dir().join(format!(
        "openless-realtime-aux-text-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let repositories = BackendRepositories::open(&data_dir).unwrap();
    repositories
        .preferences
        .set(UserPreferences {
            multimodal_pipeline_enabled: true,
            pipeline_mode: PipelineMode::Multimodal,
            active_omni_provider: "stale-omni-preference".into(),
            active_llm_provider: "stale-llm-preference".into(),
            selection_voice_intent_mode: SelectionVoiceIntentMode::Manual,
            selection_voice_manual_intent: SelectionVoiceManualIntent::Edit,
            selection_polish_output_mode: output,
            ..UserPreferences::default()
        })
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = store.clone();
    dependencies.selection_polisher = Some(Arc::new(SharedCloudTextPolisher::new(store.clone())));
    dependencies.selection_runtime = Some(Arc::new(FixtureSelectionRuntime::successful(
        SelectionCapture {
            text: "alpha beta".into(),
            source_app: Some("Fixture Editor".into()),
        },
        InsertOutcome::Inserted,
    )));
    let backend = OpenLessBackend::new_with_repositories(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
        repositories,
    )
    .unwrap();
    (backend, store, llm_id, data_dir)
}

#[tokio::test]
async fn qwen_realtime_qa_resolves_saved_channel_and_legacy_models() {
    for profile in QWEN_PROFILES {
        for legacy in [false, true] {
            let (endpoint, server) = text_server(vec!["text answer"]).await;
            let (backend, store, llm_id, data_dir) = backend(
                profile,
                &endpoint,
                legacy,
                SelectionPolishOutputMode::PreviewConfirm,
            )
            .await;
            let context = backend
                .capture_host_dictation_context(DictationStartOptions::default())
                .await
                .unwrap();
            assert_eq!(context.llm.provider_id, llm_id);
            assert_eq!(context.llm.model.as_deref(), Some(MODEL));
            assert_eq!(context.omni.provider_type, profile);
            let answer = answer_qa_with_context(
                store,
                context,
                vec![QaMessage {
                    id: "question".into(),
                    role: "user".into(),
                    content: "Explain this.".into(),
                    selection_text: None,
                }],
                None,
                SessionId::new(),
                Arc::new(DiscardQaProgress),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
            assert_eq!(answer, "text answer");
            assert_eq!(server.await.unwrap().len(), 1);
            drop(backend);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }
}

#[tokio::test]
async fn qwen_realtime_selection_polish_uses_text_channel_in_both_output_modes() {
    for profile in QWEN_PROFILES {
        for output in [
            SelectionPolishOutputMode::DirectReplace,
            SelectionPolishOutputMode::PreviewConfirm,
        ] {
            let (endpoint, server) = text_server(vec!["alpha gamma"]).await;
            let (backend, _, llm_id, data_dir) = backend(profile, &endpoint, false, output).await;
            let selection = &backend.services().selection;
            let session_id = selection
                .begin_polish(SelectionPolishRequest {
                    selected_text: Some("alpha beta".into()),
                    mode: PolishMode::Light,
                    instruction: Some("replace beta with gamma".into()),
                })
                .await
                .unwrap();
            if output == SelectionPolishOutputMode::PreviewConfirm {
                selection.confirm(session_id, None).await.unwrap();
            }
            assert_eq!(server.await.unwrap().len(), 1);
            let history = backend.repositories().history.list().unwrap();
            assert_eq!(history[0].final_text, "alpha gamma");
            assert_eq!(history[0].llm_provider.as_deref(), Some(llm_id.as_str()));
            assert_eq!(history[0].llm_model.as_deref(), Some(MODEL));
            drop(backend);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }
}

#[tokio::test]
async fn qwen_realtime_selection_voice_polishes_and_builds_edit_plan_via_text_channel() {
    for profile in QWEN_PROFILES {
        let (endpoint, server) = text_server(vec![
            "replace beta with gamma",
            "<edit_plan><summary>rename</summary><literal_replace><find>beta</find><replace>gamma</replace></literal_replace></edit_plan>",
        ]).await;
        let (backend, _, _, data_dir) = backend(
            profile,
            &endpoint,
            false,
            SelectionPolishOutputMode::DirectReplace,
        )
        .await;
        let voice = &backend.services().selection_voice;
        let session_id = voice
            .begin(SelectionCapture {
                text: "alpha beta".into(),
                source_app: None,
            })
            .await
            .unwrap();
        voice.mark_processing(session_id).await.unwrap();
        let disposition = voice
            .process_transcript(session_id, "replace beta".into())
            .await
            .unwrap();
        let SelectionVoiceRoute::ReadyToApply { preview } =
            voice.route_disposition(disposition).await.unwrap()
        else {
            panic!("expected a ready edit preview");
        };
        assert_eq!(preview.text, "alpha gamma");
        assert_eq!(server.await.unwrap().len(), 2);
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}
