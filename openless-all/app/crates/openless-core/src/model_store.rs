//! 跨平台模型清单、下载和缓存状态。
//!
//! 该模块拥有文件系统、Range/校验和进度状态；仅网络请求通过窄 Transport
//! 注入，因此 Tauri/Linux 不需要再维护第二套模型存储实现。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{future::BoxFuture, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domains::{LocalAsrModel, LocalAsrModelCard, LocalAsrRemoteFile, LocalAsrRemoteInfo};
use crate::errors::{BackendError, BackendErrorCode};
use crate::local_asr_catalog::{LocalAsrRuntime, LocalAsrTarget};

pub const MODEL_READY_SENTINEL: &str = ".openless-model-ready";
pub const MODEL_PARTIAL_INDEX: &str = ".partial.idx";
const MODEL_RELOCATION_JOURNAL: &str = ".openless-model-relocation.json";
pub const DEFAULT_MODEL_CHUNK_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MODEL_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const DEFAULT_MODEL_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const DEFAULT_MODEL_METADATA_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_MODEL_MAX_RETRIES: u8 = 4;
const PARTIAL_INDEX_VERSION: u8 = 1;

pub fn model_mirror_base(
    mirror: crate::local_asr_catalog::LocalAsrMirror,
) -> Result<&'static str, BackendError> {
    match mirror {
        crate::local_asr_catalog::LocalAsrMirror::Huggingface => Ok("https://huggingface.co"),
        crate::local_asr_catalog::LocalAsrMirror::HfMirror => Ok("https://hf-mirror.com"),
        crate::local_asr_catalog::LocalAsrMirror::GithubRelease => Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "GitHub release models use their catalog URL rather than a Hugging Face mirror",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTransportRequest {
    pub url: String,
    /// Inclusive byte range. `None` requests the complete object.
    pub range: Option<(u64, u64)>,
    pub max_response_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContentRange {
    pub start: u64,
    pub end: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelHttpMetadata {
    pub content_length: Option<u64>,
    pub content_range: Option<ModelContentRange>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTransportResponse {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub metadata: ModelHttpMetadata,
}

pub trait ModelTransport: Send + Sync {
    fn request(
        &self,
        request: ModelTransportRequest,
    ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>>;
}

#[derive(Clone, Default)]
pub struct ReqwestModelTransport;

impl ReqwestModelTransport {
    pub fn new() -> Result<Self, BackendError> {
        Ok(Self)
    }
}

impl ModelTransport for ReqwestModelTransport {
    fn request(
        &self,
        request: ModelTransportRequest,
    ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
        let client = crate::net::model_http();
        Box::pin(async move {
            let mut builder = client.get(&request.url);
            if let Some((start, end)) = request.range {
                builder = builder.header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
            }
            let response = builder.send().await.map_err(|error| {
                BackendError::new(BackendErrorCode::Provider, error.to_string()).retryable(true)
            })?;
            let status = response.status().as_u16();
            let metadata = ModelHttpMetadata {
                content_length: response.content_length(),
                content_range: response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_content_range),
                link: response
                    .headers()
                    .get(reqwest::header::LINK)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string),
            };
            if metadata
                .content_length
                .is_some_and(|length| length > request.max_response_bytes)
            {
                return Err(invalid("model response exceeds the configured size limit"));
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| {
                    BackendError::new(BackendErrorCode::Provider, error.to_string()).retryable(true)
                })?;
                if bytes.len() as u64 + chunk.len() as u64 > request.max_response_bytes {
                    return Err(invalid("model response exceeds the configured size limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(ModelTransportResponse {
                status,
                bytes,
                metadata,
            })
        })
    }
}

fn parse_content_range(value: &str) -> Option<ModelContentRange> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some(ModelContentRange {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
        total: total.parse().ok()?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub path: String,
    pub url: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub target: LocalAsrTarget,
    pub repository: String,
    pub files: Vec<ModelFile>,
    pub total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ModelArchiveSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelArchiveSpec {
    pub file_path: String,
    pub root_dir: String,
    pub required_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub target: LocalAsrTarget,
    pub repository: String,
    pub display_name: String,
    pub family: String,
    pub mode: String,
    pub languages: Vec<String>,
    pub selector: ModelFileSelector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFileMapping {
    pub remote_path: String,
    pub local_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFileSelector {
    QwenRepository,
    Exact(Vec<ModelFileMapping>),
    Native,
    Archive {
        url: String,
        root_dir: String,
        size_bytes: u64,
        sha256: String,
        required_paths: Vec<String>,
    },
}

impl ModelFileSelector {
    fn local_path(&self, remote_path: &str) -> Option<String> {
        match self {
            Self::QwenRepository if qwen_model_file(remote_path) => Some(remote_path.to_string()),
            Self::Exact(files) => files
                .iter()
                .find(|file| file.remote_path == remote_path)
                .map(|file| file.local_path.clone()),
            Self::Native | Self::Archive { .. } | Self::QwenRepository => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    entries: Vec<ModelCatalogEntry>,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::standard()
    }
}

impl ModelCatalog {
    pub fn standard() -> Self {
        let mut entries = Vec::new();
        let mut add = |runtime,
                       id: &str,
                       repository: &str,
                       display_name: &str,
                       family: &str,
                       mode: &str,
                       languages: &[&str],
                       selector| {
            entries.push(ModelCatalogEntry {
                target: LocalAsrTarget::parse(runtime, id).expect("built-in model id"),
                repository: repository.into(),
                display_name: display_name.into(),
                family: family.into(),
                mode: mode.into(),
                languages: languages
                    .iter()
                    .map(|language| (*language).into())
                    .collect(),
                selector,
            });
        };
        for (id, repository) in [
            ("qwen3-asr-0.6b", "Qwen/Qwen3-ASR-0.6B"),
            ("qwen3-asr-1.7b", "Qwen/Qwen3-ASR-1.7B"),
        ] {
            add(
                LocalAsrRuntime::Generic,
                id,
                repository,
                id,
                "qwen3",
                "offline",
                &["multi"],
                ModelFileSelector::QwenRepository,
            );
        }
        for (id, file) in [
            ("whisper-base", "ggml-base.bin"),
            ("whisper-small", "ggml-small.bin"),
            ("whisper-medium", "ggml-medium.bin"),
            ("whisper-large-v3", "ggml-large-v3.bin"),
            ("whisper-large-v3-turbo", "ggml-large-v3-turbo.bin"),
            ("whisper-large-v3-turbo-q5", "ggml-large-v3-turbo-q5_0.bin"),
        ] {
            add(
                LocalAsrRuntime::Generic,
                id,
                "ggerganov/whisper.cpp",
                id,
                "whisper",
                "offline",
                &["multi"],
                exact(&[(file, file)]),
            );
        }
        for id in [
            "whisper-small",
            "whisper-medium",
            "whisper-large-v3-turbo",
            "whisper-base",
            "whisper-tiny",
        ] {
            add(
                LocalAsrRuntime::Foundry,
                id,
                "microsoft/whisper",
                id,
                "whisper",
                "offline",
                &["multi"],
                ModelFileSelector::Native,
            );
        }
        for (id, repository, display_name, languages, files) in [
            (
                "sense-voice-small-zh",
                "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17",
                "SenseVoice Small (zh/en/ja/ko/yue)",
                &["zh", "en", "ja", "ko", "yue"][..].as_ref(),
                &[
                    ("model.int8.onnx", "model.int8.onnx"),
                    ("tokens.txt", "tokens.txt"),
                ][..],
            ),
            (
                "paraformer-zh",
                "csukuangfj/sherpa-onnx-paraformer-zh-2024-03-09",
                "Paraformer (zh)",
                &["zh"][..].as_ref(),
                &[
                    ("model.int8.onnx", "model.int8.onnx"),
                    ("tokens.txt", "tokens.txt"),
                ][..],
            ),
            (
                "whisper-small-multi",
                "csukuangfj/sherpa-onnx-whisper-small",
                "Whisper Small (multilingual)",
                &["multi"][..].as_ref(),
                &[
                    ("small-encoder.int8.onnx", "encoder.int8.onnx"),
                    ("small-decoder.int8.onnx", "decoder.int8.onnx"),
                    ("small-tokens.txt", "tokens.txt"),
                ][..],
            ),
            (
                "whisper-large-v3-multi",
                "csukuangfj/sherpa-onnx-whisper-large-v3",
                "Whisper Large V3 (multilingual)",
                &["multi"][..].as_ref(),
                &[
                    ("large-v3-encoder.int8.onnx", "encoder.int8.onnx"),
                    ("large-v3-decoder.int8.onnx", "decoder.int8.onnx"),
                    ("large-v3-tokens.txt", "tokens.txt"),
                ][..],
            ),
            (
                "zipformer-bilingual-zh-en-streaming",
                "csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20",
                "Zipformer Streaming bilingual (zh/en)",
                &["zh", "en"][..].as_ref(),
                &[
                    (
                        "encoder-epoch-99-avg-1.int8.onnx",
                        "encoder-epoch-99-avg-1.int8.onnx",
                    ),
                    ("decoder-epoch-99-avg-1.onnx", "decoder-epoch-99-avg-1.onnx"),
                    (
                        "joiner-epoch-99-avg-1.int8.onnx",
                        "joiner-epoch-99-avg-1.int8.onnx",
                    ),
                    ("tokens.txt", "tokens.txt"),
                ][..],
            ),
        ] {
            let target = LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, id)
                .expect("built-in Sherpa model id");
            let family = target
                .sherpa_family()
                .expect("Sherpa model family")
                .as_str();
            let mode = target
                .sherpa_execution_mode()
                .expect("Sherpa execution mode")
                .as_str();
            add(
                LocalAsrRuntime::SherpaOnnx,
                id,
                repository,
                display_name,
                family,
                mode,
                languages,
                exact(files),
            );
        }
        let qwen_sherpa = LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "qwen3-asr-0.6b-int8")
            .expect("built-in Sherpa Qwen model id");
        add(
            LocalAsrRuntime::SherpaOnnx,
            "qwen3-asr-0.6b-int8",
            "",
            "Qwen3-ASR 0.6B INT8",
            qwen_sherpa
                .sherpa_family()
                .expect("Sherpa model family")
                .as_str(),
            qwen_sherpa
                .sherpa_execution_mode()
                .expect("Sherpa execution mode")
                .as_str(),
            &["multi"],
            ModelFileSelector::Archive {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2".into(),
                root_dir: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25".into(),
                size_bytes: 878_702_423,
                sha256: "393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96".into(),
                required_paths: vec![
                    "conv_frontend.onnx".into(),
                    "encoder.int8.onnx".into(),
                    "decoder.int8.onnx".into(),
                    "tokenizer/tokenizer.json".into(),
                ],
            },
        );
        Self { entries }
    }

    pub fn entries(&self) -> &[ModelCatalogEntry] {
        &self.entries
    }

    pub fn find(&self, runtime: LocalAsrRuntime, model_id: &str) -> Option<&ModelCatalogEntry> {
        self.entries
            .iter()
            .find(|entry| entry.target.runtime == runtime && entry.target.model_id() == model_id)
    }
}

impl Default for ModelCatalogEntry {
    fn default() -> Self {
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b")
            .expect("built-in model id");
        Self {
            target,
            repository: "Qwen/Qwen3-ASR-0.6B".into(),
            display_name: "qwen3-asr-0.6b".into(),
            family: "qwen3".into(),
            mode: "offline".into(),
            languages: vec!["multi".into()],
            selector: ModelFileSelector::QwenRepository,
        }
    }
}

fn exact(files: &[(&str, &str)]) -> ModelFileSelector {
    ModelFileSelector::Exact(
        files
            .iter()
            .map(|(remote_path, local_path)| ModelFileMapping {
                remote_path: (*remote_path).into(),
                local_path: (*local_path).into(),
            })
            .collect(),
    )
}

fn entry_is_complete(entry: &ModelCatalogEntry, directory: &Path) -> bool {
    if !directory.join(MODEL_READY_SENTINEL).is_file() {
        return false;
    }
    match &entry.selector {
        ModelFileSelector::QwenRepository => true,
        ModelFileSelector::Exact(files) => files
            .iter()
            .all(|file| directory.join(&file.local_path).is_file()),
        ModelFileSelector::Archive { required_paths, .. } => required_paths
            .iter()
            .all(|path| directory.join(path).is_file()),
        ModelFileSelector::Native => false,
    }
}

impl ModelManifest {
    pub fn new(
        target: LocalAsrTarget,
        repository: impl Into<String>,
        files: Vec<ModelFile>,
    ) -> Result<Self, BackendError> {
        let repository = repository.into();
        validate_model_id(target.model_id())?;
        let mut seen = BTreeSet::new();
        for file in &files {
            validate_model_path(&file.path)?;
            validate_model_url(&file.url)?;
            if !seen.insert(file.path.clone()) {
                return Err(invalid("model manifest contains duplicate files"));
            }
            if file.size_bytes > DEFAULT_MODEL_MAX_FILE_BYTES {
                return Err(invalid("model file exceeds the configured size limit"));
            }
            if let Some(sha256) = &file.sha256 {
                if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(invalid("model file has an invalid sha256"));
                }
            }
        }
        if files.is_empty() {
            return Err(invalid("model manifest must contain at least one file"));
        }
        let total_bytes = files.iter().try_fold(0u64, |total, file| {
            total
                .checked_add(file.size_bytes)
                .ok_or_else(|| invalid("model total size overflowed"))
        })?;
        Ok(Self {
            target,
            repository,
            files,
            total_bytes,
            archive: None,
        })
    }

    pub fn from_hf_pages(
        target: LocalAsrTarget,
        repository: impl Into<String>,
        pages: &[Vec<serde_json::Value>],
    ) -> Result<Self, BackendError> {
        let repository = repository.into();
        let files = merge_hf_tree_pages(&repository, target.model_id(), pages)?;
        Self::new(target, repository, files)
    }

    pub fn from_hf_pages_with_base(
        target: LocalAsrTarget,
        repository: impl Into<String>,
        pages: &[Vec<serde_json::Value>],
        base_url: &str,
    ) -> Result<Self, BackendError> {
        let repository = repository.into();
        let files = merge_hf_tree_pages_with_base(&repository, target.model_id(), pages, base_url)?;
        Self::new(target, repository, files)
    }
}

#[derive(Debug, Clone)]
pub struct ModelStoreConfig {
    pub models_root_dir: PathBuf,
    pub chunk_size_bytes: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_retries: u8,
}

impl ModelStoreConfig {
    pub fn new(models_root_dir: PathBuf) -> Result<Self, BackendError> {
        if !models_root_dir.is_absolute() {
            return Err(invalid("model root directory must be absolute"));
        }
        if models_root_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(invalid("model root directory cannot contain '..'"));
        }
        Ok(Self {
            models_root_dir,
            chunk_size_bytes: DEFAULT_MODEL_CHUNK_BYTES,
            max_file_bytes: DEFAULT_MODEL_MAX_FILE_BYTES,
            max_total_bytes: DEFAULT_MODEL_MAX_TOTAL_BYTES,
            max_retries: DEFAULT_MODEL_MAX_RETRIES,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelDownloadPhase {
    Started,
    Progress,
    Finished,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadProgress {
    pub runtime: LocalAsrRuntime,
    pub model_id: String,
    pub file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub phase: ModelDownloadPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub trait DownloadProgressSink: Send + Sync {
    fn publish(&self, progress: ModelDownloadProgress);
}

impl<F> DownloadProgressSink for F
where
    F: Fn(ModelDownloadProgress) + Send + Sync,
{
    fn publish(&self, progress: ModelDownloadProgress) {
        self(progress)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCacheStatus {
    pub model_id: String,
    pub ready: bool,
    pub downloaded_bytes: u64,
    pub expected_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCard {
    pub model_id: String,
    pub repository: String,
    pub downloads: u64,
    pub likes: u64,
    pub description: String,
}

pub struct ModelStore {
    config: ModelStoreConfig,
    models_root_dir: Arc<std::sync::RwLock<PathBuf>>,
    catalog: ModelCatalog,
    transport: Arc<dyn ModelTransport>,
    progress: Arc<std::sync::RwLock<Option<Arc<dyn DownloadProgressSink>>>>,
    progress_clock: Arc<Mutex<HashMap<LocalAsrTarget, u64>>>,
    active_downloads: Arc<Mutex<HashMap<LocalAsrTarget, Arc<AtomicBool>>>>,
}

impl ModelStore {
    pub fn new(config: ModelStoreConfig) -> Result<Self, BackendError> {
        let store = Self::with_transport(config, Arc::new(ReqwestModelTransport::new()?));
        if let Err(error) = store.finish_pending_relocation() {
            log::warn!("[model-store] deferred relocation cleanup failed: {error}");
        }
        Ok(store)
    }

    pub fn with_transport(config: ModelStoreConfig, transport: Arc<dyn ModelTransport>) -> Self {
        let models_root_dir = Arc::new(std::sync::RwLock::new(config.models_root_dir.clone()));
        Self {
            config,
            models_root_dir,
            catalog: ModelCatalog::standard(),
            transport,
            progress: Arc::new(std::sync::RwLock::new(None)),
            progress_clock: Arc::new(Mutex::new(HashMap::new())),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_progress_sink(self, sink: Arc<dyn DownloadProgressSink>) -> Self {
        *self
            .progress
            .write()
            .expect("model progress sink lock poisoned") = Some(sink);
        self
    }

    pub fn set_progress_sink(&self, sink: Arc<dyn DownloadProgressSink>) {
        *self
            .progress
            .write()
            .expect("model progress sink lock poisoned") = Some(sink);
    }

    pub fn config(&self) -> &ModelStoreConfig {
        &self.config
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn models_root_dir(&self) -> PathBuf {
        self.models_root_dir
            .read()
            .expect("model root lock poisoned")
            .clone()
    }

    pub fn list_models(
        &self,
        runtime: LocalAsrRuntime,
    ) -> Result<Vec<LocalAsrModel>, BackendError> {
        self.catalog
            .entries()
            .iter()
            .filter(|entry| entry.target.runtime == runtime)
            .map(|entry| {
                let runtime_directory = self.runtime_model_dir(&entry.target)?;
                let native = matches!(entry.selector, ModelFileSelector::Native);
                Ok(LocalAsrModel {
                    target: entry.target.clone(),
                    display_name: entry.display_name.clone(),
                    family: entry.family.clone(),
                    mode: Some(entry.mode.clone()),
                    repository: (!entry.repository.is_empty()).then(|| entry.repository.clone()),
                    languages: entry.languages.clone(),
                    installed: !native && self.is_installed(&entry.target)?,
                    downloaded_bytes: if native {
                        0
                    } else {
                        directory_size(&runtime_directory)
                            .unwrap_or(0)
                            .saturating_add(self.partial_downloaded_bytes(&entry.target, None)?)
                    },
                    size_bytes: None,
                })
            })
            .collect()
    }

    pub async fn remote_info(
        &self,
        target: LocalAsrTarget,
        mirror: crate::local_asr_catalog::LocalAsrMirror,
    ) -> Result<LocalAsrRemoteInfo, BackendError> {
        let entry = self
            .catalog
            .find(target.runtime, target.model_id())
            .ok_or_else(|| invalid("unknown local ASR model"))?;
        let files = match &entry.selector {
            ModelFileSelector::Native => {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "native runtime manages this model install",
                ));
            }
            ModelFileSelector::Archive {
                url,
                size_bytes,
                sha256,
                ..
            } => vec![LocalAsrRemoteFile {
                path: url.clone(),
                local_path: archive_file_name(url),
                size_bytes: *size_bytes,
                sha256: Some(sha256.clone()),
            }],
            ModelFileSelector::QwenRepository | ModelFileSelector::Exact(_) => self
                .fetch_hf_manifest(
                    target.clone(),
                    &entry.repository,
                    model_mirror_base(mirror)?,
                )
                .await?
                .files
                .into_iter()
                .map(|file| LocalAsrRemoteFile {
                    path: file.url,
                    local_path: Some(file.path),
                    size_bytes: file.size_bytes,
                    sha256: file.sha256,
                })
                .collect(),
        };
        Ok(LocalAsrRemoteInfo {
            target,
            mirror,
            total_bytes: files.iter().map(|file| file.size_bytes).sum(),
            files,
        })
    }

    pub async fn model_card(
        &self,
        target: LocalAsrTarget,
        mirror: crate::local_asr_catalog::LocalAsrMirror,
    ) -> Result<LocalAsrModelCard, BackendError> {
        let entry = self
            .catalog
            .find(target.runtime, target.model_id())
            .ok_or_else(|| invalid("unknown local ASR model"))?;
        if matches!(entry.selector, ModelFileSelector::Native) {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "native runtime manages this model card",
            ));
        }
        let card = if entry.repository.is_empty() {
            ModelCard {
                model_id: target.model_id().into(),
                repository: String::new(),
                downloads: 0,
                likes: 0,
                description: entry.display_name.clone(),
            }
        } else {
            self.fetch_hf_model_card(
                target.model_id(),
                &entry.repository,
                model_mirror_base(mirror)?,
            )
            .await?
        };
        Ok(LocalAsrModelCard {
            target,
            mirror,
            downloads: card.downloads,
            likes: card.likes,
            description: card.description,
        })
    }

    pub async fn download_target(
        &self,
        target: LocalAsrTarget,
        mirror: crate::local_asr_catalog::LocalAsrMirror,
    ) -> Result<ModelCacheStatus, BackendError> {
        let entry = self
            .catalog
            .find(target.runtime, target.model_id())
            .cloned()
            .ok_or_else(|| invalid("unknown local ASR model"))?;
        if matches!(entry.selector, ModelFileSelector::Native) {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "native runtime manages this model install",
            ));
        }
        let (cancelled, _active_guard) = self.begin_active_download(&target)?;
        let manifest = match entry.selector {
            ModelFileSelector::Native => unreachable!("native selector returned above"),
            ModelFileSelector::Archive {
                url,
                root_dir,
                size_bytes,
                sha256,
                required_paths,
            } => archive_file_name(&url)
                .ok_or_else(|| invalid("archive URL has no file name"))
                .and_then(|file_path| {
                    let mut manifest = ModelManifest::new(
                        target.clone(),
                        "github-release",
                        vec![ModelFile {
                            path: file_path.clone(),
                            url,
                            size_bytes,
                            sha256: Some(sha256),
                        }],
                    )?;
                    manifest.archive = Some(ModelArchiveSpec {
                        file_path,
                        root_dir,
                        required_paths,
                    });
                    Ok(manifest)
                }),
            ModelFileSelector::QwenRepository | ModelFileSelector::Exact(_) => {
                self.fetch_hf_manifest_with_cancel(
                    target.clone(),
                    &entry.repository,
                    model_mirror_base(mirror)?,
                    Some(Arc::clone(&cancelled)),
                )
                .await
            }
        };
        let manifest = match manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                self.emit_target_terminal(
                    &target,
                    if error.code == BackendErrorCode::Cancelled {
                        ModelDownloadPhase::Cancelled
                    } else {
                        ModelDownloadPhase::Failed
                    },
                    error.message.clone(),
                );
                return Err(error);
            }
        };
        self.download_registered(manifest, cancelled).await
    }

    pub async fn fetch_hf_manifest(
        &self,
        target: LocalAsrTarget,
        repository: &str,
        base_url: &str,
    ) -> Result<ModelManifest, BackendError> {
        self.fetch_hf_manifest_with_cancel(target, repository, base_url, None)
            .await
    }

    async fn fetch_hf_manifest_with_cancel(
        &self,
        target: LocalAsrTarget,
        repository: &str,
        base_url: &str,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<ModelManifest, BackendError> {
        validate_model_id(target.model_id())?;
        let base_url = base_url.trim_end_matches('/');
        validate_model_url(&format!("{base_url}/"))?;
        let entry = self
            .catalog
            .entries()
            .iter()
            .find(|entry| entry.target == target && entry.repository == repository)
            .ok_or_else(|| invalid("model is not present in the Core catalog"))?;
        if matches!(
            entry.selector,
            ModelFileSelector::Native | ModelFileSelector::Archive { .. }
        ) {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "selected model is not downloaded from a Hugging Face tree",
            ));
        }
        let mut pages = Vec::new();
        let mut url = format!("{base_url}/api/models/{repository}/tree/main?limit=1000");
        let mut seen_urls = BTreeSet::new();
        for page_index in 0..100 {
            if !seen_urls.insert(url.clone()) {
                return Err(invalid("model manifest pagination repeated a URL"));
            }
            let request = self.transport.request(ModelTransportRequest {
                url: url.clone(),
                range: None,
                max_response_bytes: DEFAULT_MODEL_METADATA_BYTES,
            });
            let response = if let Some(cancelled) = cancelled.as_ref() {
                tokio::select! {
                    value = request => value?,
                    () = wait_until_cancelled(Arc::clone(cancelled)) => {
                        return Err(cancelled_error());
                    }
                }
            } else {
                request.await?
            };
            if response.status != 200 {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    format!("model manifest request returned HTTP {}", response.status),
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(&response.bytes)
                .map_err(|error| invalid(format!("invalid model manifest JSON: {error}")))?;
            let entries = value
                .as_array()
                .cloned()
                .or_else(|| {
                    value
                        .get("entries")
                        .and_then(|items| items.as_array())
                        .cloned()
                })
                .ok_or_else(|| invalid("model manifest response must be an array"))?;
            pages.push(entries);
            let next = match response.metadata.link.as_deref() {
                Some(link) => next_hf_link(link, &url, base_url)?,
                None => None,
            };
            let Some(next) = next else {
                break;
            };
            if page_index == 99 {
                return Err(invalid("model manifest pagination exceeded the page limit"));
            }
            url = next;
        }
        manifest_from_hf_pages(entry, &pages, base_url, self.config.max_total_bytes)
    }

    pub async fn fetch_hf_model_card(
        &self,
        model_id: &str,
        repository: &str,
        base_url: &str,
    ) -> Result<ModelCard, BackendError> {
        validate_model_id(model_id)?;
        let base_url = base_url.trim_end_matches('/');
        validate_model_url(&format!("{base_url}/"))?;
        let response = self
            .transport
            .request(ModelTransportRequest {
                url: format!("{base_url}/api/models/{repository}"),
                range: None,
                max_response_bytes: DEFAULT_MODEL_METADATA_BYTES,
            })
            .await?;
        if response.status != 200 {
            return Err(BackendError::new(
                BackendErrorCode::Provider,
                format!("model card request returned HTTP {}", response.status),
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&response.bytes)
            .map_err(|error| invalid(format!("invalid model card JSON: {error}")))?;
        let mut description = value
            .pointer("/cardData/summary")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                value
                    .get("description")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_default()
            .to_string();
        if description.trim().is_empty() {
            let readme = self
                .transport
                .request(ModelTransportRequest {
                    url: format!("{base_url}/{repository}/raw/main/README.md"),
                    range: None,
                    max_response_bytes: DEFAULT_MODEL_METADATA_BYTES,
                })
                .await;
            if let Ok(response) = readme {
                if response.status == 200 {
                    description = std::str::from_utf8(&response.bytes)
                        .map(first_readme_paragraph)
                        .unwrap_or_default();
                }
            }
        }
        Ok(ModelCard {
            model_id: model_id.into(),
            repository: repository.into(),
            downloads: value
                .get("downloads")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            likes: value
                .get("likes")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            description: truncate_description(&description),
        })
    }

    pub fn model_dir(&self, target: &LocalAsrTarget) -> Result<PathBuf, BackendError> {
        validate_model_id(target.model_id())?;
        let root = self.models_root_dir();
        Ok(match target.runtime {
            LocalAsrRuntime::Generic => root.join(target.model_id()),
            LocalAsrRuntime::Foundry => root.join("foundry-local"),
            LocalAsrRuntime::SherpaOnnx => root.join("sherpa-onnx").join(target.model_id()),
        })
    }

    pub fn is_native(&self, target: &LocalAsrTarget) -> Result<bool, BackendError> {
        self.catalog
            .find(target.runtime, target.model_id())
            .map(|entry| matches!(entry.selector, ModelFileSelector::Native))
            .ok_or_else(|| invalid("unknown local ASR model"))
    }

    pub fn is_installed(&self, target: &LocalAsrTarget) -> Result<bool, BackendError> {
        let entry = self
            .catalog
            .find(target.runtime, target.model_id())
            .ok_or_else(|| invalid("unknown local ASR model"))?;
        if matches!(entry.selector, ModelFileSelector::Native) {
            return Ok(false);
        }
        if entry_is_complete(entry, &self.model_dir(target)?) {
            return Ok(true);
        }
        if target.runtime == LocalAsrRuntime::Generic
            && target.model_id() == "whisper-large-v3-turbo"
        {
            let q5 = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-large-v3-turbo-q5")?;
            let q5_entry = self
                .catalog
                .find(q5.runtime, q5.model_id())
                .expect("built-in Q5 model");
            return Ok(entry_is_complete(q5_entry, &self.model_dir(&q5)?));
        }
        Ok(false)
    }

    pub fn runtime_model_dir(&self, target: &LocalAsrTarget) -> Result<PathBuf, BackendError> {
        let primary = self.model_dir(target)?;
        if target.runtime == LocalAsrRuntime::Generic
            && target.model_id() == "whisper-large-v3-turbo"
            && !entry_is_complete(
                self.catalog
                    .find(target.runtime, target.model_id())
                    .expect("built-in Turbo model"),
                &primary,
            )
        {
            let q5 = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-large-v3-turbo-q5")?;
            let fallback = self.model_dir(&q5)?;
            if self.is_installed(&q5)? {
                return Ok(fallback);
            }
        }
        Ok(primary)
    }

    pub fn status(&self, manifest: &ModelManifest) -> Result<ModelCacheStatus, BackendError> {
        let dir = self.model_dir(&manifest.target)?;
        let complete_bytes = manifest
            .files
            .iter()
            .map(|file| {
                std::fs::metadata(dir.join(&file.path))
                    .map(|meta| meta.len())
                    .unwrap_or(0)
            })
            .fold(0u64, u64::saturating_add);
        let expected = manifest
            .files
            .iter()
            .map(|file| (file.path.clone(), file.size_bytes))
            .collect();
        let downloaded_bytes = complete_bytes
            .saturating_add(self.partial_downloaded_bytes(&manifest.target, Some(&expected))?);
        let complete_files = manifest.files.iter().all(|file| {
            std::fs::metadata(dir.join(&file.path))
                .map(|meta| meta.len() == file.size_bytes)
                .unwrap_or(false)
        });
        Ok(ModelCacheStatus {
            model_id: manifest.target.model_id().to_string(),
            ready: dir.join(MODEL_READY_SENTINEL).is_file() && complete_files,
            downloaded_bytes,
            expected_bytes: manifest.total_bytes,
        })
    }

    fn partial_downloaded_bytes(
        &self,
        target: &LocalAsrTarget,
        expected: Option<&BTreeMap<String, u64>>,
    ) -> Result<u64, BackendError> {
        let staging = self.models_root_dir().join(staging_dir_name(target));
        if !staging.is_dir() || !staging.join(MODEL_PARTIAL_INDEX).is_file() {
            return Ok(0);
        }
        match trusted_partial_bytes(
            &staging,
            expected,
            self.config.max_file_bytes,
            self.config.max_total_bytes,
        )? {
            Some(bytes) => Ok(bytes),
            None => {
                let active = self
                    .active_downloads
                    .lock()
                    .expect("model download lock poisoned");
                if !active.contains_key(target) {
                    std::fs::remove_dir_all(staging).map_err(platform_error)?;
                }
                Ok(0)
            }
        }
    }

    pub async fn download(
        &self,
        manifest: ModelManifest,
    ) -> Result<ModelCacheStatus, BackendError> {
        if self.is_native(&manifest.target).unwrap_or(false) {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "native models are installed by the runtime adapter",
            ));
        }
        let (cancelled, _active_guard) = self.begin_active_download(&manifest.target)?;
        self.download_registered(manifest, cancelled).await
    }

    fn begin_active_download(
        &self,
        target: &LocalAsrTarget,
    ) -> Result<(Arc<AtomicBool>, ActiveDownloadGuard), BackendError> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut active = self
            .active_downloads
            .lock()
            .expect("model download lock poisoned");
        if active.contains_key(target) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "model download is already in progress",
            ));
        }
        active.insert(target.clone(), Arc::clone(&cancelled));
        Ok((
            Arc::clone(&cancelled),
            ActiveDownloadGuard {
                active: Arc::clone(&self.active_downloads),
                target: target.clone(),
                cancelled,
            },
        ))
    }

    async fn download_registered(
        &self,
        manifest: ModelManifest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ModelCacheStatus, BackendError> {
        let progress_manifest = manifest.clone();
        let result = self.download_with_manifest(manifest, cancelled).await;
        if let Err(error) = &result {
            if error.code != BackendErrorCode::Busy {
                self.emit(
                    &progress_manifest,
                    "",
                    progress_manifest.files.len(),
                    if error.code == BackendErrorCode::Cancelled {
                        ModelDownloadPhase::Cancelled
                    } else {
                        ModelDownloadPhase::Failed
                    },
                    None,
                    Some(error.message.clone()),
                );
            }
        }
        result
    }

    async fn download_with_manifest(
        &self,
        manifest: ModelManifest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ModelCacheStatus, BackendError> {
        if manifest.total_bytes > self.config.max_total_bytes {
            return Err(invalid("model exceeds the configured total size limit"));
        }
        if manifest
            .files
            .iter()
            .any(|file| file.size_bytes > self.config.max_file_bytes)
        {
            return Err(invalid("model file exceeds the configured size limit"));
        }
        let root = self.models_root_dir();
        std::fs::create_dir_all(&root).map_err(platform_error)?;
        let staging = root.join(staging_dir_name(&manifest.target));
        std::fs::create_dir_all(&staging).map_err(platform_error)?;
        let mut partial = restore_partial_index(&staging, &manifest)?;
        self.emit(&manifest, "", 0, ModelDownloadPhase::Started, None, None);
        let mut downloaded_before: u64 = partial.files.values().copied().sum();
        for (file_index, file) in manifest.files.iter().enumerate() {
            if cancelled.load(Ordering::Acquire) {
                return Err(cancelled_error());
            }
            let path = staging.join(&file.path);
            validate_model_path(&file.path)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(platform_error)?;
            }
            let mut offset = partial.files.get(&file.path).copied().unwrap_or(0);
            let mut output = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(offset == 0)
                .open(&path)
                .map_err(platform_error)?;
            if offset > 0 {
                output
                    .seek(std::io::SeekFrom::Start(offset))
                    .map_err(platform_error)?;
            }
            while offset < file.size_bytes {
                if cancelled.load(Ordering::Acquire) {
                    return Err(cancelled_error());
                }
                let end = (offset + self.config.chunk_size_bytes.max(1)).min(file.size_bytes) - 1;
                let mut response = None;
                let mut last_error = None;
                for attempt in 0..=self.config.max_retries {
                    let request = self.transport.request(ModelTransportRequest {
                        url: file.url.clone(),
                        range: Some((offset, end)),
                        max_response_bytes: end - offset + 1,
                    });
                    let requested = tokio::select! {
                        value = request => value,
                        () = wait_until_cancelled(Arc::clone(&cancelled)) => {
                            return Err(cancelled_error());
                        }
                    };
                    match requested {
                        Ok(value) => {
                            match validate_range_response(&value, offset, end, file.size_bytes) {
                                Ok(()) => {
                                    response = Some(value);
                                    break;
                                }
                                Err(error) => last_error = Some(error.message),
                            }
                        }
                        Err(error) => last_error = Some(error.message),
                    }
                    if attempt < self.config.max_retries {
                        tokio::time::sleep(Duration::from_millis(
                            50u64.saturating_mul(1u64 << attempt.min(6)),
                        ))
                        .await;
                    }
                }
                let response = match response {
                    Some(response) => response,
                    None => {
                        let message = last_error.unwrap_or_else(|| "model download failed".into());
                        return Err(
                            BackendError::new(BackendErrorCode::Provider, message).retryable(true)
                        );
                    }
                };
                if cancelled.load(Ordering::Acquire) {
                    return Err(cancelled_error());
                }
                output.write_all(&response.bytes).map_err(platform_error)?;
                let received = response.bytes.len() as u64;
                offset = offset.saturating_add(received);
                downloaded_before = downloaded_before.saturating_add(received);
                partial.files.insert(file.path.clone(), offset);
                write_partial_index(&staging, &partial)?;
                self.emit(
                    &manifest,
                    &file.path,
                    file_index,
                    ModelDownloadPhase::Progress,
                    Some((downloaded_before, manifest.total_bytes)),
                    None,
                );
            }
            output.flush().map_err(platform_error)?;
            if let Some(expected) = &file.sha256 {
                let actual = sha256_file(&path)?;
                if !actual.eq_ignore_ascii_case(expected) {
                    let _ = std::fs::remove_file(&path);
                    return Err(BackendError::new(
                        BackendErrorCode::Provider,
                        format!("checksum mismatch for {}", file.path),
                    ));
                }
            }
            partial.files.insert(file.path.clone(), file.size_bytes);
            write_partial_index(&staging, &partial)?;
        }
        if let Some(archive) = &manifest.archive {
            expand_tar_bz2_archive(
                &staging,
                archive,
                self.config.max_file_bytes,
                self.config.max_total_bytes,
            )?;
        }
        let sentinel = staging.join(MODEL_READY_SENTINEL);
        std::fs::write(&sentinel, b"ready\n").map_err(platform_error)?;
        let _ = std::fs::remove_file(staging.join(MODEL_PARTIAL_INDEX));
        let destination = self.model_dir(&manifest.target)?;
        commit_staging(&staging, &destination)?;
        self.emit(
            &manifest,
            "",
            manifest.files.len(),
            ModelDownloadPhase::Finished,
            Some((manifest.total_bytes, manifest.total_bytes)),
            None,
        );
        if manifest.archive.is_some() {
            Ok(ModelCacheStatus {
                model_id: manifest.target.model_id().to_string(),
                ready: destination.join(MODEL_READY_SENTINEL).is_file(),
                downloaded_bytes: manifest.total_bytes,
                expected_bytes: manifest.total_bytes,
            })
        } else {
            self.status(&manifest)
        }
    }

    pub fn cancel_download(&self, target: &LocalAsrTarget) -> Result<bool, BackendError> {
        validate_model_id(target.model_id())?;
        let active = self
            .active_downloads
            .lock()
            .expect("model download lock poisoned")
            .get(target)
            .cloned();
        if let Some(cancelled) = active {
            cancelled.store(true, Ordering::Release);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn cancel_all_downloads_and_wait(&self) -> Result<(), BackendError> {
        {
            let active = self
                .active_downloads
                .lock()
                .expect("model download lock poisoned");
            for cancelled in active.values() {
                cancelled.store(true, Ordering::Release);
            }
        }
        for _ in 0..3_000 {
            if self
                .active_downloads
                .lock()
                .expect("model download lock poisoned")
                .is_empty()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(BackendError::new(
            BackendErrorCode::Busy,
            "timed out while waiting for model downloads to stop",
        ))
    }

    fn emit(
        &self,
        manifest: &ModelManifest,
        file: &str,
        file_index: usize,
        phase: ModelDownloadPhase,
        bytes: Option<(u64, u64)>,
        error: Option<String>,
    ) {
        let sink = self
            .progress
            .read()
            .expect("model progress sink lock poisoned")
            .clone();
        let Some(sink) = sink else {
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or(0);
        let mut clocks = self
            .progress_clock
            .lock()
            .expect("model progress lock poisoned");
        let last = clocks.entry(manifest.target.clone()).or_default();
        if phase == ModelDownloadPhase::Progress && now.saturating_sub(*last) < 150 {
            return;
        }
        *last = now;
        let (downloaded, total) = bytes.unwrap_or((0, manifest.total_bytes));
        sink.publish(ModelDownloadProgress {
            runtime: manifest.target.runtime,
            model_id: manifest.target.model_id().to_string(),
            file: file.into(),
            file_index,
            file_count: manifest.files.len(),
            bytes_downloaded: downloaded,
            bytes_total: total,
            phase,
            error,
        });
    }

    fn emit_target_terminal(
        &self,
        target: &LocalAsrTarget,
        phase: ModelDownloadPhase,
        error: String,
    ) {
        let sink = self
            .progress
            .read()
            .expect("model progress sink lock poisoned")
            .clone();
        if let Some(sink) = sink {
            sink.publish(ModelDownloadProgress {
                runtime: target.runtime,
                model_id: target.model_id().to_string(),
                file: String::new(),
                file_index: 0,
                file_count: 0,
                bytes_downloaded: 0,
                bytes_total: 0,
                phase,
                error: Some(error),
            });
        }
    }

    pub fn cleanup_incomplete(&self, target: &LocalAsrTarget) -> Result<(), BackendError> {
        validate_model_id(target.model_id())?;
        let staging = self.models_root_dir().join(staging_dir_name(target));
        if staging.exists() {
            std::fs::remove_dir_all(staging).map_err(platform_error)?;
        }
        Ok(())
    }

    pub fn migrate_legacy_root(&self, legacy_root: &Path) -> Result<(), BackendError> {
        if !legacy_root.is_absolute() {
            return Err(invalid("legacy model root must be absolute"));
        }
        if !legacy_root.is_dir() {
            return Ok(());
        }
        let root = self.models_root_dir();
        std::fs::create_dir_all(&root).map_err(platform_error)?;
        for entry in self.catalog.entries() {
            if matches!(entry.selector, ModelFileSelector::Native) {
                continue;
            }
            let destination = self.model_dir(&entry.target)?;
            if entry.target.runtime == LocalAsrRuntime::SherpaOnnx {
                let legacy_staging =
                    legacy_root.join(format!(".{}.staging", entry.target.model_id()));
                let staging = root.join(staging_dir_name(&entry.target));
                if legacy_staging.is_dir() && legacy_staging != staging {
                    copy_dir_missing(&legacy_staging, &staging).map_err(platform_error)?;
                    if source_fully_copied(&legacy_staging, &staging)? {
                        remove_path(&legacy_staging)?;
                    }
                }
            }
            if entry.target.runtime == LocalAsrRuntime::Generic
                && entry.target.model_id() == "whisper-large-v3-turbo-q5"
            {
                migrate_legacy_q5(legacy_root, &destination)?;
            }
            for source in legacy_source_candidates(legacy_root, &entry.target) {
                if !source.exists() || source == destination {
                    continue;
                }
                copy_legacy_entry_missing(entry, &source, &destination)?;
                migrate_ready_sentinel(&destination)?;
                mark_ready_if_complete(entry, &destination)?;
                if source_fully_copied(&source, &destination)? {
                    remove_path(&source)?;
                }
            }
            migrate_ready_sentinel(&destination)?;
            mark_ready_if_complete(entry, &destination)?;
        }
        let legacy_foundry = legacy_root.join("foundry-local");
        let foundry_destination = root.join("foundry-local");
        if legacy_foundry.exists() && legacy_foundry != foundry_destination {
            copy_dir_missing(&legacy_foundry, &foundry_destination).map_err(platform_error)?;
            if source_fully_copied(&legacy_foundry, &foundry_destination)? {
                remove_path(&legacy_foundry)?;
            }
        }
        for entry in self
            .catalog
            .entries()
            .iter()
            .filter(|entry| entry.target.runtime == LocalAsrRuntime::Foundry)
        {
            remove_marker_only_directory(&root.join(entry.target.model_id()))?;
        }
        Ok(())
    }

    pub fn relocate_root(&self, next_root: PathBuf) -> Result<(), BackendError> {
        let next = ModelStoreConfig::new(next_root)?.models_root_dir;
        if !self
            .active_downloads
            .lock()
            .expect("model download lock poisoned")
            .is_empty()
        {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "model downloads are still active",
            ));
        }
        let current = self.models_root_dir();
        if current != next {
            if current.join(MODEL_RELOCATION_JOURNAL).is_file() {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "a model relocation is waiting for restart",
                ));
            }
            if next.starts_with(&current) || current.starts_with(&next) {
                return Err(invalid("model roots cannot be nested inside one another"));
            }
            self.migrate_root_contents(&current, &next)?;
            write_relocation_journal(&next, &current)?;
            *self
                .models_root_dir
                .write()
                .expect("model root lock poisoned") = next;
        }
        Ok(())
    }

    pub fn rollback_relocation(&self, previous_root: PathBuf) -> Result<(), BackendError> {
        let current = self.models_root_dir();
        *self
            .models_root_dir
            .write()
            .expect("model root lock poisoned") = previous_root;
        let journal = current.join(MODEL_RELOCATION_JOURNAL);
        if journal.exists() {
            std::fs::remove_file(journal).map_err(platform_error)?;
        }
        Ok(())
    }

    pub fn finish_pending_relocation(&self) -> Result<(), BackendError> {
        let root = self.models_root_dir();
        let journal_path = root.join(MODEL_RELOCATION_JOURNAL);
        if !journal_path.is_file() {
            return Ok(());
        }
        let journal: RelocationJournal =
            serde_json::from_slice(&std::fs::read(&journal_path).map_err(platform_error)?)
                .map_err(|error| invalid(format!("invalid model relocation journal: {error}")))?;
        if journal.version != 1
            || !journal.source.is_absolute()
            || journal.source == root
            || journal.source.file_name().and_then(|name| name.to_str()) != Some("models")
        {
            return Err(invalid("unsafe model relocation journal"));
        }
        if journal.source.exists() {
            copy_dir_verified(&journal.source, &root)?;
            std::fs::remove_dir_all(&journal.source).map_err(platform_error)?;
        }
        std::fs::remove_file(journal_path).map_err(platform_error)
    }

    fn migrate_root_contents(&self, current: &Path, next: &Path) -> Result<(), BackendError> {
        if !current.is_dir() {
            std::fs::create_dir_all(next).map_err(platform_error)?;
            return Ok(());
        }
        std::fs::create_dir_all(next).map_err(platform_error)?;
        for entry in std::fs::read_dir(current).map_err(platform_error)? {
            let entry = entry.map_err(platform_error)?;
            let source = entry.path();
            let destination = next.join(entry.file_name());
            copy_dir_verified(&source, &destination)?;
        }
        Ok(())
    }

    pub fn delete_model(&self, target: &LocalAsrTarget) -> Result<(), BackendError> {
        validate_model_id(target.model_id())?;
        if self.is_native(target)? {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "native model deletion must use the runtime adapter",
            ));
        }
        if self.cancel_download(target)? {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "model download cancellation is pending",
            ));
        }
        let directory = self.model_dir(target)?;
        if directory.exists() {
            std::fs::remove_dir_all(directory).map_err(platform_error)?;
        }
        self.cleanup_incomplete(target)
    }
}

fn migrate_ready_sentinel(model_dir: &Path) -> Result<(), BackendError> {
    if !model_dir.is_dir() {
        return Ok(());
    }
    if model_dir.join(MODEL_READY_SENTINEL).is_file() {
        for legacy in [".openless-asr-ready", ".ready", "ready"] {
            let path = model_dir.join(legacy);
            if path.is_file() {
                std::fs::remove_file(path).map_err(platform_error)?;
            }
        }
        return Ok(());
    }
    for legacy in [".openless-asr-ready", ".ready", "ready"] {
        let source = model_dir.join(legacy);
        if source.is_file() {
            std::fs::rename(source, model_dir.join(MODEL_READY_SENTINEL))
                .map_err(platform_error)?;
            break;
        }
    }
    Ok(())
}

fn legacy_source_candidates(root: &Path, target: &LocalAsrTarget) -> Vec<PathBuf> {
    match target.runtime {
        LocalAsrRuntime::Generic => {
            let mut candidates = vec![root.join(target.model_id())];
            if target.model_id().starts_with("qwen3-asr-") {
                candidates.insert(0, root.join("qwen3-asr").join(target.model_id()));
            }
            candidates
        }
        LocalAsrRuntime::Foundry => vec![root.join("foundry-local")],
        LocalAsrRuntime::SherpaOnnx => vec![
            root.join("sherpa-onnx").join(target.model_id()),
            // Early 2.0 builds downloaded Sherpa aliases directly under the root.
            root.join(target.model_id()),
        ],
    }
}

fn migrate_legacy_q5(legacy_root: &Path, destination: &Path) -> Result<(), BackendError> {
    const FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
    let source = legacy_root.join("whisper-large-v3-turbo").join(FILE);
    let target = destination.join(FILE);
    if source.is_file() && !target.exists() {
        std::fs::create_dir_all(destination).map_err(platform_error)?;
        std::fs::copy(&source, &target).map_err(platform_error)?;
    }
    if source.is_file() && source_fully_copied(&source, &target)? {
        std::fs::remove_file(source).map_err(platform_error)?;
        let parent = legacy_root.join("whisper-large-v3-turbo");
        if parent
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
        {
            std::fs::remove_dir(parent).map_err(platform_error)?;
        }
    }
    Ok(())
}

fn mark_ready_if_complete(
    entry: &ModelCatalogEntry,
    destination: &Path,
) -> Result<(), BackendError> {
    if destination.join(MODEL_READY_SENTINEL).is_file() {
        return Ok(());
    }
    let complete = match &entry.selector {
        ModelFileSelector::Exact(files) => files
            .iter()
            .all(|file| destination.join(&file.local_path).is_file()),
        ModelFileSelector::Archive { required_paths, .. } => required_paths
            .iter()
            .all(|path| destination.join(path).is_file()),
        ModelFileSelector::QwenRepository | ModelFileSelector::Native => false,
    };
    if complete {
        std::fs::write(destination.join(MODEL_READY_SENTINEL), b"ready\n")
            .map_err(platform_error)?;
    }
    Ok(())
}

fn staging_dir_name(target: &LocalAsrTarget) -> String {
    match target.runtime {
        LocalAsrRuntime::Generic => format!(".{}.staging", target.model_id()),
        LocalAsrRuntime::Foundry => format!(".foundry-{}.staging", target.model_id()),
        LocalAsrRuntime::SherpaOnnx => format!(".sherpa-onnx-{}.staging", target.model_id()),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialIndex {
    version: u8,
    files: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelocationJournal {
    version: u8,
    source: PathBuf,
}

fn trusted_partial_bytes(
    staging: &Path,
    expected: Option<&BTreeMap<String, u64>>,
    max_file_bytes: u64,
    max_total_bytes: u64,
) -> Result<Option<u64>, BackendError> {
    let index_path = staging.join(MODEL_PARTIAL_INDEX);
    if !std::fs::symlink_metadata(&index_path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let partial = match std::fs::read(index_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PartialIndex>(&bytes).ok())
    {
        Some(partial) if partial.version == PARTIAL_INDEX_VERSION => partial,
        _ => return Ok(None),
    };
    let mut total = 0u64;
    for (path, offset) in &partial.files {
        if validate_model_path(path).is_err()
            || *offset > max_file_bytes
            || expected.is_some_and(|files| {
                files
                    .get(path)
                    .is_none_or(|expected_bytes| offset > expected_bytes)
            })
            || !std::fs::symlink_metadata(staging.join(path))
                .map(|metadata| metadata.file_type().is_file() && metadata.len() == *offset)
                .unwrap_or(false)
        {
            return Ok(None);
        }
        total = total.saturating_add(*offset);
        if total > max_total_bytes {
            return Ok(None);
        }
    }
    let mut staged_files = Vec::new();
    collect_relative_files(staging, staging, &mut staged_files).map_err(platform_error)?;
    if staged_files.into_iter().any(|relative| {
        relative != MODEL_PARTIAL_INDEX
            && relative != format!("{MODEL_PARTIAL_INDEX}.tmp")
            && !partial.files.contains_key(&relative)
    }) {
        return Ok(None);
    }
    Ok(Some(total))
}

fn restore_partial_index(
    staging: &Path,
    manifest: &ModelManifest,
) -> Result<PartialIndex, BackendError> {
    let index_path = staging.join(MODEL_PARTIAL_INDEX);
    let decoded = std::fs::read(&index_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PartialIndex>(&bytes).ok());
    let mut valid = decoded.is_some();
    let partial = decoded.unwrap_or_else(|| PartialIndex {
        version: PARTIAL_INDEX_VERSION,
        files: BTreeMap::new(),
    });
    valid &= partial.version == PARTIAL_INDEX_VERSION;
    let expected = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.size_bytes))
        .collect::<BTreeMap<_, _>>();
    for (path, offset) in &partial.files {
        valid &= validate_model_path(path).is_ok();
        valid &= expected
            .get(path.as_str())
            .is_some_and(|size| offset <= size);
        valid &= std::fs::metadata(staging.join(path))
            .map(|metadata| metadata.is_file() && metadata.len() == *offset)
            .unwrap_or(false);
    }
    let mut staged_files = Vec::new();
    collect_relative_files(staging, staging, &mut staged_files).map_err(platform_error)?;
    for relative in staged_files {
        if relative == MODEL_PARTIAL_INDEX || relative == format!("{MODEL_PARTIAL_INDEX}.tmp") {
            continue;
        }
        valid &= partial.files.contains_key(&relative);
    }
    if valid {
        return Ok(partial);
    }
    std::fs::remove_dir_all(staging).map_err(platform_error)?;
    std::fs::create_dir_all(staging).map_err(platform_error)?;
    Ok(PartialIndex {
        version: PARTIAL_INDEX_VERSION,
        files: BTreeMap::new(),
    })
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<String>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_relative_files(root, &entry.path(), files)?;
        } else {
            files.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("walked path stays below root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn directory_size(path: &Path) -> std::io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    std::fs::read_dir(path)?.try_fold(0u64, |total, entry| {
        let size = directory_size(&entry?.path())?;
        Ok(total.saturating_add(size))
    })
}

const MODEL_CARD_DESCRIPTION_CHARS: usize = 280;

fn first_readme_paragraph(markdown: &str) -> String {
    for block in markdown.split("\n\n") {
        let block = block.trim();
        if block.is_empty() || block.starts_with("---") {
            continue;
        }
        let mut parts = Vec::new();
        for raw_line in block.lines() {
            let line = raw_line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with('!')
                || line.starts_with('|')
                || line.starts_with("---")
                || line.starts_with('<')
                || is_link_only_line(line)
            {
                continue;
            }
            let stripped = strip_markdown_inline(line);
            if !stripped.is_empty() {
                parts.push(stripped);
            }
        }
        if !parts.is_empty() {
            return truncate_description(&parts.join(" "));
        }
    }
    String::new()
}

fn is_link_only_line(line: &str) -> bool {
    if line.contains("img.shields.io") || line.trim_start().starts_with("[![") {
        return true;
    }
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        if !rest[..open].chars().all(is_markdown_link_separator) {
            return false;
        }
        let tail = &rest[open + 1..];
        let Some(close) = tail.find("](") else {
            return false;
        };
        let after = &tail[close + 2..];
        let Some(end) = after.find(')') else {
            return false;
        };
        rest = &after[end + 1..];
    }
    rest.chars().all(is_markdown_link_separator)
}

fn is_markdown_link_separator(character: char) -> bool {
    character.is_whitespace() || matches!(character, '|' | ',' | '·' | '、')
}

fn strip_markdown_inline(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        output.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        if let Some(close) = tail.find("](") {
            let after = &tail[close + 2..];
            if let Some(end) = after.find(')') {
                let image = output.ends_with('!');
                if image {
                    output.pop();
                } else {
                    output.push_str(tail[..close].trim());
                }
                rest = &after[end + 1..];
                continue;
            }
        }
        output.push('[');
        rest = tail;
    }
    output.push_str(rest);
    output
        .replace("**", "")
        .replace(['`', '*', '_'], "")
        .trim()
        .to_string()
}

fn truncate_description(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MODEL_CARD_DESCRIPTION_CHARS {
        return text.to_string();
    }
    format!(
        "{}…",
        text.chars()
            .take(MODEL_CARD_DESCRIPTION_CHARS)
            .collect::<String>()
    )
}

fn write_partial_index(staging: &Path, partial: &PartialIndex) -> Result<(), BackendError> {
    let path = staging.join(MODEL_PARTIAL_INDEX);
    let temporary = staging.join(format!("{MODEL_PARTIAL_INDEX}.tmp"));
    let bytes = serde_json::to_vec(partial)
        .map_err(|error| BackendError::new(BackendErrorCode::Internal, error.to_string()))?;
    let mut file = std::fs::File::create(&temporary).map_err(platform_error)?;
    file.write_all(&bytes).map_err(platform_error)?;
    file.sync_all().map_err(platform_error)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(platform_error)?;
    }
    std::fs::rename(temporary, path).map_err(platform_error)
}

