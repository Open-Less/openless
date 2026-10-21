import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf8');
const ast = ts.createSourceFile(
  'Capsule.tsx',
  source,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX,
);
let effect;
function visit(node) {
  if (
    ts.isCallExpression(node) &&
    node.expression.getText(ast) === 'useEffect' &&
    node.arguments[0]?.getText(ast).includes('getCapsuleSnapshot')
  ) {
    effect = node.arguments[0].getText(ast);
  }
  ts.forEachChild(node, visit);
}
visit(ast);
assert.ok(effect, 'test must execute the actual capsule replay effect');

const compile = (text) =>
  ts.transpileModule(text, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  }).outputText;
const backend = { exports: {} };
vm.runInNewContext(
  compile(readFileSync(new URL('../src/lib/backendEvent.ts', import.meta.url), 'utf8')),
  backend,
);
const effectCode = compile(`globalThis.replayEffect = (${effect});`);
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const frame = (overrides = {}) => ({
  state: 'recording',
  sessionId: 'session',
  transcript: 'existing text',
  sequence: 10,
  revision: 10,
  payloadRevision: 10,
  ...overrides,
});

// Run the component's real effect with controlled native events and delayed IPC replies.
function mount({
  snapshot = () => null,
  beforeListen = async () => {},
  auxiliary = () => {
    throw new Error('unsupported');
  },
} = {}) {
  const listeners = new Map();
  const stopped = [];
  const delays = [];
  const writes = [];
  const warnings = [];
  const values = { setState: 'idle', setLocalAsrText: '' };
  let requests = 0;
  const scope = {
    ...backend.exports,
    isTauri: true,
    capsuleStateFirstLogged: true,
    capsuleStateRef: { current: 'idle' },
    transcriptViewRef: { current: backend.exports.createTranscriptViewState() },
    stylePreferenceReadyRef: { current: true },
    insertedCharsRef: { current: 0 },
    operatingRef: { current: false },
    parseCapsuleStyle: () => undefined,
    console: { info() {}, warn: (...args) => warnings.push(args) },
    window: {
      setTimeout: (done, delay) => {
        delays.push(delay);
        return setImmediate(done);
      },
    },
    getCapsuleSnapshot: async () => {
      assert.equal(listeners.size, 4, 'all event subscriptions must precede snapshot reads');
      requests += 1;
      return snapshot(requests);
    },
    require: (name) => {
      if (name === '@tauri-apps/api/core')
        return {
          invoke: async (command) => {
            assert.equal(command, 'get_capsule_auxiliary_snapshot');
            assert.equal(listeners.size, 4);
            return auxiliary();
          },
        };
      assert.equal(name, '@tauri-apps/api/event');
      return {
        listen: async (name, callback) => {
          await beforeListen(name);
          listeners.set(name, callback);
          return () => {
            stopped.push(name);
            listeners.delete(name);
          };
        },
      };
    },
  };
  for (const name of [
    'setState',
    'setLevel',
    'setMessage',
    'setLocalAsrText',
    'setTranslation',
    'setWarming',
    'setSelectionPolish',
    'setCapsuleStyle',
    'setSuggestions',
    'setInsertFallback',
  ]) {
    scope[name] = (value) => {
      values[name] = value;
      writes.push([name, value]);
    };
  }
  vm.runInNewContext(effectCode, scope);
  const cleanup = scope.replayEffect();
  return {
    cleanup,
    listeners,
    stopped,
    delays,
    writes,
    warnings,
    values,
    scope,
    get requests() {
      return requests;
    },
    emit: (name, payload) => listeners.get(name)?.({ payload }),
  };
}

async function until(predicate) {
  for (let i = 0; i < 20 && !predicate(); i += 1) await tick();
  assert.ok(predicate(), 'effect did not settle within the bounded event turns');
}
function release(view) {
  view.cleanup();
  assert.equal(view.listeners.size, 0);
  assert.equal(new Set(view.stopped).size, view.stopped.length, 'each listener is released once');
}
function transcript(view, sequence, text, sessionId = 'session') {
  view.emit('backend:event', {
    sequence,
    sessionId,
    kind: { type: 'transcript_delta', payload: { text, offset: 0, isFinal: true } },
  });
}

// A lazy mirror replays a session whose starting event already happened.
{
  const view = mount({ snapshot: () => frame() });
  await until(() => view.values.setState === 'recording');
  assert.equal(view.values.setLocalAsrText, 'existing text');
  view.emit('capsule:state', { state: 'recording', sessionId: 'session' });
  assert.equal(view.values.setLocalAsrText, 'existing text');
  transcript(view, 11, 'updated text');
  assert.equal(view.values.setLocalAsrText, 'updated text');
  view.emit('capsule:state', { state: 'done', sessionId: 'session' });
  assert.equal(view.values.setLocalAsrText, '');
  release(view);
}

// The subscription wrapper must forward and clear the fallback payload unchanged.
{
  const view = mount({ snapshot: () => frame() });
  await until(() => view.values.setState === 'recording');
  const fallback = { text: 'copied text', reason: 'target unavailable' };
  view.emit('insert:fallback', fallback);
  assert.equal(view.values.setInsertFallback, fallback);
  view.emit('insert:fallback', null);
  assert.equal(view.values.setInsertFallback, null);
  const lateEvent = view.listeners.get('insert:fallback');
  release(view);
  lateEvent({ payload: fallback });
  assert.equal(view.values.setInsertFallback, null);
}

