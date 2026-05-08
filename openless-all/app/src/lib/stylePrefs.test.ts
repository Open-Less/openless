import {
  persistStylePreferenceChange,
  rollbackStyleEnabledChange,
  rollbackWholeStylePreferences,
} from './stylePrefs';
import type { UserPreferences } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const previousPrefs: UserPreferences = {
  hotkey: { trigger: 'rightOption', mode: 'toggle' },
  dictationHotkey: { primary: 'RightOption', modifiers: [] },
  defaultMode: 'light',
  enabledModes: ['raw', 'light', 'structured'],
  launchAtLogin: false,
  showCapsule: true,
  muteDuringRecording: false,
  microphoneDeviceName: '',
  activeAsrProvider: 'volcengine',
  activeLlmProvider: 'ark',
  restoreClipboardAfterPaste: true,
  allowNonTsfInsertionFallback: true,
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
  localAsrActiveModel: '',
  localAsrMirror: 'huggingface',
  localAsrKeepLoadedSecs: 300,
  foundryLocalAsrModel: '',
  foundryLocalRuntimeSource: 'auto',
  foundryLocalAsrLanguageHint: '',
  foundryLocalAsrKeepLoadedSecs: 300,
  historyRetentionDays: 7,
  polishContextWindowMinutes: 5,
  startMinimized: false,
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
