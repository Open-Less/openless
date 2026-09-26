import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { getHotkeyCapability, getSettings, isTauri, setSettings } from '../lib/ipc';
import type { HotkeyBinding, HotkeyCapability, UserPreferences } from '../lib/types';
import { applyThemeFromPreference } from '../lib/themeMode';
import { applyStackedLayoutFromPrefs } from '../lib/stackedLayout';
import { applyConservativeLayout } from '../lib/conservativeLayout';
import { emitSaved } from '../lib/savedEvent';
import { PreferencesWriteGate } from './preferencesWriteGate';

interface HotkeySettingsContextValue {
  prefs: UserPreferences | null;
  hotkey: HotkeyBinding | null;
  capability: HotkeyCapability | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  updatePrefs: (
    next: UserPreferences | ((current: UserPreferences) => UserPreferences),
  ) => Promise<void>;
}

const HotkeySettingsContext = createContext<HotkeySettingsContextValue | null>(null);

const errorMessage = (error: unknown) => String(error instanceof Error ? error.message : error);

type PreferenceKey = keyof UserPreferences;

const changedPreferenceEntries = (previous: UserPreferences, next: UserPreferences) => {
  const changes = new Map<PreferenceKey, UserPreferences[PreferenceKey]>();
  for (const key of Object.keys(next) as PreferenceKey[]) {
    if (!Object.is(previous[key], next[key])) {
      changes.set(key, next[key]);
    }
  }
  return changes;
};

const samePreferences = (left: UserPreferences, right: UserPreferences) =>
  JSON.stringify(left) === JSON.stringify(right);

