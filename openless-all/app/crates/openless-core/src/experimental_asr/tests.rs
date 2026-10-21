use super::*;
use crate::model_store::{
    ModelContentRange, ModelHttpMetadata, ModelStoreConfig, ModelTransport, ModelTransportRequest,
    ModelTransportResponse,
};
use crate::{InMemoryCredentialStore, ProviderSlot};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

struct Spawner;
impl TaskSpawner for Spawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        tokio::spawn(task);
    }
}
struct Transport {
    gate: Arc<tokio::sync::Semaphore>,
    calls: Arc<AtomicUsize>,
    corrupt: bool,
}
impl ModelTransport for Transport {
    fn request(
        &self,
        req: ModelTransportRequest,
    ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
        let gate = self.gate.clone();
        let calls = self.calls.clone();
        let corrupt = self.corrupt;
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let _permit = gate.acquire().await.unwrap();
            let body = if corrupt { b"oops" } else { b"test" };
            let (status, bytes, content_range) = if let Some((start, end)) = req.range {
                (
                    206,
                    body[start as usize..=end as usize].to_vec(),
                    Some(ModelContentRange {
                        start,
                        end,
                        total: 4,
                    }),
                )
            } else {
                (200, body.to_vec(), None)
            };
            Ok(ModelTransportResponse {
                status,
                metadata: ModelHttpMetadata {
                    content_length: Some(bytes.len() as u64),
                    content_range,
                    link: None,
                },
                bytes,
            })
        })
    }
}
#[derive(Default)]
struct Runtime {
    unsupported: AtomicBool,
    fail_prepare: AtomicBool,
    releases: AtomicUsize,
    prepares: AtomicUsize,
    installs: AtomicUsize,
}
impl ExperimentalAsrRuntime for Runtime {
    fn unavailable_reason(&self, _: Model) -> Option<String> {
        self.unsupported
            .load(Ordering::SeqCst)
            .then(|| "unsupported".into())
    }
    fn install(&self, _: Model, _: PathBuf) -> BoxFuture<'static, Result<(), BackendError>> {
        self.installs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
    fn prepare(&self, _: Model, _: PathBuf) -> BoxFuture<'static, Result<(), BackendError>> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let fail = self.fail_prepare.load(Ordering::SeqCst);
        Box::pin(async move {
            if fail {
                Err(error("load failed"))
            } else {
                Ok(())
            }
        })
    }
    fn release(&self, _: Model) -> BoxFuture<'static, Result<(), BackendError>> {
        self.releases.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
