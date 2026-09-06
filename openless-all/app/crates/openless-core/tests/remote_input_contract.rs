use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, BackendEventKind,
    OpenLessBackend, RemoteAuthResult, RemoteFrameCodec, RemoteInputApi, RemoteInputConfig,
    RemoteInputRuntimeAdapter, RemoteInputServerBinding, RemoteInputServerConfig,
    RemoteInputService, SecretValue, SessionId, REMOTE_INPUT_MAX_PCM_FRAME_BYTES,
};

#[derive(Default)]
struct FixtureRemoteRuntime {
    persisted_pin: Mutex<Option<String>>,
    persist_count: AtomicUsize,
    reject_persist: AtomicBool,
    start_count: AtomicUsize,
    stop_count: AtomicUsize,
    fail_start: AtomicBool,
    audio_start_count: AtomicUsize,
    audio_stop_count: AtomicUsize,
    audio_cancel_count: AtomicUsize,
    frames: Mutex<Vec<(SessionId, Vec<u8>)>>,
    insert_preferences: Mutex<Vec<bool>>,
    stop_started: Option<Arc<tokio::sync::Notify>>,
    release_stop: Option<Arc<tokio::sync::Notify>>,
    persist_started: Option<Arc<tokio::sync::Notify>>,
    release_persist: Option<Arc<tokio::sync::Notify>>,
}

impl RemoteInputRuntimeAdapter for FixtureRemoteRuntime {
    fn load_pairing_pin(&self) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let pin = self
            .persisted_pin
            .lock()
            .unwrap()
            .clone()
            .map(SecretValue::new);
        Box::pin(async move { Ok(pin) })
    }

    fn persist_pairing_pin(
        &self,
        pin: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        if self.reject_persist.load(Ordering::Acquire) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "secret persistence details",
                ))
            });
        }
        self.persist_count.fetch_add(1, Ordering::AcqRel);
        *self.persisted_pin.lock().unwrap() = Some(pin.into_exposed());
        let started = self.persist_started.clone();
        let release = self.release_persist.clone();
        Box::pin(async move {
            if let Some(started) = started {
                started.notify_one();
            }
            if let Some(release) = release {
                release.notified().await;
            }
            Ok(())
        })
    }

    fn start_server(
        &self,
        config: RemoteInputServerConfig,
    ) -> BoxFuture<'static, Result<RemoteInputServerBinding, BackendError>> {
        self.start_count.fetch_add(1, Ordering::AcqRel);
        let fail = self.fail_start.load(Ordering::Acquire);
        Box::pin(async move {
            if fail {
                return Err(BackendError::new(BackendErrorCode::Platform, "port-in-use"));
            }
            Ok(RemoteInputServerBinding {
                port: config.port,
                urls: vec![format!("https://192.168.1.2:{}", config.port)],
                urls_stale: false,
            })
        })
    }

    fn stop_server(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.stop_count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(()) })
    }

    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        Box::pin(async { Ok(vec!["192.168.1.2".to_string()]) })
    }

    fn start_audio_session(
        &self,
        insert_text: bool,
    ) -> BoxFuture<'static, Result<SessionId, BackendError>> {
        self.audio_start_count.fetch_add(1, Ordering::AcqRel);
        self.insert_preferences.lock().unwrap().push(insert_text);
        Box::pin(async { Ok(SessionId::new()) })
    }

    fn feed_audio(
        &self,
        session_id: SessionId,
        pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.frames.lock().unwrap().push((session_id, pcm_s16le));
        Box::pin(async { Ok(()) })
    }

    fn stop_audio_session(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.audio_stop_count.fetch_add(1, Ordering::AcqRel);
        let started = self.stop_started.clone();
        let release = self.release_stop.clone();
        Box::pin(async move {
            if let Some(started) = started {
                started.notify_one();
            }
            if let Some(release) = release {
                release.notified().await;
            }
            Ok(())
        })
    }

    fn cancel_audio_session(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.audio_cancel_count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(()) })
    }
}

fn backend(runtime: Arc<FixtureRemoteRuntime>) -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-remote-input-contract-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.services.remote_input =
        Arc::new(RemoteInputService::new(runtime, 8443, "zh-CN").expect("fixture config is valid"));
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

async fn authenticate(remote: &dyn RemoteInputApi, connection_id: SessionId) {
    let pin = remote.read_pairing_pin().await.unwrap();
    assert_eq!(
        remote
            .authenticate(connection_id, "192.168.1.8".to_string(), pin)
            .await
            .unwrap(),
        RemoteAuthResult::Ok
    );
}

