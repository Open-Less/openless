use openless_core::{
    credentials::is_retired_macos_asr_provider, BackendConfig, BackendDependencies, ChannelKind,
    ChannelSummary, CredentialMetadata, InMemoryCredentialStore, LocalAsrActivationRequest,
    LocalAsrRuntime, LocalAsrTarget, ModelRuntimeAdapter, ModelStore, ModelStoreConfig,
    OpenLessBackend, ProviderSlot,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openless-macos-retirement-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"fixture").unwrap();
}
fn channel(id: &str, provider: &str, enabled: bool, order: u32) -> ChannelSummary {
    ChannelSummary {
        id: id.into(),
        name: format!("Custom {id}"),
        provider_type: provider.into(),
        enabled,
        order,
        last_test: None,
    }
}

#[test]
fn retirement_disables_renamed_legacy_channels_but_preserves_cloud_windows_and_experiments() {
    let mut metadata = CredentialMetadata::from_parts(
        vec![
            channel("uuid-mlx", "local-qwen3-mlx", true, 0),
            channel("cloud", "volcengine", true, 1),
            channel("sense", "experimental-sensevoice", true, 2),
            channel("windows", "foundry-local-whisper", true, 3),
            channel("apple", "apple-speech", true, 4),
        ],
        vec![channel("llm", "openai", true, 0)],
        "uuid-mlx",
        "llm",
        "omni",
        8,
    );
    assert!(metadata.retire_macos_local_asr());
    let channels = metadata.list_channels(ChannelKind::Asr);
    for channel in &channels {
        assert_eq!(
            channel.enabled,
            !is_retired_macos_asr_provider(&channel.provider_type)
        );
    }
    assert_eq!(
        channels.iter().find(|c| c.id == "uuid-mlx").unwrap().name,
        "Custom uuid-mlx"
    );
    assert_eq!(metadata.active_provider(ProviderSlot::Asr), "cloud");
    assert_eq!(metadata.active_provider(ProviderSlot::Llm), "llm");
    assert_eq!(metadata.active_provider(ProviderSlot::Omni), "omni");
    let revision = metadata.revision();
    assert!(!metadata.retire_macos_local_asr());
    assert_eq!(metadata.revision(), revision);
}

#[test]
fn apple_system_recognition_is_not_retired_or_deselected() {
    let mut metadata = CredentialMetadata::from_parts(
        vec![
            channel("apple", "apple-speech", true, 0),
            channel("legacy", "local-whisper", true, 1),
        ],
        vec![],
        "apple",
        "",
        "",
        4,
    );
    assert!(metadata.retire_macos_local_asr());
    assert_eq!(metadata.active_provider(ProviderSlot::Asr), "apple");
    let channels = metadata.list_channels(ChannelKind::Asr);
    assert!(channels.iter().find(|c| c.id == "apple").unwrap().enabled);
    assert!(!channels.iter().find(|c| c.id == "legacy").unwrap().enabled);
    assert!(
        !metadata.retire_macos_local_asr(),
        "migration remains idempotent"
    );
}

#[test]
fn retirement_does_not_turn_on_a_disabled_cloud_channel_or_break_metadata_revisions() {
    let metadata = CredentialMetadata::from_parts(
        vec![
            channel("local", "local-whisper", true, 0),
            channel("cloud", "volcengine", false, 1),
        ],
        vec![],
        "local",
        "",
        "",
        4,
    );
    let view = metadata.clone().retired_macos_view();
    assert_eq!(view.revision(), metadata.revision());
    assert_eq!(view.active_provider(ProviderSlot::Asr), "");
    assert!(view
        .list_channels(ChannelKind::Asr)
        .iter()
        .all(|c| !c.enabled));
}

