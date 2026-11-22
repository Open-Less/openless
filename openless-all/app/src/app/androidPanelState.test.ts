// @ts-nocheck — Node runtime harness; application builds do not include Node types.

import assert from 'node:assert/strict';
import { createAndroidPanelState, type AndroidPanelView } from './androidPanelState';

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const views: AndroidPanelView[] = [];
const cancellations: ReturnType<typeof deferred>[] = [];
const route = createAndroidPanelState(
  async () => {
    const result = deferred();
    cancellations.push(result);
    await result.promise;
  },
  (view) => views.push(view),
);

await route.switchTo('voice-edit');
const voice = route.current();
const first = route.switchTo('qa');
assert.equal(route.switchTo('qa'), first, 'duplicate opens share the same cancellation');
await Promise.resolve();
route.nativeDismiss('voice-edit');
assert.equal(route.current().kind, 'voice-edit', 'native hide can precede cleanup completion');
cancellations[0].reject(new Error('native recording still owns the microphone'));
assert.equal(await first, false);
assert.equal(route.current().kind, 'voice-edit', 'failure retains the visible panel and retry');
assert.match(route.current().error ?? '', /microphone/);

const stale = route.switchTo('qa');
await Promise.resolve();
await route.switchTo('voice-edit');
const newest = route.current();
cancellations[1].resolve();
assert.equal(await stale, false, 'a late successful reply cannot overwrite a newer open');
assert.equal(route.current(), newest);
assert.equal(await route.close(voice), false, 'an old view cannot dismiss a new generation');

const latest = route.switchTo('qa');
await Promise.resolve();
cancellations[2].resolve();
assert.equal(await latest, true);
assert.equal(route.current().kind, 'qa');
const back = route.close(route.current());
await Promise.resolve();
route.dispose();
cancellations[3].resolve();
assert.equal(await back, false, 'pre-rebuild IPC cannot publish after disposal');
assert.equal(views[views.length - 1]?.kind, 'qa');

let cleanupCount = 0;
const completion = deferred();
const handoff = createAndroidPanelState(
  async () => {
    cleanupCount++;
    await completion.promise;
  },
  () => {},
);
await handoff.switchTo('voice-edit');
const dismissed = handoff.close(handoff.current());
const switching = handoff.switchTo('qa');
await Promise.resolve();
assert.equal(cleanupCount, 1, 'close and switch from the same panel share native cleanup');
completion.resolve();
assert.equal(await dismissed, false, 'the newer destination owns the completed cleanup');
assert.equal(await switching, true);
assert.equal(handoff.current().kind, 'qa');
handoff.dispose();

console.log('Android panel transitions retain errors and ignore stale results');
