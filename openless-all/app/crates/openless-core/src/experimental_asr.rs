//! Opt-in model packages. Separate IDs/directories/channels deliberately protect
//! the existing local-ASR catalog and user-created channels from switch-off cleanup.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Mutex as AsyncMutex};

use crate::credentials::{ChannelKind, ChannelMutation, CredentialStore};
use crate::errors::{BackendError, BackendErrorCode};
use crate::model_store::{ModelCatalogEntry, ModelFile, ModelFileSelector, ModelStore};
use crate::{LocalAsrMirror, LocalAsrRuntime, LocalAsrTarget, TaskSpawner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExperimentalAsrModel {
    SenseVoice,
    EmbeddingGemma,
    TextStream,
}
impl ExperimentalAsrModel {
    pub const ALL: [Self; 3] = [Self::SenseVoice, Self::EmbeddingGemma, Self::TextStream];
    fn index(self) -> usize {
        match self {
            Self::SenseVoice => 0,
            Self::EmbeddingGemma => 1,
            Self::TextStream => 2,
        }
    }
    pub fn model_id(self) -> &'static str {
        match self {
            Self::SenseVoice => "experimental-sensevoice-small",
            Self::EmbeddingGemma => "experimental-embedding-gemma-en",
            Self::TextStream => "experimental-textstream-qwen3",
        }
    }
    pub fn provider_type(self) -> &'static str {
        match self {
            Self::SenseVoice => "experimental-sensevoice",
            Self::EmbeddingGemma => "experimental-embedding-gemma",
            Self::TextStream => "experimental-textstream",
        }
    }
    pub fn channel_id(self) -> &'static str {
        self.provider_type()
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SenseVoice => "SenseVoice Small (Local)",
            Self::EmbeddingGemma => "Embedding Gemma (Local · English)",
            Self::TextStream => "TextStream (Local · Streaming)",
        }
    }
    pub fn from_model_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.model_id() == id)
    }
    pub fn from_provider(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.provider_type() == id)
    }
    pub fn target(self) -> LocalAsrTarget {
        LocalAsrTarget::parse(LocalAsrRuntime::Generic, self.model_id())
            .expect("built-in experimental model")
    }
    pub fn files(self) -> Vec<ModelFile> {
        match self {
            Self::TextStream => {
                serde_json::from_str(include_str!("experimental-textstream-manifest.json"))
                    .expect("bundled TextStream artifact manifest")
            }
            Self::EmbeddingGemma => {
                serde_json::from_str(include_str!("experimental-gemma-manifest.json"))
                    .expect("bundled Gemma file manifest")
            }
            Self::SenseVoice => {
                let base = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07";
                [
                    (
                        "model.int8.onnx",
                        239_233_841,
                        "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
                    ),
                    (
                        "tokens.txt",
                        315_894,
                        "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
                    ),
                ]
                .into_iter()
                .map(|(path, size_bytes, hash)| ModelFile {
                    path: path.into(),
                    url: format!("{base}/{path}"),
                    size_bytes,
                    sha256: Some(hash.into()),
                })
                .collect()
            }
        }
    }
}

pub(crate) fn catalog_entries() -> Vec<ModelCatalogEntry> {
    ExperimentalAsrModel::ALL
        .into_iter()
        .map(|model| ModelCatalogEntry {
            target: model.target(),
            repository: model.provider_type().into(),
            modelscope_repository: None,
            display_name: model.name().into(),
            family: "experimental_asr".into(),
            mode: if model == ExperimentalAsrModel::TextStream {
                "streaming"
            } else {
                "offline"
            }
            .into(),
            languages: match model {
                ExperimentalAsrModel::SenseVoice => vec!["zh", "en", "yue", "ja", "ko"],
                ExperimentalAsrModel::EmbeddingGemma => vec!["en"],
                ExperimentalAsrModel::TextStream => vec!["multi"],
            }
            .into_iter()
            .map(String::from)
            .collect(),
            selector: ModelFileSelector::Pinned(model.files()),
        })
        .collect()
}

