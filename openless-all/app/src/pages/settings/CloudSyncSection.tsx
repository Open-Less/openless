import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { GithubLoginModal } from '../../components/GithubLoginModal';
import { Modal } from '../../components/ui/Modal';
import { Btn, Card } from '../_atoms';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { marketplaceAuthStatus } from '../../lib/ipc';
import { isTauri } from '../../lib/ipc/shared';
import {
  CLOUD_SYNC_E2EE_CONSENT_VERSION as CONSENT_VERSION,
  cloudSyncE2eeStatus,
  cloudSyncE2eePrepareEnable,
  cloudSyncE2eeCreate,
  cloudSyncE2eeUnlock,
  cloudSyncE2eeLock,
  cloudSyncE2eeSetEnabled,
  cloudSyncE2eeSyncNow,
  cloudSyncE2eeCancel,
  cloudSyncE2eePreviewRestore,
  cloudSyncE2eeApplyRestore,
  cloudSyncE2eeChangePassword,
  cloudSyncE2eeDeleteRemote,
  cloudSyncE2eeSignOut,
  mirrorEncryptedSyncUiPreferences,
  encryptedSyncScope,
  encryptedSyncErrorKey,
  matchesEncryptedSyncEvent,
  syncSequence,
  syncPasswordMeetsBasicRequirements,
  type EncryptedSyncStatus,
  type EnablePreparation,
  type RestorePreview,
  type EncryptedSyncEvent,
  type EncryptedSyncConflictEvent,
  type EncryptedSyncRestoreEvent,
  type SyncConflictChoice,
} from '../../lib/ipc/cloud-sync-e2ee';

type Intent = 'enable' | 'unlock' | 'restore';
type Dialog =
  | { kind: 'consent'; intent: Intent }
  | { kind: 'protocol'; intent: Intent }
  | { kind: 'create'; observedRevision: string }
  | { kind: 'unlock'; intent: Intent }
  | { kind: 'password' }
  | { kind: 'restore'; preview: RestorePreview; enableAfter: boolean }
  | { kind: 'disable' }
  | { kind: 'delete'; vaultId: string; revision: string };
type Notice = { key: string; error?: boolean } | null;
type PasswordSubmission = {
  password: string;
  confirmation: string;
  currentPassword: string;
  rememberKey: boolean;
};
const categoryKeys = [
  'preferences',
  'ui_preferences',
  'channels',
  'provider_credentials',
  'dictionary',
  'vocabulary_presets',
  'corrections',
  'style_packs',
  'history',
  'activity',
  'device_profile',
] as const;
function categoryKey(kind: string): string {
  return categoryKeys.some((key) => key === kind) ? kind : 'other';
}
function localError(reason: string): unknown {
  return { details: { reason } };
}
async function prepareEncryptedSync(): Promise<EnablePreparation> {
  await mirrorEncryptedSyncUiPreferences();
  return cloudSyncE2eePrepareEnable(CONSENT_VERSION);
}