fn copy_dir_missing(source: &Path, destination: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_dir_missing(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if !destination.exists() {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

fn copy_legacy_entry_missing(
    entry: &ModelCatalogEntry,
    source: &Path,
    destination: &Path,
) -> Result<(), BackendError> {
    match &entry.selector {
        ModelFileSelector::Exact(files) => {
            std::fs::create_dir_all(destination).map_err(platform_error)?;
            for file in files {
                let source_file = source.join(&file.local_path);
                let destination_file = destination.join(&file.local_path);
                if source_file.is_file() && !destination_file.exists() {
                    if let Some(parent) = destination_file.parent() {
                        std::fs::create_dir_all(parent).map_err(platform_error)?;
                    }
                    std::fs::copy(source_file, destination_file).map_err(platform_error)?;
                }
            }
            for sentinel in [
                MODEL_READY_SENTINEL,
                ".openless-asr-ready",
                ".ready",
                "ready",
            ] {
                let source_file = source.join(sentinel);
                let destination_file = destination.join(sentinel);
                if source_file.is_file() && !destination_file.exists() {
                    std::fs::copy(source_file, destination_file).map_err(platform_error)?;
                }
            }
            Ok(())
        }
        ModelFileSelector::QwenRepository | ModelFileSelector::Archive { .. } => {
            copy_dir_missing(source, destination).map_err(platform_error)
        }
        ModelFileSelector::Native => Ok(()),
    }
}

fn copy_dir_verified(source: &Path, destination: &Path) -> Result<(), BackendError> {
    if source.is_dir() {
        if destination.exists() && !destination.is_dir() {
            return Err(invalid(format!(
                "model relocation conflicts with file {}",
                destination.display()
            )));
        }
        std::fs::create_dir_all(destination).map_err(platform_error)?;
        for entry in std::fs::read_dir(source).map_err(platform_error)? {
            let entry = entry.map_err(platform_error)?;
            copy_dir_verified(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if destination.exists() {
        if !destination.is_file()
            || std::fs::metadata(source).map_err(platform_error)?.len()
                != std::fs::metadata(destination)
                    .map_err(platform_error)?
                    .len()
            || sha256_file(source)? != sha256_file(destination)?
        {
            return Err(invalid(format!(
                "model relocation found conflicting file {}",
                destination.display()
            )));
        }
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(platform_error)?;
        }
        std::fs::copy(source, destination).map_err(platform_error)?;
    }
    Ok(())
}

fn source_fully_copied(source: &Path, destination: &Path) -> Result<bool, BackendError> {
    if source.is_dir() {
        if !destination.is_dir() {
            return Ok(false);
        }
        for entry in std::fs::read_dir(source).map_err(platform_error)? {
            let entry = entry.map_err(platform_error)?;
            let source_path = entry.path();
            let mut destination_path = destination.join(entry.file_name());
            if source_path.is_file()
                && matches!(
                    entry.file_name().to_str(),
                    Some(".openless-asr-ready" | ".ready" | "ready")
                )
                && destination.join(MODEL_READY_SENTINEL).is_file()
            {
                destination_path = destination.join(MODEL_READY_SENTINEL);
            }
            if !source_fully_copied(&source_path, &destination_path)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if !destination.is_file() {
        return Ok(false);
    }
    Ok(std::fs::metadata(source).map_err(platform_error)?.len()
        == std::fs::metadata(destination)
            .map_err(platform_error)?
            .len()
        && sha256_file(source)? == sha256_file(destination)?)
}

fn remove_path(path: &Path) -> Result<(), BackendError> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(platform_error)
    } else if path.exists() {
        std::fs::remove_file(path).map_err(platform_error)
    } else {
        Ok(())
    }
}

fn remove_marker_only_directory(path: &Path) -> Result<(), BackendError> {
    if !path.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(path)
        .map_err(platform_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(platform_error)?;
    if !entries.is_empty()
        && entries.iter().all(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && matches!(
                    entry.file_name().to_str(),
                    Some(MODEL_READY_SENTINEL | ".openless-asr-ready" | ".ready" | "ready")
                )
        })
    {
        std::fs::remove_dir_all(path).map_err(platform_error)?;
    }
    Ok(())
}

fn write_relocation_journal(root: &Path, source: &Path) -> Result<(), BackendError> {
    let path = root.join(MODEL_RELOCATION_JOURNAL);
    let temporary = root.join(format!("{MODEL_RELOCATION_JOURNAL}.tmp"));
    let bytes = serde_json::to_vec(&RelocationJournal {
        version: 1,
        source: source.to_path_buf(),
    })
    .map_err(|error| BackendError::new(BackendErrorCode::Internal, error.to_string()))?;
    let mut file = std::fs::File::create(&temporary).map_err(platform_error)?;
    file.write_all(&bytes).map_err(platform_error)?;
    file.sync_all().map_err(platform_error)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(platform_error)?;
    }
    std::fs::rename(temporary, path).map_err(platform_error)
}

fn commit_staging(staging: &Path, destination: &Path) -> Result<(), BackendError> {
    let backup = destination.with_file_name(format!(
        ".{}.previous-{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model"),
        uuid::Uuid::new_v4().simple()
    ));
    let had_previous = destination.exists();
    if had_previous {
        std::fs::rename(destination, &backup).map_err(platform_error)?;
    }
    if let Err(error) = std::fs::rename(staging, destination) {
        if had_previous {
            let _ = std::fs::rename(&backup, destination);
        }
        return Err(platform_error(error));
    }
    if had_previous {
        std::fs::remove_dir_all(backup).map_err(platform_error)?;
    }
    Ok(())
}

fn validate_range_response(
    response: &ModelTransportResponse,
    start: u64,
    end: u64,
    total: u64,
) -> Result<(), BackendError> {
    let received = response.bytes.len() as u64;
    if response
        .metadata
        .content_length
        .is_some_and(|length| length != received)
    {
        return Err(invalid(
            "model response Content-Length does not match its body",
        ));
    }
    match response.status {
        200 if start == 0 && received == total => Ok(()),
        206 if received == end - start + 1 => match &response.metadata.content_range {
            Some(range) if range.start == start && range.end == end && range.total == total => {
                Ok(())
            }
            _ => Err(invalid(
                "model response Content-Range does not match the request",
            )),
        },
        status => Err(BackendError::new(
            BackendErrorCode::Provider,
            format!("unexpected model HTTP status or length: {status}"),
        )),
    }
}

fn sha256_file(path: &Path) -> Result<String, BackendError> {
    let mut file = std::fs::File::open(path).map_err(platform_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(platform_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn validate_model_id(value: &str) -> Result<(), BackendError> {
    if value.is_empty()
        || value.len() > 128
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value == "."
        || value == ".."
    {
        return Err(invalid("invalid model id"));
    }
    Ok(())
}

pub fn validate_model_path(value: &str) -> Result<(), BackendError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || value.contains('\0')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("model manifest contains an unsafe path"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    lfs: Option<HfLfs>,
}

#[derive(Debug, Deserialize)]
struct HfLfs {
    oid: String,
    size: u64,
}

pub fn parse_hf_tree_page(
    repository: &str,
    model_id: &str,
    entries: &[serde_json::Value],
) -> Result<Vec<ModelFile>, BackendError> {
    let catalog = ModelCatalog::standard();
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.target.model_id() == model_id && entry.repository == repository)
        .ok_or_else(|| invalid("model is not present in the Core catalog"))?;
    parse_hf_tree_page_for_entry(entry, entries, "https://huggingface.co")
}

/// Merge paginated Hugging Face tree responses while rejecting duplicate paths
/// across page boundaries.
pub fn merge_hf_tree_pages(
    repository: &str,
    model_id: &str,
    pages: &[Vec<serde_json::Value>],
) -> Result<Vec<ModelFile>, BackendError> {
    merge_hf_tree_pages_with_base(repository, model_id, pages, "https://huggingface.co")
}

pub fn merge_hf_tree_pages_with_base(
    repository: &str,
    model_id: &str,
    pages: &[Vec<serde_json::Value>],
    base_url: &str,
) -> Result<Vec<ModelFile>, BackendError> {
    let catalog = ModelCatalog::standard();
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.target.model_id() == model_id && entry.repository == repository)
        .ok_or_else(|| invalid("model is not present in the Core catalog"))?;
    merge_hf_tree_pages_for_entry(entry, pages, base_url)
}

fn manifest_from_hf_pages(
    entry: &ModelCatalogEntry,
    pages: &[Vec<serde_json::Value>],
    base_url: &str,
    max_total_bytes: u64,
) -> Result<ModelManifest, BackendError> {
    let files = merge_hf_tree_pages_for_entry(entry, pages, base_url)?;
    let manifest = ModelManifest::new(entry.target.clone(), entry.repository.clone(), files)?;
    if manifest.total_bytes > max_total_bytes {
        return Err(invalid("model exceeds the configured total size limit"));
    }
    Ok(manifest)
}

fn merge_hf_tree_pages_for_entry(
    entry: &ModelCatalogEntry,
    pages: &[Vec<serde_json::Value>],
    base_url: &str,
) -> Result<Vec<ModelFile>, BackendError> {
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for page in pages {
        for file in parse_hf_tree_page_for_entry(entry, page, base_url)? {
            if !seen.insert(file.path.clone()) {
                return Err(invalid("duplicate file across Hugging Face tree pages"));
            }
            files.push(file);
        }
    }
    if files.is_empty() {
        return Err(invalid(
            "Hugging Face tree returned no selected model files",
        ));
    }
    if let ModelFileSelector::Exact(expected) = &entry.selector {
        let actual = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<BTreeSet<_>>();
        let missing = expected
            .iter()
            .find(|file| !actual.contains(file.local_path.as_str()));
        if let Some(missing) = missing {
            return Err(invalid(format!(
                "Hugging Face tree is missing required model file {}",
                missing.remote_path
            )));
        }
    }
    Ok(files)
}

fn parse_hf_tree_page_for_entry(
    catalog_entry: &ModelCatalogEntry,
    entries: &[serde_json::Value],
    base_url: &str,
) -> Result<Vec<ModelFile>, BackendError> {
    validate_model_id(catalog_entry.target.model_id())?;
    validate_repository(&catalog_entry.repository)?;
    let base_url = base_url.trim_end_matches('/');
    validate_model_url(&format!("{base_url}/"))?;
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for value in entries {
        let entry: HfTreeEntry = serde_json::from_value(value.clone())
            .map_err(|_| invalid("invalid Hugging Face tree entry"))?;
        if entry.entry_type != "file" {
            continue;
        }
        validate_model_path(&entry.path)?;
        let Some(local_path) = catalog_entry.selector.local_path(&entry.path) else {
            continue;
        };
        validate_model_path(&local_path)?;
        if !seen.insert(local_path.clone()) {
            return Err(invalid("duplicate selected file in model tree"));
        }
        let (size_bytes, sha256) = match entry.lfs {
            Some(lfs) => (lfs.size, Some(parse_lfs_sha256(&lfs.oid)?)),
            None => (
                entry
                    .size
                    .ok_or_else(|| invalid("model file size is missing"))?,
                None,
            ),
        };
        if size_bytes == 0 || size_bytes > DEFAULT_MODEL_MAX_FILE_BYTES {
            return Err(invalid(
                "model file size is invalid or exceeds the configured limit",
            ));
        }
        files.push(ModelFile {
            url: format!(
                "{base_url}/{}/resolve/main/{}",
                catalog_entry.repository, entry.path
            ),
            path: local_path,
            size_bytes,
            sha256,
        });
    }
    Ok(files)
}

fn parse_lfs_sha256(oid: &str) -> Result<String, BackendError> {
    let digest = oid
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid("unsupported Hugging Face LFS oid"))?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("invalid Hugging Face LFS sha256 oid"));
    }
    Ok(digest.to_ascii_lowercase())
}

fn qwen_model_file(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "added_tokens.json",
        "chat_template.jinja",
        "config.json",
        "generation_config.json",
        "merges.txt",
        "model.safetensors",
        "model.safetensors.index.json",
        "preprocessor_config.json",
        "special_tokens_map.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "vocab.json",
    ];
    let lower = path.to_ascii_lowercase();
    EXACT.contains(&lower.as_str())
        || (lower.starts_with("model-") && lower.ends_with(".safetensors"))
}

