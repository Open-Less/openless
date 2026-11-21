import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { NotificationHandle } from '../../api/interaction';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { RestoreReview } from './RestoreReview';
import { matchesRestorePreview } from './restoreContext';
import { Icon } from '../../components/Icon';
import { GithubLoginModal } from '../accounts/GithubLoginModal';
import { ExitPresence } from '../../ui/overlays/Modal';
import {
  Dialog as StructuredDialog,
  DialogHeader,
  DialogBody,
  DialogFooter,
  DialogAction,
  type DialogLayout,
} from '../../ui/overlays/Dialog';
import { syncDisclosure } from './disclosure';
import { SyncPasswordField } from './SyncPasswordField';
import { Collapse } from '../../components/ui/Disclosure';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { useModalActive } from '../../ui/layout/AppViewport';
import { Toggle } from '../../ui/controls/SettingRow';
import { useHotkeySettings } from '../../data/preferences';
import { marketplaceAuthStatus } from '../../api/marketplace';
import { readCredential } from '../../api/providers';
import { isTauriHost } from '../../runtime/environment';
import { getCloudSyncPreviewScenario } from '../../preview/cloud-sync';
import {
  CLOUD_SYNC_E2EE_CONSENT_VERSION as CONSENT_VERSION,
  cloudSyncE2eeStatus,
  cloudSyncE2eeRefreshBackupMetadata,
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
  cloudSyncE2eeSignInWithToken,
  cloudSyncE2eeConfigureServer,
  mirrorEncryptedSyncUiPreferences,
  encryptedSyncScope,
  encryptedSyncErrorKey,
  listenEncryptedSyncStateChanges,
  listenEncryptedSyncConflicts,
  listenEncryptedSyncRestores,
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
} from '../../api/cloud-sync';

// Must match CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT in src-tauri/src/commands/credentials.rs.
const CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT = 'cloud_sync.custom_token';

type Intent = 'enable' | 'unlock' | 'restore';
type Dialog =
  | { kind: 'consent'; intent: Intent }
  | { kind: 'create'; observedRevision: string }
  | { kind: 'unlock'; intent: Intent }
  | { kind: 'password' }
  | { kind: 'restore'; preview: RestorePreview; enableAfter: boolean; scope: string }
  | { kind: 'disable' };
type Notice = { key: string; error?: boolean } | null;
type PasswordSubmission = {
  password: string;
  confirmation: string;
  currentPassword: string;
  rememberKey: boolean;
};
type SetupFocus = 'enable' | 'password' | 'unlock' | 'closed' | 'done' | 'preparing' | 'state';
const REFRESH_ERRORS = new Set([
  'localData',
  'unknown',
  'network',
  'tooLarge',
  'rolledBack',
  'changed',
  'accountChanged',
  'invalidData',
  'rateLimited',
  'unavailable',
]);
function setupFocus(
  status: EncryptedSyncStatus,
  signedIn: boolean,
  preparing: boolean,
  reached: 'password' | 'unlock' | null,
): SetupFocus {
  if (preparing) return 'preparing';
  if (!signedIn || status.syncState === 'sign_in_required' || status.authState === 'expired')
    return 'enable';
  if (
    status.syncState === 'ready' ||
    status.syncState === 'pending' ||
    status.syncState === 'syncing' ||
    status.syncState === 'conflict' ||
    status.syncState === 'failed' ||
    status.syncState === 'outcome_unknown' ||
    status.syncState === 'recovery_required' ||
    status.recoveryRequired
  )
    return 'state';
  const noSnapshot = status.hasCloudSnapshot !== true;
  const locked = status.keyState !== 'unlocked';
  if (!status.enabled) {
    if (reached === 'password' || (noSnapshot && status.consentVersion === CONSENT_VERSION))
      return 'password';
    if (reached === 'unlock' || (status.hasCloudSnapshot === true && locked)) return 'unlock';
    if (status.hasCloudSnapshot === true && !locked) return 'closed';
    return 'enable';
  }
  if (locked || reached === 'unlock') return 'unlock';
  if (noSnapshot) return 'password';
  return 'done';
}
function retainsSyncScope(previous: EncryptedSyncStatus, next: EncryptedSyncStatus): boolean {
  return (
    previous.serviceOrigin === next.serviceOrigin &&
    (!previous.account || previous.account.githubId === next.account?.githubId) &&
    (!previous.vaultId || previous.vaultId === next.vaultId)
  );
}

function localError(reason: string): unknown {
  return { details: { reason } };
}
function normalizeCustomServerOrigin(value: string): string | null | undefined {
  const trimmed = value.trim();
  if (!trimmed) return null;
  try {
    const origin = new URL(trimmed);
    if (
      origin.protocol !== 'https:' ||
      origin.username ||
      origin.password ||
      origin.pathname !== '/' ||
      origin.search ||
      origin.hash
    ) {
      return undefined;
    }
    return origin.origin;
  } catch {
    return undefined;
  }
}
export async function prepareEncryptedSync(scope?: string): Promise<EnablePreparation> {
  await mirrorEncryptedSyncUiPreferences();
  if (scope && encryptedSyncScope(await cloudSyncE2eeStatus()) !== scope)
    throw localError('account_changed');
  const [serviceOrigin, accountId, vaultId] = scope
    ? (JSON.parse(scope) as [string, string | null, string | null])
    : [];
  return cloudSyncE2eePrepareEnable(
    CONSENT_VERSION,
    serviceOrigin
      ? { serviceOrigin, accountId: accountId ?? null, vaultId: vaultId ?? null }
      : undefined,
  );
}

