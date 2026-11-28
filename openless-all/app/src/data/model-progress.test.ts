// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { shareProgressEvent } from './model-progress';

let connections = 0;
let stops = 0;
let dispatch: (value: string) => void = () => {};
const subscribe = shareProgressEvent<string>(async (handler) => {
  connections++;
  dispatch = handler;
  return () => {
    stops++;
  };
});

const shell: string[] = [];
const models: string[] = [];
const [stopShell, stopModels] = await Promise.all([
  subscribe((job) => shell.push(job)),
  subscribe((job) => models.push(job)),
]);
assert.equal(connections, 1);
dispatch('started');
assert.deepEqual(shell, ['started']);
assert.deepEqual(models, ['started']);
stopModels();
dispatch('finished');
assert.deepEqual(shell, ['started', 'finished']);
assert.deepEqual(models, ['started']);
assert.equal(stops, 0);
stopShell();
assert.equal(stops, 1);
const resumed = await subscribe(() => {});
assert.equal(connections, 2);
resumed();

let attempt = 0;
const retry = shareProgressEvent(async () => {
  if (++attempt === 1) throw new Error('listener unavailable');
  return () => {};
});
await assert.rejects(
  retry(() => {}),
  /unavailable/,
);
(await retry(() => {}))();
assert.equal(attempt, 2);
