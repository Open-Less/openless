use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, BackendEventKind,
    CredentialKey, CredentialNamespace, CredentialStore, CredentialsStatus,
    InMemoryCredentialStore, MarketplaceConfig, MarketplaceLikeResult, MarketplaceListItem,
    MarketplaceMyPackItem, MarketplaceQuery, MarketplaceUploadResult, NoopSettingsRuntime,
    OAuthDeviceFlow, OAuthPollResult, OpenLessBackend, SecretValue, SettingsUpdateOptions,
    StylePack, StylePackStore, UserPreferences, MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
    STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn write_preferences(backend: &OpenLessBackend, preferences: UserPreferences) {
    backend
        .update_settings(
            preferences,
            SettingsUpdateOptions::STRICT,
            &NoopSettingsRuntime,
        )
        .expect("preferences should persist");
}

fn marketplace_backend(base_url: String, name: &str) -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    (backend, data_dir)
}

#[tokio::test]
async fn marketplace_follows_live_proxy_policy_and_bypasses_each_loopback_endpoint() {
    const CHILD: &str = "OPENLESS_MARKETPLACE_PROXY_CONTRACT";
    if let Ok(device_url) = std::env::var(CHILD) {
        // The subprocess owns both environment and Core's global proxy policy;
        // no concurrently running contract test can see this test's toggles.
        openless_core::net::set_use_system_proxy(false);
        let (backend, data_dir) = marketplace_backend(
            format!("http://{}.invalid", uuid::Uuid::new_v4()),
            "proxy-policy",
        );
        let marketplace = &backend.services().marketplace;
        let query = || MarketplaceQuery {
            query: None,
            sort: None,
            limit: None,
        };
        // .invalid has no origin server. Success therefore proves that the
        // local test proxy was used; an error proves direct routing was kept.
        assert!(
            marketplace.list(query()).await.is_err(),
            "saved proxy opt-out was ignored"
        );
        openless_core::net::set_use_system_proxy(true);
        assert!(marketplace.list(query()).await.unwrap().is_empty());
        openless_core::net::set_use_system_proxy(false);
        assert!(
            marketplace.list(query()).await.is_err(),
            "live proxy opt-out was ignored"
        );
        openless_core::net::set_use_system_proxy(true);
        assert!(marketplace.list(query()).await.unwrap().is_empty());

        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
        let mut config = MarketplaceConfig::production();
        // The other endpoints remain non-loopback: bypass must be selected for
        // this request URL, not only when every configured endpoint is local.
        config.github_device_code_url = device_url.parse().unwrap();
        dependencies.marketplace_config = Some(config);
        let oauth_data_dir = data_dir.join("oauth");
        let oauth = OpenLessBackend::new(
            BackendConfig {
                data_dir: oauth_data_dir,
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();
        assert_eq!(
            oauth
                .services()
                .marketplace
                .start_device_flow()
                .await
                .unwrap()
                .user_code,
            "ABCD-EFGH",
        );
        let _ = std::fs::remove_dir_all(data_dir);
        return;
    }

    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let proxy_requests = Arc::new(AtomicUsize::new(0));
    let proxy_count = Arc::clone(&proxy_requests);
    let proxy_server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = proxy.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            proxy_count.fetch_add(1, Ordering::SeqCst);
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]").await.unwrap();
        }
    });
    let direct = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let device_url = format!("http://{}/device", direct.local_addr().unwrap());
    let direct_server = tokio::spawn(async move {
        let (mut stream, _) = direct.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        let body = r#"{"device_code":"fixture-secret","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":600,"interval":5}"#;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    let output = tokio::task::spawn_blocking(move || {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "marketplace_follows_live_proxy_policy_and_bypasses_each_loopback_endpoint",
            "--nocapture",
        ]);
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            command.env_remove(name);
        }
        command
            .env("HTTP_PROXY", proxy_url)
            .env(CHILD, device_url)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    proxy_server.abort();
    direct_server.abort();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        proxy_requests.load(Ordering::SeqCst),
        2,
        "only the two proxy-enabled public requests may reach the proxy"
    );
}

