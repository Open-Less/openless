import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
const app = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(import.meta.url);
const ts = require('typescript');
const source = readFileSync(app + '/src/lib/encryptedSyncUiBridge.ts', 'utf8');
const code = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const tick = () => new Promise((r) => setImmediate(r));
const drain = async () => {
  for (let i = 0; i < 8; i++) await tick();
};
const defer = () => {
  let resolve, reject;
  const promise = new Promise((r, e) => {
    resolve = r;
    reject = e;
  });
  return { promise, resolve, reject };
};
function context({ fresh = false } = {}) {
  const window = new EventTarget(),
    handlers = new Map(),
    statusQueue = [],
    attempts = [];
  let errors = 0;
  window.addEventListener('openless:sync-ui-persistence-failed', () => errors++);
  let locale = 'en',
    fontScale = 'medium',
    rev = fresh ? 0 : 1;
  let preferences = fresh ? null : { locale, fontScale };
  let status = {
    sequence: '0',
    account: fresh ? null : { githubId: '1' },
    vaultId: fresh ? null : 'v1',
    consentVersion: fresh ? null : 'yes',
    taskId: null,
  };
  const revision = () => (rev ? `00000000-0000-4000-8000-${String(rev).padStart(12, '0')}` : null);
  const event = (key, source) =>
    window.dispatchEvent(
      new CustomEvent('openless:ui-preferences-changed', { detail: { key, source } }),
    );
  const i18n = {
    getLocalePreference: () => locale,
    setLocalePreference: async (value, source = 'user') => {
      locale = value;
      event('locale', source);
    },
    SUPPORTED_LOCALES: ['en', 'fr', 'de'],
  };
  const fonts = {
    readFontScale: () => fontScale,
    setFontScale: (value, source = 'user') => {
      fontScale = value;
      event('fontScale', source);
    },
  };
  const invoke = async (cmd, args) => {
    if (cmd === 'cloud_sync_e2ee_status')
      return statusQueue.length ? statusQueue.shift().promise : { ...status };
    if (cmd === 'cloud_sync_e2ee_get_ui_preferences_snapshot')
      return { preferences: preferences && { ...preferences }, revision: revision() };
    if (
      cmd === 'cloud_sync_e2ee_set_ui_preferences_checked' ||
      cmd === 'cloud_sync_e2ee_set_ui_preferences'
    ) {
      attempts.push({ cmd, ...args });
      if (cmd.endsWith('_checked') && args.expectedRevision !== revision())
        throw { details: { reason: 'stale_preview' } };
      const next = { locale: args.locale, fontScale: args.fontScale };
      if (JSON.stringify(next) !== JSON.stringify(preferences)) {
        preferences = next;
        rev++;
      }
      return;
    }
    throw new Error(cmd);
  };
  const exports = {},
    shared = { isTauri: true, invokeOrMock: invoke };
  const localRequire = (name) =>
    name === './ipc/shared'
      ? shared
      : name === '../i18n'
        ? i18n
        : name === './fontScale'
          ? fonts
          : name === '@tauri-apps/api/event'
            ? {
                listen: async (name, fn) => {
                  handlers.set(name, fn);
                  return () => handlers.delete(name);
                },
              }
            : null;
  new Function('require', 'exports', 'window', 'location', 'Event', 'CustomEvent', code)(
    localRequire,
    exports,
    window,
    { search: '' },
    Event,
    CustomEvent,
  );
  return {
    install: exports.installEncryptedSyncUiBridge,
    flush: exports.flushEncryptedSyncUiPreferences,
    userLocale: i18n.setLocalePreference,
    userFont: fonts.setFontScale,
    statusQueue,
    attempts,
    setStatus: (value) => {
      status = { ...status, ...value };
    },
    snapshot: () => ({ locale, fontScale, preferences, revision: revision(), errors }),
    externalWrite: (value = {}) => {
      preferences = { ...preferences, ...value };
      rev++;
    },
    unchecked: () => invoke('cloud_sync_e2ee_set_ui_preferences', { locale, fontScale }),
    restore: (sequence = '1', scope = {}) =>
      handlers.get('cloud-sync-e2ee:restored')({
        payload: {
          sequence,
          accountId: status.account.githubId,
          vaultId: status.vaultId,
          taskId: 'restore',
          ...scope,
        },
      }),
  };
}
let failed = 0;
async function test(name, fn) {
  try {
    await fn();
    console.log('PASS ' + name);
  } catch (e) {
    failed++;
    console.log('FAIL ' + name + '\n' + e.message);
  }
}
await test('old mirror queued before restore cannot overwrite its native value', async () => {
  const c = context();
  await c.install();
  const wait = defer();
  c.statusQueue.push(wait);
  await c.userLocale('de');
  await tick();
  c.externalWrite({ locale: 'fr' });
  c.restore();
  await tick();
  wait.resolve({ sequence: '2', account: { githubId: '1' }, vaultId: 'v1', consentVersion: 'yes' });
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'fr');
  assert.equal(c.snapshot().locale, 'fr');
});
await test('new choice after restore notification wins only its chosen field', async () => {
  const c = context();
  await c.install();
  const wait = defer();
  c.statusQueue.push(wait);
  c.externalWrite({ locale: 'fr', fontScale: 'large' });
  c.restore();
  await tick();
  await c.userLocale('de');
  wait.resolve({ sequence: '2', account: { githubId: '1' }, vaultId: 'v1', consentVersion: 'yes' });
  await drain();
  assert.deepEqual(c.snapshot().preferences, { locale: 'de', fontScale: 'large' });
  assert.equal(c.snapshot().locale, 'de');
  assert.equal(c.snapshot().fontScale, 'large');
});
await test('prepare mirror must not permanently stale the live bridge revision', async () => {
  const c = context({ fresh: true });
  await c.install();
  await c.flush();
  c.setStatus({ account: { githubId: '1' }, vaultId: 'v1', consentVersion: 'yes' });
  await c.userLocale('de');
  await drain();
  c.userFont('large');
  await drain();
  assert.deepEqual(
    c.snapshot().preferences,
    { locale: 'de', fontScale: 'large' },
    JSON.stringify(c.snapshot()),
  );
});
await test('legitimate account switch must rebase the cached mirror scope', async () => {
  const c = context();
  await c.install();
  c.setStatus({ sequence: '3', account: { githubId: '2' }, vaultId: 'v2' });
  await c.userLocale('de');
  await drain();
  c.userFont('large');
  await drain();
  assert.deepEqual(
    c.snapshot().preferences,
    { locale: 'de', fontScale: 'large' },
    JSON.stringify(c.snapshot()),
  );
});
await test('rollback-only UUID change must not permanently poison later user saves', async () => {
  const c = context();
  await c.install();
  c.externalWrite({ locale: 'en' });
  await c.userLocale('de');
  await drain();
  await c.userLocale('fr');
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'fr', JSON.stringify(c.snapshot()));
});

