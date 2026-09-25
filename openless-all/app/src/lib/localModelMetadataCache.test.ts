const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`Expected ${String(expected)}, got ${String(actual)}`);
  },
  async rejects(promise: Promise<unknown>) {
    try {
      await promise;
    } catch {
      return;
    }
    throw new Error('Expected rejection');
  },
};
import { LocalModelMetadataCache } from './localModelMetadataCache';

const cache = new LocalModelMetadataCache<number>();
let calls = 0;
let finish!: (value: number) => void;
const request = () => {
  calls++;
  return new Promise<number>((resolve) => {
    finish = resolve;
  });
};
const first = cache.load('qwen:huggingface', request);
const duplicate = cache.load('qwen:huggingface', request);
assert.equal(first, duplicate);
await Promise.resolve();
assert.equal(calls, 1);
finish(42);
assert.equal(await first, 42);
assert.equal(await cache.load('qwen:huggingface', request), 42);
assert.equal(calls, 1);
assert.equal(await cache.load('qwen:modelscope', async () => 43), 43);
await assert.rejects(
  cache.load('broken', async () => {
    throw Error('offline');
  }),
);
assert.equal(await cache.load('broken', async () => 44), 44);
cache.clear();
assert.equal(await cache.load('qwen:huggingface', async () => 45), 45);
console.log('model metadata requests are coalesced, mirror-scoped, and retryable');
