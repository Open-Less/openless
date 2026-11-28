// Offline Speech Kit controller regression: real ArkTS source, deterministic SDK mocks.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../../harmony/entry/src/main/ets/common/SystemSpeech.ets', import.meta.url),
  'utf8',
);
const card = readFileSync(
  new URL(
    '../../harmony/entry/src/main/ets/components/models/SystemSpeechCard.ets',
    import.meta.url,
  ),
  'utf8',
);
const compiled = ts.transpileModule(source.replace(/^import .*;\n/gm, ''), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
function harness({ supported = true, granted = true, response = [0] } = {}) {
  const storage = new Map([
    ['dictPhase', 'idle'],
    ['appForeground', true],
  ]);
  const timers = new Map();
  const updates = [];
  const engines = [];
  const creates = [];
  let permissions = 0,
    timerId = 0,
    pendingCreate,
    pendingPermission;
  function engine() {
    const e = {
      starts: [],
      finishes: [],
      cancels: [],
      shutdowns: 0,
      listener: undefined,
      setListener(listener) {
        this.listener = listener;
      },
      startListening(params) {
        assert.ok(this.listener, 'listener is installed before starting');
        this.starts.push(params);
      },
      finish(id) {
        this.finishes.push(id);
      },
      cancel(id) {
        this.cancels.push(id);
      },
      shutdown() {
        this.shutdowns++;
      },
    };
    engines.push(e);
    return e;
  }
  const exports = {};
  vm.runInNewContext(compiled, {
    exports,
    canIUse: (capability) => {
      assert.equal(capability, 'SystemCapability.AI.SpeechRecognizer');
      return supported;
    },
    Store: { put: (key, value) => storage.set(key, value) },
    AppStorage: { get: (key) => storage.get(key) },
    abilityAccessCtrl: {
      GrantStatus: { PERMISSION_GRANTED: 0 },
      createAtManager: () => ({
        checkAccessTokenSync: () => (granted ? 0 : -1),
        requestPermissionsFromUser: async (_ctx, names) => {
          permissions++;
          assert.deepEqual(Array.from(names), ['ohos.permission.MICROPHONE']);
          return pendingPermission ? pendingPermission.promise : { authResults: response };
        },
      }),
    },
    speechRecognizer: {
      createEngine: async (params) => {
        creates.push(params);
        return pendingCreate ? pendingCreate.promise : engine();
      },
    },
    setTimeout: (callback, delay) => {
      const id = ++timerId;
      timers.set(id, { callback, delay });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  });
  const controller = new exports.SystemSpeech((snapshot) => updates.push(snapshot));
  return {
    controller,
    storage,
    updates,
    engines,
    creates,
    timers,
    engine,
    start: () => controller.start({ applicationInfo: { accessTokenId: 1 } }),
    permissions: () => permissions,
    waitCreate: () => (pendingCreate = deferred()),
    waitPermission: () => (pendingPermission = deferred()),
    fire: (delay) => {
      const entry = [...timers.entries()].find(([, timer]) => timer.delay === delay);
      assert.ok(entry, 'deadline exists: ' + delay);
      timers.delete(entry[0]);
      entry[1].callback();
    },
  };
}

const unavailable = harness({ supported: false });
assert.equal(unavailable.creates.length, 0, 'construction never starts speech');
await unavailable.start();
assert.equal(unavailable.permissions(), 0);
assert.equal(unavailable.creates.length, 0);
assert.match(unavailable.controller.snapshot().error, /不会切换到云端/);

const denied = harness({ granted: false, response: [-1] });
await denied.start();
assert.equal(denied.permissions(), 1);
assert.equal(denied.creates.length, 0);
assert.equal(denied.storage.get('systemSpeechBusy'), false);
assert.equal(denied.timers.size, 0);
for (const phase of ['starting', 'recording', 'processing', 'cancelling']) {
  const busy = harness();
  busy.storage.set('dictPhase', phase);
  await busy.start();
  assert.equal(busy.creates.length, 0);
}
const monitor = harness({ granted: false });
monitor.storage.set('microphoneMonitorBusy', true);
await monitor.start();
assert.equal(monitor.creates.length, 0, 'microphone monitor excludes offline trial');
assert.equal(monitor.permissions(), 0, 'monitor exclusion happens before permission preparation');
const hidden = harness();
hidden.storage.set('appForeground', false);
await hidden.start();
assert.equal(hidden.creates.length, 0);

const consent = harness({ granted: false });
const permission = consent.waitPermission();
const permissionStart = consent.start();
consent.controller.cancel();
permission.resolve({ authResults: [0] });
await permissionStart;
assert.equal(consent.creates.length, 0, 'late permission cannot start recording');

const late = harness();
const lateCreation = late.waitCreate();
const lateStart = late.start();
late.controller.cancel();
const lateEngine = late.engine();
lateCreation.resolve(lateEngine);
await lateStart;
assert.equal(lateEngine.shutdowns, 1, 'late engine is released');
assert.equal(lateEngine.starts.length, 0, 'late engine never starts audio');

const normal = harness();
await normal.start();
await normal.start();
assert.equal(normal.creates.length, 1, 'duplicate starts are ignored');
assert.deepEqual(JSON.parse(JSON.stringify(normal.creates[0])), {
  language: 'zh-CN',
  online: 1,
  extraParams: { locate: 'CN', recognizerMode: 'short' },
});
const e = normal.engines[0];
const id = e.starts[0].sessionId;
assert.deepEqual(JSON.parse(JSON.stringify(e.starts[0].audioInfo)), {
  audioType: 'pcm',
  sampleRate: 16000,
  soundChannel: 1,
  sampleBit: 16,
});
assert.equal(e.starts[0].extraParams.recognitionMode, 0, 'system captures audio');
assert.equal(normal.controller.snapshot().phase, 'starting');
e.listener.onStart(id, 'start');
assert.equal(normal.controller.snapshot().phase, 'listening');
e.listener.onResult('wrong_session', { result: 'wrong', isFinal: true, isLast: true });
assert.equal(normal.controller.snapshot().text, '');
e.listener.onResult(id, { result: '离线试用', isFinal: false, isLast: false });
assert.equal(normal.controller.snapshot().text, '离线试用');
normal.controller.finish();
assert.deepEqual(e.finishes, [id]);
assert.equal(normal.controller.snapshot().phase, 'finishing');
e.listener.onResult(id, { result: '离线试用结果', isFinal: true, isLast: true });
e.listener.onComplete(id, 'done');
assert.equal(normal.controller.snapshot().phase, 'done');
assert.equal(e.shutdowns, 1);
assert.equal(normal.storage.get('systemSpeechBusy'), false);
assert.equal(normal.timers.size, 0);
e.listener.onError(id, 1002200011, 'late');
assert.equal(normal.controller.snapshot().text, '离线试用结果');
await normal.start();
const fresh = normal.engines[1];
const freshId = fresh.starts[0].sessionId;
e.listener.onResult(freshId, { result: 'old generation', isFinal: true, isLast: true });
assert.equal(
  normal.controller.snapshot().text,
  '',
  'generation rejects old engine with current session id',
);
fresh.listener.onError(freshId, 1002200006, 'busy');
assert.match(normal.controller.snapshot().error, /正忙/);
assert.equal(fresh.shutdowns, 1);

for (const mode of ['create', 'start', 'finish', 'recording', 'permission']) {
  const h = harness({ granted: mode !== 'permission' });
  let pending;
  if (mode === 'create') pending = h.waitCreate();
  if (mode === 'permission') pending = h.waitPermission();
  const starting = h.start();
  if (pending) {
    h.fire(mode === 'permission' ? 30000 : 10000);
    if (mode === 'create') pending.resolve(h.engine());
    else pending.resolve({ authResults: [0] });
    await starting;
  } else {
    await starting;
    const current = h.engines[0];
    if (mode !== 'start') current.listener.onStart(current.starts[0].sessionId, 'start');
    if (mode === 'finish') h.controller.finish();
    h.fire(mode === 'recording' ? 25000 : 10000);
  }
  assert.equal(h.controller.snapshot().phase, 'error', mode + ' timeout');
  assert.equal(h.storage.get('systemSpeechBusy'), false);
  assert.equal(h.timers.size, 0);
  for (const engine of h.engines) assert.equal(engine.shutdowns, 1);
}

const disposed = harness();
const disposedCreation = disposed.waitCreate();
const disposedStart = disposed.start();
disposed.controller.dispose();
const count = disposed.updates.length;
const disposedEngine = disposed.engine();
disposedCreation.resolve(disposedEngine);
await disposedStart;
assert.equal(disposed.updates.length, count, 'disposed components receive no callbacks');
assert.equal(disposedEngine.shutdowns, 1);
await disposed.start();
assert.equal(disposed.creates.length, 1);

const switched = harness();
const switchedCreate = switched.waitCreate();
const switchedStart = switched.start();
switched.storage.set('dictPhase', 'recording');
const switchedEngine = switched.engine();
switchedCreate.resolve(switchedEngine);
await switchedStart;
assert.equal(switchedEngine.starts.length, 0, 'main dictation rechecked after create');
assert.equal(switchedEngine.shutdowns, 1);

const cleanup = harness();
await cleanup.start();
const cleanupEngine = cleanup.engines[0];
cleanupEngine.cancel = () => {
  throw Error('cancel failed');
};
cleanup.controller.cancel();
assert.equal(cleanupEngine.shutdowns, 1, 'shutdown runs even if cancel throws');
assert.equal(cleanup.controller.snapshot().text, '');
assert.doesNotMatch(
  source,
  /console\.|Api\.|writeAudio\(/,
  'no content logs, core calls or manually supplied audio',
);
assert.match(card, /@StorageLink\('appForeground'\) @Watch\('onForeground'\)/);
assert.match(card, /@StorageLink\('dictPhase'\) @Watch\('onDictation'\)/);
assert.match(card, /aboutToDisappear\(\): void \{[\s\S]*?\.dispose\(\)/);
assert.doesNotMatch(card, /@Prop\s+onCopy|\bApi\./);
console.log(
  'System offline speech: official parameters, permission denial, mutual exclusion, result/finish, cancellation, stale sessions, late creation, lifecycle cleanup and all deadlines passed',
);
