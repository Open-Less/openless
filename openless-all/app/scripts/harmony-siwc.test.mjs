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
const calls = [];
const opened = [];
let expire;
let release = 0;
let browserFails = false;
const exports = {};
let finishSlow;
const nativeHost = {
  invokeAsync: async (command, raw) => {
    const args = JSON.parse(raw);
    calls.push({ command, args });
    if (command === 'slow_settings')
      return new Promise((resolve) => {
        finishSlow = resolve;
      });
    if (command === '__siwc_begin')
      return JSON.stringify({
        ok: true,
        value: {
          attemptId: 'attempt',
          url: 'https://auth.openai.com/api/accounts/authorize?id_token_hint=private-id-token',
        },
      });
    if (command === '__credential_writes') return JSON.stringify({ ok: true, value: [] });
    if (command === '__revision_snapshot')
      return JSON.stringify({ ok: true, value: { preferencesRevision: 0 } });
    return JSON.stringify({ ok: true, value: { state: 'signed_out' } });
  },
};
vm.runInNewContext(compiled, {
  exports,
  console,
  VoiceClient: class {},
  nativeHost,
  publishVoiceConfiguration: async () => {},
  backgroundTaskManager: {
    requestSuspendDelay: (_reason, callback) => {
      expire = callback;
      return { requestId: 1, actualDelayTime: 60000 };
    },
    cancelSuspendDelay: () => release++,
  },
});
const api = new exports.HarmonyApi();
api.configure(
  {
    startAbility: async (want) => {
      opened.push(want.uri);
      if (browserFails) throw Error(want.uri);
    },
  },
  {},
);
assert.equal(JSON.parse(api.invoke('__siwc_begin', '{}')).ok, false);
const blocked = await api.invokeAsync('__siwc_begin', '{}');
assert.equal(JSON.parse(blocked).ok, false);
assert.equal(calls.length, 0);
const result = await api.invokeAsync(
  'siwc_action',
  JSON.stringify({ channelId: 'channel', action: 'sign_in' }),
);
assert.equal(JSON.parse(result).ok, true);
assert(!result.includes('private-id-token'));
assert.equal(opened.length, 1);
expire();
await Promise.resolve();
assert(calls.some((c) => c.command === 'siwc_action' && c.args.action === 'cancel'));
assert(release > 0);
browserFails = true;
const failed = await api.invokeAsync(
  'siwc_action',
  JSON.stringify({ channelId: 'channel', action: 'sign_in' }),
);
assert.equal(JSON.parse(failed).ok, false);
assert(!failed.includes('private-id-token'));
console.log(
  'Harmony SIWC: native-only launch, URL redaction, background expiry and cancellation passed',
);

// Authorization control must not queue behind unrelated settings I/O.
const pendingSettings = api.invokeAsync('slow_settings', '{}');
for (let i = 0; i < 20 && !finishSlow; i++) await new Promise((resolve) => setImmediate(resolve));
assert(finishSlow);
const immediateCancel = await Promise.race([
  api.invokeAsync('siwc_action', JSON.stringify({ channelId: 'channel', action: 'cancel' })),
  new Promise((_, reject) =>
    setTimeout(() => reject(new Error('SIWC cancellation was queued')), 500),
  ),
]);
assert.equal(JSON.parse(immediateCancel).ok, true);
finishSlow(JSON.stringify({ ok: true, value: null }));
await pendingSettings;
console.log('Harmony SIWC: cancellation remains responsive while settings persistence is blocked');