#[test]
fn marketplace_result_dtos_have_stable_host_facing_json() {
    let upload = MarketplaceUploadResult {
        id: "remote-pack-id".into(),
        state: "pending".into(),
        message: "queued".into(),
    };
    assert_eq!(
        serde_json::to_value(upload).unwrap(),
        serde_json::json!({
            "id": "remote-pack-id",
            "state": "pending",
            "message": "queued",
        })
    );

    let like = MarketplaceLikeResult {
        like_count: 12,
        already_liked: true,
    };
    assert_eq!(
        serde_json::to_value(like).unwrap(),
        serde_json::json!({"likeCount": 12, "alreadyLiked": true})
    );

    let mine = MarketplaceMyPackItem {
        summary: MarketplaceListItem {
            id: "remote-pack-id".into(),
            name: "My Pack".into(),
            ..MarketplaceListItem::default()
        },
        state: "approved".into(),
    };
    let value = serde_json::to_value(mine).unwrap();
    assert_eq!(value["id"], "remote-pack-id");
    assert_eq!(value["name"], "My Pack");
    assert_eq!(value["state"], "approved");
}

#[test]
fn oauth_contract_exposes_poll_state_but_never_the_device_secret() {
    let flow = OAuthDeviceFlow {
        flow_id: "opaque-flow".into(),
        user_code: "ABCD-EFGH".into(),
        verification_uri: "https://github.com/login/device".into(),
        expires_in_secs: 600,
        interval_secs: 7,
    };
    let serialized = serde_json::to_string(&flow).unwrap();
    assert!(!serialized.contains("deviceCode"));
    assert!(!serialized.contains("raw-device-secret"));

    assert_eq!(
        serde_json::to_value(OAuthPollResult::Authorized {
            login: "octocat".into(),
        })
        .unwrap(),
        serde_json::json!({"kind": "authorized", "login": "octocat"})
    );
    assert_eq!(
        serde_json::to_value(OAuthPollResult::Pending).unwrap(),
        serde_json::json!({"kind": "pending"})
    );
    assert_eq!(
        serde_json::to_value(OAuthPollResult::SlowDown).unwrap(),
        serde_json::json!({"kind": "slowDown"})
    );
    assert_eq!(
        serde_json::to_value(OAuthPollResult::Error {
            message: "expired".into(),
        })
        .unwrap(),
        serde_json::json!({"kind": "error", "message": "expired"})
    );
}

