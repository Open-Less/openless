//! Service-level fault tests use a loopback fake OAuth/sync service and an
//! in-memory system vault. No user's data, account, or operating-system keychain.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{
    local::LocalStorage,
    service::{EncryptedSyncService, SyncServiceConfig, SyncServiceData},
    *,
};
use crate::cloud_sync_e2ee_documents::{
    self as documents, ExportedDocuments, RestoreContext, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::*;
use crate::cloud_sync_e2ee_store::gate::{ChangeOrigin, SyncChange};
use crate::credentials::SyncSecretAccount;
use crate::{
    BackendError, CredentialKey, CredentialNamespace, CredentialStore, CredentialsStatus,
    InMemoryCredentialStore, SecretValue, UserPreferences,
};

const PASSWORD: &str = "FixtureOpenLess!2684";
const OWNER: &str = "118526";
const CLIENT: &str = "Ov23liyv3nEucG7oMHNE";

#[derive(Default)]
struct Vault {
    ordinary: InMemoryCredentialStore,
    secrets: Mutex<HashMap<String, SecretValue>>,
    deny: AtomicBool,
    deny_remove: AtomicBool,
}
impl CredentialStore for Vault {
    fn status(
        &self,
        prefs: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        self.ordinary.status(prefs)
    }
    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        self.ordinary.read(key)
    }
    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.ordinary.write(key, value)
    }
    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.ordinary.remove(key)
    }
    fn read_sync_secret(
        &self,
        account: SyncSecretAccount,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let result = if self.deny.load(Ordering::Acquire) {
            Err(error("secure_storage_denied"))
        } else {
            Ok(self.secrets.lock().unwrap().get(account.as_str()).cloned())
        };
        Box::pin(async move { result })
    }
    fn write_sync_secret(
        &self,
        account: SyncSecretAccount,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = if self.deny.load(Ordering::Acquire) {
            Err(error("secure_storage_denied"))
        } else {
            self.secrets
                .lock()
                .unwrap()
                .insert(account.as_str().into(), value);
            Ok(())
        };
        Box::pin(async move { result })
    }
    fn remove_sync_secret(
        &self,
        account: SyncSecretAccount,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        if self.deny_remove.load(Ordering::Acquire) {
            return Box::pin(async { Err(error("secure_storage_denied")) });
        }
        self.secrets.lock().unwrap().remove(account.as_str());
        Box::pin(async { Ok(()) })
    }
}

struct Data {
    documents: Mutex<DocumentSet>,
    baseline: Mutex<Option<ValidatedSyncDocuments>>,
    generation: AtomicU64,
    changes: tokio::sync::watch::Sender<SyncChange>,
    fail_restore: AtomicBool,
    capture_gate: Mutex<Option<Arc<crate::cloud_sync_e2ee_store::gate::SyncWriteGate>>>,
    export_attempts: AtomicU64,
}
impl Data {
    fn new() -> Arc<Self> {
        let set = DocumentSet {
            schema_version: 1,
            exported_at: "2026-09-25T12:00:00Z".into(),
            source_device: SourceDevice {
                id: "11111111-1111-4111-8111-111111111111".into(),
                os: "macos".into(),
                arch: "aarch64".into(),
                app_version: "2.0.0-Beta.3".into(),
            },
            documents: vec![
                LogicalDocument {
                    id: "futureLayout".into(),
                    kind: DocumentKind::Preferences,
                    schema_version: 1,
                    value: json!("private-fixture-history"),
                },
                LogicalDocument {
                    id: "llm:fixture-channel".into(),
                    kind: DocumentKind::Channels,
                    schema_version: 1,
                    value: json!({"id":"fixture-channel","namespace":"llm","providerType":"openai","name":"Fixture","enabled":true,"order":0,"active":true}),
                },
                LogicalDocument {
                    id: "llm:fixture-channel".into(),
                    kind: DocumentKind::ProviderCredentials,
                    schema_version: 1,
                    value: json!({"channelId":"fixture-channel","namespace":"llm","accounts":{"ark.api_key":"sk-private-fixture-key"}}),
                },
            ],
            tombstones: vec![],
        };
        documents::validate_sync_documents(set.clone(), Revision::new(0)).unwrap();
        let (changes, _) = tokio::sync::watch::channel(SyncChange {
            generation: Revision::new(1),
            origin: ChangeOrigin::User,
        });
        Arc::new(Self {
            documents: Mutex::new(set),
            baseline: Mutex::new(None),
            generation: AtomicU64::new(1),
            changes,
            fail_restore: AtomicBool::new(false),
            capture_gate: Mutex::new(None),
            export_attempts: AtomicU64::new(0),
        })
    }
    fn edit(&self, text: &str) {
        self.documents
            .lock()
            .unwrap()
            .documents
            .iter_mut()
            .find(|d| d.id == "futureLayout")
            .unwrap()
            .value = json!(text);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.changes.send_replace(SyncChange {
            generation: Revision::new(generation),
            origin: ChangeOrigin::User,
        });
    }
}
impl SyncServiceData for Data {
    fn export(&self, _scope: SyncScope) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move {
            self.export_attempts.fetch_add(1, Ordering::AcqRel);
            if let Some(gate) = self.capture_gate.lock().unwrap().as_ref() {
                gate.coherent_generation().map_err(document_error)?;
            }
            let set = self.documents.lock().unwrap().clone();
            let baseline = self.baseline.lock().unwrap();
            let documents = match baseline.as_ref() {
                Some(base) => {
                    documents::record_missing_tombstones(set, base, "2026-09-25T12:01:00Z")
                }
                None => documents::validate_sync_documents(set, Revision::new(0)),
            }
            .map_err(document_error)?;
            Ok(ExportedDocuments {
                generation: Revision::new(self.generation.load(Ordering::Acquire)),
                documents,
            })
        })
    }
    fn restore(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move {
            if self.fail_restore.load(Ordering::Acquire) {
                return Err(error("recovery_required"));
            }
            if self.generation.load(Ordering::Acquire) != context.local_generation.get() {
                return Err(error("stale_preview"));
            }
            *self.documents.lock().unwrap() = desired.documents().clone();
            let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
            self.changes.send_replace(SyncChange {
                generation: Revision::new(generation),
                origin: ChangeOrigin::Restore,
            });
            Ok(ExportedDocuments {
                generation: Revision::new(generation),
                documents: desired,
            })
        })
    }
    fn recover(&self) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move {
            if self.fail_restore.load(Ordering::Acquire) {
                Err(error("recovery_required"))
            } else {
                Ok(())
            }
        })
    }
    fn baseline(
        &self,
        _scope: SyncScope,
        set: DocumentSet,
        revision: Revision,
    ) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move {
            *self.baseline.lock().unwrap() =
                Some(documents::validate_sync_documents(set, revision).map_err(document_error)?);
            Ok(())
        })
    }
    fn generation(&self) -> SyncResult<Revision> {
        Ok(Revision::new(self.generation.load(Ordering::Acquire)))
    }
    fn device(&self) -> SourceDevice {
        self.documents.lock().unwrap().source_device.clone()
    }
    fn changes(&self) -> tokio::sync::watch::Receiver<SyncChange> {
        self.changes.subscribe()
    }
}

