mod credentials;
mod history;
mod self_test;
mod voice_configuration;
use openless_core::{BackendConfig, BackendDependencies, OpenLessBackend};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    os::raw::c_char,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone)]
struct Host {
    runtime: Arc<tokio::runtime::Runtime>,
    backend: Arc<OpenLessBackend>,
    credentials: std::sync::Arc<credentials::HarmonyCredentials>,
}
static HOST: OnceLock<Mutex<Host>> = OnceLock::new();
// Reload and start must use the same host. Cancellation and PCM delivery stay
// independent so a slow provider cannot block either operation.
static VOICE_CONTROL: Mutex<()> = Mutex::new(());
fn capabilities() -> openless_core::shared_types::PlatformCapabilities {
    openless_core::shared_types::PlatformCapabilities {
        platform: "harmony".into(),
        supports_ime_input: true,
        supports_overlay: false,
        supports_desktop_hotkey: false,
        supports_tray: false,
        supports_local_asr: false,
        supports_local_qwen3_mlx: false,
        supports_in_app_dictation: true,
        supports_auto_update: false,
        supports_voice_edit: false,
    }
}
fn response(result: Result<Value, String>) -> *mut c_char {
    let value = match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error}),
    };
    CString::new(value.to_string()).unwrap().into_raw()
}
unsafe fn text(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("null argument".into());
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(str::to_owned)
        .map_err(|e| e.to_string())
}
#[no_mangle]
pub unsafe extern "C" fn openless_init(data: *const c_char, cache: *const c_char) -> *mut c_char {
    response(
        std::panic::catch_unwind(|| {
            if HOST.get().is_some() {
                return Ok(json!(true));
            }
            let data = PathBuf::from(text(data)?);
            let cache = PathBuf::from(text(cache)?);
            std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
            let runtime = Arc::new(tokio::runtime::Runtime::new().map_err(|e| e.to_string())?);
            let host = build_host(data, cache, runtime)?;
            let snapshot =
                serde_json::to_value(host.backend.snapshot()).map_err(|e| e.to_string())?;
            HOST.set(Mutex::new(host))
                .map_err(|_| "backend already initialized".to_string())?;
            Ok(snapshot)
        })
        .unwrap_or_else(|_| Err("native initialization failed".into())),
    )
}
fn build_host(
    data: PathBuf,
    cache: PathBuf,
    runtime: Arc<tokio::runtime::Runtime>,
) -> Result<Host, String> {
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.marketplace_config = Some(openless_core::MarketplaceConfig::production());
    let credentials = Arc::new(credentials::HarmonyCredentials::new(
        data.join("channels.json"),
    ));
    dependencies.credential_store = credentials.clone();
    let transcription = Arc::new(openless_core::TranscriptionRouter::default());
    let cloud: Arc<dyn openless_core::TranscriptionEngine> = Arc::new(
        openless_core::SharedCloudTranscriptionEngine::with_task_spawner(
            credentials.clone(),
            dependencies.task_spawner.clone(),
        ),
    );
    for provider in openless_core::SHARED_CLOUD_ASR_PROVIDER_TYPES {
        transcription
            .register(*provider, cloud.clone())
            .map_err(|e| e.to_string())?;
    }
    let polisher = Arc::new(openless_core::TextPolisherRouter::default());
    let cloud_polisher: Arc<dyn openless_core::TextPolisher> = Arc::new(
        openless_core::SharedCloudTextPolisher::new(credentials.clone()),
    );
    for provider in openless_core::SHARED_CLOUD_LLM_PROVIDER_TYPES {
        polisher
            .register(*provider, cloud_polisher.clone())
            .map_err(|e| e.to_string())?;
    }
    let recorder: Arc<dyn openless_core::AudioRecorder> = Arc::new(
        openless_core::ExternalAudioRecorder::with_recordings_directory(data.join("recordings")),
    );
    let traditional = Arc::new(openless_core::PipelineDictationEngine::new(
        recorder.clone(),
        transcription.clone(),
        polisher.clone(),
    ));
    let engine = Arc::new(openless_core::DictationEngineRouter::new(traditional));
    let omni: Arc<dyn openless_core::DictationEngine> = Arc::new(
        openless_core::SharedOmniDictationEngine::new(credentials.clone(), recorder),
    );
    for provider in openless_core::SHARED_OMNI_PROVIDER_TYPES {
        engine
            .register_omni(*provider, omni.clone())
            .map_err(|e| e.to_string())?;
    }
    dependencies.dictation_engine = engine;
    dependencies.services.configure_auxiliary_runtime(
        Arc::new(openless_core::SharedAuxiliaryTextPolisher::new(
            credentials.clone(),
            polisher,
        )),
        transcription,
    );
    dependencies.services.provider = Arc::new(openless_core::ProviderService::new(
        credentials.clone(),
        dependencies.task_spawner.clone(),
    ));
    let backend = Arc::new(
        OpenLessBackend::new(
            BackendConfig {
                data_dir: data,
                cache_dir: cache,
                platform: capabilities(),
                locale: "zh-CN".into(),
                ..BackendConfig::default()
            },
            dependencies,
        )
        .map_err(|e| e.to_string())?,
    );
    runtime
        .block_on(backend.start())
        .map_err(|e| e.to_string())?;
    Ok(Host {
        runtime,
        backend,
        credentials,
    })
}
fn required<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args[field]
        .as_str()
        .ok_or_else(|| format!("{field} required"))
}
fn channel_kind(args: &Value) -> Result<openless_core::ChannelKind, String> {
    openless_core::ChannelKind::parse(required(args, "kind")?).map_err(|e| e.to_string())
}
fn credential_key(args: &Value) -> Result<openless_core::CredentialKey, String> {
    let account = required(args, "account")?;
    let namespace = if account == openless_core::MARKETPLACE_GITHUB_TOKEN_ACCOUNT {
        openless_core::CredentialNamespace::Marketplace
    } else if account.starts_with("ark.") {
        openless_core::CredentialNamespace::Llm
    } else if account.starts_with("omni.") {
        openless_core::CredentialNamespace::Omni
    } else {
        openless_core::CredentialNamespace::Asr
    };
    openless_core::CredentialKey::new(
        namespace,
        args["provider"].as_str().map(str::to_owned),
        account,
    )
    .map_err(|e| e.to_string())
}