export function CloudSyncSection() {
  const { t, i18n } = useTranslation();
  const { prefs, updatePrefs, refresh } = useHotkeySettings();
  const [status, setStatus] = useState<EncryptedSyncStatus | null>(null);
  const [authSignedIn, setAuthSignedIn] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [cancelPending, setCancelPending] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [showLogin, setShowLogin] = useState(false);
  const current = useRef<EncryptedSyncStatus | null>(null);
  const watermark = useRef('0');
  const alive = useRef(false);
  const loadSequence = useRef(0);
  const actionSequence = useRef(0);
  const activeAction = useRef<number | null>(null);
  const loginHint = prefs?.marketplaceDevLogin?.trim() ?? '';

  const showError = (error: unknown) => {
    if (alive.current) setNotice({ key: `errors.${encryptedSyncErrorKey(error)}`, error: true });
  };
  const acceptStatus = (next: EncryptedSyncStatus): boolean => {
    if (!alive.current) return false;
    const sequence = syncSequence(next.sequence);
    const previous = syncSequence(watermark.current);
    if (sequence === null || previous === null || sequence < previous) return false;
    const scopeChanged =
      current.current && encryptedSyncScope(current.current) !== encryptedSyncScope(next);
    current.current = next;
    watermark.current = next.sequence;
    setStatus(next);
    if (scopeChanged) setDialog(null);
    return true;
  };
  const load = async () => {
    const request = ++loadSequence.current;
    setLoading(true);
    try {
      const [auth, next] = await Promise.all([marketplaceAuthStatus(), cloudSyncE2eeStatus()]);
      if (!alive.current || request !== loadSequence.current) return;
      setAuthSignedIn(auth.signedIn);
      acceptStatus(next);
    } catch (error) {
      if (alive.current && request === loadSequence.current) showError(error);
    } finally {
      if (alive.current && request === loadSequence.current) setLoading(false);
    }
  };
  const loadRef = useRef(load);
  loadRef.current = load;
  const eventsRef = useRef({
    state: (_event: EncryptedSyncEvent) => {},
    conflict: (_event: EncryptedSyncConflictEvent) => {},
    restored: (_event: EncryptedSyncRestoreEvent) => {},
  });
  eventsRef.current = {
    state(event) {
      if (!alive.current) return;
      const sequence = syncSequence(event.sequence);
      if (sequence === null || sequence <= (syncSequence(watermark.current) ?? 0n)) return;
      const snapshot = current.current;
      if (
        !snapshot ||
        !matchesEncryptedSyncEvent(snapshot, event, watermark.current, false) ||
        encryptedSyncScope(snapshot) !== encryptedSyncScope(event.status)
      ) {
        // Never take a different account/vault from an unsolicited event.
        void loadRef.current();
        return;
      }
      if (
        event.sequence !== event.status.sequence ||
        event.accountId !== (event.status.account?.githubId ?? null) ||
        event.vaultId !== event.status.vaultId ||
        event.taskId !== event.status.taskId
      )
        return;
      acceptStatus(event.status);
    },
    conflict(event) {
      const snapshot = current.current;
      if (
        !alive.current ||
        !snapshot ||
        !matchesEncryptedSyncEvent(snapshot, event, watermark.current)
      )
        return;
      watermark.current = event.sequence;
      setDialog({ kind: 'restore', preview: event.preview, enableAfter: false });
      setNotice({ key: 'states.conflict' });
    },
    restored(event) {
      const snapshot = current.current;
      if (
        !alive.current ||
        !snapshot ||
        !matchesEncryptedSyncEvent(snapshot, event, watermark.current)
      )
        return;
      watermark.current = event.sequence;
      setDialog(null);
      setNotice({ key: 'restored' });
      // UI-only preferences are applied by the central native restore bridge.
      void refresh().catch(showError);
      void loadRef.current();
    },
  };

  useEffect(() => {
    alive.current = true;
    let disposed = false;
    const unlisten: Array<() => void> = [];
    const uiPersistenceFailed = (event: Event) => {
      if (disposed) return;
      // Render only allow-listed stable codes; preserve a denied-vault or busy
      // cause instead of replacing every failure with a generic storage error.
      showError((event as CustomEvent<unknown>).detail ?? localError('local_storage_unavailable'));
      void loadRef.current();
    };
    window.addEventListener('openless:sync-ui-persistence-failed', uiPersistenceFailed);
    void (async () => {
      try {
        if (isTauri) {
          const { listen } = await import('@tauri-apps/api/event');
          for (const [name, kind] of [
            ['cloud-sync-e2ee:state', 'state'],
            ['cloud-sync-e2ee:conflict', 'conflict'],
            ['cloud-sync-e2ee:restored', 'restored'],
          ] as const) {
            const stop = await listen(name, (event) => {
              if (disposed) return;
              if (kind === 'state') eventsRef.current.state(event.payload as EncryptedSyncEvent);
              else if (kind === 'conflict')
                eventsRef.current.conflict(event.payload as EncryptedSyncConflictEvent);
              else eventsRef.current.restored(event.payload as EncryptedSyncRestoreEvent);
            });
            if (disposed) {
              stop();
              return;
            }
            unlisten.push(stop);
          }
        }
      } catch (error) {
        if (!disposed) showError(error);
      }
      if (!disposed) await loadRef.current();
    })();
    return () => {
      disposed = true;
      alive.current = false;
      loadSequence.current += 1;
      actionSequence.current += 1;
      unlisten.forEach((stop) => stop());
      window.removeEventListener('openless:sync-ui-persistence-failed', uiPersistenceFailed);
    };
  }, []);
  useEffect(() => {
    if (alive.current) void loadRef.current();
  }, [loginHint]);

  const perform = async (work: (valid: () => boolean) => Promise<void>) => {
    if (activeAction.current !== null || current.current?.syncState === 'syncing') return;
    const action = ++actionSequence.current;
    activeAction.current = action;
    setBusy(true);
    setNotice(null);
    const valid = () => alive.current && actionSequence.current === action;
    try {
      await work(valid);
    } catch (error) {
      if (valid()) {
        showError(error);
        if (encryptedSyncErrorKey(error) === 'outcomeUnknown') setDialog(null);
        void loadRef.current();
      }
    } finally {
      if (activeAction.current === action) activeAction.current = null;
      if (alive.current) setBusy(false);
    }
  };
  const observedRevision = (): string => {
    const revision = current.current?.remoteRevision;
    if (revision == null) throw localError('stale_preview');
    return revision;
  };
  const previewRestore = async (enableAfter: boolean, valid: () => boolean) => {
    const scope = current.current ? encryptedSyncScope(current.current) : null;
    const preview = await cloudSyncE2eePreviewRestore(observedRevision());
    if (!valid()) return;
    const next = await cloudSyncE2eeStatus();
    if (!valid() || (current.current && encryptedSyncScope(current.current) !== scope)) return;
    if (!acceptStatus(next)) return;
    if (
      preview.observedRevision !== next.remoteRevision ||
      preview.localGeneration !== next.localGeneration
    )
      throw localError('stale_preview');
    setDialog({ kind: 'restore', preview, enableAfter });
  };
  const routePreparation = async (
    prepared: EnablePreparation,
    intent: Intent,
    valid: () => boolean,
  ) => {
    if (!valid() || !acceptStatus(prepared.status)) return;
    if (prepared.nextStep === 'create') {
      if (
        intent !== 'enable' ||
        prepared.status.hasCloudSnapshot !== false ||
        prepared.status.remoteRevision === null
      )
        throw localError('invalid_response');
      setDialog({ kind: 'create', observedRevision: prepared.status.remoteRevision });
    } else if (prepared.nextStep === 'unlock') {
      setDialog({ kind: 'unlock', intent });
    } else if (prepared.nextStep === 'restore_review') {
      await previewRestore(intent === 'enable', valid);
    } else if (intent === 'restore') {
      await previewRestore(false, valid);
    } else {
      if (intent === 'enable') {
        const next = await cloudSyncE2eeSetEnabled(true);
        if (!valid()) return;
        acceptStatus(next);
      }
      if (valid()) {
        setDialog(null);
        setNotice({ key: 'done' });
      }
    }
  };
  const begin = (intent: Intent) => {
    setNotice(null);
    if (intent === 'enable' || current.current?.consentVersion !== CONSENT_VERSION) {
      setDialog({ kind: 'consent', intent });
    } else {
      void perform(async (valid) => routePreparation(await prepareEncryptedSync(), intent, valid));
    }
  };
  const simple = (request: () => Promise<EncryptedSyncStatus>, signOut = false) =>
    void perform(async (valid) => {
      const next = await request();
      if (!valid() || !acceptStatus(next)) return;
      if (signOut) {
        setAuthSignedIn(false);
        await refresh();
      }
      if (next.syncState === 'conflict') {
        if (valid()) setNotice({ key: 'states.conflict' });
        await previewRestore(false, valid);
        return;
      }
      if (valid()) {
        setDialog(null);
        setNotice({ key: 'done' });
      }
    });
  const submitPassword = (value: PasswordSubmission) => {
    const active = dialog;
    if (!active || !['create', 'unlock', 'password'].includes(active.kind)) return;
    void perform(async (valid) => {
      if (active.kind === 'create') {
        const next = await cloudSyncE2eeCreate({
          password: value.password,
          passwordConfirmation: value.confirmation,
          rememberKey: value.rememberKey,
          consentVersion: CONSENT_VERSION,
          observedRevision: active.observedRevision,
        });
        if (valid() && acceptStatus(next)) {
          setDialog(null);
          setNotice({ key: 'done' });
        }
      } else if (active.kind === 'unlock') {
        const next = await cloudSyncE2eeUnlock({
          password: value.password,
          rememberKey: value.rememberKey,
        });
        if (!valid() || !acceptStatus(next)) return;
        await routePreparation(await prepareEncryptedSync(), active.intent, valid);
      } else if (active.kind === 'password') {
        const next = await cloudSyncE2eeChangePassword({
          currentPassword: value.currentPassword,
          newPassword: value.password,
          confirmation: value.confirmation,
          rememberKey: value.rememberKey,
        });
        if (valid() && acceptStatus(next)) {
          setDialog(null);
          setNotice({ key: 'done' });
        }
      }
    });
  };
  const applyRestore = (mode: 'replace' | 'merge', conflictChoices: SyncConflictChoice[]) => {
    if (dialog?.kind !== 'restore') return;
    const { preview, enableAfter } = dialog;
    void perform(async (valid) => {
      const snapshot = current.current;
      if (
        !snapshot ||
        preview.observedRevision !== snapshot.remoteRevision ||
        preview.localGeneration !== snapshot.localGeneration
      )
        throw localError('stale_preview');
      const next = await cloudSyncE2eeApplyRestore({
        previewId: preview.previewId,
        mode,
        conflictChoices,
      });
      if (!valid() || !acceptStatus(next)) return;
      if (enableAfter && !next.enabled) {
        const enabled = await cloudSyncE2eeSetEnabled(true);
        if (!valid()) return;
        acceptStatus(enabled);
      }
      await refresh();
      if (valid()) {
        setDialog(null);
        setNotice({ key: 'restored' });
      }
    });
  };
  const cancelTask = async () => {
    const task = current.current?.taskId;
    if (!task || cancelPending) return;
    setCancelPending(true);
    try {
      const next = await cloudSyncE2eeCancel(task);
      if (alive.current && acceptStatus(next)) setNotice({ key: 'cancelRequested' });
    } catch (error) {
      showError(error);
    } finally {
      if (alive.current) setCancelPending(false);
    }
  };

  const signedIn =
    status?.authState !== 'expired' &&
    status?.syncState !== 'sign_in_required' &&
    (status?.authState === 'signed_in' || authSignedIn);
  const working = busy || status?.syncState === 'syncing';
  const unlocked = status?.keyState === 'unlocked';
  const available = status !== null;
  const lastSync = status?.lastSuccessfulSyncAt ? new Date(status.lastSuccessfulSyncAt) : null;
  const lastError = status?.lastError ? encryptedSyncErrorKey(status.lastError) : null;
  const dialogTitle =
    dialog?.kind === 'create'
      ? 'createTitle'
      : dialog?.kind === 'unlock'
        ? 'unlockTitle'
        : dialog?.kind === 'password'
          ? 'passwordTitle'
          : `${dialog?.kind ?? 'consent'}Title`;
  const closeDialog = () => {
    if (!working) {
      setDialog(null);
      setNotice(null);
    }
  };
  const taskControls = status?.taskId ? (
    <Btn disabled={cancelPending} onClick={() => void cancelTask()}>
      {t('cloudSyncE2ee.cancelTask')}
    </Btn>
  ) : null;

  return (
    <Card>
      <section className="ol-cloud-sync" aria-labelledby="cloud-sync-e2ee-title">
        <header>
          <span className="ol-cloud-sync-icon">
            <Icon name="cloud" size={21} />
          </span>
          <div>
            <h3 id="cloud-sync-e2ee-title">{t('cloudSyncE2ee.title')}</h3>
            <p>{t('cloudSyncE2ee.description')}</p>
          </div>
          <button
            type="button"
            className="ol-settings-back"
            onClick={() => void load()}
            disabled={loading}
            aria-label={t('cloudSyncE2ee.refresh')}
            title={t('cloudSyncE2ee.refresh')}
          >
            <Icon name="refresh" size={16} />
          </button>
        </header>
        <label className="ol-cloud-sync-account" style={{ justifyContent: 'space-between' }}>
          <strong>{t('cloudSyncE2ee.enable')}</strong>
          <input
            type="checkbox"
            role="switch"
            checked={status?.enabled ?? false}
            disabled={!available || !signedIn || working || loading || status?.recoveryRequired}
            onChange={(event) =>
              event.currentTarget.checked ? begin('enable') : setDialog({ kind: 'disable' })
            }
          />
        </label>
        {loading && <p role="status">{t('cloudSyncE2ee.loading')}</p>}
        {!loading && !signedIn && (
          <Btn
            variant="primary"
            icon="user"
            disabled={!isTauri || !available || working}
            onClick={() => setShowLogin(true)}
          >
            {t('cloudSyncE2ee.signIn')}
          </Btn>
        )}
        {signedIn && (
          <div className="ol-cloud-sync-account">
            <Icon name="user" size={16} />
            <span>{t('cloudSyncE2ee.account')}</span>
            <strong dir="auto">
              {status?.account?.login
                ? `@${status.account.login}`
                : loginHint
                  ? `@${loginHint}`
                  : 'GitHub'}
            </strong>
            <Btn size="sm" disabled={working} onClick={() => simple(cloudSyncE2eeSignOut, true)}>
              {t('cloudSyncE2ee.signOut')}
            </Btn>
          </div>
        )}
        {status && (
          <div className="ol-cloud-sync-backup" role="status" aria-live="polite">
            <strong>{t(`cloudSyncE2ee.states.${status.syncState}`)}</strong>
            <p>{t(unlocked ? 'cloudSyncE2ee.keyUnlocked' : 'cloudSyncE2ee.keyLocked')}</p>
            <p>
              {t(
                status.hasCloudSnapshot === true
                  ? 'cloudSyncE2ee.hasSnapshot'
                  : status.hasCloudSnapshot === false
                    ? 'cloudSyncE2ee.noSnapshot'
                    : 'cloudSyncE2ee.snapshotUnknown',
              )}
            </p>
            {lastSync && Number.isFinite(lastSync.getTime()) && (
              <small>
                {t('cloudSyncE2ee.lastSync', {
                  time: lastSync.toLocaleString(i18n.resolvedLanguage ?? i18n.language),
                })}
              </small>
            )}
          </div>
        )}
        {signedIn && available && (
          <div className="ol-cloud-sync-actions">
            {!unlocked && status?.hasCloudSnapshot && (
              <Btn disabled={working} onClick={() => begin('unlock')}>
                {t('cloudSyncE2ee.unlock')}
              </Btn>
            )}
            <Btn
              variant="primary"
              disabled={
                working ||
                status?.recoveryRequired ||
                (!status?.pendingOperationId &&
                  (!status?.enabled || !unlocked || status?.syncState === 'conflict'))
              }
              onClick={() => simple(cloudSyncE2eeSyncNow)}
            >
              {t(
                status?.pendingOperationId ? 'cloudSyncE2ee.checkPending' : 'cloudSyncE2ee.syncNow',
              )}
            </Btn>
            <Btn
              disabled={!status?.hasCloudSnapshot || working || status?.recoveryRequired}
              onClick={() => begin('restore')}
            >
              {t('cloudSyncE2ee.restore')}
            </Btn>
            {unlocked && (
              <>
                <Btn disabled={working} onClick={() => simple(cloudSyncE2eeLock)}>
                  {t('cloudSyncE2ee.lock')}
                </Btn>
                <Btn
                  disabled={working || status?.recoveryRequired}
                  onClick={() => setDialog({ kind: 'password' })}
                >
                  {t('cloudSyncE2ee.changePassword')}
                </Btn>
              </>
            )}
            <button
              type="button"
              className="ol-vocab-delete-selected"
              disabled={
                !status?.hasCloudSnapshot ||
                !status.vaultId ||
                status.remoteRevision === null ||
                working
              }
              onClick={() => {
                if (status?.vaultId && status.remoteRevision !== null)
                  setDialog({
                    kind: 'delete',
                    vaultId: status.vaultId,
                    revision: status.remoteRevision,
                  });
              }}
            >
              <Icon name="trash" size={14} />
              {t('cloudSyncE2ee.delete')}
            </button>
          </div>
        )}
        {working && (
          <div className="ol-cloud-sync-actions">
            <span role="status">{t('cloudSyncE2ee.working')}</span>
            {taskControls}
          </div>
        )}
        {notice ? (
          <p
            role={notice.error ? 'alert' : 'status'}
            className={notice.error ? 'ol-cloud-sync-error' : undefined}
          >
            {t(`cloudSyncE2ee.${notice.key}`)}
          </p>
        ) : (
          lastError && (
            <p role="alert" className="ol-cloud-sync-error">
              {t(`cloudSyncE2ee.errors.${lastError}`)}
            </p>
          )
        )}
        {status?.lastError?.retryAfterSeconds != null &&
          Number.isSafeInteger(status.lastError.retryAfterSeconds) &&
          status.lastError.retryAfterSeconds > 0 &&
          status.lastError.retryAfterSeconds <= 86400 && (
            <p>{t('cloudSyncE2ee.retryAfter', { seconds: status.lastError.retryAfterSeconds })}</p>
          )}
        <p className="ol-cloud-sync-scope">{t('cloudSyncE2ee.passwordWarning')}</p>
      </section>
      {showLogin && (
        <GithubLoginModal
          onClose={() => setShowLogin(false)}
          onSuccess={(login) => {
            setShowLogin(false);
            void updatePrefs((value) => ({ ...value, marketplaceDevLogin: login }))
              .then(() => loadRef.current())
              .catch(showError);
          }}
        />
      )}
      {dialog && (
        <SyncDialog
          key={dialog.kind}
          title={t(`cloudSyncE2ee.${dialogTitle}`)}
          busy={working}
          onClose={closeDialog}
        >
          {notice?.error && (
            <p role="alert" style={{ color: 'var(--ol-err)' }}>
              {t(`cloudSyncE2ee.${notice.key}`)}
            </p>
          )}
          {dialog.kind === 'consent' && (
            <ConsentForm
              busy={working}
              onConfirm={() => {
                setNotice(null);
                setDialog({ kind: 'protocol', intent: dialog.intent });
              }}
            />
          )}
          {dialog.kind === 'protocol' && (
            <ProtocolWarning
              busy={working}
              onBack={() => {
                setNotice(null);
                setDialog({ kind: 'consent', intent: dialog.intent });
              }}
              onConfirm={() =>
                void perform(async (valid) =>
                  routePreparation(await prepareEncryptedSync(), dialog.intent, valid),
                )
              }
            />
          )}
          {(dialog.kind === 'create' || dialog.kind === 'unlock' || dialog.kind === 'password') && (
            <PasswordForm
              key={dialog.kind}
              mode={dialog.kind}
              busy={working}
              onSubmit={submitPassword}
            />
          )}
          {dialog.kind === 'restore' && (
            <RestoreReview
              key={dialog.preview.previewId}
              preview={dialog.preview}
              busy={working}
              stale={
                dialog.preview.observedRevision !== status?.remoteRevision ||
                dialog.preview.localGeneration !== status?.localGeneration
              }
              onApply={applyRestore}
            />
          )}
          {dialog.kind === 'disable' && (
            <>
              <p>{t('cloudSyncE2ee.disableDescription')}</p>
              <Btn
                variant="primary"
                disabled={working}
                onClick={() => simple(() => cloudSyncE2eeSetEnabled(false))}
              >
                {t('cloudSyncE2ee.confirmDisable')}
              </Btn>
            </>
          )}
          {dialog.kind === 'delete' && (
            <DeleteConfirmation
              busy={working}
              backupRetentionDays={status?.backupRetentionDays ?? null}
              onConfirm={() =>
                simple(() =>
                  cloudSyncE2eeDeleteRemote({
                    expectedVaultId: dialog.vaultId,
                    observedRevision: dialog.revision,
                    confirmed: true,
                  }),
                )
              }
            />
          )}
          {working && (
            <div className="ol-cloud-sync-actions">
              <span role="status">{t('cloudSyncE2ee.working')}</span>
              {taskControls}
            </div>
          )}
        </SyncDialog>
      )}
    </Card>
  );
}