#[tokio::test]
async fn authentication_queued_during_pin_rotation_rejects_the_previous_pin() {
    let persist_started = Arc::new(tokio::sync::Notify::new());
    let release_persist = Arc::new(tokio::sync::Notify::new());
    let runtime = Arc::new(FixtureRemoteRuntime {
        persisted_pin: Mutex::new(Some("123456".into())),
        persist_started: Some(Arc::clone(&persist_started)),
        release_persist: Some(Arc::clone(&release_persist)),
        ..Default::default()
    });
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    let previous_pin = remote.read_pairing_pin().await.unwrap();

    // Hold the native persistence boundary while rotation owns the lifecycle
    // gate. Polling explicitly queues authentication behind that rotation,
    // without relying on scheduler timing or a real socket/keychain.
    let rotation = loop {
        let mut rotation = remote.regenerate_pairing_pin();
        assert!(futures_util::poll!(rotation.as_mut()).is_pending());
        persist_started.notified().await;
        // The generator can legitimately repeat one of its million values.
        // Select a changed PIN so this concurrency test never fails by chance.
        if runtime.persisted_pin.lock().unwrap().as_deref() != Some(previous_pin.expose_secret()) {
            break rotation;
        }
        release_persist.notify_one();
        rotation.await.unwrap();
    };
    let mut authentication =
        remote.authenticate(SessionId::new(), "192.168.1.8".into(), previous_pin);
    assert!(futures_util::poll!(authentication.as_mut()).is_pending());
    release_persist.notify_one();
    rotation.await.unwrap();

    assert_eq!(authentication.await.unwrap(), RemoteAuthResult::BadPin);
    authenticate(remote.as_ref(), SessionId::new()).await;
    assert_eq!(remote.status().unwrap().connection_count, 1);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn contract_2_audio_frames_use_ol20_uuid_big_endian_sequence_and_pcm() {
    let session_id = SessionId::from_uuid(
        uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap(),
    );
    let expected = vec![
        0x4f, 0x4c, 0x32, 0x30, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa,
        0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x34, 0x12,
        0xcc, 0xff,
    ];

    assert_eq!(
        RemoteFrameCodec::encode(session_id, 0x0102_0304_0506_0708, &[0x34, 0x12, 0xcc, 0xff])
            .unwrap(),
        expected
    );
    assert_eq!(
        RemoteFrameCodec::decode(&expected).unwrap(),
        (
            session_id,
            0x0102_0304_0506_0708,
            vec![0x34, 0x12, 0xcc, 0xff]
        )
    );
}

#[test]
fn contract_2_audio_frames_reject_invalid_headers_and_pcm() {
    let session_id = SessionId::new();
    assert_eq!(
        RemoteFrameCodec::encode(session_id, 0, &[])
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        RemoteFrameCodec::encode(session_id, 0, &[0])
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        RemoteFrameCodec::encode(
            session_id,
            0,
            &vec![0; REMOTE_INPUT_MAX_PCM_FRAME_BYTES + 2]
        )
        .unwrap_err()
        .code,
        BackendErrorCode::InvalidArgument
    );

    let mut frame = RemoteFrameCodec::encode(session_id, 0, &[0, 0]).unwrap();
    frame[0] = b'X';
    assert_eq!(
        RemoteFrameCodec::decode(&frame).unwrap_err().code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        RemoteFrameCodec::decode(b"OL20").unwrap_err().code,
        BackendErrorCode::InvalidArgument
    );
}

#[tokio::test]
async fn pairing_pin_is_explicit_persisted_and_absent_from_public_surfaces() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;

    let pin = remote.read_pairing_pin().await.unwrap();
    assert_eq!(pin.expose_secret().len(), 6);
    assert!(pin
        .expose_secret()
        .bytes()
        .all(|byte| byte.is_ascii_digit()));
    assert_eq!(runtime.persist_count.load(Ordering::Acquire), 1);
    assert_eq!(
        remote.read_pairing_pin().await.unwrap().expose_secret(),
        pin.expose_secret()
    );
    assert_eq!(runtime.persist_count.load(Ordering::Acquire), 1);

    let json = format!(
        "{} {}",
        serde_json::to_string(&remote.status().unwrap()).unwrap(),
        serde_json::to_string(&backend.replay_events_after(0)).unwrap()
    )
    .to_ascii_lowercase();
    assert!(!json.contains(pin.expose_secret()));
    assert!(!json.contains("\"pin\""));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn enable_disable_and_port_change_are_idempotent_and_evented() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;

    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    assert_eq!(runtime.start_count.load(Ordering::Acquire), 1);
    assert!(remote.status().unwrap().running);

    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 9443,
        })
        .await
        .unwrap();
    assert_eq!(runtime.start_count.load(Ordering::Acquire), 2);
    assert_eq!(runtime.stop_count.load(Ordering::Acquire), 1);
    assert_eq!(remote.status().unwrap().port, 9443);

    remote
        .configure(RemoteInputConfig {
            enabled: false,
            port: 9443,
        })
        .await
        .unwrap();
    remote
        .configure(RemoteInputConfig {
            enabled: false,
            port: 9443,
        })
        .await
        .unwrap();
    assert_eq!(runtime.stop_count.load(Ordering::Acquire), 2);
    assert!(!remote.status().unwrap().running);
    assert!(backend
        .replay_events_after(0)
        .events
        .iter()
        .any(|event| matches!(event.kind, BackendEventKind::RemoteInputStatusChanged(_))));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn port_conflict_is_classified_without_leaking_runtime_details() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    runtime.fail_start.store(true, Ordering::Release);
    let (backend, data_dir) = backend(runtime);
    let error = backend
        .services()
        .remote_input
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(error.message, "port-in-use");
    assert!(backend
        .replay_events_after(0)
        .events
        .iter()
        .any(|event| matches!(
            event.kind,
            BackendEventKind::RemoteInputFailed(ref failure)
                if failure.reason == "port-in-use" && failure.port == 8443
        )));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn stream_association_validates_frames_and_rejects_duplicates_and_late_pcm() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    let connection_id = SessionId::new();
    authenticate(remote.as_ref(), connection_id).await;
    let session_id = remote.start_stream(connection_id).await.unwrap();
    assert_eq!(
        remote.start_stream(connection_id).await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    assert_eq!(
        remote
            .feed_pcm(connection_id, session_id, 0, vec![0])
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        remote
            .feed_pcm(
                connection_id,
                session_id,
                0,
                vec![0; REMOTE_INPUT_MAX_PCM_FRAME_BYTES + 2],
            )
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    remote
        .feed_pcm(connection_id, session_id, 0, vec![0, 1, 2, 3])
        .await
        .unwrap();
    assert_eq!(
        remote
            .feed_pcm(connection_id, session_id, 0, vec![0, 1])
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        remote
            .feed_pcm(connection_id, session_id, 2, vec![0, 1])
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    remote
        .feed_pcm(connection_id, session_id, 1, vec![0, 1])
        .await
        .unwrap();
    remote.stop_stream(connection_id, session_id).await.unwrap();
    assert_eq!(runtime.audio_stop_count.load(Ordering::Acquire), 1);
    assert_eq!(runtime.frames.lock().unwrap().len(), 2);
    assert_eq!(
        remote
            .feed_pcm(connection_id, session_id, 2, vec![0, 0])
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::Cancelled
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn slow_finalization_remains_cancellable_and_cannot_clear_a_new_stream() {
    for disconnect in [false, true] {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let runtime = Arc::new(FixtureRemoteRuntime {
            stop_started: Some(Arc::clone(&started)),
            release_stop: Some(Arc::clone(&release)),
            ..Default::default()
        });
        let (backend, data_dir) = backend(Arc::clone(&runtime));
        let remote = Arc::clone(&backend.services().remote_input);
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        let connection_id = SessionId::new();
        authenticate(remote.as_ref(), connection_id).await;
        let old_session = remote.start_stream(connection_id).await.unwrap();
        let stopping = {
            let remote = Arc::clone(&remote);
            tokio::spawn(async move { remote.stop_stream(connection_id, old_session).await })
        };
        started.notified().await;

        // A slow ASR/LLM finalization still belongs to this connection. The
        // phone must be able to revoke it without waiting for the provider.
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            if disconnect {
                remote.disconnect(connection_id).await
            } else {
                remote.cancel_stream(connection_id, old_session).await
            }
        })
        .await
        .expect("cancel/disconnect must not wait for finalization")
        .unwrap();
        assert_eq!(runtime.audio_cancel_count.load(Ordering::Acquire), 1);
        if disconnect {
            authenticate(remote.as_ref(), connection_id).await;
        }
        let new_session = remote.start_stream(connection_id).await.unwrap();
        release.notify_one();
        assert_eq!(
            stopping.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert_eq!(
            remote.status().unwrap().active_session_id,
            Some(new_session)
        );
        remote
            .cancel_stream(connection_id, new_session)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn authentication_lockout_and_insert_preference_are_core_owned() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();

    for _ in 0..5 {
        assert_eq!(
            remote
                .authenticate(
                    SessionId::new(),
                    "192.168.1.9".to_string(),
                    SecretValue::new("invalid"),
                )
                .await
                .unwrap(),
            RemoteAuthResult::BadPin
        );
    }
    let valid_pin = remote.read_pairing_pin().await.unwrap();
    assert_eq!(
        remote
            .authenticate(SessionId::new(), "192.168.1.9".to_string(), valid_pin,)
            .await
            .unwrap(),
        RemoteAuthResult::Locked
    );

    let connection_id = SessionId::new();
    authenticate(remote.as_ref(), connection_id).await;
    remote.set_insert(connection_id, false).await.unwrap();
    remote.start_stream(connection_id).await.unwrap();
    assert_eq!(*runtime.insert_preferences.lock().unwrap(), vec![false]);
    assert_eq!(
        remote
            .set_insert(connection_id, true)
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::Busy
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn disconnect_and_pin_rotation_cancel_active_streams_before_transport_restart() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    let first_connection = SessionId::new();
    authenticate(remote.as_ref(), first_connection).await;
    remote.start_stream(first_connection).await.unwrap();
    remote.disconnect(first_connection).await.unwrap();
    remote.disconnect(first_connection).await.unwrap();
    assert_eq!(runtime.audio_cancel_count.load(Ordering::Acquire), 1);

    let second_connection = SessionId::new();
    authenticate(remote.as_ref(), second_connection).await;
    remote.start_stream(second_connection).await.unwrap();
    remote.regenerate_pairing_pin().await.unwrap();
    assert_eq!(runtime.audio_cancel_count.load(Ordering::Acquire), 2);
    assert_eq!(runtime.start_count.load(Ordering::Acquire), 2);
    assert_eq!(runtime.stop_count.load(Ordering::Acquire), 1);
    assert_eq!(remote.status().unwrap().connection_count, 0);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn failed_pin_persistence_keeps_the_committed_pin_and_server_state() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    let old_pin = remote.read_pairing_pin().await.unwrap().into_exposed();
    runtime.reject_persist.store(true, Ordering::Release);
    let error = remote.regenerate_pairing_pin().await.unwrap_err();
    assert_eq!(error.message, "remote input operation failed");
    assert_eq!(
        remote.read_pairing_pin().await.unwrap().expose_secret(),
        old_pin
    );
    assert!(remote.status().unwrap().running);
    assert_eq!(runtime.start_count.load(Ordering::Acquire), 1);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn locale_and_connection_status_are_core_owned() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(runtime);
    let remote = &backend.services().remote_input;
    remote.set_locale("en".to_string()).await.unwrap();
    assert_eq!(remote.status().unwrap().locale, "en");
    assert_eq!(
        remote.set_locale("fr".to_string()).await.unwrap_err().code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(
        remote
            .configure(RemoteInputConfig {
                enabled: true,
                port: 0,
            })
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidArgument
    );
    assert_eq!(remote.list_local_ips().await.unwrap(), vec!["192.168.1.2"]);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn backend_shutdown_stops_transport_and_cancels_active_remote_audio() {
    let runtime = Arc::new(FixtureRemoteRuntime::default());
    let (backend, data_dir) = backend(Arc::clone(&runtime));
    backend.start().await.unwrap();
    let remote = &backend.services().remote_input;
    remote
        .configure(RemoteInputConfig {
            enabled: true,
            port: 8443,
        })
        .await
        .unwrap();
    let connection_id = SessionId::new();
    authenticate(remote.as_ref(), connection_id).await;
    remote.start_stream(connection_id).await.unwrap();

    backend.shutdown().await.unwrap();

    let status = remote.status().unwrap();
    assert!(!status.enabled);
    assert!(!status.running);
    assert_eq!(runtime.audio_cancel_count.load(Ordering::Acquire), 1);
    assert_eq!(runtime.stop_count.load(Ordering::Acquire), 1);
    let _ = std::fs::remove_dir_all(data_dir);
}
