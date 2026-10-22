export {};

const assert = {
  equal(actual: unknown, expected: unknown, message = 'values must match') {
    if (actual !== expected) throw new Error(`${message}: ${actual} != ${expected}`);
  },
  deepEqual(actual: unknown, expected: unknown, message = 'values must match') {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      throw new Error(`${message}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
    }
  },
};

type Appearance = 'light' | 'dark' | null;
const calls: Appearance[] = [];
let appearance: Appearance = null;
let gate: Promise<void> | undefined;
let started: (() => void) | undefined;
const metadata = { currentWindow: { label: 'main' } };

Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'Macintosh', platform: 'MacIntel' },
});
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      metadata,
      invoke: async (command: string, args: { theme: Appearance }) => {
        assert.equal(command, 'plugin:app|set_app_theme');
        calls.push(args.theme);
        started?.();
        await gate;
        appearance = args.theme;
      },
    },
  },
});

const { syncMacOSWindowTheme } = await import('./nativeWindowTheme');

assert.equal(await syncMacOSWindowTheme('light'), true);
assert.equal(appearance, 'light');
assert.equal(await syncMacOSWindowTheme('dark'), true);
assert.equal(appearance, 'dark');
assert.equal(await syncMacOSWindowTheme('system'), true);
assert.equal(appearance, null, 'system must clear the native override, not pin a resolved color');

metadata.currentWindow.label = 'capsule';
assert.equal(await syncMacOSWindowTheme('light'), false);
assert.equal(appearance, null, 'a helper window must not replace application appearance');
assert.deepEqual(calls, ['light', 'dark', null]);
metadata.currentWindow.label = 'main';

let release!: () => void;
gate = new Promise<void>((resolve) => {
  release = resolve;
});
const entered = new Promise<void>((resolve) => {
  started = resolve;
});
const first = syncMacOSWindowTheme('light');
await entered;
const superseded = syncMacOSWindowTheme('dark');
const last = syncMacOSWindowTheme('system');
release();
assert.deepEqual(await Promise.all([first, superseded, last]), [false, false, true]);
assert.equal(appearance, null, 'a slow old write cannot override the newest preference');
assert.deepEqual(calls.slice(3), ['light', null], 'skip obsolete queued requests');

Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'Windows', platform: 'Win32' },
});
assert.equal(await syncMacOSWindowTheme('dark'), false);
assert.equal(appearance, null, 'Windows retains its existing native caption path');

Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'iPhone', platform: 'iPhone' },
});
assert.equal(await syncMacOSWindowTheme('dark'), false);
assert.equal(appearance, null, 'mobile WebViews must not invoke the desktop appearance API');

console.log(
  'macOS theme: preference mapping, system reset, main-window ownership and ordering passed',
);
