//! Framework-independent OpenLess application core.
//!
//! This crate deliberately has no dependency on Tauri, WebView, egui, or eframe.
//! Host applications provide platform integrations through [`ports`] and consume
//! semantic state changes through [`events`].

pub mod activity;
pub mod android_types;
pub mod api;
pub mod asr;
pub mod audio;
pub mod auxiliary;
pub mod cli;
mod cloud_providers;
pub mod coding_agent;
pub mod coding_agent_guard;
pub mod config;
pub mod correction;
pub mod credentials;
pub mod credentials_legacy;
pub mod dictation_context;
pub mod dictation_engine;
pub mod domains;
pub mod edit_plan;
pub mod endpoint_security;
pub mod errors;
pub mod events;
pub mod external_audio;
pub mod history;
pub mod host_document;
mod hotkey_interpreter;
mod less_computer;
pub mod llm_gemini;
mod marketplace;
pub mod model_store;
pub mod net;
pub mod omni;
pub mod output_cleaning;
mod persistence;
pub mod polish;
pub mod ports;
pub mod preferences;
pub mod prompt_compose;
pub mod prompts;
pub mod provider_registry;
mod provider_resolution;
pub mod provider_rules;
mod provider_service;
pub mod provider_transport;
pub mod providers;
mod qa_service;
mod remote_input_service;
mod selection_service;
pub mod selection_voice_intent;
mod selection_voice_service;
pub mod settings;
pub mod shared_types;
pub mod silence_auto_stop;
pub mod streaming_insert;
mod style_pack_archive;
pub mod style_pack_store;
pub mod style_packs;
pub mod testing;
pub mod types;
pub mod vocabulary;
mod voice_session;

mod local_asr_catalog;
mod local_asr_service;
mod shortcut_types;

/// Version of the public host-facing DTO/event/lifecycle contract.
///
/// This is independent from the application release version.  Increment the
/// major component for a breaking host contract change and document the
/// migration in `docs/linux-egui-backend-contract.md`.
pub const BACKEND_CONTRACT_VERSION: &str = "2.0.0";

pub fn require_backend_contract_version(version: &str) -> Result<(), errors::BackendError> {
    if version == BACKEND_CONTRACT_VERSION {
        Ok(())
    } else {
        Err(errors::BackendError::new(
            errors::BackendErrorCode::InvalidArgument,
            format!("unsupported backend contract version: {version}"),
        ))
    }
}