await test('a new vault restore is hydrated after an account switch', async () => {
  const c = context();
  await c.install();
  c.setStatus({ sequence: '3', account: { githubId: '2' }, vaultId: 'v2' });
  c.externalWrite({ locale: 'fr', fontScale: 'large' });
  c.restore('4');
  await drain();
  assert.equal(c.snapshot().locale, 'fr');
  assert.equal(c.snapshot().fontScale, 'large');
  await c.userLocale('de');
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'de');
});
await test('an equal-value restore invalidates an older unsent user choice', async () => {
  const c = context();
  await c.install();
  const wait = defer();
  c.statusQueue.push(wait);
  await c.userLocale('de');
  await tick();
  c.externalWrite({ locale: 'en' });
  c.restore();
  wait.resolve({ sequence: '2', account: { githubId: '1' }, vaultId: 'v1', consentVersion: 'yes' });
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'en');
  await c.userLocale('fr');
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'fr');
});
await test('prepare and user edits use only the checked writer', async () => {
  const c = context({ fresh: true });
  await c.install();
  await c.flush();
  c.setStatus({ consentVersion: 'yes' });
  await c.userLocale('de');
  c.userFont('large');
  await drain();
  assert.deepEqual(c.snapshot().preferences, { locale: 'de', fontScale: 'large' });
  assert(c.attempts.every((a) => a.cmd === 'cloud_sync_e2ee_set_ui_preferences_checked'));
});
await test('a late old-vault event cannot discard a new-account user choice', async () => {
  const c = context();
  await c.install();
  c.setStatus({ account: { githubId: '2' }, vaultId: 'v2' });
  const wait = defer();
  c.statusQueue.push(wait);
  await c.userLocale('de');
  await tick();
  c.restore('99', { accountId: '1', vaultId: 'v1' });
  wait.resolve({ account: { githubId: '2' }, vaultId: 'v2', consentVersion: 'yes' });
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'de');
  c.externalWrite({ locale: 'fr' });
  c.restore('5');
  await drain();
  assert.equal(c.snapshot().locale, 'fr');
});
await test('a failed old-scope notification read retains the newer local choice', async () => {
  const c = context();
  await c.install();
  c.setStatus({ account: { githubId: '2' }, vaultId: 'v2' });
  const userStatus = defer(),
    restoreStatus = defer();
  c.statusQueue.push(userStatus, restoreStatus);
  await c.userLocale('de');
  await tick();
  c.restore('99', { accountId: '1', vaultId: 'v1' });
  userStatus.resolve({ account: { githubId: '2' }, vaultId: 'v2', consentVersion: 'yes' });
  await tick();
  restoreStatus.reject(new Error('transient native status failure'));
  await drain();
  assert.equal(c.snapshot().preferences.locale, 'de', JSON.stringify(c.snapshot()));
  assert.equal(c.snapshot().locale, 'de');
});
await test('overlapping valid and old-scope notifications preserve a later field choice', async () => {
  const c = context();
  await c.install();
  c.setStatus({ account: { githubId: '2' }, vaultId: 'v2' });
  const wait = defer();
  c.statusQueue.push(wait);
  c.externalWrite({ locale: 'fr', fontScale: 'large' });
  c.restore('4');
  await tick();
  await c.userLocale('de');
  c.restore('99', { accountId: '1', vaultId: 'v1' });
  wait.resolve({ account: { githubId: '2' }, vaultId: 'v2', consentVersion: 'yes' });
  await drain();
  assert.deepEqual(
    c.snapshot().preferences,
    { locale: 'de', fontScale: 'large' },
    JSON.stringify(c.snapshot()),
  );
  assert.equal(c.snapshot().locale, 'de');
  assert.equal(c.snapshot().fontScale, 'large');
});
console.log(`failures=${failed}`);
process.exitCode = failed ? 1 : 0;