#[derive(Default)]
struct Remote {
    snapshot: Option<Value>,
    revision: u64,
    receipt: Option<Value>,
    drop_response: bool,
    puts: Vec<Vec<u8>>,
    requests: usize,
    owner: String,
    delay_upload: bool,
    reject_upload: bool,
    hide_receipts: bool,
    drop_metadata_response: bool,
}
struct Server {
    origin: String,
    state: Arc<Mutex<Remote>>,
    upload_arrived: Arc<tokio::sync::Notify>,
    allow_upload: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(Remote {
            owner: OWNER.into(),
            ..Remote::default()
        }));
        let shared = state.clone();
        let upload_arrived = Arc::new(tokio::sync::Notify::new());
        let allow_upload = Arc::new(tokio::sync::Notify::new());
        let arrived = upload_arrived.clone();
        let allow = allow_upload.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                let header_end = loop {
                    let size = socket.read(&mut buffer).await.unwrap();
                    if size == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..size]);
                    if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break offset + 4;
                    }
                };
                let head = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let mut first = head.lines().next().unwrap().split_whitespace();
                let method = first.next().unwrap();
                let path = first.next().unwrap();
                let headers: HashMap<_, _> = head
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let length: usize = headers
                    .get("content-length")
                    .map_or(0, |v| v.parse().unwrap());
                while bytes.len() < header_end + length {
                    let size = socket.read(&mut buffer).await.unwrap();
                    assert!(size > 0);
                    bytes.extend_from_slice(&buffer[..size]);
                }
                let body = &bytes[header_end..header_end + length];
                if method == "DELETE" && path == "/v1/auth/session" {
                    socket.write_all(b"HTTP/1.1 204 No Content\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    continue;
                }
                let drop_metadata = method == "GET"
                    && path == "/v1/me/vault"
                    && std::mem::take(&mut shared.lock().unwrap().drop_metadata_response);
                if drop_metadata {
                    shared.lock().unwrap().requests += 1;
                    continue;
                }
                let hidden_receipt =
                    path.starts_with("/v1/me/operations/") && shared.lock().unwrap().hide_receipts;
                if hidden_receipt {
                    let body = serde_json::to_vec(&json!({"error":{"code":"operation_not_found","message":"fixture receipt expired","requestId":"dddddddd-dddd-4ddd-8ddd-dddddddddddd"}})).unwrap();
                    let reply = format!("HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    socket.write_all(reply.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                    continue;
                }
                let rejected = if method == "PUT" {
                    let mut state = shared.lock().unwrap();
                    std::mem::take(&mut state.reject_upload)
                        || headers.get("if-match")
                            != Some(&format!("\"fixture-{}\"", state.revision))
                } else {
                    false
                };
                if rejected {
                    let body = serde_json::to_vec(&json!({"error":{"code":"revision_conflict","message":"fixture CAS lost","requestId":"dddddddd-dddd-4ddd-8ddd-dddddddddddd","currentRevision":"2"}})).unwrap();
                    let reply = format!("HTTP/1.1 412 Precondition Failed\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    socket.write_all(reply.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                    continue;
                }
                let delayed = method == "PUT" && shared.lock().unwrap().delay_upload;
                if delayed {
                    arrived.notify_one();
                    allow.notified().await;
                }
                let (status, response, etag, disconnect) = {
                    let mut remote = shared.lock().unwrap();
                    remote.requests += 1;
                    let owner = remote.owner.clone();
                    let mut disconnect = false;
                    let response = match (method, path) {
                        ("GET", "/v1/capabilities") => {
                            json!({"protocolVersion":1,"cryptoProfile":"argon2id-xchacha20poly1305-v1","githubClientId":CLIENT,"maxHttpBodyBytes":25165824,"maxCiphertextBytes":16777232,"maxPlaintextJsonBytes":15728640,"idempotencyRetentionSeconds":604800,"maxBackupRetentionDays":30})
                        }
                        ("GET", "/github/user") => {
                            json!({"id":owner.parse::<u64>().unwrap(),"login":"fixture-user"})
                        }
                        ("POST", "/v1/auth/github") => {
                            json!({"protocolVersion":1,"accessToken":"a".repeat(43),"tokenType":"Bearer","expiresIn":900,"account":{"githubId":owner,"login":"fixture-user"}})
                        }
                        ("GET", "/v1/me/vault") => match &remote.snapshot {
                            None => {
                                json!({"protocolVersion":1,"state":"empty","ownerGithubId":owner,"revision":"0","vaultId":null,"keyId":null,"updatedAt":null,"payloadSchemaVersion":null,"ciphertextBytes":0,"ciphertextSha256":null,"lastOperationId":null})
                            }
                            Some(s) => {
                                json!({"protocolVersion":1,"state":"active","ownerGithubId":owner,"revision":remote.revision.to_string(),"vaultId":s["vaultId"],"keyId":s["keyId"],"updatedAt":"2026-09-25T12:00:00Z","payloadSchemaVersion":1,"ciphertextBytes":URL_SAFE_NO_PAD.decode(s["ciphertext"].as_str().unwrap()).unwrap().len(),"ciphertextSha256":s["ciphertextSha256"],"lastOperationId":s["operationId"]})
                            }
                        },
                        ("GET", "/v1/me/vault/snapshot") => remote.snapshot.clone().unwrap(),
                        ("PUT", "/v1/me/vault/snapshot") => {
                            let value: Value = serde_json::from_slice(body).unwrap();
                            assert_eq!(
                                headers["if-match"],
                                format!("\"fixture-{}\"", remote.revision)
                            );
                            assert_eq!(headers["idempotency-key"], value["operationId"]);
                            remote.revision += 1;
                            assert_eq!(value["revision"], remote.revision.to_string());
                            remote.puts.push(body.to_vec());
                            let receipt = json!({"operationId":value["operationId"],"status":"committed","kind":value["kind"],"committedRevision":value["revision"],"committedAt":"2026-09-25T12:00:00Z","vaultId":value["vaultId"],"ciphertextSha256":value["ciphertextSha256"]});
                            remote.snapshot = Some(value);
                            remote.receipt = Some(receipt.clone());
                            disconnect = std::mem::take(&mut remote.drop_response);
                            receipt
                        }
                        ("GET", path) if path.starts_with("/v1/me/operations/") => {
                            remote.receipt.clone().unwrap()
                        }
                        _ => panic!("unexpected fixture request: {method} {path}"),
                    };
                    (
                        "200 OK",
                        response,
                        format!("\"fixture-{}\"", remote.revision),
                        disconnect,
                    )
                };
                if disconnect {
                    continue;
                }
                let body = serde_json::to_vec(&response).unwrap();
                let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nIdempotency-Replayed: false\r\nContent-Length: {}\r\nETag: {etag}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        Self {
            origin,
            state,
            upload_arrived,
            allow_upload,
            task,
        }
    }
}

struct Fixture {
    service: EncryptedSyncService,
    data: Arc<Data>,
    vault: Arc<Vault>,
    root: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    async fn new(server: &Server) -> Self {
        let root =
            std::env::temp_dir().join(format!("openless-e2ee-service-{}", uuid::Uuid::new_v4()));
        let repos = crate::BackendRepositories::open(&root).unwrap();
        let vault = Arc::new(Vault::default());
        vault
            .write(
                CredentialKey::new(CredentialNamespace::Marketplace, None, "github.oauth_token")
                    .unwrap(),
                SecretValue::new("fixture-github-token"),
            )
            .await
            .unwrap();
        let mut config = crate::MarketplaceConfig::new(&server.origin).unwrap();
        config.github_user_url = format!("{}/github/user", server.origin).parse().unwrap();
        let events =
            crate::events::BackendEventPublisher::new(Arc::new(crate::events::EventBus::new(100)));
        let marketplace = Arc::new(
            crate::marketplace::MarketplaceService::new(
                config,
                vault.clone(),
                repos.preferences,
                repos.style_packs,
                events.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap(),
        );
        let data = Data::new();
        let local = LocalStorage::new(
            root.join("protected"),
            server.origin.clone(),
            data.device().id.clone(),
            vault.clone(),
        );
        let service = EncryptedSyncService::new(
            SyncServiceConfig {
                origin: server.origin.clone(),
                github_client_id: CLIENT.into(),
            },
            marketplace,
            local,
            data.clone(),
            events,
        );
        Self {
            service,
            data,
            vault,
            root,
        }
    }
    async fn prepare(&self) {
        assert_eq!(
            self.service
                .prepare_enable(CONSENT_VERSION.into())
                .await
                .unwrap()
                .next_step,
            EnableStep::Create
        );
    }
    async fn create(&self, remember: bool) -> SyncResult<EncryptedSyncStatus> {
        self.service
            .create(
                PASSWORD.into(),
                PASSWORD.into(),
                remember,
                CONSENT_VERSION.into(),
                "0".into(),
            )
            .await
    }
}

#[tokio::test]
async fn disabled_start_and_changes_make_no_network_requests_or_key_entries() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture
        .service
        .start(Arc::new(crate::TokioTaskSpawner))
        .await
        .unwrap();
    fixture.data.edit("changed while disabled");
    tokio::time::sleep(std::time::Duration::from_millis(850)).await;
    assert_eq!(server.state.lock().unwrap().requests, 0);
    assert!(fixture.vault.secrets.lock().unwrap().is_empty());
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn cached_connection_rebuilds_after_captured_proxy_policy_changes() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    // Inject only this connection's captured setting. No global proxy setting
    // or environment variable changes, and every endpoint is loopback/direct.
    fixture
        .service
        .connect_with_proxy_setting_for_test(false)
        .await
        .unwrap();
    let direct_requests = server.state.lock().unwrap().requests;
    fixture
        .service
        .connect_with_proxy_setting_for_test(false)
        .await
        .unwrap();
    assert_eq!(server.state.lock().unwrap().requests, direct_requests);
    fixture
        .service
        .connect_with_proxy_setting_for_test(true)
        .await
        .unwrap();
    let enabled_requests = server.state.lock().unwrap().requests;
    assert!(
        enabled_requests >= direct_requests + 2,
        "new policy must recheck capabilities and exchange a new session"
    );
    fixture
        .service
        .connect_with_proxy_setting_for_test(true)
        .await
        .unwrap();
    assert_eq!(server.state.lock().unwrap().requests, enabled_requests);
    fixture
        .service
        .connect_with_proxy_setting_for_test(false)
        .await
        .unwrap();
    assert!(server.state.lock().unwrap().requests >= enabled_requests + 2);
    assert!(server.state.lock().unwrap().puts.is_empty());
    fixture.service.shutdown().await;
}

async fn wait_for_auto_sync(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !ready() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("automatic sync did not reach the expected state");
}

async fn start_ready_auto_sync(fixture: &Fixture) {
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    let before = fixture.data.export_attempts.load(Ordering::Acquire);
    fixture
        .service
        .start(Arc::new(crate::TokioTaskSpawner))
        .await
        .unwrap();
    wait_for_auto_sync(|| {
        fixture.data.export_attempts.load(Ordering::Acquire) > before
            && fixture.service.status().sync_state == SyncState::Ready
    })
    .await;
}

#[tokio::test]
async fn auto_sync_retries_capture_after_local_only_writer_without_another_user_event() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    let gate = crate::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
        fixture.root.join("capture-gate.json"),
    )
    .unwrap();
    *fixture.data.capture_gate.lock().unwrap() = Some(gate.clone());
    let changes = gate.subscribe();
    let generation = gate.generation().unwrap();
    let local_only = gate.begin_mutation().unwrap();
    let before = fixture.data.export_attempts.load(Ordering::Acquire);
    fixture.data.edit("user edit preceding a local-only write");
    wait_for_auto_sync(|| {
        fixture.data.export_attempts.load(Ordering::Acquire) > before
            && fixture
                .service
                .status()
                .last_error
                .as_ref()
                .is_some_and(|failure| failure.code == "sync_documents_source_changed")
    })
    .await;
    local_only.commit(ChangeOrigin::LocalOnly).unwrap();
    assert_eq!(gate.generation().unwrap(), generation);
    assert!(
        !changes.has_changed().unwrap(),
        "LocalOnly completion must not be mistaken for another user edit"
    );
    wait_for_auto_sync(|| {
        fixture
            .service
            .status()
            .last_synced_local_generation
            .as_deref()
            == Some("2")
    })
    .await;
    assert_eq!(fixture.service.status().sync_state, SyncState::Ready);
    assert_eq!(server.state.lock().unwrap().puts.len(), 2);
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn auto_sync_capture_retry_is_bounded_while_local_writer_stays_busy() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    let gate = crate::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
        fixture.root.join("capture-gate.json"),
    )
    .unwrap();
    *fixture.data.capture_gate.lock().unwrap() = Some(gate.clone());
    let local_only = gate.begin_mutation().unwrap();
    let before = fixture.data.export_attempts.load(Ordering::Acquire);
    fixture
        .data
        .edit("pending while local writer remains active");
    wait_for_auto_sync(|| {
        fixture.data.export_attempts.load(Ordering::Acquire) == before + 5
            && fixture.service.status().last_error.is_some()
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert_eq!(
        fixture.data.export_attempts.load(Ordering::Acquire),
        before + 5
    );
    assert_eq!(server.state.lock().unwrap().puts.len(), 1);
    local_only.commit(ChangeOrigin::LocalOnly).unwrap();
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn auto_sync_does_not_retry_an_unknown_upload_outcome() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    server.state.lock().unwrap().drop_response = true;
    fixture.data.edit("upload with a lost acknowledgement");
    wait_for_auto_sync(|| fixture.service.status().sync_state == SyncState::OutcomeUnknown).await;
    let requests = server.state.lock().unwrap().requests;
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert_eq!(server.state.lock().unwrap().requests, requests);
    assert_eq!(server.state.lock().unwrap().puts.len(), 2);
    assert!(fixture.service.status().pending_operation_id.is_some());
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn auto_sync_does_not_retry_network_failure() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    server.state.lock().unwrap().drop_metadata_response = true;
    fixture
        .data
        .edit("pending after a metadata transport failure");
    wait_for_auto_sync(|| {
        fixture
            .service
            .status()
            .last_error
            .as_ref()
            .is_some_and(|failure| failure.code == "transport_failed")
    })
    .await;
    let requests = server.state.lock().unwrap().requests;
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert_eq!(server.state.lock().unwrap().requests, requests);
    assert_eq!(server.state.lock().unwrap().puts.len(), 1);
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn auto_sync_local_retry_releases_runtime_and_respects_pause() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    let gate = crate::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
        fixture.root.join("capture-gate.json"),
    )
    .unwrap();
    *fixture.data.capture_gate.lock().unwrap() = Some(gate.clone());
    let local_only = gate.begin_mutation().unwrap();
    fixture.data.edit("pause before a delayed local retry");
    wait_for_auto_sync(|| {
        fixture
            .service
            .status()
            .last_error
            .as_ref()
            .is_some_and(|failure| failure.code == "sync_documents_source_changed")
    })
    .await;
    tokio::time::timeout(
        std::time::Duration::from_millis(200),
        fixture.service.set_enabled(false),
    )
    .await
    .expect("local retry must release the runtime lock")
    .unwrap();
    let attempts = fixture.data.export_attempts.load(Ordering::Acquire);
    local_only.commit(ChangeOrigin::LocalOnly).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert_eq!(
        fixture.data.export_attempts.load(Ordering::Acquire),
        attempts
    );
    assert_eq!(server.state.lock().unwrap().puts.len(), 1);
    fixture.service.set_enabled(true).await.unwrap();
    wait_for_auto_sync(|| {
        fixture
            .service
            .status()
            .last_synced_local_generation
            .as_deref()
            == Some("2")
    })
    .await;
    assert_eq!(server.state.lock().unwrap().puts.len(), 2);
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn auto_sync_local_retry_cannot_reconcile_a_later_manual_unknown_upload() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    start_ready_auto_sync(&fixture).await;
    let gate = crate::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
        fixture.root.join("capture-gate.json"),
    )
    .unwrap();
    *fixture.data.capture_gate.lock().unwrap() = Some(gate.clone());
    let local_only = gate.begin_mutation().unwrap();
    fixture
        .data
        .edit("manual upload takes over a local-busy automatic retry");
    wait_for_auto_sync(|| {
        fixture
            .service
            .status()
            .last_error
            .as_ref()
            .is_some_and(|failure| failure.code == "sync_documents_source_changed")
    })
    .await;
    local_only.commit(ChangeOrigin::LocalOnly).unwrap();
    server.state.lock().unwrap().drop_response = true;
    assert_eq!(
        fixture.service.sync_now().await.unwrap_err().message,
        "outcome_unknown"
    );
    let pending = fixture.service.status().pending_operation_id;
    let requests = server.state.lock().unwrap().requests;
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert_eq!(
        server.state.lock().unwrap().requests,
        requests,
        "the obsolete retry must not query/replay the newer pending operation"
    );
    assert_eq!(fixture.service.status().pending_operation_id, pending);
    assert_eq!(
        fixture.service.status().sync_state,
        SyncState::OutcomeUnknown
    );
    assert_eq!(server.state.lock().unwrap().puts.len(), 2);
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn create_encrypts_real_data_and_wrong_password_never_changes_local_data() {
    let server = Server::start().await;
    let first = Fixture::new(&server).await;
    first.prepare().await;
    let status = first.create(false).await.unwrap();
    assert!(status.enabled);
    assert_eq!(status.key_state, KeyState::Unlocked);
    assert_eq!(status.remote_revision.as_deref(), Some("1"));
    let raw = server.state.lock().unwrap().puts[0].clone();
    let request = std::str::from_utf8(&raw).unwrap();
    for private in [
        PASSWORD,
        "sk-private-fixture-key",
        "private-fixture-history",
        "fixture-github-token",
    ] {
        assert!(!request.contains(private));
    }
    let second = Fixture::new(&server).await;
    assert_eq!(
        second
            .service
            .prepare_enable(CONSENT_VERSION.into())
            .await
            .unwrap()
            .next_step,
        EnableStep::Unlock
    );
    let before = second.data.documents.lock().unwrap().clone();
    assert!(second
        .service
        .unlock("WrongPassword!9876".into(), true)
        .await
        .is_err());
    assert_eq!(*second.data.documents.lock().unwrap(), before);
    assert_eq!(second.service.status().key_state, KeyState::Locked);
    assert!(!second.service.status().enabled);
    assert_eq!(server.state.lock().unwrap().revision, 1);
    let status = second.service.unlock(PASSWORD.into(), false).await.unwrap();
    assert_eq!(status.key_state, KeyState::Unlocked);
    assert!(
        !status.enabled,
        "a successful unlock must still require first restore review"
    );
}