/// Host implements real availability/loading. A successful download alone must
/// never create a seemingly usable channel for a missing runtime.
pub trait ExperimentalAsrRuntime: Send + Sync {
    /// Stable reason code when unsupported, None when the runtime can be tried.
    fn unavailable_reason(&self, model: ExperimentalAsrModel) -> Option<String>;
    /// Optional module setup downloads code/dependencies, but must not load models.
    fn install(
        &self,
        _model: ExperimentalAsrModel,
        _directory: PathBuf,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
    /// Filesystem-only check, safe on startup/settings/status paths.
    fn installed(&self, _model: ExperimentalAsrModel, _directory: &Path) -> bool {
        true
    }
    fn prepare(
        &self,
        model: ExperimentalAsrModel,
        directory: PathBuf,
    ) -> BoxFuture<'static, Result<(), BackendError>>;
    fn release(&self, model: ExperimentalAsrModel) -> BoxFuture<'static, Result<(), BackendError>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExperimentalAsrPhase {
    Disabled,
    Downloading,
    Preparing,
    Ready,
    Removing,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalAsrStatus {
    pub model: ExperimentalAsrModel,
    pub enabled: bool,
    pub phase: ExperimentalAsrPhase,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub channel_id: Option<String>,
    pub needs_guidance: bool,
    pub error: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    enabled: bool,
    needs_guidance: bool,
    cleanup_pending: bool,
    download_pending: bool,
    install_pending: bool,
    delete_pending: bool,
}
struct Slot {
    saved: Saved,
    phase: ExperimentalAsrPhase,
    error: Option<String>,
    generation: u64,
    cancel: Option<watch::Sender<bool>>,
}

type ChannelChangeHandler = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

pub struct ExperimentalAsrService {
    store: Arc<ModelStore>,
    credentials: Arc<dyn CredentialStore>,
    runtime: Arc<dyn ExperimentalAsrRuntime>,
    spawner: Arc<dyn TaskSpawner>,
    path: PathBuf,
    slots: Mutex<Vec<Slot>>,
    workers: Vec<AsyncMutex<()>>,
    channel_change: Mutex<Option<ChannelChangeHandler>>,
}
impl ExperimentalAsrService {
    pub fn new(
        store: Arc<ModelStore>,
        credentials: Arc<dyn CredentialStore>,
        runtime: Arc<dyn ExperimentalAsrRuntime>,
        spawner: Arc<dyn TaskSpawner>,
        path: PathBuf,
    ) -> Result<Arc<Self>, BackendError> {
        let mut saved: Vec<Saved> = crate::persistence::read_or_default(&path)?;
        if saved.len() > ExperimentalAsrModel::ALL.len() {
            return Err(error("Unsupported newer experimental settings"));
        }
        saved.resize_with(ExperimentalAsrModel::ALL.len(), Saved::default);
        Ok(Arc::new(Self {
            store,
            credentials,
            runtime,
            spawner,
            path,
            slots: Mutex::new(
                saved
                    .into_iter()
                    .map(|saved| Slot {
                        phase: if saved.enabled {
                            ExperimentalAsrPhase::Preparing
                        } else if saved.cleanup_pending {
                            ExperimentalAsrPhase::Removing
                        } else {
                            ExperimentalAsrPhase::Disabled
                        },
                        saved,
                        error: None,
                        generation: 0,
                        cancel: None,
                    })
                    .collect(),
            ),
            workers: ExperimentalAsrModel::ALL
                .iter()
                .map(|_| AsyncMutex::new(()))
                .collect(),
            channel_change: Mutex::new(None),
        }))
    }
    pub fn set_channel_change_handler(&self, handler: ChannelChangeHandler) {
        *self.channel_change.lock().unwrap() = Some(handler);
    }
    async fn channels_changed(&self) {
        let handler = self.channel_change.lock().unwrap().clone();
        if let Some(handler) = handler {
            handler().await;
        }
    }
    fn save(&self, slots: &[Slot]) -> Result<(), BackendError> {
        let bytes =
            serde_json::to_vec_pretty(&slots.iter().map(|slot| &slot.saved).collect::<Vec<_>>())
                .map_err(|e| error(e.to_string()))?;
        crate::persistence::atomic_write(&self.path, &bytes)
    }
    /// Called once by the Host after construction. Interrupted enable/disable
    /// operations recover without tying their lifetime to a settings WebView.
    pub fn resume(self: &Arc<Self>) {
        for model in ExperimentalAsrModel::ALL {
            let needed = {
                let mut slots = self.slots.lock().unwrap();
                let slot = &mut slots[model.index()];
                if slot.saved.enabled
                    && !slot.saved.download_pending
                    && !slot.saved.install_pending
                    && (!self.store.is_installed(&model.target()).unwrap_or(false)
                        || !self
                            .store
                            .runtime_model_dir(&model.target())
                            .is_ok_and(|dir| self.runtime.installed(model, &dir)))
                {
                    slot.phase = ExperimentalAsrPhase::Failed;
                    slot.error =
                        Some("Model files are missing; explicitly enable/retry to download".into());
                    false
                } else {
                    slot.saved.enabled || slot.saved.cleanup_pending
                }
            };
            // Restoring an installed enabled item only reconciles channel metadata.
            // Only an explicitly persisted interrupted download may fetch files.
            if needed {
                self.launch(model);
            }
        }
    }
    pub fn set_enabled(
        self: &Arc<Self>,
        model: ExperimentalAsrModel,
        enabled: bool,
    ) -> Result<(), BackendError> {
        if enabled {
            if let Some(reason) = self.runtime.unavailable_reason(model) {
                return Err(BackendError::new(BackendErrorCode::Unsupported, reason));
            }
        }
        {
            let mut slots = self.slots.lock().unwrap();
            let slot = &mut slots[model.index()];
            if slot.saved.enabled == enabled && !matches!(slot.phase, ExperimentalAsrPhase::Failed)
            {
                return Ok(());
            }
            let previous = slot.saved.clone();
            slot.saved.enabled = enabled;
            slot.saved.cleanup_pending = !enabled;
            slot.saved.download_pending = enabled && !self.store.is_installed(&model.target())?;
            slot.saved.install_pending = enabled;
            slot.saved.delete_pending = false;
            slot.saved.needs_guidance = enabled;
            if let Err(e) = self.save(&slots) {
                slots[model.index()].saved = previous;
                return Err(e);
            }
        }
        self.launch(model);
        Ok(())
    }
    pub fn delete_model(self: &Arc<Self>, model: ExperimentalAsrModel) -> Result<(), BackendError> {
        {
            let mut slots = self.slots.lock().unwrap();
            let slot = &mut slots[model.index()];
            if slot.saved.enabled {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Disable the model before deleting its files",
                ));
            }
            let previous = slot.saved.clone();
            slot.saved.cleanup_pending = true;
            slot.saved.delete_pending = true;
            if let Err(error) = self.save(&slots) {
                slots[model.index()].saved = previous;
                return Err(error);
            }
        }
        self.launch(model);
        Ok(())
    }

