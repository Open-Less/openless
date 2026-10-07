// TypeScript mirror of src-tauri/src/types.rs.
// All keys are camelCase (Rust serializes with #[serde(rename_all = "camelCase")]).
// PolishMode is an exception — Rust uses lowercase serialization.

import type {
  AndroidAccessibilityStatus,
  AndroidInsertStrategy,
  AndroidOverlayActivationMode,
  AndroidOverlayCancelSwipeDirection,
  AndroidOverlayGestureAction,
  AndroidOverlayGestureActions,
  AndroidOverlayLeftSwipeAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
} from '../../android/frontend/lib/androidTypes';

export type {
  AndroidAccessibilityStatus,
  AndroidInsertStrategy,
  AndroidOverlayActivationMode,
  AndroidOverlayCancelSwipeDirection,
  AndroidOverlayGestureAction,
  AndroidOverlayGestureActions,
  AndroidOverlayLeftSwipeAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
};

export type PolishMode = 'raw' | 'light' | 'structured' | 'formal';

/** Recognition pipeline mode (issue #902): traditional = two-stage ASR + LLM;
 *  multimodal = one multimodal model turns audio + prompt into final text in a single step.
 *  The two configurations are fully isolated in the credentials vault; runtime reads only the current mode. */
export type PipelineMode = 'traditional' | 'multimodal';

export type InsertStatus = 'inserted' | 'pasteSent' | 'copiedFallback' | 'failed' | 'notRequested';

export type HistorySource = 'voice' | 'quick_note' | 'selection_polish' | 'selection_voice_edit';

/** Per-day count for the Overview yearly activity heatmap (date = local date YYYY-MM-DD). */
export interface ActivityDay {
  date: string;
  count: number;
  /** Total characters of final inserted text that day. Missing for dates written before the upgrade (reads as 0). */
  chars?: number;
  /** Total recording duration that day (ms). Missing for dates written before the upgrade (reads as 0). */
  durationMs?: number;
}

export interface DictationSession {
  id: string;
  createdAt: string; // ISO-8601
  source?: HistorySource;
  rawTranscript: string;
  /** ASR raw text before correction rules. `rawTranscript` holds the post-rule version;
   *  when the two match, the backend omits this field (null). Used to attribute a misrecognition
   *  to ASR mishearing vs. an LLM rewrite. Absent in older history. */
  asrTranscript: string | null;
  finalText: string;
  mode: PolishMode;
  stylePackId: string | null;
  translationActive: boolean;
  polishSource: string | null;
  appBundleId: string | null;
  appName: string | null;
  insertStatus: InsertStatus;
  errorCode: string | null;
  durationMs: number | null;
  dictionaryEntryCount: number | null;
  /** Whether the session archived the raw wav while recording (per prefs.recordAudioForDebug at the time).
   *  When true, History renders a play button and fetches the byte stream via the read_audio_recording IPC by id. */
  hasAudioRecording: boolean | null;
  /** ASR provider id used for this transcription (e.g. "volcengine" / "local-qwen3"). Null in older history. */
  asrProvider: string | null;
  /** ASR model id used for this transcription. Null when the provider has no model concept. */
  asrModel: string | null;
  /** LLM provider id used for polishing. Null for Raw passthrough (no LLM call). */
  llmProvider: string | null;
  /** LLM model id used for polishing. Null for Raw passthrough. */
  llmModel: string | null;
  /** Pipeline mode used by this session ("multimodal" / missing = traditional two-stage). */
  pipelineMode?: string | null;
  /** Measured wait for the transcription result after key release (ms). Tail latency for streaming ASR; full transcription time for batch. */
  asrMs: number | null;
  /** Measured duration of the LLM polish/translate call (ms). Null when no LLM call. */
  polishMs: number | null;
}

export interface DictionaryEntry {
  id: string;
  phrase: string;
  note: string | null;
  enabled: boolean;
  hits: number;
  createdAt: string;
}

/** Where a correction rule came from. Old correction-rules.json lacks this field; the backend
 *  deserializes it as 'manual' — those rules were indeed all added manually. */
export type RuleSource = 'manual' | 'learned';

export interface CorrectionRule {
  id: string;
  pattern: string;
  replacement: string;
  enabled: boolean;
  createdAt: string;
  source: RuleSource;
}

/** Return of `debug_read_cursor_context`: full result of one cursor-context probe.
 *  Every non-ok status must state why nothing was read — during install verification this tells
 *  whether an app was blocked by the safety gate or AX simply doesn't support it. */
