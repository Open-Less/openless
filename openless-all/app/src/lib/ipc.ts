// ipc.ts — typed wrapper around Tauri `invoke`. When running outside Tauri
// (e.g. `vite dev` in a browser), every command falls back to mock data so
// the UI is still operable for visual review.

import type {
  CredentialsStatus,
  DictationSession,
  DictionaryEntry,
  HotkeyCapability,
  HotkeyStatus,
  PermissionStatus,
  PolishMode,
  QaHotkeyBinding,
  UserPreferences,
} from './types';
import { OL_DATA } from './mockData';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function invokeOrMock<T>(
  cmd: string,
  args: Record<string, unknown> | undefined,
  mock: () => T,
): Promise<T> {
  if (!isTauri) {
    return mock();
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

// ── Mock fixtures ──────────────────────────────────────────────────────
const mockSettings: UserPreferences = {
  hotkey: { trigger: 'rightControl', mode: 'toggle' },
  defaultMode: 'structured',
  enabledModes: ['raw', 'light', 'structured', 'formal'],
  launchAtLogin: false,
  showCapsule: true,
  activeAsrProvider: 'volcengine',
  activeLlmProvider: 'ark',
  restoreClipboardAfterPaste: true,
  workingLanguages: ['简体中文'],
  translationTargetLanguage: '',
  qaHotkey: { primary: ';', modifiers: ['cmd', 'shift'] },
  qaSaveHistory: false,
};

const mockHotkeyCapability: HotkeyCapability = {
  adapter: 'windowsLowLevel',
  availableTriggers: ['rightControl', 'rightAlt', 'leftControl', 'rightCommand'],
  requiresAccessibilityPermission: false,
  supportsModifierOnlyTrigger: true,
  supportsSideSpecificModifiers: true,
  explicitFallbackAvailable: false,
  statusHint: '默认建议使用“右 Control + 切换式说话”；若更习惯按住说话，可在录音设置里切回。若无响应，可在权限页查看 hook 安装状态。',
};

const mockCredentialsStatus: CredentialsStatus = {
  volcengineConfigured: true,
  arkConfigured: true,
};

export interface ProviderCheckResult {
  ok: boolean;
  modelCount: number;
}

export interface ProviderModelsResult {
  models: string[];
}

const mockHotkeyStatus: HotkeyStatus = {
  adapter: 'windowsLowLevel',
  state: 'installed',
  message: 'Windows 低层键盘 hook 已安装',
  lastError: null,
};

const mockHistory: DictationSession[] = OL_DATA.history.map((h, i) => ({
  id: `mock-${i}`,
  createdAt: new Date().toISOString(),
  rawTranscript: h.preview,
  finalText: h.preview,
  mode: 'structured',
  appBundleId: null,
  appName: 'VS Code',
  insertStatus: 'inserted',
  errorCode: null,
  durationMs: 600,
  dictionaryEntryCount: 28,
}));

const mockVocab: DictionaryEntry[] = OL_DATA.vocab.map((v, i) => ({
  id: `vocab-${i}`,
  phrase: v.word,
  note: null,
  enabled: true,
  hits: v.count,
  createdAt: new Date().toISOString(),
}));

// ── Settings ───────────────────────────────────────────────────────────
export function getSettings(): Promise<UserPreferences> {
  return invokeOrMock('get_settings', undefined, () => mockSettings);
}

export function setSettings(prefs: UserPreferences): Promise<void> {
  return invokeOrMock('set_settings', { prefs }, () => undefined);
}

export function getHotkeyStatus(): Promise<HotkeyStatus> {
  return invokeOrMock('get_hotkey_status', undefined, () => mockHotkeyStatus);
}

export function getHotkeyCapability(): Promise<HotkeyCapability> {
  return invokeOrMock('get_hotkey_capability', undefined, () => mockHotkeyCapability);
}

// ── Credentials ────────────────────────────────────────────────────────
export function getCredentials(): Promise<CredentialsStatus> {
  return invokeOrMock('get_credentials', undefined, () => mockCredentialsStatus);
}

export function setCredential(account: string, value: string): Promise<void> {
  return invokeOrMock('set_credential', { account, value }, () => undefined);
}

export function setActiveAsrProvider(provider: string): Promise<void> {
  return invokeOrMock('set_active_asr_provider', { provider }, () => undefined);
}

export function setActiveLlmProvider(provider: string): Promise<void> {
  return invokeOrMock('set_active_llm_provider', { provider }, () => undefined);
}

export function readCredential(account: string): Promise<string | null> {
  return invokeOrMock<string | null>('read_credential', { account }, () => null);
}

export function validateProviderCredentials(kind: 'llm' | 'asr'): Promise<ProviderCheckResult> {
  return invokeOrMock('validate_provider_credentials', { kind }, () => ({ ok: true, modelCount: 2 }));
}

export function listProviderModels(kind: 'llm' | 'asr'): Promise<ProviderModelsResult> {
  return invokeOrMock('list_provider_models', { kind }, () => ({ models: kind === 'llm' ? ['gpt-4o', 'deepseek-v4-flash', 'deepseek-v4-pro'] : ['whisper-1'] }));
}

// ── History ────────────────────────────────────────────────────────────
export function listHistory(): Promise<DictationSession[]> {
  return invokeOrMock('list_history', undefined, () => mockHistory);
}

export function deleteHistoryEntry(id: string): Promise<void> {
  return invokeOrMock('delete_history_entry', { id }, () => undefined);
}

export function clearHistory(): Promise<void> {
  return invokeOrMock('clear_history', undefined, () => undefined);
}

// ── Vocab ──────────────────────────────────────────────────────────────
export function listVocab(): Promise<DictionaryEntry[]> {
  return invokeOrMock('list_vocab', undefined, () => mockVocab);
}

export function addVocab(phrase: string, note?: string): Promise<DictionaryEntry> {
  return invokeOrMock('add_vocab', { phrase, note }, () => ({
    id: `vocab-new-${Date.now()}`,
    phrase,
    note: note ?? null,
    enabled: true,
    hits: 0,
    createdAt: new Date().toISOString(),
  }));
}

export function removeVocab(id: string): Promise<void> {
  return invokeOrMock('remove_vocab', { id }, () => undefined);
}

export function setVocabEnabled(id: string, enabled: boolean): Promise<void> {
  return invokeOrMock('set_vocab_enabled', { id, enabled }, () => undefined);
}

// ── Dictation lifecycle ────────────────────────────────────────────────
export function startDictation(): Promise<void> {
  return invokeOrMock('start_dictation', undefined, () => undefined);
}

export function stopDictation(): Promise<void> {
  return invokeOrMock('stop_dictation', undefined, () => undefined);
}

export function cancelDictation(): Promise<void> {
  return invokeOrMock('cancel_dictation', undefined, () => undefined);
}

export function handleWindowHotkeyEvent(
  eventType: 'keydown' | 'keyup',
  key: string,
  code: string,
  repeat: boolean,
): Promise<void> {
  return invokeOrMock(
    'handle_window_hotkey_event',
    { event_type: eventType, key, code, repeat },
    () => undefined,
  );
}

// ── Polish ─────────────────────────────────────────────────────────────
export function repolish(rawText: string, mode: PolishMode): Promise<string> {
  return invokeOrMock('repolish', { rawText, mode }, () => rawText);
}

export function setDefaultPolishMode(mode: PolishMode): Promise<void> {
  return invokeOrMock('set_default_polish_mode', { mode }, () => undefined);
}

export function setStyleEnabled(mode: PolishMode, enabled: boolean): Promise<void> {
  return invokeOrMock('set_style_enabled', { mode, enabled }, () => undefined);
}

// ── Permissions ────────────────────────────────────────────────────────
export function checkAccessibilityPermission(): Promise<PermissionStatus> {
  return invokeOrMock('check_accessibility_permission', undefined, () => 'granted' as const);
}

export function requestAccessibilityPermission(): Promise<PermissionStatus> {
  return invokeOrMock('request_accessibility_permission', undefined, () => 'granted' as const);
}

export function checkMicrophonePermission(): Promise<PermissionStatus> {
  return invokeOrMock('check_microphone_permission', undefined, () => 'granted' as const);
}

export function requestMicrophonePermission(): Promise<PermissionStatus> {
  return invokeOrMock('request_microphone_permission', undefined, () => 'granted' as const);
}

export function openSystemSettings(pane: 'accessibility' | 'microphone'): Promise<void> {
  return invokeOrMock('open_system_settings', { pane }, () => undefined);
}

export function triggerMicrophonePrompt(): Promise<void> {
  return invokeOrMock('trigger_microphone_prompt', undefined, () => undefined);
}

export function restartApp(): Promise<void> {
  return invokeOrMock('restart_app', undefined, () => undefined);
}

// ── QA (划词语音问答) ───────────────────────────────────────────────────
// 详见 issue #118。后端会发 `qa:state` / `qa:dismiss` 事件；前端通过下面四个
// 命令查询与控制 QA 浮窗。
export function getQaHotkeyLabel(): Promise<string> {
  return invokeOrMock('get_qa_hotkey_label', undefined, () => 'Cmd+Shift+;');
}

export function setQaHotkey(binding: QaHotkeyBinding): Promise<void> {
  return invokeOrMock('set_qa_hotkey', { binding }, () => undefined);
}

export function qaWindowDismiss(): Promise<void> {
  return invokeOrMock('qa_window_dismiss', undefined, () => undefined);
}

export function qaWindowPin(pinned: boolean): Promise<void> {
  return invokeOrMock('qa_window_pin', { pinned }, () => undefined);
}

export async function openExternal(url: string): Promise<void> {
  if (!isTauri) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  const { open } = await import('@tauri-apps/plugin-shell');
  await open(url);
}

export { isTauri };