function ConsentForm({ busy, onConfirm }: { busy: boolean; onConfirm: () => void }) {
  const { t } = useTranslation();
  const [consented, setConsented] = useState(false);
  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <p>{t('cloudSyncE2ee.consentDescription')}</p>
      <details>
        <summary style={{ cursor: 'pointer', fontWeight: 600 }}>
          {t('cloudSyncE2ee.scopeSummary')}
        </summary>
        <ul style={{ paddingLeft: 20, lineHeight: 1.75 }}>
          {['preferences', 'credentials', 'personal', 'history', 'device'].map((key) => (
            <li key={key}>{t(`cloudSyncE2ee.scope.${key}`)}</li>
          ))}
        </ul>
        <p>{t('cloudSyncE2ee.scope.excluded')}</p>
      </details>
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 9 }}>
        <input
          type="checkbox"
          checked={consented}
          disabled={busy}
          onChange={(event) => setConsented(event.currentTarget.checked)}
        />
        {t('cloudSyncE2ee.consentCheck')}
      </label>
      <Btn variant="primary" disabled={!consented || busy} onClick={onConfirm}>
        {t('cloudSyncE2ee.continue')}
      </Btn>
    </div>
  );
}

function ProtocolWarning({
  busy,
  onConfirm,
  onBack,
}: {
  busy: boolean;
  onConfirm: () => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const [acknowledged, setAcknowledged] = useState(false);
  return (
    <div style={{ display: 'grid', gap: 18 }}>
      <p style={{ margin: 0, color: 'var(--ol-ink-3)', lineHeight: 1.65 }}>
        {t('cloudSyncE2ee.protocolIntro')}
      </p>
      <div
        style={{
          display: 'grid',
          gap: 16,
          padding: 18,
          border: '1px solid var(--ol-line)',
          borderRadius: 14,
          background: 'var(--ol-surface-2)',
        }}
      >
        {['Password', 'Encryption', 'Excluded'].map((section) => (
          <section key={section}>
            <h4 style={{ fontSize: 14, margin: '0 0 6px', color: 'var(--ol-ink)' }}>
              {t(`cloudSyncE2ee.protocol${section}Title`)}
            </h4>
            <p style={{ margin: 0, lineHeight: 1.7, color: 'var(--ol-ink-3)' }}>
              {t(`cloudSyncE2ee.protocol${section}`)}
            </p>
          </section>
        ))}
      </div>
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 9, lineHeight: 1.6 }}>
        <input
          type="checkbox"
          checked={acknowledged}
          disabled={busy}
          onChange={(event) => setAcknowledged(event.currentTarget.checked)}
        />
        {t('cloudSyncE2ee.protocolCheck')}
      </label>
      <div className="ol-cloud-sync-actions" style={{ justifyContent: 'flex-end' }}>
        <Btn disabled={busy} onClick={onBack}>
          {t('cloudSyncE2ee.protocolBack')}
        </Btn>
        <Btn variant="primary" disabled={!acknowledged || busy} onClick={onConfirm}>
          {t('cloudSyncE2ee.protocolConfirm')}
        </Btn>
      </div>
    </div>
  );
}