#[tokio::test]
async fn public_marketplace_browsing_never_sends_the_saved_bearer_token() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        let body = r#"[{"id":"remote","slug":"demo","name":"Demo","description":"","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":[],"likeCount":1,"downloadCount":2,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}]"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("gho_must_not_leave_process"),
        )
        .await
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let items = backend
        .services()
        .marketplace
        .list(MarketplaceQuery {
            query: Some("hello".into()),
            sort: Some("popular".into()),
            limit: Some(25),
        })
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Demo");

    let request = server.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /packs?q=hello&sort=popular&limit=25 http/1.1"));
    assert!(!request.contains("authorization:"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn public_marketplace_detail_is_available_through_the_core_interface() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        let body = r#"{"id":"00000000-0000-0000-0000-000000000001","slug":"demo","name":"Demo","description":"A pack","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":["demo"],"likeCount":1,"downloadCount":2,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","prompt":"Be concise","state":"approved"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-detail-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let detail = backend
        .services()
        .marketplace
        .detail("00000000-0000-0000-0000-000000000001".into())
        .await
        .unwrap();
    assert_eq!(detail.summary.author_login, "octocat");
    assert_eq!(detail.prompt, "Be concise");
    assert_eq!(detail.state, "approved");

    let request = server.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /packs/00000000-0000-0000-0000-000000000001 http/1.1"));
    assert!(!request.contains("authorization:"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn marketplace_archive_download_returns_only_a_validated_style_pack_archive() {
    let style_packs = StylePackStore::in_memory();
    let pack = style_packs
        .create(StylePack {
            id: "download-fixture".into(),
            name: "Download fixture".into(),
            prompt: "Keep the fixture concise".into(),
            ..StylePack::default()
        })
        .unwrap();
    let archive = style_packs.export_zip_bytes(&pack.id).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let expected_archive = archive.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            expected_archive.len()
        );
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(&expected_archive).await.unwrap();
        request
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-download-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let downloaded = backend
        .services()
        .marketplace
        .download_archive("00000000-0000-0000-0000-000000000001".into())
        .await
        .unwrap();
    assert_eq!(downloaded, archive);
    let request = server.await.unwrap().to_ascii_lowercase();
    assert!(
        request.starts_with("get /packs/00000000-0000-0000-0000-000000000001/download http/1.1")
    );
    assert!(!request.contains("authorization:"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn marketplace_install_commits_origin_revision_and_event_as_one_success() {
    let source_store = StylePackStore::in_memory();
    let source = source_store
        .create(StylePack {
            id: "install-fixture".into(),
            name: "Install fixture".into(),
            prompt: "Install me".into(),
            ..StylePack::default()
        })
        .unwrap();
    let archive = source_store.export_zip_bytes(&source.id).unwrap();
    let remote_id = "00000000-0000-0000-0000-000000000001";

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for response_index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            if response_index == 0 {
                assert!(request.starts_with(&format!("GET /packs/{remote_id} HTTP/1.1")));
                let body = format!(
                    r#"{{"id":"{remote_id}","slug":"install","name":"Install fixture","description":"","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":[],"likeCount":0,"downloadCount":0,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","prompt":"Install me","state":"approved"}}"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            } else {
                assert!(request.starts_with(&format!("GET /packs/{remote_id}/download HTTP/1.1")));
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    archive.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&archive).await.unwrap();
            }
        }
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-install-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    let before = backend.snapshot().style_pack_revision;
    let mut events = backend.subscribe();

    let installed = backend
        .services()
        .marketplace
        .install(remote_id.into())
        .await
        .unwrap();
    assert_eq!(installed.origin_pack_id.as_deref(), Some(remote_id));
    assert_eq!(installed.origin_author_login.as_deref(), Some("octocat"));
    assert_eq!(backend.snapshot().style_pack_revision, before + 1);
    let event = events.try_recv().unwrap();
    assert_eq!(event.sequence, 1);
    assert!(matches!(
        event.kind,
        BackendEventKind::StylePacksChanged(change) if change.revision == before + 1
    ));
    assert!(events.try_recv().is_err());
    server.await.unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

struct RemoveFailingCredentialStore {
    value: Arc<Mutex<Option<SecretValue>>>,
    remove_calls: Arc<AtomicUsize>,
}

impl RemoveFailingCredentialStore {
    fn new(token: &str) -> Self {
        Self {
            value: Arc::new(Mutex::new(Some(SecretValue::new(token)))),
            remove_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl CredentialStore for RemoveFailingCredentialStore {
    fn status(
        &self,
        _: openless_core::UserPreferences,
    ) -> futures_util::future::BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        Box::pin(async { Ok(CredentialsStatus::default()) })
    }

    fn read(
        &self,
        _: CredentialKey,
    ) -> futures_util::future::BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let value = self.value.lock().unwrap().clone();
        Box::pin(async move { Ok(value) })
    }

    fn write(
        &self,
        _: CredentialKey,
        value: SecretValue,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        *self.value.lock().unwrap() = Some(value);
        Box::pin(async { Ok(()) })
    }

    fn remove(
        &self,
        _: CredentialKey,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        self.remove_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Persistence,
                "injected credential deletion failure",
            ))
        })
    }
}

#[tokio::test]
async fn rejected_marketplace_token_is_tombstoned_before_durable_delete() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: 21\r\nConnection: close\r\n\r\ngho_response_secret!!",
            )
            .await
            .unwrap();
        let contacted_again =
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_ok();
        (request, contacted_again)
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-tombstone-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(RemoveFailingCredentialStore::new("gho_rejected_secret"));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials.clone();
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    let mut preferences = backend.get_preferences();
    preferences.marketplace_dev_login = "octocat".into();
    write_preferences(&backend, preferences);

    let first = backend.services().marketplace.my_likes().await.unwrap_err();
    assert_eq!(first.code, BackendErrorCode::PermissionDenied);
    assert!(!first.message.contains("gho_"));
    assert!(
        !backend
            .services()
            .marketplace
            .auth_status()
            .await
            .unwrap()
            .signed_in
    );
    assert!(backend.get_preferences().marketplace_dev_login.is_empty());
    assert_eq!(credentials.remove_calls.load(Ordering::SeqCst), 1);

    let second = backend.services().marketplace.my_likes().await.unwrap_err();
    assert_eq!(second.code, BackendErrorCode::PermissionDenied);
    let (request, contacted_again) = server.await.unwrap();
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer gho_rejected_secret"));
    assert!(
        !contacted_again,
        "tombstoned token reached the network again"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn authenticated_marketplace_like_returns_the_server_toggle_result() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        let body = r#"{"likeCount":12,"alreadyLiked":true}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-like-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("gho_like_secret"),
        )
        .await
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let result = backend
        .services()
        .marketplace
        .toggle_like("00000000-0000-0000-0000-000000000001".into())
        .await
        .unwrap();
    assert_eq!(result.like_count, 12);
    assert!(result.already_liked);
    let request = server.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /packs/00000000-0000-0000-0000-000000000001/like http/1.1"));
    assert_eq!(
        request
            .lines()
            .filter(|line| *line == "authorization: bearer gho_like_secret")
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn marketplace_upload_exports_the_local_pack_and_binds_the_returned_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let remote_id = "00000000-0000-0000-0000-000000000002";
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut expected = None;
        loop {
            let mut chunk = [0u8; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if expected.is_none() {
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    expected = Some(header_end + 4 + content_length);
                }
            }
            if expected.is_some_and(|expected| request.len() >= expected) {
                break;
            }
        }
        let body = format!(r#"{{"id":"{remote_id}","state":"pending","message":"queued"}}"#);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-upload-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("gho_upload_secret"),
        )
        .await
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    let local = backend
        .create_style_pack(StylePack {
            id: "local-upload".into(),
            name: "Local upload".into(),
            prompt: "Upload this".into(),
            ..StylePack::default()
        })
        .unwrap();
    let mut preferences = backend.get_preferences();
    preferences.marketplace_dev_login = "octocat".into();
    write_preferences(&backend, preferences);

    let result = backend
        .services()
        .marketplace
        .upload(local.id.clone(), None)
        .await
        .unwrap();
    assert_eq!(result.id, remote_id);
    assert_eq!(result.state, "pending");
    assert_eq!(result.message, "queued");
    let updated = backend.get_style_pack(&local.id).unwrap();
    assert_eq!(updated.origin_pack_id.as_deref(), Some(remote_id));
    assert_eq!(updated.origin_author_login.as_deref(), Some("octocat"));

    let request = server.await.unwrap();
    let request_text = String::from_utf8_lossy(&request).to_ascii_lowercase();
    assert!(request_text.starts_with("post /packs http/1.1"));
    assert!(request_text.contains("authorization: bearer gho_upload_secret"));
    assert!(request_text.contains("content-type: multipart/form-data; boundary="));
    assert!(request.windows(2).any(|window| window == b"PK"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn authenticated_marketplace_collection_operations_use_the_core_interface() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let remote_id = "00000000-0000-0000-0000-000000000003";
    let server = tokio::spawn(async move {
        let responses = [
            format!(r#"["{remote_id}"]"#),
            format!(
                r#"[{{"id":"{remote_id}","slug":"mine","name":"Mine","description":"","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":[],"likeCount":1,"downloadCount":2,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","state":"approved"}}]"#
            ),
            "{}".into(),
        ];
        let mut requests = Vec::new();
        for body in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-collections-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("gho_collections_secret"),
        )
        .await
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    assert_eq!(
        backend.services().marketplace.my_likes().await.unwrap(),
        vec![remote_id]
    );
    let mine = backend.services().marketplace.my_packs().await.unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].summary.id, remote_id);
    assert_eq!(mine[0].state, "approved");
    backend
        .services()
        .marketplace
        .delete(remote_id.into())
        .await
        .unwrap();

    let requests = server.await.unwrap();
    assert!(requests[0].starts_with("GET /me/likes HTTP/1.1"));
    assert!(requests[1].starts_with("GET /me/packs HTTP/1.1"));
    assert!(requests[2].starts_with(&format!("DELETE /packs/{remote_id} HTTP/1.1")));
    assert!(requests.iter().all(|request| request
        .to_ascii_lowercase()
        .contains("authorization: bearer gho_collections_secret")));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn github_device_flow_keeps_secrets_inside_core_and_consumes_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        let bodies = [
            r#"{"device_code":"raw-device-secret","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","interval":1,"expires_in":600}"#,
            r#"{"access_token":"gho_oauth_secret"}"#,
            r#"{"login":"octocat"}"#,
        ];
        for body in bodies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-oauth-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let mut config = MarketplaceConfig::new(format!("http://{address}")).unwrap();
    config.github_device_code_url = format!("http://{address}/device").parse().unwrap();
    config.github_access_token_url = format!("http://{address}/token").parse().unwrap();
    config.github_user_url = format!("http://{address}/user").parse().unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials.clone();
    dependencies.marketplace_config = Some(config);
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let flow = backend
        .services()
        .marketplace
        .start_device_flow()
        .await
        .unwrap();
    assert_eq!(flow.user_code, "ABCD-EFGH");
    assert_eq!(flow.interval_secs, 1);
    let serialized = serde_json::to_string(&flow).unwrap();
    assert!(!serialized.contains("device_code"));
    assert!(!serialized.contains("raw-device-secret"));

    let poll = backend
        .services()
        .marketplace
        .poll_device_flow(flow.flow_id.clone())
        .await
        .unwrap();
    assert_eq!(
        poll,
        OAuthPollResult::Authorized {
            login: "octocat".into()
        }
    );
    assert_eq!(backend.get_preferences().marketplace_dev_login, "octocat");
    let saved = credentials
        .read(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.expose_secret(), "gho_oauth_secret");
    assert!(matches!(
        backend
            .services()
            .marketplace
            .poll_device_flow(flow.flow_id)
            .await
            .unwrap(),
        OAuthPollResult::Error { .. }
    ));

    let requests = server.await.unwrap();
    assert!(requests[0].starts_with("POST /device HTTP/1.1"));
    assert!(requests[1].starts_with("POST /token HTTP/1.1"));
    assert!(requests[1].contains("device_code=raw-device-secret"));
    assert!(requests[2].starts_with("GET /user HTTP/1.1"));
    assert!(requests[2]
        .to_ascii_lowercase()
        .contains("authorization: bearer gho_oauth_secret"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn authenticated_marketplace_redirect_is_rejected_without_contacting_the_target() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_url = format!(
        "http://{}/gho_location_secret",
        target.local_addr().unwrap()
    );
    let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", source.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = source.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        let target_contacted =
            tokio::time::timeout(std::time::Duration::from_millis(200), target.accept())
                .await
                .is_ok();
        (request, target_url, target_contacted)
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-redirect-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    credentials
        .write(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap(),
            SecretValue::new("gho_redirect_secret"),
        )
        .await
        .unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.marketplace_config = Some(MarketplaceConfig::new(base_url).unwrap());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let error = backend
        .services()
        .marketplace
        .toggle_like("00000000-0000-0000-0000-000000000001".into())
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Provider);
    assert_eq!(error.message, "marketplace_authenticated_redirect_rejected");
    let (request, location, target_contacted) = server.await.unwrap();
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer gho_redirect_secret"));
    assert!(!target_contacted);
    assert!(!error.message.contains("gho_redirect_secret"));
    assert!(!error.message.contains(&location));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn marketplace_archive_enforces_declared_and_streamed_limits_with_an_exact_boundary() {
    let remote_id = "00000000-0000-0000-0000-000000000001";

    let declared_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let declared_url = format!("http://{}", declared_listener.local_addr().unwrap());
    let declared_server = tokio::spawn(async move {
        let (mut stream, _) = declared_listener.accept().await.unwrap();
        let mut request = [0u8; 2048];
        let _ = stream.read(&mut request).await.unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES + 1
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    let (declared_backend, declared_dir) = marketplace_backend(declared_url, "declared-limit");
    let declared_error = declared_backend
        .services()
        .marketplace
        .download_archive(remote_id.into())
        .await
        .unwrap_err();
    assert!(declared_error.message.contains("exceeds"));
    declared_server.await.unwrap();
    let _ = std::fs::remove_dir_all(declared_dir);

    let streamed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let streamed_url = format!("http://{}", streamed_listener.local_addr().unwrap());
    let streamed_server = tokio::spawn(async move {
        let (mut stream, _) = streamed_listener.accept().await.unwrap();
        let mut request = [0u8; 2048];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let body = vec![0u8; STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES + 1];
        stream
            .write_all(format!("{:X}\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        stream.write_all(&body).await.unwrap();
        stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
    });
    let (streamed_backend, streamed_dir) = marketplace_backend(streamed_url, "streamed-limit");
    let streamed_error = streamed_backend
        .services()
        .marketplace
        .download_archive(remote_id.into())
        .await
        .unwrap_err();
    assert!(streamed_error.message.contains("exceeds"));
    streamed_server.await.unwrap();
    let _ = std::fs::remove_dir_all(streamed_dir);

    let exact_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let exact_url = format!("http://{}", exact_listener.local_addr().unwrap());
    let exact_server = tokio::spawn(async move {
        let (mut stream, _) = exact_listener.accept().await.unwrap();
        let mut request = [0u8; 2048];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let body = vec![0u8; STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES];
        stream
            .write_all(format!("{:X}\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        stream.write_all(&body).await.unwrap();
        stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
    });
    let (exact_backend, exact_dir) = marketplace_backend(exact_url, "exact-limit");
    let exact_error = exact_backend
        .services()
        .marketplace
        .download_archive(remote_id.into())
        .await
        .unwrap_err();
    assert_eq!(
        exact_error.message,
        "Marketplace returned an invalid style pack archive"
    );
    exact_server.await.unwrap();
    let _ = std::fs::remove_dir_all(exact_dir);
}

#[tokio::test]
async fn marketplace_rejects_a_concurrent_install_before_the_second_request() {
    let source_store = StylePackStore::in_memory();
    let source = source_store
        .create(StylePack {
            id: "concurrent-install".into(),
            name: "Concurrent install".into(),
            prompt: "Install once".into(),
            ..StylePack::default()
        })
        .unwrap();
    let archive = source_store.export_zip_bytes(&source.id).unwrap();
    let remote_id = "00000000-0000-0000-0000-000000000004";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (detail_seen_tx, detail_seen_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut detail_stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = detail_stream.read(&mut request).await.unwrap();
        detail_seen_tx.send(()).unwrap();
        resume_rx.await.unwrap();
        let detail_body = format!(
            r#"{{"id":"{remote_id}","slug":"install","name":"Concurrent install","description":"","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":[],"likeCount":0,"downloadCount":0,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","prompt":"Install once","state":"approved"}}"#
        );
        let detail_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{detail_body}",
            detail_body.len()
        );
        detail_stream
            .write_all(detail_response.as_bytes())
            .await
            .unwrap();

        let (mut download_stream, _) = listener.accept().await.unwrap();
        let _ = download_stream.read(&mut request).await.unwrap();
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            archive.len()
        );
        download_stream.write_all(headers.as_bytes()).await.unwrap();
        download_stream.write_all(&archive).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
            .await
            .is_ok()
    });

    let (backend, data_dir) = marketplace_backend(base_url, "concurrent-install");
    let marketplace = backend.services().marketplace.clone();
    let first = tokio::spawn({
        let marketplace = marketplace.clone();
        async move { marketplace.install(remote_id.into()).await }
    });
    detail_seen_rx.await.unwrap();
    let second = marketplace.install(remote_id.into()).await.unwrap_err();
    assert_eq!(second.code, BackendErrorCode::Busy);
    resume_tx.send(()).unwrap();
    assert!(first.await.unwrap().is_ok());
    assert!(!server.await.unwrap(), "second install reached the network");
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn failed_marketplace_install_leaves_no_pack_revision_or_success_event() {
    let remote_id = "00000000-0000-0000-0000-000000000005";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            let body = if index == 0 {
                format!(
                    r#"{{"id":"{remote_id}","slug":"broken","name":"Broken","description":"","authorLogin":"octocat","version":"1.0.0","baseMode":"structured","tags":[],"likeCount":0,"downloadCount":0,"publishedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","prompt":"Broken","state":"approved"}}"#
                )
                .into_bytes()
            } else {
                b"not a ZIP archive".to_vec()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        }
    });

    let (backend, data_dir) = marketplace_backend(base_url, "install-rollback");
    let active_style_pack = backend.get_preferences().active_style_pack_id;
    let before_packs = backend.list_style_packs(&active_style_pack).unwrap();
    let before_revision = backend.snapshot().style_pack_revision;
    let mut events = backend.subscribe();
    let error = backend
        .services()
        .marketplace
        .install(remote_id.into())
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Provider);
    assert_eq!(
        backend.list_style_packs(&active_style_pack).unwrap(),
        before_packs
    );
    assert_eq!(backend.snapshot().style_pack_revision, before_revision);
    assert!(events.try_recv().is_err());
    server.await.unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn cancelling_an_in_flight_oauth_verification_prevents_token_persistence() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (user_seen_tx, user_seen_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let responses = [
            r#"{"device_code":"raw-device-secret","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","interval":1,"expires_in":600}"#,
            r#"{"access_token":"gho_cancelled_secret"}"#,
        ];
        for body in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        let (mut user_stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = user_stream.read(&mut request).await.unwrap();
        user_seen_tx.send(()).unwrap();
        resume_rx.await.unwrap();
        let body = r#"{"login":"octocat"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        user_stream.write_all(response.as_bytes()).await.unwrap();
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-oauth-cancel-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let mut config = MarketplaceConfig::new(format!("http://{address}")).unwrap();
    config.github_device_code_url = format!("http://{address}/device").parse().unwrap();
    config.github_access_token_url = format!("http://{address}/token").parse().unwrap();
    config.github_user_url = format!("http://{address}/user").parse().unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials.clone();
    dependencies.marketplace_config = Some(config);
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap(),
    );
    let flow = backend
        .services()
        .marketplace
        .start_device_flow()
        .await
        .unwrap();
    let poll_backend = backend.clone();
    let flow_id = flow.flow_id.clone();
    let poll = tokio::spawn(async move {
        poll_backend
            .services()
            .marketplace
            .poll_device_flow(flow_id)
            .await
            .unwrap()
    });
    user_seen_rx.await.unwrap();
    backend
        .services()
        .marketplace
        .cancel_device_flow(Some(flow.flow_id))
        .await
        .unwrap();
    resume_tx.send(()).unwrap();
    assert!(matches!(poll.await.unwrap(), OAuthPollResult::Error { .. }));
    assert!(credentials
        .read(
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )
            .unwrap()
        )
        .await
        .unwrap()
        .is_none());
    server.await.unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn oauth_polling_enforces_interval_slow_down_cancellation_and_expiry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let responses = [
            r#"{"device_code":"first-device","user_code":"FIRST","verification_uri":"https://github.com/login/device","interval":1,"expires_in":30}"#,
            r#"{"error":"slow_down"}"#,
            r#"{"error":"authorization_pending"}"#,
            r#"{"device_code":"expiring-device","user_code":"EXPIRE","verification_uri":"https://github.com/login/device","interval":1,"expires_in":1}"#,
        ];
        let mut paths = Vec::new();
        for body in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            paths.push(request.lines().next().unwrap_or_default().to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        let unexpected_request =
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_ok();
        (paths, unexpected_request)
    });

    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-marketplace-oauth-timing-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let mut config = MarketplaceConfig::new(format!("http://{address}")).unwrap();
    config.github_device_code_url = format!("http://{address}/device").parse().unwrap();
    config.github_access_token_url = format!("http://{address}/token").parse().unwrap();
    config.github_user_url = format!("http://{address}/user").parse().unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.marketplace_config = Some(config);
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    let first = backend
        .services()
        .marketplace
        .start_device_flow()
        .await
        .unwrap();
    assert_eq!(
        backend
            .services()
            .marketplace
            .poll_device_flow(first.flow_id.clone())
            .await
            .unwrap(),
        OAuthPollResult::SlowDown
    );
    assert_eq!(
        backend
            .services()
            .marketplace
            .poll_device_flow(first.flow_id.clone())
            .await
            .unwrap(),
        OAuthPollResult::Pending
    );
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    assert_eq!(
        backend
            .services()
            .marketplace
            .poll_device_flow(first.flow_id.clone())
            .await
            .unwrap(),
        OAuthPollResult::Pending,
        "slow_down must extend the original interval by five seconds"
    );
    tokio::time::sleep(std::time::Duration::from_millis(5_100)).await;
    assert_eq!(
        backend
            .services()
            .marketplace
            .poll_device_flow(first.flow_id.clone())
            .await
            .unwrap(),
        OAuthPollResult::Pending
    );
    backend
        .services()
        .marketplace
        .cancel_device_flow(Some(first.flow_id.clone()))
        .await
        .unwrap();
    assert!(matches!(
        backend
            .services()
            .marketplace
            .poll_device_flow(first.flow_id)
            .await
            .unwrap(),
        OAuthPollResult::Error { .. }
    ));

    let expiring = backend
        .services()
        .marketplace
        .start_device_flow()
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    assert!(matches!(
        backend
            .services()
            .marketplace
            .poll_device_flow(expiring.flow_id)
            .await
            .unwrap(),
        OAuthPollResult::Error { message } if message.contains("过期")
    ));

    let (paths, unexpected_request) = server.await.unwrap();
    assert_eq!(paths[0], "POST /device HTTP/1.1");
    assert_eq!(paths[1], "POST /token HTTP/1.1");
    assert_eq!(paths[2], "POST /token HTTP/1.1");
    assert_eq!(paths[3], "POST /device HTTP/1.1");
    assert!(!unexpected_request);
    let _ = std::fs::remove_dir_all(data_dir);
}