fn ime_dictionary_base(host: &Host) -> PathBuf {
    let config = host.backend.config();
    config
        .data_dir
        .parent()
        .unwrap_or(&config.data_dir)
        .join(openless_core::ime_dictionary::BUNDLED_DIRECTORY)
}

fn dispatch(host: &Host, command: &str, args: Value) -> Result<Value, String> {
    let b = &host.backend;
    let value = match command {
        "__export_voice_configuration" => voice_configuration::export(host)?,
        "__import_voice_configuration" => voice_configuration::import(host, args)?,
        "ime_dictionary_select" => json!(openless_core::ime_dictionary::select(
            &ime_dictionary_base(host)
        )),
        "ime_dictionary_reject" => json!(openless_core::ime_dictionary::reject(
            &ime_dictionary_base(host)
        )),
        "ime_dictionary_update" => {
            json!(host
                .runtime
                .block_on(openless_core::ime_dictionary::update(
                    &ime_dictionary_base(host),
                    args["force"].as_bool().unwrap_or(false)
                ))?)
        }
        "ime_cloud_candidates" => json!(host
            .runtime
            .block_on(openless_core::ime_cloud::query(required(&args, "pinyin")?))),
        "accept_pending_correction" => {
            b.accept_pending_correction(required(&args, "id")?)
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "reject_pending_correction" => {
            b.reject_pending_correction(required(&args, "id")?);
            Value::Null
        }
        "dismiss_vocab_suggestions" => {
            b.dismiss_pending_corrections();
            Value::Null
        }
        "record_channel_test" => {
            host.runtime
                .block_on(b.record_channel_test(
                    channel_kind(&args)?,
                    required(&args, "id")?.into(),
                    args["ok"].as_bool().ok_or("ok required")?,
                    serde_json::from_value(args["latencyMs"].clone()).map_err(|e| e.to_string())?,
                    args["error"].as_str().map(str::to_owned),
                ))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "cloud_sync_status" => json!(host
            .runtime
            .block_on(b.cloud_sync_status())
            .map_err(|e| e.to_string())?),
        "cloud_sync_upload" => json!(host
            .runtime
            .block_on(
                b.cloud_sync_upload(
                    args["baseRevision"]
                        .as_u64()
                        .ok_or("baseRevision required")?,
                    serde_json::from_value(args["uiPreferences"].clone())
                        .map_err(|e| e.to_string())?
                )
            )
            .map_err(|e| e.to_string())?),
        "cloud_sync_restore" => json!(host
            .runtime
            .block_on(b.cloud_sync_restore())
            .map_err(|e| e.to_string())?),
        "cloud_sync_delete" => json!(host
            .runtime
            .block_on(
                b.cloud_sync_delete(
                    args["baseRevision"]
                        .as_u64()
                        .ok_or("baseRevision required")?
                )
            )
            .map_err(|e| e.to_string())?),
        "__marketplace_download_archive" => {
            use base64::Engine;
            let bytes = host
                .runtime
                .block_on(
                    b.services()
                        .marketplace
                        .download_archive(required(&args, "packId")?.into()),
                )
                .map_err(|e| e.to_string())?;
            if bytes.len() > 512 * 1024 {
                return Err("风格包 ZIP 最大为 512 KiB".into());
            }
            json!(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        "read_audio_recording" => history::read(host, &args)?,
        "retranscribe_recording" => history::retranscribe(host, &args)?,
        "apply_quick_note_repolish" => json!(b
            .apply_history_repolish(
                required(&args, "sessionId")?,
                required(&args, "text")?.into(),
                args["stylePackId"].as_str().map(str::to_owned)
            )
            .map_err(|e| e.to_string())?),
        "repolish" => json!(host
            .runtime
            .block_on(
                b.services()
                    .auxiliary
                    .repolish(openless_core::RepolishRequest {
                        raw_text: required(&args, "rawText")?.into(),
                        style_pack_id: args["stylePackId"].as_str().map(str::to_owned),
                        front_app: None,
                    })
            )
            .map_err(|e| e.to_string())?),
        "start_dictation" => json!(host
            .runtime
            .block_on(b.start_external_dictation_with_options(
                openless_core::DictationStartOptions {
                    insert_text: false,
                    ..Default::default()
                }
            ))
            .map_err(|e| e.to_string())?),
        "stop_dictation" => json!(host
            .runtime
            .block_on(b.stop_dictation_session(
                serde_json::from_value(args["session"].clone()).map_err(|e| e.to_string())?
            ))
            .map_err(|e| e.to_string())?),
        "cancel_dictation" => {
            let id = serde_json::from_value(args["session"].clone()).map_err(|e| e.to_string())?;
            if let Err(error) = host.runtime.block_on(b.cancel_dictation(id)) {
                // Cleanup may race a completed stop or a previous cancel.
                // Never absorb a mismatched ID targeting a newer session.
                if error.code != openless_core::BackendErrorCode::InvalidState
                    || error.message != "no active dictation session"
                    || b.snapshot().dictation.session_id.is_some()
                {
                    return Err(error.to_string());
                }
            }
            Value::Null
        }
        "get_startup_snapshot" => {
            json!({"contractVersion":openless_core::BACKEND_CONTRACT_VERSION,"backend":b.snapshot()})
        }
        "__test_style_pack_roundtrip" => self_test::styles(host)?,
        "__test_voice_roundtrip" => self_test::voice(host)?,
        "__revision_snapshot" => json!(b.snapshot()),
        "get_platform_capabilities" => json!(capabilities()),
        "check_network" => host.runtime.block_on(async {
            let start = std::time::Instant::now();
            let probe = openless_core::net::http()
                .get(format!(
                    "{}/packs?limit=1",
                    openless_core::MARKETPLACE_BASE_URL
                ))
                .timeout(std::time::Duration::from_secs(8))
                .send()
                .await;
            match probe {
                Ok(_) => json!({"online":true,"latencyMs":start.elapsed().as_millis() as u64}),
                Err(_) => json!({"online":false,"latencyMs":null}),
            }
        }),
        "get_settings" => json!(b.get_preferences()),
        "get_credentials" => json!(host
            .runtime
            .block_on(b.get_credentials_status())
            .map_err(|e| e.to_string())?),
        "get_settings_snapshot" => {
            json!({"preferences":b.get_preferences(),"revision":b.snapshot().preferences_revision})
        }
        "list_history" => json!(b.list_history().map_err(|e| e.to_string())?),
        "delete_history_entry" => {
            let id = required(&args, "id")?;
            if b.snapshot()
                .dictation
                .session_id
                .is_some_and(|session| session.to_string() == id)
            {
                return Err("stop the active recording before deleting it".into());
            }
            b.delete_history(id).map_err(|e| e.to_string())?;
            history::remove_audio(host, id)?;
            Value::Null
        }
        "clear_history" => {
            if b.snapshot().dictation.session_id.is_some() {
                return Err("stop the active recording before clearing history".into());
            }
            let entries = b.list_history().map_err(|e| e.to_string())?;
            b.clear_history().map_err(|e| e.to_string())?;
            for entry in entries {
                history::remove_audio(host, &entry.id)?;
            }
            Value::Null
        }
        "list_vocab" => json!(b.list_vocabulary().map_err(|e| e.to_string())?),
        "get_activity_stats" => json!(b.list_activity().map_err(|e| e.to_string())?),
        "list_correction_rules" => json!(b.list_correction_rules().map_err(|e| e.to_string())?),
        "add_learned_vocab" => {
            b.add_learned_vocabulary(required(&args, "phrase")?.into())
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "add_correction_rule" => json!(b
            .add_correction_rule(
                required(&args, "pattern")?.into(),
                required(&args, "replacement")?.into()
            )
            .map_err(|e| e.to_string())?),
        "remove_correction_rule" => {
            b.remove_correction_rule(required(&args, "id")?)
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "set_correction_rule_enabled" => {
            b.set_correction_rule_enabled(
                required(&args, "id")?,
                args["enabled"].as_bool().ok_or("enabled required")?,
            )
            .map_err(|e| e.to_string())?;
            Value::Null
        }
        "list_vocab_presets" => json!(b.list_vocabulary_presets().map_err(|e| e.to_string())?),
        "save_vocab_presets" => {
            let store = serde_json::from_value(args["store"].clone()).map_err(|e| e.to_string())?;
            b.save_vocabulary_presets(&store)
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "add_vocab" => json!(b
            .add_vocabulary(
                args["phrase"].as_str().ok_or("phrase required")?.to_owned(),
                args["note"].as_str().map(str::to_owned)
            )
            .map_err(|e| e.to_string())?),
        "remove_vocab" => {
            b.remove_vocabulary(args["id"].as_str().ok_or("id required")?)
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "set_vocab_enabled" => {
            b.set_vocabulary_enabled(
                args["id"].as_str().ok_or("id required")?,
                args["enabled"].as_bool().ok_or("enabled required")?,
            )
            .map_err(|e| e.to_string())?;
            Value::Null
        }
        "update_vocab" => {
            b.update_vocabulary_phrase(
                args["id"].as_str().ok_or("id required")?,
                args["phrase"].as_str().ok_or("phrase required")?.into(),
            )
            .map_err(|e| e.to_string())?;
            Value::Null
        }
        "list_style_packs" => json!(b
            .list_style_packs(&b.get_preferences().active_style_pack_id)
            .map_err(|e| e.to_string())?),
        "create_style_pack_from_template" => json!(b
            .create_style_pack(
                serde_json::from_value(args["template"].clone()).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?),
        "save_style_pack" => json!(b
            .update_style_pack(
                serde_json::from_value(args["stylePack"].clone()).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?),
        "preview_style_pack_runtime" => json!(b.preview_style_pack_runtime(
            &serde_json::from_value(args["stylePack"].clone()).map_err(|e| e.to_string())?
        )),
        "set_active_style_pack" => json!(b
            .activate_style_pack(required(&args, "id")?)
            .map_err(|e| e.to_string())?),
        "set_style_pack_enabled" => {
            b.set_style_pack_enabled(
                required(&args, "id")?,
                args["enabled"].as_bool().ok_or("enabled required")?,
            )
            .map_err(|e| e.to_string())?;
            json!(b
                .list_style_packs(&b.get_preferences().active_style_pack_id)
                .map_err(|e| e.to_string())?)
        }
        "read_style_pack_icon" => json!(b
            .read_style_pack_icon(required(&args, "id")?)
            .map_err(|e| e.to_string())?),
        "set_style_pack_icon" => {
            let png: Option<Vec<u8>> =
                serde_json::from_value(args["png"].clone()).map_err(|e| e.to_string())?;
            json!(b
                .set_style_pack_icon(required(&args, "id")?, png.as_deref())
                .map_err(|e| e.to_string())?)
        }
        "reset_builtin_style_pack" => json!(b
            .reset_builtin_style_pack(required(&args, "id")?)
            .map_err(|e| e.to_string())?),
        "delete_style_pack" => {
            b.remove_style_pack(required(&args, "id")?)
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "set_default_polish_mode" | "set_style_enabled" => {
            let mode = serde_json::from_value(args["mode"].clone()).map_err(|e| e.to_string())?;
            let id = openless_core::style_packs::builtin_style_pack_id(mode);
            if command == "set_default_polish_mode" {
                b.activate_style_pack(id).map_err(|e| e.to_string())?;
            } else {
                b.set_style_pack_enabled(id, args["enabled"].as_bool().ok_or("enabled required")?)
                    .map_err(|e| e.to_string())?;
            }
            Value::Null
        }
        "import_style_pack_from_zip" => json!(b
            .import_style_pack_path(std::path::Path::new(required(&args, "zipPath")?))
            .map_err(|e| e.to_string())?),
        "export_style_pack_to_zip" => {
            b.export_style_pack_path(
                required(&args, "id")?,
                std::path::Path::new(required(&args, "targetPath")?),
            )
            .map_err(|e| e.to_string())?;
            json!(required(&args, "targetPath")?)
        }
        "take_splash_playback" => json!(b.take_splash_playback("2")),
        "set_settings" => {
            let prefs = serde_json::from_value(args["prefs"].clone()).map_err(|e| e.to_string())?;
            let _entered = host.runtime.enter();
            json!(
                b.update_settings(
                    prefs,
                    openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                    &openless_core::settings::NoopSettingsRuntime
                )
                .map_err(|e| e.to_string())?
                .preferences
            )
        }
        "update_setting_fields" => {
            let edits = serde_json::from_value(args["edits"].clone()).map_err(|e| e.to_string())?;
            let preferences =
                openless_core::preference_patch::patch_preferences(&b.get_preferences(), &edits)
                    .map_err(|e| e.to_string())?;
            let _entered = host.runtime.enter();
            let outcome = b
                .update_settings(
                    preferences,
                    openless_core::SettingsUpdateOptions::STRICT
                        .at_revision(b.snapshot().preferences_revision),
                    &openless_core::settings::NoopSettingsRuntime,
                )
                .map_err(|e| e.to_string())?;
            json!({"preferences":outcome.preferences,"revision":b.snapshot().preferences_revision})
        }
        "get_default_style_system_prompts" => json!(openless_core::StyleSystemPrompts::default()),
        "list_provider_models" | "validate_provider_credentials" => {
            let kind = match required(&args, "kind")? {
                "asr" => openless_core::ProviderKind::Asr,
                "llm" => openless_core::ProviderKind::Llm,
                "omni" => openless_core::ProviderKind::Omni,
                _ => return Err("invalid provider kind".into()),
            };
            let request = openless_core::ProviderRequest {
                kind,
                channel_id: args["channelId"].as_str().map(str::to_owned),
                thinking_enabled: b.get_preferences().llm_thinking_enabled,
            };
            if command == "list_provider_models" {
                json!(host
                    .runtime
                    .block_on(b.services().provider.list_models(request))
                    .map_err(|e| e.to_string())?)
            } else {
                json!(host
                    .runtime
                    .block_on(b.services().provider.validate(request))
                    .map_err(|e| e.to_string())?)
            }
        }
        "github_device_flow_start" => {
            let flow = host
                .runtime
                .block_on(b.services().marketplace.start_device_flow())
                .map_err(|e| e.to_string())?;
            json!({"flowId":flow.flow_id,"userCode":flow.user_code,"verificationUri":flow.verification_uri,"interval":flow.interval_secs,"expiresIn":flow.expires_in_secs})
        }
        "github_device_flow_poll" => json!(host
            .runtime
            .block_on(
                b.services()
                    .marketplace
                    .poll_device_flow(required(&args, "flowId")?.into())
            )
            .map_err(|e| e.to_string())?),
        "github_device_flow_cancel" => {
            host.runtime
                .block_on(
                    b.services()
                        .marketplace
                        .cancel_device_flow(args["flowId"].as_str().map(str::to_owned)),
                )
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "marketplace_auth_status" => json!(host
            .runtime
            .block_on(b.services().marketplace.auth_status())
            .map_err(|e| e.to_string())?),
        "marketplace_logout" => {
            host.runtime
                .block_on(b.sign_out_account())
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "marketplace_upload" => json!(host
            .runtime
            .block_on(b.services().marketplace.upload(
                required(&args, "packId")?.into(),
                args["originPackId"].as_str().map(str::to_owned)
            ))
            .map_err(|e| e.to_string())?),
        "marketplace_like" => json!(host
            .runtime
            .block_on(
                b.services()
                    .marketplace
                    .toggle_like(required(&args, "packId")?.into())
            )
            .map_err(|e| e.to_string())?),
        "marketplace_my_likes" => json!(host
            .runtime
            .block_on(b.services().marketplace.my_likes())
            .map_err(|e| e.to_string())?),
        "marketplace_my_packs" => json!(host
            .runtime
            .block_on(b.services().marketplace.my_packs())
            .map_err(|e| e.to_string())?),
        "marketplace_delete" => {
            host.runtime
                .block_on(
                    b.services()
                        .marketplace
                        .delete(required(&args, "packId")?.into()),
                )
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "marketplace_list" => json!(host
            .runtime
            .block_on(
                b.services()
                    .marketplace
                    .list(openless_core::MarketplaceQuery {
                        query: args["query"].as_str().map(str::to_owned),
                        sort: args["sort"].as_str().map(str::to_owned),
                        limit: args["limit"].as_u64().map(|v| v as u32)
                    })
            )
            .map_err(|e| e.to_string())?),
        "marketplace_detail" => json!(host
            .runtime
            .block_on(
                b.services()
                    .marketplace
                    .detail(required(&args, "packId")?.into())
            )
            .map_err(|e| e.to_string())?),
        "marketplace_install" => json!(host
            .runtime
            .block_on(
                b.services()
                    .marketplace
                    .install(required(&args, "packId")?.into())
            )
            .map_err(|e| e.to_string())?),
        "list_provider_descriptors" => {
            let kind = match required(&args, "kind")? {
                "asr" => openless_core::ProviderKind::Asr,
                "llm" => openless_core::ProviderKind::Llm,
                "omni" => openless_core::ProviderKind::Omni,
                _ => return Err("invalid provider kind".into()),
            };
            json!(openless_core::provider_rules::provider_descriptors(kind))
        }
        "list_channels" => json!(host
            .runtime
            .block_on(b.list_channels(channel_kind(&args)?))
            .map_err(|e| e.to_string())?),
        "create_channel" => json!(host
            .runtime
            .block_on(b.create_channel(
                channel_kind(&args)?,
                required(&args, "providerType")?.into(),
                args["name"].as_str().unwrap_or("").into()
            ))
            .map_err(|e| e.to_string())?),
        "rename_channel" => {
            host.runtime
                .block_on(b.rename_channel(
                    channel_kind(&args)?,
                    required(&args, "id")?.into(),
                    required(&args, "name")?.into(),
                ))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "delete_channel" => {
            host.runtime
                .block_on(b.delete_channel(channel_kind(&args)?, required(&args, "id")?.into()))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "delete_channel_if_blank" => json!(host
            .runtime
            .block_on(
                b.delete_channel_if_blank(channel_kind(&args)?, required(&args, "id")?.into())
            )
            .map_err(|e| e.to_string())?),
        "set_channel_enabled" => {
            host.runtime
                .block_on(b.set_channel_enabled(
                    channel_kind(&args)?,
                    required(&args, "id")?.into(),
                    args["enabled"].as_bool().ok_or("enabled required")?,
                ))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "set_channel_provider_type" => {
            host.runtime
                .block_on(b.set_channel_provider_type(
                    channel_kind(&args)?,
                    required(&args, "id")?.into(),
                    required(&args, "providerType")?.into(),
                ))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "reorder_channels" => {
            host.runtime
                .block_on(b.reorder_channels(
                    channel_kind(&args)?,
                    serde_json::from_value(args["ids"].clone()).map_err(|e| e.to_string())?,
                ))
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "read_credential" => json!(host
            .runtime
            .block_on(b.read_credential(credential_key(&args)?))
            .map_err(|e| e.to_string())?
            .map(|v| v.into_exposed())),
        "set_credential" => json!(host
            .runtime
            .block_on(b.set_credential(
                credential_key(&args)?,
                openless_core::SecretValue::new(required(&args, "value")?)
            ))
            .map_err(|e| e.to_string())?),
        "__hydrate_credential" => {
            host.credentials
                .hydrate(
                    credential_key(&args)?,
                    openless_core::SecretValue::new(required(&args, "value")?),
                )
                .map_err(|e| e.to_string())?;
            json!(host
                .runtime
                .block_on(b.refresh_and_publish_credentials())
                .map_err(|e| e.to_string())?)
        }
        "__credential_writes" => host
            .credentials
            .dirty_records()
            .map_err(|e| e.to_string())?,
        "__ack_credential_writes" => {
            host.credentials
                .acknowledge_dirty()
                .map_err(|e| e.to_string())?;
            Value::Null
        }
        "set_active_asr_provider" | "set_active_llm_provider" | "set_active_omni_provider" => {
            let slot = match command {
                "set_active_asr_provider" => openless_core::ProviderSlot::Asr,
                "set_active_llm_provider" => openless_core::ProviderSlot::Llm,
                _ => openless_core::ProviderSlot::Omni,
            };
            json!(host
                .runtime
                .block_on(b.set_active_provider(slot, required(&args, "provider")?.into()))
                .map_err(|e| e.to_string())?)
        }
        _ => return Err(format!("HarmonyOS command not implemented: {command}")),
    };
    Ok(value)
}
#[no_mangle]
pub unsafe extern "C" fn openless_invoke(
    command: *const c_char,
    args: *const c_char,
) -> *mut c_char {
    response(
        std::panic::catch_unwind(|| {
            let command = text(command)?;
            let args = serde_json::from_str(&text(args)?).map_err(|e| e.to_string())?;
            let _voice_control = if matches!(
                command.as_str(),
                "__reload_voice_backend" | "__import_voice_configuration" | "start_dictation"
            ) {
                Some(
                    VOICE_CONTROL
                        .lock()
                        .map_err(|_| "voice control lock poisoned")?,
                )
            } else {
                None
            };
            let host = HOST
                .get()
                .ok_or("backend unavailable")?
                .lock()
                .map_err(|_| "backend lock poisoned")?
                .clone();
            if command == "__reload_voice_backend" {
                if !matches!(
                    host.backend.snapshot().dictation.phase,
                    openless_core::DictationPhase::Idle
                        | openless_core::DictationPhase::Completed
                        | openless_core::DictationPhase::Cancelled
                        | openless_core::DictationPhase::Failed
                ) {
                    return Err("voice backend is busy".into());
                }
                let config = host.backend.config();
                let refreshed = build_host(
                    config.data_dir.clone(),
                    config.cache_dir.clone(),
                    host.runtime.clone(),
                )?;
                *HOST
                    .get()
                    .unwrap()
                    .lock()
                    .map_err(|_| "backend lock poisoned")? = refreshed;
                return Ok(Value::Null);
            }
            dispatch(&host, &command, args)
        })
        .unwrap_or_else(|_| Err("native command failed".into())),
    )
}
#[no_mangle]
pub unsafe extern "C" fn openless_feed_pcm(
    session: *const c_char,
    pcm: *const u8,
    length: usize,
) -> *mut c_char {
    response(
        std::panic::catch_unwind(|| {
            if pcm.is_null() || length == 0 || length > 32768 || length % 2 != 0 {
                return Err("invalid PCM frame".into());
            }
            let id = serde_json::from_value(json!(text(session)?)).map_err(|e| e.to_string())?;
            let host = HOST
                .get()
                .ok_or("backend unavailable")?
                .lock()
                .map_err(|_| "backend lock poisoned")?
                .clone();
            host.backend
                .feed_external_pcm(id, std::slice::from_raw_parts(pcm, length))
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        })
        .unwrap_or_else(|_| Err("PCM delivery failed".into())),
    )
}
#[no_mangle]
pub unsafe extern "C" fn openless_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
