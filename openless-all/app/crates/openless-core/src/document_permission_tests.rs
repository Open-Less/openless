// Included in api::tests: production capture, settings and provider boundaries.

struct DocPermissionCredentials {
    inner: Arc<crate::InMemoryCredentialStore>,
    armed: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl DocPermissionCredentials {
    fn new(endpoint: &str) -> Self {
        use crate::credentials::*;
        let key = |account| {
            CredentialKey::new(
                CredentialNamespace::Llm,
                Some("doc-channel".into()),
                account,
            )
            .unwrap()
        };
        Self {
            inner: Arc::new(
                InMemoryCredentialStore::default()
                    .with_secret(key(LLM_API_KEY_ACCOUNT), SecretValue::new("fixture-key"))
                    .with_secret(key(LLM_ENDPOINT_ACCOUNT), SecretValue::new(endpoint)),
            ),
            armed: Arc::new(AtomicBool::new(false)),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl crate::CredentialStore for DocPermissionCredentials {
    fn status(
        &self,
        prefs: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        self.inner.status(prefs)
    }
    fn read(
        &self,
        key: crate::CredentialKey,
    ) -> BoxFuture<'static, Result<Option<crate::SecretValue>, BackendError>> {
        let wait = key.namespace == crate::CredentialNamespace::Llm
            && key.provider_id.as_deref() == Some("doc-channel")
            && key.account == crate::credentials::LLM_ENDPOINT_ACCOUNT
            && self.armed.swap(false, Ordering::AcqRel);
        let read = self.inner.read(key);
        let entered = self.entered.clone();
        let release = self.release.clone();
        boxed(async move {
            if wait {
                entered.notify_one();
                release.notified().await;
            }
            read.await
        })
    }
    fn write(
        &self,
        key: crate::CredentialKey,
        value: crate::SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.inner.write(key, value)
    }
    fn remove(&self, key: crate::CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.inner.remove(key)
    }
}

struct DocPermissionHost;
impl HostContextAdapter for DocPermissionHost {
    fn capture_document_context(
        &self,
    ) -> BoxFuture<'static, Option<crate::host_document::CursorContext>> {
        boxed(async {
            Some(crate::host_document::CursorContext {
                before: "DOC-BEFORE".into(),
                after: "DOC-AFTER".into(),
                selection: "DOC-SELECTION".into(),
            })
        })
    }
    fn capture(
        &self,
        include: bool,
    ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
        boxed(async move {
            Ok(HostContextCapture {
                front_app: Some("Test editor".into()),
                cursor_context: include.then(|| "LEGACY-DOC".into()),
            })
        })
    }
}

fn doc_permission_backend(
    data: &std::path::Path,
    credentials: Arc<DocPermissionCredentials>,
) -> (Arc<OpenLessBackend>, String) {
    let mut deps = BackendDependencies::unsupported();
    deps.credential_store = credentials;
    deps.host_actions = Arc::new(FakeHost::default());
    deps.text_inserter = Arc::new(FakeInserter);
    deps.dictation_engine = Arc::new(crate::testing::FixtureDictationEngine::successful(
        "raw", "polished",
    ));
    deps.services.host_context = Arc::new(DocPermissionHost);
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data.into(),
                ..Default::default()
            },
            deps,
        )
        .unwrap(),
    );
    let pack = backend
        .create_style_pack(StylePack {
            name: "Document test".into(),
            prompt: "FROZEN-DOC-STYLE".into(),
            base_mode: crate::PolishMode::Light,
            ..Default::default()
        })
        .unwrap();
    let mut prefs = backend.get_preferences();
    prefs.cursor_context_enabled = true;
    prefs.translation_target_language = "English".into();
    prefs.working_languages = vec!["简体中文".into()];
    prefs.multimodal_pipeline_enabled = false;
    prefs.pipeline_mode = crate::shared_types::PipelineMode::Traditional;
    backend.set_preferences(prefs).unwrap();
    (backend, pack.id)
}

fn doc_permission_warm(backend: &OpenLessBackend) {
    let mut llm = crate::ProviderInvocation::new("doc-channel", "custom");
    llm.model = Some("fixture-model".into());
    *backend.dictation_provider_warmth.lock().unwrap() = Some(DictationProviderWarmth {
        revision: backend.dictation_provider_revision(),
        token: Arc::new(()),
        until: std::time::Instant::now() + std::time::Duration::from_secs(60),
        providers: Some(ResolvedDictationProviders {
            asr: crate::ProviderInvocation::for_provider("sherpa-onnx-local"),
            llm,
            omni: crate::ProviderInvocation::for_provider("unused"),
            llm_error: None,
        }),
    });
}

