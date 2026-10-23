use super::account_sync::AccountSync;
use super::credentials::{IosCredentialStore, MemoryVault};
use super::*;
use openless_core::credentials::{CredentialKey, CredentialNamespace, SecretValue};
use openless_core::{MarketplaceConfig, MARKETPLACE_GITHUB_TOKEN_ACCOUNT};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

struct ReplyFixture {
    path: &'static str,
    body: Value,
    status: &'static str,
    delay: Duration,
}
fn response(path: &'static str, body: Value) -> ReplyFixture {
    ReplyFixture {
        path,
        body,
        status: "200 OK",
        delay: Duration::ZERO,
    }
}
fn device(url: &str) -> ReplyFixture {
    response(
        "/device",
        json!({"device_code":"private-device-code", "user_code":"ABCD-EFGH",
        "verification_uri":url,"expires_in":600,"interval":1}),
    )
}
fn authorized() -> Vec<ReplyFixture> {
    vec![
        device("https://github.com/login/device"),
        response("/token", json!({"access_token":"private-oauth-token"})),
        response("/user", json!({"login":"FixtureUser","id":12345})),
    ]
}
struct Server {
    origin: String,
    stop: Arc<AtomicBool>,
    count: Arc<AtomicU64>,
    replies: Arc<Mutex<VecDeque<ReplyFixture>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(replies: Vec<ReplyFixture>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicU64::new(0));
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let (stopped, requests, responses) = (stop.clone(), count.clone(), replies.clone());
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => panic!("fixture accept failed"),
                };
                // Darwin can inherit the listener's nonblocking flag.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 2048];
                    let n = stream.read(&mut chunk).unwrap();
                    assert!(n > 0, "fixture request incomplete");
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() <= 16384);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let reply = responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected network request");
                let head = String::from_utf8_lossy(&bytes);
                // Do not include request bodies/Authorization in assertion output.
                assert!(
                    head.lines().next().unwrap().split_whitespace().nth(1) == Some(reply.path),
                    "unexpected request path"
                );
                requests.fetch_add(1, Ordering::Release);
                thread::sleep(reply.delay);
                let body = reply.body.to_string();
                stream.write_all(format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", reply.status, body.len()).as_bytes()).unwrap();
            }
        });
        Self {
            origin,
            stop,
            count,
            replies,
            thread: Some(thread),
        }
    }
    fn assert_complete(&self) {
        assert!(
            self.replies.lock().unwrap().is_empty(),
            "fixture replies not consumed"
        );
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}
fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}
fn backend(
    dir: &std::path::Path,
    server: &Server,
    storage: Arc<MemoryVault>,
) -> (OpenLessBackend, Arc<IosCredentialStore>) {
    let credentials = Arc::new(IosCredentialStore::open(storage).unwrap());
    let mut deps = BackendDependencies::unsupported();
    deps.credential_store = credentials.clone();
    let mut config = MarketplaceConfig::new(&server.origin).unwrap();
    config.github_device_code_url = format!("{}/device", server.origin).parse().unwrap();
    config.github_access_token_url = format!("{}/token", server.origin).parse().unwrap();
    config.github_user_url = format!("{}/user", server.origin).parse().unwrap();
    deps.marketplace_config = Some(config);
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: dir.into(),
            ..Default::default()
        },
        deps,
    )
    .unwrap();
    (backend, credentials)
}
fn token_key() -> CredentialKey {
    CredentialKey::new(
        CredentialNamespace::Marketplace,
        None,
        MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
    )
    .unwrap()
}
fn poll(id: &Value) -> Value {
    json!({"command":"pollSignIn","authorizationSessionID":id})
}
fn cancel(id: &Value) -> Value {
    json!({"command":"cancelSignIn","authorizationSessionID":id})
}