export interface HostDocumentReadResult {
  status: 'ok' | 'blocked' | 'unsupported' | 'unavailable' | 'timeout';
  reason: string | null;
  window: { text: string; cursor: number } | null;
  appName: string | null;
  bundleId: string | null;
  elapsedMs: number;
}

/** A correction suggestion awaiting user confirmation (Tier2). The backend keeps it only in memory
 *  and it is lost on restart — suggestions are ephemeral; repeating the same mistake regenerates one. */
export interface PendingCorrection {
  expiresAtMs: number;
  id: string;
  pattern: string;
  replacement: string;
}

/** Why this text failed to land in the target app. Backend logs only; the card does not render it. */
export type InsertFallbackReason = 'partialStream' | 'insertFailed';

/** Content of the insert-failure fallback card. `text` is always the full text, even if only part landed on screen. */
export interface InsertFallbackCardPayload {
  text: string;
  reason: InsertFallbackReason;
  /** Presentation generation; size reports must carry it verbatim so the backend can drop stale IPC from older cards. */
  presentationId: number;
}

export interface VocabPreset {
  id: string;
  name: string;
  phrases: string[];
}

export interface VocabPresetStore {
  custom: VocabPreset[];
  overrides: VocabPreset[];
  disabledBuiltinPresetIds: string[];
}

export type HotkeyTrigger =
  | 'rightOption'
  | 'leftOption'
  | 'rightControl'
  | 'leftControl'
  | 'rightCommand'
  | 'leftCommand'
  | 'leftShift'
  | 'rightShift'
  | 'fn'
  | 'rightAlt'
  | 'mediaPlayPause'
  | 'custom';

export type HotkeyMode = 'toggle' | 'hold' | 'doubleClick' | 'auto';

export interface HotkeyKey {
  code: string;
}

export interface HotkeyBinding {
  trigger: HotkeyTrigger;
  mode: HotkeyMode;
  keys?: HotkeyKey[] | null;
}

export type HotkeyAdapterKind = 'macEventTap' | 'windowsLowLevel' | 'fcitx5' | 'unavailable';

export interface HotkeyCapability {
  adapter: HotkeyAdapterKind;
  availableTriggers: HotkeyTrigger[];
  requiresAccessibilityPermission: boolean;
  supportsModifierOnlyTrigger: boolean;
  supportsSideSpecificModifiers: boolean;
  explicitFallbackAvailable: boolean;
  statusHint: string | null;
}

export interface HotkeyInstallError {
  code: string;
  message: string;
}

export type HotkeyStatusState = 'starting' | 'installed' | 'failed';

export interface HotkeyStatus {
  adapter: HotkeyAdapterKind;
  state: HotkeyStatusState;
  message: string | null;
  lastError: HotkeyInstallError | null;
}

export interface ShortcutBinding {
  /** Primary key, e.g. "D" / "Space" / "F1" / "RightOption" / "LeftShift" */
  primary: string;
  /** Modifiers: generic tags (cmd/ctrl/…) or side-specific tags (cmd-left/ctrl-right/…). */
  modifiers: string[];
}

/** Style pack direct hotkey: pressing the binding activates the style pack for packId (issue #759). */
export interface StylePackHotkey {
  packId: string;
  binding: ShortcutBinding;
}

/** Selection voice Q&A hotkey binding. null = not enabled. See issue #118. */
export type QaHotkeyBinding = ShortcutBinding;

/** Custom recording combo key binding. Used when hotkey.trigger == 'custom'. */
export type ComboBinding = ShortcutBinding;

export type CodingAgentProviderId = 'claude-code-cli' | 'opencode-cli' | 'codex-cli' | 'dsh-cli';
export type CodingAgentPermissionMode = 'plan' | 'default' | 'acceptEdits' | 'bypassPermissions';

/** Shortcut pressed for simulated paste. Windows/Linux only; macOS uses direct AX writes.
 *  - ctrlV       : standard paste (default; most editors, browsers, IDEs)
 *  - ctrlShiftV  : terminals like kitty / alacritty / wezterm / gnome-terminal / foot
 *  - shiftInsert : legacy X11 terminals like xterm / urxvt
 *  See issue #360. */
export type PasteShortcut = 'ctrlV' | 'ctrlShiftV' | 'shiftInsert';

/** Windows dictation text insertion strategy. */
export type WindowsInsertionMode = 'tsf' | 'sendInput' | 'paste';

/** Newline simulation for the Windows SendInput path. */
export type WindowsSendInputNewlineMode = 'enter' | 'shiftEnter' | 'crlf';

/** How newlines are sent during macOS per-character insertion. `auto` picks per foreground app; `lineFeed` for terminals;
 *  `return` acts as send in chat boxes — style packs that split one message per newline need this. */
