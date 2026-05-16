// ipc.ts — typed wrapper around Tauri `invoke`. When running outside Tauri
// (e.g. `vite dev` in a browser), every command falls back to mock data so
// the UI is still operable for visual review.

import type {
  ComboBinding,
  CorrectionRule,
  CredentialsStatus,
  DictationSession,
  DictionaryEntry,
  HotkeyCapability,
  HotkeyStatus,
  MicrophoneDevice,
  PermissionStatus,
  PolishMode,
  QaHotkeyBinding,
  ShortcutBinding,
  StylePack,
  StylePackExample,
  StylePackKind,
  StylePackRuntimeDiagnostics,
  StyleSystemPrompts,
  UpdateChannel,
  UserPreferences,
  VocabPresetStore,
  WindowsImeStatus,
} from './types';
export type { UpdateChannel } from './types';
import { OL_DATA } from './mockData';
import { defaultAppShortcutModifiers, defaultQaShortcut, formatComboLabel } from './hotkey';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const isTauri =
  globalThis.window !== undefined && '__TAURI_INTERNALS__' in globalThis.window;

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
let mockSettings: UserPreferences = {
  hotkey: { trigger: 'rightControl', mode: 'toggle', keys: [{ code: 'ControlRight' }] },
  dictationHotkey: { primary: 'RightControl', modifiers: [] },
  defaultMode: 'structured',
  enabledModes: ['raw', 'light', 'structured', 'formal'],
  activeStylePackId: 'builtin.structured',
  styleSystemPrompts: {
    raw: '只做最小化整理：补全标点、必要分句，保留原话顺序、用词和语气。',
    light: '把口语转写整理成自然文字，去掉口癖和重复，保留原意与语气。',
    structured: '把口述整理成结构清晰的文本，必要时按主题分组输出。',
    formal: '输出适合工作沟通与邮件场景的正式表达，不扩写事实。',
  },
  customStylePrompts: { raw: '', light: '', structured: '', formal: '' },
  launchAtLogin: false,
  showCapsule: true,
  muteDuringRecording: false,
  microphoneDeviceName: '',
  activeAsrProvider: 'foundry-local-whisper',
  activeLlmProvider: 'ark',
  llmThinkingEnabled: false,
  restoreClipboardAfterPaste: true,
  pasteShortcut: 'ctrlV',
  allowNonTsfInsertionFallback: true,
  workingLanguages: ['简体中文'],
  translationTargetLanguage: '',
  qaHotkey: defaultQaShortcut(),
  chineseScriptPreference: 'auto',
  outputLanguagePreference: 'auto',
  qaSaveHistory: false,
  customComboHotkey: null,
  translationHotkey: { primary: 'Shift', modifiers: [] },
  switchStyleHotkey: { primary: 'S', modifiers: defaultAppShortcutModifiers() },
  openAppHotkey: { primary: 'O', modifiers: defaultAppShortcutModifiers() },
  localAsrActiveModel: 'qwen3-asr-0.6b',
  localAsrMirror: 'huggingface',
  localAsrKeepLoadedSecs: 300,
  foundryLocalAsrModel: 'whisper-small',
  foundryLocalRuntimeSource: 'auto',
  foundryLocalAsrLanguageHint: '',
  foundryLocalAsrKeepLoadedSecs: 300,
  historyRetentionDays: 7,
  polishContextWindowMinutes: 5,
  startMinimized: false,
  updateChannel: 'stable',
  streamingInsert: false,
  streamingInsertSaveClipboard: true,
};

