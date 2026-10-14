//! Regression coverage for the dedicated transcription route in Multimodal mode.

use super::*;
use crate::credentials::{OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_MODEL_ACCOUNT};
use crate::omni::{OmniConfig, OmniProvider};
use crate::{
    BackendError, DictationAudioSource, DictationContext, DictationEngine, EngineProgress,
    EngineProgressSink, ProviderInvocation, ProviderSlot, SessionId, SharedOmniDictationEngine,
};

fn omni_provider(endpoint: String) -> OmniProvider {
    OmniProvider::new(OmniConfig {
        provider_id: PROVIDER_ID.into(),
        base_url: endpoint,
        api_key: "key".into(),
        model: DEFAULT_MODEL.into(),
        extra_headers: Default::default(),
        temperature: Some(0.3),
        thinking_enabled: true,
    })
}

async fn omni_credentials(endpoint: &str) -> Arc<InMemoryCredentialStore> {
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .set_active_provider(ProviderSlot::Omni, PROVIDER_ID.into())
        .await
        .unwrap();
    for (account, value) in [
        (OMNI_API_KEY_ACCOUNT, "key"),
        (OMNI_ENDPOINT_ACCOUNT, endpoint),
        (OMNI_MODEL_ACCOUNT, DEFAULT_MODEL),
    ] {
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Omni, Some(PROVIDER_ID.into()), account)
                    .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    credentials
}

fn assert_smart_request(request: &Value) {
    assert_eq!(
        request["generation_config"]["transcription_config"]["mode"],
        "smart"
    );
    assert!(request.get("system_instruction").is_none());
    assert_eq!(request["input"].as_array().unwrap().len(), 1);
    assert!(request["generation_config"]
        .get("thinking_config")
        .is_none());
    assert!(request["generation_config"].get("temperature").is_none());
}