export type MacosNewlineMode = 'auto' | 'shiftReturn' | 'lineFeed' | 'return';

export type WindowsImeInstallState =
  'installed' | 'notInstalled' | 'registrationBroken' | 'notWindows';

export interface WindowsImeStatus {
  state: WindowsImeInstallState;
  usingTsfBackend: boolean;
  message: string;
  dllPath: string | null;
}

/** Background auto-update channel. Follows the build type when not explicitly chosen; manual check buttons are unaffected. */
export type UpdateChannel = 'stable' | 'beta';

export type ThemeMode = 'system' | 'light' | 'dark';

/** Selection polish result replaces directly, or is confirmed in an editable preview first. */
export type SelectionPolishOutputMode = 'directReplace' | 'previewConfirm';

export type SelectionVoiceIntentMode = 'prompt' | 'auto' | 'manual' | 'heuristic';
export type SelectionVoiceManualIntent = 'question' | 'edit' | 'compose';
/** Preferred EditPlan serialization when parsing selection-voice model output. */
export type EditPlanFormat = 'xml' | 'json';

export interface CustomStylePrompts {
  raw: string;
  light: string;
  structured: string;
  formal: string;
}

export interface StyleSystemPrompts {
  raw: string;
  light: string;
  structured: string;
  formal: string;
}

export type StylePackKind = 'builtin' | 'imported';

export interface StylePackExample {
  title?: string | null;
  input: string;
  output: string;
}

export interface StylePack {
  id: string;
  name: string;
  description: string;
  author?: string | null;
  version: string;
  kind: StylePackKind;
  baseMode: PolishMode;
  /** For selected written text. Empty values in legacy packs use a safe backend default. */
  selectionPrompt: string;
  /** Selection-voice EditPlan system prompt. Empty = prefs custom / built-in default. */
  voiceEditPrompt: string;
  prompt: string;
  examples: StylePackExample[];
  tags: string[];
  iconPath?: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
  enabled: boolean;
  active: boolean;
  recommendedModel?: string | null;
  compatibleAppVersion?: string | null;
  /** Derivation: null = locally authored (or never first-published to the cloud); non-null = this pack was installed from cloud originPackId. */
  originPackId?: string | null;
  originAuthorLogin?: string | null;
}

export interface StylePackRuntimeDiagnostics {
  packId: string;
  packName: string;
  packPrompt: string;
  packPromptChars: number;
  contextPremise: string;
  contextPremiseChars: number;
  hotwordBlock: string;
  hotwordBlockChars: number;
  historyInstruction: string;
  historyInstructionChars: number;
  singleTurnPrompt: string;
  singleTurnPromptChars: number;
  multiTurnPrompt: string;
  multiTurnPromptChars: number;
  workingLanguages: string[];
  hotwords: string[];
  contextWindowMinutes: number;
  includesContextPremise: boolean;
  includesHotwordBlock: boolean;
  includesHistoryInstruction: boolean;
  previewOmitsFrontApp: boolean;
}