type Model = ExperimentalAsrModel;
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openless-experimental-asr-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    _temp: TempDir,
    store: Arc<ModelStore>,
    credentials: Arc<InMemoryCredentialStore>,
    runtime: Arc<Runtime>,
    service: Arc<ExperimentalAsrService>,
    gate: Arc<tokio::sync::Semaphore>,
    calls: Arc<AtomicUsize>,
    path: PathBuf,
}
impl Fixture {
    fn new(permits: usize, corrupt: bool) -> Self {
        let temp = TempDir::new();
        let path = temp.path().join("state.json");
        let gate = Arc::new(tokio::sync::Semaphore::new(permits));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut entries = catalog_entries();
        for entry in &mut entries {
            entry.selector = ModelFileSelector::Pinned(vec![ModelFile {
                path: "model.bin".into(),
                url: "https://example.com/pinned/model.bin".into(),
                size_bytes: 4,
                sha256: Some(hex::encode(Sha256::digest(b"test"))),
            }]);
        }
        let store = Arc::new(
            ModelStore::with_transport(
                ModelStoreConfig::new(temp.path().join("models")).unwrap(),
                Arc::new(Transport {
                    gate: gate.clone(),
                    calls: calls.clone(),
                    corrupt,
                }),
            )
            .with_test_catalog(entries),
        );
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let runtime = Arc::new(Runtime::default());
        let service = ExperimentalAsrService::new(
            store.clone(),
            credentials.clone(),
            runtime.clone(),
            Arc::new(Spawner),
            path.clone(),
        )
        .unwrap();
        Self {
            _temp: temp,
            store,
            credentials,
            runtime,
            service,
            gate,
            calls,
            path,
        }
    }
    async fn phase(&self, model: Model, phase: ExperimentalAsrPhase) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = self.service.status().unwrap();
                let status = &status[model.index()];
                if status.phase == phase {
                    break;
                }
                assert_ne!(
                    status.phase,
                    ExperimentalAsrPhase::Failed,
                    "{:?}",
                    status.error
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("model operation did not settle");
    }
    async fn wait_failed(&self, model: Model) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.service.status().unwrap()[model.index()].phase
                != ExperimentalAsrPhase::Failed
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
#[test]
fn pinned_catalog_is_scoped_and_complete() {
    for model in Model::ALL {
        let files = model.files();
        crate::ModelManifest::new(model.target(), model.provider_type(), files.clone()).unwrap();
        assert!(files
            .iter()
            .all(|f| f.sha256.as_ref().is_some_and(|s| s.len() == 64)));
        assert_ne!(model.target().model_id(), "sense-voice-small-zh");
    }
    assert_eq!(
        Model::EmbeddingGemma
            .files()
            .iter()
            .map(|f| f.size_bytes)
            .sum::<u64>(),
        714_975_724
    );
    assert_eq!(Model::EmbeddingGemma.files().len(), 26);
    let catalog = crate::ModelCatalog::standard();
    assert!(catalog
        .find(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh")
        .is_some());
    for model in Model::ALL {
        let d = crate::provider_rules::provider_descriptor(
            crate::ProviderKind::Asr,
            model.provider_type(),
        )
        .unwrap();
        assert_eq!(
            d.auth_requirement,
            crate::provider_rules::AuthRequirement::None
        );
        assert!(
            !crate::provider_rules::provider_descriptors(crate::ProviderKind::Llm)
                .iter()
                .any(|d| d.provider_type.as_str() == model.provider_type())
        );
    }
}
#[tokio::test]
async fn register_is_idempotent_append_only_and_keeps_existing_routes() {
    let f = Fixture::new(4, false);
    f.credentials
        .mutate_channel(ChannelMutation::Create {
            kind: ChannelKind::Asr,
            provider_type: "volcengine".into(),
            name: "Existing".into(),
        })
        .await
        .unwrap();
    let previous = f
        .credentials
        .active_provider(ProviderSlot::Asr)
        .await
        .unwrap();
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.service.set_enabled(Model::EmbeddingGemma, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
    f.phase(Model::EmbeddingGemma, ExperimentalAsrPhase::Ready)
        .await;
    assert_eq!(
        f.credentials
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap(),
        previous
    );
    let channels = f.credentials.list_channels(ChannelKind::Asr).await.unwrap();
    assert_eq!(channels.len(), 3);
    assert_eq!(channels[0].id, previous);
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    assert_eq!(
        f.credentials
            .list_channels(ChannelKind::Asr)
            .await
            .unwrap()
            .len(),
        3
    );
    assert!(f.service.status().unwrap()[0].needs_guidance);
    f.service.acknowledge(Model::SenseVoice).unwrap();
    let resumed = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    resumed.resume();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !resumed.status().unwrap()[0].needs_guidance,
        "restart must not re-arm acknowledged guidance"
    );
    assert!(
        resumed.status().unwrap()[1].needs_guidance,
        "unacknowledged guidance survives restart"
    );
    assert_eq!(
        f.credentials
            .list_channels(ChannelKind::Asr)
            .await
            .unwrap()
            .len(),
        3
    );
    let legacy = f
        .store
        .models_root_dir()
        .join("sherpa-onnx/sense-voice-small-zh/model.int8.onnx");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, b"existing legacy model").unwrap();
    f.credentials
        .mutate_channel(ChannelMutation::Reorder {
            kind: ChannelKind::Asr,
            ids: vec![
                Model::SenseVoice.channel_id().into(),
                previous.clone(),
                Model::EmbeddingGemma.channel_id().into(),
            ],
        })
        .await
        .unwrap();
    assert_eq!(
        f.credentials
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap(),
        Model::SenseVoice.channel_id()
    );
    f.service.set_enabled(Model::SenseVoice, false).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Disabled)
        .await;
    assert!(f.store.is_installed(&Model::SenseVoice.target()).unwrap());
    f.service.delete_model(Model::SenseVoice).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Disabled)
        .await;
    assert!(!f
        .store
        .model_dir(&Model::SenseVoice.target())
        .unwrap()
        .exists());
    assert_eq!(std::fs::read(&legacy).unwrap(), b"existing legacy model");
    assert_eq!(
        f.runtime.prepares.load(Ordering::SeqCst),
        0,
        "downloads and restored flags never prepare inference"
    );
    assert!(f
        .store
        .is_installed(&Model::EmbeddingGemma.target())
        .unwrap());
    assert_eq!(
        f.credentials
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap(),
        previous
    );
    assert_eq!(
        f.credentials
            .list_channels(ChannelKind::Asr)
            .await
            .unwrap()
            .len(),
        2
    );
}
#[tokio::test]
async fn empty_channel_list_does_not_activate_a_downloaded_model() {
    let f = Fixture::new(1, false);
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
    let channels = f.credentials.list_channels(ChannelKind::Asr).await.unwrap();
    assert_eq!(channels.len(), 1);
    assert!(!channels[0].enabled);
    assert_ne!(
        f.credentials
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap(),
        Model::SenseVoice.channel_id()
    );
}
#[tokio::test]
async fn cancel_waits_for_download_cleanup_and_never_registers_a_stale_channel() {
    let f = Fixture::new(0, false);
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.service.set_enabled(Model::EmbeddingGemma, true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.calls.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.service.set_enabled(Model::SenseVoice, false).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Disabled)
        .await;
    f.gate.add_permits(2);
    f.phase(Model::EmbeddingGemma, ExperimentalAsrPhase::Ready)
        .await;
    assert!(!f
        .store
        .model_dir(&Model::SenseVoice.target())
        .unwrap()
        .exists());
    assert!(f
        .credentials
        .list_channels(ChannelKind::Asr)
        .await
        .unwrap()
        .iter()
        .all(|c| c.id != Model::SenseVoice.channel_id()));
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
}
#[tokio::test]
async fn failed_download_never_creates_ready_channels() {
    for corrupt in [true] {
        let f = Fixture::new(1, corrupt);
        f.runtime.fail_prepare.store(!corrupt, Ordering::SeqCst);
        f.service.set_enabled(Model::SenseVoice, true).unwrap();
        f.wait_failed(Model::SenseVoice).await;
        assert!(f
            .credentials
            .list_channels(ChannelKind::Asr)
            .await
            .unwrap()
            .is_empty());
        assert!(!f.service.status().unwrap()[0].needs_guidance);
        f.service.set_enabled(Model::SenseVoice, false).unwrap();
        f.phase(Model::SenseVoice, ExperimentalAsrPhase::Disabled)
            .await;
    }
}
#[tokio::test]
async fn recording_lease_is_invalid_after_disable_and_reenable() {
    let f = Fixture::new(1, false);
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
    let (_, generation) = f.service.lease(Model::SenseVoice).unwrap();
    f.service.set_enabled(Model::SenseVoice, false).unwrap();
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
    assert!(f
        .service
        .check_lease(Model::SenseVoice, generation)
        .is_err());
    let (_, current) = f.service.lease(Model::SenseVoice).unwrap();
    assert!(f.service.check_lease(Model::SenseVoice, current).is_ok());
}