#[test]
fn cleanup_removes_only_known_legacy_paths_and_is_repeatable() {
    let temp = Temp::new();
    let root = temp.0.join("OpenLess/models");
    let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
    let old = [
        root.join("qwen3-asr-0.6b/model.safetensors"),
        root.join("qwen3-asr/qwen3-asr-1.7b/model.safetensors"),
        root.join("whisper-small/ggml-small.bin"),
        root.join("whisper-large-v3-turbo-q5/ggml-large-v3-turbo-q5_0.bin"),
        root.join(".qwen3-asr-0.6b.staging/part"),
    ];
    let preserved = [
        root.join("experimental-sensevoice-small/model.int8.onnx"),
        root.join("experimental-embedding-gemma-en/reader_P6.mlmodelc/data"),
        root.join("sherpa-onnx/sense-voice-small-zh/model.int8.onnx"),
        root.join("unrelated/notes.txt"),
        temp.0.join("other-app/model.safetensors"),
    ];
    for path in old.iter().chain(preserved.iter()) {
        write(path);
    }
    store.retire_macos_local_models(&root).unwrap();
    store.retire_macos_local_models(&root).unwrap();
    assert!(old.iter().all(|path| !path.exists()));
    assert!(preserved.iter().all(|path| path.exists()));
    assert!(root.is_dir());
}

#[cfg(unix)]
#[test]
fn cleanup_never_follows_model_or_parent_symlinks() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let root = temp.0.join("models");
    let outside = temp.0.join("other-app");
    write(&outside.join("qwen3-asr-0.6b/weights"));
    write(&outside.join("keep"));
    std::fs::create_dir_all(&root).unwrap();
    symlink(&outside, root.join("qwen3-asr")).unwrap();
    symlink(&outside, root.join("whisper-small")).unwrap();
    let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
    store.retire_macos_local_models(&root).unwrap();
    assert!(outside.join("keep").is_file());
    assert!(outside.join("qwen3-asr-0.6b/weights").is_file());
    assert!(std::fs::symlink_metadata(root.join("whisper-small")).is_err());
}

#[cfg(unix)]
#[test]
fn cleanup_never_follows_a_symlinked_models_root() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let outside = temp.0.join("other-app");
    write(&outside.join("whisper-small/weights"));
    let root = temp.0.join("models");
    symlink(&outside, &root).unwrap();
    let store = ModelStore::new(ModelStoreConfig::new(root.clone()).unwrap()).unwrap();
    store.retire_macos_local_models(&root).unwrap();
    assert!(outside.join("whisper-small/weights").is_file());
    assert!(std::fs::symlink_metadata(root)
        .unwrap()
        .file_type()
        .is_symlink());
}

struct RetiredRuntime;
impl ModelRuntimeAdapter for RetiredRuntime {}

#[tokio::test]
async fn retired_host_rejects_download_activation_prepare_preload_and_test_before_native_calls() {
    let temp = Temp::new();
    let mut deps = BackendDependencies::unsupported();
    deps.local_asr_runtime = Some(Arc::new(RetiredRuntime));
    deps.credential_store = Arc::new(InMemoryCredentialStore::default());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: temp.0.clone(),
            ..BackendConfig::default()
        },
        deps,
    )
    .unwrap();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let api = &backend.services().local_asr;
    assert!(api.start_download(target.clone(), None).await.is_err());
    assert!(api.set_active_model(target.clone()).await.is_err());
    assert!(api.prepare(target.clone()).await.is_err());
    assert!(api.preload(LocalAsrRuntime::Generic).await.is_err());
    assert!(api.test_model(target.clone()).await.is_err());
    let legacy_channel = backend
        .create_channel(ChannelKind::Asr, "local-qwen3-mlx".into(), "Legacy".into())
        .await
        .unwrap();
    let error = api.test_channel(legacy_channel).await.unwrap_err();
    assert_eq!(error.code, openless_core::BackendErrorCode::Unsupported);
    assert!(backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "local-qwen3-mlx".into()
        })
        .await
        .is_err());
    assert!(api
        .list_models(LocalAsrRuntime::Generic)
        .await
        .unwrap()
        .is_empty());
}
