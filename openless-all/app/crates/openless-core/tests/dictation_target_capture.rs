use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::app_style_routing::{AppCategory, AppStyleAssignment, InputApp};
use openless_core::testing::{
    FixtureAudioRecorder, FixtureDictationEngine, FixtureTranscriptionEngine, RecordingHostActions,
};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, CapturedTextTarget,
    CliDispatchOutcome, CliIntent, CredentialKey, CredentialNamespace, CredentialStore,
    CredentialsStatus, DictationContext, DictationHotkeyEdge, DictationInsertStatus,
    DictationStartOptions, HostContextAdapter, HostContextCapture, InMemoryCredentialStore,
    InsertOutcome, InsertWriteResult, OpenLessBackend, PipelineDictationEngine, ProviderSlot,
    SecretValue, SessionId, SharedCloudTextPolisher, StylePack, TextInserter, TextInsertionSession,
    UserPreferences,
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
    context_captured: Vec<&'static str>,
    prepared: Vec<&'static str>,
    delivered: Vec<(&'static str, String)>,
}

struct FocusContextAdapter {
    focus: Arc<Mutex<Focus>>,
    target: &'static str,
}

impl HostContextAdapter for FocusContextAdapter {
    fn input_app(&self) -> Option<InputApp> {
        let bundle = match self.target {
            "VS Code" => "com.microsoft.VSCode",
            "WeChat" => "com.tencent.xinWeChat",
            "Mail" => "com.apple.mail",
            "Custom" => "org.example.unknown",
            _ => return None,
        };
        InputApp::macos(bundle, Some(self.target))
    }

    fn capture(
        &self,
        _include_cursor: bool,
    ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
        self.focus
            .lock()
            .unwrap()
            .context_captured
            .push(self.target);
        let front_app = self.target.to_string();
        Box::pin(async move {
            Ok(HostContextCapture {
                front_app: Some(front_app),
                cursor_context: None,
            })
        })
    }
}

struct FocusInserter {
    focus: Arc<Mutex<Focus>>,
    target: Option<&'static str>,
}

impl TextInserter for FocusInserter {
    fn capture_target(&self) -> Option<CapturedTextTarget> {
        let mut focus = self.focus.lock().unwrap();
        let target = focus.current;
        focus.captured.push(target);
        Some(CapturedTextTarget {
            inserter: Arc::new(Self {
                focus: self.focus.clone(),
                target: Some(target),
            }),
            host_context: Arc::new(FocusContextAdapter {
                focus: self.focus.clone(),
                target,
            }),
        })
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
        Self::with_engine(|_| Arc::new(FixtureDictationEngine::successful("spoken", "spoken")))
            .await
    }