/** Advanced-only configuration; the normal sync flow never loads the server token. */
export function CustomSyncServerSection() {
  const { t } = useTranslation();
  const { notify } = useInteraction('custom-sync-server');
  const { prefs, refresh } = useHotkeySettings();
  const [origin, setOrigin] = useState(prefs?.syncCustomServerOrigin ?? '');
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<EncryptedSyncStatus | null>(null);
  const alive = useRef(false);
  const saving = useRef(false);
  const reportError = (error: unknown) =>
    notify({
      key: 'custom-sync-server-error',
      state: 'failed',
      message: t(`cloudSyncE2ee.errors.${encryptedSyncErrorKey(error)}`),
    });
  const showError = useRef(reportError);
  showError.current = reportError;
  useEffect(() => {
    setOrigin(prefs?.syncCustomServerOrigin ?? '');
  }, [prefs?.syncCustomServerOrigin]);
  useEffect(() => {
    alive.current = true;
    void Promise.all([readCredential(CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT), cloudSyncE2eeStatus()])
      .then(([savedToken, next]) => {
        if (!alive.current) return;
        setToken(savedToken ?? '');
        setStatus(next);
      })
      .catch((error) => {
        if (alive.current) showError.current(error);
      })
      .finally(() => {
        if (alive.current) setLoading(false);
      });
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (status?.syncState !== 'syncing') return;
    let disposed = false;
    let pending = false;
    let reportedFailure = false;
    const timer = window.setInterval(() => {
      if (pending) return;
      pending = true;
      void cloudSyncE2eeStatus()
        .then((next) => {
          if (!disposed) setStatus(next);
          reportedFailure = false;
        })
        .catch((error) => {
          if (!disposed && !reportedFailure) {
            reportedFailure = true;
            showError.current(error);
          }
        })
        .finally(() => {
          pending = false;
        });
    }, 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [status?.syncState]);
  const working = busy || loading || status?.syncState === 'syncing';
  const save = () => {
    if (saving.current || working || !prefs) return;
    const normalized = normalizeCustomServerOrigin(origin);
    const trimmedToken = token.trim();
    if (normalized === undefined || Boolean(normalized) !== Boolean(trimmedToken)) {
      showError.current(localError('unsupported_protocol'));
      return;
    }
    saving.current = true;
    setBusy(true);
    void (async () => {
      const configured = await cloudSyncE2eeConfigureServer({
        origin: normalized,
        token: trimmedToken,
      });
      await refresh();
      const next = trimmedToken ? await cloudSyncE2eeSignInWithToken() : configured;
      if (alive.current) {
        setStatus(next);
        setToken('');
        notify({ state: 'saved', message: t('cloudSyncE2ee.done') });
      }
    })()
      .catch((error) => {
        if (alive.current) showError.current(error);
      })
      .finally(() => {
        saving.current = false;
        if (alive.current) setBusy(false);
      });
  };
  const inputStyle = {
    width: '100%',
    boxSizing: 'border-box' as const,
    border: '1px solid var(--ol-line-strong)',
    borderRadius: 'var(--ol-r-md)',
    background: 'var(--ol-control-solid)',
    color: 'var(--ol-ink)',
    padding: '10px 12px',
  };
  return (
    <Card>
      <div
        className="ol-cloud-sync-account"
        style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8 }}
      >
        <strong>{t('cloudSyncE2ee.customServerTitle')}</strong>
        <p className="ol-cloud-sync-scope">{t('cloudSyncE2ee.customServerHint')}</p>
        <label>
          {t('cloudSyncE2ee.customServerOrigin')}
          <input
            type="text"
            value={origin}
            onChange={(event) => setOrigin(event.currentTarget.value)}
            placeholder="https://sync.example.com/"
            disabled={working}
            style={inputStyle}
          />
        </label>
        <label>
          {t('cloudSyncE2ee.customServerToken')}
          <input
            type="password"
            value={token}
            onChange={(event) => setToken(event.currentTarget.value)}
            autoComplete="off"
            disabled={working}
            style={inputStyle}
          />
        </label>
        <Btn size="sm" variant="blue" disabled={working || !prefs || !status} onClick={save}>
          {t('cloudSyncE2ee.customServerSave')}
        </Btn>
      </div>
    </Card>
  );
}

