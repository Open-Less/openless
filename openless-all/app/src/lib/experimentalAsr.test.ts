const assert = Object.assign(
  (condition: unknown, message = 'assertion failed') => {
    if (!condition) throw new Error(message);
  },
  {
    equal(actual: unknown, expected: unknown, message = 'values differ') {
      if (actual !== expected)
        throw new Error(`${message}: ${String(actual)} !== ${String(expected)}`);
    },
    deepEqual(actual: unknown, expected: unknown) {
      if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('structures differ');
    },
  },
);
import { listChannels } from './ipc/channels';
import {
  acknowledgeExperimentalAsr,
  downloadPercent,
  deleteExperimentalAsr,
  getExperimentalAsrStatus,
  isExperimentalAsrProvider,
  isRetiredMacAsrProvider,
  needsModelGuidance,
  setExperimentalAsrEnabled,
} from './experimentalAsr';

assert(!isRetiredMacAsrProvider('apple-speech'), 'Apple system speech must remain usable');
assert(!isExperimentalAsrProvider('apple-speech'), 'system speech is not a downloaded experiment');
assert(isRetiredMacAsrProvider('apple-whisper'), 'retired models stay retired');
assert(isExperimentalAsrProvider('experimental-textstream'));
assert(isExperimentalAsrProvider('experimental-sensevoice'));
assert(isExperimentalAsrProvider('experimental-embedding-gemma'));
assert(!isExperimentalAsrProvider('sherpa-onnx-local'), 'legacy models must stay separate');
const before = (await listChannels('asr')).map((channel) => ({ ...channel }));
const primary = before.find((c) => c.enabled)!.id;
const clock = Date.now;
let now = clock();
Date.now = () => now;
try {
  await Promise.all([
    setExperimentalAsrEnabled('text-stream', true),
    setExperimentalAsrEnabled('sense-voice', true),
    setExperimentalAsrEnabled('embedding-gemma', true),
  ]);
  let status = await getExperimentalAsrStatus();
  assert.equal(status[0].model, 'text-stream', 'streaming recommendation is first');
  assert(status.every((s) => s.enabled && s.phase === 'downloading'));
  assert(
    status.every((s) => !needsModelGuidance(s)),
    'not ready until downloaded and registered',
  );
  now += 3000;
  status = await getExperimentalAsrStatus();
  assert(status.every(needsModelGuidance));
  const channels = await listChannels('asr');
  assert.equal(channels.length, before.length + 3);
  assert.equal(channels.find((c) => c.enabled)!.id, primary, 'download does not switch providers');
  assert.equal(downloadPercent({ ...status[0], downloadedBytes: -5 }), 0);
  assert.equal(downloadPercent({ ...status[0], downloadedBytes: status[0].totalBytes + 1e9 }), 100);
  await acknowledgeExperimentalAsr('sense-voice');
  status = await getExperimentalAsrStatus();
  assert(!needsModelGuidance(status.find((s) => s.model === 'sense-voice')!));
  assert(needsModelGuidance(status.find((s) => s.model === 'text-stream')!));
  await setExperimentalAsrEnabled('sense-voice', false);
  status = await getExperimentalAsrStatus();
  const sense = status.find((s) => s.model === 'sense-voice')!;
  assert.equal(sense.phase, 'disabled');
  assert.equal(sense.downloadedBytes, sense.totalBytes, 'disable keeps downloaded files');
  await deleteExperimentalAsr('sense-voice');
  assert.equal(
    (await getExperimentalAsrStatus()).find((s) => s.model === 'sense-voice')!.downloadedBytes,
    0,
  );
  assert.equal(status[0].phase, 'ready', 'switch-off must not delete the TextStream package');
  assert.equal((await listChannels('asr')).length, before.length + 2);
  await setExperimentalAsrEnabled('embedding-gemma', false);
  await setExperimentalAsrEnabled('text-stream', false);
  assert.deepEqual(await listChannels('asr'), before);
} finally {
  Date.now = clock;
}
console.log('experimental ASR package and guidance contracts passed');
