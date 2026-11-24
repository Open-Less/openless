import { createDeletionBatch, IncompleteDeletion } from './deletion-batch';

function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function assertEqual(actual: unknown, expected: unknown, message: string) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), message);
}

const calls: string[] = [];
const updates: string[][] = [];
const update = (removed: ReadonlySet<string>) => {
  updates.push([...removed]);
};
let reject = true;
const remove = createDeletionBatch(['first', 'failed', 'third', 'first'], async (id) => {
  calls.push(id);
  if (id === 'failed' && reject) throw new Error('write failed');
});

assertEqual(calls, [], 'Creating a batch must not touch data before confirmation');
let failure: unknown;
try {
  await remove(update);
} catch (error) {
  failure = error;
}
assert(
  failure instanceof IncompleteDeletion && failure.remaining === 1,
  'Partial deletion reports the remaining count',
);
assertEqual(updates, [['first', 'third']], 'Confirmed deletions are reflected before showing an error');
assertEqual(calls, ['first', 'failed', 'third'], 'The first attempt tries each distinct ID once');
reject = false;
await remove(update);
assertEqual(updates, [['first', 'third'], ['failed']], 'The retry reports the newly deleted ID');
assertEqual(calls, ['first', 'failed', 'third', 'failed'], 'Retry only sends unresolved IDs');
