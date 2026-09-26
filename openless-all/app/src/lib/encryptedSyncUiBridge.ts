// The main native window owns the device-local UI mirror. Every write shares
// this queue and a native revision, including the initial consent preparation.
import { invokeOrMock, isTauri } from './ipc/shared';
import {
  getLocalePreference,
  setLocalePreference,
  SUPPORTED_LOCALES,
  type SupportedLocale,
} from '../i18n';
import { readFontScale, setFontScale, type FontScaleId } from './fontScale';

type UiPreferences = { locale?: string; fontScale?: string };
type UiSnapshot = { preferences: UiPreferences | null; revision: string | null };
type UiKey = 'locale' | 'fontScale';
type Status = {
  sequence: string;
  account: { githubId: string } | null;
  vaultId: string | null;
  consentVersion: string | null;
};
type Restored = { sequence: string; accountId: string; vaultId: string; taskId: string | null };
let installation: Promise<void> | null = null;
let flushBridge: (() => Promise<void>) | null = null;

export async function flushEncryptedSyncUiPreferences(): Promise<void> {
  await installEncryptedSyncUiBridge();
  if (!flushBridge) throw new Error('cloud_sync_unavailable');
  await flushBridge();
}

export function installEncryptedSyncUiBridge(): Promise<void> {
  if (!isTauri || new URLSearchParams(location.search).has('window')) return Promise.resolve();
  return (installation ??= install());
}