/// Versioned host-facing contract used by the Linux UI crate.
///
/// Tauri compatibility code may continue to use the broader crate surface
/// while it is migrated. New non-Tauri hosts should depend on this module so
/// repository and implementation details do not accidentally become UI API.
pub mod contract {
    pub use crate::android_types::{
        AndroidInsertStrategy, AndroidOverlayActivationMode, AndroidOverlayCancelSwipeDirection,
        AndroidOverlayLeftSwipeAction, AndroidOverlayTrigger,
    };
    pub use crate::auxiliary::{
        AsrCallLabel, AuxiliaryApi, RepolishRequest, RetranscriptionFailure, RetranscriptionResult,
    };
    pub use crate::coding_agent::{
        CodingAgentAvailability, CodingAgentDetectRequest, CodingAgentModelsRequest,
        CodingAgentPermissionMode, CodingAgentProvider, CodingAgentRequest, CodingAgentRunOutcome,
        CodingAgentRunResult, CodingAgentRunner, CodingAgentTestRequest, CodingAgentTestStatus,
        CommandRisk, CommandRiskAssessment, McpHealth, McpServerStatus,
    };
    pub use crate::domains::{
        BackendServices, CodingAgentApi, LessComputerApi, LocalAsrApi, LocalAsrModel,
        LocalAsrModelCard, LocalAsrRemoteFile, LocalAsrRemoteInfo, LocalAsrRuntimeStatus,
        LocalAsrSettings, LocalAsrStorageSettings, LocalAsrTestResult, MarketplaceApi,
        MarketplaceAuthStatus, MarketplaceDetail, MarketplaceLikeResult, MarketplaceListItem,
        MarketplaceMyPackItem, MarketplaceQuery, MarketplaceUploadResult, MicrophoneDevice,
        OAuthDeviceFlow, OAuthPollResult, PlatformApi, ProviderApi, ProviderCheckResult,
        ProviderKind, ProviderModelsResult, ProviderRequest, QaApi, QaInput, QaMessage, QaPhase,
        QaSnapshot, RemoteInputApi, RemoteInputConfig, RemoteInputStatus, SelectionApi,
        SelectionCapture, SelectionPhase, SelectionPolishRequest, SelectionSnapshot,
        SelectionVoiceApi, SelectionVoiceApplyOutcome, SelectionVoiceApplyTicket,
        SelectionVoiceDisposition, SelectionVoiceEditAction, SelectionVoiceEditPreviewResult,
        SelectionVoiceEditRequest, SelectionVoiceInstructionRequest, SelectionVoiceIntentPrompt,
        SelectionVoicePhase, SelectionVoicePreview, SelectionVoicePreviewUpdate,
        SelectionVoiceSnapshot,
    };
    pub use crate::host_document::{
        edit_is_within_typed_text, is_vocab_worthy, learned_rule, minimal_edit, plan_window,
        utf16_offset_to_char_offset, window_around_cursor, DocumentWindow, EditPair, LearnedRule,
        WindowSpan,
    };
    pub use crate::local_asr_service::{
        LocalAsrRuntimeLease, ModelPrepareProgressSink, ModelRuntimeAdapter, NativeModelState,
        StorageRebind,
    };
    pub use crate::model_store::{
        extract_archive_safely, merge_hf_tree_pages, merge_hf_tree_pages_with_base,
        model_mirror_base, parse_hf_tree_page, validate_model_id, validate_model_path,
        validate_model_url, DownloadProgressSink, ModelArchiveSpec, ModelCacheStatus, ModelCard,
        ModelCatalog, ModelCatalogEntry, ModelContentRange, ModelDownloadPhase,
        ModelDownloadProgress, ModelFile, ModelFileMapping, ModelFileSelector, ModelHttpMetadata,
        ModelManifest, ModelStore, ModelStoreConfig, ModelTransport, ModelTransportRequest,
        ModelTransportResponse, ReqwestModelTransport,
    };
    pub use crate::provider_rules::{AuthRequirement, ProviderDescriptor, ValidationProbe};
    pub use crate::provider_transport::{
        ProviderCancellation, ProviderTransport, ProviderTransportError, ProviderTransportRequest,
        ProviderTransportResponse, ReqwestProviderTransport,
    };
    pub use crate::remote_input_service::{
        constant_time_eq, validate_pairing_pin, RemoteFrameCodec, RemoteStreamSequence,
        REMOTE_INPUT_PAIRING_PIN_LEN,
    };
    pub use crate::shared_types::{
        ChineseScriptPreference, ComboBinding, HotkeyBinding, HotkeyMode, HotkeyTrigger,
        MacosNewlineMode, OutputLanguagePreference, PasteShortcut, PipelineMode, ShortcutBinding,
        StylePackHotkey, ThemeMode, UpdateChannel, UserPreferences, WindowsInsertionMode,
        WindowsSendInputNewlineMode,
    };
    pub use crate::style_packs::{CustomStylePrompts, StyleSystemPrompts};
    pub use crate::testing::{
        FakeProviderTransport, FakeProviderTransportOutcome, FixedClock, FixtureAudioRecorder,
        FixtureDictationEngine, FixtureEngineAction, FixtureInsertionAction,
        FixtureSelectionAction, FixtureSelectionRuntime, FixtureTextInserter, FixtureTextPolisher,
        FixtureTranscriptionEngine, LinuxCapabilityFixture, RecordingHostActions,
        RecordingRemoteInputRuntime,
    };
    pub use crate::{
        require_backend_contract_version, ActivityDay, AudioConsumer, AudioRecorder, BackendConfig,
        BackendDependencies, BackendError, BackendErrorCode, BackendEvent, BackendEventKind,
        BackendSnapshot, CliDispatchOutcome, CliIntent, Clock, CorrectionRule, CredentialKey,
        CredentialMetadata, CredentialNamespace, CredentialStore, CredentialsStatus,
        DictationContext, DictationEngine, DictationHotkeyDispatchOptions, DictationHotkeyEdge,
        DictationInsertStatus, DictationPhase, DictationResult, DictationSession,
        DictationStartOptions, DictationStateSnapshot, DictionaryEntry, DirectoryResourceResolver,
        DownloadProgress, EngineFailure, EngineFailureStage, EngineProgress, EngineProgressSink,
        EngineResult, EngineStage, EventRecvError, EventSubscription, HistoryChange,
        HistoryInsertStatus, HistorySource, HostAction, HostActions, HostContextAdapter,
        HostContextCapture, HotkeyRuntimeTarget, HotkeyStatus, InMemoryCredentialStore,
        InsertFallbackPayload, InsertOutcome, LessComputerEvent, LessComputerEventKind,
        LessComputerHotkeyAction, LessComputerVoiceSession, LocalAsrMirror, LocalAsrModelId,
        LocalAsrRuntime, LocalAsrTarget, NotificationLevel, NotificationPayload, OpenLessBackend,
        PendingCorrection, PermissionSnapshot, PermissionState, PlatformCapabilities, PolishDelta,
        PolishFailurePolicy, PolishMode, PolishOutput, ProviderService, QaVoiceCaptureResult,
        QaVoiceCaptureSession, RecordingArchive, RecordingControlAction, RecordingControlRequest,
        RecordingControlSink, RecordingEvent, RecordingPlan, RecordingProgressSink,
        ResourceResolver, RuleSource, SecretValue, SelectionPolishOutputMode,
        SelectionVoiceIntentMode, SelectionVoiceManualIntent, SessionId, SettingsCollisionPolicy,
        SettingsEffectFailure, SettingsEffectKind, SettingsEffectPlan, SettingsEffectReceipt,
        SettingsRuntime, SettingsUpdateOptions, SettingsUpdateOutcome, SettingsValueChange,
        StartupSnapshot, StylePack, StylePackChange, StylePackExample, StylePackKind, TaskSpawner,
        TextInserter, TextPolisher, TextStreamChunk, TextStreamSink, TokioTaskSpawner,
        TranscriptAccumulator, TranscriptDelta, TranscriptOutput, TranscriptionEngine,
        TranscriptionSession, VocabPreset, VocabPresetStore, VocabularyChange,
        VoiceTranscriptionSession, BACKEND_CONTRACT_VERSION, DICTATION_SAMPLE_RATE,
    };
}