export function CloudSyncSection() {
  const { t, i18n } = useTranslation();
  const { notify, confirm } = useInteraction('cloud-sync');
  const modalActive = useModalActive();
  const dialogDescriptionId = useId();
  const previewScenario = getCloudSyncPreviewScenario();
  const { prefs, updatePrefs, refresh } = useHotkeySettings();
  const [status, setStatus] = useState<EncryptedSyncStatus | null>(null);
  const [authSignedIn, setAuthSignedIn] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [cancelRequestedTask, setCancelRequestedTask] = useState<string | null>(null);
  const cancelling = useRef<string | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const currentDialog = useRef(dialog);
  currentDialog.current = dialog;
  const [showLogin, setShowLogin] = useState(false);
  const [preparingStep, setPreparingStep] = useState(false);
  const [setupReached, setSetupReached] = useState<'password' | 'unlock' | null>(null);
  const current = useRef<EncryptedSyncStatus | null>(null);
  const watermark = useRef('0');
  const alive = useRef(false);
  const loadSequence = useRef(0);
  const actionSequence = useRef(0);
  const activeAction = useRef<number | null>(null);
  const loginHint = prefs?.marketplaceDevLogin?.trim() ?? '';
  const customServerOrigin = prefs?.syncCustomServerOrigin ?? '';

  const progressNotification = useRef<NotificationHandle>();
  const notificationSettled = useRef(false);
  const syncIntent = useRef(false);
  const awaitingSync = useRef(false);
  const announce = (key: string, state: 'saved' | 'info' | 'failed' = 'saved') => {
    const options = {
      state,
      message: t(`cloudSyncE2ee.${key}`),
      progress: false,
      actionLabel: undefined,
      onAction: undefined,
      durationMs: state === 'saved' ? 3500 : 0,
    };
    if (progressNotification.current) progressNotification.current.update(options);
    else notify({ ...options, key: 'cloud-sync-operation' });
    progressNotification.current = undefined;
    notificationSettled.current = true;
  };
  const showError = (error: unknown, settled = false) => {
    if (!alive.current) return;
    const errorKey = encryptedSyncErrorKey(error);
    const key = `errors.${errorKey}`;
    if (currentDialog.current && errorKey !== 'outcomeUnknown') setNotice({ key, error: true });
    else if (settled) announce(key, 'failed');
    else
      notify({
        key: 'cloud-sync-status-error',
        state: 'failed',
        message: t(`cloudSyncE2ee.${key}`),
      });
  };
  const acceptStatus = (next: EncryptedSyncStatus): boolean => {
    if (!alive.current) return false;
    const sequence = syncSequence(next.sequence);
    const previous = syncSequence(watermark.current);
    if (sequence === null || previous === null || sequence < previous) return false;
    const before = current.current;
    const scopeChanged = before && encryptedSyncScope(before) !== encryptedSyncScope(next);
    current.current = next;
    watermark.current = next.sequence;
    setStatus(next);
    if (scopeChanged) {
      if (activeAction.current === null || !retainsSyncScope(before, next)) {
        setDialog(null);
      }
      awaitingSync.current = false;
      syncIntent.current = false;
    } else if (awaitingSync.current && next.syncState !== 'syncing') {
      awaitingSync.current = false;
      syncIntent.current = false;
      if (next.lastError) showError(next.lastError, true);
      else if (next.syncState === 'ready') announce('synced');
      else if (next.syncState === 'conflict') announce('states.conflict', 'info');
    }
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
      if (event.status.syncState === 'syncing' && event.status.taskId !== snapshot.taskId)
        notificationSettled.current = false;
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
      setDialog({
        kind: 'restore',
        preview: event.preview,
        enableAfter: false,
        scope: encryptedSyncScope(snapshot),
      });
      announce('states.conflict', 'info');
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
      // Restoring local data can precede a final upload; only the command result announces success.
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
        if (isTauriHost()) {
          const subscriptions = [
            () => listenEncryptedSyncStateChanges((event) => {
              if (!disposed) eventsRef.current.state(event);
            }),
            () => listenEncryptedSyncConflicts((event) => {
              if (!disposed) eventsRef.current.conflict(event);
            }),
            () => listenEncryptedSyncRestores((event) => {
              if (!disposed) eventsRef.current.restored(event);
            }),
          ];
          for (const subscribe of subscriptions) {
            const stop = await subscribe();
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
    notificationSettled.current = false;
    setBusy(true);
    setNotice(null);
    const initial = current.current;
    const valid = () =>
      alive.current &&
      actionSequence.current === action &&
      (!initial || !current.current || retainsSyncScope(initial, current.current));
    try {
      await work(valid);
    } catch (error) {
      if (valid()) {
        showError(error, true);
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
    if (!scope || !matchesRestorePreview(preview, next, scope)) throw localError('stale_preview');
    if (!acceptStatus(next)) return;
    setDialog({ kind: 'restore', preview, enableAfter, scope });
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
      setSetupReached('password');
      setDialog({ kind: 'create', observedRevision: prepared.status.remoteRevision });
    } else if (prepared.nextStep === 'unlock') {
      setSetupReached('unlock');
      setDialog({ kind: 'unlock', intent });
    } else if (prepared.nextStep === 'restore_review') {
      await previewRestore(intent === 'enable', valid);
    } else if (intent === 'restore') {
      await previewRestore(false, valid);
    } else {
      if (intent === 'enable' && !prepared.status.enabled) {
        const next = await cloudSyncE2eeSetEnabled(true);
        if (!valid()) return;
        acceptStatus(next);
        if (next.enabled && next.keyState === 'unlocked' && next.hasCloudSnapshot === true)
          setSetupReached(null);
      }
      if (valid()) {
        setDialog(null);
        announce('done');
      }
    }
  };
  const begin = (intent: Intent) => {
    setNotice(null);
    if (current.current?.consentVersion !== CONSENT_VERSION) {
      setDialog({ kind: 'consent', intent });
    } else {
      const scope = current.current ? encryptedSyncScope(current.current) : undefined;
      void perform(async (valid) =>
        routePreparation(await prepareEncryptedSync(scope), intent, valid),
      );
    }
  };
  const simple = (
    request: () => Promise<EncryptedSyncStatus>,
    signOut = false,
    syncing = false,
  ) => {
    syncIntent.current = syncing;
    void perform(async (valid) => {
      const next = await request();
      if (!valid() || !acceptStatus(next)) return;
      if (signOut) {
        try {
          const auth = await marketplaceAuthStatus();
          if (valid()) setAuthSignedIn(auth.signedIn);
        } catch {
          if (valid()) setAuthSignedIn(false);
        }
        await refresh();
      }
      if (next.syncState === 'conflict') {
        if (valid()) announce('states.conflict', 'info');
        await previewRestore(false, valid);
        return;
      }
      if (valid()) {
        setDialog(null);
        if (next.syncState === 'syncing') {
          awaitingSync.current = syncing;
          return;
        }
        if (next.lastError) {
          showError(next.lastError, true);
          return;
        }
        if (!syncing || next.syncState === 'ready') announce(syncing ? 'synced' : 'done');
      }
    }).finally(() => {
      if (!awaitingSync.current) syncIntent.current = false;
    });
  };

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
          announce('done');
        }
      } else if (active.kind === 'unlock') {
        const next = await cloudSyncE2eeUnlock({
          password: value.password,
          rememberKey: value.rememberKey,
        });
        if (!valid() || !acceptStatus(next)) return;
        await routePreparation(
          await prepareEncryptedSync(
            current.current ? encryptedSyncScope(current.current) : undefined,
          ),
          active.intent,
          valid,
        );
      } else if (active.kind === 'password') {
        const next = await cloudSyncE2eeChangePassword({
          currentPassword: value.currentPassword,
          newPassword: value.password,
          confirmation: value.confirmation,
          rememberKey: value.rememberKey,
        });
        if (valid() && acceptStatus(next)) {
          setDialog(null);
          announce('done');
        }
      }
    });
  };
  const applyRestore = (mode: 'replace' | 'merge', conflictChoices: SyncConflictChoice[]) => {
    if (dialog?.kind !== 'restore') return;
    const { preview, enableAfter, scope } = dialog;
    void perform(async (valid) => {
      const snapshot = current.current;
      if (!matchesRestorePreview(preview, snapshot, scope)) throw localError('stale_preview');
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
        announce('restored');
      }
    });
  };
  const cancelTask = async (task: string) => {
    if (current.current?.taskId !== task || cancelling.current === task) return;
    cancelling.current = task;
    setCancelRequestedTask(task);
    try {
      const next = await cloudSyncE2eeCancel(task);
      if (alive.current) acceptStatus(next);
    } catch (error) {
      if (alive.current && current.current?.taskId === task) {
        setCancelRequestedTask((value) => (value === task ? null : value));
        showError(error);
      }
    } finally {
      if (cancelling.current === task) cancelling.current = null;
    }
  };
  const cancelTaskRef = useRef(cancelTask);
  cancelTaskRef.current = cancelTask;

  const customServerConfigured = Boolean(prefs?.syncCustomServerOrigin?.trim());
  const syncSignedIn =
    status?.authState === 'signed_in' && status?.syncState !== 'sign_in_required';
  const identityAvailable =
    status?.authState !== 'expired' &&
    status?.syncState !== 'sign_in_required' &&
    (syncSignedIn || (Boolean(prefs) && !customServerConfigured && authSignedIn));
  const working = busy || status?.syncState === 'syncing';
  const taskId = status?.taskId ?? null;
  const cancelRequested = taskId !== null && cancelRequestedTask === taskId;
  const sectionHeaderTask = modalActive && !dialog && working ? taskId : null;
  const dialogHeaderTask = dialog && working ? taskId : null;
  const sectionHeaderAction = t(
    sectionHeaderTask ? 'cloudSyncE2ee.cancelTask' : 'cloudSyncE2ee.refresh',
  );
  const progressMessage = t(
    cancelRequested
      ? 'cloudSyncE2ee.cancelRequested'
      : syncIntent.current
        ? 'cloudSyncE2ee.syncing'
        : 'cloudSyncE2ee.working',
  );
  useEffect(() => {
    if (!working) {
      progressNotification.current?.dismiss();
      progressNotification.current = undefined;
      return;
    }
    if (notificationSettled.current) return;
    // Global notification actions are outside the modal focus scope.
    const canCancel = taskId !== null && !cancelRequested && !modalActive;
    const options = {
      message: progressMessage,
      state: 'saving' as const,
      progress: true,
      actionLabel: canCancel ? t('cloudSyncE2ee.cancelTask') : undefined,
      onAction: canCancel ? () => void cancelTaskRef.current(taskId) : undefined,
    };
    if (progressNotification.current) progressNotification.current.update(options);
    else progressNotification.current = notify({ ...options, key: 'cloud-sync-operation' });
  }, [working, taskId, cancelRequested, modalActive, progressMessage, notify, t]);
  useEffect(
    () => () => {
      progressNotification.current?.dismiss();
      progressNotification.current = undefined;
    },
    [],
  );
  useEffect(() => {
    if (cancelRequestedTask !== null && cancelRequestedTask !== taskId)
      setCancelRequestedTask(null);
  }, [cancelRequestedTask, taskId]);
  const unlocked = status?.keyState === 'unlocked';
  const available = status !== null;
  const lastSync = status?.lastSuccessfulSyncAt ? new Date(status.lastSuccessfulSyncAt) : null;
  const lastError = status?.lastError ? encryptedSyncErrorKey(status.lastError) : null;
  const visibleError = !dialog ? lastError : null;
  const focus = status
    ? setupFocus(status, identityAvailable, preparingStep, setupReached)
    : identityAvailable
      ? 'enable'
      : 'enable';
  const focusTitle =
    focus === 'state' && status
      ? `cloudSyncE2ee.states.${status.recoveryRequired ? 'recovery_required' : status.syncState}`
      : `cloudSyncE2ee.step${focus[0].toUpperCase()}${focus.slice(1)}Title`;
  const focusDetail =
    focus === 'state'
      ? 'cloudSyncE2ee.stepFollowDetail'
      : `cloudSyncE2ee.step${focus[0].toUpperCase()}${focus.slice(1)}Detail`;
  const canSync =
    !working &&
    status?.consentVersion === CONSENT_VERSION &&
    !status?.recoveryRequired &&
    (Boolean(status?.pendingOperationId) ||
      (Boolean(status?.enabled) && unlocked && status?.syncState !== 'conflict'));
  const consentNeedsUpdate = Boolean(status?.enabled && status.consentVersion !== CONSENT_VERSION);
  const dialogTitle =
    dialog?.kind === 'create'
      ? 'setSyncPasswordTitle'
      : dialog?.kind === 'unlock'
        ? 'unlockTitle'
        : dialog?.kind === 'password'
          ? 'passwordTitle'
          : `${dialog?.kind ?? 'consent'}Title`;
  const closeDialog = () => {
    if (activeAction.current !== null) return;
    if (!working) {
      setDialog(null);
      setNotice(null);
    }
  };

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
            onClick={() => void (sectionHeaderTask ? cancelTask(sectionHeaderTask) : load())}
            disabled={sectionHeaderTask ? cancelRequested : loading}
            aria-label={sectionHeaderAction}
            title={sectionHeaderAction}
          >
            <Icon name={sectionHeaderTask ? 'close' : 'refresh'} size={16} />
          </button>
        </header>
        {previewScenario && <p role="note">{t('cloudSyncE2ee.previewNotice')}</p>}
        <div className="ol-cloud-sync-account" style={{ justifyContent: 'space-between' }}>
          <strong id="cloud-sync-e2ee-enable">{t('cloudSyncE2ee.enable')}</strong>
          <Toggle
            on={status?.enabled ?? false}
            label={t('cloudSyncE2ee.enable')}
            disabled={
              !available || !identityAvailable || working || loading || status?.recoveryRequired
            }
            onToggle={(next) => (next ? begin('enable') : setDialog({ kind: 'disable' }))}
          />
        </div>
        {loading && <p role="status">{t('cloudSyncE2ee.loading')}</p>}
        {!loading && !customServerConfigured && !syncSignedIn && !authSignedIn && (
          <Btn
            variant="primary"
            icon="user"
            disabled={!isTauriHost() || !available || working}
            onClick={() => setShowLogin(true)}
          >
            {t('cloudSyncE2ee.signIn')}
          </Btn>
        )}
        {syncSignedIn && (
          <div className="ol-cloud-sync-account">
            <Icon name="user" size={16} />
            <span>{t('cloudSyncE2ee.account')}</span>
            <strong dir="auto">
              {status?.account?.login
                ? customServerOrigin.trim()
                  ? `${status.account.login}@${customServerOrigin
                      .trim()
                      .replace(/^[a-z]+:\/\//i, '')
                      .replace(/\/$/, '')}`
                  : `@${status.account.login}`
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
            <strong>{t(focusTitle)}</strong>
            <p>{t(focusDetail)}</p>
            <div className="ol-cloud-sync-status-line">
              <span>{t(unlocked ? 'cloudSyncE2ee.keyUnlocked' : 'cloudSyncE2ee.keyLocked')}</span>
              {!unlocked && syncSignedIn && (
                <Btn
                  size="sm"
                  variant="blue"
                  disabled={working || loading}
                  onClick={() => begin(status.hasCloudSnapshot ? 'unlock' : 'enable')}
                >
                  {t(
                    status.hasCloudSnapshot ? 'cloudSyncE2ee.unlock' : 'cloudSyncE2ee.setPassword',
                  )}
                </Btn>
              )}
            </div>
            <div className="ol-cloud-sync-status-line">
              <span>
                {t(
                  status.hasCloudSnapshot === true
                    ? 'cloudSyncE2ee.hasSnapshot'
                    : status.hasCloudSnapshot === false
                      ? 'cloudSyncE2ee.noSnapshot'
                      : 'cloudSyncE2ee.snapshotUnknown',
                )}
              </span>
              {status.hasCloudSnapshot === false && syncSignedIn && unlocked && (
                <Btn
                  size="sm"
                  variant="blue"
                  disabled={working || loading}
                  onClick={() => begin('enable')}
                >
                  {t('cloudSyncE2ee.setPassword')}
                </Btn>
              )}
            </div>
            <small>
              {lastSync &&
                Number.isFinite(lastSync.getTime()) &&
                t('cloudSyncE2ee.lastSync', {
                  time: lastSync.toLocaleString(i18n.resolvedLanguage ?? i18n.language),
                })}
            </small>
          </div>
        )}
        {status?.consentVersion && status.consentVersion !== CONSENT_VERSION && (
          <div className="ol-cloud-sync-actions">
            {status.enabled && (
              <Btn variant="blue" disabled={working} onClick={() => begin('enable')}>
                {syncDisclosure(i18n.resolvedLanguage ?? i18n.language).updatedTitle}
              </Btn>
            )}
            <Btn disabled={working} onClick={() => simple(cloudSyncE2eeRefreshBackupMetadata)}>
              {t('cloudSyncE2ee.refreshBackup')}
            </Btn>
          </div>
        )}
        {syncSignedIn && available && (
          <div className="ol-cloud-sync-actions">
            {!unlocked && status?.hasCloudSnapshot && (
              <Btn disabled={working} onClick={() => begin('unlock')}>
                {t('cloudSyncE2ee.unlock')}
              </Btn>
            )}
            <Btn
              variant={canSync ? 'blue' : 'soft'}
              disabled={!canSync}
              onClick={() => simple(cloudSyncE2eeSyncNow, false, true)}
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
                if (!status?.vaultId || status.remoteRevision === null) return;
                const snapshot = status;
                const scope = encryptedSyncScope(snapshot);
                void confirm({
                  title: t('cloudSyncE2ee.deleteTitle'),
                  description: (
                    <CloudBackupDeletionDisclosure
                      backupRetentionDays={snapshot.backupRetentionDays}
                    />
                  ),
                  tone: 'danger',
                  confirmLabel: t('cloudSyncE2ee.confirmDelete'),
                  errorMessage: (error) =>
                    t(`cloudSyncE2ee.errors.${encryptedSyncErrorKey(error)}`),
                  onConfirm: async () => {
                    const latest = await cloudSyncE2eeStatus();
                    if (!alive.current || encryptedSyncScope(latest) !== scope)
                      throw localError('account_changed');
                    const next = await cloudSyncE2eeDeleteRemote({
                      expectedVaultId: snapshot.vaultId!,
                      observedRevision: snapshot.remoteRevision!,
                      confirmed: true,
                    });
                    if (next.lastError) throw next.lastError;
                    if (
                      alive.current &&
                      current.current?.serviceOrigin === snapshot.serviceOrigin &&
                      current.current.account?.githubId === snapshot.account?.githubId &&
                      acceptStatus(next)
                    ) {
                      announce('done');
                    }
                  },
                });
              }}
            >
              <Icon name="trash" size={14} />
              {t('cloudSyncE2ee.delete')}
            </button>
          </div>
        )}
        <p className="ol-cloud-sync-scope">{t('cloudSyncE2ee.passwordWarning')}</p>
        <Collapse open={!!visibleError}>
          <div className="ol-cloud-sync-status-line">
            <p role="alert" className="ol-cloud-sync-error">
              {t(`cloudSyncE2ee.errors.${visibleError}`)}
            </p>
            {REFRESH_ERRORS.has(visibleError ?? '') && (
              <Btn
                size="sm"
                variant="blue"
                disabled={loading || working}
                onClick={() => void load()}
              >
                {t('cloudSyncE2ee.refresh')}
              </Btn>
            )}
            {(visibleError === 'unlock' ||
              visibleError === 'invalidPassword' ||
              visibleError === 'secureStorage') &&
              identityAvailable && (
                <Btn
                  size="sm"
                  variant="blue"
                  disabled={working}
                  onClick={() => begin(status?.hasCloudSnapshot ? 'unlock' : 'enable')}
                >
                  {t(
                    status?.hasCloudSnapshot ? 'cloudSyncE2ee.unlock' : 'cloudSyncE2ee.setPassword',
                  )}
                </Btn>
              )}
          </div>
        </Collapse>
        {status?.lastError?.retryAfterSeconds != null &&
          Number.isSafeInteger(status.lastError.retryAfterSeconds) &&
          status.lastError.retryAfterSeconds > 0 &&
          status.lastError.retryAfterSeconds <= 86400 && (
            <p>{t('cloudSyncE2ee.retryAfter', { seconds: status.lastError.retryAfterSeconds })}</p>
          )}
      </section>
      <ExitPresence
        open={showLogin}
        render={(closing) => (
          <GithubLoginModal
            closing={closing}
            onClose={() => setShowLogin(false)}
            onSuccess={(login) => {
              setShowLogin(false);
              void updatePrefs((value) => ({ ...value, marketplaceDevLogin: login }))
                .then(() => loadRef.current())
                .catch(showError);
            }}
          />
        )}
      />
      <ExitPresence
        open={!!dialog}
        render={(closing) =>
          dialog && (
            <SyncDialog
              closing={closing}
              key={dialog.kind}
              layout={dialog.kind === 'consent' ? 'terms' : undefined}
              title={
                dialog.kind === 'consent'
                  ? syncDisclosure(i18n.resolvedLanguage ?? i18n.language)[
                      consentNeedsUpdate ? 'updatedTitle' : 'firstTimeTitle'
                    ]
                  : t(`cloudSyncE2ee.${dialogTitle}`)
              }
              descriptionId={
                dialog.kind === 'consent' || dialog.kind === 'create' ? dialogDescriptionId : undefined
              }
              busy={working}
              onClose={closeDialog}
              taskAction={
                dialogHeaderTask ? (
                  <Btn
                    size="sm"
                    disabled={cancelRequested}
                    onClick={() => void cancelTask(dialogHeaderTask)}
                  >
                    {t('cloudSyncE2ee.cancelTask')}
                  </Btn>
                ) : undefined
              }
            >
              {notice?.error && (
                <p role="alert" className="ol-dialog-error">
                  {t(`cloudSyncE2ee.${notice.key}`)}
                </p>
              )}
              {dialog.kind === 'consent' && (
                <ConsentForm
                  busy={working}
                  updated={consentNeedsUpdate}
                  descriptionId={dialogDescriptionId}
                  onCancel={closeDialog}
                  onConfirm={() => {
                    const intent = dialog.intent;
                    const scope = current.current ? encryptedSyncScope(current.current) : undefined;
                    setPreparingStep(true);
                    void perform(async (valid) => {
                      await routePreparation(await prepareEncryptedSync(scope), intent, valid);
                    }).finally(() => {
                      if (alive.current) setPreparingStep(false);
                    });
                  }}
                />
              )}
              {(dialog.kind === 'create' ||
                dialog.kind === 'unlock' ||
                dialog.kind === 'password') && (
                <PasswordForm
                  key={dialog.kind}
                  mode={dialog.kind}
                  busy={working}
                  descriptionId={dialogDescriptionId}
                  preview={Boolean(previewScenario)}
                  onCancel={closeDialog}
                  onSubmit={submitPassword}
                />
              )}
              {dialog.kind === 'restore' && (
                <DialogBody>
                  <RestoreReview
                    key={dialog.preview.previewId}
                    preview={dialog.preview}
                    busy={working}
                    stale={!matchesRestorePreview(dialog.preview, status, dialog.scope)}
                    onApply={applyRestore}
                  />
                </DialogBody>
              )}
              {dialog.kind === 'disable' && (
                <>
                  <DialogBody>
                    <p>{t('cloudSyncE2ee.disableDescription')}</p>
                  </DialogBody>
                  <DialogFooter>
                    <Btn
                      variant="primary"
                      disabled={working}
                      onClick={() => simple(() => cloudSyncE2eeSetEnabled(false))}
                    >
                      {t('cloudSyncE2ee.confirmDisable')}
                    </Btn>
                  </DialogFooter>
                </>
              )}
            </SyncDialog>
          )
        }
      />
    </Card>
  );
}

