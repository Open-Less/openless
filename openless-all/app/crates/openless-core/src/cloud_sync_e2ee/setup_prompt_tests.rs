use super::*;
use std::sync::atomic::AtomicUsize;

struct ReadyCredentials {
    status: CredentialsStatus,
    deny: bool,
    reads: Arc<AtomicUsize>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
}
impl CredentialStore for ReadyCredentials {
    fn status(
        &self,
        _: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let status = self.status.clone();
        let deny = self.deny;
        let reads = self.reads.clone();
        let barrier = self.barrier.clone();
        Box::pin(async move {
            reads.fetch_add(1, Ordering::AcqRel);
            if let Some(barrier) = barrier {
                barrier.wait().await;
                barrier.wait().await;
            }
            if deny {
                Err(error("secure_storage_denied"))
            } else {
                Ok(status)
            }
        })
    }
    fn read(
        &self,
        _: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        Box::pin(async { panic!("claim must use Host readiness, not require a local-ASR API key") })
    }
    fn write(
        &self,
        _: CredentialKey,
        _: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { panic!("claim must not mutate provider credentials") })
    }
    fn remove(&self, _: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { panic!("claim must not mutate provider credentials") })
    }
}
fn readiness(asr: bool, llm: bool, omni: bool) -> Arc<ReadyCredentials> {
    Arc::new(ReadyCredentials {
        status: CredentialsStatus {
            asr_configured: asr,
            llm_configured: llm,
            omni_configured: omni,
            ..Default::default()
        },
        deny: false,
        reads: Arc::new(AtomicUsize::new(0)),
        barrier: None,
    })
}
fn eligible(
    prefs: UserPreferences,
) -> impl Fn() -> SyncResult<Option<(UserPreferences, (Revision, u64))>> + Send + Sync {
    move || Ok(Some((prefs.clone(), (Revision::new(1), 0))))
}
fn reopened_service(fixture: &Fixture, server: &Server) -> EncryptedSyncService {
    let repos = crate::BackendRepositories::open(&fixture.root).unwrap();
    let config = crate::MarketplaceConfig::new(&server.origin).unwrap();
    let events =
        crate::events::BackendEventPublisher::new(Arc::new(crate::events::EventBus::new(32)));
    let marketplace = Arc::new(
        crate::marketplace::MarketplaceService::new(
            config,
            fixture.vault.clone(),
            repos.preferences,
            repos.style_packs,
            events.clone(),
            Arc::new(AtomicU64::new(0)),
        )
        .unwrap(),
    );
    let local = LocalStorage::new(
        fixture.root.join("protected"),
        server.origin.clone(),
        fixture.data.device().id.clone(),
        fixture.vault.clone(),
    );
    EncryptedSyncService::new(
        SyncServiceConfig {
            origin: server.origin.clone(),
            github_client_id: CLIENT.into(),
        },
        marketplace,
        local,
        fixture.data.clone(),
        events,
    )
}

#[tokio::test]
async fn setup_prompt_requires_the_selected_pipeline_and_accepts_ready_local_asr_without_a_key() {
    let server = Server::start().await;
    for (multi, enabled, asr, llm, omni, expected) in [
        (false, false, true, false, true, false),
        (false, false, false, true, true, false),
        (false, false, true, true, false, true),
        (true, true, false, false, true, true),
        (true, true, true, true, false, false),
        (true, false, true, true, false, true),
    ] {
        let fixture = Fixture::new(&server).await;
        let prefs = UserPreferences {
            pipeline_mode: if multi {
                crate::shared_types::PipelineMode::Multimodal
            } else {
                crate::shared_types::PipelineMode::Traditional
            },
            multimodal_pipeline_enabled: enabled,
            active_asr_provider: "local-qwen3".into(),
            ..Default::default()
        };
        assert_eq!(
            fixture
                .service
                .claim_setup_prompt(readiness(asr, llm, omni), eligible(prefs))
                .await
                .unwrap(),
            expected
        );
    }
    assert_eq!(server.state.lock().unwrap().requests, 0);
}

