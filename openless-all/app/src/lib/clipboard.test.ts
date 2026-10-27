// @ts-nocheck — Node-only clipboard harness; production clipboard code remains strictly typed.
import assert from 'node:assert/strict';
import { copyTextToClipboard } from './clipboard';

let asyncValue = '';
let asyncShouldFail = false;
let fallbackValue = '';
let fallbackCalls = 0;
const textarea = {
  value: '',
  style: {} as Record<string, string>,
  setAttribute: () => undefined,
  select: () => {
    fallbackValue = textarea.value;
  },
};

Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: {
    clipboard: {
      writeText: async (text: string) => {
        if (asyncShouldFail) throw new Error('clipboard unavailable');
        asyncValue = text;
      },
    },
  },
});
Object.defineProperty(globalThis, 'document', {
  configurable: true,
  value: {
    createElement: () => textarea,
    body: {
      appendChild: () => undefined,
      removeChild: () => undefined,
    },
    execCommand: () => {
      fallbackCalls += 1;
      return true;
    },
  },
});

try {
  assert.equal(await copyTextToClipboard('async copy'), true);
  assert.equal(asyncValue, 'async copy');

  asyncShouldFail = true;
  assert.equal(await copyTextToClipboard('fallback copy'), true);
  assert.equal(fallbackValue, 'fallback copy');
  assert.equal(fallbackCalls, 1);

  (globalThis.document as { execCommand: () => boolean }).execCommand = () => false;
  assert.equal(await copyTextToClipboard('failed copy'), false);
} finally {
  Reflect.deleteProperty(globalThis, 'navigator');
  Reflect.deleteProperty(globalThis, 'document');
}

console.log('clipboard helper contract passed');
