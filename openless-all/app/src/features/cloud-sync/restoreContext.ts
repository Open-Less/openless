import {
  encryptedSyncScope,
  type EncryptedSyncStatus,
  type RestorePreview,
} from '../../api/cloud-sync';

export function matchesRestorePreview(
  preview: RestorePreview,
  status: EncryptedSyncStatus | null,
  scope: string,
): boolean {
  return (
    !!status &&
    encryptedSyncScope(status) === scope &&
    preview.observedRevision === status.remoteRevision &&
    preview.localGeneration === status.localGeneration
  );
}