#[tokio::test]
async fn enabled_saved_model_is_metadata_only_on_resume_and_loader_failures_are_deferred() {
    let f = Fixture::new(1, false);
    f.runtime.fail_prepare.store(true, Ordering::SeqCst);
    f.service.set_enabled(Model::SenseVoice, true).unwrap();
    f.phase(Model::SenseVoice, ExperimentalAsrPhase::Ready)
        .await;
    let downloads = f.calls.load(Ordering::SeqCst);
    let resumed = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    resumed.resume();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        resumed.status().unwrap()[0].phase,
        ExperimentalAsrPhase::Ready
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), downloads);
    assert_eq!(f.runtime.prepares.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn saved_enabled_without_files_never_silently_downloads_on_startup() {
    let f = Fixture::new(1, false);
    std::fs::write(&f.path, br#"[{"enabled":true},{}]"#).unwrap();
    let resumed = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    resumed.resume();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        resumed.status().unwrap()[0].phase,
        ExperimentalAsrPhase::Failed
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.runtime.prepares.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn only_an_explicit_interrupted_download_resumes() {
    let f = Fixture::new(1, false);
    std::fs::write(&f.path, br#"[{"enabled":true,"downloadPending":true},{}]"#).unwrap();
    let resumed = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    resumed.resume();
    tokio::time::timeout(Duration::from_secs(2), async {
        while resumed.status().unwrap()[0].phase != ExperimentalAsrPhase::Ready {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.runtime.prepares.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn all_three_packages_install_independently_without_loading_and_migrate_two_slot_settings() {
    let f = Fixture::new(4, false);
    for model in Model::ALL {
        f.service.set_enabled(model, true).unwrap();
    }
    for model in Model::ALL {
        f.phase(model, ExperimentalAsrPhase::Ready).await;
    }
    assert_eq!(f.runtime.installs.load(Ordering::SeqCst), 3);
    assert_eq!(f.runtime.prepares.load(Ordering::SeqCst), 0);
    let downloads = f.calls.load(Ordering::SeqCst);
    let resumed = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    resumed.resume();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(f.calls.load(Ordering::SeqCst), downloads);
    assert_eq!(
        f.runtime.installs.load(Ordering::SeqCst),
        3,
        "resume cannot install or execute a module again"
    );
    assert_eq!(f.runtime.prepares.load(Ordering::SeqCst), 0);
    // The legacy two-slot file must not shift SenseVoice/Gemma into TextStream.
    std::fs::write(&f.path, br#"[{"enabled":true},{"enabled":false}]"#).unwrap();
    let upgraded = ExperimentalAsrService::new(
        f.store.clone(),
        f.credentials.clone(),
        f.runtime.clone(),
        Arc::new(Spawner),
        f.path.clone(),
    )
    .unwrap();
    let statuses = upgraded.status().unwrap();
    assert!(
        statuses
            .iter()
            .find(|s| s.model == Model::SenseVoice)
            .unwrap()
            .enabled
    );
    assert!(
        !statuses
            .iter()
            .find(|s| s.model == Model::TextStream)
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn optional_runtime_files_are_not_download_progress() {
    let f = Fixture::new(4, false);
    let directory = f.store.model_dir(&Model::TextStream.target()).unwrap();
    std::fs::create_dir_all(directory.join(".runtime/cache")).unwrap();
    let unrelated = std::fs::File::create(directory.join(".runtime/cache/wheels")).unwrap();
    unrelated.set_len(5 * 1024 * 1024 * 1024).unwrap();
    let status = f.service.status().unwrap();
    assert_eq!(status[Model::TextStream.index()].downloaded_bytes, 0);
    f.service.set_enabled(Model::TextStream, true).unwrap();
    f.phase(Model::TextStream, ExperimentalAsrPhase::Ready)
        .await;
    std::fs::create_dir_all(directory.join(".runtime/cache")).unwrap();
    std::fs::write(directory.join(".runtime/cache/extra"), b"not model weights").unwrap();
    let status = f.service.status().unwrap();
    assert_eq!(status[Model::TextStream.index()].downloaded_bytes, 4);
    assert_eq!(status[Model::TextStream.index()].total_bytes, 4);
}

#[tokio::test]
async fn unsupported_models_are_not_downloaded() {
    let f = Fixture::new(1, false);
    f.runtime.unsupported.store(true, Ordering::SeqCst);
    assert!(f.service.set_enabled(Model::EmbeddingGemma, true).is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(!f.service.status().unwrap()[1].enabled);
}
