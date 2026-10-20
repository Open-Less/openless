import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../harmony/entry/src/main/ets/HarmonyApi.ets', import.meta.url),
  'utf8',
)
  .replace(/\r\n/g, '\n')
  .replace(/^import .*;\n/gm, '');
const compiled = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
function host(granted = true) {
  const events = [];
  let starts = 0,
    cancels = 0,
    level,
    failed;
  let stopped = Promise.resolve({ rawText: '模拟识别', polishedText: '模拟润色' });
  let startup = Promise.resolve();
  class VoiceClient {
    async start(_context, onLevel, onFailure) {
      starts++;
      level = onLevel;
      failed = onFailure;
      await startup;
    }
    async stop() {
      return stopped;
    }
    async cancel() {
      cancels++;
    }
  }
  const exports = {};
  vm.runInNewContext(compiled, {
    exports,
    console,
    VoiceClient,
    abilityAccessCtrl: {
      GrantStatus: { PERMISSION_GRANTED: 0 },
      createAtManager: () => ({
        checkAccessTokenSync: () => (granted ? 0 : -1),
        requestPermissionsFromUser: async () => ({ authResults: [-1] }),
      }),
    },
  });
  const api = new exports.HarmonyApi();
  api.configure(
    { applicationInfo: { accessTokenId: 1 } },
    {
      runJavaScript: async (code) =>
        vm.runInNewContext(code, {
          window: { dispatchEvent: (event) => events.push(event) },
          CustomEvent: class {
            constructor(type, options) {
              this.type = type;
              this.detail = options?.detail;
            }
          },
        }),
    },
  );
  return {
    api,
    events,
    starts: () => starts,
    cancels: () => cancels,
    level: (value) => level(value),
    fail: (message) => failed(message),
    stopWith: (reply) => {
      stopped = reply;
    },
    startWith: (reply) => {
      startup = reply;
    },
    invoke: async (command) => JSON.parse(await api.invokeAsync(command, '{}')),
    last: () =>
      events.filter((event) => event.type === 'openless:harmony-dictation').at(-1)?.detail,
  };
}
const normal = host();
assert.equal((await normal.invoke('start_dictation')).ok, true);
assert.equal(normal.last().phase, 'recording');
assert.equal((await normal.invoke('start_dictation')).ok, false, 'duplicate start is rejected');
assert.equal(normal.starts(), 1);
normal.level(0.5);
assert.equal(normal.last().level, 0.5);
assert.equal(
  (await normal.invoke('start_microphone_level_monitor')).ok,
  false,
  'monitor cannot take over the recording microphone',
);
const completed = await normal.invoke('stop_dictation');
assert.deepEqual(completed.value, { rawText: '模拟识别', polishedText: '模拟润色' });
assert.equal(normal.last().phase, 'idle');
assert(normal.events.some((event) => event.type === 'openless:harmony-backend-change'));
normal.fail('迟到的回调');
assert.equal(normal.last().error, '', 'finished sessions ignore late callbacks');

const cancelled = host();
await cancelled.invoke('start_dictation');
let finishStop;
cancelled.stopWith(
  new Promise((resolve) => {
    finishStop = resolve;
  }),
);
const pendingStop = cancelled.invoke('stop_dictation');
assert.equal(cancelled.last().phase, 'processing');
cancelled.level(0.8);
assert.equal(
  cancelled.last().phase,
  'processing',
  'late audio must not revive recording while ASR is processing',
);
await cancelled.invoke('cancel_dictation');
finishStop({ rawText: '过期结果', polishedText: '' });
assert.equal(
  (await pendingStop).ok,
  false,
  'cancelled processing cannot return a result to the UI',
);
assert.equal(cancelled.last().phase, 'idle');
assert.equal(cancelled.cancels(), 1);

const startup = host();
let finishStart;
startup.startWith(
  new Promise((resolve) => {
    finishStart = resolve;
  }),
);
const pendingStart = startup.invoke('start_dictation');
for (let i = 0; i < 10; i++) await Promise.resolve();
await startup.api.release();
finishStart();
assert.equal((await pendingStart).ok, false, 'leaving the page cancels an unfinished startup');
assert.equal(startup.last().phase, 'idle');

const denied = host(false);
assert.equal((await denied.invoke('start_dictation')).ok, false);
assert.equal(denied.starts(), 0, 'denied microphone permission never starts audio capture');
assert.equal(denied.last().phase, 'idle');

const failed = host();
await failed.invoke('start_dictation');
failed.fail('音频传输失败');
assert.equal(failed.last().error, '音频传输失败');
assert.equal(failed.last().phase, 'idle');
console.log(
  'Harmony app dictation: start/stop/results, level feedback, duplicate start, cancellation races, page cleanup and permission denial passed',
);
