use openless_core::{BackendConfig, BackendDependencies, BackendErrorCode, OpenLessBackend};
use openless_core::{
    CloudSyncUiPreferences, CredentialKey, CredentialNamespace, CredentialStore,
    InMemoryCredentialStore, MarketplaceConfig, SecretValue, StylePackKind, SyncFontScale,
    SyncLocale, MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const ICON: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

#[derive(Default)]
struct RemoteState {
    revision: u64,
    payload: Option<Value>,
    response_status: Option<u16>,
    requests: Vec<Value>,
}

struct Server {
    url: String,
    state: Arc<Mutex<RemoteState>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut block = [0u8; 8192];
                    let read = socket.read(&mut block).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&block[..read]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..index]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (index + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let mut block = [0u8; 8192];
                    let read = socket.read(&mut block).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&block[..read]);
                }
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let method = headers.split_whitespace().next().unwrap();
                assert_eq!(headers.split_whitespace().nth(1).unwrap(), "/me/sync");
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer cloud-fixture-token"));
                let request: Value = if length > 0 {
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                } else {
                    Value::Null
                };
                let (status, response) = {
                    let mut state = shared.lock().unwrap();
                    state.requests.push(request.clone());
                    if let Some(status) = state.response_status {
                        (status, json!({"error":"fixture_error"}))
                    } else if method != "GET"
                        && request["baseRevision"].as_u64() != Some(state.revision)
                    {
                        (
                            409,
                            json!({"error":"revision_conflict","revision":state.revision,"updatedAt":"2026-09-08T10:00:00Z"}),
                        )
                    } else {
                        if method == "PUT" {
                            state.revision += 1;
                            state.payload = Some(request["payload"].clone());
                        }
                        if method == "DELETE" {
                            state.revision += 1;
                            state.payload = None;
                        }
                        (
                            200,
                            json!({"schemaVersion":1,"revision":state.revision,"updatedAt":if state.revision == 0 { None } else { Some("2026-09-08T10:00:00Z") },"payload":state.payload}),
                        )
                    }
                };
                let body = response.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self { url, state, task }
    }
}

