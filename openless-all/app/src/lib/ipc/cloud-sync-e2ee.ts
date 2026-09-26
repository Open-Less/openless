import { invokeOrMock, isTauri } from './shared';

export const CLOUD_SYNC_E2EE_CONSENT_VERSION = 'encrypted-full-snapshot-v1';

export type EncryptedSyncState =
  | 'disabled'
  | 'sign_in_required'
  | 'unlock_required'
  | 'ready'
  | 'pending'
  | 'syncing'
  | 'conflict'
  | 'failed'
  | 'outcome_unknown'
  | 'recovery_required';

export interface EncryptedSyncStatus {
  sequence: string;
  enabled: boolean;
  authState: 'signed_out' | 'signed_in' | 'expired';
  keyState: 'locked' | 'unlocked';
  syncState: EncryptedSyncState;
  account: { githubId: string; login: string } | null;
  vaultId: string | null;
  keyId: string | null;
  localGeneration: string;
  lastSyncedLocalGeneration: string | null;
  remoteRevision: string | null;
  lastSuccessfulSyncAt: string | null;
  pendingOperationId: string | null;
  lastError: { code: string; retryAfterSeconds: number | null } | null;
  recoveryRequired: boolean;
  hasCloudSnapshot: boolean | null;
  taskId: string | null;
  serviceOrigin: string;
  backupRetentionDays: number | null;
  consentVersion: string | null;
}

export interface EnablePreparation {
  nextStep: 'create' | 'unlock' | 'restore_review' | 'ready';
  status: EncryptedSyncStatus;
}

export interface SyncConflictChoice {
  id: string;
  side: 'local' | 'cloud';
}
export interface RestorePreview {
  previewId: string;
  unconfirmedOperationId?: string | null;
  observedRevision: string;
  localGeneration: string;
  counts: Record<string, number>;
  deviceSettingsToReview: string[];
  conflicts: Array<{ id: string; kind: string; reason: string }>;
}

export interface EncryptedSyncEventScope {
  sequence: string;
  accountId: string | null;
  vaultId: string | null;
  taskId: string | null;
}
export interface EncryptedSyncEvent extends EncryptedSyncEventScope {
  status: EncryptedSyncStatus;
}
export interface EncryptedSyncConflictEvent extends EncryptedSyncEventScope {
  preview: RestorePreview;
}
export interface EncryptedSyncRestoreEvent extends EncryptedSyncEventScope {
  localGeneration: string;
  uiPreferences: Record<string, string>;
}

// Browser previews have neither a native vault nor a verified GitHub session.
// Never invent an enabled/unlocked/successful state in the preview adapter.
function unavailable(): never {
  throw Object.assign(new Error('cloud_sync_e2ee_unavailable'), {
    code: 'unsupported',
    details: { reason: 'service_unavailable' },
  });
}

export const cloudSyncE2eeStatus = (): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_status', undefined, unavailable);
export const cloudSyncE2eeClaimSetupPrompt = (): Promise<boolean> =>
  invokeOrMock('cloud_sync_e2ee_claim_setup_prompt', undefined, unavailable);
export const cloudSyncE2eePrepareEnable = (consentVersion: string): Promise<EnablePreparation> =>
  invokeOrMock('cloud_sync_e2ee_prepare_enable', { consentVersion }, unavailable);
export const cloudSyncE2eeCreate = (input: {
  password: string;
  passwordConfirmation: string;
  rememberKey: boolean;
  consentVersion: string;
  observedRevision: string;
}): Promise<EncryptedSyncStatus> => invokeOrMock('cloud_sync_e2ee_create', input, unavailable);
export const cloudSyncE2eeUnlock = (input: {
  password: string;
  rememberKey: boolean;
}): Promise<EncryptedSyncStatus> => invokeOrMock('cloud_sync_e2ee_unlock', input, unavailable);
export const cloudSyncE2eeLock = (): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_lock', undefined, unavailable);
export const cloudSyncE2eeSetEnabled = (enabled: boolean): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_set_enabled', { enabled }, unavailable);
export const cloudSyncE2eeSyncNow = (): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_sync_now', undefined, unavailable);
export const cloudSyncE2eeCancel = (taskId: string): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_cancel', { taskId }, unavailable);
export const cloudSyncE2eePreviewRestore = (observedRevision: string): Promise<RestorePreview> =>
  invokeOrMock('cloud_sync_e2ee_preview_restore', { observedRevision }, unavailable);
export const cloudSyncE2eeApplyRestore = (input: {
  previewId: string;
  mode: 'replace' | 'merge';
  conflictChoices: SyncConflictChoice[];
}): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_apply_restore', input, unavailable);
export const cloudSyncE2eeChangePassword = (input: {
  currentPassword: string;
  newPassword: string;
  confirmation: string;
  rememberKey: boolean;
}): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_change_password', input, unavailable);
export const cloudSyncE2eeDeleteRemote = (input: {
  expectedVaultId: string;
  observedRevision: string;
  confirmed: boolean;
}): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_delete_remote', input, unavailable);
export const cloudSyncE2eeSignOut = (): Promise<EncryptedSyncStatus> =>
  invokeOrMock('cloud_sync_e2ee_sign_out', undefined, unavailable);