fn next_hf_link(
    header: &str,
    current_url: &str,
    base_url: &str,
) -> Result<Option<String>, BackendError> {
    let base = url::Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
        .map_err(|_| invalid("invalid Hugging Face base URL"))?;
    let current =
        url::Url::parse(current_url).map_err(|_| invalid("invalid Hugging Face pagination URL"))?;
    for value in header.split(',') {
        let mut parts = value.trim().split(';');
        let target = parts.next().unwrap_or_default().trim();
        let is_next = parts.any(|part| {
            part.trim()
                .strip_prefix("rel=")
                .map(|rel| rel.trim_matches('"') == "next")
                .unwrap_or(false)
        });
        if !is_next {
            continue;
        }
        let target = target
            .strip_prefix('<')
            .and_then(|value| value.strip_suffix('>'))
            .ok_or_else(|| invalid("invalid Hugging Face Link header"))?;
        let next = current
            .join(target)
            .map_err(|_| invalid("invalid Hugging Face next-page URL"))?;
        if next.scheme() != base.scheme()
            || next.host_str() != base.host_str()
            || next.port_or_known_default() != base.port_or_known_default()
        {
            return Err(invalid("Hugging Face pagination changed origin"));
        }
        return Ok(Some(next.into()));
    }
    Ok(None)
}

