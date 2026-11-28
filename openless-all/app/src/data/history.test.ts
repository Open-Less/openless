// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { setImmediate } from 'node:timers/promises';
import type { DictationSession } from '../api/contracts';
import { createHistoryStore } from './history';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const value = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { value, resolve, reject };
}

const makeEntry = (id: string) => ({ id }) as DictationSession;
const deliveries = deferred<() => void>();
const readCalls: ReturnType<typeof deferred<DictationSession[]>>[] = [];
const appQueries: number[] = [];
let notify: (revision?: number) => void = () => {};
let activityQueries = 0;
let subscriptions = 0;
const store = createHistoryStore({
  listen(onChange) {
    subscriptions++;
    notify = onChange;
    return deliveries.value;
  },
  listHistory() {
    const next = deferred<DictationSession[]>();
    readCalls.push(next);
    return next.value;
  },
  async getActivityStats() {
    activityQueries++;
    return [];
  },
  async getActivityApps(year) {
    appQueries.push(year);
    return { status: 'available', rows: [[String(year), appQueries.length]] };
  },
});

const published: string[][] = [];
const unsubscribe = store.records.subscribe(() => {
  const snapshot = store.records.getSnapshot();
  if (!snapshot.loading && snapshot.data) published.push(snapshot.data.map((entry) => entry.id));
});
const first = store.records.load();
const initialActivity = store.refreshActivity();
const years = Promise.all([store.refreshApps(2025), store.refreshApps(2026)]);
await setImmediate();
assert.equal(subscriptions, 1);
assert.equal(readCalls.length, 0, 'register the listener before any query starts');
assert.equal(activityQueries, 0);
deliveries.resolve(() => {});
await Promise.all([initialActivity, years]);
assert.equal(readCalls.length, 1);
assert.deepEqual(appQueries.sort(), [2025, 2026]);

notify(11);
notify(12);
notify(11);
await setImmediate();
assert.equal(readCalls.length, 1, 'one read per key may be in flight');
readCalls[0].resolve([makeEntry('stale')]);
await setImmediate();
assert.equal(readCalls.length, 2);
assert.deepEqual(published, [], 'the invalidated response must not publish');
assert.equal(activityQueries, 2);
assert.equal(appQueries.length, 4);
readCalls[1].resolve([makeEntry('latest')]);
assert.deepEqual(
  (await first).data?.map((record) => record.id),
  ['latest'],
);
notify(12);
await setImmediate();
assert.equal(readCalls.length, 2, 'duplicate revision does not read again');

notify(10);
await setImmediate();
assert.equal(readCalls.length, 3, 'an unseen revision can arrive after a newer one');
notify(13);
readCalls[2].reject(new Error('old request failed'));
await setImmediate();
assert.equal(readCalls.length, 4);
assert.equal(
  store.records.getSnapshot().error,
  null,
  'stale rejection must not replace current state',
);
readCalls[3].resolve([makeEntry('before write')]);
await store.records.load();

const committed = deferred<void>();
const mutation = store.mutate(() => {
  notify(14);
  return committed.value;
});
await setImmediate();
assert.equal(readCalls.length, 4, 'an event cannot start a second read before its write settles');
committed.resolve();
await mutation;
await setImmediate();
assert.equal(readCalls.length, 5, 'the signal and successful reply cause one post-commit read');
readCalls[4].resolve([makeEntry('after write')]);
await store.records.load();
assert.deepEqual(
  store.records.getSnapshot().data?.map((entry) => entry.id),
  ['after write'],
);

await assert.rejects(
  store.mutate(async () => {
    throw new Error('write failed');
  }),
  /write failed/,
);
await setImmediate();
assert.equal(
  readCalls.length,
  6,
  'reconcile a possibly committed operation after a transport error',
);
readCalls[5].resolve([makeEntry('after write')]);
await store.records.load();
unsubscribe();
const unsubscribeAgain = store.records.subscribe(() => {});
await setImmediate();
assert.equal(subscriptions, 1);
assert.equal(readCalls.length, 6, 'navigating away and back retains the shared live snapshot');
unsubscribeAgain();
console.log('History subscribe-before-read, revision dedupe, and write reconciliation passed');
