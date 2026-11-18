import type {
  ConsentScope,
  EnablePreparation,
  EncryptedSyncStatus,
  RestorePreview,
} from '../lib/ipc/cloud-sync-e2ee';
import { getRuntimeEnvironment } from '../runtime/environment';

type Scenario = 'first-time' | 'reconsent';
type Failure = 'prepare' | 'create';

export function getCloudSyncPreviewScenario(): Scenario | undefined {
  const { host, isMobile } = getRuntimeEnvironment();
  if (host !== 'preview' || isMobile) return undefined;
  const location = globalThis.window?.location;
  const dev = import.meta.env?.DEV;
  const loopback =
    location &&
    (['localhost', '127.0.0.1', '[::1]'].includes(location.hostname) ||
      (dev && location.hostname.endsWith('.orca.localhost')));
  if (!loopback) return undefined;
  const selected = new URLSearchParams(location.search).get('cloudSync');
  if (selected === 'first-time' || selected === 'reconsent') return selected;
  if (selected === null && dev) return 'first-time';
}

function fail(reason: string): never {
  throw { details: { reason } };
}

function createCloudSyncPreview(
  scenario: Scenario,
  consentVersion: string,
  meetsPasswordRequirements: (password: string) => boolean,
  failOnce?: Failure,
) {
  const existing = scenario === 'reconsent';
  const account = { githubId: 'preview-account', login: 'openless-preview' };
  let state: EncryptedSyncStatus = {
    sequence: '1',
    enabled: existing,
    authState: 'signed_in',
    keyState: existing ? 'unlocked' : 'locked',
    syncState: existing ? 'ready' : 'disabled',
    account,
    vaultId: existing ? 'preview-vault' : null,
    keyId: existing ? 'preview-key' : null,
    localGeneration: '1',
    lastSyncedLocalGeneration: existing ? '1' : null,
    remoteRevision: existing ? '7' : null,
    lastSuccessfulSyncAt: null,
    pendingOperationId: null,
    lastError: null,
    recoveryRequired: false,
    hasCloudSnapshot: existing ? true : null,
    taskId: null,
    serviceOrigin: 'https://cloud-sync.preview.invalid',
    backupRetentionDays: 7,
    consentVersion: existing ? 'encrypted-full-snapshot-v1' : null,
  };
  const status = (): EncryptedSyncStatus => ({
    ...state,
    account: state.account && { ...state.account },
  });
  const update = (patch: Partial<EncryptedSyncStatus>): EncryptedSyncStatus => {
    state = { ...state, ...patch, sequence: String(Number(state.sequence) + 1) };
    return status();
  };
  const checkAuth = () => {
    if (state.authState !== 'signed_in') fail('sign_in_required');
  };
  const checkConsent = () => {
    checkAuth();
    if (state.consentVersion !== consentVersion) fail('consent_required');
  };
  const checkRevision = (revision: string) => {
    if (state.remoteRevision !== revision) fail('revision_conflict');
  };
  let pending = false;
  const settle = async <T>(step: Failure, operation: () => T): Promise<T> => {
    if (pending) fail('busy');
    pending = true;
    try {
      await new Promise<void>((resolve) => setTimeout(resolve, 220));
      if (failOnce === step) {
        failOnce = undefined;
        fail('transport_failed');
      }
      return operation();
    } finally {
      pending = false;
    }
  };
  const previewRestore = (revision: string): RestorePreview => {
    checkConsent();
    checkRevision(revision);
    if (!state.hasCloudSnapshot || state.keyState !== 'unlocked') fail('unlock_required');
    return {
      previewId: `${state.vaultId}-${revision}-${state.localGeneration}`,
      observedRevision: revision,
      localGeneration: state.localGeneration,
      counts: { preferences: 1 },
      deviceSettingsToReview: [],
      conflicts: [],
    };
  };

  return {
    status,
    refreshBackupMetadata: () => {
      checkAuth();
      if (!state.consentVersion || state.consentVersion === consentVersion) fail('consent_required');
      return status();
    },
    signOut: () =>
      update({
        account: null,
        enabled: false,
        authState: 'signed_out',
        keyState: 'locked',
        syncState: 'sign_in_required',
      }),
    prepare: (version: string, scope?: ConsentScope): Promise<EnablePreparation> =>
      settle<EnablePreparation>('prepare', () => {
        checkAuth();
        if (version !== consentVersion) fail('consent_required');
        if (
          scope &&
          (scope.serviceOrigin !== state.serviceOrigin ||
            scope.accountId !== state.account?.githubId ||
            scope.vaultId !== state.vaultId)
        )
          fail('account_changed');
        const nextStep = !state.hasCloudSnapshot
          ? 'create'
          : state.keyState === 'locked'
            ? 'unlock'
            : 'ready';
        return {
          nextStep,
          status: update({
            consentVersion: version,
            hasCloudSnapshot: state.hasCloudSnapshot ?? false,
            remoteRevision: state.remoteRevision ?? '0',
          }),
        };
      }),
    create: (input: {
      password: string;
      passwordConfirmation: string;
      consentVersion: string;
      observedRevision: string;
    }) =>
      settle('create', () => {
        checkConsent();
        checkRevision(input.observedRevision);
        if (input.consentVersion !== consentVersion) fail('consent_required');
        if (state.hasCloudSnapshot !== false) fail('revision_conflict');
        if (!meetsPasswordRequirements(input.password)) fail('weak_password');
        if (input.password.normalize('NFC') !== input.passwordConfirmation.normalize('NFC'))
          fail('password_confirmation_mismatch');
        return update({
          enabled: true,
          keyState: 'unlocked',
          syncState: 'ready',
          vaultId: 'preview-vault',
          keyId: 'preview-key',
          hasCloudSnapshot: true,
          remoteRevision: '1',
          lastSyncedLocalGeneration: state.localGeneration,
        });
      }),
    unlock: (password: string) => {
      checkConsent();
      if (!password || !state.hasCloudSnapshot) fail('invalid_password_or_ciphertext');
      return update({ keyState: 'unlocked', syncState: state.enabled ? 'ready' : 'disabled' });
    },
    lock: () => {
      checkAuth();
      return update({ keyState: 'locked', syncState: state.enabled ? 'unlock_required' : 'disabled' });
    },
    setEnabled: (enabled: boolean) => {
      if (enabled) {
        checkConsent();
        if (state.keyState !== 'unlocked' || !state.hasCloudSnapshot) fail('unlock_required');
      }
      return update({ enabled, syncState: enabled ? 'ready' : 'disabled' });
    },
    sync: () => {
      checkConsent();
      if (!state.enabled || state.keyState !== 'unlocked') fail('unlock_required');
      return update({ lastSuccessfulSyncAt: new Date().toISOString() });
    },
    previewRestore,
    applyRestore: (previewId: string) => {
      if (state.remoteRevision === null || previewRestore(state.remoteRevision).previewId !== previewId)
        fail('stale_preview');
      return update({ localGeneration: String(Number(state.localGeneration) + 1) });
    },
    changePassword: (current: string, next: string, confirmation: string) => {
      checkConsent();
      if (!current || state.keyState !== 'unlocked') fail('invalid_password_or_ciphertext');
      if (!meetsPasswordRequirements(next)) fail('weak_password');
      if (next.normalize('NFC') !== confirmation.normalize('NFC'))
        fail('password_confirmation_mismatch');
      return update({ keyId: 'preview-rotated-key' });
    },
    deleteRemote: (id: string, revision: string, confirmed: boolean) => {
      checkAuth();
      checkRevision(revision);
      if (!confirmed || id !== state.vaultId) fail('stale_preview');
      return update({
        enabled: false,
        keyState: 'locked',
        syncState: 'disabled',
        vaultId: null,
        keyId: null,
        hasCloudSnapshot: false,
        lastSyncedLocalGeneration: null,
        lastSuccessfulSyncAt: null,
      });
    },
  };
}

let preview: { scenario: Scenario; adapter: ReturnType<typeof createCloudSyncPreview> } | undefined;

export function getCloudSyncPreview(
  consentVersion: string,
  meetsPasswordRequirements: (password: string) => boolean,
) {
  const scenario = getCloudSyncPreviewScenario();
  if (!scenario) return undefined;
  if (preview?.scenario !== scenario) {
    const failure = new URLSearchParams(globalThis.window?.location?.search).get('cloudSyncFailure');
    preview = {
      scenario,
      adapter: createCloudSyncPreview(
        scenario,
        consentVersion,
        meetsPasswordRequirements,
        failure === 'prepare' || failure === 'create' ? failure : undefined,
      ),
    };
  }
  // Scenario state belongs to this document only; no real credentials or backup data are stored.
  return preview?.adapter;
}
