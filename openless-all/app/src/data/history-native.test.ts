// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { getActivityApps } from '../api/history';

let failure: string | undefined;
const calls: Array<[string, unknown]> = [];
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    location: { protocol: 'https:', hostname: 'tauri.localhost' },
    __TAURI_INTERNALS__: {
      async invoke(command: string, args?: Record<string, unknown>) {
        calls.push([command, args]);
        if (command === 'get_startup_snapshot')
          return { backend: { running: true }, contractVersion: '2.0.0' };
        if (command === 'get_activity_apps') {
          if (failure) throw failure;
          return [['Editor', 43]];
        }
        throw new Error(`Unexpected command ${command}`);
      },
    },
  },
});

assert.deepEqual(await getActivityApps(2026), { status: 'available', rows: [['Editor', 43]] });
assert.deepEqual(calls.at(-1), ['get_activity_apps', { year: 2026 }]);
failure = 'Command get_activity_apps not found';
assert.deepEqual(await getActivityApps(2026), { status: 'unsupported' });
failure = 'failed to open yearly activity file';
await assert.rejects(getActivityApps(2026), (error) => error === failure);
