import assert from 'node:assert/strict';
import { appendFileSync, readFileSync } from 'node:fs';
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
const railSource = readFileSync(
  new URL('../src/components/LiveTranscriptPill.tsx', import.meta.url),
  'utf8',
);
const railAst = ts.createSourceFile(
  'LiveTranscriptPill.tsx',
  railSource,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX,
);
let railEffect;
function visitRail(node) {
  if (
    ts.isCallExpression(node) &&
    node.expression.getText(railAst) === 'useEffect' &&
    node.arguments[0]?.getText(railAst).includes('getCapsuleSnapshot')
  ) {
    railEffect = node.arguments[0].getText(railAst);
  }
  ts.forEachChild(node, visitRail);
}
visitRail(railAst);
assert.ok(railEffect, 'test must execute the actual separate transcript overlay effect');
const railEffectCode = compile(`globalThis.replayEffect = (${railEffect});`);

// Match Tauri 2's routing: an Any listener receives even events addressed to another label.
function eventBus() {
  const handlers = new Set();
  return {
    listen(label, name, callback, options) {
      const entry = { label, name, callback, target: options?.target ?? { kind: 'Any' } };
      handlers.add(entry);
      return () => handlers.delete(entry);
    },
    emit(label, name, payload) {
      for (const entry of handlers) {
        const target = entry.target;
        if (
          entry.name === name &&
          (label === null || target.kind === 'Any' || target.label === label || target === label)
        ) {
          entry.callback({ payload });
        }
      }
    },
    get size() {
      return handlers.size;
    },
  };
}