export interface UserPreferences {
  hotkey: HotkeyBinding;
  dictationHotkey: ShortcutBinding;
  defaultMode: PolishMode;
  enabledModes: PolishMode[];
  activeStylePackId: string;
  styleSystemPrompts: StyleSystemPrompts;
  customStylePrompts: CustomStylePrompts;
  launchAtLogin: boolean;
  showCapsule: boolean;
  /** Recording capsule appearance; synced to the capsule window on save. */
  capsuleStyle: CapsuleStyle;
  capsuleTranscriptEnabled: boolean;
  capsuleTranscriptFontSize: number;
  /** Temporarily mutes system output during recording; restores the original mute state on stop/cancel/error. */
  muteDuringRecording: boolean;
  /** Record fully first, then connect to the current ASR and submit the whole audio on stop. Off by default. */
  stableTranscriptionEnabled: boolean;
  /** When the recording hotkey enters the recording state, plays a synthesized cue that recording has started.
   *  On by default; synthesized with the Web Audio API in the capsule window, independent of showCapsule. */
  audioCueOnRecord: boolean;
  /** Toggle mode "auto-stop after speech" (issue #860). Off by default; when on, speech followed by
   *  silenceAutoStopSeconds of continuous silence auto-stops and submits; 10 seconds of no speech cancels. */
  silenceAutoStopEnabled: boolean;
  /** Continuous silence threshold after speech (seconds). Options: 1 / 1.5 / 2 / 3 / 4 / 5; default 3. */
  silenceAutoStopSeconds: number;
  /** Recording input device name. Empty string = system default microphone. */
  microphoneDeviceName: string;
  activeAsrProvider: string;
  activeLlmProvider: string;
  /** Recognition pipeline mode (experimental, issue #902). In multimodal mode, voice pipelines use the omni config. */
  pipelineMode: PipelineMode;
  /** Legacy capability gate; the Services pipeline selector enables it when selecting multimodal. */
  multimodalPipelineEnabled: boolean;
  /** Currently active provider id for the multimodal (Omni) model; mirrors credentials vault omni.active. */
  activeOmniProvider: string;
  /** LLM thinking mode toggle. Off by default; plain OpenAI chat models skip unsupported fields. See issue #402. */
  llmThinkingEnabled: boolean;
  /** Whether to use the system proxy (issue #869). On by default; when off, all requests connect directly and foreign services (GitHub login/updates, etc.) may be unreachable. */
  useSystemProxy: boolean;
  /** Windows/Linux only: whether to restore the user's original clipboard after a successful paste. Default true. See issue #111. */
  restoreClipboardAfterPaste: boolean;
  /** Windows/Linux only: shortcut pressed for simulated paste. See issue #360: terminals like kitty/alacritty
   *  only accept Ctrl+Shift+V — a hardcoded Ctrl+V is swallowed and the dictation text is left in the clipboard.
   *  macOS uses direct AX writes and is unaffected. Default 'ctrlV' matches historical behavior. */
  pasteShortcut: PasteShortcut;
  /** Windows: whether paste-shortcut / clipboard fallback is allowed after TSF fails. SendInput is retried only when the clipboard write fails. Disable to verify genuine TSF insertion. */
  allowNonTsfInsertionFallback: boolean;
  /** Windows: dictation insertion strategy (TSF / SendInput / clipboard paste). */
  windowsInsertionMode: WindowsInsertionMode;
  /** Newline simulation for the Windows SendInput path. */
  windowsSendInputNewlineMode: WindowsSendInputNewlineMode;
  macosNewlineMode: MacosNewlineMode;
  /** Legacy compat: `true` is equivalent to `windowsInsertionMode === 'sendInput'`. */
  windowsSendInputInsertionOnly: boolean;
  /** Windows: whether to show OpenLess in the system keyboard list (Win+Space) for non-TSF insertion. */
  windowsShowOpenlessInKeyboardList: boolean;
  /** User's working languages (multi-select, native names); injected as a premise at the head of LLM polish/translate prompts. */
  workingLanguages: string[];
  /** Translation mode target language (single-select, native name); empty string = Shift translation disabled. See issue #4. */
  translationTargetLanguage: string;
  /** Chinese output script preference: auto-synced from the UI language (simplified/traditional); not exposed as a separate setting. */
  chineseScriptPreference: 'auto' | 'simplified' | 'traditional';
  /** Final output language preference: auto-synced from the UI language; not exposed as a separate setting. */
  outputLanguagePreference: 'auto' | 'zhCn' | 'zhTw' | 'en' | 'ja' | 'ko';
  /** Selection voice Q&A hotkey. null = not enabled. See issue #118. */
  qaHotkey: QaHotkeyBinding | null;
  /** Standalone quick-note hotkey. null = not configured. */
  quickNoteHotkey: ShortcutBinding | null;
  /** Selection polish hotkey. null = disabled. */
  selectionPolishHotkey: ShortcutBinding | null;
  /** The style pack used only by selected written-text polishing. */
  selectionPolishStylePackId: string;
  /** How the selection polish result is delivered. */
  selectionPolishOutputMode: SelectionPolishOutputMode;
  /** Selection voice edit (issue #987 Windows MVP). Off by default. */
  selectionVoiceEnabled: boolean;
  /** Selection voice intent routing: auto / manual / keyword heuristic. */
  selectionVoiceIntentMode: SelectionVoiceIntentMode;
  /** Fixed intent in manual mode. */
  selectionVoiceManualIntent: SelectionVoiceManualIntent;
  /** Keywords that route to the edit branch on a hit in heuristic mode. */
  selectionVoiceEditKeywords: string[];
  /** Preferred EditPlan output format for selection voice (default xml). */
  selectionVoiceEditPlanFormat: EditPlanFormat;
  /** Custom selection-voice EditPlan system prompt; empty string = style pack / built-in default. */
  selectionVoiceEditSystemPrompt: string;
  /** Whether to write Q&A history to the local archive. See issue #118. */
  qaSaveHistory: boolean;
  /** Custom recording combo key. Used when hotkey.trigger == 'custom'. null = not set. */
  customComboHotkey: ComboBinding | null;
  /** Global hotkey to trigger translation while recording. Default Shift. */
  translationHotkey: ShortcutBinding;
  /** Global hotkey to switch to the previous polish style. null = disabled by user (issue #576). */
  switchStyleHotkey: ShortcutBinding | null;
  /** Global hotkey to open the OpenLess main window. null = disabled by user (issue #576). */
  openAppHotkey: ShortcutBinding | null;
  /** Style pack direct hotkeys: pressing one activates the corresponding style pack. Default empty list (issue #759). */
  stylePackHotkeys: StylePackHotkey[];
  /** Less Computer: whether enabled. Off by default. */
  codingAgentEnabled: boolean;
  /** Agent backend: claude-code-cli (default) / opencode-cli / codex-cli / dsh-cli. */
  codingAgentProvider: CodingAgentProviderId;
  /**
   * Agent model, null = let the backend use its own default.
   * Claude takes aliases (sonnet etc.), OpenCode needs `provider/model`, Codex takes a bare model name;
   * dsh's headless profile has no model switch, so this has no effect for it.
   */
  codingAgentModel: string | null;
  /** Permission mode: plan/default/acceptEdits/bypassPermissions. */
  codingAgentPermissionMode: CodingAgentPermissionMode;
  /** Agent working directory, null = temp directory. */
  codingAgentWorkdir: string | null;
  /** Agent executable path/command, null or empty = backend default (claude / opencode). */
  codingAgentExe: string | null;
  /** Windows/macOS Less Computer push-to-talk hotkey. null = disabled. */
  codingAgentVoiceHotkey: ShortcutBinding | null;
  /** Hotkey 1: voice agent panel key. null = disabled. */
  codingAgentPanelHotkey: ShortcutBinding | null;
  /** Hotkey 2: quick-capture key (select → Claude → insert back). null = not configured. */
  codingAgentQuickHotkey: ShortcutBinding | null;
  /** Currently active model id for local Qwen3-ASR. Only meaningful for local-qwen3 providers. */
  localAsrActiveModel: string;
  /** Currently active model id for macOS local Whisper. */
  localWhisperActiveModel: string;
  /** Local model download source ('huggingface' / 'hf-mirror' / 'modelscope'). */
  localAsrMirror: string;
  /** How long the local ASR engine stays in memory (seconds). 0 = release right after speech;
   *  300 = default 5 minutes; 86400 = never auto-release (stay loaded). */
  localAsrKeepLoadedSecs: number;
  /** Currently active model alias for Windows Foundry Local Whisper. */
  foundryLocalAsrModel: string;
  /** Download source for the Windows Foundry Local native runtime. */
  foundryLocalRuntimeSource: string;
  /** Windows Foundry Local Whisper language hint. Empty string = auto-detect. */
  foundryLocalAsrLanguageHint: string;
  /** Seconds the Windows Foundry Local Whisper model stays loaded in the runtime. */
  foundryLocalAsrKeepLoadedSecs: number;
  /** Currently active model alias for Windows sherpa-onnx local ASR. */
  sherpaOnnxModel: string;
  /** Windows sherpa-onnx language hint. Empty string = auto-detect. */
  sherpaOnnxLanguageHint: string;
  /** Seconds the Windows sherpa-onnx model stays loaded in the runtime. */
  sherpaOnnxKeepLoadedSecs: number;
  /** History retention days. 0 = no time-based cleanup (the 200-entry cap still applies). Default 7. */
  historyRetentionDays: number;
  /** Conversation-aware polish context window (minutes). 0 = off. Default 5. See PR-A. */
  polishContextWindowMinutes: number;
  /** Run silently at startup (no main window). Common for Windows auto-start — background + tray only,
   *  without the main window popping up. When on, no startup path shows the window; open it from the menu bar / tray. Default false. */
  startMinimized: boolean;
  /** UI theme preference: follow OS, light, or dark. */
  themeMode: ThemeMode;
  /** Background auto-update channel. Follows the current build type until the user chooses one;
   * the manual check buttons in About / Advanced each pin stable/beta. */
  updateChannel: UpdateChannel;
  /** Whether the user explicitly chose the update channel; when missing, the current build type decides the default. */
  updateChannelExplicit?: boolean;
  /** Streaming insert: polished SSE output is typed into the current focus as keyboard events as it arrives,
   *  greatly reducing perceived latency. v1 is limited to macOS + OpenAI-compatible providers; other configs
   *  automatically fall back to the original one-shot insertion. Default true. */
  streamingInsert: boolean;
  /** One-time migration marker for issue #440: when old configs lack this field, the backend migrates the old default false to true;
   *  if the user later turns streamingInsert off manually, false is preserved. */
  streamingInsertDefaultMigrated: boolean;
  /** Whether to write the final polished text back to the clipboard after a successful streaming insert.
   *  When on, Cmd+V can re-paste that output, matching the one-shot path. Default true. */
  streamingInsertSaveClipboard: boolean;
  /** Whether to send the text near the cursor in the document the user is writing to LLM polish as context.
   *  Default false — when on, every dictation reads the foreground app's body text and sends part of it to the LLM provider.
   *  macOS only; password fields / Secure Input / password managers / terminals are always hard-blocked. */
  cursorContextEnabled: boolean;
  vocabularyLearningEnabled: boolean;
  vocabularyLearningSettings: {
    observationSeconds: number;
    suggestionSeconds: number;
    maxPhraseChars: number;
  };
  /** Whether Overview shows the yearly activity heatmap card. Default true; turning it off only hides the card, activity counting continues. */
  showOverviewActivityHeatmap: boolean;
  /** Readable layout: wraps same-row controls on small screens or large fonts to avoid horizontal overflow. Default false. */
  stackedRowLayout: boolean;
  /** Conservative layout: forces single-column full-width content everywhere except home, top bar, bottom bar, and capsule. Default false. */
  conservativeLayout: boolean;
  /** Auto-check updates at main-window startup and in the background every 60 minutes. Default true.
   *  Android: when on, checks and downloads automatically, then opens the system installer after verification.
   *  Desktop: when on, checks automatically; an update dialog asks the user to confirm installation.
   *  When off, only the manual "Check for updates" button in Settings works. */
  autoUpdateCheck: boolean;
  /** History entry cap. null = default 200; 5..=200 for user-defined values. */
  historyMaxEntries: number | null;
  /** Whether to keep the raw microphone audio file (wav) per session, for debugging ASR misrecognition / mic sensitivity.
   *  Default false. When on, it uses disk space and follows the same cleanup policy as historyRetentionDays. */
  recordAudioForDebug: boolean;
  /** Number of recent wav files kept in recordings/. null = follows the 200 hard cap; 1..=200 for user-defined values.
   *  Decoupled from historyMaxEntries — many text entries with only the 5 most recent wavs is a valid combination. */
  audioRecordingMaxEntries: number | null;
  /** Save directory for quick-note exported recordings. Empty string = show a save dialog on each export. */
  quickNoteExportDirectory: string;
  /** Marketplace HTTP base URL. Empty = local dev default http://127.0.0.1:8090; production uses https://api.<domain>. */
  marketplaceBaseUrl: string;
  /** Cached GitHub login for display. Not used for auth; the OAuth token lives only in the Rust CredentialsVault. */
  marketplaceDevLogin: string;
  /** Whether to enable the remote input (LAN phone recording) HTTPS+WS service. Default false. */
  remoteInputEnabled: boolean;
  /** Remote input service listening port (HTTPS). Default 8443. */
  remoteInputPort: number;
  /** Remote input pairing code (6 digits). Empty = generated randomly on first server start. */
  remoteInputPin: string;
  /** Default interaction mode of the phone recording page: 'toggle' (tap to switch) / 'hold' (push to talk). */
  remoteInputDefaultMode: 'toggle' | 'hold';
  /** Android: cross-app dictation insert strategy. */
  androidInsertStrategy: AndroidInsertStrategy;
  /** Android: floating overlay visibility trigger mode. */
  androidOverlayTrigger: AndroidOverlayTrigger;
  /** Android: how the floating overlay enters the armed interaction state. */
  androidOverlayActivationMode: AndroidOverlayActivationMode;
  /** Android: action performed by left swiping while the overlay is armed. */
  androidOverlayLeftSwipeAction: AndroidOverlayLeftSwipeAction;
  /** Android: vertical swipe direction that cancels recording. */
  androidOverlayCancelSwipeDirection: AndroidOverlayCancelSwipeDirection;
  /** Android: action assigned to each overlay swipe direction. */
  androidOverlayGestureActions: AndroidOverlayGestureActions;
  /** Android: floating overlay control diameter in dp. */
  androidOverlaySizeDp: number;
  /** Major-version generation marker of the splash PV (e.g. '2'). Empty = never played; advanced exclusively by
   *  the Rust-side take_splash_playback, preserved verbatim by the settings save path; the frontend is read-only. */
  splashSeenVersion?: string;
}

