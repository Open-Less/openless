import { reduceMarketplaceLikes } from './marketplaceLikes';

const assert = {
  deepEqual(actual: unknown, expected: unknown, message: string) {
    const left = JSON.stringify(actual);
    const right = JSON.stringify(expected);
    if (left !== right) throw new Error(`${message}\n${left}\n${right}`);
  },
  equal(actual: unknown, expected: unknown, message: string) {
    if (actual !== expected)
      throw new Error(`${message}: ${String(actual)} !== ${String(expected)}`);
  },
};

const liked = new Set(['pack-a', 'pack-b']);

assert.deepEqual(
  [...reduceMarketplaceLikes(liked, { type: 'status-failed' })],
  ['pack-a', 'pack-b'],
  'a failed auth read keeps the liked set',
);
assert.deepEqual(
  [...reduceMarketplaceLikes(liked, { type: 'fetch-failed' })],
  ['pack-a', 'pack-b'],
  'a failed likes read keeps the liked set',
);
assert.deepEqual(
  [...reduceMarketplaceLikes(liked, { type: 'fetched', ids: ['pack-c'] })],
  ['pack-c'],
  'a completed likes read replaces the set',
);
assert.equal(
  reduceMarketplaceLikes(liked, { type: 'signed-out' }).size,
  0,
  'an explicit sign-out clears likes',
);

console.log('marketplace likes tests passed');
