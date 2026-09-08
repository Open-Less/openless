import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), 'utf8');
const [
  types,
  qaPanel,
  remoteIpc,
  remoteSection,
  selectionVoiceIpc,
  remoteCommand,
  tauriEvents,
  lessComputerIpc,
  lessComputerPanel,
  qaCommand,
  remoteServer,
  remoteInputCore,
  coordinator,
  coordinatorDictation,
  coordinatorCapsule,
  coordinatorHotkeys,
  tauriCoordinatorHost,
  coreAdapters,
  providersCommand,
  qaCore,
  selectionVoiceCore,
  coreApi,
  qaAdapter,
  selectionVoiceCoordinator,
  dictionaryCommand,
  stylePacksCommand,
  androidNativeBridge,
  androidKotlinBridge,
  androidOverlayService,
  appShell,
  providerSection,
  channelList,
  providerLabels,
  historyCommand,
  credentialCommands,
  credentialPersistence,
  hostDocumentMacos,
  linuxFcitx,
  localAsrPage,
  localAsrCore,
  credentialCore,
  mobileHotkey,
  mobileSelection,
] =
  await Promise.all([
    read('src/lib/types.ts'),
    read('src/pages/QaPanel.tsx'),
    read('src/lib/ipc/remote-server.ts'),
    read('src/pages/settings/RemoteInputSection.tsx'),
    read('src/lib/ipc/selection-voice-preview.ts'),
    read('src-tauri/src/commands/remote_input.rs'),
    read('src-tauri/src/tauri_events.rs'),
    read('src/lib/ipc/less-computer.ts'),
    read('src/pages/LessComputerPanel.tsx'),
    read('src-tauri/src/commands/qa.rs'),
    read('src-tauri/src/remote_server/mod.rs'),
    read('crates/openless-core/src/remote_input_service.rs'),
    read('src-tauri/src/coordinator.rs'),
    read('src-tauri/src/coordinator/dictation_core.rs'),
    read('src-tauri/src/coordinator/capsule_focus.rs'),
    read('src-tauri/src/coordinator/hotkey_loops.rs'),
    read('src-tauri/src/tauri_coordinator_host.rs'),
    read('src-tauri/src/core_adapters.rs'),
    read('src-tauri/src/commands/providers.rs'),
    read('crates/openless-core/src/qa_service.rs'),
    read('crates/openless-core/src/selection_voice_service.rs'),
    read('crates/openless-core/src/api.rs'),
    read('src-tauri/src/qa_adapter.rs'),
    read('src-tauri/src/coordinator/selection_voice_session.rs'),
    read('src-tauri/src/commands/dictionary.rs'),
    read('src-tauri/src/commands/style_packs.rs'),
    read('src-tauri/src/android/native_bridge.rs'),
    read('android/kotlin/OpenLessNative.kt'),
    read('android/kotlin/OpenLessOverlayService.kt'),
    read('src/App.tsx'),
    read('src/pages/settings/ProvidersSection.tsx'),
    read('src/pages/settings/ChannelList.tsx'),
    read('src/pages/settings/shared.tsx'),
    read('src-tauri/src/commands/history.rs'),
    read('src-tauri/src/commands/credentials.rs'),
    read('src-tauri/src/persistence/credentials.rs'),
    read('src-tauri/src/host_document/macos.rs'),
    read('linux-egui/src/fcitx5.rs'),
    read('src/pages/LocalAsr/index.tsx'),
    read('crates/openless-core/src/local_asr_service.rs'),
    read('crates/openless-core/src/credentials.rs'),
    read('src-tauri/src/mobile_stubs/hotkey.rs'),
    read('src-tauri/src/mobile_stubs/selection.rs'),
  ]);