export const cloudSyncE2eeGetUiPreferences = (): Promise<{
  locale?: string;
  fontScale?: string;
} | null> => invokeOrMock('cloud_sync_e2ee_get_ui_preferences', undefined, unavailable);

export async function mirrorEncryptedSyncUiPreferences(): Promise<void> {
  if (!isTauri) unavailable();
  const { flushEncryptedSyncUiPreferences } = await import('../encryptedSyncUiBridge');
  await flushEncryptedSyncUiPreferences();
}

export function syncSequence(value: unknown): bigint | null {
  if (typeof value !== 'string' || !/^(0|[1-9]\d{0,19})$/.test(value)) return null;
  const sequence = BigInt(value);
  return sequence <= 18446744073709551615n ? sequence : null;
}

export function encryptedSyncScope(status: EncryptedSyncStatus): string {
  return JSON.stringify([status.serviceOrigin, status.account?.githubId ?? null, status.vaultId]);
}

/** State events establish task transitions; conflict/restore events must match it. */
export function matchesEncryptedSyncEvent(
  status: EncryptedSyncStatus,
  event: EncryptedSyncEventScope,
  watermark: string,
  requireTask = true,
): boolean {
  const incoming = syncSequence(event.sequence);
  const previous = syncSequence(watermark);
  return (
    incoming !== null &&
    previous !== null &&
    incoming > previous &&
    event.accountId === (status.account?.githubId ?? null) &&
    event.vaultId === status.vaultId &&
    (!requireTask || event.taskId === status.taskId)
  );
}

// Only these value-free codes may influence UI text. Raw server messages,
// unknown detail fields and credential-bearing strings are never displayed.
const errorKeys = {
  sign_in_required: 'signIn',
  unlock_required: 'unlock',
  sync_documents_locked: 'unlock',
  service_unavailable: 'unavailable',
  unsupported_protocol: 'unavailable',
  weak_password: 'weakPassword',
  password_confirmation_mismatch: 'passwordMismatch',
  invalid_password_or_ciphertext: 'invalidPassword',
  secure_storage_denied: 'secureStorage',
  revision_conflict: 'changed',
  cloud_deleted: 'changed',
  conflict: 'reviewRequired',
  stale_preview: 'changed',
  sync_documents_source_changed: 'changed',
  account_changed: 'accountChanged',
  busy: 'busy',
  runtime_busy: 'busy',
  revision_rollback: 'invalidData',
  cancelled: 'cancelled',
  stale_task: 'changed',
  transport_failed: 'network',
  rate_limited: 'rateLimited',
  service_failed: 'network',
  outcome_unknown: 'outcomeUnknown',
  recovery_required: 'recovery',
  local_storage_unavailable: 'recovery',
  sync_journal_unavailable: 'recovery',
  sync_restore_rolled_back: 'rolledBack',
  payload_too_large: 'tooLarge',
  consent_required: 'reviewRequired',
  restore_review_required: 'reviewRequired',
  sync_conflict_choice_required: 'choicesRequired',
  invalid_response: 'invalidData',
  sync_documents_invalid: 'invalidData',
  sync_documents_invalid_reference: 'invalidData',
  sync_documents_duplicate_id: 'invalidData',
  sync_documents_excluded_field: 'invalidData',
  sync_documents_missing_tombstone: 'invalidData',
  sync_documents_unsupported: 'unavailable',
  sync_documents_capture_failed: 'localData',
  secure_random_unavailable: 'localData',
  crypto_worker_failed: 'localData',
} as const;
export type EncryptedSyncErrorKey = (typeof errorKeys)[keyof typeof errorKeys] | 'unknown';

export function encryptedSyncErrorKey(error: unknown): EncryptedSyncErrorKey {
  if (!error || typeof error !== 'object') return 'unknown';
  const value = error as { message?: unknown; code?: unknown; details?: { reason?: unknown } };
  for (const reason of [value.details?.reason, value.code, value.message]) {
    if (typeof reason === 'string' && Object.prototype.hasOwnProperty.call(errorKeys, reason)) {
      return errorKeys[reason as keyof typeof errorKeys];
    }
  }
  return 'unknown';
}

/** Advisory form check only; Core owns normalization and the complete policy. */
export function syncPasswordMeetsBasicRequirements(password: string): boolean {
  const normalized = password.normalize('NFC');
  const length = Array.from(normalized).length;
  return (
    length >= 12 &&
    length <= 128 &&
    /[a-z]/.test(normalized) &&
    /[A-Z]/.test(normalized) &&
    /[0-9]/.test(normalized)
  );
}