// Optional, single session-owned diagnostic log. Never record transcript content.
function probe(probeId, event, safe) {
  if (!process.env.OPENLESS_CAPSULE_DIAG_LOG) return;
  appendFileSync(
    process.env.OPENLESS_CAPSULE_DIAG_LOG,
    JSON.stringify({
      schema: 'diagnosing-bugs.v1',
      session_id: 'capsule-live-switch',
      run_id: process.env.OPENLESS_CAPSULE_DIAG_RUN ?? 'regression',
      probe_id: probeId,
      hypothesis_ids: ['H1', 'H2'],
      event,
      outcome: 'observed',
      safe,
      ts: new Date().toISOString(),
    }) + '\n',
  );
}
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
  label = 'capsule',
  bus,
  overlay = false,
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
    transcriptRef: { current: backend.exports.createTranscriptViewState() },
    stateRef: { current: 'idle' },
    snapshotRetryDelays: [0, 16, 32, 64, 128, 256],
    capsuleTranscriptFontSize: (value) => value ?? 14,
    getSettings: async () => ({ capsuleStyle: 'classic' }),
    parseCapsuleStyle: () => undefined,
    console: { info() {}, warn: (...args) => warnings.push(args) },
    window: {
      setTimeout: (done, delay) => {
        delays.push(delay);
        return setImmediate(done);
      },
    },
    getCapsuleSnapshot: async () => {
      assert.equal(
        listeners.size,
        overlay ? 3 : 4,
        'all event subscriptions must precede snapshot reads',
      );
      requests += 1;
      return snapshot(requests);
    },
    require: (name) => {
      if (name === '@tauri-apps/api/webviewWindow')
        return { getCurrentWebviewWindow: () => ({ label }) };
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
        listen: async (name, callback, options) => {
          await beforeListen(name);
          listeners.set(name, callback);
          const stopBus = bus?.listen(label, name, callback, options);
          return () => {
            stopBus?.();
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
    'setText',
    'setStyle',
    'setEnabled',
    'setFontSize',
  ]) {
    scope[name] = (value) => {
      values[name] = value;
      writes.push([name, value]);
    };
  }
  vm.runInNewContext(overlay ? railEffectCode : effectCode, scope);
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
// Real primary/mirror effects: leaving allScreens must not clear or re-enter the active view.
for (const overlay of [false, true]) {
  const bus = eventBus();
  const primaryLabel = overlay ? 'capsule-rail' : 'capsule';
  const mirrorLabel = overlay ? 'capsule-rail-mirror-0' : 'capsule-mirror-0';
  const primary = mount({ bus, overlay, label: primaryLabel, snapshot: () => frame() });
  const mirror = mount({ bus, overlay, label: mirrorLabel, snapshot: () => frame() });
  await until(
    () => primary.values.setState === 'recording' && mirror.values.setState === 'recording',
  );
  const textKey = overlay ? 'setText' : 'setLocalAsrText';
  const primaryWrites = primary.writes.length;
  probe('P1', 'before_switch', {
    overlay,
    primary_active: primary.values.setState === 'recording',
  });
  bus.emit(mirrorLabel, 'capsule:state', { state: 'idle', sessionId: 'session' });
  probe('P2', 'mirror_hidden', {
    overlay,
    primary_active: primary.values.setState === 'recording',
    primary_text_present: Boolean(primary.values[textKey]),
  });
  assert.equal(primary.values.setState, 'recording', 'mirror-only idle must not exit the primary');
  assert.equal(
    primary.values[textKey],
    'existing text',
    'mirror-only idle must not clear primary text',
  );
  assert.equal(mirror.values.setState, 'idle');
  for (let i = 11; i < 31; i += 1) {
    bus.emit(primaryLabel, 'capsule:state', { state: 'recording', sessionId: 'session' });
    bus.emit(null, 'backend:event', {
      sequence: i,
      sessionId: 'session',
      kind: {
        type: 'transcript_delta',
        payload: { text: 'x'.repeat(i), offset: 0, isFinal: true },
      },
    });
    // Repeat a hidden mirror cleanup, including the feedback induced by native layout sync.
    bus.emit(mirrorLabel, 'capsule:state', { state: 'idle', sessionId: 'session' });
    assert.equal(primary.values.setState, 'recording');
    assert.equal(primary.values[textKey], 'x'.repeat(i));
  }
  assert.ok(
    primary.writes
      .slice(primaryWrites)
      .every(([key, value]) => key !== 'setState' || value !== 'idle'),
  );
  probe('P3', 'stream_finished', { overlay, updates: 20, primary_active: true });
  // Returning to allScreens must replay the active session rather than reopening the old-session gate.
  bus.emit(mirrorLabel, 'capsule:state', { state: 'recording', sessionId: 'session' });
  await until(() => mirror.values.setState === 'recording');
  transcript(mirror, 31, 'reactivated text');
  assert.equal(mirror.values[textKey], 'reactivated text');
  assert.equal(primary.values[textKey], 'x'.repeat(30));
  release(primary);
  release(mirror);
  assert.equal(bus.size, 0);
  probe('P4', 'cleanup_finished', { overlay, listeners: bus.size });
}
// Only the primary capsule may drive native transcript visibility, including teardown.
{
  const code = compile(
    readFileSync(new URL('../src/lib/ipc/dictation.ts', import.meta.url), 'utf8'),
  );
  for (const label of [
    'capsule',
    'capsule-mirror-0',
    'capsule-rail',
    'capsule-rail-mirror-0',
    'main',
  ]) {
    const calls = [];
    const module = { exports: {} };
    vm.runInNewContext(code, {
      exports: module.exports,
      require(name) {
        if (name === './platform-exports') return { isDesktop: () => true };
        if (name === './shared')
          return {
            isTauriNow: () => true,
            invokeOrMock: async (command, args) => calls.push({ command, visible: args.visible }),
          };
        assert.equal(name, '@tauri-apps/api/webviewWindow');
        return { getCurrentWebviewWindow: () => ({ label }) };
      },
    });
    await module.exports.setCapsuleTranscriptVisible(true);
    await module.exports.setCapsuleTranscriptVisible(false);
    assert.equal(
      calls.length,
      label === 'capsule' ? 2 : 0,
      'mirror updates and cleanup must not change primary transcript visibility',
    );
  }
}
// The main window's recording cue must not see synthetic mirror lifecycle events either.
{
  const cueSource = readFileSync(
    new URL('../src/components/AudioCue.tsx', import.meta.url),
    'utf8',
  );
  const cueAst = ts.createSourceFile(
    'AudioCue.tsx',
    cueSource,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  let cueEffect;
  function visitCue(node) {
    if (
      ts.isCallExpression(node) &&
      node.expression.getText(cueAst) === 'useEffect' &&
      node.arguments[0]?.getText(cueAst).includes("'capsule:state'")
    )
      cueEffect = node.arguments[0].getText(cueAst);
    ts.forEachChild(node, visitCue);
  }
  visitCue(cueAst);
  assert.ok(cueEffect);
  const bus = eventBus();
  let plays = 0;
  let stops = 0;
  const scope = {
    isTauri: true,
    audioCueRuntimeEnabled: true,
    prevStateRef: { current: 'idle' },
    audioCueEnabledRef: { current: true },
    playRecordStartCue: () => plays++,
    stopAudioCue: () => stops++,
    require(name) {
      if (name === '@tauri-apps/api/webviewWindow')
        return { getCurrentWebviewWindow: () => ({ label: 'main' }) };
      assert.equal(name, '@tauri-apps/api/event');
      return {
        listen: async (name, callback, options) => bus.listen('main', name, callback, options),
      };
    },
  };
  vm.runInNewContext(compile(`globalThis.effect = (${cueEffect});`), scope);
  const cleanup = scope.effect();
  await until(() => bus.size === 1);
  bus.emit('main', 'capsule:state', { state: 'recording' });
  bus.emit('capsule-mirror-0', 'capsule:state', { state: 'idle' });
  bus.emit('main', 'capsule:state', { state: 'recording' });
  assert.equal(plays, 1, 'hidden mirror cleanup must not replay the recording cue');
  assert.equal(stops, 0);
  bus.emit('main', 'capsule:state', { state: 'idle' });
  assert.equal(stops, 1);
  cleanup();
  assert.equal(bus.size, 0);
}
console.log(
  'capsule ready replay: session, card ordering, bounded retries, cleanup and live display isolation passed',
);