async function install(): Promise<void> {
  let ready = false;
  let choiceEpoch = 0;
  let obsoleteThrough = 0;
  let restoredSequence = 0n;
  let restorationEpoch = 0;
  let settledRestorationEpoch = 0;
  let known: UiSnapshot = { preferences: null, revision: null };
  const choices = new Map<UiKey, { epoch: number; value: string }>();
  let queue: Promise<unknown> = Promise.resolve();
  const unavailable = (): never => {
    throw new Error('cloud_sync_unavailable');
  };
  const readStatus = () => invokeOrMock<Status>('cloud_sync_e2ee_status', undefined, unavailable);
  const readUi = () =>
    invokeOrMock<UiSnapshot>('cloud_sync_e2ee_get_ui_preferences_snapshot', undefined, unavailable);
  const sameScope = (a: Status, b: Status) =>
    a.account?.githubId === b.account?.githubId && a.vaultId === b.vaultId;
  const belongs = (state: Status, event: Restored) =>
    state.account?.githubId === event.accountId && state.vaultId === event.vaultId;
  const failed = (error: unknown) =>
    window.dispatchEvent(new CustomEvent('openless:sync-ui-persistence-failed', { detail: error }));
  const enqueue = (work: () => Promise<void>): Promise<void> => {
    const next = queue.catch(() => {}).then(work);
    queue = next.catch(failed);
    return next;
  };
  const invalidate = (through: number) => {
    obsoleteThrough = Math.max(obsoleteThrough, through);
    for (const [key, choice] of choices) if (choice.epoch <= through) choices.delete(key);
  };
  const apply = async (preferences: UiPreferences, expectedEpoch: number) => {
    if (
      (choices.get('locale')?.epoch ?? 0) <= expectedEpoch &&
      (preferences.locale === 'system' ||
        SUPPORTED_LOCALES.some((locale) => locale === preferences.locale))
    ) {
      await setLocalePreference(preferences.locale as SupportedLocale | 'system', 'sync-restore');
    }
    if (
      (choices.get('fontScale')?.epoch ?? 0) <= expectedEpoch &&
      ['small', 'medium', 'large'].includes(preferences.fontScale ?? '')
    ) {
      setFontScale(preferences.fontScale as FontScaleId, 'sync-restore');
    }
  };
  const reconcile = async (through: number) => {
    const current = await readUi();
    known = current;
    invalidate(through);
    if (current.preferences) await apply(current.preferences, through);
  };
  const persist = async (explicit: boolean, epoch: number) => {
    if (!explicit && (epoch !== choiceEpoch || epoch <= obsoleteThrough)) return;
    const status = await readStatus();
    if (!explicit && (epoch !== choiceEpoch || epoch <= obsoleteThrough || !status.consentVersion))
      return;
    const current = await readUi();
    if (restorationEpoch !== settledRestorationEpoch) {
      if (explicit) throw new Error('cloud_sync_ui_restore_pending');
      return;
    }
    // A restore or rollback may have happened without its notification reaching
    // this WebView. Rebase from native, but never replay the old user's intent
    // over that revision. A subsequent edit uses the refreshed revision.
    if (current.revision !== known.revision) {
      known = current;
      invalidate(epoch);
      if (current.preferences) await apply(current.preferences, epoch);
      throw new Error('cloud_sync_ui_revision_changed');
    }
    if (!explicit && epoch <= obsoleteThrough) return;
    const pending = [...choices].filter(
      ([, value]) => value.epoch > obsoleteThrough && value.epoch <= epoch,
    );
    if (!pending.length && current.preferences) return;
    const desired = {
      locale: getLocalePreference(),
      fontScale: readFontScale(),
      ...current.preferences,
    };
    for (const [key, choice] of pending) desired[key] = choice.value;
    try {
      await invokeOrMock<void>(
        'cloud_sync_e2ee_set_ui_preferences_checked',
        {
          ...desired,
          expectedRevision: current.revision,
        },
        unavailable,
      );
      await reconcile(epoch);
    } catch (error) {
      // Refresh even after CAS failure or an uncertain reply. Keeping the old
      // revision would make future edits fail until the WebView restarts.
      await reconcile(epoch);
      throw error;
    }
  };
  const changed = (event: Event) => {
    if (!ready || (event as CustomEvent<{ source?: string }>).detail?.source === 'sync-restore')
      return;
    const epoch = ++choiceEpoch;
    const values = { locale: getLocalePreference(), fontScale: readFontScale() };
    const detailKey = (event as CustomEvent<{ key?: UiKey }>).detail?.key;
    const storageKey = (event as StorageEvent).key;
    const keys: UiKey[] = detailKey
      ? [detailKey]
      : storageKey === 'ol-font-scale'
        ? ['fontScale']
        : storageKey
          ? ['locale']
          : ['locale', 'fontScale'];
    for (const key of keys) choices.set(key, { epoch, value: values[key] });
    void enqueue(() => persist(false, epoch)).catch(() => {});
  };
  const { listen } = await import('@tauri-apps/api/event');
  await listen<Restored>('cloud-sync-e2ee:restored', ({ payload }) => {
    if (
      !/^(0|[1-9]\d{0,19})$/.test(payload.sequence) ||
      BigInt(payload.sequence) <= restoredSequence
    )
      return;
    const eventChoiceEpoch = choiceEpoch;
    const notificationEpoch = ++restorationEpoch;
    // Pause old writes immediately, but retain their choices until native has
    // verified this event's scope. A late event from another vault must not
    // discard the current user's pending changes.
    void enqueue(async () => {
      try {
        if (BigInt(payload.sequence) <= restoredSequence) return;
        const before = await readStatus();
        if (!belongs(before, payload)) return;
        const current = await readUi();
        const after = await readStatus();
        if (
          !belongs(after, payload) ||
          !sameScope(before, after) ||
          BigInt(payload.sequence) <= restoredSequence
        )
          return;
        invalidate(eventChoiceEpoch);
        known = current;
        restoredSequence = BigInt(payload.sequence);
        if (current.preferences) await apply(current.preferences, eventChoiceEpoch);
      } finally {
        settledRestorationEpoch = notificationEpoch;
        if (notificationEpoch === restorationEpoch && choices.size > 0) {
          void enqueue(() => persist(false, choiceEpoch)).catch(() => {});
        }
      }
    }).catch(() => {});
  });
  window.addEventListener('openless:ui-preferences-changed', changed);
  window.addEventListener('storage', (event) => {
    if (event.key === 'ol-font-scale' || event.key?.includes('locale')) changed(event);
  });
  flushBridge = () => enqueue(() => persist(true, choiceEpoch));
  try {
    await enqueue(async () => {
      known = await readUi();
      if (known.preferences) await apply(known.preferences, choiceEpoch);
    });
  } catch {
    // Settings remain available to retry a denied keychain read.
  } finally {
    ready = true;
  }
}