    fn launch(self: &Arc<Self>, model: ExperimentalAsrModel) {
        let (generation, enabled, rx) = {
            let mut slots = self.slots.lock().unwrap();
            let slot = &mut slots[model.index()];
            if let Some(tx) = slot.cancel.take() {
                let _ = tx.send(true);
            }
            slot.generation += 1;
            let (tx, rx) = watch::channel(false);
            slot.cancel = Some(tx);
            slot.error = None;
            slot.phase = if slot.saved.enabled {
                ExperimentalAsrPhase::Downloading
            } else {
                ExperimentalAsrPhase::Removing
            };
            (slot.generation, slot.saved.enabled, rx)
        };
        let service = Arc::clone(self);
        self.spawner.spawn(Box::pin(async move {
            let _worker = service.workers[model.index()].lock().await;
            if !service.current(model, generation) {
                return;
            }
            let outcome = service.run(model, generation, enabled, rx).await;
            let mut slots = service.slots.lock().unwrap();
            let slot = &mut slots[model.index()];
            if slot.generation != generation {
                return;
            }
            if let Err(e) = outcome {
                slot.phase = ExperimentalAsrPhase::Failed;
                slot.error = Some(e.to_string());
            }
        }));
    }
    fn current(&self, model: ExperimentalAsrModel, generation: u64) -> bool {
        self.slots.lock().unwrap()[model.index()].generation == generation
    }
    async fn run(
        &self,
        model: ExperimentalAsrModel,
        generation: u64,
        enabled: bool,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<(), BackendError> {
        if !enabled {
            // Only the extension-owned, type-matching channel may be removed.
            let channels = self.credentials.list_channels(ChannelKind::Asr).await?;
            if channels
                .iter()
                .any(|c| c.id == model.channel_id() && c.provider_type == model.provider_type())
            {
                self.credentials
                    .mutate_channel(ChannelMutation::Delete {
                        kind: ChannelKind::Asr,
                        id: model.channel_id().into(),
                    })
                    .await?;
                self.channels_changed().await;
            }
            self.runtime.release(model).await?;
            if !self.current(model, generation) {
                return Ok(());
            }
            if self.slots.lock().unwrap()[model.index()]
                .saved
                .delete_pending
            {
                self.store.delete_model(&model.target())?;
            }
            let mut slots = self.slots.lock().unwrap();
            let slot = &mut slots[model.index()];
            if slot.generation == generation {
                slot.saved.cleanup_pending = false;
                slot.saved.delete_pending = false;
                slot.phase = ExperimentalAsrPhase::Disabled;
                self.save(&slots)?;
            }
            return Ok(());
        }
        if let Some(reason) = self.runtime.unavailable_reason(model) {
            return Err(BackendError::new(BackendErrorCode::Unsupported, reason));
        }
        // Dropping a cancelled download also drops ModelStore's active-download
        // guard; switch-off waits for this worker before deleting staging files.
        tokio::select! {
            biased;
            _ = cancel.wait_for(|cancelled| *cancelled) => return Ok(()),
            result = async {
                if !self.store.is_installed(&model.target())? {
                    if !self.slots.lock().unwrap()[model.index()].saved.download_pending {
                        return Err(error("Model files are missing; explicit download is required"));
                    }
                    self.store.download_target(model.target(), LocalAsrMirror::Huggingface).await?;
                }
                Ok::<(), BackendError>(())
            } => result?,
        }
        if !self.current(model, generation) {
            return Ok(());
        }
        if model == ExperimentalAsrModel::EmbeddingGemma {
            crate::persistence::atomic_write(
                &self
                    .store
                    .runtime_model_dir(&model.target())?
                    .join("LICENSE-Apache-2.0.txt"),
                include_bytes!("experimental-gemma-LICENSE.txt"),
            )?;
        }
        if self.slots.lock().unwrap()[model.index()]
            .saved
            .install_pending
        {
            self.slots.lock().unwrap()[model.index()].phase = ExperimentalAsrPhase::Preparing;
            tokio::select! {
                biased;
                _ = cancel.wait_for(|cancelled| *cancelled) => return Ok(()),
                result = self.runtime.install(model, self.store.runtime_model_dir(&model.target())?) => result?,
            }
            if !self.current(model, generation) {
                return Ok(());
            }
        }
        if !self
            .runtime
            .installed(model, &self.store.runtime_model_dir(&model.target())?)
        {
            return Err(error(
                "Runtime files are missing; explicitly retry to install",
            ));
        }
        {
            let mut slots = self.slots.lock().unwrap();
            slots[model.index()].saved.install_pending = false;
            slots[model.index()].saved.download_pending = false;
            self.save(&slots)?;
        }
        // Download/enable NEVER prepares the runtime. The transcription start
        // path checks the opt-in lease and loads on the first actual recording.
        // Idempotent and append-only: never activates/reorders an existing route.
        self.credentials
            .mutate_channel(ChannelMutation::RegisterExperimentalAsr { model })
            .await?;
        self.channels_changed().await;
        let mut slots = self.slots.lock().unwrap();
        let slot = &mut slots[model.index()];
        if slot.generation == generation {
            slot.phase = ExperimentalAsrPhase::Ready;
            // Guidance was persisted at opt-in; restarting must not re-arm it.
            self.save(&slots)?;
        }
        Ok(())
    }
    /// Changing/turning off an ASR channel releases native state without
    /// uninstalling its package or changing the extension opt-in preference.
    pub fn release_runtime(
        &self,
        model: ExperimentalAsrModel,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.runtime.release(model)
    }

    pub fn acknowledge(&self, model: ExperimentalAsrModel) -> Result<(), BackendError> {
        let mut slots = self.slots.lock().unwrap();
        let previous = slots[model.index()].saved.needs_guidance;
        slots[model.index()].saved.needs_guidance = false;
        if let Err(e) = self.save(&slots) {
            slots[model.index()].saved.needs_guidance = previous;
            return Err(e);
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Vec<ExperimentalAsrStatus>, BackendError> {
        let slots = self.slots.lock().unwrap();
        ExperimentalAsrModel::ALL
            .into_iter()
            .map(|model| {
                let slot = &slots[model.index()];
                let reason = self.runtime.unavailable_reason(model);
                let cache = self.store.pinned_model_status(&model.target())?;
                Ok(ExperimentalAsrStatus {
                    model,
                    enabled: slot.saved.enabled,
                    phase: slot.phase,
                    available: reason.is_none(),
                    unavailable_reason: reason,
                    downloaded_bytes: cache.downloaded_bytes.min(cache.expected_bytes),
                    total_bytes: cache.expected_bytes,
                    channel_id: (slot.phase == ExperimentalAsrPhase::Ready)
                        .then(|| model.channel_id().into()),
                    needs_guidance: slot.saved.needs_guidance
                        && slot.phase == ExperimentalAsrPhase::Ready,
                    error: slot.error.clone(),
                })
            })
            .collect()
    }
    /// Freeze the opt-in generation for a recording; a disable/re-enable cycle
    /// must not revive a session that was using the previous model instance.
    pub fn lease(&self, model: ExperimentalAsrModel) -> Result<(PathBuf, u64), BackendError> {
        let slots = self.slots.lock().unwrap();
        let slot = &slots[model.index()];
        if !slot.saved.enabled
            || slot.phase != ExperimentalAsrPhase::Ready
            || !self.store.is_installed(&model.target())?
            || !self
                .runtime
                .installed(model, &self.store.runtime_model_dir(&model.target())?)
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "Experimental model is not enabled and ready",
            ));
        }
        Ok((
            self.store.runtime_model_dir(&model.target())?,
            slot.generation,
        ))
    }
    pub fn check_lease(
        &self,
        model: ExperimentalAsrModel,
        generation: u64,
    ) -> Result<(), BackendError> {
        let (_, current) = self.lease(model)?;
        if current != generation {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "Experimental model changed during recording",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;

fn error(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, message)
}

/// Verify every pinned byte before native loaders see an existing cache. The
/// installer already checks hashes; this check detects modified installed files.
pub fn verify_package(model: ExperimentalAsrModel, directory: &Path) -> Result<(), BackendError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    for file in model.files() {
        let mut input =
            std::fs::File::open(directory.join(&file.path)).map_err(|e| error(e.to_string()))?;
        if input.metadata().map_err(|e| error(e.to_string()))?.len() != file.size_bytes {
            return Err(error(format!("Invalid size: {}", file.path)));
        }
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let n = input.read(&mut buffer).map_err(|e| error(e.to_string()))?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if Some(format!("{:x}", hash.finalize())) != file.sha256 {
            return Err(error(format!("Invalid checksum: {}", file.path)));
        }
    }
    Ok(())
}
