import { bailianProtocols, readBailianProtocol, writeBailianProtocol } from './bailianProtocol';

const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected)
      throw new Error(`Expected ${String(expected)}, received ${String(actual)}`);
  },
  throws(action: () => unknown) {
    try {
      action();
    } catch {
      return;
    }
    throw new Error('Expected invalid configuration to be rejected');
  },
};

assert.equal(readBailianProtocol(null), 'auto');
for (const protocol of bailianProtocols) {
  const raw = writeBailianProtocol('{"enableItn":false,"chunkDurationMs":1234}', protocol);
  assert.equal(readBailianProtocol(raw), protocol);
  assert.equal(JSON.parse(raw).enableItn, false);
  assert.equal(JSON.parse(raw).chunkDurationMs, 1234);
}
const reset = writeBailianProtocol(
  '{"bailianProtocol":"qwen-realtime","verboseJson":true}',
  'auto',
);
assert.equal('bailianProtocol' in JSON.parse(reset), false);
assert.equal(JSON.parse(reset).verboseJson, true);
assert.throws(() => readBailianProtocol('{"bailianProtocol":"unknown"}'));
assert.throws(() => writeBailianProtocol('broken JSON', 'multimodal'));
console.log('bailianProtocol tests passed');
