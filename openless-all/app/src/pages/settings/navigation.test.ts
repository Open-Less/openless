import {
  ADVANCED_PAGES,
  SETTINGS_SECTIONS,
  searchSettingsSections,
  showsCursorContextControl,
  visibleSettingsSections,
  availableServiceViews,
  isServiceViewInactive,
  resolveServiceView,
  visibleAdvancedPages,
} from './navigation';

const assert = {
  deepEqual(actual: unknown, expected: unknown, message: string) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(message);
  },
  equal(actual: unknown, expected: unknown, message: string) {
    if (actual !== expected) throw new Error(message);
  },
};

const sections = [
  {
    id: 'general' as const,
    icon: 'mic',
    title: '录音与输入',
    description: '麦克风与手机输入',
    keywords: 'microphone remote PIN',
  },
  {
    id: 'services' as const,
    icon: 'cloud',
    title: 'AI 服务与模型',
    description: '配置识别和润色',
    keywords: 'API 本地模型 ASR LLM',
  },
];

assert.deepEqual(
  searchSettingsSections(sections, '  ＡＰＩ  ').map((item) => item.id),
  ['services'],
  'search normalizes width, case and surrounding whitespace',
);
assert.deepEqual(
  searchSettingsSections(sections, '润色').map((item) => item.id),
  ['services'],
  'search includes the explanation',
);
assert.deepEqual(
  searchSettingsSections(sections, '输入 PIN').map((item) => item.id),
  ['general'],
  'each search term must match within the same category',
);
assert.deepEqual(
  searchSettingsSections(sections, '麦克风 LLM'),
  [],
  'terms from different categories do not create a false result',
);
assert.deepEqual(searchSettingsSections(sections, '不存在'), [], 'unknown query has no results');
assert.deepEqual(
  searchSettingsSections(sections, '  '),
  sections,
  'clearing a query restores all categories',
);
assert.equal(
  visibleSettingsSections(false).some((item) => item.id === 'shortcuts'),
  false,
  'mobile cannot reach an empty desktop-only category',
);
assert.equal(
  visibleSettingsSections(true).some((item) => item.id === 'shortcuts'),
  true,
  'desktop retains shortcuts',
);
assert.equal(
  visibleSettingsSections(false, 'android').some((item) => item.id === 'inputMethod'),
  true,
  'Android exposes input method settings',
);
assert.equal(
  visibleSettingsSections(true, 'desktop').some((item) => item.id === 'inputMethod'),
  false,
  'desktop hides Android input method settings',
);
assert.equal(
  visibleAdvancedPages('desktop', 'win').some((item) => String(item.id) === 'lessComputer'),
  false,
  'Less Computer is retired even for opted-in devices',
);
assert.equal(
  visibleAdvancedPages('desktop', 'win').some((item) => String(item.id) === 'claudeConsole'),
  false,
  'the macOS Claude console is not exposed on Windows',
);
assert.deepEqual(
  visibleAdvancedPages('android', 'android').map((item) => item.id),
  [
    'webSearch',
    'voiceEnhancement',
    'cursorContext',
    'startupDiagnostics',
    'customSyncServer',
    'debug',
  ],
  'the pipeline switch lives in Services on every platform',
);
console.log('settings navigation tests passed');

assert.equal(
  visibleAdvancedPages('mobile', 'mac').some((item) => item.id === 'cursorContext'),
  true,
  'short-input settings stay on the text experiments page without a native cursor reader',
);
assert.equal(
  showsCursorContextControl('mobile', 'mac'),
  false,
  'generic mobile hosts without a native cursor reader do not expose cursor context',
);
assert.equal(showsCursorContextControl('desktop', 'mac'), true, 'macOS keeps cursor context');
assert.equal(showsCursorContextControl('desktop', 'win'), true, 'Windows keeps cursor context');
assert.equal(showsCursorContextControl('android', 'android'), true, 'Android keeps cursor context');
assert.equal(
  showsCursorContextControl('harmony', 'harmony'),
  false,
  'Harmony has no cursor-context reader',
);
const advancedIds: readonly string[] = ADVANCED_PAGES.map((item) => item.id);
assert.equal(
  advancedIds.includes('vocabularyLearning'),
  false,
  'correction learning shares the awareness page rather than a duplicate menu entry',
);

assert.equal(
  advancedIds.includes('cursorContext'),
  true,
  'awareness remains reachable from advanced features',
);
assert.equal(
  visibleAdvancedPages('desktop', 'mac').some((item) => item.id === 'liveInput'),
  false,
  'retired macOS in-field streaming is not offered',
);
assert.equal(
  visibleAdvancedPages('desktop', 'win').some((item) => item.id === 'liveInput'),
  true,
  'Windows retains its advanced text insertion page',
);
assert.equal(
  visibleAdvancedPages('android', 'android').some((item) => item.id === 'liveInput'),
  false,
  'Windows text insertion is not offered to Android',
);

const sectionIds = SETTINGS_SECTIONS.map((item) => item.id);
assert.equal(
  sectionIds.indexOf('cloudServices'),
  sectionIds.indexOf('services') + 1,
  'cloud services follows AI',
);
assert.equal(
  sectionIds[sectionIds.indexOf('cloudServices') + 1],
  'appearance',
  'cloud services precedes appearance',
);
assert.equal(
  SETTINGS_SECTIONS.find((item) => item.id === 'services')?.icon,
  'sparkle',
  'AI does not use the cloud icon',
);
assert.equal(
  SETTINGS_SECTIONS.find((item) => item.id === 'advanced')?.icon,
  'flask',
  'experiments do not reuse the AI sparkle icon',
);
assert.equal(
  SETTINGS_SECTIONS.find((item) => item.id === 'cloudServices')?.icon,
  'cloud',
  'the cloud icon belongs to cloud services',
);