fn validate_repository(repository: &str) -> Result<(), BackendError> {
    if repository.trim().is_empty()
        || repository.contains('\\')
        || repository.contains("..")
        || repository.starts_with('/')
        || repository.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/'))
        })
    {
        return Err(invalid("model repository is invalid"));
    }
    Ok(())
}

fn expand_tar_bz2_archive(
    staging: &Path,
    spec: &ModelArchiveSpec,
    max_file_bytes: u64,
    max_total_bytes: u64,
) -> Result<(), BackendError> {
    validate_model_path(&spec.file_path)?;
    validate_model_path(&spec.root_dir)?;
    let extraction = staging.join(format!(
        ".archive-extract-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&extraction).map_err(platform_error)?;
    let mut extraction_guard = ArchiveStagingGuard {
        path: extraction.clone(),
        committed: false,
    };
    let archive_file =
        std::fs::File::open(staging.join(&spec.file_path)).map_err(platform_error)?;
    let decoder = bzip2::read::BzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for entry in archive.entries().map_err(platform_error)? {
        let mut entry = entry.map_err(platform_error)?;
        let path = entry.path().map_err(platform_error)?.into_owned();
        let raw = path.to_string_lossy().replace('\\', "/");
        validate_model_path(&raw)?;
        let relative = path
            .strip_prefix(&spec.root_dir)
            .map_err(|_| invalid("model archive entry is outside its declared root"))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let relative = relative.to_string_lossy().replace('\\', "/");
        validate_model_path(&relative)?;
        if matches!(
            relative.as_str(),
            MODEL_READY_SENTINEL | MODEL_PARTIAL_INDEX
        ) || !seen.insert(relative.clone())
        {
            return Err(invalid(
                "model archive contains a reserved or duplicate path",
            ));
        }
        let output = extraction.join(&relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            std::fs::create_dir_all(&output).map_err(platform_error)?;
            continue;
        }
        if !kind.is_file() {
            return Err(invalid(
                "model archive links and special files are not supported",
            ));
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("model archive size overflowed"))?;
        if size > max_file_bytes || total > max_total_bytes {
            return Err(invalid("model archive exceeds the configured size limit"));
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(platform_error)?;
        }
        entry.unpack(&output).map_err(platform_error)?;
    }
    for required in &spec.required_paths {
        validate_model_path(required)?;
        if !extraction.join(required).is_file() {
            return Err(invalid(format!(
                "model archive is missing required path {required}"
            )));
        }
    }
    std::fs::remove_file(staging.join(&spec.file_path)).map_err(platform_error)?;
    for entry in std::fs::read_dir(&extraction).map_err(platform_error)? {
        let entry = entry.map_err(platform_error)?;
        std::fs::rename(entry.path(), staging.join(entry.file_name())).map_err(platform_error)?;
    }
    std::fs::remove_dir(&extraction).map_err(platform_error)?;
    extraction_guard.committed = true;
    Ok(())
}

pub fn extract_archive_safely(
    bytes: &[u8],
    destination: &Path,
    max_file_bytes: u64,
) -> Result<(), BackendError> {
    if !destination.is_absolute() {
        return Err(invalid("archive destination must be absolute"));
    }
    let staging = destination.with_file_name(format!(
        ".{}.archive-staging-{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model"),
        uuid::Uuid::new_v4().simple()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(platform_error)?;
    }
    std::fs::create_dir_all(&staging).map_err(platform_error)?;
    let mut staging_guard = ArchiveStagingGuard {
        path: staging.clone(),
        committed: false,
    };
    let reader = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(reader) {
        Ok(archive) => archive,
        Err(error) => {
            return Err(invalid(error.to_string()));
        }
    };
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| invalid(error.to_string()))?;
        validate_model_path(entry.name())?;
        if matches!(entry.name(), MODEL_READY_SENTINEL | MODEL_PARTIAL_INDEX)
            || !seen.insert(entry.name().to_string())
        {
            return Err(invalid("archive contains a reserved or duplicate path"));
        }
        if entry.is_dir() {
            continue;
        }
        if entry.size() > max_file_bytes {
            return Err(invalid("archive entry exceeds the configured size limit"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| invalid("archive size overflowed"))?;
        if total > DEFAULT_MODEL_MAX_TOTAL_BYTES {
            return Err(invalid("archive exceeds the configured total size limit"));
        }
        let output = staging.join(entry.name());
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(platform_error)?;
        }
        let mut file = std::fs::File::create(output).map_err(platform_error)?;
        std::io::copy(&mut entry, &mut file).map_err(platform_error)?;
    }
    commit_staging(&staging, destination)?;
    staging_guard.committed = true;
    Ok(())
}

struct ArchiveStagingGuard {
    path: PathBuf,
    committed: bool,
}

struct ActiveDownloadGuard {
    active: Arc<Mutex<HashMap<LocalAsrTarget, Arc<AtomicBool>>>>,
    target: LocalAsrTarget,
    cancelled: Arc<AtomicBool>,
}

impl Drop for ActiveDownloadGuard {
    fn drop(&mut self) {
        let mut active = self.active.lock().expect("model download lock poisoned");
        if active
            .get(&self.target)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            active.remove(&self.target);
        }
    }
}

