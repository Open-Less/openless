use std::sync::Arc;

use openless_core::testing::{FixtureDictationEngine, FixtureTextInserter, RecordingHostActions};
use openless_core::{
    BackendConfig, BackendDependencies, BackendEventKind, DictationPhase, InMemoryCredentialStore,
    InsertOutcome, OpenLessBackend, TokioTaskSpawner,
};

#[tokio::test]
async fn backend_tests_can_use_the_framework_independent_core_contract() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-backend-core-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    ));
    let host = RecordingHostActions::default();
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        BackendDependencies {
            host_actions: Arc::new(host.clone()),
            text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
            dictation_engine: Arc::new(FixtureDictationEngine::successful("raw", "polished")),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(InMemoryCredentialStore::default()),
            services: openless_core::BackendServices::unsupported(),
            local_asr_runtime: None,
            selection_runtime: None,
            selection_polisher: None,
            qa_runtime: None,
            marketplace_config: None,
        },
    )
    .expect("fixture backend should construct");

    let mut events = backend.subscribe();
    backend.start().await.expect("fixture backend should start");
    backend
        .start_dictation()
        .await
        .expect("dictation should start");
    let result = backend
        .stop_dictation()
        .await
        .expect("dictation should complete");

    assert_eq!(result.polished_text, "polished");
    assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    assert!(matches!(
        events.recv().await.unwrap().kind,
        BackendEventKind::BackendStarted
    ));
    assert_eq!(host.actions().len(), 2);
    backend
        .shutdown()
        .await
        .expect("fixture backend should stop");
    let _ = std::fs::remove_dir_all(data_dir);
}
