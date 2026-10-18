//! Explicit debug-bundle acceptance, without microphone/UI/OS credential access.
//! The same service, downloads, leases and native engines as production are used.
use super::*;
use anyhow::{bail, ensure, Context, Result};
use openless_core::{
    credentials::{ChannelKind, CredentialStore},
    experimental_asr::ExperimentalAsrPhase as Phase,
    InMemoryCredentialStore, ModelStore, ModelStoreConfig, ProviderInvocation, TaskSpawner,
    TextStreamChunk,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

struct Spawner;
impl TaskSpawner for Spawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        tokio::spawn(task);
    }
}
#[derive(Default)]
struct Previews(Mutex<Vec<String>>);
impl TextStreamSink for Previews {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        assert_eq!(
            chunk.offset, 0,
            "TextStream previews must replace, not append"
        );
        self.0.lock().unwrap().push(chunk.text);
        Ok(())
    }
}
fn unloaded(runtime: &ExperimentalNativeRuntime) -> Result<()> {
    for slot in &runtime.loaded {
        ensure!(
            slot.lock().unwrap().is_none(),
            "unexpected native initialization"
        );
    }
    ensure!(
        !runtime.textstream.is_running(),
        "unexpected TextStream worker"
    );
    Ok(())
}
async fn phase(service: &ExperimentalAsrService, model: Model, expected: Phase) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(1800), async {
        let mut last = Instant::now() - Duration::from_secs(5);
        loop {
            let status = service
                .status()?
                .into_iter()
                .find(|s| s.model == model)
                .unwrap();
            if status.phase == expected {
                return Ok(());
            }
            if status.phase == Phase::Failed {
                bail!("{model:?}: {}", status.error.unwrap_or_default());
            }
            if last.elapsed() >= Duration::from_secs(5) {
                println!(
                    "{model:?}: {:?} {}/{}",
                    status.phase, status.downloaded_bytes, status.total_bytes
                );
                last = Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("experimental operation timed out")?
}
fn service(
    store: Arc<ModelStore>,
    credentials: Arc<dyn CredentialStore>,
    runtime: Arc<ExperimentalNativeRuntime>,
    path: PathBuf,
) -> Result<Arc<ExperimentalAsrService>> {
    Ok(ExperimentalAsrService::new(
        store,
        credentials,
        runtime,
        Arc::new(Spawner),
        path,
    )?)
}
fn scoped_retirement(store: &ModelStore, root: &Path) -> Result<()> {
    let old = root.join("qwen3-asr-0.6b");
    let partial = root.join(".whisper-small.staging");
    let preserved = root.join("unrelated/keep.txt");
    for path in [&old, &partial, preserved.parent().unwrap()] {
        std::fs::create_dir_all(path)?;
    }
    std::fs::write(old.join("model.safetensors"), b"legacy fixture")?;
    std::fs::write(partial.join("part"), b"interrupted fixture")?;
    std::fs::write(&preserved, b"keep")?;
    store.retire_macos_local_models(root)?;
    store.retire_macos_local_models(root)?;
    ensure!(
        !old.exists() && !partial.exists() && preserved.is_file(),
        "upgrade cleanup scope violated"
    );
    for model in Model::ALL {
        ensure!(
            store.is_installed(&model.target())?,
            "upgrade deleted an experiment"
        );
    }
    println!("PASS scoped upgrade cleanup and idempotence");
    Ok(())
}

pub(crate) async fn run(root: PathBuf, wav: PathBuf, download: bool) -> Result<()> {
    // Never accept the installed application's data directory or follow a supplied symlink.
    ensure!(
        root.is_absolute()
            && root.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("openless-asr-acceptance")),
        "use an absolute isolated openless-asr-acceptance* directory"
    );
    ensure!(
        !std::fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()),
        "acceptance root cannot be a symlink"
    );
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    ensure!(
        root.file_name().is_some_and(|name| name
            .to_string_lossy()
            .starts_with("openless-asr-acceptance")),
        "unsafe acceptance root"
    );
    ensure!(
        !std::fs::symlink_metadata(root.join("models")).is_ok_and(|m| m.file_type().is_symlink()),
        "models root cannot be a symlink"
    );
    let store = Arc::new(ModelStore::new(ModelStoreConfig::new(
        root.join("models"),
    )?)?);
    let credentials: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::default());
    let runtime = ExperimentalNativeRuntime::new();
    let state = root.join("experimental-asr.json");
    let models = service(
        store.clone(),
        credentials.clone(),
        runtime.clone(),
        state.clone(),
    )?;
    models.resume();
    // Cold startup/status is file-only, even with saved enabled models.
    for _ in 0..3 {
        models.status()?;
    }
    unloaded(&runtime)?;
    println!("PASS cold construction and settings/status without inference");
    let initial = credentials
        .active_provider(openless_core::ProviderSlot::Asr)
        .await?;
    for model in Model::ALL {
        if !download {
            ensure!(
                store.is_installed(&model.target())?
                    && runtime.installed(model, &store.runtime_model_dir(&model.target())?),
                "{model:?} is missing: use explicit --download once"
            );
        }
        models.set_enabled(model, true)?;
    }
    for model in Model::ALL {
        phase(&models, model, Phase::Ready).await?;
    }
    unloaded(&runtime)?;
    ensure!(
        credentials
            .active_provider(openless_core::ProviderSlot::Asr)
            .await?
            == initial,
        "downloading/registration changed channel selection"
    );
    scoped_retirement(&store, &store.models_root_dir())?;
    println!("PASS three installed packages, no inference after download/enable");

    let mut deps = openless_core::BackendDependencies::unsupported();
    deps.credential_store = credentials.clone();
    deps.task_spawner = Arc::new(Spawner);
    deps.services.model_store = Some(store.clone());
    deps.services.experimental_asr = Some(models.clone());
    let backend = openless_core::OpenLessBackend::new(
        openless_core::BackendConfig {
            data_dir: root.join("backend"),
            ..openless_core::BackendConfig::default()
        },
        deps,
    )?;
    backend
        .set_channel_enabled(
            ChannelKind::Asr,
            Model::TextStream.channel_id().into(),
            true,
        )
        .await?;
    backend
        .set_active_provider(
            openless_core::ProviderSlot::Asr,
            Model::TextStream.channel_id().into(),
        )
        .await?;
    unloaded(&runtime)?;
    let resumed_runtime = ExperimentalNativeRuntime::new();
    let resumed = service(
        store.clone(),
        credentials.clone(),
        resumed_runtime.clone(),
        state.clone(),
    )?;
    resumed.resume();
    for model in Model::ALL {
        phase(&resumed, model, Phase::Ready).await?;
    }
    for _ in 0..3 {
        resumed.status()?;
    }
    unloaded(&resumed_runtime)?;
    ensure!(
        credentials
            .active_provider(openless_core::ProviderSlot::Asr)
            .await?
            == Model::TextStream.channel_id(),
        "restart changed the explicitly selected channel"
    );
    println!(
        "PASS installed-enabled/selected restart and repeated settings/status without inference"
    );

    let bytes = std::fs::read(&wav).with_context(|| format!("read WAV {}", wav.display()))?;
    let samples = super::super::test_run::decode_wav_16k_mono(&bytes)?;
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * 32768.0).clamp(-32768.0, 32767.0) as i16).to_le_bytes())
        .collect();
    ensure!(
        pcm.len() >= 32_000,
        "provide at least one second of real non-silent 16kHz mono WAV"
    );
    let engine = ExperimentalTranscriptionEngine {
        models: models.clone(),
        runtime: runtime.clone(),
    };
    for model in [Model::SenseVoice, Model::EmbeddingGemma, Model::TextStream] {
        backend
            .set_channel_enabled(ChannelKind::Asr, model.channel_id().into(), true)
            .await?;
        backend
            .set_active_provider(openless_core::ProviderSlot::Asr, model.channel_id().into())
            .await?;
        unloaded(&runtime)?;
        let previews = Arc::new(Previews::default());
        let mut context = DictationContext::default();
        context.asr = ProviderInvocation::for_provider(model.provider_type());
        let started = Instant::now();
        let session = engine
            .start(
                SessionId::new(),
                Arc::new(context.clone()),
                previews.clone(),
            )
            .await?;
        let loaded = started.elapsed();
        let sse = if model == Model::TextStream {
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(180))
                .build()?
                .get("http://localhost:7890/stream")
                .send()
                .await?;
            ensure!(response.status().is_success(), "SSE subscription failed");
            Some(tokio::spawn(async move { response.text().await }))
        } else {
            None
        };
        let transcription = Instant::now();
        for chunk in pcm.chunks(16_000) {
            session.consume_pcm_chunk(chunk);
            if model == Model::TextStream {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let output = session.finish().await?;
        println!(
            "{model:?}: load={loaded:?}, transcribe={:?}, duration={}ms, text={}",
            transcription.elapsed(),
            output.duration_ms,
            output.text
        );
        let text = output.text.to_lowercase();
        ensure!(
            text.contains("hello") && text.contains("test") && text.contains("system"),
            "unexpected fixture transcript: {}",
            output.text
        );
        ensure!(
            output.duration_ms == pcm.len() as u64 / 32,
            "incorrect recording duration"
        );
        if let Some(sse) = sse {
            let wire = sse.await??;
            let terminal = wire
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .map(serde_json::from_str::<serde_json::Value>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let terminal = terminal.last().context("no SSE snapshots")?;
            ensure!(
                terminal["type"] == "stream"
                    && terminal["done"] == true
                    && terminal["draft"] == ""
                    && terminal["finalized"] == output.text,
                "SSE final transcript differs from the committed transcript"
            );
            ensure!(
                !previews.0.lock().unwrap().is_empty(),
                "no live replacement snapshots"
            );
            ensure!(
                !runtime.textstream.is_running(),
                "worker remained after finish"
            );
            ensure!(
                std::net::TcpStream::connect("127.0.0.1:7890").is_err(),
                "SSE remained after finish"
            );
        }
        // Exercise the real public Core channel mutation, not a test-only release.
        backend
            .set_channel_enabled(ChannelKind::Asr, model.channel_id().into(), false)
            .await?;
        unloaded(&runtime)?;
        ensure!(
            models
                .status()?
                .iter()
                .find(|s| s.model == model)
                .unwrap()
                .enabled,
            "channel disable changed experiment opt-in"
        );
        ensure!(
            store.is_installed(&model.target())?,
            "channel disable deleted model files"
        );
        backend
            .set_channel_enabled(ChannelKind::Asr, model.channel_id().into(), true)
            .await?;
        backend
            .set_active_provider(openless_core::ProviderSlot::Asr, model.channel_id().into())
            .await?;
        let session = engine
            .start(
                SessionId::new(),
                Arc::new(context.clone()),
                Arc::new(Previews::default()),
            )
            .await?;
        session.consume_pcm_chunk(&pcm[..32_000]);
        session.cancel().await?;
        ensure!(
            session.finish().await.is_err(),
            "cancelled audio was committed"
        );
        models.set_enabled(model, false)?;
        phase(&models, model, Phase::Disabled).await?;
        unloaded(&runtime)?;
        ensure!(
            store.is_installed(&model.target())?,
            "disable deleted model files"
        );
        ensure!(
            engine
                .start(
                    SessionId::new(),
                    Arc::new(context),
                    Arc::new(Previews::default())
                )
                .await
                .is_err(),
            "disabled model loaded"
        );
        println!("PASS {model:?}: actual inference, release, cancellation, disable retains files");
    }
    let runtime = ExperimentalNativeRuntime::new();
    let restored = service(store.clone(), credentials, runtime.clone(), state)?;
    restored.resume();
    tokio::time::sleep(Duration::from_millis(100)).await;
    ensure!(
        restored
            .status()?
            .iter()
            .all(|s| !s.enabled && s.phase == Phase::Disabled),
        "disabled state did not persist"
    );
    unloaded(&runtime)?;
    for model in Model::ALL {
        ensure!(restored.lease(model).is_err(), "restart re-enabled a model");
    }
    println!("PASS disabled-model restart without inference or downloads");
    std::fs::write(
        root.join("passed.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "status": "passed", "models": ["sense-voice", "embedding-gemma", "text-stream"],
            "fixture": wav, "microphoneUsed": false, "keychainUsed": false
        }))?,
    )?;
    Ok(())
}