function PasswordForm({
  mode,
  busy,
  onSubmit,
}: {
  mode: 'create' | 'unlock' | 'password';
  busy: boolean;
  onSubmit: (value: PasswordSubmission) => void;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [rememberKey, setRememberKey] = useState(true);
  const composing = useRef(false);
  const creating = mode !== 'unlock';
  const valid = creating
    ? syncPasswordMeetsBasicRequirements(password) &&
      password.normalize('NFC') === confirmation.normalize('NFC') &&
      (mode !== 'password' || currentPassword.length > 0)
    : password.length > 0;
  const inputStyle = {
    width: '100%',
    boxSizing: 'border-box' as const,
    border: '1px solid var(--ol-line-strong)',
    borderRadius: 9,
    background: 'var(--ol-control-solid)',
    color: 'var(--ol-ink)',
    padding: '10px 12px',
    font: 'inherit',
  };
  return (
    <form
      style={{ display: 'grid', gap: 14 }}
      onCompositionStart={() => {
        composing.current = true;
      }}
      onCompositionEnd={() => {
        composing.current = false;
      }}
      onKeyDown={(event) => {
        if (
          event.key === 'Enter' &&
          (composing.current || event.nativeEvent.isComposing || event.keyCode === 229)
        )
          event.preventDefault();
      }}
      onSubmit={(event) => {
        event.preventDefault();
        if (busy || !valid || composing.current) return;
        const value = { password, confirmation, currentPassword, rememberKey };
        // Clear controlled inputs immediately, before the async native operation.
        setPassword('');
        setConfirmation('');
        setCurrentPassword('');
        onSubmit(value);
      }}
    >
      {mode === 'password' && (
        <label>
          {t('cloudSyncE2ee.currentPassword')}
          <input
            type="password"
            value={currentPassword}
            onChange={(event) => setCurrentPassword(event.currentTarget.value)}
            autoComplete="current-password"
            maxLength={4096}
            disabled={busy}
            style={inputStyle}
          />
        </label>
      )}
      <label>
        {t(creating ? 'cloudSyncE2ee.newPassword' : 'cloudSyncE2ee.password')}
        <input
          type="password"
          value={password}
          onChange={(event) => setPassword(event.currentTarget.value)}
          autoComplete={creating ? 'new-password' : 'current-password'}
          maxLength={4096}
          disabled={busy}
          style={inputStyle}
        />
      </label>
      {creating && (
        <label>
          {t('cloudSyncE2ee.confirmPassword')}
          <input
            type="password"
            value={confirmation}
            onChange={(event) => setConfirmation(event.currentTarget.value)}
            autoComplete="new-password"
            maxLength={4096}
            disabled={busy}
            style={inputStyle}
          />
        </label>
      )}
      <p>
        {t('cloudSyncE2ee.passwordPolicy')} {t('cloudSyncE2ee.passwordWarning')}
      </p>
      <label style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
        <input
          type="checkbox"
          checked={rememberKey}
          onChange={(event) => setRememberKey(event.currentTarget.checked)}
          disabled={busy}
        />
        {t('cloudSyncE2ee.rememberKey')}
      </label>
      <button
        type="submit"
        className="ol-focus-ring"
        disabled={!valid || busy}
        style={{
          padding: '10px 14px',
          borderRadius: 9,
          background: 'var(--ol-primary-solid-bg)',
          color: 'var(--ol-primary-solid-ink)',
          fontWeight: 600,
          opacity: !valid || busy ? 0.5 : 1,
        }}
      >
        {t(
          mode === 'create'
            ? 'cloudSyncE2ee.create'
            : mode === 'unlock'
              ? 'cloudSyncE2ee.unlock'
              : 'cloudSyncE2ee.changePassword',
        )}
      </button>
    </form>
  );
}

function RestoreReview({
  preview,
  busy,
  stale,
  onApply,
}: {
  preview: RestorePreview;
  busy: boolean;
  stale: boolean;
  onApply: (mode: 'replace' | 'merge', choices: SyncConflictChoice[]) => void;
}) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<'replace' | 'merge'>('merge');
  const [reviewed, setReviewed] = useState(false);
  const [choices, setChoices] = useState<Record<string, 'local' | 'cloud'>>({});
  const [page, setPage] = useState(0);
  const pageSize = 20;
  const selected = preview.conflicts.filter(
    (conflict) => choices[conflict.id] !== undefined,
  ).length;
  const complete = mode === 'replace' || selected === preview.conflicts.length;
  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <p>{t('cloudSyncE2ee.restoreDescription')}</p>
      {preview.unconfirmedOperationId && (
        <p role="note">{t('cloudSyncE2ee.errors.outcomeUnknown')}</p>
      )}
      <ul style={{ margin: 0, paddingLeft: 20, lineHeight: 1.75 }}>
        {Object.entries(preview.counts)
          .filter(([, count]) => count > 0)
          .map(([kind, count]) => (
            <li key={kind}>
              {t(`cloudSyncE2ee.categories.${categoryKey(kind)}`)}: {count}
            </li>
          ))}
      </ul>
      {preview.deviceSettingsToReview.length > 0 && (
        <p>{t('cloudSyncE2ee.deviceReview', { count: preview.deviceSettingsToReview.length })}</p>
      )}
      <fieldset
        style={{ border: 0, padding: 0, margin: 0, display: 'grid', gap: 9 }}
        disabled={busy}
      >
        <legend>{t('cloudSyncE2ee.restoreMode')}</legend>
        <label>
          <input
            type="radio"
            name="e2ee-restore-mode"
            checked={mode === 'merge'}
            onChange={() => {
              setMode('merge');
              setReviewed(false);
            }}
          />{' '}
          {t('cloudSyncE2ee.merge')}
        </label>
        <label>
          <input
            type="radio"
            name="e2ee-restore-mode"
            checked={mode === 'replace'}
            onChange={() => {
              setMode('replace');
              setReviewed(false);
            }}
          />{' '}
          {t('cloudSyncE2ee.replace')}
        </label>
      </fieldset>
      {mode === 'replace' ? (
        <p style={{ color: 'var(--ol-err)' }}>{t('cloudSyncE2ee.replaceWarning')}</p>
      ) : (
        preview.conflicts.length > 0 && (
          <>
            <p>
              {t('cloudSyncE2ee.conflictProgress', { selected, total: preview.conflicts.length })}
            </p>
            {preview.conflicts
              .slice(page * pageSize, (page + 1) * pageSize)
              .map((conflict, index) => (
                <fieldset
                  key={conflict.id}
                  disabled={busy}
                  style={{ border: '1px solid var(--ol-line)', borderRadius: 10, padding: 12 }}
                >
                  <legend>
                    {t('cloudSyncE2ee.conflictItem', {
                      index: page * pageSize + index + 1,
                      category: t(`cloudSyncE2ee.categories.${categoryKey(conflict.kind)}`),
                    })}
                  </legend>
                  <p>
                    {t(
                      `cloudSyncE2ee.conflictReasons.${['both_modified', 'delete_modify', 'no_common_baseline'].includes(conflict.reason) ? conflict.reason : 'other'}`,
                    )}
                  </p>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16, marginTop: 8 }}>
                    {(['local', 'cloud'] as const).map((side) => (
                      <label key={side}>
                        <input
                          type="radio"
                          name={`e2ee-conflict-${page * pageSize + index}`}
                          checked={choices[conflict.id] === side}
                          onChange={() => {
                            setChoices((current) => ({ ...current, [conflict.id]: side }));
                            setReviewed(false);
                          }}
                        />{' '}
                        {t(`cloudSyncE2ee.conflict${side === 'local' ? 'Local' : 'Cloud'}`)}
                      </label>
                    ))}
                  </div>
                </fieldset>
              ))}
            {preview.conflicts.length > pageSize && (
              <div className="ol-cloud-sync-actions">
                <Btn disabled={page === 0 || busy} onClick={() => setPage((value) => value - 1)}>
                  {t('cloudSyncE2ee.previous')}
                </Btn>
                <span>
                  {page + 1} / {Math.ceil(preview.conflicts.length / pageSize)}
                </span>
                <Btn
                  disabled={(page + 1) * pageSize >= preview.conflicts.length || busy}
                  onClick={() => setPage((value) => value + 1)}
                >
                  {t('cloudSyncE2ee.next')}
                </Btn>
              </div>
            )}
          </>
        )
      )}
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <input
          type="checkbox"
          checked={reviewed}
          disabled={busy || stale}
          onChange={(event) => setReviewed(event.currentTarget.checked)}
        />
        {t('cloudSyncE2ee.restoreCheck')}
      </label>
      {stale && (
        <p role="alert" style={{ color: 'var(--ol-err)' }}>
          {t('cloudSyncE2ee.errors.changed')}
        </p>
      )}
      <Btn
        variant="primary"
        disabled={busy || stale || !reviewed || !complete}
        onClick={() =>
          onApply(
            mode,
            mode === 'replace'
              ? []
              : preview.conflicts.map((conflict) => ({
                  id: conflict.id,
                  side: choices[conflict.id],
                })),
          )
        }
      >
        {t('cloudSyncE2ee.applyRestore')}
      </Btn>
    </div>
  );
}