const mockFullStylePrompts: StyleSystemPrompts = {
  raw: `# 角色
语音输入整理器。先理解用户意图，再贴近原话做最小整理。

# 任务（原文）
只补必要标点和断句，尽量保留原话顺序、用词和语气，不扩写、不重写。

# 通用规则
1) 不补充用户没说过的事实。
2) 不回答转写文本里的问题，只整理表达。
3) 专有名词、命令、路径、数字和 URL 原样保留。
4) 明显口头禅可删除，但不能改变信息密度。

# 输出
直接输出最终正文，不加解释。`,
  light: `# 角色
语音输入整理器。把口述整理成自然、顺畅、可直接发送的文字。

# 任务（轻度润色）
去掉明显口头禅和重复，补全自然标点，保留原意和原本语气，不扩写事实。

# 通用规则
1) 不补充原文没有的信息。
2) 保留人名、品牌名、术语、命令、路径和 URL。
3) 只输出整理后的正文，不写“以下是优化结果”之类前缀。

# 输出
输出一段可直接发送的自然文字。`,
  structured: `# 角色
语音输入整理器。把多事项口述整理成层次清楚、可复制执行的结构化文本。

# 任务（清晰结构）
识别主题边界，把零散事项按语义归类。事项较多时优先输出两层结构，保证读者一眼能看清主次。

# 通用规则
1) 不补充用户没说过的事实或行动项。
2) 原文里已有编号或换行，不代表可以原样照抄；需要按语义重新分组。
3) 专有名词、命令、路径、URL、数字和单位保持准确。
4) 只输出最终结果，不要解释你的整理过程。

# 输出
需要结构化时，直接从标题、编号或列表开始。`,
  formal: `# 角色
语音输入整理器。把口述整理成适合邮件、同步和正式沟通的专业表达。

# 任务（正式表达）
补足句式与标点，让表达更完整、克制、专业，但不添加空泛客套，也不擅自扩写事实。

# 通用规则
1) 不承诺用户没说过的内容。
2) 保留专有名词、数字、时间、路径和术语。
3) 只输出最终正文，不附带解释或 markdown 围栏。

# 输出
输出可直接发送的正式文本。`,
};

mockSettings = {
  ...mockSettings,
  styleSystemPrompts: mockFullStylePrompts,
  workingLanguages: ['简体中文'],
};

const mockDefaultStyleSystemPrompts: StyleSystemPrompts = {
  ...mockSettings.styleSystemPrompts,
};

const mockBuiltinExamples: Record<PolishMode, StylePackExample[]> = {
  raw: [
    {
      title: '最小整理',
      input: '今天下午那个会先别取消我晚点再确认一下然后把下周二也先空出来',
      output: '今天下午那个会先别取消，我晚点再确认一下。然后把下周二也先空出来。',
    },
  ],
  light: [
    {
      title: '聊天消息',
      input: '你帮我跟设计那边说一下这个首页先别上线我晚上再过一遍',
      output: '你帮我跟设计那边说一下，这个首页先别上线，我今晚再过一遍。',
    },
  ],
  structured: [
    {
      title: '任务整理',
      input: '这周要做三件事一个是把登录页 bug 修掉第二个是补 README 第三个是把发版脚本再走一遍',
      output: '这周要完成以下三件事：\n1. 修复登录页相关 bug。\n2. 补充 README 文档。\n3. 重新走一遍发版脚本。',
    },
  ],
  formal: [
    {
      title: '工作同步',
      input: '你帮我发个消息说这个需求今天先不上了等测试和产品都确认完我们再一起推进',
      output: '麻烦帮我同步一下：这个需求今天先不上线，待测试和产品都确认完成后，我们再统一推进。',
    },
  ],
};

function makeMockStylePack(
  id: string,
  kind: StylePackKind,
  baseMode: PolishMode,
  name: string,
  description: string,
  prompt: string,
  tags: string[],
): StylePack {
  return {
    id,
    name,
    description,
    author: 'OpenLess',
    version: '1.0.0',
    kind,
    baseMode,
    prompt,
    examples: mockBuiltinExamples[baseMode].map(example => ({ ...example })),
    tags,
    iconPath: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    enabled: true,
    active: false,
    recommendedModel: null,
    compatibleAppVersion: '1.0.0',
  };
}