impl Drop for ArchiveStagingGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn invalid(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, message)
}

pub fn validate_model_url(value: &str) -> Result<(), BackendError> {
    let parsed = url::Url::parse(value).map_err(|_| invalid("model file URL is invalid"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(invalid("model file URL must use http or https"));
    }
    Ok(())
}
fn archive_file_name(value: &str) -> Option<String> {
    url::Url::parse(value)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}
fn platform_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, error.to_string())
}
fn cancelled_error() -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, "model operation cancelled")
}

async fn wait_until_cancelled(cancelled: Arc<AtomicBool>) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct FakeTransport {
        calls: Arc<AtomicUsize>,
        body: Vec<u8>,
        ignore_range: bool,
    }

    struct BlockingTransport {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
        body: Vec<u8>,
    }

    struct BlockingMetadataTransport {
        entered: Arc<tokio::sync::Notify>,
    }

    struct ModelCardTransport {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl ModelTransport for ModelCardTransport {
        fn request(
            &self,
            request: ModelTransportRequest,
        ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
            self.calls.lock().unwrap().push(request.url.clone());
            let bytes = if request.url.contains("/api/models/") {
                br#"{"downloads":7,"likes":3,"cardData":{}}"#.to_vec()
            } else {
                b"---\nlicense: apache-2.0\n---\n\n# Model\n\n[English](en) | [Chinese](zh)\n\nThe **first** [useful paragraph](https://example.test).\n\n## Details"
                    .to_vec()
            };
            Box::pin(async move {
                Ok(ModelTransportResponse {
                    status: 200,
                    metadata: ModelHttpMetadata {
                        content_length: Some(bytes.len() as u64),
                        ..ModelHttpMetadata::default()
                    },
                    bytes,
                })
            })
        }
    }

    impl ModelTransport for BlockingMetadataTransport {
        fn request(
            &self,
            _: ModelTransportRequest,
        ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
            let entered = Arc::clone(&self.entered);
            Box::pin(async move {
                entered.notify_one();
                std::future::pending().await
            })
        }
    }

    impl ModelTransport for BlockingTransport {
        fn request(
            &self,
            request: ModelTransportRequest,
        ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
            let entered = Arc::clone(&self.entered);
            let release = Arc::clone(&self.release);
            let body = self.body.clone();
            Box::pin(async move {
                entered.notify_one();
                let permit = release.acquire_owned().await.unwrap();
                permit.forget();
                let (start, end) = request.range.unwrap();
                let bytes = body[start as usize..=end as usize].to_vec();
                Ok(ModelTransportResponse {
                    status: 206,
                    metadata: ModelHttpMetadata {
                        content_length: Some(bytes.len() as u64),
                        content_range: Some(ModelContentRange {
                            start,
                            end,
                            total: body.len() as u64,
                        }),
                        link: None,
                    },
                    bytes,
                })
            })
        }
    }
    impl ModelTransport for FakeTransport {
        fn request(
            &self,
            request: ModelTransportRequest,
        ) -> BoxFuture<'static, Result<ModelTransportResponse, BackendError>> {
            let calls = Arc::clone(&self.calls);
            let body = self.body.clone();
            let ignore_range = self.ignore_range;
            Box::pin(async move {
                calls.fetch_add(1, Ordering::Relaxed);
                let total = body.len() as u64;
                let (status, bytes, content_range) = match request.range {
                    Some(_) if ignore_range => (200, body, None),
                    Some((start, end)) => {
                        let bytes =
                            body[start as usize..=(end as usize).min(body.len() - 1)].to_vec();
                        (206, bytes, Some(ModelContentRange { start, end, total }))
                    }
                    None => (200, body, None),
                };
                assert!(bytes.len() as u64 <= request.max_response_bytes);
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

    #[test]
    fn path_validation_rejects_traversal_and_absolute_names() {
        assert!(validate_model_path("weights/model.bin").is_ok());
        assert!(validate_model_path("../model.bin").is_err());
        assert!(validate_model_path("/tmp/model.bin").is_err());
        assert!(validate_model_path("C:\\\\model.bin").is_err());
    }

    #[test]
    fn hf_tree_uses_catalog_selector_and_lfs_checksum() {
        let checksum = "a".repeat(64);
        let entries = vec![
            serde_json::json!({"type":"directory","path":"nested"}),
            serde_json::json!({"type":"file","path":"ggml-base.bin","size":3}),
            serde_json::json!({"type":"file","path":"ggml-small.bin","size":3,"lfs":{"oid":format!("sha256:{checksum}"),"size":4}}),
        ];
        let files = parse_hf_tree_page("ggerganov/whisper.cpp", "whisper-small", &entries).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "ggml-small.bin");
        assert_eq!(files[0].size_bytes, 4);
        assert_eq!(files[0].sha256.as_deref(), Some(checksum.as_str()));
        assert!(parse_hf_tree_page(
            "ggerganov/whisper.cpp",
            "whisper-small",
            &[serde_json::json!({"type":"file","path":"../x"})]
        )
        .is_err());
    }

    #[test]
    fn hf_link_pagination_accepts_same_origin_and_rejects_redirected_origin() {
        let current = "https://huggingface.co/api/models/org/model/tree/main?limit=1000";
        assert_eq!(
            next_hf_link(
                "<https://huggingface.co/api/models/org/model/tree/main?cursor=next>; rel=\"next\"",
                current,
                "https://huggingface.co",
            )
            .unwrap()
            .as_deref(),
            Some("https://huggingface.co/api/models/org/model/tree/main?cursor=next")
        );
        assert!(next_hf_link(
            "<https://evil.example/tree?cursor=next>; rel=\"next\"",
            current,
            "https://huggingface.co",
        )
        .is_err());
    }

    #[test]
    fn range_contract_accepts_complete_200_and_exact_206_only() {
        let complete = ModelTransportResponse {
            status: 200,
            bytes: vec![0; 4],
            metadata: ModelHttpMetadata {
                content_length: Some(4),
                ..ModelHttpMetadata::default()
            },
        };
        assert!(validate_range_response(&complete, 0, 3, 4).is_ok());
        let partial = ModelTransportResponse {
            status: 206,
            bytes: vec![0; 2],
            metadata: ModelHttpMetadata {
                content_length: Some(2),
                content_range: Some(ModelContentRange {
                    start: 2,
                    end: 3,
                    total: 4,
                }),
                link: None,
            },
        };
        assert!(validate_range_response(&partial, 2, 3, 4).is_ok());
        assert!(validate_range_response(&partial, 0, 1, 4).is_err());
    }

    #[tokio::test]
    async fn download_resumes_ranges_and_writes_ready_sentinel() {
        let root =
            std::env::temp_dir().join(format!("openless-model-store-{}", uuid::Uuid::new_v4()));
        let body = b"0123456789".to_vec();
        let calls = Arc::new(AtomicUsize::new(0));
        let transport = Arc::new(FakeTransport {
            calls: Arc::clone(&calls),
            body: body.clone(),
            ignore_range: false,
        });
        let mut config = ModelStoreConfig::new(root.clone()).unwrap();
        config.chunk_size_bytes = 4;
        let store = ModelStore::with_transport(config, transport);
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        let manifest = ModelManifest::new(
            target.clone(),
            "org/demo",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: body.len() as u64,
                sha256: Some(format!("{:x}", Sha256::digest(&body))),
            }],
        )
        .unwrap();
        let staging = root.join(staging_dir_name(&target));
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("weights.bin"), &body[..4]).unwrap();
        std::fs::write(
            staging.join(MODEL_PARTIAL_INDEX),
            br#"{"version":1,"files":{"weights.bin":4}}"#,
        )
        .unwrap();
        let status = store.download(manifest.clone()).await.unwrap();
        assert!(status.ready);
        assert_eq!(
            std::fs::read(root.join("qwen3-asr-0.6b/weights.bin")).unwrap(),
            body
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_list_restores_trusted_partial_bytes_and_cleans_bad_indexes() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-list-partial-{}",
            uuid::Uuid::new_v4()
        ));
        let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        let staging = root.join(staging_dir_name(&target));
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("weights.bin"), b"1234").unwrap();
        std::fs::write(
            staging.join(MODEL_PARTIAL_INDEX),
            br#"{"version":1,"files":{"weights.bin":4}}"#,
        )
        .unwrap();

        let models = store.list_models(LocalAsrRuntime::Generic).unwrap();
        assert_eq!(
            models
                .iter()
                .find(|model| model.target == target)
                .unwrap()
                .downloaded_bytes,
            4
        );
        let manifest = ModelManifest::new(
            target.clone(),
            "org/demo",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: 8,
                sha256: None,
            }],
        )
        .unwrap();
        assert_eq!(store.status(&manifest).unwrap().downloaded_bytes, 4);

        std::fs::write(
            staging.join(MODEL_PARTIAL_INDEX),
            br#"{"version":1,"files":{"../escape":4}}"#,
        )
        .unwrap();
        let models = store.list_models(LocalAsrRuntime::Generic).unwrap();
        assert_eq!(
            models
                .iter()
                .find(|model| model.target == target)
                .unwrap()
                .downloaded_bytes,
            0
        );
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn model_card_falls_back_to_the_first_useful_readme_paragraph() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-card-readme-{}",
            uuid::Uuid::new_v4()
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let store = ModelStore::with_transport(
            ModelStoreConfig::new(root.clone()).unwrap(),
            Arc::new(ModelCardTransport {
                calls: Arc::clone(&calls),
            }),
        );

        let card = store
            .fetch_hf_model_card(
                "qwen3-asr-0.6b",
                "Qwen/Qwen3-ASR-0.6B",
                "https://huggingface.co",
            )
            .await
            .unwrap();

        assert_eq!(card.downloads, 7);
        assert_eq!(card.likes, 3);
        assert_eq!(card.description, "The first useful paragraph.");
        assert_eq!(calls.lock().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancellation_during_the_final_range_never_commits_ready() {
        let root =
            std::env::temp_dir().join(format!("openless-model-cancel-{}", uuid::Uuid::new_v4()));
        let body = b"0123".to_vec();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let store = Arc::new(ModelStore::with_transport(
            ModelStoreConfig::new(root.clone()).unwrap(),
            Arc::new(BlockingTransport {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                body: body.clone(),
            }),
        ));
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-1.7b").unwrap();
        let manifest = ModelManifest::new(
            target.clone(),
            "org/cancelled",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: body.len() as u64,
                sha256: Some(format!("{:x}", Sha256::digest(&body))),
            }],
        )
        .unwrap();
        let task = tokio::spawn({
            let store = Arc::clone(&store);
            async move { store.download(manifest).await }
        });
        entered.notified().await;
        assert!(store.cancel_download(&target).unwrap());
        release.add_permits(1);
        assert_eq!(
            task.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(!root
            .join("qwen3-asr-1.7b")
            .join(MODEL_READY_SENTINEL)
            .exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancel_all_waits_until_active_downloads_reach_terminal_state() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-cancel-all-{}",
            uuid::Uuid::new_v4()
        ));
        let body = b"0123".to_vec();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let store = Arc::new(ModelStore::with_transport(
            ModelStoreConfig::new(root.clone()).unwrap(),
            Arc::new(BlockingTransport {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                body: body.clone(),
            }),
        ));
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        let manifest = ModelManifest::new(
            target,
            "fixture",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: body.len() as u64,
                sha256: None,
            }],
        )
        .unwrap();
        let download = tokio::spawn({
            let store = Arc::clone(&store);
            async move { store.download(manifest).await }
        });
        entered.notified().await;
        let cancel = tokio::spawn({
            let store = Arc::clone(&store);
            async move { store.cancel_all_downloads_and_wait().await }
        });
        cancel.await.unwrap().unwrap();
        assert_eq!(
            download.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancellation_covers_the_manifest_request_before_file_downloads_start() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-manifest-cancel-{}",
            uuid::Uuid::new_v4()
        ));
        let entered = Arc::new(tokio::sync::Notify::new());
        let store = Arc::new(ModelStore::with_transport(
            ModelStoreConfig::new(root.clone()).unwrap(),
            Arc::new(BlockingMetadataTransport {
                entered: Arc::clone(&entered),
            }),
        ));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&progress);
        store.set_progress_sink(Arc::new(move |event| {
            captured.lock().unwrap().push(event);
        }));
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-small").unwrap();
        let download = tokio::spawn({
            let store = Arc::clone(&store);
            let target = target.clone();
            async move {
                store
                    .download_target(target, crate::LocalAsrMirror::Huggingface)
                    .await
            }
        });
        entered.notified().await;

        assert!(store.cancel_download(&target).unwrap());
        assert_eq!(
            download.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        let terminal = progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(terminal.phase, ModelDownloadPhase::Cancelled);
        assert_eq!(terminal.runtime, LocalAsrRuntime::Generic);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_partial_index_cleans_untrusted_staging_state() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-corrupt-partial-{}",
            uuid::Uuid::new_v4()
        ));
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        let staging = root.join(staging_dir_name(&target));
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("weights.bin"), b"12").unwrap();
        std::fs::write(
            staging.join(MODEL_PARTIAL_INDEX),
            br#"{"version":1,"files":{"weights.bin":3}}"#,
        )
        .unwrap();
        let manifest = ModelManifest::new(
            target,
            "org/demo",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: 4,
                sha256: None,
            }],
        )
        .unwrap();

        let partial = restore_partial_index(&staging, &manifest).unwrap();
        assert!(partial.files.is_empty());
        assert!(!staging.join("weights.bin").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn checksum_failure_never_commits_the_model() {
        let root =
            std::env::temp_dir().join(format!("openless-model-checksum-{}", uuid::Uuid::new_v4()));
        let body = b"wrong".to_vec();
        let store = ModelStore::with_transport(
            ModelStoreConfig::new(root.clone()).unwrap(),
            Arc::new(FakeTransport {
                calls: Arc::new(AtomicUsize::new(0)),
                body: body.clone(),
                ignore_range: false,
            }),
        );
        let progress = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&progress);
        store.set_progress_sink(Arc::new(move |event| {
            captured.lock().unwrap().push(event);
        }));
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-base").unwrap();
        let manifest = ModelManifest::new(
            target,
            "org/checksum",
            vec![ModelFile {
                path: "weights.bin".into(),
                url: "https://example.test/weights.bin".into(),
                size_bytes: body.len() as u64,
                sha256: Some("0".repeat(64)),
            }],
        )
        .unwrap();

        assert!(store
            .download(manifest)
            .await
            .unwrap_err()
            .message
            .contains("checksum"));
        assert!(!root.join("whisper-base").exists());
        let terminal = progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(terminal.phase, ModelDownloadPhase::Failed);
        assert!(terminal.error.unwrap().contains("checksum"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_migration_merges_missing_files_and_preserves_destination() {
        let root =
            std::env::temp_dir().join(format!("openless-model-migrate-{}", uuid::Uuid::new_v4()));
        let current = root.join("current");
        let legacy = root.join("legacy");
        std::fs::create_dir_all(current.join("whisper-base")).unwrap();
        std::fs::create_dir_all(legacy.join("whisper-base")).unwrap();
        std::fs::write(current.join("whisper-base/conflict.bin"), b"current").unwrap();
        std::fs::write(legacy.join("whisper-base/conflict.bin"), b"legacy").unwrap();
        std::fs::write(legacy.join("whisper-base/ggml-base.bin"), b"missing").unwrap();
        std::fs::write(legacy.join("whisper-base/.openless-asr-ready"), b"ready").unwrap();
        let store = ModelStore::new(ModelStoreConfig::new(current.clone()).unwrap()).unwrap();

        store.migrate_legacy_root(&legacy).unwrap();

        assert_eq!(
            std::fs::read(current.join("whisper-base/conflict.bin")).unwrap(),
            b"current"
        );
        assert_eq!(
            std::fs::read(current.join("whisper-base/ggml-base.bin")).unwrap(),
            b"missing"
        );
        assert!(current
            .join("whisper-base")
            .join(MODEL_READY_SENTINEL)
            .is_file());
        assert!(legacy.join("whisper-base/conflict.bin").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn archive_extraction_rejects_parent_paths() {
        let root =
            std::env::temp_dir().join(format!("openless-model-archive-{}", uuid::Uuid::new_v4()));
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"escape").unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        assert!(extract_archive_safely(&bytes, &root, 1024).is_err());
        assert!(!root.with_file_name("escape").exists());
    }

    #[test]
    fn tar_archive_requires_declared_root_and_manifest_paths() {
        let staging =
            std::env::temp_dir().join(format!("openless-model-tar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&staging).unwrap();
        let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o600);
        header.set_cksum();
        archive
            .append_data(&mut header, "fixture/model.onnx", &b"model"[..])
            .unwrap();
        let encoder = archive.into_inner().unwrap();
        let bytes = encoder.finish().unwrap();
        std::fs::write(staging.join("model.tar.bz2"), bytes).unwrap();
        let spec = ModelArchiveSpec {
            file_path: "model.tar.bz2".into(),
            root_dir: "fixture".into(),
            required_paths: vec!["model.onnx".into()],
        };

        expand_tar_bz2_archive(&staging, &spec, 1024, 2048).unwrap();

        assert_eq!(std::fs::read(staging.join("model.onnx")).unwrap(), b"model");
        assert!(!staging.join("model.tar.bz2").exists());
        let _ = std::fs::remove_dir_all(staging);
    }

    #[test]
    fn model_directories_are_scoped_by_runtime() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-runtime-scope-{}",
            uuid::Uuid::new_v4()
        ));
        let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
        let generic = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-small").unwrap();
        let foundry = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();

        assert_eq!(
            store.model_dir(&generic).unwrap(),
            root.join("whisper-small")
        );
        assert_eq!(
            store.model_dir(&foundry).unwrap(),
            root.join("foundry-local")
        );
        assert_ne!(
            store.model_dir(&generic).unwrap(),
            store.model_dir(&foundry).unwrap()
        );
        assert!(store.delete_model(&foundry).is_err());
    }

    #[test]
    fn whisper_turbo_falls_back_to_q5_without_sharing_delete_state() {
        let root = std::env::temp_dir().join(format!(
            "openless-whisper-q5-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
        let turbo =
            LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-large-v3-turbo").unwrap();
        let q5 =
            LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-large-v3-turbo-q5").unwrap();
        let q5_dir = store.model_dir(&q5).unwrap();
        std::fs::create_dir_all(&q5_dir).unwrap();
        std::fs::write(q5_dir.join("ggml-large-v3-turbo-q5_0.bin"), b"q5").unwrap();
        std::fs::write(q5_dir.join(MODEL_READY_SENTINEL), b"ready").unwrap();

        assert!(store.is_installed(&turbo).unwrap());
        assert_eq!(store.runtime_model_dir(&turbo).unwrap(), q5_dir);
        store.delete_model(&turbo).unwrap();
        assert!(store.is_installed(&q5).unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_layout_migration_maps_qwen_sherpa_and_whisper_q5() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-layout-migrate-{}",
            uuid::Uuid::new_v4()
        ));
        let legacy = root.join("legacy");
        let current = root.join("current");
        std::fs::create_dir_all(legacy.join("qwen3-asr/qwen3-asr-0.6b")).unwrap();
        std::fs::write(
            legacy.join("qwen3-asr/qwen3-asr-0.6b/.openless-asr-ready"),
            b"ready",
        )
        .unwrap();
        std::fs::write(legacy.join("qwen3-asr/qwen3-asr-0.6b/config.json"), b"{}").unwrap();
        std::fs::create_dir_all(legacy.join("sherpa-onnx/sense-voice-small-zh")).unwrap();
        std::fs::write(
            legacy.join("sherpa-onnx/sense-voice-small-zh/model.int8.onnx"),
            b"model",
        )
        .unwrap();
        std::fs::write(
            legacy.join("sherpa-onnx/sense-voice-small-zh/tokens.txt"),
            b"tokens",
        )
        .unwrap();
        std::fs::create_dir_all(legacy.join("whisper-large-v3-turbo")).unwrap();
        std::fs::write(
            legacy.join("whisper-large-v3-turbo/ggml-large-v3-turbo-q5_0.bin"),
            b"q5",
        )
        .unwrap();
        let store = ModelStore::new(ModelStoreConfig::new(current.clone()).unwrap()).unwrap();

        store.migrate_legacy_root(&legacy).unwrap();

        assert!(current
            .join("qwen3-asr-0.6b/.openless-model-ready")
            .is_file());
        assert!(current
            .join("sherpa-onnx/sense-voice-small-zh/model.int8.onnx")
            .is_file());
        assert!(current
            .join("sherpa-onnx/sense-voice-small-zh/.openless-model-ready")
            .is_file());
        assert_eq!(
            std::fs::read(current.join("whisper-large-v3-turbo-q5/ggml-large-v3-turbo-q5_0.bin"))
                .unwrap(),
            b"q5"
        );
        assert!(!legacy.join("qwen3-asr/qwen3-asr-0.6b").exists());
        assert!(!legacy.join("sherpa-onnx/sense-voice-small-zh").exists());
        assert!(!legacy
            .join("whisper-large-v3-turbo/ggml-large-v3-turbo-q5_0.bin")
            .exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_layout_migration_runs_in_place_for_the_default_root() {
        let root = std::env::temp_dir().join(format!(
            "openless-model-layout-in-place-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("qwen3-asr/qwen3-asr-0.6b");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("config.json"), b"{}").unwrap();
        std::fs::write(source.join(".ready"), b"ready").unwrap();
        std::fs::create_dir_all(root.join("whisper-small")).unwrap();
        std::fs::write(root.join("whisper-small/.openless-model-ready"), b"ready").unwrap();
        let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();

        store.migrate_legacy_root(&root).unwrap();

        assert!(root.join("qwen3-asr-0.6b/config.json").is_file());
        assert!(root.join("qwen3-asr-0.6b/.openless-model-ready").is_file());
        assert!(!source.exists());
        assert!(!root.join("whisper-small").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tar_archive_required_file_cannot_be_a_directory() {
        let staging = std::env::temp_dir().join(format!(
            "openless-model-tar-directory-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&staging).unwrap();
        let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "fixture/model.onnx/", std::io::empty())
            .unwrap();
        let encoder = archive.into_inner().unwrap();
        let bytes = encoder.finish().unwrap();
        std::fs::write(staging.join("model.tar.bz2"), bytes).unwrap();
        let spec = ModelArchiveSpec {
            file_path: "model.tar.bz2".into(),
            root_dir: "fixture".into(),
            required_paths: vec!["model.onnx".into()],
        };

        assert!(expand_tar_bz2_archive(&staging, &spec, 1024, 2048).is_err());
        let _ = std::fs::remove_dir_all(staging);
    }

    #[test]
    fn relocation_is_verified_and_old_root_is_removed_only_on_finish() {
        let base = std::env::temp_dir().join(format!(
            "openless-model-relocation-{}",
            uuid::Uuid::new_v4()
        ));
        let current = base.join("old/OpenLess/models");
        let next = base.join("new/OpenLess/models");
        std::fs::create_dir_all(current.join("whisper-base")).unwrap();
        std::fs::write(current.join("whisper-base/ggml-base.bin"), b"model").unwrap();
        let store = ModelStore::new(ModelStoreConfig::new(current.clone()).unwrap()).unwrap();

        store.relocate_root(next.clone()).unwrap();

        assert!(current.is_dir());
        assert_eq!(
            std::fs::read(next.join("whisper-base/ggml-base.bin")).unwrap(),
            b"model"
        );
        assert!(next.join(MODEL_RELOCATION_JOURNAL).is_file());
        drop(store);
        let _resumed = ModelStore::new(ModelStoreConfig::new(next.clone()).unwrap()).unwrap();
        assert!(!current.exists());
        assert!(!next.join(MODEL_RELOCATION_JOURNAL).exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn relocation_rejects_conflicting_destination_files() {
        let base = std::env::temp_dir().join(format!(
            "openless-model-relocation-conflict-{}",
            uuid::Uuid::new_v4()
        ));
        let current = base.join("old/OpenLess/models");
        let next = base.join("new/OpenLess/models");
        std::fs::create_dir_all(current.join("whisper-base")).unwrap();
        std::fs::create_dir_all(next.join("whisper-base")).unwrap();
        std::fs::write(current.join("whisper-base/ggml-base.bin"), b"source").unwrap();
        std::fs::write(next.join("whisper-base/ggml-base.bin"), b"target").unwrap();
        let store = ModelStore::new(ModelStoreConfig::new(current.clone()).unwrap()).unwrap();

        assert!(store.relocate_root(next.clone()).is_err());
        assert_eq!(store.models_root_dir(), current);
        assert!(!next.join(MODEL_RELOCATION_JOURNAL).exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
