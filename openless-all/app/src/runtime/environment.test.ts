// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import {
  getRuntimeEnvironment,
  resolveRuntimeEnvironment,
  shortcutPlatformFor,
} from './environment';
import { getPreviewProfile } from '../preview/profiles';
import { invokeOrPreview } from './transport';

const macBrowser = { userAgent: 'Macintosh', hostname: 'localhost', protocol: 'http:' };
for (const os of ['mac', 'win', 'android', 'harmony'] as const) {
  const environment = resolveRuntimeEnvironment({ ...macBrowser, search: `?os=${os}` });
  const profile = getPreviewProfile(environment.os);
  assert.equal(environment.host, 'preview');
  assert.equal(environment.os, os);
  assert.deepEqual(shortcutPlatformFor(environment), profile.shortcutPlatform);
  assert.equal(profile.capabilities.supportsImeInput, os === 'win');
  assert.equal(profile.capabilities.supportsLocalQwen3Mlx, os === 'mac');
  assert.equal(
    profile.hotkeyCapability?.adapter,
    os === 'mac' ? 'macEventTap' : os === 'win' ? 'windowsLowLevel' : undefined,
  );
}
assert.equal(resolveRuntimeEnvironment({ ...macBrowser, search: '?os=other' }).os, 'mac');
assert.deepEqual(
  resolveRuntimeEnvironment({ ...macBrowser, search: '?os=win', buildPlatform: 'harmony' }),
  { host: 'harmony', os: 'harmony', isMobile: true },
);
assert.deepEqual(
  resolveRuntimeEnvironment({ ...macBrowser, search: '?os=mac', tauriPlatform: 'androideabi' }),
  { host: 'tauri', os: 'android', isMobile: true },
);

const nativeWindow = {
  location: { protocol: 'https:', hostname: 'tauri.localhost', search: '?os=win' },
  __TAURI_INTERNALS__: undefined as { invoke(command: string): Promise<unknown> } | undefined,
};
Object.defineProperty(globalThis, 'window', { configurable: true, value: nativeWindow });
let previewCalled = false;
const preview = () => {
  previewCalled = true;
  return 'mock';
};
const commands: string[] = [];
try {
  assert.equal(getRuntimeEnvironment().host, 'tauri');
  assert.notEqual(getRuntimeEnvironment().os, 'win');
  const first = invokeOrPreview('first_read', undefined, preview);
  const second = invokeOrPreview('second_read', undefined, preview);
  await Promise.resolve();
  assert.equal(previewCalled, false, 'native origin must protect reads before bridge injection');
  nativeWindow.__TAURI_INTERNALS__ = {
    async invoke(command) {
      commands.push(command);
      if (command === 'get_startup_snapshot') {
        return { backend: { running: true }, contractVersion: '2.0.0' };
      }
      if (command === 'broken_command') throw new Error('native command failed');
      return command;
    },
  };
  assert.deepEqual(await Promise.all([first, second]), ['first_read', 'second_read']);
  assert.equal(commands.filter((command) => command === 'get_startup_snapshot').length, 1);
  await assert.rejects(
    invokeOrPreview('broken_command', undefined, preview),
    /native command failed/,
  );
  assert.equal(previewCalled, false, 'native errors cannot be replaced with mock results');
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}
console.log('Runtime profiles and native bridge readiness passed');