pub use activity::{ActivityDay, ActivityStore, DayStats};
pub use api::{
    BackendRepositories, BackendSnapshot, CliDispatchOutcome, DictationHotkeyDispatchOptions,
    DictationHotkeyEdge, LessComputerHotkeyAction, LessComputerVoiceSession, OpenLessBackend,
    QaVoiceCaptureResult, QaVoiceCaptureSession, StartupSnapshot, VoiceTranscriptionSession,
};
pub use audio::{encode_dictation_wav, NormalizedPcmChunk, PcmNormalizer, DICTATION_SAMPLE_RATE};
pub use auxiliary::{
    AsrCallLabel, AuxiliaryApi, RepolishRequest, RetranscriptionFailure, RetranscriptionResult,
};
pub use cli::{
    decode_launch_intent, encode_launch_intent, parse_cli_intent, CliIntent, LaunchIntent,
};
pub use cloud_providers::{
    answer_qa_with_context, SharedAuxiliaryTextPolisher, SharedCloudTextPolisher,
    SharedCloudTranscriptionEngine, SharedOmniDictationEngine, SHARED_CLOUD_ASR_PROVIDER_TYPES,
    SHARED_CLOUD_LLM_PROVIDER_TYPES, SHARED_OMNI_PROVIDER_TYPES,
};
pub use coding_agent::*;
pub use coding_agent_guard::*;
pub use config::{
    BackendConfig, BackendDependencies, Clock, SystemClock, TaskSpawner, TokioTaskSpawner,
};
pub use correction::{apply_correction_rules, CorrectionRuleStore};
pub use credentials::{
    ChannelKind, ChannelMutation, ChannelMutationResult, ChannelSummary, ChannelTestSummary,
    CredentialDirectory, CredentialKey, CredentialMetadata, CredentialMetadataStore,
    CredentialNamespace, CredentialStore, InMemoryCredentialStore, ProviderChannelId, ProviderSlot,
    ProviderType, SecretValue, UnsupportedCredentialStore,
};
pub use dictation_context::{
    build_asr_prompt, eligible_polish_context_turns, DictationAudioSource, DictationContext,
    DictationInsertionContext, DictationPolishContext, DictationStartOptions, DictationStopOptions,
    PolishHistoryTurn, ProviderInvocation, RecordingPlan, ASR_PROMPT_CHAR_BUDGET,
};
pub use dictation_engine::{PipelineDictationEngine, PolishFailurePolicy};
pub use domains::*;
pub use edit_plan::{
    apply_edit_plan, parse_edit_plan, parse_edit_plan_json, parse_edit_plan_xml, EditApplyError,
    EditOperation, EditPlan, RegexFlags,
};
pub use errors::{BackendError, BackendErrorCode};
pub use events::{
    BackendEvent, BackendEventKind, BackendEventPublisher, CodingAgentStreamEvent, EventRecvError,
    EventReplay, EventSubscription, LessComputerEvent, LessComputerEventKind,
    LessComputerVoicePhase, LocalAsrDownloadPhase, LocalAsrDownloadProgress, LocalAsrPreparePhase,
    LocalAsrPrepareProgress, LocalAsrRuntimeKind, QaRecordingLevel, QaStateEvent, QaStateKind,
    RecordingControlAction, RecordingControlRequest, RemoteInputErrorEvent,
    RemoteInputRuntimeEvent,
};
pub use external_audio::{AudioRecorderRouter, ExternalAudioRecorder};
pub use history::{HistoryStore, HISTORY_CAP};
pub use less_computer::LessComputerService;
pub use local_asr_catalog::{
    normalize_foundry_language_hint, normalize_sherpa_language_hint, FoundryRuntimeSource,
    LocalAsrExecutionMode, LocalAsrMirror, LocalAsrModelId, LocalAsrRuntime, LocalAsrTarget,
    SherpaModelFamily,
};
pub use local_asr_service::{
    LocalAsrRuntimeLease, ModelPrepareProgressSink, ModelRuntimeAdapter, NativeModelState,
    StorageRebind,
};
pub use marketplace::{MarketplaceConfig, MARKETPLACE_BASE_URL, MARKETPLACE_GITHUB_TOKEN_ACCOUNT};
pub use model_store::{
    extract_archive_safely, merge_hf_tree_pages, merge_hf_tree_pages_with_base, model_mirror_base,
    parse_hf_tree_page, validate_model_path, validate_model_url, DownloadProgressSink,
    ModelArchiveSpec, ModelCacheStatus, ModelCard, ModelCatalog, ModelCatalogEntry,
    ModelContentRange, ModelDownloadPhase, ModelDownloadProgress, ModelFile, ModelFileMapping,
    ModelFileSelector, ModelHttpMetadata, ModelManifest, ModelStore, ModelStoreConfig,
    ModelTransport, ModelTransportRequest, ModelTransportResponse, ReqwestModelTransport,
    MODEL_PARTIAL_INDEX, MODEL_READY_SENTINEL,
};
pub use ports::{
    ActiveRecording, AudioConsumer, AudioRecorder, DictationEngine, DirectoryResourceResolver,
    EditObservationAdapter, EditObservationSink, EngineFailure, EngineFailureStage, EngineProgress,
    EngineProgressSink, EngineResult, EngineStage, HostAction, HostActions, HostContextAdapter,
    HostContextCapture, InsertOutcome, InsertWriteResult, NoopEditObservationAdapter,
    NoopHostActions, NoopHostContextAdapter, PolishOutput, RecordingArchive, RecordingControlSink,
    RecordingEvent, RecordingProgressSink, ResourceResolver, TextInserter, TextInsertionSession,
    TextPolisher, TextStreamChunk, TextStreamSink, TranscriptOutput, TranscriptionEngine,
    TranscriptionSession, UnsupportedTextInserter, VoiceCapture,
};
pub use preferences::PreferencesStore;
pub use prompt_compose::{
    assemble_polish_system_prompt, build_hotword_block, build_polish_translate_system_prompt,
    compose_hotword_block_preview, compose_polish_prompts, compose_qa_system_prompt,
    compose_system_prompt, compose_translate_prompts, context_premise,
    split_polish_translate_output, PolishSystemPromptAssembly, POLISH_TRANSLATE_SRC_MARKER,
    POLISH_TRANSLATE_TGT_MARKER,
};
pub use provider_registry::{DictationEngineRouter, TextPolisherRouter, TranscriptionRouter};
pub use provider_rules::{AuthRequirement, ProviderDescriptor, ValidationProbe};
pub use provider_service::ProviderService;
pub use provider_transport::{
    ProviderCancellation, ProviderTransport, ProviderTransportError, ProviderTransportRequest,
    ProviderTransportResponse, ReqwestProviderTransport,
};
pub use providers::{
    OpenAiBatchTranscriptionEngine, OpenAiChatPolisher, OpenAiChatPolisherConfig,
    OpenAiTranscriptionConfig,
};
pub use qa_service::QaService;
pub use remote_input_service::{
    constant_time_eq, validate_pairing_pin, RemoteFrameCodec, RemoteInputService,
    RemoteStreamSequence, REMOTE_INPUT_MAX_PCM_FRAME_BYTES, REMOTE_INPUT_PAIRING_PIN_LEN,
};
pub use selection_voice_intent::SelectionVoiceIntent;
pub use settings::*;
pub use shared_types::{
    CapsulePayload, CapsuleState, CapsuleStyle, CredentialsStatus, HotkeyMode, HotkeyStatus,
    PendingCorrection, PlatformCapabilities, SelectionPolishOutputMode, UserPreferences,
};
pub use shortcut_types::{
    binding_from_legacy_trigger, binding_requires_side_aware_hook, bindings_overlap,
    is_side_specific_modifier_tag, legacy_modifier_trigger, normalize_side_modifier_tag,
    reconcile_hotkey_collisions, reject_bare_shift_dictation_shortcut,
    reject_dictation_qa_hotkey_overlap, reject_dictation_translation_hotkey_overlap,
    reject_hotkey_collisions, reject_modifier_only_action_shortcut,
    reject_non_dictation_side_specific_shortcuts, reject_qa_less_computer_hotkey_overlap,
    reject_qa_open_app_hotkey_overlap, reject_qa_switch_style_hotkey_overlap,
    reject_qa_translation_hotkey_overlap, reject_selection_polish_hotkey_collisions,
    reject_side_specific_non_dictation, reject_style_pack_hotkey_conflicts,
    sync_dictation_hotkey_legacy_fields, validate_shortcut_binding, ShortcutBindingError,
    SIDE_SPECIFIC_NON_DICTATION_MSG,
};
pub use silence_auto_stop::{SilenceAutoStop, SilenceDecision};
pub use streaming_insert::{
    append_typed_prefix, streaming_insert_eligible, StreamingInsertState,
    STREAMING_FLUSH_INTERVAL_MS,
};
pub use style_pack_archive::{
    validate_style_pack_archive_bytes, STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES,
};
pub use style_pack_store::{
    enabled_modes_from_style_packs, migrate_style_packs_from_preferences,
    sync_style_pack_preferences, StylePackStore,
};
pub use style_packs::*;
pub use types::InsertStatus as DictationInsertStatus;
pub use types::{
    CorrectionRule, DictationPhase, DictationResult, DictationSession, DictationStateSnapshot,
    DictionaryEntry, DownloadProgress, HistoryChange, HistoryInsertStatus, HistorySource,
    InsertFallbackPayload, NotificationLevel, NotificationPayload, PermissionSnapshot,
    PermissionState, PolishDelta, PolishMode, PreferencesChange, RuleSource,
    SelectionVoiceIntentMode, SelectionVoiceManualIntent, SessionId, StylePackChange,
    TranscriptAccumulator, TranscriptDelta, VocabPreset, VocabPresetStore, VocabularyChange,
};
pub use vocabulary::{list_vocab_presets, save_vocab_presets, DictionaryStore};