#[tokio::test]
async fn omni_verify_uses_smart_audio_and_accepts_completed_silence() {
    let (endpoint, server) =
        transcription_server_check("200 OK", r#"{"status":"completed"}"#, assert_smart_request);
    let service = ProviderService::new(
        omni_credentials(&endpoint).await,
        Arc::new(TokioTaskSpawner),
    );
    let result = service
        .validate(ProviderRequest {
            kind: ProviderKind::Omni,
            channel_id: None,
            thinking_enabled: true,
        })
        .await
        .unwrap();
    assert!(result.ok);
    server.join().unwrap();
}

#[tokio::test]
async fn omni_verify_preserves_http_errors() {
    let (endpoint, server) = transcription_server_check(
        "401 Unauthorized",
        r#"{"error":"secret"}"#,
        assert_smart_request,
    );
    let service = ProviderService::new(
        omni_credentials(&endpoint).await,
        Arc::new(TokioTaskSpawner),
    );
    let error = service
        .validate(ProviderRequest {
            kind: ProviderKind::Omni,
            channel_id: None,
            thinking_enabled: false,
        })
        .await
        .unwrap_err();
    assert_eq!(error.message, "providerHttpStatus:401");
    server.join().unwrap();
}

#[tokio::test]
async fn omni_model_catalog_accepts_the_full_interactions_endpoint() {
    let credentials =
        omni_credentials("https://generativelanguage.googleapis.com/v1beta/interactions/").await;
    let transport = Arc::new(crate::testing::FakeProviderTransport::default());
    transport.push_response(200, br#"{"models":[{"name":"models/gemini-3.5-transcribe","supportedGenerationMethods":["generateContent","countTokens"]}]}"#.to_vec());
    let service = ProviderService::new_with_transport(
        credentials,
        Arc::new(TokioTaskSpawner),
        transport.clone(),
    );
    let models = service
        .list_models(ProviderRequest {
            kind: ProviderKind::Omni,
            channel_id: None,
            thinking_enabled: false,
        })
        .await
        .unwrap();
    assert_eq!(models.models, [DEFAULT_MODEL]);
    let requests = transport.requests();
    assert_eq!(
        requests[0].url,
        "https://generativelanguage.googleapis.com/v1beta/models"
    );
    assert_eq!(
        requests[0].headers,
        [("x-goog-api-key".into(), "key".into())]
    );
}

#[derive(Default)]
struct Progress(Mutex<Vec<EngineProgress>>);

impl EngineProgressSink for Progress {
    fn publish(&self, _: SessionId, progress: EngineProgress) -> Result<(), BackendError> {
        self.0.lock().push(progress);
        Ok(())
    }
}

/// Drive the real engine with only Omni credentials, including structured language hints.
#[tokio::test]
async fn omni_engine_returns_smart_text_without_a_second_llm_call() {
    let (endpoint, server) = transcription_server_check(
        "200 OK",
        r#"{"status":"completed","output_text":"1. Review OpenLess.\n2. Ship Gemini support."}"#,
        |request| {
            assert_smart_request(request);
            let config = &request["generation_config"]["transcription_config"];
            assert_eq!(config["language_codes"], json!(["en", "zh"]));
            assert_eq!(config["custom_vocabulary"], json!(["OpenLess", "Gemini"]));
        },
    );
    let engine = SharedOmniDictationEngine::new(
        omni_credentials(&endpoint).await,
        Arc::new(crate::testing::FixtureAudioRecorder::new(
            Vec::new(),
            Vec::new(),
        )),
    );
    let mut context = DictationContext {
        audio_source: DictationAudioSource::External,
        pipeline_mode: crate::shared_types::PipelineMode::Multimodal,
        omni: ProviderInvocation::for_provider(PROVIDER_ID),
        ..Default::default()
    };
    context.polish.working_languages = vec!["en".into(), "zh".into(), "en".into()];
    context.polish.hotwords = vec!["OpenLess".into(), " Gemini ".into()];
    let context = Arc::new(context);
    let progress = Arc::new(Progress::default());
    let session = SessionId::new();
    engine
        .start(session, Arc::clone(&context), progress.clone())
        .await
        .unwrap();

    // A finish-time translation gesture must not silently return untranslated text.
    let mut translation = (*context).clone();
    translation.polish.translation_active = true;
    assert!(engine
        .update_context(session, Arc::new(translation))
        .await
        .is_err());

    engine.feed_audio(session, &vec![0; 32_000]).unwrap();
    let result = engine.finish(session, progress.clone()).await.unwrap();
    assert_eq!(
        result.polished_text,
        "1. Review OpenLess.\n2. Ship Gemini support."
    );
    assert_eq!(result.raw_text, result.polished_text);
    assert_eq!(result.duration_ms, 1_000);
    assert!(progress.0.lock().iter().any(|event| matches!(event,
        EngineProgress::PolishDelta(delta) if delta.is_final && delta.text == result.polished_text
    )));
    server.join().unwrap();
}

#[tokio::test]
async fn text_tasks_and_qa_are_rejected_without_network_requests() {
    let provider = omni_provider("http://127.0.0.1:1/v1beta".into());
    assert!(provider
        .complete("", "polish this text", None)
        .await
        .is_err());
    assert!(provider
        .complete("", "answer this question", Some(b"wav"))
        .await
        .is_err());
}

#[tokio::test]
async fn dropping_omni_request_cancels_interaction_and_deletes_audio() {
    let (ready, received) = tokio::sync::oneshot::channel();
    let (endpoint, server) = transcription_server_check(
        "200 OK",
        r#"{"id":"i-1","status":"in_progress"}"#,
        move |request| {
            assert_smart_request(request);
            ready.send(()).unwrap();
        },
    );
    let task = tokio::spawn(async move {
        let provider = omni_provider(endpoint);
        provider
            .complete("", "", Some(&encode_wav_16k_mono(&[0; 8_000])))
            .await
    });
    received.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn smart_stream_never_delivers_text_after_cancellation() {
    let (endpoint, server) = transcription_server_check(
        "200 OK",
        r#"{"status":"completed","output_text":"must not be delivered"}"#,
        assert_smart_request,
    );
    let checks = std::sync::atomic::AtomicUsize::new(0);
    let result = omni_provider(endpoint)
        .complete_streaming(
            "",
            "",
            Some(&encode_wav_16k_mono(&[0; 8_000])),
            |_| panic!("cancelled output was delivered"),
            || checks.fetch_add(1, Ordering::AcqRel) > 0,
        )
        .await;
    assert!(result.is_err());
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}
