import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { mockSettings } from '../../src/lib/ipc/mock-data';
import {
  CLOUD_SYNC_E2EE_CONSENT_VERSION,
  type EncryptedSyncStatus,
} from '../../src/api/cloud-sync';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const mode = new URLSearchParams(location.search).get('mode');
const fresh = mode === 'fresh';
const cold = mode === 'renew-cold';
const disabled = cold && sessionStorage.getItem('sync-fixture-disabled') === 'true';
let failRefresh = cold && !disabled;
const owner = { githubId: '42', login: 'fixture' };
let status: EncryptedSyncStatus = {
  sequence: '1',
  enabled: !fresh && !disabled,
  authState: fresh || cold ? 'signed_out' : 'signed_in',
  keyState: fresh || cold ? 'locked' : 'unlocked',
  syncState: fresh || disabled ? 'disabled' : 'ready',
  account: fresh || cold ? null : owner,
  vaultId: fresh || cold ? null : 'old-vault',
  keyId: fresh || cold ? null : 'old-key',
  localGeneration: '1',
  lastSyncedLocalGeneration: fresh ? null : '1',
  remoteRevision: fresh || cold ? null : '7',
  lastSuccessfulSyncAt: null,
  pendingOperationId: null,
  lastError: null,
  recoveryRequired: false,
  hasCloudSnapshot: fresh || cold ? null : true,
  taskId: null,
  serviceOrigin: 'https://sync.example',
  backupRetentionDays: 7,
  consentVersion: fresh
    ? null
    : mode === 'renew' || cold
      ? 'encrypted-full-snapshot-v1'
      : CLOUD_SYNC_E2EE_CONSENT_VERSION,
};
const attempts: Array<{ command: string; args: unknown }> = [];
let finish: ((failed: boolean) => void) | undefined;
Object.assign(window, {
  syncDialogFixture: { attempts, finish: (failed = false) => finish?.(failed) },
});
function update(change: Partial<EncryptedSyncStatus>) {
  status = { ...status, ...change, sequence: String(Number(status.sequence) + 1) };
  if (cold) void emit('backend:event', { kind: { type: 'cloud_sync_state_changed' } });
  void emit('cloud-sync-e2ee:state', {
    sequence: status.sequence,
    serviceOrigin: status.serviceOrigin,
    accountId: status.account?.githubId ?? null,
    vaultId: status.vaultId,
    taskId: status.taskId,
    status: { ...status },
  });
  return { ...status };
}
mockIPC(
  (command, args) => {
    switch (command) {
      case 'get_startup_snapshot':
        return { contractVersion: '2.0.0', backend: { running: true } };
      case 'get_platform_capabilities':
        return { platform: 'desktop', supportsDesktopHotkey: false };
      case 'get_settings_snapshot':
        return { preferences: { ...mockSettings, themeMode: 'light' }, revision: 1 };
      case 'marketplace_auth_status':
        return { signedIn: true };
      case 'set_remote_locale':
        return undefined;
      case 'cloud_sync_e2ee_status':
        return { ...status };
      case 'cloud_sync_e2ee_claim_setup_prompt':
        return false;
      case 'cloud_sync_e2ee_get_ui_preferences_snapshot':
        return { preferences: { locale: 'en', fontScale: 'medium' }, revision: '1' };
      case 'cloud_sync_e2ee_set_enabled':
        attempts.push({ command, args });
        if (cold && (args as { enabled: boolean }).enabled === false) {
          sessionStorage.setItem('sync-fixture-disabled', 'true');
          return update({ enabled: false, syncState: 'disabled' });
        }
        throw new Error('Unexpected enabled state');
      case 'cloud_sync_e2ee_refresh_backup_metadata':
        attempts.push({ command, args });
        if (failRefresh) {
          failRefresh = false;
          throw { details: { reason: 'transport_failed' } };
        }
        return update({ account: owner, authState: 'signed_in', syncState: status.enabled ? 'unlock_required' : 'disabled', hasCloudSnapshot: true, vaultId: 'old-vault', keyId: 'old-key', remoteRevision: '7' });
      case 'cloud_sync_e2ee_prepare_enable':
      case 'cloud_sync_e2ee_create':
      case 'cloud_sync_e2ee_delete_remote':
        attempts.push({ command, args });
        update({ syncState: 'syncing', taskId: `task-${status.sequence}`, lastError: null });
        return new Promise((resolve, reject) => {
          finish = (failed) => {
            finish = undefined;
            if (failed) {
              update({
                syncState: 'failed',
                taskId: null,
                lastError: { code: 'transport_failed', retryAfterSeconds: null },
              });
              reject({ details: { reason: 'transport_failed' } });
            } else if (command === 'cloud_sync_e2ee_prepare_enable') {
              resolve({
                nextStep: fresh ? 'create' : 'ready',
                status: update(
                  fresh
                    ? {
                        consentVersion: CLOUD_SYNC_E2EE_CONSENT_VERSION,
                        account: owner,
                        authState: 'signed_in',
                        syncState: 'disabled',
                        taskId: null,
                        hasCloudSnapshot: false,
                        remoteRevision: '7',
                      }
                    : {
                        consentVersion: CLOUD_SYNC_E2EE_CONSENT_VERSION,
                        syncState: 'ready',
                        taskId: null,
                      },
                ),
              });
            } else if (command === 'cloud_sync_e2ee_create') {
              resolve(
                update({
                  enabled: true,
                  keyState: 'unlocked',
                  syncState: 'ready',
                  taskId: null,
                  hasCloudSnapshot: true,
                  vaultId: 'new-vault',
                  keyId: 'new-key',
                  remoteRevision: '8',
                }),
              );
            } else {
              resolve(
                update({
                  enabled: false,
                  keyState: 'locked',
                  syncState: 'disabled',
                  taskId: null,
                  hasCloudSnapshot: false,
                  vaultId: null,
                  keyId: null,
                  remoteRevision: '8',
                }),
              );
            }
          };
        });
      default:
        throw new Error(`Unexpected cloud sync fixture command: ${command}`);
    }
  },
  { shouldMockEvents: true },
);

const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { AppViewport, PreviewFrame } = await import('../../src/ui/layout');
const { InteractionProvider } = await import('../../src/ui/overlays/InteractionProvider');
const { HotkeySettingsProvider } = await import('../../src/data/preferences');
const { CloudSyncSection } = await import('../../src/features/cloud-sync/CloudSyncSection');
const { CloudSyncSetupPrompt } = await import('../../src/features/cloud-sync/CloudSyncSetupPrompt');

createRoot(document.getElementById('root')!).render(
  <PreviewFrame width={650} height={470}>
    <AppViewport>
      <InteractionProvider>
        <HotkeySettingsProvider>
          <main style={{ height: '100%', overflow: 'auto', padding: 16 }}>
            <CloudSyncSection />
            {(mode === 'renew' || cold) && <CloudSyncSetupPrompt blocked={false} onSetup={() => {}} />}
          </main>
        </HotkeySettingsProvider>
      </InteractionProvider>
    </AppViewport>
  </PreviewFrame>,
);