let mockStylePacks: StylePack[] = [
  makeMockStylePack(
    'builtin.raw',
    'builtin',
    'raw',
    '原文',
    '盡量保留原話順序和語氣，只做必要的斷句與標點整理。',
    mockSettings.styleSystemPrompts.raw,
    ['原文', '最小改寫'],
  ),
  makeMockStylePack(
    'builtin.light',
    'builtin',
    'light',
    '輕度潤色',
    '把口述整理成順暢、自然、可直接發送的文字，不擴寫事實。',
    mockSettings.styleSystemPrompts.light,
    ['溝通', '自然'],
  ),
  makeMockStylePack(
    'builtin.structured',
    'builtin',
    'structured',
    '清晰結構',
    '適合多事項和多主題口述，自動整理為層次清楚的結構化輸出。',
    mockSettings.styleSystemPrompts.structured,
    ['結構化', '條理'],
  ),
  makeMockStylePack(
    'builtin.formal',
    'builtin',
    'formal',
    '正式表達',
    '適合郵件、同步和工作溝通場景，語氣更完整、專業、克制。',
    mockSettings.styleSystemPrompts.formal,
    ['正式', '工作溝通'],
  ),
  {
    ...makeMockStylePack(
      'imported.creator-note',
      'imported',
      'light',
      '创作者口播',
      '给短视频口播和社区帖文使用，句子更紧凑，保留情绪和节奏。',
      '你是一个负责整理创作者口播稿的编辑。请把输入整理成适合发帖和口播的自然文本，保留节奏感，不要补充原文没有的信息。',
      ['社区', '口播', '节奏感'],
    ),
    author: 'Demo Community',
  },
];

function cloneStylePack(stylePack: StylePack): StylePack {
  return {
    ...stylePack,
    tags: [...stylePack.tags],
    examples: stylePack.examples.map(example => ({ ...example })),
  };
}

function cloneMockStylePacks(): StylePack[] {
  return mockStylePacks.map(cloneStylePack);
}

function composeMockStylePackRuntimeDiagnostics(stylePack: StylePack): StylePackRuntimeDiagnostics {
  const trimmedPrompt = stylePack.prompt.trimEnd();
  const contextPremise = mockSettings.workingLanguages.length
    ? ['# Context', `Working languages: ${mockSettings.workingLanguages.join(', ')}`].join('\n')
    : '';
  const hotwordLines = [`GitHub`, `OpenLess`];
  const hotwordBlock = hotwordLines.length > 0
    ? ['Hotwords (keep the spelling below when they appear in the transcript):', ...hotwordLines.map(word => `- ${word}`)].join('\n')
    : '';
  const singleTurnPrompt = [contextPremise, trimmedPrompt, hotwordBlock].filter(Boolean).join('\n\n');
  const historyInstruction = 'When prior turns exist, do not repeat previous assistant outputs. Only polish the current transcript.';
  const multiTurnPrompt = `${singleTurnPrompt}\n\n${historyInstruction}`;
  return {
    packId: stylePack.id,
    packName: stylePack.name,
    packPrompt: stylePack.prompt,
    packPromptChars: stylePack.prompt.length,
    contextPremise,
    contextPremiseChars: contextPremise.length,
    hotwordBlock,
    hotwordBlockChars: hotwordBlock.length,
    historyInstruction,
    historyInstructionChars: historyInstruction.length,
    singleTurnPrompt,
    singleTurnPromptChars: singleTurnPrompt.length,
    multiTurnPrompt,
    multiTurnPromptChars: multiTurnPrompt.length,
    workingLanguages: [...mockSettings.workingLanguages],
    hotwords: [...hotwordLines],
    contextWindowMinutes: mockSettings.polishContextWindowMinutes,
    includesContextPremise: Boolean(contextPremise),
    includesHotwordBlock: hotwordLines.length > 0,
    includesHistoryInstruction: true,
    previewOmitsFrontApp: true,
  };
}