function DeleteConfirmation({
  busy,
  backupRetentionDays,
  onConfirm,
}: {
  busy: boolean;
  backupRetentionDays: number | null;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const [confirmed, setConfirmed] = useState(false);
  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <p>{t('cloudSyncE2ee.deleteDescription')}</p>
      {backupRetentionDays !== null &&
        Number.isSafeInteger(backupRetentionDays) &&
        backupRetentionDays >= 0 && (
          <p>{t('cloudSyncE2ee.backupRetention', { days: backupRetentionDays })}</p>
        )}
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <input
          type="checkbox"
          checked={confirmed}
          disabled={busy}
          onChange={(event) => setConfirmed(event.currentTarget.checked)}
        />
        {t('cloudSyncE2ee.deleteCheck')}
      </label>
      <Btn variant="primary" disabled={!confirmed || busy} onClick={onConfirm}>
        {t('cloudSyncE2ee.confirmDelete')}
      </Btn>
    </div>
  );
}

function SyncDialog({
  title,
  busy,
  onClose,
  children,
}: {
  title: string;
  busy: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const titleId = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef(document.activeElement);
  useEffect(() => {
    const previous = opener.current;
    const background =
      previous instanceof HTMLElement ? previous.closest<HTMLElement>('[role="dialog"]') : null;
    const wasInert = background?.inert ?? false;
    if (background) background.inert = true;
    return () => {
      if (background) background.inert = wasInert;
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  useEffect(() => {
    dialog.current?.focus();
  }, [title]);
  return (
    <Modal zIndex={100} width="min(580px, 100%)" onClose={onClose}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby={titleId}
        style={{ display: 'grid', gap: 18, outline: 'none', fontSize: 13, lineHeight: 1.65 }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            if (!busy) onClose();
          }
          if (event.key !== 'Tab') return;
          const controls = Array.from(
            dialog.current?.querySelectorAll<HTMLElement>(
              'button:not(:disabled), input:not(:disabled), summary, [tabindex="0"]',
            ) ?? [],
          );
          if (controls.length === 0) {
            event.preventDefault();
            return;
          }
          const first = controls[0];
          const last = controls[controls.length - 1];
          if (
            event.shiftKey &&
            (document.activeElement === first || document.activeElement === dialog.current)
          ) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }}
      >
        <header
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 12,
          }}
        >
          <h3 id={titleId} style={{ margin: 0, fontSize: 17 }}>
            {title}
          </h3>
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            aria-label={t('common.close')}
            className="ol-tool-close"
          >
            <Icon name="close" size={17} />
          </button>
        </header>
        {children}
        <div>
          <Btn disabled={busy} onClick={onClose}>
            {t('common.close')}
          </Btn>
        </div>
      </div>
    </Modal>
  );
}
