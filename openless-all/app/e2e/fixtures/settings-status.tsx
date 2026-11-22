import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { mockSettings } from '../../src/lib/ipc/mock-data';
import type { LocalAsrDownloadProgress } from '../../src/api/local-asr';
import {
  CLOUD_SYNC_E2EE_CONSENT_VERSION,
  type cloudSyncE2eeDeleteRemote,
  type EncryptedSyncStatus,
} from '../../src/api/cloud-sync';
import { applyFontScale, type FontScaleId } from '../../src/lib/fontScale';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
import '../../src/styles/refinements.css';

const params = new URLSearchParams(location.search);
applyFontScale((params.get('scale') as FontScaleId) ?? 'medium');
let status: EncryptedSyncStatus = {
  sequence: '1',
  enabled: true,
  authState: 'signed_in',
  keyState: 'unlocked',
  syncState: 'ready',
  account: { githubId: 'fixture-owner', login: 'fixture' },
  vaultId: 'fixture-vault',
  keyId: 'fixture-key',
  localGeneration: '1',
  lastSyncedLocalGeneration: '1',
  remoteRevision: '7',
  lastSuccessfulSyncAt: null,
  pendingOperationId: null,
  lastError: null,
  recoveryRequired: false,
  hasCloudSnapshot: true,
  taskId: null,
  serviceOrigin: 'https://sync.example',
  backupRetentionDays: null,
  consentVersion: CLOUD_SYNC_E2EE_CONSENT_VERSION,
};
export type SettingsOperation = 'save' | 'sync' | 'delete';
type DeleteRequest = Parameters<typeof cloudSyncE2eeDeleteRemote>[0];
const calls: string[] = [];
const deleteRequests: DeleteRequest[] = [];
const cancelledSyncTasks: string[] = [];
const cancelledDownloads: string[] = [];
const attempts = { save: 0, sync: 0, delete: 0 };
const pending = new Map<SettingsOperation, { attempt: number; finish: (error: boolean) => void }>();
let stored = JSON.stringify({ apiKey: 'fixture-key', voice: 'stella' });

function deferOperation<T>(
  operation: SettingsOperation,
  complete: (error: boolean) => T,
): Promise<T> {
  if (pending.has(operation)) throw new Error(`Duplicate pending ${operation} operation`);
  return new Promise((resolve, reject) => {
    pending.set(operation, {
      attempt: ++attempts[operation],
      finish: (error) => {
        try {
          resolve(complete(error));
        } catch (cause) {
          reject(cause);
        }
      },
    });
  });
}

const fixture = {
  calls,
  deleteRequests,
  cancelledSyncTasks,
  cancelledDownloads,
  hasDownloadListener: () => Boolean(listeners.get('local-asr-download-progress')?.size),
  emitDownload(modelId: string, phase: LocalAsrDownloadProgress['phase']) {
    return emit('local-asr-download-progress', {
      modelId,
      phase,
      file: 'model.bin',
      fileIndex: 0,
      fileCount: 1,
      bytesDownloaded: phase === 'finished' ? 100 : 1,
      bytesTotal: 100,
      error: null,
    } satisfies LocalAsrDownloadProgress);
  },
  hide: () => {},
  pendingAttempt: (operation: SettingsOperation) => pending.get(operation)?.attempt ?? null,
  finish: (operation: SettingsOperation, attempt: number, error = false) => {
    const active = pending.get(operation);
    if (active?.attempt !== attempt) throw new Error(`No pending ${operation} attempt ${attempt}`);
    pending.delete(operation);
    active.finish(error);
  },
  stored: () => stored,
  canRestoreDuringSync: () => Boolean(listeners.get('cloud-sync-e2ee:restored')?.size),
  restoreDuringSync: () => {
    update({ localGeneration: '2' });
    return emit('cloud-sync-e2ee:restored', {
      sequence: status.sequence,
      serviceOrigin: status.serviceOrigin,
      accountId: status.account!.githubId,
      vaultId: status.vaultId,
      taskId: status.taskId,
      localGeneration: '2',
      uiPreferences: {},
    });
  },
};
export type SettingsStatusFixture = typeof fixture;
Object.assign(window, { settingsStatusFixture: fixture });
const update = (change: Partial<EncryptedSyncStatus>) => {
  status = { ...status, ...change, sequence: String(Number(status.sequence) + 1) };
  return { ...status };
};
const publish = () =>
  emit('cloud-sync-e2ee:state', {
    sequence: status.sequence,
    serviceOrigin: status.serviceOrigin,
    accountId: status.account!.githubId,
    vaultId: status.vaultId,
    taskId: status.taskId,
    status: { ...status },
  });