// Real state and transcript events must survive an older in-flight snapshot.
{
  const pending = deferred();
  const view = mount({
    snapshot: (n) =>
      n === 1
        ? pending.promise
        : frame({ sequence: 21, revision: 21, payloadRevision: 21, transcript: 'new text' }),
  });
  await until(() => view.requests === 1);
  view.emit('capsule:state', { state: 'recording', sessionId: 'session' });
  transcript(view, 21, 'new text');
  assert.equal(view.values.setLocalAsrText, 'new text');
  pending.resolve(
    frame({
      state: 'idle',
      sessionId: null,
      sequence: 1,
      revision: 1,
      payloadRevision: 1,
      transcript: '',
    }),
  );
  await until(() => view.requests === 2);
  await tick();
  assert.equal(view.values.setState, 'recording');
  assert.equal(view.values.setLocalAsrText, 'new text');
  assert.ok(view.writes.every(([name, value]) => name !== 'setState' || value !== 'idle'));
  release(view);
}

// A newer backend watermark rejects both the text and payload of a stale replay.
{
  const pending = deferred();
  const view = mount({ snapshot: () => pending.promise });
  await until(() => view.requests === 1);
  view.emit('backend:event', {
    sequence: 40,
    sessionId: 'session',
    kind: { type: 'dictation_state_changed', payload: { phase: 'starting', sessionId: 'session' } },
  });
  transcript(view, 41, 'latest text');
  pending.resolve(frame());
  await tick();
  assert.equal(view.values.setLocalAsrText, 'latest text');
  assert.equal(view.values.setState, 'idle');
  assert.equal(view.requests, 1);
  release(view);
}

// Torn payload/text frames are retried before any of their presentation is applied.
{
  const view = mount({
    snapshot: (n) => (n === 1 ? frame({ state: 'idle', payloadRevision: 9 }) : frame()),
  });
  await until(() => view.values.setState === 'recording');
  assert.equal(view.requests, 2);
  assert.ok(view.writes.every(([name, value]) => name !== 'setState' || value !== 'idle'));
  release(view);
}
for (const snapshot of [() => frame({ payloadRevision: 9 }), () => null]) {
  const view = mount({ snapshot });
  await until(() => view.requests === 6);
  await tick();
  assert.deepEqual(view.delays, [16, 32, 64, 128, 256]);
  assert.equal(view.writes.length, 0);
  release(view);
}

// A failed read keeps the already subscribed live stream usable.
{
  const view = mount({
    snapshot: () => {
      throw new Error('native read failed');
    },
  });
  await until(() => view.warnings.length === 1);
  assert.equal(view.listeners.size, 4);
  view.emit('capsule:state', { state: 'recording', sessionId: 'session' });
  transcript(view, 11, 'live after failure');
  assert.equal(view.values.setLocalAsrText, 'live after failure');
  release(view);
}

// Unmount during a read releases subscriptions and ignores the late response.
{
  const pending = deferred();
  const view = mount({ snapshot: () => pending.promise });
  await until(() => view.requests === 1);
  release(view);
  const writes = view.writes.length;
  pending.resolve(frame());
  await tick();
  assert.equal(view.writes.length, writes);
  assert.equal(view.requests, 1);
}

// An unfinished native subscription must also be released after unmount.
{
  const pending = deferred();
  const view = mount({
    beforeListen: (name) => (name === 'vocab:suggested' ? pending.promise : undefined),
  });
  await until(() => view.listeners.size === 2);
  release(view);
  pending.resolve();
  await tick();
  assert.equal(view.listeners.size, 0);
  assert.equal(view.stopped.length, 3);
  assert.equal(view.requests, 0);
}

// A registration failure cleans up the listeners that were already registered.
{
  const view = mount({
    beforeListen: (name) => {
      if (name === 'vocab:suggested') throw new Error('native subscription failed');
    },
  });
  await until(() => view.warnings.length === 1);
  assert.equal(view.listeners.size, 0);
  assert.equal(view.stopped.length, 2);
  assert.equal(view.requests, 0);
  release(view);
}

// Each card stream has its own revision: a vocabulary event must not lose a fallback replay.
{
  const pending = deferred();
  const view = mount({ auxiliary: () => pending.promise });
  await until(() => view.listeners.size === 4);
  await tick();
  const live = [{ id: 'live', pattern: 'old', replacement: 'new' }];
  const fallback = { text: 'not inserted', reason: 'missing target' };
  view.emit('vocab:suggested', live);
  pending.resolve({ suggestions: [], fallback });
  await until(() => view.values.setInsertFallback === fallback);
  assert.equal(view.values.setSuggestions, live);
  release(view);
}
{
  const pending = deferred();
  const view = mount({ auxiliary: () => pending.promise });
  await until(() => view.listeners.size === 4);
  await tick();
  view.emit('insert:fallback', null);
  const suggestions = [{ id: 'pending', pattern: 'a', replacement: 'b' }];
  pending.resolve({ suggestions, fallback: { text: 'stale' } });
  await until(() => view.values.setSuggestions === suggestions);
  assert.equal(view.values.setInsertFallback, null);
  release(view);
}
{
  const pending = deferred();
  const view = mount({ auxiliary: () => pending.promise });
  await until(() => view.listeners.size === 4);
  release(view);
  const writes = view.writes.length;
  pending.resolve({ suggestions: [], fallback: null });
  await tick();
  assert.equal(view.writes.length, writes);
}
console.log('capsule ready replay: session, card ordering, bounded retries and cleanup passed');