#[test]
fn account_sync_real_core_device_flow_keychain_adapter_reopen_and_signout_preserve_local_data() {
    let runtime = runtime();
    runtime.block_on(async {
        let server = Server::new(authorized());
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(MemoryVault::default());
        let (backend, credentials) = backend(dir.path(), &server, storage.clone());
        backend.add_vocabulary("ConfirmedFixture".into(),Some("manual".into())).unwrap();
        let record = serde_json::from_value(json!({"id":Uuid::new_v4(),"createdAt":"2099-01-01T00:00:00Z","rawTranscript":"local history","finalText":"local history","mode":"raw","insertStatus":"notRequested","durationMs":1000})).unwrap();
        backend.append_history(record,0,Some(200)).unwrap();
        let before = (serde_json::to_value(backend.list_vocabulary().unwrap()).unwrap(),serde_json::to_value(backend.list_history().unwrap()).unwrap());
        let provider = CredentialKey::new(CredentialNamespace::Llm,Some("custom".into()),"api_key").unwrap();
        credentials.write(provider.clone(),SecretValue::new("local-provider-secret")).await.unwrap();
        let bridge = AccountSync::new(backend.services().marketplace.clone(),credentials.clone());
        let state = bridge.execute(json!({"command":"status"})).await.unwrap();
        assert_eq!(state["authState"],"signed_out");
        assert_eq!(server.count.load(Ordering::Acquire),0);
        let flow = bridge.execute(json!({"command":"beginSignIn"})).await.unwrap();
        assert_eq!(flow["verificationURL"],"https://github.com/login/device");
        assert_eq!(flow["expiresIn"],600); assert_eq!(flow["interval"],1);
        let authorized = bridge.execute(poll(&flow["authorizationSessionID"])).await.unwrap();
        assert_eq!(authorized["state"]["authState"],"signed_in");
        assert_eq!(authorized["state"]["identityState"],"verified_login_only");
        assert_eq!(authorized["state"]["account"],json!({"githubID":null,"login":"FixtureUser"}));
        assert!(authorized["state"]["capabilities"].is_null());
        assert_eq!(authorized["state"]["sync"]["available"],false);
        assert!(!authorized.to_string().contains("private-"));
        let reopened = Arc::new(IosCredentialStore::open(storage.clone()).unwrap());
        assert!(reopened.read(token_key()).await.unwrap().is_some());
        let new_bridge = AccountSync::new(backend.services().marketplace.clone(),reopened);
        let state = new_bridge.execute(json!({"command":"status"})).await.unwrap();
        assert_eq!(state["authState"],"unverified"); assert!(state["account"].is_null());
        // A replaced token must never inherit the previous login label.
        credentials.write(token_key(),SecretValue::new("other-account-token")).await.unwrap();
        assert_eq!(bridge.execute(json!({"command":"status"})).await.unwrap()["authState"],"unverified");
        assert_eq!(bridge.execute(json!({"command":"signOut"})).await.unwrap()["authState"],"signed_out");
        assert!(credentials.read(token_key()).await.unwrap().is_none());
        assert!(IosCredentialStore::open(storage).unwrap().read(token_key()).await.unwrap().is_none());
        assert!(credentials.read(provider).await.unwrap().is_some());
        assert_eq!(before.0,serde_json::to_value(backend.list_vocabulary().unwrap()).unwrap());
        assert_eq!(before.1,serde_json::to_value(backend.list_history().unwrap()).unwrap());
        // Core's local migration journal shares this root; remote sync stays absent.
        assert!(!dir.path().join("encrypted-sync/protected").exists());
        assert!(backend.cloud_sync_e2ee_status().is_err());
        assert_eq!(server.count.load(Ordering::Acquire),3); server.assert_complete();
    });
}

