import assert from 'node:assert/strict';
import { createDeletionBatch, IncompleteDeletion } from './deletion-batch';

const calls: string[] = [];
const updates: string[][] = [];
const update = (removed: ReadonlySet<string>) => { updates.push([...removed]); };
let reject = true;
const remove = createDeletionBatch(['first', 'failed', 'third', 'first'], async (id) => {
  calls.push(id);
  if (id === 'failed' && reject) throw new Error('write failed');
});

assert.deepEqual(calls, [], 'Creating a batch must not touch data before confirmation');
await assert.rejects(remove(update), (error) => error instanceof IncompleteDeletion && error.remaining === 1);
assert.deepEqual(updates, [['first', 'third']], 'Confirmed deletions are reflected before showing an error');
assert.deepEqual(calls, ['first', 'failed', 'third']);
reject = false;
await remove(update);
assert.deepEqual(updates, [['first', 'third'], ['failed']]);
assert.deepEqual(calls, ['first', 'failed', 'third', 'failed'], 'Retry only sends unresolved IDs');
