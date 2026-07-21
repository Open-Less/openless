import {
  applyStylePreferencesNotification,
  isStyleMasterEnabled,
  rollbackDefaultAndEnabledChange,
  persistStylePreferenceChange,
  rollbackStyleEnabledChange,
  rollbackWholeStylePreferences,
  styleDefaultModePreferences,
  styleMasterFallbackModes,
  styleMasterOffPreferences,
} from './stylePrefs';
import type { UserPreferences } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const previousPrefs: UserPreferences = {
  hotkey: { trigger: 'rightOption', mode: 'toggle' },
  dictationHotkey: { primary: 'RightOption', modifiers: [] },
  selectionPolishHotkey: { primary: 'RightControl', modifiers: [] },
  selectionPolishStylePackId: 'builtin.light',
  selectionPolishOutputMode: 'directReplace',
  showOverviewActivityHeatmap: true,
  defaultMode: 'light',
  enabledModes: ['raw', 'light', 'structured'],
  activeStylePackId: '',
  styleSystemPrompts: {
    raw: 'raw system prompt',
    light: 'light system prompt',
    structured: 'structured system prompt',
    formal: 'formal system prompt',
  },
  customStylePrompts: { raw: '', light: '', structured: '', formal: '' },
  launchAtLogin: false,
  showCapsule: true,
  muteDuringRecording: false,
  audioCueOnRecord: true,
  microphoneDeviceName: '',
  activeAsrProvider: 'volcengine',
  activeLlmProvider: 'ark',
  llmThinkingEnabled: false,
  restoreClipboardAfterPaste: true,
  pasteShortcut: 'ctrlV',
  allowNonTsfInsertionFallback: true,
  windowsInsertionMode: 'tsf',
  windowsSendInputNewlineMode: 'enter',
  windowsSendInputInsertionOnly: false,
  windowsShowOpenlessInKeyboardList: true,
  workingLanguages: ['简体中文'],
  translationTargetLanguage: '',
  chineseScriptPreference: 'auto',
  outputLanguagePreference: 'auto',
  qaHotkey: null,
  qaSaveHistory: false,
  customComboHotkey: null,
  translationHotkey: { primary: 'Shift', modifiers: [] },
  switchStyleHotkey: { primary: 'S', modifiers: ['alt'] },
  openAppHotkey: { primary: 'O', modifiers: ['alt'] },
  codingAgentEnabled: false,
  codingAgentProvider: 'claude-code-cli',
  codingAgentModel: null,
  codingAgentPermissionMode: 'acceptEdits',
  codingAgentWorkdir: null,
  codingAgentExe: null,
  codingAgentVoiceHotkey: { primary: 'LeftControl', modifiers: [] },
  codingAgentPanelHotkey: { primary: 'Enter', modifiers: ['cmd', 'shift'] },
  codingAgentQuickHotkey: null,
  localAsrActiveModel: '',
  localAsrMirror: 'huggingface',
  localAsrKeepLoadedSecs: 300,
  foundryLocalAsrModel: '',
  foundryLocalRuntimeSource: 'auto',
  foundryLocalAsrLanguageHint: '',
  foundryLocalAsrKeepLoadedSecs: 300,
  sherpaOnnxModel: 'sense-voice-small-zh',
  sherpaOnnxLanguageHint: '',
  sherpaOnnxKeepLoadedSecs: 300,
  historyRetentionDays: 7,
  polishContextWindowMinutes: 5,
  startMinimized: false,
  themeMode: 'system',
  updateChannel: 'stable',
  streamingInsert: true,
  streamingInsertDefaultMigrated: true,
  streamingInsertSaveClipboard: true,
  autoUpdateCheck: true,
  historyMaxEntries: null,
  recordAudioForDebug: false,
  audioRecordingMaxEntries: null,
  marketplaceBaseUrl: '',
  marketplaceDevLogin: '',
  remoteInputEnabled: false,
  remoteInputPort: 8443,
  remoteInputPin: '000000',
  remoteInputDefaultMode: 'toggle',
  androidInsertStrategy: 'accessibility',
  androidOverlayTrigger: 'background',
  androidOverlayActivationMode: 'tap',
  androidOverlayLeftSwipeAction: 'translation',
  androidOverlayCancelSwipeDirection: 'up',
  androidOverlaySizeDp: 72,
};

const nextPrefs: UserPreferences = {
  ...previousPrefs,
  enabledModes: [],
};