export function ConsentForm({
  busy,
  updated = false,
  descriptionId,
  onConfirm,
  onCancel,
}: {
  busy: boolean;
  updated?: boolean;
  descriptionId?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { i18n } = useTranslation();
  const disclosure = syncDisclosure(i18n.resolvedLanguage ?? i18n.language);
  const [consented, setConsented] = useState(false);
  return (
    <>
      <DialogBody data-testid="sync-terms-body">
        {updated && (
          <p role="note">
            <strong>{disclosure.updatedDescription}</strong>
          </p>
        )}
        <p id={descriptionId}>
          <strong>{disclosure.apiKeyNotice}</strong>
        </p>
        <p>{disclosure.apiProviderUse}</p>
        <p>{disclosure.intro}</p>
        <p>
          <code>{CONSENT_VERSION}</code>
        </p>
        <div className="ol-dialog-disclosure">
          {disclosure.sections.map((section) => (
            <section key={section.id}>
              <h4>{section.title}</h4>
              <p>{section.body}</p>
            </section>
          ))}
        </div>
        <p style={{ marginTop: 16 }}>{disclosure.draftBoundary}</p>
        <p>{updated ? disclosure.cancelHint : disclosure.firstTimeCancelHint}</p>
      </DialogBody>
      <DialogFooter>
        <label className="ol-dialog-acknowledgement">
          <input
            type="checkbox"
            checked={consented}
            disabled={busy}
            onChange={(event) => setConsented(event.currentTarget.checked)}
          />
          {updated ? disclosure.checkbox : disclosure.firstTimeCheckbox}
        </label>
        <DialogAction tone="neutral" disabled={busy} onClick={onCancel}>
          {disclosure.cancel}
        </DialogAction>
        <DialogAction disabled={!consented || busy} onClick={onConfirm}>
          {updated ? disclosure.confirm : disclosure.firstTimeConfirm}
        </DialogAction>
      </DialogFooter>
    </>
  );
}

export function CloudSyncTermsDialog({
  updated = false,
  busy,
  error,
  onClose,
  onConfirm,
}: {
  updated?: boolean;
  busy: boolean;
  error?: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const { i18n } = useTranslation();
  const disclosure = syncDisclosure(i18n.resolvedLanguage ?? i18n.language);
  const descriptionId = useId();
  return (
    <SyncDialog
      title={updated ? disclosure.updatedTitle : disclosure.firstTimeTitle}
      layout="terms"
      descriptionId={descriptionId}
      busy={busy}
      onClose={onClose}
    >
      {error && (
        <p className="ol-dialog-error" role="alert">
          {error}
        </p>
      )}
      <ConsentForm
        updated={updated}
        busy={busy}
        descriptionId={descriptionId}
        onConfirm={onConfirm}
        onCancel={onClose}
      />
    </SyncDialog>
  );
}

export function PasswordForm({
  mode,
  busy,
  preview = false,
  descriptionId,
  onCancel,
  onSubmit,
}: {
  mode: 'create' | 'unlock' | 'password';
  busy: boolean;
  preview?: boolean;
  descriptionId?: string;
  onCancel?: () => void;
  onSubmit: (value: PasswordSubmission) => void;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [rememberKey, setRememberKey] = useState(true);
  const composing = useRef(false);
  const submitted = useRef(false);
  useEffect(() => {
    if (!busy) submitted.current = false;
  }, [busy]);
  const creating = mode !== 'unlock';
  const passwordsMatch = password.normalize('NFC') === confirmation.normalize('NFC');
  const mismatch = creating && Boolean(password && confirmation) && !passwordsMatch;
  const valid = creating
    ? syncPasswordMeetsBasicRequirements(password) &&
      passwordsMatch &&
      (mode !== 'password' || currentPassword.length > 0)
    : password.length > 0;
  const formId = useId();
  const mismatchId = `${formId}-mismatch`;
  return (
    <>
      <DialogBody>
        {preview && <p role="note">{t('cloudSyncE2ee.previewNotice')}</p>}
        {mode === 'create' && (
          <p id={descriptionId}>{t('cloudSyncE2ee.setSyncPasswordDescription')}</p>
        )}
        <form
          id={formId}
          className="ol-sync-password-fields"
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
            if (busy || !valid || composing.current || submitted.current) return;
            submitted.current = true;
            const value = { password, confirmation, currentPassword, rememberKey };
            setPassword('');
            setConfirmation('');
            setCurrentPassword('');
            onSubmit(value);
          }}
        >
          {mode === 'password' && (
            <SyncPasswordField
              label={t('cloudSyncE2ee.currentPassword')}
              value={currentPassword}
              onChange={setCurrentPassword}
              autoComplete={preview ? 'off' : 'current-password'}
              disabled={busy}
            />
          )}
          <SyncPasswordField
            label={t(mode === 'password' ? 'cloudSyncE2ee.newPassword' : 'cloudSyncE2ee.password')}
            value={password}
            onChange={setPassword}
            autoComplete={preview ? 'off' : creating ? 'new-password' : 'current-password'}
            disabled={busy}
          />
          {creating && (
            <SyncPasswordField
              label={t(mode === 'create' ? 'cloudSyncE2ee.confirmSyncPassword' : 'cloudSyncE2ee.confirmPassword')}
              value={confirmation}
              onChange={setConfirmation}
              autoComplete={preview ? 'off' : 'new-password'}
              errorId={mismatch ? mismatchId : undefined}
              disabled={busy}
            />
          )}
          {mismatch && (
            <p id={mismatchId} className="ol-cloud-sync-error" role="status">
              {t('cloudSyncE2ee.errors.passwordMismatch')}
            </p>
          )}
          <p>
            {creating && <>{t('cloudSyncE2ee.passwordPolicy')} </>}
            {t('cloudSyncE2ee.passwordWarning')}
          </p>
          <label className="ol-dialog-acknowledgement">
            <input
              type="checkbox"
              checked={rememberKey}
              disabled={busy}
              onChange={(event) => setRememberKey(event.currentTarget.checked)}
            />
            {t('cloudSyncE2ee.rememberKey')}
          </label>
        </form>
      </DialogBody>
      <DialogFooter>
        {onCancel && (
          <DialogAction tone="neutral" disabled={busy} onClick={onCancel}>
            {t('common.cancel')}
          </DialogAction>
        )}
        <DialogAction type="submit" form={formId} disabled={!valid || busy}>
          {t(
            mode === 'create'
              ? 'cloudSyncE2ee.confirmAndContinue'
              : mode === 'unlock'
                ? 'cloudSyncE2ee.unlock'
                : 'cloudSyncE2ee.changePassword',
          )}
        </DialogAction>
      </DialogFooter>
    </>
  );
}

export function CloudBackupDeletionDisclosure({
  backupRetentionDays,
}: {
  backupRetentionDays: number | null;
}) {
  const { t, i18n } = useTranslation();
  const chinese = (i18n.resolvedLanguage ?? i18n.language).startsWith('zh');
  return (
    <>
      <p>{t('cloudSyncE2ee.deleteDescription')}</p>
      <p>
        {chinese
          ? '本机资料和离线设备上的本机副本会保留。关闭同步或退出账号不会删除云端备份。'
          : 'Local data and local copies on offline devices are kept. Turning sync off or signing out does not delete the cloud backup.'}
      </p>
      {backupRetentionDays !== null &&
        Number.isSafeInteger(backupRetentionDays) &&
        backupRetentionDays >= 0 && (
          <p>{t('cloudSyncE2ee.backupRetention', { days: backupRetentionDays })}</p>
        )}
      <p>
        {chinese
          ? '服务备份保留期不保证账号信息、日志、操作收据或删除标记同时清除，也不提供任意历史版本恢复。'
          : 'The service backup retention period does not guarantee simultaneous deletion of account information, logs, operation receipts or deletion markers, or provide selectable historical restores.'}
      </p>
    </>
  );
}

function SyncDialog({
  title,
  layout = 'standard',
  descriptionId,
  closing = false,
  busy,
  onClose,
  taskAction,
  children,
}: {
  title: string;
  layout?: DialogLayout;
  descriptionId?: string;
  closing?: boolean;
  busy: boolean;
  onClose: () => void;
  taskAction?: ReactNode;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const titleId = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <StructuredDialog
      zIndex={100}
      layout={layout}
      width={layout === 'standard' ? 'min(560px, 100%)' : undefined}
      labelledBy={titleId}
      describedBy={descriptionId}
      initialFocus={cancelRef}
      onClose={() => {
        if (!busy) onClose();
      }}
      closing={closing}
    >
      <DialogHeader>
        <h3 id={titleId}>{title}</h3>
        {taskAction ?? (
          <button
            type="button"
            ref={cancelRef}
            disabled={busy}
            onClick={onClose}
            aria-label={t('common.close')}
            className="ol-dialog-icon-close"
          >
            <Icon name="close" size={16} />
          </button>
        )}
      </DialogHeader>
      {children}
    </StructuredDialog>
  );
}