    async fn with_engine(
        build_engine: impl FnOnce(Arc<DelayedCredentials>) -> Arc<dyn openless_core::DictationEngine>,
    ) -> Self {
        let directory = std::env::temp_dir().join(format!("openless-target-{}", SessionId::new()));
        let credentials = Arc::new(DelayedCredentials::default());
        let focus = Arc::new(Mutex::new(Focus {
            current: "A",
            captured: Vec::new(),
            context_captured: Vec::new(),
            prepared: Vec::new(),
            delivered: Vec::new(),
        }));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        dependencies.host_actions = Arc::new(RecordingHostActions::default());
        dependencies.dictation_engine = build_engine(credentials.clone());
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
        assert_eq!(focus.context_captured, vec!["A"]);
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
    assert_eq!(focus.context_captured, vec!["A", "B"]);
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

#[tokio::test]
async fn routed_dictation_sends_the_frozen_pack_and_delivers_to_the_original_app() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sent, mut payloads) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut bytes = vec![0; length];
            socket.read_exact(&mut bytes).await.unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let (mime, response) = if payload["stream"] == true {
                ("text/event-stream", "data: {\"choices\":[{\"delta\":{\"content\":\"polished\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n")
            } else {
                (
                    "application/json",
                    r#"{"choices":[{"message":{"content":"polished"}}]}"#,
                )
            };
            sent.send(payload).unwrap();
            let reply = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n{response}", response.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    let fixture = Fixture::with_engine(|credentials| {
        Arc::new(PipelineDictationEngine::new(
            Arc::new(FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![])),
            Arc::new(FixtureTranscriptionEngine::successful(
                "spoken test transcript",
                500,
            )),
            Arc::new(SharedCloudTextPolisher::new(credentials)),
        ))
    })
    .await;
    let backend = &fixture.backend;
    for (account, secret) in [("ark.api_key", "fixture-key"), ("ark.endpoint", &endpoint)] {
        fixture
            .credentials
            .write(
                CredentialKey::new(CredentialNamespace::Llm, Some("ark".into()), account).unwrap(),
                SecretValue::new(secret),
            )
            .await
            .unwrap();
    }
    let mut preferences = backend.get_preferences();
    preferences.multimodal_pipeline_enabled = false;
    preferences.active_llm_provider = "ark".into();
    preferences.polish_skip_max_chars = 0;
    preferences.streaming_insert = false;
    backend
        .update_settings(
            preferences,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &openless_core::NoopSettingsRuntime,
        )
        .unwrap();

    let make_pack = |name: &str| {
        backend
            .create_style_pack(StylePack {
                name: name.into(),
                prompt: format!("Apply the unique {name} marker."),
                ..StylePack::default()
            })
            .unwrap()
    };
    let current = make_pack("MANUAL-STYLE");
    backend.activate_style_pack(&current.id).unwrap();
    let coding = make_pack("CODING-STYLE");
    let chat = make_pack("CHAT-STYLE");
    let mail = make_pack("MAIL-STYLE");
    for (category, pack) in [
        (AppCategory::Coding, &coding),
        (AppCategory::Chat, &chat),
        (AppCategory::Email, &mail),
    ] {
        backend
            .set_app_style_assignment(AppStyleAssignment::Category {
                category,
                style_pack_id: Some(pack.id.clone()),
            })
            .unwrap();
    }
    let expect_request = |payload: serde_json::Value, marker: &str| {
        assert!(
            payload["messages"][0]["content"]
                .as_str()
                .unwrap_or_default()
                .contains(marker),
            "{payload}"
        );
    };

    fixture.focus.lock().unwrap().current = "VS Code";
    let starting = tokio::spawn(start(backend.clone(), 2));
    fixture.credentials.entered.notified().await;
    fixture.focus.lock().unwrap().current = "WeChat";
    backend
        .set_app_style_assignment(AppStyleAssignment::Category {
            category: AppCategory::Coding,
            style_pack_id: None,
        })
        .unwrap();
    fixture.credentials.release.notify_one();
    starting.await.unwrap().unwrap();
    let result = backend.stop_dictation().await.unwrap();
    assert_eq!(result.polished_text, "polished");
    expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");
    assert_eq!(fixture.focus.lock().unwrap().delivered[0].0, "VS Code");
    assert_eq!(
        backend.app_style_routing().unwrap().apps[0].app.name,
        "VS Code"
    );

    let override_app = AppStyleAssignment::App {
        app_id: "macos:com.microsoft.VSCode".into(),
        style_pack_id: Some(chat.id.clone()),
    };
    backend.set_app_style_assignment(override_app).unwrap();
    backend
        .set_app_style_assignment(AppStyleAssignment::Category {
            category: AppCategory::Coding,
            style_pack_id: Some(coding.id.clone()),
        })
        .unwrap();

    for (app, marker) in [
        ("WeChat", "CHAT-STYLE"),
        ("Mail", "MAIL-STYLE"),
        ("VS Code", "CHAT-STYLE"),
        ("Custom", "MANUAL-STYLE"),
        ("No stable identifier", "MANUAL-STYLE"),
    ] {
        fixture.focus.lock().unwrap().current = app;
        backend.start_dictation().await.unwrap();
        assert_eq!(
            backend.stop_dictation().await.unwrap().polished_text,
            "polished"
        );
        expect_request(payloads.try_recv().unwrap(), marker);
        assert_eq!(
            fixture.focus.lock().unwrap().delivered.last().unwrap().0,
            app
        );
    }

    fixture.focus.lock().unwrap().current = "VS Code";
    backend.set_style_pack_enabled(&chat.id, false).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");
    backend.set_style_pack_enabled(&chat.id, true).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CHAT-STYLE");
    backend.remove_style_pack(&chat.id).unwrap();
    backend.start_dictation().await.unwrap();
    backend.stop_dictation().await.unwrap();
    expect_request(payloads.try_recv().unwrap(), "CODING-STYLE");

    let before_clear = backend.app_style_routing().unwrap();
    assert_eq!(before_clear.apps.len(), 4);
    assert_eq!(before_clear.categories[&AppCategory::Chat], chat.id);
    let cleared = backend.clear_input_app_history().unwrap();
    assert_eq!(cleared.apps.len(), 1);
    assert!(!cleared.apps[0].seen);
    assert_eq!(
        cleared.apps[0].style_pack_id.as_deref(),
        Some(chat.id.as_str())
    );
    assert_eq!(backend.get_preferences().active_style_pack_id, current.id);
    let local_file =
        std::fs::read_to_string(fixture.directory.join("app-style-routing.json")).unwrap();
    assert!(!local_file.contains("spoken test transcript"));
    assert!(!local_file.contains("polished"));
    backend.shutdown().await.unwrap();
    server.abort();
}
