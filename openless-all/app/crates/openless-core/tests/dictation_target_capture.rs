use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::testing::{FixtureDictationEngine, RecordingHostActions};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, CliDispatchOutcome,
    CliIntent, CredentialKey, CredentialStore, CredentialsStatus, DictationContext,
    DictationHotkeyEdge, DictationInsertStatus, DictationStartOptions, InMemoryCredentialStore,
    InsertOutcome, InsertWriteResult, OpenLessBackend, ProviderSlot, SecretValue, SessionId,
    TextInserter, TextInsertionSession, UserPreferences,
};
use tokio::sync::Notify;

#[derive(Default)]
struct DelayedCredentials {
    store: InMemoryCredentialStore,
    waited: AtomicBool,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl CredentialStore for DelayedCredentials {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        self.store.status(preferences)
    }

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        self.store.read(key)
    }

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.store.write(key, value)
    }

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.store.remove(key)
    }

    fn active_provider(
        &self,
        _slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let wait = !self.waited.swap(true, Ordering::AcqRel);
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(async move {
            if wait {
                entered.notify_one();
                release.notified().await;
            }
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "fixture fallback",
            ))
        })
    }
}

struct Focus {
    current: &'static str,
    captured: Vec<&'static str>,
    prepared: Vec<&'static str>,
    delivered: Vec<(&'static str, String)>,
}

struct FocusInserter {
    focus: Arc<Mutex<Focus>>,
    target: Option<&'static str>,
}

impl TextInserter for FocusInserter {
    fn capture_target(&self) -> Option<Arc<dyn TextInserter>> {
        let mut focus = self.focus.lock().unwrap();
        let target = focus.current;
        focus.captured.push(target);
        Some(Arc::new(Self {
            focus: self.focus.clone(),
            target: Some(target),
        }))
    }

    fn begin(
        &self,
        _session_id: SessionId,
        _context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
        // Like the native adapter, an uncaptured inserter sees the current
        // foreground target. This makes the old late-capture route fail.
        let mut focus = self.focus.lock().unwrap();
        let target = self.target.unwrap_or(focus.current);
        focus.prepared.push(target);
        let session: Arc<dyn TextInsertionSession> = Arc::new(FocusSession {
            focus: self.focus.clone(),
            target,
        });
        Box::pin(async move { Ok(session) })
    }
}

struct FocusSession {
    focus: Arc<Mutex<Focus>>,
    target: &'static str,
}

impl TextInsertionSession for FocusSession {
    fn write(&self, _text: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        panic!("fixture has no streaming deltas")
    }

    fn copy(&self, _text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        panic!("successful target restoration must not use clipboard fallback")
    }

    fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        self.focus
            .lock()
            .unwrap()
            .delivered
            .push((self.target, text));
        Box::pin(async { Ok(InsertOutcome::Inserted) })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

struct Fixture {
    backend: Arc<OpenLessBackend>,
    credentials: Arc<DelayedCredentials>,
    focus: Arc<Mutex<Focus>>,
    directory: std::path::PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("openless-target-{}", SessionId::new()));
        let credentials = Arc::new(DelayedCredentials::default());
        let focus = Arc::new(Mutex::new(Focus {
            current: "A",
            captured: Vec::new(),
            prepared: Vec::new(),
            delivered: Vec::new(),
        }));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        dependencies.host_actions = Arc::new(RecordingHostActions::default());
        dependencies.dictation_engine =
            Arc::new(FixtureDictationEngine::successful("spoken", "spoken"));
        dependencies.text_inserter = Arc::new(FocusInserter {
            focus: focus.clone(),
            target: None,
        });
        let backend = Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: directory.clone(),
                    ..BackendConfig::default()
                },
                dependencies,
            )
            .unwrap(),
        );
        backend.start().await.unwrap();
        Self {
            backend,
            credentials,
            focus,
            directory,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn start(backend: Arc<OpenLessBackend>, route: u8) -> Result<SessionId, BackendError> {
    if route == 0 {
        return backend.start_dictation().await;
    }
    let outcome = if route == 1 {
        backend
            .dispatch_cli_intent(CliIntent::ToggleDictation)
            .await?
    } else {
        backend
            .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Pressed {
                press_id: 1,
                at: std::time::Instant::now(),
            })
            .await?
    };
    match outcome {
        CliDispatchOutcome::DictationStarted(session_id) => Ok(session_id),
        other => panic!("expected a started session, got {other:?}"),
    }
}

#[tokio::test]
async fn every_dictation_entry_freezes_target_before_waiting_for_credentials() {
    for route in 0..3 {
        let fixture = Fixture::new().await;
        let starting = tokio::spawn(start(fixture.backend.clone(), route));
        fixture.credentials.entered.notified().await;
        fixture.focus.lock().unwrap().current = "B";
        fixture.credentials.release.notify_one();
        starting.await.unwrap().unwrap();
        fixture.backend.stop_dictation().await.unwrap();
        let focus = fixture.focus.lock().unwrap();
        assert_eq!(
            focus.delivered,
            vec![("A", "spoken".to_string())],
            "route {route}"
        );
        assert_eq!(focus.captured, vec!["A"]);
        assert_eq!(focus.prepared, vec!["A"]);
    }
}

#[tokio::test]
async fn cancelled_context_capture_never_prepares_or_reuses_its_target() {
    let fixture = Fixture::new().await;
    let starting = tokio::spawn(start(fixture.backend.clone(), 0));
    fixture.credentials.entered.notified().await;
    fixture.backend.cancel_dictation(None).await.unwrap();
    fixture.focus.lock().unwrap().current = "B";
    fixture.credentials.release.notify_one();
    assert_eq!(
        starting.await.unwrap().unwrap_err().code,
        BackendErrorCode::Cancelled
    );
    assert!(fixture.focus.lock().unwrap().prepared.is_empty());
    fixture.backend.start_dictation().await.unwrap();
    fixture.backend.stop_dictation().await.unwrap();
    let focus = fixture.focus.lock().unwrap();
    assert_eq!(focus.captured, vec!["A", "B"]);
    assert_eq!(focus.prepared, vec!["B"]);
    assert_eq!(focus.delivered, vec![("B", "spoken".to_string())]);
}

#[tokio::test]
async fn no_insertion_skips_target_capture_and_native_preparation() {
    let fixture = Fixture::new().await;
    fixture.credentials.release.notify_one();
    fixture
        .backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            ..DictationStartOptions::default()
        })
        .await
        .unwrap();
    let result = fixture.backend.stop_dictation().await.unwrap();
    assert_eq!(result.inserted, DictationInsertStatus::NotRequested);
    let focus = fixture.focus.lock().unwrap();
    assert!(focus.captured.is_empty());
    assert!(focus.prepared.is_empty());
    assert!(focus.delivered.is_empty());
}