#[tokio::test]
async fn lost_upload_response_reconciles_one_commit_without_another_put() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    server.state.lock().unwrap().drop_response = true;
    let failed = fixture.create(true).await.unwrap_err();
    assert_eq!(failed.message, "outcome_unknown");
    assert!(fixture.service.status().pending_operation_id.is_some());
    let status = fixture.service.sync_now().await.unwrap();
    assert!(status.pending_operation_id.is_none());
    assert!(status.last_successful_sync_at.is_some());
    assert_eq!(server.state.lock().unwrap().puts.len(), 1);
    assert_eq!(server.state.lock().unwrap().revision, 1);
}

#[tokio::test]
async fn changes_during_upload_remain_pending_after_its_receipt() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    server.state.lock().unwrap().delay_upload = true;
    let service = fixture.service.clone();
    let upload = tokio::spawn(async move {
        service
            .create(
                PASSWORD.into(),
                PASSWORD.into(),
                false,
                CONSENT_VERSION.into(),
                "0".into(),
            )
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        server.upload_arrived.notified(),
    )
    .await
    .unwrap();
    fixture.data.edit("new text while upload in flight");
    server.allow_upload.notify_one();
    let status = upload.await.unwrap().unwrap();
    assert_eq!(status.last_synced_local_generation.as_deref(), Some("1"));
    assert_eq!(status.local_generation, "2");
    assert_eq!(status.sync_state, SyncState::Pending);
}