export function HotkeySettingsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [capability, setCapability] = useState<HotkeyCapability | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const persistQueueRef = useRef<Promise<void>>(Promise.resolve());
  const latestPrefsRef = useRef<UserPreferences | null>(null);
  const persistedPrefsRef = useRef<UserPreferences | null>(null);
  const writeGateRef = useRef(new PreferencesWriteGate<UserPreferences>(samePreferences));
  const prefsChangeVersionRef = useRef(0);
  const pendingLocalChangesRef = useRef(new Map<PreferenceKey, UserPreferences[PreferenceKey]>());

  const applyIncomingPrefs = useCallback(
    (nextPrefs: UserPreferences, persistedPrefs = nextPrefs) => {
      prefsChangeVersionRef.current += 1;
      latestPrefsRef.current = nextPrefs;
      persistedPrefsRef.current = persistedPrefs;
      setPrefs(nextPrefs);
      applyThemeFromPreference(nextPrefs.themeMode ?? 'system');
      applyStackedLayoutFromPrefs(nextPrefs.stackedRowLayout);
      applyConservativeLayout(nextPrefs.conservativeLayout === true);
    },
    [],
  );

  const mergePendingLocalChanges = useCallback((incoming: UserPreferences) => {
    const merged = { ...incoming };
    for (const [key, value] of pendingLocalChangesRef.current) {
      Object.assign(merged, { [key]: value });
    }
    return merged;
  }, []);

  const waitForPersistence = useCallback(async () => {
    while (true) {
      const observedQueue = persistQueueRef.current;
      await observedQueue.catch(() => undefined);
      if (observedQueue === persistQueueRef.current) return;
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await waitForPersistence();
      const readVersion = prefsChangeVersionRef.current;
      const [prefsResult, capabilityResult] = await Promise.allSettled([
        getSettings(),
        getHotkeyCapability(),
      ]);
      let nextError: string | null = null;
      if (prefsResult.status === 'fulfilled') {
        if (prefsChangeVersionRef.current === readVersion) {
          const mergedPrefs = mergePendingLocalChanges(prefsResult.value);
          applyIncomingPrefs(mergedPrefs, prefsResult.value);
        }
      } else {
        console.error('[hotkey-settings] failed to load preferences', prefsResult.reason);
        nextError = errorMessage(prefsResult.reason);
      }
      if (capabilityResult.status === 'fulfilled') {
        setCapability(capabilityResult.value);
      } else {
        console.error(
          '[hotkey-settings] failed to load hotkey capability',
          capabilityResult.reason,
        );
        nextError = errorMessage(capabilityResult.reason);
      }
      setError(nextError);
    } catch (error) {
      console.error('[hotkey-settings] failed to refresh hotkey settings', error);
      setError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, [applyIncomingPrefs, mergePendingLocalChanges, waitForPersistence]);

  const queueSetSettings = useCallback(
    (
      resolved: UserPreferences,
      previous: UserPreferences | null = latestPrefsRef.current,
      trackLocalChanges = true,
    ) => {
      if (trackLocalChanges && previous) {
        for (const [key, value] of changedPreferenceEntries(previous, resolved)) {
          pendingLocalChangesRef.current.set(key, value);
        }
      }
      const finishWrite = writeGateRef.current.beginWrite(resolved);
      let savedPrefs: UserPreferences | null = null;
      const task = persistQueueRef.current
        .catch(() => undefined)
        .then(async () => {
          savedPrefs = await setSettings(resolved);
        })
        .then(() => {
          persistedPrefsRef.current = savedPrefs;
        })
        .finally(() => {
          if (finishWrite(savedPrefs ?? undefined)) {
            pendingLocalChangesRef.current.clear();
            if (savedPrefs) applyIncomingPrefs(savedPrefs);
          }
        });
      persistQueueRef.current = task;
      return task;
    },
    [applyIncomingPrefs],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen<UserPreferences>('prefs:changed', (event) => {
          const nextPrefs = event.payload;
          if (!nextPrefs) return;
          // 一次保存的广播先于其 IPC promise resolve 到达。若不拦截，较旧的
          // 排队保存会覆盖较新的乐观点击，让开关看起来「弹回去」。
          const incoming = writeGateRef.current.receiveIncoming(nextPrefs);
          if (incoming.isOwnWrite) return;

          const mergedPrefs = mergePendingLocalChanges(nextPrefs);
          applyIncomingPrefs(mergedPrefs, nextPrefs);
          if (incoming.wasPending) {
            // The queued full snapshot may still overwrite fields changed by
            // another window. Re-save the reconciled snapshot after it drains.
            void queueSetSettings(mergedPrefs, null, false).catch((error) => {
              console.warn('[settings] reconcile external preference change failed', error);
            });
          }
        });
        if (cancelled) {
          handle();
        } else {
          unlisten = handle;
        }
      } catch (error) {
        console.warn('[settings] prefs:changed listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [applyIncomingPrefs, mergePendingLocalChanges, queueSetSettings]);

  useEffect(() => {
    latestPrefsRef.current = prefs;
  }, [prefs]);

  const updatePrefs = useCallback(
    async (next: UserPreferences | ((current: UserPreferences) => UserPreferences)) => {
      const current = latestPrefsRef.current;
      if (!current) return;
      const resolved = typeof next === 'function' ? next(current) : next;
      if (resolved === current) return;
      setPrefs(resolved);
      prefsChangeVersionRef.current += 1;
      latestPrefsRef.current = resolved;
      applyStackedLayoutFromPrefs(resolved.stackedRowLayout);
      applyConservativeLayout(resolved.conservativeLayout === true);
      try {
        await queueSetSettings(resolved, current);
      } catch (error) {
        // 兜底（#904）：保存失败必须回滚乐观状态并可见，
        // 不能出现界面显示已切换、重启后回退的“假保存”。
        const fallback = persistedPrefsRef.current ?? current;
        latestPrefsRef.current = fallback;
        setPrefs(fallback);
        console.error('[hotkey-settings] save failed, rolled back', error);
        emitSaved('failed', errorMessage(error));
        throw error;
      }
    },
    [queueSetSettings],
  );

  const value = useMemo<HotkeySettingsContextValue>(
    () => ({
      prefs,
      hotkey: prefs?.hotkey ?? null,
      capability,
      loading,
      error,
      refresh,
      updatePrefs,
    }),
    [capability, error, loading, prefs, refresh, updatePrefs],
  );

  return <HotkeySettingsContext.Provider value={value}>{children}</HotkeySettingsContext.Provider>;
}

export function useHotkeySettings() {
  const value = useContext(HotkeySettingsContext);
  if (!value) {
    throw new Error('useHotkeySettings must be used within HotkeySettingsProvider');
  }
  return value;
}