for (const kind of ['awaiting_approval', 'cancelled', 'error']) {
  assert.match(types, new RegExp(`\\| '${kind}'`), `QaStateKind must retain ${kind}`);
  assert.match(qaPanel, new RegExp(`case '${kind}':`), `QaPanel must handle ${kind}`);
}
for (const field of [
  'sessionId',
  'messages',
  'selectionPreview',
  'chunk',
  'error',
  'editInstructionMode',
  'editApplyAvailable',
  'editRevertAvailable',
  'approvalToken',
]) {
  assert.match(types, new RegExp(`\\b${field}\\?`), `QaStatePayload is missing ${field}`);
}
assert.match(qaPanel, /listen<QaStatePayload>\('qa:state'/, 'QA must consume the typed state event');

const remoteStatus = remoteIpc.match(/export interface RemoteInputStatus \{([\s\S]*?)\n\}/)?.[1];
assert.ok(remoteStatus, 'RemoteInputStatus interface must exist');
const remoteFields = [...remoteStatus.matchAll(/^\s+(\w+):/gm)].map((match) => match[1]);
assert.deepEqual(
  remoteFields,
  ['running', 'starting', 'port', 'pin', 'urls', 'urlsStale'],
  'the explicit remote status command must preserve the beta secret wire shape',
);
assert.match(remoteIpc, /invokeOrMock\("get_remote_input_status"/, 'remote status command name drifted');
assert.match(remoteSection, /listen\('remote-input:running'/, 'remote running event must refresh status');
assert.match(remoteSection, /listen\('remote-input:error'/, 'remote errors must remain visible');
assert.match(
  remoteCommand,
  /read_pairing_pin\(\)[\s\S]*?map_remote_input_status\(status, pin\)/,
  'PIN must only be added by the explicit status command conversion',
);
assert.match(
  tauriEvents,
  /QaStateEvent::from_snapshot\(&snapshot\)/,
  'lagged resync must rebuild QA from the Core snapshot',
);
assert.match(
  tauriEvents,
  /RemoteInputRuntimeEvent::from\(&status\)/,
  'lagged resync must rebuild Remote Input without a PIN',
);

for (const [command, args] of [
  ['get_selection_voice_preview', '{ qaSessionId }'],
  ['confirm_selection_voice_preview', '{ text, qaSessionId }'],
  ['revert_selection_voice_preview', '{ qaSessionId }'],
]) {
  assert(
    selectionVoiceIpc.includes(`'${command}', ${args}`),
    `${command} wire drifted`,
  );
}

for (const field of ['events', 'oldestSequence', 'latestSequence', 'truncated']) {
  assert.match(types, new RegExp(`\\b${field}[?:]`), `LessComputerSyncResult is missing ${field}`);
}
assert.match(
  lessComputerIpc,
  /less_computer_sync[\s\S]*?\{ afterSequence \}/,
  'Less Computer sync must send its applied sequence waterline',
);
assert.match(
  qaCommand,
  /less_computer_sync[\s\S]*?after_sequence: Option<u64>[\s\S]*?LessComputerEventReplay/,
  'the Tauri sync command must return bounded replay metadata',
);
assert.match(
  lessComputerPanel,
  /reconcileLessComputerReplay\(lcAppliedSeq, replay, pending\)/,
  'Less Computer must merge replay with events buffered during listener installation',
);
assert.match(
  lessComputerPanel,
  /reconciled\.reset\) \{\s*setTurns\(\[\]\);\s*setVoice\(null\)/,
  'a truncated replay must reset both the conversation and voice presentation',
);

assert.match(
  remoteServer,
  /\.authenticate\(connection_id[^]*?match authed \{[^]*?RemoteAuthResult::BadPin[^]*?return;[^]*?RemoteAuthResult::Locked[^]*?return;/,
  'Remote Input must delegate authentication to Core and reject invalid results before handling controls',
);
assert.match(
  remoteInputCore,
  /authenticate_inner[^]*?constant_time_eq\([^]*?candidate\.expose_secret\(\)[^]*?expected\.expose_secret\(\)/,
  'Remote Input PIN verification must remain constant-time inside Core',
);
assert.doesNotMatch(
  remoteServer,
  /constant_time_eq|PIN_(?:MAX_FAILS|LOCK_SECS|GLOBAL_MAX_FAILS)/,
  'the Tauri WebSocket adapter must not retain authentication or lockout policy',
);
assert.match(
  remoteServer,
  /apply_remote_control\([^]*?state\.backend\.services\(\)\.remote_input\.as_ref\(\)/,
  'Remote Input WebSocket control must use the Core lifecycle',
);
const remoteFrameParser = remoteServer.match(
  /fn parse_audio_frame\([^]*?\n\}/,
)?.[0];
assert.ok(remoteFrameParser, 'the Remote Input WebSocket frame adapter must remain present');
assert.match(
  remoteFrameParser,
  /openless_core::RemoteFrameCodec::decode\(frame\)/,
  'Remote Input WebSocket frames must be decoded by openless-core',
);
assert.doesNotMatch(
  remoteFrameParser,
  /b"OL20"|Uuid::from_slice|u64::from_be_bytes/,
  'the Tauri WebSocket adapter must not retain Remote Input frame policy',
);
assert.match(
  remoteServer,
  /\.remote_input[^]*?\.disconnect\(connection_id\)/,
  'Remote Input WebSocket disconnect must release the Core connection',
);

const coordinatorBusinessSources = [
  coordinator,
  coordinatorDictation,
  coordinatorCapsule,
  coordinatorHotkeys,
].join('\n');
assert.doesNotMatch(
  coordinatorBusinessSources,
  /\.emit\(\s*["']local-asr-token["']/,
  'local-asr-token must be derived from typed Core events by the centralized Tauri bridge',
);
assert.doesNotMatch(
  `${remoteServer}\n${tauriEvents}`,
  /listen_any\("remote:result"|\.emit\("remote:result"/,
  'Remote Input results must never use an ownerless global broadcast',
);
assert.match(
  remoteServer,
  /event\.session_id != \*remote_session_id/,
  'Remote Input may only forward events belonging to the authenticated connection session',
);
assert.match(
  coordinatorDictation,
  /\.dispatch_dictation_hotkey_edge\(edge\)/,
  'Tauri dictation hotkeys must delegate the complete session transition to Core',
);
assert.match(
  coordinatorDictation,
  /async fn dispatch\([^]*?Result<openless_core::CliDispatchOutcome,\s*openless_core::BackendError>/,
  'the hotkey adapter must retain the typed Core transition result',
);
assert.match(
  coordinatorHotkeys,
  /\.update_dictation_translation_requested\(true\)/,
  'translation hotkeys must update the current Core session',
);
assert.doesNotMatch(
  coordinatorBusinessSources,
  /inner\.translation_active|translation_active:\s*AtomicBool|fn finish_bookkeeping\(/,
  'translation intent must not survive in Host state or depend on one stop entry for cleanup',
);
const coordinatorInner = coordinator.match(/struct Inner \{([^]*?)\n\}/)?.[1];
assert.ok(coordinatorInner, 'Coordinator Inner must remain inspectable by the architecture contract');
assert.doesNotMatch(
  coordinatorInner,
  /AppHandle|AppHandleSlot|\bapp\s*:/,
  'Coordinator must not retain a Tauri AppHandle',
);
assert.match(
  coordinatorInner,
  /host: crate::tauri_coordinator_host::TauriCoordinatorHost/,
  'Coordinator must depend on the explicit Tauri host Module',
);
assert.doesNotMatch(
  coordinatorBusinessSources,
  /\.emit(?:_to)?\(/,
  'Coordinator modules must publish typed Core events or call semantic Tauri host actions',
);
assert.match(
  tauriCoordinatorHost,
  /struct TauriCoordinatorHost[^{]*\{[^]*?AppHandleSlot/,
  'the late-bound AppHandle must be owned by the Tauri host Module',
);
assert.doesNotMatch(
  tauriCoordinatorHost,
  /crate::coordinator::(?:Inner|capsule_focus)/,
  'the Tauri host must not reach back into Coordinator internals to operate capsule windows',
);
assert.doesNotMatch(
  coordinator,
  /use tauri::AppHandle|fn bind_app\s*\(/,
  'Coordinator must expose its Tauri host seam instead of accepting AppHandle directly',
);
assert.match(
  coordinator,
  /\nmod capsule_focus;/,
  'capsule_focus must remain private to the Coordinator module',
);
assert.doesNotMatch(
  coreAdapters,
  /managed_coordinator|try_state::<Arc<crate::coordinator::Coordinator>>/,
  'Core adapters must receive narrow shared host state instead of reaching back into Coordinator',
);
for (const legacyProviderCopy of [
  /#\[cfg\(any\(\)\)\]/,
  /\bTauriCloudTranscriptionEngine\b/,
  /\bTauriCloudTextPolisher\b/,
  /\bTauriOmniDictationEngine\b/,
  /\bbuild_tauri_omni_provider\b/,
]) {
  assert.doesNotMatch(
    coreAdapters,
    legacyProviderCopy,
    'Tauri must not retain a disabled copy of provider protocol construction owned by openless-core',
  );
}
assert.match(
  providersCommand,
  /\.provider\s*\n?\s*\.validate\(/,
  'Tauri provider validation command must delegate to Core ProviderApi',
);
assert.match(
  providersCommand,
  /\.provider\s*\n?\s*\.list_models\(/,
  'Tauri provider model-list command must delegate to Core ProviderApi',
);
for (const forbiddenProviderBusinessToken of [
  /CredentialsVault/,
  /ProviderScope/,
  /ProviderConfig/,
  /reqwest::/,
  /validate_provider_service/,
  /list_provider_models_service/,
  /tokio::time::timeout/,
  /build_active_omni_provider/,
]) {
  assert.doesNotMatch(
    providersCommand,
    forbiddenProviderBusinessToken,
    'Tauri provider commands must not recreate Core credential/protocol business logic',
  );
}
assert.match(
  coordinatorInner,
  /hotkey_status: Arc<Mutex<HotkeyStatus>>/,
  'Coordinator and the platform adapter must share one hotkey status slot',
);
assert.match(
  coordinatorInner,
  /qa_context: Arc<TauriQaHostContext>/,
  'Coordinator and QA adapters must share one QA host context',
);
assert.match(
  qaAdapter,
  /fn is_panel_visible\(&self\)[^]*?panel_visible\.load\(Ordering::Acquire\)/,
  'QA host visibility must be read from the shared atomic context',
);
assert.match(
  selectionVoiceCoordinator,
  /\.process_transcript\(session_id, transcript\)/,
  'the Tauri selection-voice adapter must submit raw ASR text to the Core workflow',
);
assert.match(
  selectionVoiceCoordinator,
  /\.route_disposition\(disposition\)/,
  'the Tauri selection-voice adapter must consume the complete Core-owned QA/Edit route',
);
for (const forbiddenBusinessToken of [
  /apply_correction_rules/,
  /list_correction_rules/,
  /polish_text/,
  /translate_text/,
  /parse_edit_plan/,
  /apply_edit_plan/,
  /voice_edit_system_prompt/,
  /selection_voice_intent_classification_prompt/,
  /infer_selection_voice_translation_target/,
  /selection_polish_output_mode/,
]) {
  assert.doesNotMatch(
    selectionVoiceCoordinator,
    forbiddenBusinessToken,
    'Selection Voice correction, prompting, intent, EditPlan and delivery policy must remain in openless-core',
  );
}
assert.match(
  qaCore,
  /\.edit_preview\(crate::domains::SelectionVoiceEditRequest/,
  'the Core QA service must own edit-vs-answer routing and preview generation',
);
assert.match(
  selectionVoiceCore,
  /qa\.submit_selection_edit\(session_id, selection, instruction\)/,
  'Selection Voice must pass the pre-focus capture into the Core QA use-case',
);
assert.match(
  qaAdapter,
  /fn prepare_selection_edit\([^]*?Do not call `capture_turn`[^]*?prebound_selection_voice_session_id/,
  'the QA host adapter must preserve the already-bound Selection Voice target',
);
assert.doesNotMatch(
  qaAdapter,
  /request\.edit_instruction_mode|\.edit_preview\(|parse_edit_plan|apply_edit_plan|generate_edit_plan|voice_edit_system_prompt/,
  'the QA adapter must not decide edit-vs-answer or recreate the Core selection-edit workflow',
);
for (const mode of ['Hold', 'Toggle', 'Auto']) {
  assert.match(
    selectionVoiceCore,
    new RegExp(`HotkeyMode::${mode}`),
    `Selection Voice ${mode} policy must remain in openless-core`,
  );
}
// The trailing, explicitly test-only fixture configures HotkeyMode to exercise
// the real Core path. The shipping adapter must still contain no mode policy.
const selectionVoiceProduction = selectionVoiceCoordinator.replace(
  /\n#\[cfg\(all\(test, target_os = "windows"\)\)\]\r?\nmod tests \{[^]*\n\}\s*$/,
  '',
);
assert.doesNotMatch(
  selectionVoiceProduction,
  /HotkeyMode|selection_polish_output_mode|classify_selection_voice_intent|SelectionVoiceIntent::/,
  'the Tauri selection-voice coordinator must only execute Core actions and host effects',
);
assert.match(
  qaAdapter,
  /\.start_qa_voice_capture\(/,
  'the QA adapter must use the Core-owned capture and transcription session',
);
assert.doesNotMatch(
  qaAdapter,
  /\b(?:AudioRecorder|TranscriptionEngine|ActiveRecording|TauriQaPcmBuffer|TauriQaAudioFanout)\b/,
  'the QA adapter must not own recorder, transcription, or PCM lifecycle implementations',
);
assert.match(
  coreApi,
  /struct QaRecordingProgress[^]*?SilenceAutoStop[^]*?qa\.recording_fault/,
  'QA silence and typed recording-fault routing must be Core-owned',
);
assert.match(
  coreApi,
  /struct SelectionVoiceRecordingProgress[^]*?SilenceAutoStop[^]*?selection_voice\.recording_fault/,
  'Selection Voice silence and typed recording-fault routing must be Core-owned',
);
assert.match(
  coreApi,
  /struct LessComputerRecordingProgress[^]*?SilenceAutoStop[^]*?less_computer\.capture_fault/,
  'Less Computer silence and typed recording-fault routing must be Core-owned',
);
assert.doesNotMatch(
  qaAdapter,
  /\.recording_fault\(/,
  'the QA host adapter must only forward recorder effects, never decide fault state',
);
assert.doesNotMatch(
  qaAdapter,
  /try_state::<Arc<crate::coordinator::Coordinator>>|Tauri coordinator state is unavailable/,
  'the QA adapter must use its narrow host callback instead of looking up Coordinator state',
);
assert.match(
  qaAdapter,
  /set_selection_voice_target_binder|bind_selection_voice_target/,
  'the QA adapter must expose the narrow opaque-target host seam',
);

for (const method of [
  'accept_pending_correction',
  'reject_pending_correction',
  'dismiss_pending_corrections',
]) {
  assert.match(
    dictionaryCommand,
    new RegExp(`core\\.${method}\\(`),
    `vocabulary suggestion command must delegate ${method} to Core`,
  );
}
assert.match(
  dictionaryCommand,
  /refresh_vocab_suggestion_presentation/,
  'the Tauri command may only pass Core suggestion presence to the host presentation seam',
);
assert.doesNotMatch(
  dictionaryCommand,
  /coord\.(?:accept_pending_correction|reject_pending_correction|dismiss_vocab_suggestions)\(/,
  'Tauri Coordinator must not own vocabulary suggestion mutations',
);
assert.match(
  qaCommand,
  /less_computer_window_dismiss\([^]*?coord\.dismiss_less_computer\(\)\.await/,
  'Less Computer dismiss must await the Host capture cleanup seam',
);
assert.match(
  coordinator,
  /async fn dismiss_less_computer\([^]*?less_computer\.dismiss\(\)[^]*?hide_less_computer\(\)[^]*?cancel_active_less_computer\(&self\.inner\)\.await/,
  'closing Less Computer must clear Core conversation, hide the panel, and release its capture',
);
assert.match(
  qaCommand,
  /less_computer_submit_text\([^]*?(?:core|backend)\.submit_less_computer\(text\)/,
  'Less Computer text submit must delegate the run to Core',
);
assert.doesNotMatch(
  coordinator,
  /pub fn less_computer_(?:window_dismiss|window_open|submit_text)\(/,
  'Coordinator must not own Less Computer command business or window wrappers',
);
assert.match(
  stylePacksCommand,
  /core\.preview_style_pack_runtime\(&style_pack\)/,
  'style-pack runtime diagnostics must be assembled by Core',
);
const stylePackPreviewCommand = stylePacksCommand.match(
  /pub fn preview_style_pack_runtime\([^]*?\r?\n\}\r?\n/,
)?.[0];
assert.ok(stylePackPreviewCommand, 'style-pack preview command must remain present');
assert.doesNotMatch(
  stylePackPreviewCommand,
  /CoordinatorState/,
  'style-pack commands must not reach back into Coordinator for business diagnostics',
);
assert.doesNotMatch(
  coordinatorBusinessSources,
  /\bActiveAsr\b|CredentialsVault|ProviderScope|build_active_omni_provider|build_tauri_omni_provider|(?:fn|const)\s+(?:asr_vocab_phrases|prioritize_vocab_for_asr|FRESH_VOCAB_SEATS)\b/,
  'Coordinator must not own ASR provider, credential, active-session, or vocabulary policy',
);
assert.doesNotMatch(
  `${credentialCommands}\n${credentialPersistence}`,
  /\b(?:allocate_channel_id|compact_orders|reposition_after_toggle|apply_order|mutate_vault_channel)\b/,
  'Tauri credential persistence must not retain Core channel mutation/order/active policy',
);
assert.doesNotMatch(
  `${coreAdapters}\n${linuxFcitx}`,
  /\b(?:streamed_text|stream_failed)\b|starts_with\(/,
  'Host insertion adapters must not retain final reconciliation policy',
);
assert.doesNotMatch(
  hostDocumentMacos,
  /\b(?:edit_is_within_typed_text|is_vocab_worthy|learned_rule)\b/,
  'the macOS edit watcher must report EditPair and leave learning policy to Core',
);
assert.match(
  historyCommand,
  /core\.apply_history_retranscription\(/,
  'history retranscription mutation and attribution must be owned by Core',
);
assert.doesNotMatch(
  historyCommand,
  /entry\.(?:raw_transcript|final_text|asr_provider|asr_model|llm_provider|llm_model|polish_ms)\s*=/,
  'the Tauri history command must not mutate retranscription records',
);
const providerUiSources = `${providerSection}\n${channelList}\n${providerLabels}`;
assert.match(
  providerUiSources,
  /listProviderDescriptors/,
  'React provider UI must consume Core ProviderDescriptor',
);
assert.doesNotMatch(
  providerUiSources,
  /\b(?:ASR_PRESETS|LLM_PRESETS|OMNI_PRESETS|LOCAL_ASR_PROVIDER_IDS)\b/,
  'React must retain labels only, not provider defaults or capability tables',
);
assert.doesNotMatch(
  providerUiSources,
  /https:\/\/(?:api\.openai\.com|api\.deepseek\.com|dashscope\.aliyuncs\.com|ark\.cn-beijing\.volces\.com)/,
  'React must not hard-code provider endpoints owned by Core descriptors',
);
assert.match(
  appShell,
  /export function App\([^]*?getStartupSnapshot\(\)[^]*?return <ReadyApp \{\.\.\.props\} \/>/,
  'every Tauri webview route must mount behind the shared readiness handshake',
);
assert.doesNotMatch(
  localAsrPage,
  /\b(?:createChannel|setChannelEnabled|reorderChannels|setActiveAsrProvider)\s*\(/,
  'the model page must not mutate channels before Core activation commits',
);
for (const activation of [
  /activateLocalAsr\("generic", modelId, provider\)/,
  /activateLocalAsr\("foundry", alias, "foundry-local-whisper"\)/,
  /activateLocalAsr\("sherpa_onnx", modelAlias, "sherpa-onnx-local"\)/,
]) {
  assert.match(localAsrPage, activation, 'every local runtime must use the single Core activation');
}
const localAsrActivation = localAsrCore.match(/    fn activate\([^]*?(?=\r?\n    fn )/)?.[0];
assert.ok(localAsrActivation, 'Core must own local ASR activation');
assert.match(
  localAsrActivation,
  /list_channels\(ChannelKind::Asr\)[^]*?channel\.id == request\.provider_id[^]*?channel\.provider_type == request\.provider_id/,
  'Core must resolve a concrete channel ID or provider type without a prior UI write',
);
assert.match(
  localAsrActivation,
  /\.prepare\([^]*?\.preload_lease\([^]*?preferences\.update\([^]*?\.mutate_channel\(ChannelMutation::ActivateLocalAsr/,
  'channel activation must commit only after native preparation and preference persistence succeed',
);
assert.match(
  coreAdapters,
  /fn preload_lease\([^]*?preload_for_lease\(\s*lease\.target,\s*model_dir,\s*provider_type,\s*Some\(lease\.generation\),?\s*\)/,
  'the Tauri native cache must receive the Core activation generation',
);
assert.match(
  coreAdapters,
  /let loaded\s*=\s*if openless_core::LocalAsrModelId::from_wire_id\(\s*&settings\.active_model,?\s*\)[^]*?is_whisper\)[^]*?whisper_cache\.loaded_model_id\(\)[^]*?else[^]*?qwen_cache\.loaded_model_id\(\)/,
  'macOS engine status must select the active model family rather than prefer any loaded Qwen cache',
);
for (const cache of await Promise.all([
  read('src-tauri/src/asr/local/cache.rs'),
  read('src-tauri/src/asr/local/whisper_provider.rs'),
])) {
  const release = cache.match(/pub\(crate\) fn release_lease\([^]*?(?=\r?\n    (?:pub|\/\/))/)?.[0];
  assert.ok(release, 'each native cache must implement model-scoped activation cleanup');
  assert.match(release, /inner\.lock\(\)[^]*?cached\.model_id == model_id\s*&&\s*cached\.activation_generation == Some\(generation\)[^]*?slot\.take\(\)/,
    'model and generation checks must happen under the same lock as cache eviction');
  assert.doesNotMatch(release, /\.cancel\(/, 'retiring a cache lease must preserve an in-flight transcription Arc');
  assert.match(cache, /get_or_load_for_lease\([^\n]*None\)/,
    'ordinary preload or transcription must revoke the previous activation owner');
  assert.match(cache, /if self\.load_generation\.load\(Ordering::Acquire\) != load_generation\s*\{\s*if activation_generation\.is_some\(\)\s*\{\s*anyhow::bail!\([^]*?\);\s*\}\s*return Ok\(engine\);\s*\}\s*(?:\*slot = Some|slot\.replace)/,
    'a superseded activation must fail, while ordinary ASR keeps its uncached Arc without replacing the current cache');
  for (const method of ['finish_use', 'release_current_if_idle', 'release_if_idle']) {
    const cleanup = cache.match(new RegExp(`pub fn ${method}\\([^]*?(?=\\r?\\n    (?:pub|//))`))?.[0];
    assert.ok(cleanup, `${method} must remain an explicit native cache cleanup path`);
    assert.match(cleanup, /inner\.lock\(\)[^]*?activation_generation\.is_none\(\)[^]*?slot\.take\(\)/,
      `${method} must preserve a newer activation owner even when it reuses the same Arc`);
  }
}
assert.equal(
  localAsrActivation.match(/\.mutate_channel\(/g)?.length,
  1,
  'activation must commit channel creation, enabling, ordering and selection as one metadata mutation',
);
assert.doesNotMatch(
  localAsrActivation,
  /\.set_active_provider\(/,
  'activation must not layer a separate active-provider write around the metadata transaction',
);
assert.match(
  localAsrActivation,
  /Ok\(ChannelMutationResult::Activated\(id\)\)\s*=>\s*id[^]*?LocalAsrActivationResult\s*\{[^]*?provider_id,/,
  'activation must return the channel ID allocated or selected by the final transaction',
);
assert.match(
  credentialCore,
  /ChannelMutation::ActivateLocalAsr \{ id, provider_type \} => \{[^]*?allocate_channel_id[^]*?channel\.enabled = true[^]*?channels\.insert\(0, channel\)[^]*?ChannelMutationResult::Activated\(id\)/,
  'Core metadata must create or enable and promote the activated channel in the same mutation',
);
assert.doesNotMatch(
  localAsrPage,
  /activateSherpaProvider\(selectedSherpaAlias\)[^]*?prepareSherpaOnnxAsr\(/,
  'local ASR activation must not layer the old prepare sequence after the Core transaction',
);
for (const legacyDictationFacade of [
  'start_dictation',
  'start_dictation_with_translation',
  'stop_dictation',
  'stop_dictation_with_translation',
  'cancel_dictation',
]) {
  assert.doesNotMatch(
    coordinator,
    new RegExp(`\\bpub(?:\\(crate\\))?\\s+(?:async\\s+)?fn\\s+${legacyDictationFacade}\\b`),
    `Coordinator must not expose the legacy ${legacyDictationFacade} facade; production entry points use openless-core`,
  );
}
assert.match(
  androidNativeBridge,
  /nativeBackendSnapshot[\s\S]*?android_backend_snapshot_response\(CORE_BACKEND\.get\(\)\.map\(Arc::as_ref\)\)/,
  'Android JNI must expose the typed Core startup handshake',
);
assert.match(
  androidKotlinBridge,
  /BACKEND_CONTRACT_VERSION = "2\.0\.0"[\s\S]*?requireBackendContract\(\)[\s\S]*?contractVersion/,
  'Android Kotlin must reject a backend contract version other than 2.0.0',
);
assert.match(
  androidOverlayService,
  /onCreate\(\)[\s\S]*?OpenLessNative\.requireBackendContract\(\)[\s\S]*?stopSelf\(\)/,
  'Android overlay startup must execute the JNI contract handshake before accepting actions',
);
assert.match(
  mobileHotkey,
  /pub fn next_press_id\(\) -> u64/,
  'the mobile hotkey stub must preserve the Core press-identity shape',
);
assert.match(
  mobileSelection,
  /capture_selection_insertion_target[^]*?reactivate_selection_insertion_target[^]*?true/,
  'mobile dictation insertion must provide a no-op target restore without enabling Selection Polish',
);

console.log('shared-backend-wire-contract.test.mjs passed');
