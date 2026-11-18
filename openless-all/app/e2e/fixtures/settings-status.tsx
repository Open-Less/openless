import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { mockSettings } from '../../src/lib/ipc/mock-data';
import type { EncryptedSyncStatus } from '../../src/api/cloud-sync';
import { applyFontScale, type FontScaleId } from '../../src/lib/fontScale';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
applyFontScale((params.get('scale') as FontScaleId) ?? 'medium');
let status: EncryptedSyncStatus = {
  sequence: '1', enabled: true, authState: 'signed_in', keyState: 'unlocked', syncState: 'ready',
  account: { githubId: 'fixture-owner', login: 'fixture' }, vaultId: 'fixture-vault', keyId: 'fixture-key',
  localGeneration: '1', lastSyncedLocalGeneration: '1', remoteRevision: '7',
  lastSuccessfulSyncAt: null, pendingOperationId: null, lastError: null, recoveryRequired: false,
  hasCloudSnapshot: true, taskId: null, serviceOrigin: 'https://sync.example', backupRetentionDays: null,
  consentVersion: 'encrypted-full-snapshot-v1',
};
const calls: string[] = [];
let saveResult: ((error?: boolean) => void) | undefined;
let syncResult: ((error?: boolean) => void) | undefined;
let stored = JSON.stringify({ apiKey: 'fixture-key', voice: 'stella' });
const fixture = {
  calls,
  hide: () => {},
  finishSave: (error = false) => saveResult?.(error),
  finishSync: (error = false) => syncResult?.(error),
  stored: () => stored,
  restoreDuringSync: () => {
    update({ localGeneration: '2' });
    return emit('cloud-sync-e2ee:restored', {
      sequence: status.sequence, serviceOrigin: status.serviceOrigin, accountId: status.account!.githubId,
      vaultId: status.vaultId, taskId: status.taskId, localGeneration: '2', uiPreferences: {},
    });
  },
};
Object.assign(window, { settingsStatusFixture: fixture });
const update = (change: Partial<EncryptedSyncStatus>) => {
  status = { ...status, ...change, sequence: String(Number(status.sequence) + 1) };
  return { ...status };
};
const publish = () => emit('cloud-sync-e2ee:state', {
  sequence: status.sequence, serviceOrigin: status.serviceOrigin, accountId: status.account!.githubId,
  vaultId: status.vaultId, taskId: status.taskId, status: { ...status },
});
mockIPC((command, args) => {
  if (command === 'get_startup_snapshot') return { backend: { running: true }, contractVersion: '2.0.0' };
  if (command === 'get_platform_capabilities') return { platform: 'desktop', supportsDesktopHotkey: false };
  if (command === 'get_settings_snapshot') return { preferences: { ...mockSettings, themeMode: params.get('theme') ?? 'light' }, revision: 1 };
  if (command === 'set_remote_locale') return undefined;
  if (command === 'marketplace_auth_status') return { signedIn: true };
  if (command === 'cloud_sync_e2ee_status') return { ...status };
  if (command === 'read_credential') return stored;
  if (command === 'set_credential') {
    calls.push(command);
    return new Promise<void>((resolve, reject) => {
      saveResult = (error) => {
        if (error) reject({ code: 'secure_storage_denied' });
        else { stored = (args as { value: string }).value; resolve(); }
      };
    });
  }
  if (command === 'cloud_sync_e2ee_sync_now') {
    calls.push(command);
    update({ syncState: 'syncing', taskId: `task-${status.sequence}`, lastError: null });
    void publish();
    return new Promise((resolve, reject) => {
      syncResult = (error) => {
        update({ syncState: error ? 'failed' : 'ready', taskId: null,
          lastError: error ? { code: 'transport_failed', retryAfterSeconds: null } : null,
          lastSuccessfulSyncAt: error ? null : new Date().toISOString() });
        void publish();
        if (error) reject({ code: 'transport_failed' });
        else resolve({ ...status });
      };
    });
  }
  if (command === 'cloud_sync_e2ee_set_enabled') { calls.push(command); return update({ enabled: false }); }
  throw new Error(`Unexpected fixture command: ${command}`);
}, { shouldMockEvents: true });

const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { AppViewport, PreviewFrame } = await import('../../src/ui/layout');
const { InteractionProvider } = await import('../../src/ui/overlays/InteractionProvider');
const { HotkeySettingsProvider } = await import('../../src/data/preferences');
const { DeepSeekSuiteSection } = await import('../../src/features/providers/DeepSeekSuiteSection');
const { CloudSyncSection } = await import('../../src/features/cloud-sync/CloudSyncSection');

function Fixture() {
  const [visible, setVisible] = useState(true);
  fixture.hide = () => setVisible(false);
  return <main style={{ width: '100%', overflow: 'auto', padding: 16 }}>
    {visible && <><DeepSeekSuiteSection /><CloudSyncSection /></>}
  </main>;
}
createRoot(document.getElementById('root')!).render(
  <StrictMode><PreviewFrame width={Number(params.get('width') ?? 700)} height={Number(params.get('height') ?? 460)}><AppViewport>
    <InteractionProvider><HotkeySettingsProvider><Fixture /></HotkeySettingsProvider></InteractionProvider>
  </AppViewport></PreviewFrame></StrictMode>,
);
