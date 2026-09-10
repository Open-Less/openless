use std::path::PathBuf;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::credentials::CredentialStore;
use crate::domains::BackendServices;
use crate::errors::BackendError;
use crate::ports::{DictationEngine, HostActions, TextInserter, TextPolisher};
use crate::shared_types::PlatformCapabilities;

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc>;
    fn today_local(&self) -> chrono::NaiveDate;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn today_local(&self) -> chrono::NaiveDate {
        chrono::Local::now().date_naive()
    }
}

#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// Host-resolved home/workspace fallback used by Core workdir policy.
    pub home_dir: Option<PathBuf>,
    pub resource_dir: Option<PathBuf>,
    pub platform: PlatformCapabilities,
    pub locale: String,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::new(),
            cache_dir: PathBuf::new(),
            home_dir: None,
            resource_dir: None,
            platform: PlatformCapabilities::default(),
            locale: "en-US".to_string(),
        }
    }
}

pub trait TaskSpawner: Send + Sync {
    fn spawn(&self, task: BoxFuture<'static, ()>);
}

pub struct BackendDependencies {
    pub host_actions: Arc<dyn HostActions>,
    pub text_inserter: Arc<dyn TextInserter>,
    pub dictation_engine: Arc<dyn DictationEngine>,
    pub task_spawner: Arc<dyn TaskSpawner>,
    pub credential_store: Arc<dyn CredentialStore>,
    pub services: BackendServices,
    pub local_asr_runtime: Option<Arc<dyn crate::local_asr_service::ModelRuntimeAdapter>>,
    pub marketplace_config: Option<crate::marketplace::MarketplaceConfig>,
    pub selection_runtime: Option<Arc<dyn crate::domains::SelectionRuntimeAdapter>>,
    pub selection_polisher: Option<Arc<dyn TextPolisher>>,
    pub qa_runtime: Option<Arc<dyn crate::domains::QaRuntimeAdapter>>,
}

impl BackendDependencies {
    /// Dependency set for data-only hosts and transitional adapters.
    /// Dictation calls fail explicitly with `Unsupported`; repository APIs and
    /// lifecycle/event contracts remain fully usable.
    pub fn unsupported() -> Self {
        Self {
            host_actions: Arc::new(crate::ports::NoopHostActions),
            text_inserter: Arc::new(crate::ports::UnsupportedTextInserter),
            dictation_engine: Arc::new(UnsupportedDictationEngine),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(crate::credentials::UnsupportedCredentialStore),
            services: BackendServices::unsupported(),
            local_asr_runtime: None,
            marketplace_config: None,
            selection_runtime: None,
            selection_polisher: None,
            qa_runtime: None,
        }
    }
}

impl std::fmt::Debug for BackendDependencies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackendDependencies")
            .finish_non_exhaustive()
    }
}

pub struct TokioTaskSpawner;

impl TaskSpawner for TokioTaskSpawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        // The host owns the Tokio runtime. A synchronous teardown can race
        // with runtime shutdown, so cleanup must never create a private
        // runtime (or panic) when no host runtime is available.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(task);
            }
            Err(_) => {
                log::warn!("task spawner called without a host Tokio runtime");
            }
        }
    }
}

pub struct UnsupportedDictationEngine;

impl DictationEngine for UnsupportedDictationEngine {
    fn start(
        &self,
        _session_id: crate::types::SessionId,
        _context: Arc<crate::dictation_context::DictationContext>,
        _progress: Arc<dyn crate::ports::EngineProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                crate::errors::BackendErrorCode::Unsupported,
                "dictation engine is not configured",
            ))
        })
    }

    fn finish(
        &self,
        _session_id: crate::types::SessionId,
        _progress: Arc<dyn crate::ports::EngineProgressSink>,
    ) -> BoxFuture<'static, Result<crate::ports::EngineResult, crate::ports::EngineFailure>> {
        Box::pin(async {
            Err(crate::ports::EngineFailure::from(BackendError::new(
                crate::errors::BackendErrorCode::Unsupported,
                "dictation engine is not configured",
            )))
        })
    }

    fn cancel(
        &self,
        _session_id: crate::types::SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
