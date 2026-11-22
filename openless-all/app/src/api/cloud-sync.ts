import type {
  EncryptedSyncConflictEvent,
  EncryptedSyncEvent,
  EncryptedSyncRestoreEvent,
} from '../lib/ipc/cloud-sync-e2ee';
import { listenTauriEvent } from '../runtime/transport';
import type { BackendEvent } from './contracts';

export * from '../lib/ipc/cloud-sync';
export * from '../lib/ipc/cloud-sync-e2ee';

const SETUP_ELIGIBILITY_EVENTS = new Set([
  'backend_started',
  'preferences_changed',
  'credentials_changed',
  'dictation_completed',
  'dictation_state_changed',
  'qa_state',
  'selection_state_changed',
  'selection_voice_state_changed',
  'less_computer_event',
  'local_asr_engine_changed',
  'permission_changed',
  'cloud_sync_state_changed',
]);

export function listenCloudSyncEligibilityChanges(onChange: () => void): Promise<() => void> {
  return listenTauriEvent<BackendEvent>('backend:event', ({ kind }) => {
    if (SETUP_ELIGIBILITY_EVENTS.has(kind.type)) onChange();
  });
}

export function listenEncryptedSyncStateChanges(
  onChange: (event: EncryptedSyncEvent) => void,
): Promise<() => void> {
  return listenTauriEvent('cloud-sync-e2ee:state', onChange);
}

export function listenEncryptedSyncConflicts(
  onConflict: (event: EncryptedSyncConflictEvent) => void,
): Promise<() => void> {
  return listenTauriEvent('cloud-sync-e2ee:conflict', onConflict);
}

export function listenEncryptedSyncRestores(
  onRestore: (event: EncryptedSyncRestoreEvent) => void,
): Promise<() => void> {
  return listenTauriEvent('cloud-sync-e2ee:restored', onRestore);
}
