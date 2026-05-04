// TypeScript mirror of src-tauri/src/types.rs.
// All keys are camelCase (Rust serializes with #[serde(rename_all = "camelCase")]).
// PolishMode is an exception — Rust uses lowercase serialization.

export type PolishMode = 'raw' | 'light' | 'structured' | 'formal';

export type InsertStatus = 'inserted' | 'pasteSent' | 'copiedFallback' | 'failed';

export interface DictationSession {
  id: string;
  createdAt: string; // ISO-8601
  rawTranscript: string;
  finalText: string;
  mode: PolishMode;
  appBundleId: string | null;
  appName: string | null;
  insertStatus: InsertStatus;
  errorCode: string | null;
  durationMs: number | null;
  dictionaryEntryCount: number | null;
}

export interface DictionaryEntry {
  id: string;
  phrase: string;
  note: string | null;
  enabled: boolean;
  hits: number;
  createdAt: string;
}

export type HotkeyTrigger =
  | 'rightOption'
  | 'leftOption'
  | 'rightControl'
  | 'leftControl'
  | 'rightCommand'
  | 'fn'
  | 'rightAlt';

export type HotkeyMode = 'toggle' | 'hold';

export interface HotkeyBinding {
  trigger: HotkeyTrigger;
  mode: HotkeyMode;
}

export type HotkeyAdapterKind = 'macEventTap' | 'windowsLowLevel' | 'rdev';

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

/** 划词语音问答快捷键绑定。null 表示未启用。详见 issue #118。 */
export interface QaHotkeyBinding {
  /** 主键（去掉所有修饰符的字面字符），例如 ";" / "/" / "a" */
  primary: string;
  /** 修饰符列表，元素小写："cmd" | "shift" | "option" | "ctrl"。 */
  modifiers: string[];
}

export type WindowsImeInstallState =
  | 'installed'
  | 'notInstalled'
  | 'registrationBroken'
  | 'notWindows';

export interface WindowsImeStatus {
  state: WindowsImeInstallState;
  usingTsfBackend: boolean;
  message: string;
  dllPath: string | null;
}

export interface UserPreferences {
  hotkey: HotkeyBinding;
  defaultMode: PolishMode;
  enabledModes: PolishMode[];
  launchAtLogin: boolean;
  showCapsule: boolean;
  activeAsrProvider: string;
  activeLlmProvider: string;
  /** 仅 Windows/Linux：粘贴成功后是否恢复用户原剪贴板。默认 true。详见 issue #111。 */
  restoreClipboardAfterPaste: boolean;
  /** Windows：TSF 失败后是否允许 SendInput / 粘贴类非 TSF 兜底。关闭后可验证是否真实 TSF 上屏。 */
  allowNonTsfInsertionFallback: boolean;
  /** 用户的工作语言（多选，原生名）；作为前提注入 LLM polish/translate prompt 头部。 */
  workingLanguages: string[];
  /** 翻译模式目标语言（单选，原生名）；空串 = 不启用 Shift 翻译。详见 issue #4。 */
  translationTargetLanguage: string;
  /** 划词语音问答快捷键。null = 未启用。详见 issue #118。 */
  qaHotkey: QaHotkeyBinding | null;
  /** 是否把 Q&A 历史写到本地存档。详见 issue #118。 */
  qaSaveHistory: boolean;
}

/** Rust 通过 `qa:state` 事件下发的 payload。
 *  v2 (issue #118 v2)：支持多轮对话，messages 数组每次由后端整段下发（单一可信源）。
 *  v2.1：开 `stream:true`，LLM 答案逐 chunk 通过 `answer_delta` 事件推前端边渲染。 */
export type QaStateKind =
  | 'idle'
  | 'recording'
  | 'loading'
  | 'thinking'
  | 'answer_delta'
  | 'answer'
  | 'error';

export interface QaChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface QaStatePayload {
  kind: QaStateKind;
  /** 后端权威：当前已有的多轮对话历史（user → assistant 交替）。answer 事件带完整版。 */
  messages?: QaChatMessage[];
  /** recording 状态时附带的选区预览（前 60 字）。 */
  selection_preview?: string | null;
  /** error 状态时附带的提示。 */
  error?: string;
  /** answer_delta 事件时附带的本帧增量字符串。 */
  chunk?: string;
  /** 后端权威：QA 面板是否已固定（pinned）。 */
  pinned?: boolean;
}

/** 内置语言列表 — 前端 Settings UI 用，后端只接收原生名字符串拼 prompt。
 *  添加新语言时直接在这里加一项（原生名），无需修改后端。 */
export const SUPPORTED_LANGUAGES: readonly string[] = [
  '简体中文',
  '繁体中文',
  'English',
  '日本語',
  '한국어',
  'Français',
  'Deutsch',
  'Español',
  'Italiano',
  'Português',
  'Русский',
  'العربية',
  'Tiếng Việt',
  'ไทย',
  'हिन्दी',
] as const;

export type CapsuleState =
  | 'idle'
  | 'recording'
  | 'transcribing'
  | 'polishing'
  | 'done'
  | 'cancelled'
  | 'error';

export interface CapsulePayload {
  state: CapsuleState;
  level: number; // 0..1 RMS
  elapsedMs: number;
  message: string | null;
  insertedChars: number | null;
  /** 当前 session 是否处于翻译模式（用户已按过 Shift）。详见 issue #4。 */
  translation: boolean;
}

export interface CredentialsStatus {
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
  | 'granted'
  | 'denied'
  | 'notDetermined'
  | 'restricted'
  | 'notApplicable';