export interface MarketplaceListItem {
  id: string;
  slug: string;
  name: string;
  description: string;
  authorLogin: string;
  version: string;
  baseMode: PolishMode;
  tags: string[];
  likeCount: number;
  downloadCount: number;
  publishedAt: string;
  updatedAt: string;
  /** Derivation: null = original; non-null = derived from originPackId, UI shows "derived from @originAuthorLogin". */
  originPackId?: string | null;
  originAuthorLogin?: string | null;
}

export interface MarketplaceDetail extends MarketplaceListItem {
  prompt: string;
  state: 'pending' | 'approved' | 'rejected';
}

export interface MarketplaceMyPackItem extends MarketplaceListItem {
  state: 'pending' | 'approved' | 'rejected' | 'withdrawn' | 'superseded' | string;
}

export interface MicrophoneDevice {
  name: string;
  isDefault: boolean;
}

/** Payload delivered by Rust via the `qa:state` event.
 *  v2 (issue #118 v2): multi-turn support; the backend sends the whole messages array each time (single source of truth).
 *  v2.1: enables `stream:true`, pushing LLM answer chunks to the frontend via `answer_delta` events for streaming render. */
export type QaStateKind =
  | 'idle'
  | 'recording'
  | 'loading'
  | 'thinking'
  | 'answer_delta'
  | 'answer'
  | 'awaiting_approval'
  | 'cancelled'
  | 'error';

