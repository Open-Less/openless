import {
  guideSteps,
  needsDesktopGuide,
  newGuideProgress,
  parseGuideProgress,
  permissionGate,
} from './desktopOnboarding';
import type { CredentialsStatus } from './types';

const assert = {
  equal(actual: unknown, expected: unknown, message = 'Unexpected value') {
    if (actual !== expected) throw new Error(`${message}: ${actual} !== ${expected}`);
  },
  deepEqual(actual: unknown, expected: unknown, message = 'Unexpected structure') {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(message);
  },
};
const credentials: CredentialsStatus = {
  activeAsrProvider: 'asr',
  activeLlmProvider: 'llm',
  pipelineMode: 'traditional',
  asrConfigured: true,
  llmConfigured: true,
  omniConfigured: false,
  volcengineConfigured: true,
  arkConfigured: true,
};
assert.equal(
  needsDesktopGuide(null, credentials),
  false,
  'configured installations retain normal startup',
);
assert.equal(needsDesktopGuide(null, { ...credentials, asrConfigured: false }), true);
assert.equal(
  needsDesktopGuide(newGuideProgress(), credentials),
  true,
  'unfinished setup survives newly configured services',
);
assert.equal(
  needsDesktopGuide(
    { ...newGuideProgress(), finished: true },
    { ...credentials, asrConfigured: false },
  ),
  false,
  'explicit deferral is not a startup permission gate',
);
assert.equal(
  needsDesktopGuide(newGuideProgress(), { ...credentials, asrConfigured: false }, true),
  false,
  'native completion marker survives a recreated main webview',
);
assert.equal(
  needsDesktopGuide(null, {
    ...credentials,
    pipelineMode: 'multimodal',
    omniConfigured: true,
    asrConfigured: false,
    llmConfigured: false,
  }),
  false,
);
assert.equal(needsDesktopGuide(null, { ...credentials, pipelineMode: 'multimodal' }), true);
assert.deepEqual(
  permissionGate({
    os: 'mac',
    persistedFinished: true,
    accessibility: 'granted',
    microphone: 'notDetermined',
  }),
  { gate: 'ready' },
  'reopening a finished install does not prompt again for an undetermined microphone',
);
assert.deepEqual(
  permissionGate({
    os: 'mac',
    persistedFinished: false,
    accessibility: 'granted',
    microphone: 'notDetermined',
  }),
  { gate: 'onboarding', step: 'microphone' },
  'first launch still asks when the microphone has not been decided',
);
assert.deepEqual(
  permissionGate({
    os: 'mac',
    persistedFinished: true,
    accessibility: 'granted',
    microphone: 'denied',
  }),
  { gate: 'onboarding', step: 'microphone' },
  'a real microphone denial still returns to onboarding',
);
assert.deepEqual(
  permissionGate({
    os: 'mac',
    persistedFinished: true,
    accessibility: 'denied',
    microphone: 'granted',
  }),
  { gate: 'onboarding', step: 'accessibility' },
  'a real accessibility denial still returns to onboarding',
);
for (const invalid of [
  null,
  '',
  '{',
  'null',
  '[]',
  '{"version":2,"step":"welcome"}',
  '{"version":1,"step":"secret"}',
]) {
  assert.equal(parseGuideProgress(invalid), null);
}
const progress = parseGuideProgress(
  JSON.stringify({
    ...newGuideProgress(),
    name: 'a'.repeat(80),
    step: 'recognition',
    answers: { microphone: 'later', accessibility: 'done', mode: 'invalid', apiKey: 'secret' },
    finished: 'true',
    apiKey: 'secret',
  }),
);
assert.equal(progress?.name.length, 40);
assert.equal(progress?.finished, false);
assert.deepEqual(progress?.answers, { microphone: 'later', accessibility: 'done' });
assert.equal(
  JSON.stringify(progress).includes('secret'),
  false,
  'unknown fields never become saved progress',
);
assert.deepEqual(guideSteps(true, false), [
  'welcome',
  'microphone',
  'accessibility',
  'mode',
  'recognition',
  'polish',
  'shortcut',
  'finish',
]);
assert.equal(guideSteps(false, false).includes('accessibility'), false);
assert.equal(guideSteps(true, true).includes('polish'), false);
console.log('Desktop onboarding progress and eligibility checks passed');