assert.equal(
  visibleAdvancedPages('desktop', 'mac')[0].id,
  'localModels',
  'advanced features opens its peer pages directly',
);
assert.equal(
  visibleAdvancedPages('desktop', 'win').some((page) => page.id === 'localModels'),
  false,
  'Windows experiments stay unchanged',
);
assert.equal(
  availableServiceViews(false, 'mac').includes('models'),
  true,
  'macOS retains only a disabled retirement placeholder',
);
assert.equal(
  availableServiceViews(true, 'win').includes('models'),
  true,
  'Windows keeps model management',
);

assert.equal(
  isServiceViewInactive('models', false, 'mac'),
  true,
  'Mac legacy entry is always disabled',
);
assert.equal(
  resolveServiceView('models', availableServiceViews(false, 'mac'), false, 'mac'),
  'llm',
  'retired entry never opens old model controls',
);
const serviceViews = availableServiceViews(true);
assert.deepEqual(
  serviceViews,
  ['llm', 'asr', 'omni', 'models', 'connections'],
  'both pipeline configurations remain visible in the same order',
);
assert.equal(
  resolveServiceView('llm', serviceViews, true),
  'omni',
  'switching to multimodal leaves the disabled traditional editor',
);
assert.equal(
  resolveServiceView('omni', serviceViews, false),
  'llm',
  'switching to traditional leaves the disabled multimodal editor',
);
assert.equal(
  resolveServiceView('models', serviceViews, true),
  'omni',
  'multimodal mode cannot retain a previously selected local model page',
);
assert.equal(
  resolveServiceView('models', serviceViews, false),
  'models',
  'traditional mode retains local model management on supported platforms',
);
assert.deepEqual(
  serviceViews.filter((view) => isServiceViewInactive(view, true)),
  ['llm', 'asr', 'models'],
  'multimodal mode disables traditional services and local models',
);
assert.deepEqual(
  serviceViews.filter((view) => isServiceViewInactive(view, false)),
  ['omni'],
  'traditional mode only disables the multimodal page',
);
assert.equal(
  resolveServiceView('connections', serviceViews, true),
  'connections',
  'connection settings remain reachable in multimodal mode',
);
const phoneViews = availableServiceViews(false);
assert.equal(
  phoneViews.includes('models'),
  false,
  'unsupported local model management is not exposed',
);
assert.equal(
  phoneViews.includes('omni'),
  true,
  'the multimodal configuration remains reachable on mobile',
);
assert.equal(
  resolveServiceView('models', phoneViews, false),
  'llm',
  'a no-longer-available page falls back to a working editor',
);
assert.equal(
  resolveServiceView('models', phoneViews, true),
  'omni',
  'unsupported local model navigation falls back to the active multimodal page',
);
assert.deepEqual(
  phoneViews,
  ['llm', 'asr', 'omni', 'connections'],
  'mobile service tabs stay stable',
);
console.log('settings service view tests passed');

assert.equal(
  visibleSettingsSections(false, 'harmony').some((item) => item.id === 'inputMethod'),
  false,
  'Harmony wiring does not add an Android input-method navigation entry',
);
assert.deepEqual(
  visibleAdvancedPages('harmony', 'harmony').map((item) => item.id),
  [
    'webSearch',
    'voiceEnhancement',
    'cursorContext',
    'startupDiagnostics',
    'customSyncServer',
    'debug',
  ],
  'Harmony retains shared utility settings without desktop coding-agent controls',
);
for (const os of ['mac', 'win', 'android'] as const) {
  assert.equal(
    visibleAdvancedPages('desktop', os).some((item) =>
      ['lessComputer', 'claudeConsole'].includes(item.id),
    ),
    false,
    `${os}: unfinished features are hidden without a device opt-in`,
  );
  assert.equal(
    visibleAdvancedPages('desktop', os).some((item) => String(item.id) === 'testingFeatures'),
    false,
    `${os}: the retired Beta feature switch is removed`,
  );
}
assert.deepEqual(
  visibleAdvancedPages('desktop', 'mac')
    .filter((item) => ['lessComputer', 'claudeConsole'].includes(item.id))
    .map((item) => item.id),
  [],
  'retired testing features cannot be restored by saved opt-in',
);

assert.equal(
  resolveServiceView('models', serviceViews, true, 'win', true),
  'models',
  'voice activation can prepare models without changing the multimodal pipeline',
);
assert.equal(
  isServiceViewInactive('models', true, 'win', true),
  false,
  'the model button and resolved view agree for voice activation',
);
for (const os of ['mac', 'harmony', 'android'] as const) {
  assert.equal(
    isServiceViewInactive('models', true, os, true),
    true,
    'voice activation does not unlock models on other platforms',
  );
}
assert.equal(isServiceViewInactive('llm', true, 'win', true), true, 'LLM settings remain inactive');
assert.equal(isServiceViewInactive('asr', true, 'win', true), true, 'ASR settings remain inactive');
