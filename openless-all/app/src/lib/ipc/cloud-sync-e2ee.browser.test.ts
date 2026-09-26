// @ts-nocheck — Node-only runtime harness; production UI and IPC remain strictly typed.
import assert from 'node:assert/strict';
import * as api from './cloud-sync-e2ee';

const operations: Array<() => Promise<unknown>> = [
  api.cloudSyncE2eeStatus,
  api.cloudSyncE2eeClaimSetupPrompt,
  () => api.cloudSyncE2eePrepareEnable(api.CLOUD_SYNC_E2EE_CONSENT_VERSION),
  () =>
    api.cloudSyncE2eeCreate({
      password: 'SyntheticOnly1A',
      passwordConfirmation: 'SyntheticOnly1A',
      rememberKey: true,
      consentVersion: api.CLOUD_SYNC_E2EE_CONSENT_VERSION,
      observedRevision: '0',
    }),
  () => api.cloudSyncE2eeUnlock({ password: 'SyntheticOnly1A', rememberKey: true }),
  api.cloudSyncE2eeLock,
  () => api.cloudSyncE2eeSetEnabled(true),
  api.cloudSyncE2eeSyncNow,
  () => api.cloudSyncE2eeCancel('task'),
  () => api.cloudSyncE2eePreviewRestore('1'),
  () =>
    api.cloudSyncE2eeApplyRestore({ previewId: 'preview', mode: 'replace', conflictChoices: [] }),
  () =>
    api.cloudSyncE2eeChangePassword({
      currentPassword: 'SyntheticOnly1A',
      newPassword: 'SyntheticOnly2A',
      confirmation: 'SyntheticOnly2A',
      rememberKey: true,
    }),
  () =>
    api.cloudSyncE2eeDeleteRemote({
      expectedVaultId: 'vault',
      observedRevision: '1',
      confirmed: true,
    }),
  api.cloudSyncE2eeSignOut,
  api.cloudSyncE2eeGetUiPreferences,
  api.mirrorEncryptedSyncUiPreferences,
];
for (const invoke of operations) {
  await assert.rejects(
    invoke,
    (error: unknown) => api.encryptedSyncErrorKey(error) === 'unavailable',
  );
}
console.log('cloud-sync-e2ee.browser.test.ts passed');