export interface QaChatMessage {
  role: 'user' | 'assistant';
  content: string;
  /** Raw selection text not escaped by the model safety envelope; for UI text display only. */
  selectionText?: string;
}

export interface QaStatePayload {
  kind: QaStateKind;
  /** Backend session token; the frontend uses it to drop stale late-turn events after close/reopen. */
  sessionId?: string;
  /** Backend-authoritative multi-turn history so far (alternating user → assistant). answer events carry the full version. */
  messages?: QaChatMessage[];
  /** Selection preview attached in recording state (first 60 chars). */
  selectionPreview?: string | null;
  /** Message attached in error state. */
  error?: string;
  /** Increment string of the current frame attached to answer_delta events. */
  chunk?: string;
  /** The selection voice edit result can replace the selection. */
  editApplyAvailable?: boolean;
  /** Can revert to the previous edit preview. */
  editRevertAvailable?: boolean;
  /** "Edit instruction" checkbox in the selection Q&A panel. */
  editInstructionMode?: boolean;
  /** Session-scoped token while the current turn waits for tool approval. */
  approvalToken?: string;
}

/**
 * Less Computer voice agent overlay window events (window label = "less-computer", event name
 * `less-computer:event`). The backend tags each by `kind`; the frontend renders the interaction as a chat structure.
 */