struct Device {
    backend: OpenLessBackend,
    root: PathBuf,
    credentials: Arc<InMemoryCredentialStore>,
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Device {
    async fn new(server: &Server) -> Self {
        let root =
            std::env::temp_dir().join(format!("openless-cloud-device-{}", uuid::Uuid::new_v4()));
        let credentials = Arc::new(InMemoryCredentialStore::default());
        credentials
            .write(
                CredentialKey::new(
                    CredentialNamespace::Marketplace,
                    None,
                    MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("cloud-fixture-token"),
            )
            .await
            .unwrap();
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Asr, Some("fixture".into()), "api_key")
                    .unwrap(),
                SecretValue::new("never-sync-asr-secret"),
            )
            .await
            .unwrap();
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        dependencies.marketplace_config = Some(MarketplaceConfig::new(&server.url).unwrap());
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: root.clone(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .unwrap();
        Self {
            backend,
            root,
            credentials,
        }
    }

    fn documents(&self) -> Vec<Option<Vec<u8>>> {
        [
            "preferences.json",
            "dictionary.json",
            "correction-rules.json",
            "style-packs.json",
        ]
        .iter()
        .map(|name| std::fs::read(self.root.join(name)).ok())
        .collect()
    }
}

#[tokio::test]
async fn cloud_sync_requires_an_official_service_and_never_invents_an_empty_backup() {
    let root = std::env::temp_dir().join(format!("openless-cloud-sync-{}", uuid::Uuid::new_v4()));
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: root.clone(),
            ..BackendConfig::default()
        },
        BackendDependencies::unsupported(),
    )
    .unwrap();
    let error = backend.cloud_sync_status().await.unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Unsupported);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn portable_data_round_trips_without_uploading_credentials_or_device_settings() {
    use base64::Engine;
    let server = Server::new().await;
    let source = Device::new(&server).await;
    let word = source
        .backend
        .add_vocabulary("OpenLess".into(), Some("产品名称".into()))
        .unwrap();
    let rule = source
        .backend
        .add_correction_rule("Open Less".into(), "OpenLess".into())
        .unwrap();
    let mut pack = source.backend.get_style_pack("builtin.structured").unwrap();
    pack.id = "custom.cloud".into();
    pack.kind = StylePackKind::Imported;
    pack.name = "Cloud writing".into();
    pack.prompt = "Use clear sentences.".into();
    source.backend.create_style_pack(pack).unwrap();
    source
        .backend
        .set_style_pack_icon(
            "custom.cloud",
            Some(
                &base64::engine::general_purpose::STANDARD
                    .decode(ICON)
                    .unwrap(),
            ),
        )
        .unwrap();
    source.backend.activate_style_pack("custom.cloud").unwrap();
    let mut preferences = source.backend.get_preferences();
    preferences.coding_agent_workdir = Some("/private/source-workspace".into());
    preferences.coding_agent_permission_mode = "bypassPermissions".into();
    preferences.remote_input_pin = "never-sync-device-pin".into();
    preferences.active_asr_provider = "source-only-asr".into();
    preferences.theme_mode = openless_core::shared_types::ThemeMode::Dark;
    source
        .backend
        .repositories()
        .preferences
        .set(preferences)
        .unwrap();
    let empty = source.backend.cloud_sync_status().await.unwrap();
    assert!(!empty.has_snapshot);
    let saved = source
        .backend
        .cloud_sync_upload(
            0,
            CloudSyncUiPreferences {
                locale: Some(SyncLocale::De),
                font_scale: Some(SyncFontScale::Large),
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.counts.dictionary, 1);
    assert_eq!(saved.counts.corrections, 1);
    let uploaded = server.state.lock().unwrap().payload.clone().unwrap();
    let serialized = uploaded.to_string();
    for private in [
        "cloud-fixture-token",
        "never-sync-asr-secret",
        "never-sync-device-pin",
        "/private/source-workspace",
        "codingAgent",
        "activeAsrProvider",
        "iconPath",
    ] {
        assert!(
            !serialized.contains(private),
            "private field or value was uploaded: {private}"
        );
    }

    let target = Device::new(&server).await;
    target
        .backend
        .add_vocabulary("Local replacement".into(), None)
        .unwrap();
    let mut local = target.backend.get_preferences();
    local.coding_agent_workdir = Some("/private/target-workspace".into());
    local.coding_agent_permission_mode = "plan".into();
    local.active_asr_provider = "target-asr".into();
    local.remote_input_pin = "target-pin".into();
    target
        .backend
        .repositories()
        .preferences
        .set(local)
        .unwrap();
    let result = target.backend.cloud_sync_restore().await.unwrap();
    assert_eq!(result.status, saved);
    assert_eq!(result.ui_preferences.locale, Some(SyncLocale::De));
    assert_eq!(result.ui_preferences.font_scale, Some(SyncFontScale::Large));
    assert_eq!(target.backend.list_vocabulary().unwrap(), vec![word]);
    assert_eq!(target.backend.list_correction_rules().unwrap(), vec![rule]);
    assert_eq!(
        target
            .backend
            .get_style_pack("custom.cloud")
            .unwrap()
            .prompt,
        "Use clear sentences."
    );
    assert_eq!(
        target.backend.read_style_pack_icon("custom.cloud").unwrap(),
        Some(format!("data:image/png;base64,{ICON}"))
    );
    let preferences = target.backend.get_preferences();
    assert_eq!(
        preferences.theme_mode,
        openless_core::shared_types::ThemeMode::Dark
    );
    assert_eq!(preferences.active_style_pack_id, "custom.cloud");
    assert_eq!(preferences.active_asr_provider, "target-asr");
    assert_eq!(preferences.coding_agent_permission_mode, "plan");
    assert_eq!(
        preferences.coding_agent_workdir.as_deref(),
        Some("/private/target-workspace")
    );
    assert_eq!(preferences.remote_input_pin, "target-pin");
    let secret = target
        .credentials
        .read(
            CredentialKey::new(CredentialNamespace::Asr, Some("fixture".into()), "api_key")
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(secret.expose_secret(), "never-sync-asr-secret");
    let status_json = serde_json::to_value(result).unwrap();
    assert!(status_json.get("payload").is_none());
    assert!(!status_json.to_string().contains("OpenLess"));
    let reopened = openless_core::BackendRepositories::open(&target.root).unwrap();
    assert_eq!(
        reopened.preferences.get().active_style_pack_id,
        "custom.cloud"
    );
}

#[tokio::test]
async fn conflicts_and_cloud_deletion_leave_local_documents_unchanged() {
    let server = Server::new().await;
    let device = Device::new(&server).await;
    device
        .backend
        .add_vocabulary("keep local".into(), None)
        .unwrap();
    device
        .backend
        .cloud_sync_upload(0, CloudSyncUiPreferences::default())
        .await
        .unwrap();
    let before = device.documents();
    let error = device
        .backend
        .cloud_sync_upload(0, CloudSyncUiPreferences::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Busy);
    assert_eq!(error.details.unwrap()["currentRevision"], 1);
    assert_eq!(device.documents(), before);
    let deleted = device.backend.cloud_sync_delete(1).await.unwrap();
    assert_eq!(deleted.revision, 2);
    assert!(!deleted.has_snapshot);
    assert_eq!(device.documents(), before);
    assert_eq!(
        device.backend.cloud_sync_restore().await.unwrap_err().code,
        BackendErrorCode::InvalidState
    );
    assert_eq!(device.documents(), before);
}

#[tokio::test]
async fn invalid_remote_data_and_write_preparation_failure_preserve_local_state() {
    let server = Server::new().await;
    let device = Device::new(&server).await;
    device
        .backend
        .add_vocabulary("keep local".into(), None)
        .unwrap();
    device
        .backend
        .cloud_sync_upload(0, CloudSyncUiPreferences::default())
        .await
        .unwrap();
    let good = server.state.lock().unwrap().payload.clone();
    let before = device.documents();
    server.state.lock().unwrap().payload.as_mut().unwrap()["preferences"]["codingAgentExe"] =
        json!("/tmp/untrusted-executable");
    assert_eq!(
        device.backend.cloud_sync_restore().await.unwrap_err().code,
        BackendErrorCode::Provider
    );
    assert_eq!(device.documents(), before);
    server.state.lock().unwrap().payload = good;
    let path = device.root.join("preferences.json");
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::create_dir(&path).unwrap();
    let memory_before = serde_json::to_value(device.backend.get_preferences()).unwrap();
    let failing_before = device.documents();
    assert_eq!(
        device.backend.cloud_sync_restore().await.unwrap_err().code,
        BackendErrorCode::Persistence
    );
    assert_eq!(device.documents(), failing_before);
    assert_eq!(
        serde_json::to_value(device.backend.get_preferences()).unwrap(),
        memory_before
    );
}

#[tokio::test]
async fn unsupported_server_auth_failures_and_logout_do_not_invent_success() {
    let server = Server::new().await;
    let device = Device::new(&server).await;
    let before = device.documents();
    for (http, code) in [
        (404, BackendErrorCode::Unsupported),
        (401, BackendErrorCode::PermissionDenied),
        (503, BackendErrorCode::Provider),
    ] {
        server.state.lock().unwrap().response_status = Some(http);
        assert_eq!(
            device.backend.cloud_sync_status().await.unwrap_err().code,
            code
        );
        assert_eq!(device.documents(), before);
    }
    server.state.lock().unwrap().response_status = None;
    device
        .backend
        .services()
        .marketplace
        .logout()
        .await
        .unwrap();
    let count = server.state.lock().unwrap().requests.len();
    assert_eq!(
        device.backend.cloud_sync_status().await.unwrap_err().code,
        BackendErrorCode::PermissionDenied
    );
    assert_eq!(server.state.lock().unwrap().requests.len(), count);
}