#[test]
fn account_sync_all_sync_controls_fail_closed_without_capture_credentials_or_network() {
    let runtime = runtime();
    runtime.block_on(async {
        let server = Server::new(vec![]);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        let bridge = AccountSync::new(backend.services().marketplace.clone(), credentials);
        for command in [
            "initialize",
            "prepareEnable",
            "lock",
            "unlock",
            "recover",
            "syncNow",
            "previewRestore",
            "applyRestore",
            "changePassword",
            "deleteRemote",
            "setEnabled",
        ] {
            let error = bridge
                .execute(json!({"command":command}))
                .await
                .unwrap_err();
            assert_eq!(error.code, BackendErrorCode::Unsupported);
            assert_eq!(error.message, "sync_scope_unavailable");
        }
        for request in [
            json!({"command":"unlock","password":"private-password"}),
            json!({"command":"status","unexpected":"private-token"}),
            json!({"command":"unknown"}),
            json!({"command":"pollSignIn","authorizationSessionID":"private-token"}),
            json!({"command":"pollSignIn","authorizationSessionID":Uuid::nil().to_string()}),
        ] {
            assert_eq!(
                bridge.execute(request).await.unwrap_err().message,
                "invalid_account_sync_request"
            );
        }
        assert_eq!(server.count.load(Ordering::Acquire), 0);
        assert!(backend.cloud_sync_e2ee_status().is_err());
        assert!(!dir.path().join("encrypted-sync/protected").exists());
    });
}

#[test]
fn account_sync_cancellation_old_session_cannot_poll_or_cancel_new_flow() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut replies = vec![device("https://github.com/login/device")];
        replies.extend(authorized());
        let server = Server::new(replies);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        let bridge = AccountSync::new(backend.services().marketplace.clone(), credentials);
        let old = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap()["authorizationSessionID"]
            .clone();
        let new = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap()["authorizationSessionID"]
            .clone();
        assert_ne!(old, new);
        bridge.execute(cancel(&old)).await.unwrap();
        assert_eq!(
            bridge.execute(poll(&old)).await.unwrap_err().message,
            "account_auth_session_stale"
        );
        assert_eq!(
            bridge.execute(poll(&new)).await.unwrap()["status"],
            "signed_in"
        );
        bridge.execute(json!({"command":"signOut"})).await.unwrap();
        assert_eq!(
            bridge.execute(poll(&new)).await.unwrap_err().message,
            "account_auth_session_stale"
        );
        server.assert_complete();
    });
}

#[test]
fn account_sync_rejects_untrusted_verification_url_and_redacts_oauth_error() {
    let runtime = runtime();
    runtime.block_on(async {
        let server = Server::new(vec![
            device("https://attacker.invalid/private-token"),
            device("https://github.com/login/device"),
            response("/token", json!({"error":"private-reflected-token"})),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        let bridge = AccountSync::new(backend.services().marketplace.clone(), credentials);
        assert_eq!(
            bridge
                .execute(json!({"command":"beginSignIn"}))
                .await
                .unwrap_err()
                .message,
            "invalid_account_auth_response"
        );
        let flow = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap();
        assert_eq!(
            bridge
                .execute(poll(&flow["authorizationSessionID"]))
                .await
                .unwrap_err()
                .message,
            "account_authorization_failed"
        );
        assert_eq!(
            bridge.execute(json!({"command":"status"})).await.unwrap()["authState"],
            "signed_out"
        );
        server.assert_complete();
    });
}

#[test]
fn account_sync_signout_serializes_after_inflight_poll_without_token_resurrection() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut replies = authorized();
        replies[2].delay = Duration::from_millis(100);
        let server = Server::new(replies);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        let bridge = Arc::new(AccountSync::new(
            backend.services().marketplace.clone(),
            credentials.clone(),
        ));
        let flow = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap();
        let polling = {
            let bridge = bridge.clone();
            tokio::spawn(async move { bridge.execute(poll(&flow["authorizationSessionID"])).await })
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while server.count.load(Ordering::Acquire) < 3 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let state = bridge.execute(json!({"command":"signOut"})).await.unwrap();
        assert_eq!(polling.await.unwrap().unwrap()["status"], "signed_in");
        assert_eq!(state["authState"], "signed_out");
        assert!(credentials.read(token_key()).await.unwrap().is_none());
        server.assert_complete();
    });
}

