import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const ts = require('typescript');
const source = ts.createSourceFile(
  'DeepSeekReadAloud.tsx',
  readFileSync(new URL('../src/components/DeepSeekReadAloud.tsx', import.meta.url), 'utf8'),
  ts.ScriptTarget.ES2020,
  true,
  ts.ScriptKind.TSX,
);
const body = source.statements
  .filter((n) => !ts.isImportDeclaration(n))
  .map((n) => n.getFullText(source))
  .join('\n');
const compiled = ts.transpileModule(body, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const factory = new Function(
  'useEffect',
  'useRef',
  'useState',
  'useTranslation',
  'Channel',
  'invoke',
  'isTauri',
  'AudioContext',
  'crypto',
  'setTimeout',
  'clearTimeout',
  'exports',
  'require',
  `${compiled}; return {DeepSeekReadAloud, stopDeepSeekReading};`,
);
function deferred() {
  let resolve;
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));
function harness({ failAudio = false } = {}) {
  const slots = [],
    effects = [],
    contexts = [],
    starts = [],
    stops = [],
    timers = new Map();
  let cursor = 0,
    serial = 0,
    nextTimer = 0;
  const state = (initial) => {
    const index = cursor++;
    slots[index] ??= { value: initial };
    return [
      slots[index].value,
      (value) => {
        slots[index].value = value;
      },
    ];
  };
  const effect = (fn, deps) => {
    const index = cursor++,
      old = slots[index];
    if (old && deps.every((d, i) => Object.is(d, old.deps[i]))) return;
    slots[index] = { deps };
    effects.push(() => {
      old?.cleanup?.();
      slots[index].cleanup = fn();
    });
  };
  class Audio {
    activation = deferred();
    currentTime = 0;
    destination = {};
    closed = false;
    constructor() {
      if (failAudio) throw new Error('audio unavailable');
      contexts.push(this);
    }
    resume() {
      return this.activation.promise;
    }
    close() {
      this.closed = true;
      return Promise.resolve();
    }
  }
  const api = factory(
    effect,
    (value) => state({ current: value })[0],
    state,
    () => ({ i18n: { language: 'en' } }),
    class {},
    (name, args) => {
      if (name === 'deepseek_tts_stop') {
        stops.push(args.requestId);
        return Promise.resolve();
      }
      const completion = deferred();
      starts.push({ ...args, completion });
      return completion.promise;
    },
    true,
    Audio,
    { randomUUID: () => `request-${++serial}` },
    (callback) => {
      const id = ++nextTimer;
      timers.set(id, callback);
      return id;
    },
    (id) => timers.delete(id),
    {},
    () => ({ jsx: (type, props) => ({ type, props }), jsxs: (type, props) => ({ type, props }) }),
  );
  const render = (text = 'hello') => {
    cursor = 0;
    const tree = api.DeepSeekReadAloud({ text });
    effects.splice(0).forEach((fn) => fn());
    return tree;
  };
  const button = (text) => render(text).props.children.find((n) => n?.type === 'button');
  const unmount = () => {
    slots.forEach((s) => s.cleanup?.());
  };
  return {
    api,
    render,
    click: (text) => button(text).props.onClick(),
    unmount,
    contexts,
    starts,
    stops,
    timers,
  };
}

{
  const h = harness();
  const pending = h.click();
  await h.click();
  h.contexts[0].activation.resolve();
  await tick();
  assert.equal(h.starts.length, 0, 'stopped device activation must not start a billed TTS call');
  assert(h.contexts[0].closed);
  await pending;
  h.unmount();
}
{
  const h = harness();
  const old = h.click();
  await h.click();
  const next = h.click();
  h.contexts[1].activation.resolve();
  await tick();
  assert.deepEqual(
    h.starts.map((s) => s.requestId),
    ['request-2'],
  );
  h.contexts[0].activation.resolve();
  await old;
  assert.deepEqual(
    h.starts.map((s) => s.requestId),
    ['request-2'],
    'late old activation cannot replace newer speech',
  );
  assert(!h.contexts[1].closed);
  h.starts[0].completion.resolve();
  await next;
  await h.click();
  assert.equal(h.timers.size, 0, 'Stop releases the pending playback completion timer');
  assert(h.stops.includes('request-2'));
  h.unmount();
}
for (const operation of ['unmount', 'replace']) {
  const h = harness();
  const pending = h.click();
  if (operation === 'unmount') h.unmount();
  else h.render('replacement answer');
  h.contexts[0].activation.resolve();
  await pending;
  assert.equal(h.starts.length, 0, `${operation} cancels pending audio activation`);
  h.unmount();
}
{
  const h = harness({ failAudio: true });
  await h.click();
  assert(h.render().props.children.some((n) => n?.props?.role === 'alert'));
  assert.equal(h.starts.length, 0, 'audio initialization failure must not send a request');
  h.unmount();
}
console.log(
  'DeepSeek read aloud: Stop, replacement, disposal, timer cleanup and device failure passed',
);