const states: UserPreferences[] = [];
const errors: string[] = [];
let firstCurrentPrefs: UserPreferences | null = previousPrefs;
const saved = await persistStylePreferenceChange(
  nextPrefs,
  async () => {
    throw 'disk full';
  },
  update => {
    firstCurrentPrefs = typeof update === 'function' ? update(firstCurrentPrefs) : update;
    if (firstCurrentPrefs) states.push(firstCurrentPrefs);
  },
  message => errors.push(message),
  rollbackWholeStylePreferences(previousPrefs, nextPrefs),
);

assert(saved === false, 'setSettings reject should report save failure');
assert(states.length === 2, `expected optimistic state then rollback, got ${states.length} updates`);
assert(states[0] === nextPrefs, 'first state update should be the optimistic next prefs');
assert(
  states[1].enabledModes.join(',') === previousPrefs.enabledModes.join(','),
  'second state update should roll back enabled modes to previous prefs',
);
assert(errors[0] === 'disk full', `expected backend error message, got ${errors[0]}`);

let currentPrefs: UserPreferences | null = previousPrefs;
const disableLightPrefs: UserPreferences = {
  ...previousPrefs,
  enabledModes: ['raw', 'structured'],
};
const disableStructuredAfterLightPrefs: UserPreferences = {
  ...previousPrefs,
  enabledModes: ['raw'],
};
const overlapSaved = await persistStylePreferenceChange(
  disableLightPrefs,
  async () => {
    currentPrefs = disableStructuredAfterLightPrefs;
    throw 'slow failure';
  },
  update => {
    currentPrefs = typeof update === 'function' ? update(currentPrefs) : update;
  },
  () => undefined,
  rollbackStyleEnabledChange('light', previousPrefs, disableLightPrefs),
);

assert(overlapSaved === false, 'overlapped style save should still report failure');
assert(
  currentPrefs?.enabledModes.includes('light') === true,
  'failed light toggle should roll back only the light mode',
);
assert(
  currentPrefs?.enabledModes.includes('structured') === false,
  'failed light toggle should preserve newer structured edit',
);

const notifiedPrefs: UserPreferences = {
  ...previousPrefs,
  defaultMode: 'formal',
  enabledModes: ['raw', 'formal'],
};
const syncedPrefs = applyStylePreferencesNotification(previousPrefs, notifiedPrefs);
assert(syncedPrefs === notifiedPrefs, 'prefs notification should replace stale style page prefs');

const masterOffPrefs = styleMasterOffPreferences(previousPrefs);
assert(
  masterOffPrefs.enabledModes.join(',') === 'raw,light',
  `master toggle off should persist raw and current default, got ${masterOffPrefs.enabledModes.join(',')}`,
);

const masterFallback = styleMasterFallbackModes('light');
assert(
  masterFallback.join(',') === 'raw,light',
  `master toggle off should preserve raw and current default, got ${masterFallback.join(',')}`,
);
assert(
  isStyleMasterEnabled({ ...previousPrefs, enabledModes: masterFallback }) === false,
  'master toggle should render off when only raw and default remain enabled',
);
assert(
  isStyleMasterEnabled(previousPrefs) === true,
  'master toggle should render on when extra styles remain enabled',
);
const rawFallback = styleMasterFallbackModes('raw');
assert(
  rawFallback.join(',') === 'raw',
  `raw default fallback should not duplicate raw, got ${rawFallback.join(',')}`,
);


const defaultAfterMasterOff = styleDefaultModePreferences(
  { ...previousPrefs, enabledModes: ['raw', 'light'] },
  'formal',
);
assert(
  defaultAfterMasterOff.defaultMode === 'formal' && defaultAfterMasterOff.enabledModes.join(',') === 'raw,formal',
  `default change while master is off should refresh fallback modes, got ${defaultAfterMasterOff.defaultMode}/${defaultAfterMasterOff.enabledModes.join(',')}`,
);
assert(
  isStyleMasterEnabled(defaultAfterMasterOff) === false,
  'master toggle should stay off after changing default while off',
);

let rolledBackDefaultAndEnabled: UserPreferences | null = defaultAfterMasterOff;
const rollbackDefaultAndEnabled = rollbackDefaultAndEnabledChange(
  { ...previousPrefs, enabledModes: ['raw', 'light'] },
  defaultAfterMasterOff,
);
rolledBackDefaultAndEnabled = rollbackDefaultAndEnabled(rolledBackDefaultAndEnabled);
assert(
  rolledBackDefaultAndEnabled?.defaultMode === 'light' && rolledBackDefaultAndEnabled.enabledModes.join(',') === 'raw,light',
  'failed off-state default save should roll back both default mode and enabled modes',
);