#[test]
fn account_sync_host_control_is_correlated_and_available_in_normal_mode() {
    use super::control_tests::{control, open_at, result};
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(MemoryVault::default()));
    let state = result(control(
        handle,
        context,
        json!({"action":"accountSync","request":{"command":"status"}}),
    ));
    assert_eq!(state["authState"], "signed_out");
    assert_eq!(state["sync"]["available"], false);
    let rejected = control(
        handle,
        context,
        json!({"action":"accountSync","request":{"command":"syncNow"}}),
    );
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["message"], "sync_scope_unavailable");
    let invalid = control(
        handle,
        context,
        json!({"action":"accountSync","request":{"command":"unlock","password":"private-password"}}),
    );
    assert_eq!(invalid["ok"], false);
    assert_eq!(invalid["message"], "invalid_account_sync_request");
    assert!(host(handle)
        .unwrap()
        .backend
        .cloud_sync_e2ee_status()
        .is_err());
    ol_ios_destroy(handle);
}

#[test]
fn account_sync_current_cancel_and_failed_authorization_preserve_existing_token_and_dictionary() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut rejected = response("/token", json!({"error":"private-upstream-body"}));
        rejected.status = "401 Unauthorized";
        let server = Server::new(vec![
            device("https://github.com/login/device"),
            device("https://github.com/login/device"),
            rejected,
        ]);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        backend
            .add_vocabulary("ExistingConfirmed".into(), None)
            .unwrap();
        credentials
            .write(token_key(), SecretValue::new("previous-account-token"))
            .await
            .unwrap();
        let words = serde_json::to_value(backend.list_vocabulary().unwrap()).unwrap();
        let bridge = AccountSync::new(backend.services().marketplace.clone(), credentials.clone());
        let old = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap();
        bridge
            .execute(cancel(&old["authorizationSessionID"]))
            .await
            .unwrap();
        assert_eq!(
            bridge
                .execute(poll(&old["authorizationSessionID"]))
                .await
                .unwrap_err()
                .message,
            "account_auth_session_stale"
        );
        let new = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap();
        assert_eq!(
            bridge
                .execute(poll(&new["authorizationSessionID"]))
                .await
                .unwrap_err()
                .message,
            "account_authorization_failed"
        );
        assert_eq!(
            bridge.execute(json!({"command":"status"})).await.unwrap()["authState"],
            "unverified"
        );
        assert!(
            credentials.read(token_key()).await.unwrap()
                == Some(SecretValue::new("previous-account-token"))
        );
        assert_eq!(
            words,
            serde_json::to_value(backend.list_vocabulary().unwrap()).unwrap()
        );
        server.assert_complete();
    });
}

#[test]
fn account_sync_cancel_serializes_with_inflight_poll_and_invalidates_later_polls() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut pending = response("/token", json!({"error":"authorization_pending"}));
        pending.delay = Duration::from_millis(100);
        let server = Server::new(vec![device("https://github.com/login/device"), pending]);
        let dir = tempfile::tempdir().unwrap();
        let (backend, credentials) = backend(dir.path(), &server, Arc::new(MemoryVault::default()));
        let bridge = Arc::new(AccountSync::new(
            backend.services().marketplace.clone(),
            credentials.clone(),
        ));
        let id = bridge
            .execute(json!({"command":"beginSignIn"}))
            .await
            .unwrap()["authorizationSessionID"]
            .clone();
        let polling = {
            let bridge = bridge.clone();
            let id = id.clone();
            tokio::spawn(async move { bridge.execute(poll(&id)).await })
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while server.count.load(Ordering::Acquire) < 2 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        bridge.execute(cancel(&id)).await.unwrap();
        assert_eq!(polling.await.unwrap().unwrap()["status"], "pending");
        assert_eq!(
            bridge.execute(poll(&id)).await.unwrap_err().message,
            "account_auth_session_stale"
        );
        assert!(credentials.read(token_key()).await.unwrap().is_none());
        server.assert_complete();
    });
}