#[tokio::test]
async fn denied_system_key_storage_aborts_before_private_upload() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.vault.deny.store(true, Ordering::Release);
    assert!(fixture
        .service
        .prepare_enable(CONSENT_VERSION.into())
        .await
        .is_err());
    assert!(server.state.lock().unwrap().puts.is_empty());
    assert!(fixture.vault.secrets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn review_rotation_lost_response_must_not_keep_old_key_after_receipt() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    server.state.lock().unwrap().drop_response = true;
    let failure = fixture
        .service
        .change_password(
            PASSWORD.into(),
            "FreshOpenLess!8436".into(),
            "FreshOpenLess!8436".into(),
            true,
        )
        .await
        .unwrap_err();
    assert_eq!(failure.message, "outcome_unknown");
    let reconciled = fixture.service.sync_now().await;
    assert!(
        reconciled.is_ok(),
        "receipt of password rotation must load its matching key: {:?}",
        reconciled.as_ref().err()
    );
}

#[tokio::test]
async fn review_lock_must_drop_memory_key_even_when_recovery_fails() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    fixture.data.fail_restore.store(true, Ordering::Release);
    assert!(fixture.service.lock().await.is_err());
    assert_eq!(
        fixture.service.status().key_state,
        KeyState::Locked,
        "lock must not preserve unlocked key on recovery error"
    );
}