export type LessComputerEvent =
  /** Core voice lifecycle snapshot; seq for dedup, sessionId keeps old sessions' terminal states/levels from overwriting a new recording. */
  (
    | {
        kind: 'voice_state';
        sessionId: string;
        phase: 'starting' | 'recording' | 'transcribing' | 'idle';
        level: number;
        elapsedMs: number;
        /** Defaults to submit (legacy events and hotkey path). */
        mode?: LessComputerVoiceMode;
        /** Full transcript for this session so far, updated by live recognition while recording. */
        transcript?: string;
        /** Present only in idle, describing how the recording ended. */
        outcome?: LessComputerVoiceOutcome;
      }
    /** One user bubble (voice command transcript). fresh=true means a new session (history cleared); otherwise appended as a follow-up turn. */
    | { kind: 'user'; text: string; fresh?: boolean }
    /** Agent started, entering the running state. */
    | { kind: 'started' }
    /** Streaming reply delta (from CodingAgentEvent::Delta). */
    | { kind: 'delta'; text: string }
    /** Tool call hint (from CodingAgentEvent::ToolUse, e.g. "Bash"). */
    | { kind: 'tool'; name: string }
    /** Session context compacted (from CodingAgentEvent::Compaction); an inline hint is embedded at the matching point in the output stream. */
    | { kind: 'compaction' }
    /** Inline approval card: a high-risk action was blocked by the guardrail, waiting for Approve / Deny. */
    | { kind: 'approval'; token: string; command: string; reason: string }
    /** Run completed: final result + cost (USD). */
    | { kind: 'completed'; text: string; costUsd?: number | null }
    /** User cancelled the running agent from the capsule. */
    | { kind: 'cancelled' }
    /** Run failed with an error. */
    | { kind: 'error'; message: string }
  ) & {
    /** Monotonic event sequence number (assigned by the backend on emit). Used for less_computer_sync replay and
     *  live-stream dedup; the backend may omit it when the buffer lock is abnormal, and events without seq are applied unconditionally. */
    seq?: number;
  };

export type LessComputerVoiceEvent = Extract<LessComputerEvent, { kind: 'voice_state' }>;