const listeners = new Map<string, Set<number>>();
mockWindows('main');
mockIPC((command, args) => {
  // Tauri's built-in event mock removes listeners by `id`, but unlisten sends `eventId`.
  // In StrictMode the first effect teardown would otherwise leave dead callbacks in its table.
  if (command === 'plugin:event|listen') {
    const { event, handler } = args as { event: string; handler: number };
    const callbacks = listeners.get(event) ?? new Set<number>();
    callbacks.add(handler);
    listeners.set(event, callbacks);
    return handler;
  }
  if (command === 'plugin:event|unlisten') {
    const { event, eventId } = args as { event: string; eventId: number };
    listeners.get(event)?.delete(eventId);
    return undefined;
  }
  if (command === 'plugin:event|emit') {
    const { event, payload } = args as { event: string; payload: unknown };
    const { runCallback } = window.__TAURI_INTERNALS__ as {
      runCallback: (id: number, data: unknown) => void;
    };
    for (const id of listeners.get(event) ?? []) runCallback(id, { id, event, payload });
    return undefined;
  }
  if (command === 'get_startup_snapshot')
    return { backend: { running: true }, contractVersion: '2.0.0' };
  if (command === 'get_platform_capabilities')
    return { platform: 'desktop', supportsDesktopHotkey: false };
  if (command === 'get_settings_snapshot')
    return {
      preferences: { ...mockSettings, themeMode: params.get('theme') ?? 'light' },
      revision: 1,
    };
  if (command === 'set_remote_locale' || command === 'plugin:app|set_app_theme') return undefined;
  if (command === 'marketplace_auth_status') return { signedIn: true };
  if (command === 'cloud_sync_e2ee_status') return { ...status };
  if (command === 'experimental_asr_status') return [];
  if (command === 'local_asr_cancel_download') {
    cancelledDownloads.push((args as { modelId: string }).modelId);
    return undefined;
  }
  if (command === 'read_credential') return stored;
  if (command === 'set_credential') {
    calls.push(command);
    return deferOperation('save', (error) => {
      if (error) throw { code: 'secure_storage_denied' };
      stored = (args as { value: string }).value;
    });
  }
  if (command === 'cloud_sync_e2ee_sync_now') {
    calls.push(command);
    const result = deferOperation('sync', (error) => {
      const cancelled = status.taskId !== null && cancelledSyncTasks.includes(status.taskId);
      const code = cancelled ? 'cancelled' : error ? 'transport_failed' : null;
      const next = update({
        syncState: cancelled ? 'pending' : error ? 'failed' : 'ready',
        taskId: null,
        lastError: code ? { code, retryAfterSeconds: null } : null,
        lastSuccessfulSyncAt: code ? status.lastSuccessfulSyncAt : new Date().toISOString(),
      });
      void publish();
      if (code) throw { code };
      return next;
    });
    update({ syncState: 'syncing', taskId: `task-${status.sequence}`, lastError: null });
    void publish();
    return result;
  }
  if (command === 'cloud_sync_e2ee_cancel') {
    const { taskId } = args as { taskId: string };
    if (status.taskId !== taskId) throw { code: 'stale_task' };
    cancelledSyncTasks.push(taskId);
    return { ...status };
  }
  if (command === 'cloud_sync_e2ee_delete_remote') {
    calls.push(command);
    deleteRequests.push(args as DeleteRequest);
    return deferOperation('delete', (error) => {
      if (error) throw { code: 'transport_failed' };
      return update({
        enabled: false,
        hasCloudSnapshot: false,
        keyState: 'locked',
        keyId: null,
        vaultId: null,
        remoteRevision: '8',
      });
    });
  }
  if (command === 'cloud_sync_e2ee_set_enabled') {
    calls.push(command);
    return update({ enabled: false });
  }
  throw new Error(`Unexpected fixture command: ${command}`);
});

const { i18nReady } = await import('../../src/i18n');
await i18nReady;
const { AppViewport, PreviewFrame, TransitionViewport } = await import('../../src/ui/layout');
const { InteractionProvider } = await import('../../src/ui/overlays/InteractionProvider');
const { HotkeySettingsProvider } = await import('../../src/data/preferences');
const { DeepSeekSuiteSection } = await import('../../src/features/providers/DeepSeekSuiteSection');
const { CloudSyncSection } = await import('../../src/features/cloud-sync/CloudSyncSection');
const { CloudSyncWelcome } = await import('../../src/features/cloud-sync/CloudSyncSetupPrompt');
const { PresenceModal } = await import('../../src/ui/overlays/Modal');
const { WebSearchSection } = await import('../../src/features/settings/sections/WebSearchSection');
const { GlobalDownloadProgress } =
  await import('../../src/features/downloads/GlobalDownloadProgress');

function Fixture() {
  const [visible, setVisible] = useState(true);
  fixture.hide = () => setVisible(false);
  return (
    <main style={{ width: '100%', overflow: 'auto', padding: 16 }}>
      {visible && (
        <>
          <DeepSeekSuiteSection />
          <CloudSyncSection />
        </>
      )}
    </main>
  );
}
function WebSearchFixture() {
  const [active, setActive] = useState(false);
  return (
    <main style={{ width: '100%', display: 'flex', flexDirection: 'column' }}>
      <button style={{ alignSelf: 'flex-start' }} onClick={() => setActive(!active)}>
        {active ? 'Leave web search' : 'Open web search'}
      </button>
      <TransitionViewport
        contentKey={active ? 'web-search' : 'next-page'}
        page={active ? 'web-search' : 'next-page'}
        mode="page"
      >
        {active ? <WebSearchSection /> : <h3>Next settings page</h3>}
      </TransitionViewport>
    </main>
  );
}

function DownloadFixture() {
  const [open, setOpen] = useState(false);
  return (
    <main style={{ width: '100%', padding: 16 }}>
      <button onClick={() => setOpen(true)}>Open download modal</button>
      <PresenceModal
        open={open}
        labelledBy="download-modal-title"
        onClose={() => setOpen(false)}
        render={() => <h3 id="download-modal-title">Download modal</h3>}
      />
    </main>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewFrame
      width={Number(params.get('width') ?? 700)}
      height={Number(params.get('height') ?? 460)}
    >
      <AppViewport>
        <InteractionProvider
          notificationAside={params.has('download') ? <GlobalDownloadProgress /> : undefined}
        >
          {params.has('download') ? (
            <DownloadFixture />
          ) : params.has('webSearch') ? (
            <WebSearchFixture />
          ) : (
            <HotkeySettingsProvider>
              {params.has('cloudWelcome') ? <CloudSyncWelcome inline /> : <Fixture />}
            </HotkeySettingsProvider>
          )}
        </InteractionProvider>
      </AppViewport>
    </PreviewFrame>
  </StrictMode>,
);
