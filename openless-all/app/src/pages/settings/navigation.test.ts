import {
  searchSettingsSections,
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
  visibleAdvancedPages('desktop', 'win').some((item) => item.id === 'lessComputer'),
  true,
  'Windows retains Less Computer configuration',
);
assert.equal(
  visibleAdvancedPages('desktop', 'win').some((item) => item.id === 'claudeConsole'),
  false,
  'the macOS Claude console is not exposed on Windows',
);
assert.deepEqual(
  visibleAdvancedPages('android', 'android').map((item) => item.id),
  ['vocabularyLearning', 'debug'],
  'the pipeline switch lives in Services on every platform',
);
console.log('settings navigation tests passed');

assert.equal(
  visibleAdvancedPages('mobile', 'mac').some((item) => item.id === 'vocabularyLearning'),
  false,
  'generic mobile hosts without a native observer do not expose learning configuration',
);
assert.equal(
  visibleAdvancedPages('desktop', 'mac').some((item) => item.id === 'vocabularyLearning'),
  true,
  'macOS retains its native correction observer settings',
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