function syncMockSettingsFromStylePacks() {
  const enabled = mockStylePacks.filter(pack => pack.enabled);
  const active =
    mockStylePacks.find(pack => pack.id === mockSettings.activeStylePackId && pack.enabled) ??
    enabled[0] ??
    mockStylePacks[0];
  mockStylePacks = mockStylePacks.map(pack => ({
    ...pack,
    active: pack.id === active.id,
  }));
  mockSettings = {
    ...mockSettings,
    activeStylePackId: active.id,
    defaultMode: active.baseMode,
    enabledModes: ['raw', 'light', 'structured', 'formal'].filter(mode =>
      mockStylePacks.some(pack => pack.enabled && pack.baseMode === mode),
    ) as PolishMode[],
    styleSystemPrompts: {
      raw: mockStylePacks.find(pack => pack.id === 'builtin.raw')?.prompt ?? mockSettings.styleSystemPrompts.raw,
      light:
        mockStylePacks.find(pack => pack.id === 'builtin.light')?.prompt ??
        mockSettings.styleSystemPrompts.light,
      structured:
        mockStylePacks.find(pack => pack.id === 'builtin.structured')?.prompt ??
        mockSettings.styleSystemPrompts.structured,
      formal:
        mockStylePacks.find(pack => pack.id === 'builtin.formal')?.prompt ??
        mockSettings.styleSystemPrompts.formal,
    },
  };
}

syncMockSettingsFromStylePacks();

const mockHotkeyCapability: HotkeyCapability = {
  adapter: 'windowsLowLevel',
  availableTriggers: ['rightControl', 'rightAlt', 'leftControl', 'rightCommand', 'custom'],
  requiresAccessibilityPermission: false,
  supportsModifierOnlyTrigger: true,
  supportsSideSpecificModifiers: true,
  explicitFallbackAvailable: false,
  statusHint: '默认建议使用“右Ctrl + 单击”；若更习惯按住说话，可在录音设置里切回“按住”。若无响应，可在权限页查看 hook 安装状态。',
};

const mockCredentialsStatus: CredentialsStatus = {
  activeAsrProvider: 'foundry-local-whisper',
  activeLlmProvider: 'ark',
  asrConfigured: true,
  llmConfigured: true,
  volcengineConfigured: true,
  arkConfigured: true,
};