fn doc_permission_toggle(backend: &OpenLessBackend, enabled: bool) {
    let mut prefs = backend.get_preferences();
    prefs.cursor_context_enabled = enabled;
    backend
        .update_settings(
            prefs,
            crate::SettingsUpdateOptions::STRICT,
            &crate::NoopSettingsRuntime,
        )
        .unwrap();
}

#[tokio::test]
async fn last_configuration_credential_await_revoke_regrant_clears_both_document_channels() {
    for regrant in [false, true] {
        let data = TestDataDir::new("document-last-credential");
        let credentials = Arc::new(DocPermissionCredentials::new("http://127.0.0.1:1/v1"));
        let (backend, style) = doc_permission_backend(data.path(), credentials.clone());
        doc_permission_warm(&backend);
        credentials.armed.store(true, Ordering::Release);
        let capture = tokio::spawn({
            let backend = backend.clone();
            let style = style.clone();
            async move {
                backend
                    .capture_dictation_context(
                        &DictationStartOptions {
                            style_pack_id: Some(style),
                            ..Default::default()
                        },
                        DictationContextPurpose::Dictation,
                    )
                    .await
            }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            credentials.entered.notified(),
        )
        .await
        .unwrap();
        doc_permission_toggle(&backend, false);
        if regrant {
            doc_permission_toggle(&backend, true);
        }
        credentials.release.notify_one();
        let context = tokio::time::timeout(std::time::Duration::from_secs(5), capture)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(context.polish.document_context.is_none());
        assert!(context.polish.cursor_context.is_none());
        assert!(context.request_cursor_context().is_none());
        assert_eq!(context.polish.style_pack_id, style);
        assert_eq!(context.polish.translation_target_language, "English");
        assert!(context
            .with_translation_requested(true)
            .request_cursor_context()
            .is_none());
    }
}

struct DocPermissionSink;
impl crate::ports::TextStreamSink for DocPermissionSink {
    fn publish(&self, _: crate::ports::TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

#[tokio::test]
async fn recording_snapshot_revoked_while_provider_credentials_wait_never_dispatches_old_documents()
{
    use crate::ports::TextPolisher;
    for translation in [false, true] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (requests, receive) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            listener.set_nonblocking(true).unwrap();
            for _ in 0..if translation { 2 } else { 1 } {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let (mut socket, _) = loop {
                    match listener.accept() {
                        Ok(socket) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "expected request was not dispatched"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("controlled server failed: {error}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let n = socket.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.send(String::from_utf8(bytes).unwrap()).unwrap();
                // No translation marker: the translation run must use its
                // existing document-free plain-translation fallback.
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"result\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let data = TestDataDir::new("document-queued-dispatch");
        let credentials = Arc::new(DocPermissionCredentials::new(&endpoint));
        let (backend, style) = doc_permission_backend(data.path(), credentials.clone());
        backend.start().await.unwrap();
        doc_permission_warm(&backend);
        let id = backend
            .start_dictation_with_options(DictationStartOptions {
                style_pack_id: Some(style.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        let context = backend
            .state
            .read()
            .unwrap()
            .dictation_context
            .clone()
            .unwrap();
        assert!(
            context.polish.document_context.is_some() && context.polish.cursor_context.is_some()
        );
        let context = Arc::new((*context).clone().with_translation_requested(translation));
        credentials.armed.store(true, Ordering::Release);
        let polisher = crate::cloud_providers::SharedCloudTextPolisher::new(credentials.clone());
        let pending = tokio::spawn(polisher.polish(
            id,
            context.clone(),
            "本轮语音".into(),
            Arc::new(DocPermissionSink),
        ));
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            credentials.entered.notified(),
        )
        .await
        .unwrap();
        doc_permission_toggle(&backend, false);
        doc_permission_toggle(&backend, true);
        credentials.release.notify_one();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .text,
            "result"
        );
        server.join().unwrap();
        let requests: Vec<_> = receive.try_iter().collect();
        assert_eq!(requests.len(), if translation { 2 } else { 1 });
        for request in &requests {
            assert!(!request.contains("DOC-BEFORE") && !request.contains("DOC-AFTER"));
            assert!(!request.contains("DOC-SELECTION") && !request.contains("LEGACY-DOC"));
            assert!(!request.contains("<cursor_context>"));
        }
        assert!(requests[0].contains("FROZEN-DOC-STYLE"));
        if translation {
            assert!(requests[1].contains("English"));
        }
        assert_eq!(context.polish.style_pack_id, style);
        assert_eq!(context.polish.translation_target_language, "English");
        backend.cancel_dictation(Some(id)).await.unwrap();
        backend.shutdown().await.unwrap();
    }
}
