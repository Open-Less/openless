import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const ts = require('typescript');
const source = readFileSync(
  new URL('../src/components/CloudSyncSetupPrompt.tsx', import.meta.url),
  'utf8',
);
const code = ts.transpileModule(source, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const tick = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
};
const jsx = (type, props) => ({ type, props: props ?? {} });
const all = (tree) =>
  Array.isArray(tree) ? tree.flatMap(all) : tree?.props ? [tree, ...all(tree.props.children)] : [];
function context({ native = true, blocked = false } = {}) {
  const slots = [],
    effects = [],
    listeners = new Map(),
    timers = new Map();
  let cursor = 0,
    id = 0,
    calls = 0,
    response = false,
    props = { blocked, onSetup: () => {} },
    last;
  const document = Object.assign(new EventTarget(), {
    visibilityState: 'visible',
    hasFocus: () => true,
    activeElement: null,
  });
  const window = new EventTarget();
  const react = {
    useState(initial) {
      const i = cursor++;
      slots[i] ??= { value: initial };
      return [
        slots[i].value,
        (v) => {
          slots[i].value = typeof v === 'function' ? v(slots[i].value) : v;
        },
      ];
    },
    useRef(initial) {
      return this.useState({ current: initial })[0];
    },
    useEffect(fn, deps) {
      const i = cursor++,
        previous = slots[i];
      if (previous && deps.every((v, j) => Object.is(v, previous.deps[j]))) return;
      slots[i] = { deps };
      effects.push(() => {
        previous?.cleanup?.();
        slots[i].cleanup = fn();
      });
    },
  };
  react.useRef = (initial) => react.useState({ current: initial })[0];
  const imports = {
    react,
    'react-i18next': { useTranslation: () => ({ t: (k) => k }) },
    'react/jsx-runtime': { jsx, jsxs: jsx, Fragment: 'fragment' },
    'lucide-react': { CloudIcon: 'cloud', XIcon: 'x' },
    '../lib/ipc/shared': { isTauri: native },
    '../lib/ipc/cloud-sync-e2ee': {
      cloudSyncE2eeClaimSetupPrompt: async () => {
        calls++;
        return typeof response === 'function' ? response() : response;
      },
    },
    '../pages/settings/CloudSyncSection': { CloudSyncSection: 'sync-section' },
    './ui/Modal': { Modal: 'modal' },
    '@tauri-apps/api/event': {
      listen: async (name, fn) => {
        listeners.set(name, fn);
        return () => listeners.delete(name);
      },
    },
  };
  const exports = {};
  new Function(
    'require',
    'exports',
    'window',
    'document',
    'HTMLElement',
    'setTimeout',
    'clearTimeout',
    code,
  )(
    (name) => imports[name],
    exports,
    window,
    document,
    class {},
    (fn) => {
      timers.set(++id, fn);
      return id;
    },
    (i) => timers.delete(i),
  );
  const render = () => {
    cursor = 0;
    last = exports.CloudSyncSetupPrompt(props);
    effects.splice(0).forEach((fn) => fn());
    return last;
  };
  return {
    render,
    document,
    window,
    setResponse: (v) => {
      response = v;
    },
    setBlocked: (v) => {
      props = { ...props, blocked: v };
    },
    calls: () => calls,
    listeners,
    timers,
    event: (type) => listeners.get('backend:event')?.({ payload: { kind: { type } } }),
    async flush() {
      await tick();
      const queued = [...timers.values()];
      timers.clear();
      queued.forEach((fn) => fn());
      await tick();
      return render();
    },
    unmount() {
      slots.forEach((s) => s?.cleanup?.());
    },
    welcome: exports.CloudSyncWelcome,
  };
}
for (const options of [{ native: false }, { blocked: true }]) {
  const c = context(options);
  c.setResponse(true);
  assert.equal(c.render(), null);
  await c.flush();
  assert.equal(c.calls(), 0);
  assert.equal(c.listeners.size, 0);
  c.unmount();
}
{
  const c = context();
  c.setResponse(true);
  c.render();
  const tree = await c.flush();
  assert.equal(tree.type, 'modal');
  assert.equal(c.calls(), 1);
  const later = all(tree).find(
    (n) => n.type === 'button' && n.props.children === 'cloudSyncE2ee.setupPromptLater',
  );
  assert(later);
  later.props.onClick();
  assert.equal(c.render(), null);
  c.unmount();
  assert.equal(c.listeners.size, 0);
}
{
  const c = context();
  c.render();
  await c.flush();
  assert.equal(c.calls(), 1);
  assert.equal(c.render(), null);
  c.event('qa_level');
  assert.equal(c.timers.size, 0);
  c.setResponse(true);
  c.event('credentials_changed');
  assert.equal((await c.flush()).type, 'modal');
  c.unmount();
}
{
  const c = context();
  c.setResponse(() => Promise.reject(new Error('vault denied')));
  c.render();
  await c.flush();
  assert.equal(c.render(), null);
  assert.equal(c.listeners.size, 1);
  c.setResponse(true);
  c.window.dispatchEvent(new Event('focus'));
  assert.equal((await c.flush()).type, 'modal');
  c.unmount();
}
{
  const c = context();
  const pending = deferred();
  c.setResponse(() => pending.promise);
  c.render();
  await c.flush();
  c.setBlocked(true);
  c.render();
  pending.resolve(true);
  await tick();
  assert.equal(c.render(), null);
  assert.equal(c.listeners.size, 0);
  c.unmount();
}
{
  const c = context();
  c.document.visibilityState = 'hidden';
  c.setResponse(true);
  c.render();
  await c.flush();
  assert.equal(c.calls(), 0);
  c.document.visibilityState = 'visible';
  c.document.dispatchEvent(new Event('visibilitychange'));
  assert.equal((await c.flush()).type, 'modal');
  c.unmount();
}
for (const change of ['hidden', 'focus', 'recording']) {
  const c = context();
  const pending = deferred();
  c.setResponse(() => pending.promise);
  c.render();
  await c.flush();
  if (change === 'hidden') {
    c.document.visibilityState = 'hidden';
    c.document.dispatchEvent(new Event('visibilitychange'));
  }
  if (change === 'focus') c.document.hasFocus = () => false;
  if (change === 'recording') c.event('dictation_state_changed');
  pending.resolve(true);
  await tick();
  assert.equal(c.render(), null, `late claim after ${change} must not open`);
  c.unmount();
}
console.log('encrypted sync setup prompt: 10 lifecycle cases passed');