/** submit: hand the transcript straight to the agent after speech; dictate: fill the transcript into the input box for the user to edit before sending. */
export type LessComputerVoiceMode = 'submit' | 'dictate';

export type LessComputerVoiceOutcome = 'submitted' | 'committed' | 'empty' | 'failed' | 'cancelled';

/** Bounded replay result of `less_computer_sync`. `truncated=true` means the caller's watermark
 * is older than the oldest event the backend still holds; the frontend must clear derived views before applying `events`. */
export interface LessComputerSyncResult {
  events: LessComputerEvent[];
  oldestSequence?: number;
  latestSequence: number;
  truncated: boolean;
  /** Latest Core voice display projection, recoverable even when phase events of long transcripts were evicted by the bounded replay. */
  voiceState?: LessComputerVoiceEvent;
}

export { SUPPORTED_LANGUAGES } from './languageCatalog';

export type CapsuleState =
  'idle' | 'recording' | 'transcribing' | 'polishing' | 'done' | 'cancelled' | 'error';

/** Recording capsule style: 'siri' = flowing Siri light-effect version (default); 'classic' = Openless classic pill version. */
export type CapsuleStyle = 'siri' | 'classic' | 'typeless';

export interface CapsulePayload {
  /** Session generation that owns this capsule frame. */
  sessionId?: string | null;
  state: CapsuleState;
  level: number; // 0..1 RMS
  elapsedMs: number;
  message: string | null;
  insertedChars: number | null;
  /** Whether the current session is in translation mode (the user pressed Shift). See issue #4. */
  translation: boolean;
  /** Whether this is a Less Computer session: the processing label shows "using" instead of "thinking". */
  operating?: boolean;
  /**
   * Warming state: the capsule is shown optimistically (pops up with the entrance animation on hotkey press)
   * but the microphone has not produced its first PCM frame yet. When true, the recording level bar renders in
   * a standby form (soft breathing, no real level), signaling the user to wait a moment before speaking; it flips
   * to false once the mic is ready. Only meaningful for recording.
   */
  warming?: boolean;
  /**
   * User-selected capsule style (siri / classic). Sent with every state event; falls back to the default
   * 'siri' when missing, for compatibility with older backend payloads.
   */
  capsuleStyle?: CapsuleStyle;
  /**
   * Selection polish reuses the capsule's focus-free native window but renders a lightweight status hint; when missing,
   * the original voice/QA capsule behavior is kept, for compatibility with older backend payloads.
   */
  selectionPolish?: boolean;
}

/**
 * Native replay frame used when the standalone Windows transcript rail mounts
 * after the capsule has already started. The payload fields are deliberately
 * flattened so the rail can restore presentation state and transcript state
 * from one revisioned snapshot.
 */
export interface CapsuleSnapshot extends CapsulePayload {
  transcript: string;
  sessionId: string | null;
  /** Backend event watermark for rejecting delayed events from an old session. */
  sequence: number;
  /** Monotonic native snapshot revision; currently aligned with the event watermark. */
  revision: number;
  /** Revision at which the flattened payload was committed. Torn frames are rejected. */
  payloadRevision: number;
}

export interface CredentialsStatus {
  activeAsrProvider: string;
  activeLlmProvider: string;
  /** Current recognition pipeline mode; the frontend uses it to render the config page and the Overview "configured" check. */
  pipelineMode: PipelineMode;
  asrConfigured: boolean;
  llmConfigured: boolean;
  /** Whether the multimodal (omni) model is configured. Only meaningful in multimodal mode. */
  omniConfigured: boolean;
  /** Legacy compatibility field (kept during the transition). */
  volcengineConfigured: boolean;
  arkConfigured: boolean;
}

export interface TodayMetrics {
  charsToday: number;
  segmentsToday: number;
  avgLatencyMs: number;
  totalDurationMs: number;
}

export type PermissionStatus =
  'granted' | 'denied' | 'notDetermined' | 'restricted' | 'notApplicable' | 'noDevice';

/** Runtime platform kind returned by `get_platform_capabilities`. */
export type PlatformKind = 'desktop' | 'android' | 'mobile';

/** Feature flags for desktop vs Android APK UI gating. Mirrors src-tauri PlatformCapabilities. */
export interface PlatformCapabilities {
  platform: PlatformKind;
  supportsDesktopHotkey: boolean;
  supportsTray: boolean;
  supportsOverlay: boolean;
  supportsImeInput: boolean;
  supportsLocalAsr: boolean;
  supportsLocalQwen3Mlx: boolean;
  supportsInAppDictation: boolean;
  supportsAutoUpdate: boolean;
}
