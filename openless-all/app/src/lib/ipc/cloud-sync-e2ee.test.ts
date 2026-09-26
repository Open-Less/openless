// @ts-nocheck — Node-only runtime harness; production UI and IPC remain strictly typed.
import assert from 'node:assert/strict';
import type { EncryptedSyncStatus } from './cloud-sync-e2ee';

const calls: Array<{ command: string; args: unknown }> = [];
const response = { marker: 'native-result' };
let failure: unknown = null;
const stored = new Map([
  ['ol.locale', 'fr'],
  ['ol-font-scale', 'large'],
]);
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    localStorage: { getItem: (key: string) => stored.get(key) ?? null },
    addEventListener() {},
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: unknown) => {
        if (command === 'get_startup_snapshot')
          return { contractVersion: '2.0.0', backend: { running: true } };
        if (command === 'set_remote_locale') return;
        calls.push({ command, args });
        if (failure) throw failure;
        return response;
      },
    },
  },
});

try {
  const api = await import('./cloud-sync-e2ee');
  const secret = 'OnlyA1SyntheticFixture';
  const create = {
    password: secret,
    passwordConfirmation: secret,
    rememberKey: true,
    consentVersion: api.CLOUD_SYNC_E2EE_CONSENT_VERSION,
    observedRevision: '9007199254740993',
  };
  const change = {
    currentPassword: secret,
    newPassword: `${secret}2`,
    confirmation: `${secret}2`,
    rememberKey: false,
  };
  const apply = {
    previewId: 'preview',
    mode: 'merge' as const,
    conflictChoices: [{ id: 'opaque', side: 'cloud' as const }],
  };
  const deletion = {
    expectedVaultId: 'vault',
    observedRevision: '9007199254740993',
    confirmed: true,
  };
  const cases: Array<[string, unknown, () => Promise<unknown>]> = [
    ['status', undefined, api.cloudSyncE2eeStatus],
    ['claim_setup_prompt', undefined, api.cloudSyncE2eeClaimSetupPrompt],
    [
      'prepare_enable',
      { consentVersion: api.CLOUD_SYNC_E2EE_CONSENT_VERSION },
      () => api.cloudSyncE2eePrepareEnable(api.CLOUD_SYNC_E2EE_CONSENT_VERSION),
    ],
    ['create', create, () => api.cloudSyncE2eeCreate(create)],
    [
      'unlock',
      { password: secret, rememberKey: false },
      () => api.cloudSyncE2eeUnlock({ password: secret, rememberKey: false }),
    ],
    ['lock', undefined, api.cloudSyncE2eeLock],
    ['set_enabled', { enabled: false }, () => api.cloudSyncE2eeSetEnabled(false)],
    ['sync_now', undefined, api.cloudSyncE2eeSyncNow],
    ['cancel', { taskId: 'task' }, () => api.cloudSyncE2eeCancel('task')],
    [
      'preview_restore',
      { observedRevision: '9007199254740993' },
      () => api.cloudSyncE2eePreviewRestore('9007199254740993'),
    ],
    ['apply_restore', apply, () => api.cloudSyncE2eeApplyRestore(apply)],
    ['change_password', change, () => api.cloudSyncE2eeChangePassword(change)],
    ['delete_remote', deletion, () => api.cloudSyncE2eeDeleteRemote(deletion)],
    ['sign_out', undefined, api.cloudSyncE2eeSignOut],
    ['get_ui_preferences', undefined, api.cloudSyncE2eeGetUiPreferences],
  ];
  for (const [command, args, invoke] of cases) {
    assert.equal(await invoke(), response);
    assert.deepEqual(calls.pop(), { command: `cloud_sync_e2ee_${command}`, args: args ?? {} });
  }
  // The mirror uses the same revisioned queue as live changes; exercised by
  // scripts/encrypted-sync-ui-bridge.test.mjs, including consent preparation.
  assert.equal(stored.get('ol.locale'), 'fr');
  assert.equal(stored.get('ol-font-scale'), 'large');
  failure = {
    code: 'provider',
    details: { reason: 'revision_conflict' },
    message: 'DO_NOT_RENDER_PRIVATE_RESPONSE',
  };
  assert.equal(await api.cloudSyncE2eeSyncNow().catch((error: unknown) => error), failure);
  assert.equal(api.encryptedSyncErrorKey(failure), 'changed');
  assert.equal(api.encryptedSyncErrorKey({ message: 'DO_NOT_RENDER_PRIVATE_RESPONSE' }), 'unknown');
  assert.equal(api.encryptedSyncErrorKey({ details: { reason: '__proto__' } }), 'unknown');

  const status = {
    sequence: '9007199254740993',
    account: { githubId: 'owner', login: 'name' },
    vaultId: 'vault',
    taskId: 'task',
    serviceOrigin: 'https://sync.example',
  } as EncryptedSyncStatus;
  const event = {
    sequence: '9007199254740994',
    accountId: 'owner',
    vaultId: 'vault',
    taskId: 'task',
  };
  assert(api.matchesEncryptedSyncEvent(status, event, status.sequence));
  for (const invalid of [
    { ...event, sequence: status.sequence },
    { ...event, sequence: '01' },
    { ...event, sequence: '18446744073709551616' },
    { ...event, accountId: 'other' },
    { ...event, vaultId: 'other' },
    { ...event, taskId: 'old-task' },
  ])
    assert(!api.matchesEncryptedSyncEvent(status, invalid, status.sequence));
  assert(
    api.matchesEncryptedSyncEvent(status, { ...event, taskId: 'new-task' }, status.sequence, false),
  );
  assert.notEqual(
    api.encryptedSyncScope(status),
    api.encryptedSyncScope({ ...status, serviceOrigin: 'https://other.example' }),
  );
  assert.equal(api.syncSequence('18446744073709551615'), 18446744073709551615n);
  assert.equal(api.syncSequence('not-a-sequence'), null);
  assert(api.syncPasswordMeetsBasicRequirements('LongFixture1Ab'));
  assert(api.syncPasswordMeetsBasicRequirements('A1' + '界'.repeat(10)) === false);
  assert(!api.syncPasswordMeetsBasicRequirements('shortA1'));
  assert(!api.syncPasswordMeetsBasicRequirements('A1a' + '界'.repeat(126)));
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}
console.log('cloud-sync-e2ee.test.ts passed');