#[tokio::test]
async fn setup_prompt_claim_is_once_per_installation_and_survives_restart_without_http() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    let ready = readiness(true, true, false);
    assert!(fixture
        .service
        .claim_setup_prompt(ready.clone(), eligible(UserPreferences::default()))
        .await
        .unwrap());
    assert!(!fixture
        .service
        .claim_setup_prompt(ready.clone(), eligible(UserPreferences::default()))
        .await
        .unwrap());
    let reopened = reopened_service(&fixture, &server);
    assert!(!reopened
        .claim_setup_prompt(ready, eligible(UserPreferences::default()))
        .await
        .unwrap());
    assert_eq!(server.state.lock().unwrap().requests, 0);
    assert!(server.state.lock().unwrap().puts.is_empty());
}

#[tokio::test]
async fn setup_prompt_read_error_busy_or_stale_capture_never_consumes_the_flag() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    let denied = Arc::new(ReadyCredentials {
        deny: true,
        status: CredentialsStatus::default(),
        reads: Arc::new(AtomicUsize::new(0)),
        barrier: None,
    });
    assert!(fixture
        .service
        .claim_setup_prompt(denied, eligible(UserPreferences::default()))
        .await
        .is_err());
    assert!(!fixture
        .service
        .claim_setup_prompt(readiness(true, true, false), || Ok(None))
        .await
        .unwrap());
    let calls = AtomicUsize::new(0);
    assert!(!fixture
        .service
        .claim_setup_prompt(readiness(true, true, false), || Ok(Some((
            UserPreferences::default(),
            (
                Revision::new(1),
                calls.fetch_add(1, Ordering::AcqRel) as u64
            )
        ))))
        .await
        .unwrap());
    assert!(fixture.vault.secrets.lock().unwrap().is_empty());
    assert!(fixture
        .service
        .claim_setup_prompt(
            readiness(true, true, false),
            eligible(UserPreferences::default())
        )
        .await
        .unwrap());
    assert_eq!(server.state.lock().unwrap().requests, 0);
}

#[tokio::test]
async fn setup_prompt_durable_write_failure_and_enabled_state_do_not_claim() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    fixture.vault.deny.store(true, Ordering::Release);
    assert!(fixture
        .service
        .claim_setup_prompt(
            readiness(true, true, false),
            eligible(UserPreferences::default())
        )
        .await
        .is_err());
    fixture.vault.deny.store(false, Ordering::Release);
    assert!(fixture
        .service
        .claim_setup_prompt(
            readiness(true, true, false),
            eligible(UserPreferences::default())
        )
        .await
        .unwrap());
    let other = Fixture::new(&server).await;
    let local = LocalStorage::new(
        other.root.join("protected"),
        server.origin.clone(),
        other.data.device().id.clone(),
        other.vault.clone(),
    );
    let settings = local::ClientSettings {
        enabled: true,
        ..Default::default()
    };
    local.write("device", "client", settings).await.unwrap();
    let ready = readiness(true, true, false);
    assert!(!other
        .service
        .claim_setup_prompt(ready.clone(), eligible(UserPreferences::default()))
        .await
        .unwrap());
    assert_eq!(ready.reads.load(Ordering::Acquire), 0);
    assert_eq!(server.state.lock().unwrap().requests, 0);
}

#[tokio::test]
async fn setup_prompt_service_busy_is_false_and_concurrent_claims_do_not_duplicate() {
    let server = Server::start().await;
    let fixture = Fixture::new(&server).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let ready = Arc::new(ReadyCredentials {
        status: CredentialsStatus {
            asr_configured: true,
            llm_configured: true,
            ..Default::default()
        },
        deny: false,
        reads: Arc::new(AtomicUsize::new(0)),
        barrier: Some(barrier.clone()),
    });
    let service = fixture.service.clone();
    let first = tokio::spawn(async move {
        service
            .claim_setup_prompt(ready, eligible(UserPreferences::default()))
            .await
    });
    barrier.wait().await;
    assert!(!fixture
        .service
        .claim_setup_prompt(
            readiness(true, true, false),
            eligible(UserPreferences::default())
        )
        .await
        .unwrap());
    barrier.wait().await;
    assert!(first.await.unwrap().unwrap());
    assert_eq!(server.state.lock().unwrap().requests, 0);
}
