import {
  searchSettingsSections,
  visibleSettingsSections,
  availableServiceViews,
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
  ['multimodal', 'debug'],
  'Android keeps its experimental switch and diagnostics without desktop agents',
);
console.log('settings navigation tests passed');

const omniViews = availableServiceViews(true, true);
assert.deepEqual(
  omniViews,
  ['omni', 'models', 'connections'],
  'multimodal exposes its real service configuration instead of empty LLM/ASR pages',
);
assert.equal(
  resolveServiceView('llm', omniViews),
  'omni',
  'switching to Omni mode keeps the service editor reachable',
);
assert.equal(
  resolveServiceView('models', omniViews),
  'models',
  'pipeline changes do not redirect a user managing local models',
);
const phoneViews = availableServiceViews(false, false);
assert.equal(
  phoneViews.includes('models'),
  false,
  'unsupported local model management is not exposed',
);
assert.equal(
  resolveServiceView('models', phoneViews),
  'llm',
  'a no-longer-available page falls back to a working editor',
);
assert.equal(
  resolveServiceView('omni', phoneViews),
  'llm',
  'leaving Omni returns to a traditional service',
);
