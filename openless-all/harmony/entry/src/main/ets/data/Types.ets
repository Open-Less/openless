/**
 * JSON shapes of the shared `openless-core` contract, as the Rust host serializes
 * them (camelCase). Only the fields the native UI reads are declared; everything
 * optional is `?` so an older core simply leaves it undefined.
 */

export interface HistoryEntry {
  id: string;
  createdAt: string;
  source?: string;
  rawTranscript?: string;
  asrTranscript?: string | null;
  finalText: string;
  mode?: string;
  stylePackId?: string | null;
  translationActive?: boolean;
  appName?: string | null;
  insertStatus?: string;
  errorCode?: string | null;
  durationMs?: number | null;
  hasAudioRecording?: boolean | null;
  asrProvider?: string | null;
  asrModel?: string | null;
  llmProvider?: string | null;
  llmModel?: string | null;
  pipelineMode?: string | null;
  asrMs?: number | null;
  polishMs?: number | null;
}

export interface ActivityDay {
  date: string;
  count: number;
  chars?: number;
  durationMs?: number;
}

export interface TodayMetrics {
  count?: number;
  chars?: number;
  durationMs?: number;
  /** Some cores report the day as a list of one element instead. */
  sessions?: number;
}

export interface DictionaryEntry {
  id: string;
  phrase: string;
  note?: string | null;
  enabled: boolean;
  hits?: number;
  createdAt?: string;
}

export interface CorrectionRule {
  id: string;
  pattern: string;
  replacement: string;
  enabled: boolean;
  createdAt?: string;
  source?: string;
}

/** Exact Core StylePack wire value. Updates are whole values, not field patches. */
export interface StylePack {
  id: string;
  name: string;
  description: string;
  author: string | null;
  version: string;
  kind: string;
  baseMode: string;
  selectionPrompt: string;
  voiceEditPrompt: string;
  prompt: string;
  examples: StylePackExample[];
  tags: string[];
  iconPath: string | null;
  createdAt: string | null;
  updatedAt: string | null;
  enabled: boolean;
  active: boolean;
  recommendedModel: string | null;
  compatibleAppVersion: string | null;
  originPackId: string | null;
  originAuthorLogin: string | null;
}

export interface StylePackExample {
  title: string | null;
  input: string;
  output: string;
}

export interface Channel {
  id: string;
  name: string;
  providerType: string;
  enabled: boolean;
  order: number;
  lastTest?: ChannelTest | null;
}

export interface ChannelTest {
  ok: boolean;
  latencyMs?: number;
  at?: number;
  error?: string | null;
}

export interface EndpointPreset {
  name: string;
  endpoint: string;
  modelsUrl?: string;
}

export interface ProviderDescriptor {
  kind: string;
  providerType: string;
  labelKey: string;
  defaultEndpoint?: string | null;
  endpointPresets?: EndpointPreset[];
  defaultModel?: string | null;
  authRequirement: string;
  staticModels?: string[];
  defaultRequestFormat?: string | null;
}

export interface SettingsSnapshot {
  preferences: UserPreferences;
  revision: number;
}

/** Only the preference fields the native settings pages read or write. */
export interface UserPreferences {
  defaultMode?: string;
  enabledModes?: string[];
  activeStylePackId?: string;
  activeAsrProvider?: string;
  activeLlmProvider?: string;
  activeOmniProvider?: string;
  pipelineMode?: string;
  multimodalPipelineEnabled?: boolean;
  llmThinkingEnabled?: boolean;
  microphoneDeviceName?: string;
  audioCueOnRecord?: boolean;
  silenceAutoStopEnabled?: boolean;
  silenceAutoStopSeconds?: number;
  muteDuringRecording?: boolean;
  audioEnhancementEnabled?: boolean;
  stableTranscriptionEnabled?: boolean;
  lowLatencyDictationEnabled?: boolean;
  restoreClipboardAfterPaste?: boolean;
  useSystemProxy?: boolean;
  themeMode?: string;
  locale?: string;
  translationEnabled?: boolean;
  translationTargetLanguage?: string;
  recordAudioForDebug?: boolean;
  historyRetentionDays?: number;
  launchAtLogin?: boolean;
  showCapsule?: boolean;
  capsuleTranscriptEnabled?: boolean;
  capsuleTranscriptFontSize?: number;
  cloudSyncEnabled?: boolean;
  autoUpdateEnabled?: boolean;
  updateChannel?: string;
  vocabularyLearning?: VocabularyLearning;
  correctionRulesEnabled?: boolean;
  localAsrEnabled?: boolean;
  localAsrModel?: string;
  polishMode?: string;
}

export interface VocabularyLearning {
  enabled?: boolean;
  suggestionsEnabled?: boolean;
}

export interface StartupSnapshot {
  contractVersion?: string;
  backend?: StartupBackend;
  platform?: PlatformCapabilities;
  preferences?: UserPreferences;
  revision?: number;
}

export interface StartupBackend {
  running?: boolean;
  dataDir?: string;
  version?: string;
}

export interface PlatformCapabilities {
  platform: string;
  supportsImeInput?: boolean;
  supportsOverlay?: boolean;
  supportsDesktopHotkey?: boolean;
  supportsLocalAsr?: boolean;
  supportsInAppDictation?: boolean;
  supportsAutoUpdate?: boolean;
  supportsVoiceEdit?: boolean;
}

/** Reply envelope of every command the native bridge forwards. */
export interface NativeReply {
  ok: boolean;
  value?: object | null;
  error?: string;
}

export interface VoiceResult {
  rawText: string;
  polishedText: string;
}

export interface DictationEvent {
  phase: string;
  level: number;
  error: string;
}

/** MarketplaceListItem does not report device-local installation state. */
export interface MarketplacePack {
  id: string;
  slug: string;
  name: string;
  description: string;
  authorLogin: string;
  version: string;
  baseMode: string;
  tags: string[];
  likeCount: number;
  downloadCount: number;
  publishedAt: string;
  updatedAt: string;
  originPackId?: string | null;
  originAuthorLogin?: string | null;
}

export interface MarketplaceLikeResult {
  likeCount: number;
  alreadyLiked: boolean;
}

export interface CloudSyncCounts {
  dictionary?: number;
  corrections?: number;
  stylePacks?: number;
}

export interface CloudSyncStatus {
  schemaVersion?: number;
  /** Server revision the next upload must be based on. */
  revision?: number;
  updatedAt?: string | null;
  hasSnapshot?: boolean;
  counts?: CloudSyncCounts;
}