#[tokio::test]
async fn review_pending_receipt_must_not_undo_a_later_explicit_lock() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    fixture.data.edit("changed data");
    server.state.lock().unwrap().drop_response = true;
    assert_eq!(
        fixture.service.sync_now().await.unwrap_err().message,
        "outcome_unknown"
    );
    fixture.vault.deny_remove.store(true, Ordering::Release);
    assert_eq!(
        fixture.service.lock().await.unwrap_err().message,
        "secure_storage_denied"
    );
    assert_eq!(fixture.service.status().key_state, KeyState::Locked);
    let _ = fixture.service.sync_now().await;
    assert_eq!(
        fixture.service.status().key_state,
        KeyState::Locked,
        "replaying a prior operation must preserve later explicit lock intent"
    );
}

#[tokio::test]
async fn review_first_submit_explicit_cas_rejection_must_not_stick_as_unknown() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    fixture.data.edit("local edit racing another device");
    server.state.lock().unwrap().reject_upload = true;
    let rejection = fixture.service.sync_now().await.unwrap_err();
    assert_eq!(rejection.message, "revision_conflict");
    assert!(
        fixture.service.status().pending_operation_id.is_none(),
        "a first submit definitively rejected by CAS must allow a fresh metadata/merge cycle"
    );
}

#[tokio::test]
async fn cancelled_local_write_cannot_overwrite_a_later_lock_record() {
    struct Paused {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }
    impl serde::Serialize for Paused {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.entered.notify_one();
            let (mutex, cv) = &*self.release;
            let guard = mutex.lock().unwrap();
            drop(cv.wait_while(guard, |released| !*released).unwrap());
            json!({"enabled": true, "rememberKey": true}).serialize(serializer)
        }
    }
    let root = std::env::temp_dir().join(format!(
        "openless-e2ee-local-cancel-{}",
        uuid::Uuid::new_v4()
    ));
    let store = LocalStorage::new(
        root.clone(),
        "https://sync.example".into(),
        "fixture-device".into(),
        Arc::new(Vault::default()),
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let old_store = store.clone();
    let old = tokio::spawn({
        let entered = entered.clone();
        let release = release.clone();
        async move {
            old_store
                .write("device", "client", Paused { entered, release })
                .await
        }
    });
    entered.notified().await;
    old.abort();
    let _ = old.await;
    let current_store = store.clone();
    let mut current = tokio::spawn(async move {
        current_store
            .write(
                "device",
                "client",
                json!({"enabled":false,"rememberKey":false}),
            )
            .await
    });
    let early = tokio::time::timeout(std::time::Duration::from_millis(30), &mut current).await;
    {
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
    }
    let was_blocked = early.is_err();
    if was_blocked {
        current.await.unwrap().unwrap();
    }
    assert!(
        was_blocked,
        "new settings must queue behind the actual old blocking worker"
    );
    let value: Value = store.read("device", "client").await.unwrap().unwrap();
    assert_eq!(value, json!({"enabled":false,"rememberKey":false}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn device_identity_is_create_only_and_never_replaced_over_existing_ciphertext() {
    let root =
        std::env::temp_dir().join(format!("openless-e2ee-device-id-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let first = super::adapter::load_device_id(&root).unwrap();
    assert_eq!(first, super::adapter::load_device_id(&root).unwrap());
    assert!(!super::local::durable_create(&root.join("device-id"), b"replacement").unwrap());
    assert_eq!(
        first,
        std::fs::read_to_string(root.join("device-id")).unwrap()
    );
    std::fs::create_dir_all(root.join("protected")).unwrap();
    std::fs::write(root.join("protected/state.enc"), b"existing ciphertext").unwrap();
    std::fs::remove_file(root.join("device-id")).unwrap();
    assert_eq!(
        super::adapter::load_device_id(&root).unwrap_err().message,
        "recovery_required"
    );
    assert!(!root.join("device-id").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn expired_unknown_receipt_can_be_resolved_by_explicit_review_of_a_newer_head() {
    let server = Server::start().await;
    let first = Fixture::new(&server).await;
    first.prepare().await;
    first.create(true).await.unwrap();
    first.data.edit("first device change");
    server.state.lock().unwrap().drop_response = true;
    assert_eq!(
        first.service.sync_now().await.unwrap_err().message,
        "outcome_unknown"
    );
    let second = Fixture::new(&server).await;
    second
        .service
        .prepare_enable(CONSENT_VERSION.into())
        .await
        .unwrap();
    second.service.unlock(PASSWORD.into(), false).await.unwrap();
    let preview = second.service.preview_restore("2".into()).await.unwrap();
    second
        .service
        .apply_restore(preview.preview_id, RestoreMode::Replace, vec![])
        .await
        .unwrap();
    second.data.edit("second device change");
    second.service.sync_now().await.unwrap();
    assert_eq!(server.state.lock().unwrap().revision, 3);
    server.state.lock().unwrap().hide_receipts = true;
    assert_eq!(
        first.service.sync_now().await.unwrap_err().message,
        "outcome_unknown"
    );
    let preview = first.service.preview_restore("3".into()).await.unwrap();
    assert!(preview.unconfirmed_operation_id.is_some());
    assert!(!preview.conflicts.is_empty());
    let choices = preview
        .conflicts
        .iter()
        .map(|item| SyncConflictChoice {
            id: item.id.clone(),
            side: ConflictSide::Cloud,
        })
        .collect();
    let status = first
        .service
        .apply_restore(preview.preview_id, RestoreMode::Merge, choices)
        .await
        .unwrap();
    assert!(status.pending_operation_id.is_none());
    first.data.edit("a reviewed new operation");
    first.service.sync_now().await.unwrap();
    assert_eq!(server.state.lock().unwrap().revision, 4);
}

#[tokio::test]
async fn round2_lock_must_remove_key_of_retired_unknown_password_rotation() {
    let server = Server::start().await;
    let first = Fixture::new(&server).await;
    first.prepare().await;
    first.create(true).await.unwrap();
    server.state.lock().unwrap().drop_response = true;
    let second_password = "MiddleOpenLess!3827";
    assert_eq!(
        first
            .service
            .change_password(
                PASSWORD.into(),
                second_password.into(),
                second_password.into(),
                true
            )
            .await
            .unwrap_err()
            .message,
        "outcome_unknown"
    );
    let second = Fixture::new(&server).await;
    second
        .service
        .prepare_enable(CONSENT_VERSION.into())
        .await
        .unwrap();
    second
        .service
        .unlock(second_password.into(), false)
        .await
        .unwrap();
    let preview = second.service.preview_restore("2".into()).await.unwrap();
    second
        .service
        .apply_restore(preview.preview_id, RestoreMode::Replace, vec![])
        .await
        .unwrap();
    let third_password = "ThirdOpenLess!4829";
    second
        .service
        .change_password(
            second_password.into(),
            third_password.into(),
            third_password.into(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(server.state.lock().unwrap().revision, 3);
    server.state.lock().unwrap().hide_receipts = true;
    assert_eq!(
        first.service.sync_now().await.unwrap_err().message,
        "outcome_unknown"
    );
    first
        .service
        .unlock(third_password.into(), true)
        .await
        .unwrap();
    let preview = first.service.preview_restore("3".into()).await.unwrap();
    assert!(preview.unconfirmed_operation_id.is_some());
    first.vault.deny_remove.store(true, Ordering::Release);
    let denied = first
        .service
        .apply_restore(preview.preview_id.clone(), RestoreMode::Replace, vec![])
        .await
        .unwrap_err();
    assert_eq!(denied.message, "secure_storage_denied");
    assert!(
        first.service.status().pending_operation_id.is_some(),
        "failed key deletion must preserve the pending cleanup index"
    );
    first.vault.deny_remove.store(false, Ordering::Release);
    first
        .service
        .apply_restore(preview.preview_id, RestoreMode::Replace, vec![])
        .await
        .unwrap();
    first.service.lock().await.unwrap();
    let secrets = first.vault.secrets.lock().unwrap();
    assert!(
        !secrets
            .keys()
            .any(|key| key.starts_with("cloud-sync.e2ee.key.")),
        "explicit lock must forget even the retired uncertain password key"
    );
}

#[tokio::test]
async fn round2_refuses_previously_seen_revision_rollback_before_local_changes() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    let old = server.state.lock().unwrap().snapshot.clone();
    fixture.data.edit("newest local value");
    fixture.service.sync_now().await.unwrap();
    let before = fixture.data.documents.lock().unwrap().clone();
    let puts = server.state.lock().unwrap().puts.len();
    {
        let mut remote = server.state.lock().unwrap();
        remote.snapshot = old;
        remote.revision = 1;
    }
    assert_eq!(
        fixture.service.sync_now().await.unwrap_err().message,
        "revision_rollback"
    );
    assert_eq!(
        fixture.service.status().remote_revision.as_deref(),
        Some("2")
    );
    assert_eq!(fixture.data.documents.lock().unwrap().clone(), before);
    assert_eq!(server.state.lock().unwrap().puts.len(), puts);
}

#[tokio::test]
async fn round2_sync_key_deletion_failure_must_not_prevent_account_sign_out() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.prepare().await;
    fixture.create(true).await.unwrap();
    fixture.vault.deny_remove.store(true, Ordering::Release);
    assert_eq!(
        fixture.service.sign_out().await.unwrap_err().message,
        "secure_storage_denied"
    );
    let oauth = fixture
        .vault
        .read(
            CredentialKey::new(CredentialNamespace::Marketplace, None, "github.oauth_token")
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        oauth.is_none(),
        "a sync key cleanup failure must still attempt ordinary account token removal"
    );
    assert_eq!(fixture.service.status().auth_state, AuthState::SignedOut);
}

#[path = "setup_prompt_tests.rs"]
mod setup_prompt_tests;
