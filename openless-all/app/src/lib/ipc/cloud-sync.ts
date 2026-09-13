import { invokeOrMock } from './shared';

export interface CloudSyncStatus {
  schemaVersion: number;
  revision: number;
  updatedAt: string | null;
  hasSnapshot: boolean;
  counts: { dictionary: number; corrections: number; stylePacks: number };
}

export interface CloudSyncUiPreferences {
  locale?: string;
  fontScale?: string;
}

export interface CloudSyncRestoreResult {
  status: CloudSyncStatus;
  uiPreferences: CloudSyncUiPreferences;
}

// A browser preview has no native stores or authenticated cloud account.
function unavailable(): never {
  throw new Error('cloud_sync_unavailable');
}

export function cloudSyncStatus(): Promise<CloudSyncStatus> {
  return invokeOrMock('cloud_sync_status', undefined, unavailable);
}

export function cloudSyncUpload(
  baseRevision: number,
  uiPreferences: CloudSyncUiPreferences,
): Promise<CloudSyncStatus> {
  return invokeOrMock('cloud_sync_upload', { baseRevision, uiPreferences }, unavailable);
}

export function cloudSyncRestore(): Promise<CloudSyncRestoreResult> {
  return invokeOrMock('cloud_sync_restore', undefined, unavailable);
}

export function cloudSyncDelete(baseRevision: number): Promise<CloudSyncStatus> {
  return invokeOrMock('cloud_sync_delete', { baseRevision }, unavailable);
}