export interface ProviderCheckResult {
  ok: boolean;
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

const mockWindowsImeStatus: WindowsImeStatus = {
  state: 'notWindows',
  usingTsfBackend: false,
  message: 'Browser dev mock',
  dllPath: null,
};

const mockMicrophoneDevices: MicrophoneDevice[] = [
  { name: 'Built-in Microphone', isDefault: true },
  { name: 'USB Microphone', isDefault: false },
];

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

const mockCorrectionRules: CorrectionRule[] = [
  {
    id: 'rule-quantity-classifier',
    pattern: '{num}粒',
    replacement: '{num}例',
    enabled: true,
    createdAt: new Date().toISOString(),
  },
];

// ── Settings ───────────────────────────────────────────────────────────
export function getSettings(): Promise<UserPreferences> {
  return invokeOrMock('get_settings', undefined, () => ({ ...mockSettings }));
}

export function getDefaultStyleSystemPrompts(): Promise<StyleSystemPrompts> {
  return invokeOrMock('get_default_style_system_prompts', undefined, () => ({ ...mockDefaultStyleSystemPrompts }));
}

export function setSettings(prefs: UserPreferences): Promise<void> {
  return invokeOrMock('set_settings', { prefs }, () => {
    mockSettings = { ...prefs };
    mockStylePacks = mockStylePacks.map(pack => {
      if (pack.kind === 'builtin') {
        return {
          ...pack,
          enabled: prefs.enabledModes.includes(pack.baseMode),
          prompt: prefs.styleSystemPrompts[pack.baseMode],
        };
      }
      return { ...pack };
    });
    syncMockSettingsFromStylePacks();
    return undefined;
  });
}

// ── Release channel (Beta opt-in) ──────────────────────────────────────
// 渠道偏好与 fetch_latest_beta_release 实际效果只在 Tauri runtime 内有意义；
// 浏览器开发模式下走 mock，避免设置页因 invoke 抛错而白屏。
// UpdateChannel 类型搬到 types.ts（UserPreferences.updateChannel 字段使用），
// 这里 re-export 保持外部模块（SettingsModal 等）import 路径不变。

export interface LatestBetaRelease {
  tagName: string;
  htmlUrl: string;
  publishedAt: string;
}

export function getUpdateChannel(): Promise<UpdateChannel> {
  return invokeOrMock('get_update_channel', undefined, () => 'stable' as UpdateChannel);
}

export function setUpdateChannel(channel: UpdateChannel): Promise<void> {
  return invokeOrMock('set_update_channel', { channel }, () => undefined);
}

export function fetchLatestBetaRelease(): Promise<LatestBetaRelease | null> {
  return invokeOrMock('fetch_latest_beta_release', undefined, () => null);
}

export function getHotkeyStatus(): Promise<HotkeyStatus> {
  return invokeOrMock('get_hotkey_status', undefined, () => mockHotkeyStatus);
}

export function getHotkeyCapability(): Promise<HotkeyCapability> {
  return invokeOrMock('get_hotkey_capability', undefined, () => mockHotkeyCapability);
}

// Linux/Wayland 检测：rdev 监听在 Wayland 协议层面失败（issue #420），需引导用户
// 把 `openless --toggle-dictation` 绑到桌面环境快捷键。浏览器 / 非 Tauri 环境下永远 false。
export function isWaylandCliMode(): Promise<boolean> {
  return invokeOrMock('is_wayland_cli_mode', undefined, () => false);
}

export function getWindowsImeStatus(): Promise<WindowsImeStatus> {
  return invokeOrMock('get_windows_ime_status', undefined, () => mockWindowsImeStatus);
}

export function listMicrophoneDevices(): Promise<MicrophoneDevice[]> {
  return invokeOrMock('list_microphone_devices', undefined, () => mockMicrophoneDevices);
}

export function startMicrophoneLevelMonitor(deviceName: string): Promise<void> {
  return invokeOrMock('start_microphone_level_monitor', { deviceName }, () => undefined);
}

export function stopMicrophoneLevelMonitor(): Promise<void> {
  return invokeOrMock('stop_microphone_level_monitor', undefined, () => undefined);
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
  return invokeOrMock('validate_provider_credentials', { kind }, () => ({ ok: true }));
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

export function listCorrectionRules(): Promise<CorrectionRule[]> {
  return invokeOrMock('list_correction_rules', undefined, () => mockCorrectionRules);
}

export function addCorrectionRule(pattern: string, replacement: string): Promise<CorrectionRule> {
  return invokeOrMock('add_correction_rule', { pattern, replacement }, () => ({
    id: `rule-new-${Date.now()}`,
    pattern,
    replacement,
    enabled: true,
    createdAt: new Date().toISOString(),
  }));
}

export function removeCorrectionRule(id: string): Promise<void> {
  return invokeOrMock('remove_correction_rule', { id }, () => undefined);
}

export function setCorrectionRuleEnabled(id: string, enabled: boolean): Promise<void> {
  return invokeOrMock('set_correction_rule_enabled', { id, enabled }, () => undefined);
}

export function listVocabPresets(): Promise<VocabPresetStore> {
  return invokeOrMock('list_vocab_presets', undefined, () => ({
    custom: [],
    overrides: [],
    disabledBuiltinPresetIds: [],
  }));
}

export function saveVocabPresets(store: VocabPresetStore): Promise<void> {
  return invokeOrMock('save_vocab_presets', { store }, () => undefined);
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
  return invokeOrMock('set_default_polish_mode', { mode }, () => {
    const packId = `builtin.${mode}`;
    mockStylePacks = mockStylePacks.map(pack => ({
      ...pack,
      enabled: pack.id === packId ? true : pack.enabled,
      active: pack.id === packId,
    }));
    mockSettings = { ...mockSettings, activeStylePackId: packId };
    syncMockSettingsFromStylePacks();
    return undefined;
  });
}

export function setStyleEnabled(mode: PolishMode, enabled: boolean): Promise<void> {
  return invokeOrMock('set_style_enabled', { mode, enabled }, () => {
    const packId = `builtin.${mode}`;
    mockStylePacks = mockStylePacks.map(pack =>
      pack.id === packId ? { ...pack, enabled } : { ...pack },
    );
    syncMockSettingsFromStylePacks();
    return undefined;
  });
}

export function listStylePacks(): Promise<StylePack[]> {
  return invokeOrMock('list_style_packs', undefined, () => cloneMockStylePacks());
}

export function saveStylePack(stylePack: StylePack): Promise<StylePack> {
  return invokeOrMock('save_style_pack', { stylePack }, () => {
    mockStylePacks = mockStylePacks.map(pack => (pack.id === stylePack.id ? cloneStylePack(stylePack) : pack));
    syncMockSettingsFromStylePacks();
    return cloneStylePack(mockStylePacks.find(pack => pack.id === stylePack.id) ?? stylePack);
  });
}

export function previewStylePackRuntime(stylePack: StylePack): Promise<StylePackRuntimeDiagnostics> {
  return invokeOrMock('preview_style_pack_runtime', { stylePack }, () => composeMockStylePackRuntimeDiagnostics(stylePack));
}

export function setActiveStylePack(id: string): Promise<StylePack> {
  return invokeOrMock('set_active_style_pack', { id }, () => {
    mockStylePacks = mockStylePacks.map(pack => ({
      ...pack,
      enabled: pack.id === id ? true : pack.enabled,
      active: pack.id === id,
    }));
    mockSettings = { ...mockSettings, activeStylePackId: id };
    syncMockSettingsFromStylePacks();
    return cloneStylePack(mockStylePacks.find(pack => pack.id === id)!);
  });
}

export function setStylePackEnabled(id: string, enabled: boolean): Promise<StylePack[]> {
  return invokeOrMock('set_style_pack_enabled', { id, enabled }, () => {
    mockStylePacks = mockStylePacks.map(pack =>
      pack.id === id ? { ...pack, enabled } : { ...pack },
    );
    syncMockSettingsFromStylePacks();
    return cloneMockStylePacks();
  });
}

export function resetBuiltinStylePack(id: string): Promise<StylePack> {
  return invokeOrMock('reset_builtin_style_pack', { id }, () => {
    const builtinDefaults: Record<string, StylePack> = {
      'builtin.raw': makeMockStylePack(
        'builtin.raw',
        'builtin',
        'raw',
        '原文',
        '尽量保留原话顺序和语气，只做必要的断句与标点整理。',
        mockDefaultStyleSystemPrompts.raw,
        ['原文', '最小改写'],
      ),
      'builtin.light': makeMockStylePack(
        'builtin.light',
        'builtin',
        'light',
        '轻度润色',
        '把口述整理成顺畅、自然、可直接发送的文字，不扩写事实。',
        '把口述整理成自然、顺畅、可直接发送的文字，去掉口头禅和重复，保留原意与语气。',
        ['沟通', '自然'],
      ),
      'builtin.structured': makeMockStylePack(
        'builtin.structured',
        'builtin',
        'structured',
        '清晰结构',
        '适合多事项和多主题口述，自动整理为层次清楚的结构化输出。',
        '把口述整理成结构清楚的文本，必要时按主题分组或分点输出。',
        ['结构化', '条理'],
      ),
      'builtin.formal': makeMockStylePack(
        'builtin.formal',
        'builtin',
        'formal',
        '正式表达',
        '适合邮件、同步和工作沟通场景，语气更完整、专业、克制。',
        '输出适合工作沟通、邮件和汇报场景的正式表达，不扩写事实。',
        ['正式', '工作沟通'],
      ),
    };
    const current = mockStylePacks.find(pack => pack.id === id);
    const reset = builtinDefaults[id];
    if (!current || !reset) {
      throw new Error(`style pack not found: ${id}`);
    }
    mockStylePacks = mockStylePacks.map(pack =>
      pack.id === id
        ? {
            ...reset,
            enabled: current.enabled,
            active: current.active,
          }
        : pack,
    );
    syncMockSettingsFromStylePacks();
    return cloneStylePack(mockStylePacks.find(pack => pack.id === id)!);
  });
}

export function deleteStylePack(id: string): Promise<void> {
  return invokeOrMock('delete_style_pack', { id }, () => {
    mockStylePacks = mockStylePacks.filter(pack => pack.id !== id);
    syncMockSettingsFromStylePacks();
    return undefined;
  });
}

export function importStylePackFromZip(zipPath: string): Promise<StylePack> {
  return invokeOrMock('import_style_pack_from_zip', { zipPath }, () => {
    const seed = Date.now();
    const pack = {
      ...makeMockStylePack(
        `imported.mock-${seed}`,
        'imported',
        'light',
        '导入风格包',
        `从 ${zipPath.split(/[\\\\/]/).pop() || 'ZIP'} 导入的风格包`,
        '你是一个负责把口述整理成清晰、利落、适合社区分享文本的编辑，请完整保留事实，不要补充原文没有的信息。',
        ['导入', 'ZIP'],
      ),
      author: 'Imported ZIP',
    };
    mockStylePacks = [pack, ...mockStylePacks];
    syncMockSettingsFromStylePacks();
    return cloneStylePack(pack);
  });
}

export function exportStylePackToZip(id: string, targetPath: string): Promise<string> {
  return invokeOrMock('export_style_pack_to_zip', { id, targetPath }, () => targetPath);
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
  return invokeOrMock('get_qa_hotkey_label', undefined, () => formatComboLabel(defaultQaShortcut()));
}

export function setQaHotkey(binding: QaHotkeyBinding | null): Promise<void> {
  return invokeOrMock('set_qa_hotkey', { binding }, () => undefined);
}

export function qaWindowDismiss(): Promise<void> {
  return invokeOrMock('qa_window_dismiss', undefined, () => undefined);
}

export function qaWindowPin(pinned: boolean): Promise<void> {
  return invokeOrMock('qa_window_pin', { pinned }, () => undefined);
}

// ── Combo Hotkey (自定义录音组合键) ───────────────────────────────────
export function validateComboHotkey(binding: ComboBinding): Promise<void> {
  return invokeOrMock('validate_combo_hotkey', { binding }, () => undefined);
}

export function setComboHotkey(binding: ComboBinding): Promise<void> {
  return invokeOrMock('set_combo_hotkey', { binding }, () => undefined);
}

export function validateShortcutBinding(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('validate_shortcut_binding', { binding }, () => undefined);
}

export function setDictationHotkey(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('set_dictation_hotkey', { binding }, () => undefined);
}

export function setTranslationHotkey(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('set_translation_hotkey', { binding }, () => undefined);
}

export function setSwitchStyleHotkey(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('set_switch_style_hotkey', { binding }, () => undefined);
}

export function setOpenAppHotkey(binding: ShortcutBinding): Promise<void> {
  return invokeOrMock('set_open_app_hotkey', { binding }, () => undefined);
}

export function setShortcutRecordingActive(active: boolean): Promise<void> {
  return invokeOrMock('set_shortcut_recording_active', { active }, () => undefined);
}

export async function openExternal(url: string): Promise<void> {
  if (!isTauri) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  const { open } = await import('@tauri-apps/plugin-shell');
  await open(url);
}

/**
 * 让用户选 save 路径并把当前会话日志（openless.log）复制过去。
 * 浏览器开发模式下走 mock 不实际写盘。返回最终 save 的绝对路径，取消选择则返回 null。
 */
export async function exportErrorLog(suggestedFileName: string): Promise<string | null> {
  if (!isTauri) {
    return `~/Downloads/${suggestedFileName}`;
  }
  const { save } = await import('@tauri-apps/plugin-dialog');
  const target = await save({
    defaultPath: suggestedFileName,
    filters: [{ name: 'Log', extensions: ['log', 'txt'] }],
  });
  if (!target) return null;
  await invokeOrMock<void>('export_error_log', { targetPath: target }, () => undefined);
  return target;
}

export { isTauri };
