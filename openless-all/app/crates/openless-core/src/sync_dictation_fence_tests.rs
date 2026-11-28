// Included in api::tests: use the public stop path and its actual restore store.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn submitted_dictation_restore_fence_covers_final_history() {
    use crate::cloud_sync_e2ee_documents::{
        parse_sync_documents, DocumentError, RestoreContext, SyncScope,
    };
    use crate::cloud_sync_e2ee_protocol::types::Revision;
    use std::sync::{atomic::AtomicBool, Condvar};

    struct SubmittedEngine {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl DictationEngine for SubmittedEngine {
        fn start(
            &self,
            id: SessionId,
            context: Arc<DictationContext>,
            progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            FakeEngine.start(id, context, progress)
        }
        fn stop_recording(&self, _: SessionId) -> BoxFuture<'static, Result<bool, BackendError>> {
            boxed(async { Ok(true) })
        }
        fn finish(
            &self,
            id: SessionId,
            progress: Arc<dyn EngineProgressSink>,
        ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
            let entered = self.entered.clone();
            let release = self.release.clone();
            boxed(async move {
                entered.notify_one();
                release.notified().await;
                FakeEngine.finish(id, progress).await
            })
        }
        fn cancel(&self, id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
            FakeEngine.cancel(id)
        }
    }

    struct TerminalClock {
        block_next: AtomicBool,
        entered: tokio::sync::Notify,
        release: (Mutex<bool>, Condvar),
    }
    impl Clock for TerminalClock {
        fn now_utc(&self) -> chrono::DateTime<chrono::Utc> {
            if self.block_next.swap(false, Ordering::AcqRel) {
                self.entered.notify_one();
                let (lock, wake) = &self.release;
                let ready = lock.lock().unwrap();
                let (_ready, timeout) = wake
                    .wait_timeout_while(ready, std::time::Duration::from_secs(5), |ready| !*ready)
                    .unwrap();
                assert!(
                    !timeout.timed_out(),
                    "terminal history test barrier timed out"
                );
            }
            chrono::Utc::now()
        }
        fn today_local(&self) -> chrono::NaiveDate {
            chrono::Utc::now().date_naive()
        }
    }

    let data = TestDataDir::new("submitted-restore-fence");
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let clock = Arc::new(TerminalClock {
        block_next: AtomicBool::new(false),
        entered: tokio::sync::Notify::new(),
        release: (Mutex::new(false), Condvar::new()),
    });
    let mut backend = backend_with_dictation_engine(
        data.path().into(),
        Arc::new(SubmittedEngine {
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    backend.clock = clock.clone();
    backend.history.attach_clock(clock.clone());
    let backend = Arc::new(backend);
    backend.start().await.unwrap();
    let id = backend
        .start_dictation_with_options(DictationStartOptions {
            output_target: DictationOutputTarget::QuickNote,
            ..Default::default()
        })
        .await
        .unwrap();
    let stopping = tokio::spawn({
        let backend = backend.clone();
        async move { backend.stop_dictation_session(id).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
    assert_eq!(backend.snapshot().processing_dictations.len(), 1);

    let store = backend.local_migration_store.as_ref().unwrap();
    let device = store.device();
    let documents = parse_sync_documents(
        &serde_json::to_vec(&serde_json::json!({
            "schemaVersion":1, "exportedAt":"2026-11-23T00:00:00Z", "sourceDevice":device,
            "documents":[], "tombstones":[]
        }))
        .unwrap(),
        Revision::new(0),
    )
    .unwrap();
    let scope = SyncScope {
        service_origin: "https://sync.example.test".into(),
        owner_github_id: "42".into(),
        vault_id: uuid::Uuid::new_v4().to_string(),
        key_id: uuid::Uuid::new_v4().to_string(),
        device_id: store.device().id.clone(),
    };
    let context = || RestoreContext {
        scope: scope.clone(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        observed_revision: Revision::new(0),
        local_generation: store.generation().unwrap(),
        target_device: store.device(),
    };
    assert_eq!(
        store
            .restore_scope(documents.clone(), context())
            .await
            .unwrap_err(),
        DocumentError::RuntimeBusy
    );

    // Remove only this test's draft so final history creation calls the injected
    // clock before taking a repository mutation permit. At that point both UI
    // slots are empty, but the terminal history write has not happened yet.
    backend.delete_history(&id.to_string()).unwrap();
    clock.block_next.store(true, Ordering::Release);
    release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(3), clock.entered.notified())
        .await
        .unwrap();
    let snapshot = backend.snapshot();
    assert_eq!(snapshot.dictation.phase, DictationPhase::Idle);
    assert!(snapshot.processing_dictations.is_empty());
    let terminal_voice_idle = backend.voice_sessions.runtime_restore_idle();
    let terminal_restore = store.restore_scope(documents, context()).await;
    *clock.release.0.lock().unwrap() = true;
    clock.release.1.notify_all();
    stopping.await.unwrap().unwrap();
    assert!(!terminal_voice_idle, "terminal history still owns the voice lease");
    assert_eq!(terminal_restore.unwrap_err(), DocumentError::RuntimeBusy);
    assert!(backend.voice_sessions.runtime_restore_idle());
    assert!(backend
        .list_history()
        .unwrap()
        .iter()
        .any(|row| row.id == id.to_string() && row.final_text == "polished"));
}
