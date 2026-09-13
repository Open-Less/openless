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

export function HotkeySettingsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [capability, setCapability] = useState<HotkeyCapability | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const persistQueueRef = useRef<Promise<void>>(Promise.resolve());
  const latestPrefsRef = useRef<UserPreferences | null>(null);
  const persistedPrefsRef = useRef<UserPreferences | null>(null);
  const writeGateRef = useRef(new PreferencesWriteGate<UserPreferences>());

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [prefsResult, capabilityResult] = await Promise.allSettled([
        getSettings(),
        getHotkeyCapability(),
      ]);
      let nextError: string | null = null;
      if (prefsResult.status === 'fulfilled') {
        latestPrefsRef.current = prefsResult.value;
        persistedPrefsRef.current = prefsResult.value;
        setPrefs(prefsResult.value);
        applyThemeFromPreference(prefsResult.value.themeMode ?? 'system');
        applyStackedLayoutFromPrefs(prefsResult.value.stackedRowLayout);
        applyConservativeLayout(prefsResult.value.conservativeLayout === true);
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
  }, []);

  const applyIncomingPrefs = useCallback((nextPrefs: UserPreferences) => {
    latestPrefsRef.current = nextPrefs;
    persistedPrefsRef.current = nextPrefs;
    setPrefs(nextPrefs);
    applyThemeFromPreference(nextPrefs.themeMode ?? 'system');
    applyStackedLayoutFromPrefs(nextPrefs.stackedRowLayout);
    applyConservativeLayout(nextPrefs.conservativeLayout === true);
  }, []);

  const queueSetSettings = useCallback(
    (resolved: UserPreferences) => {
      const finishWrite = writeGateRef.current.beginWrite();
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
          if (finishWrite() && savedPrefs) applyIncomingPrefs(savedPrefs);
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
          const applicable = writeGateRef.current.receiveIncoming(nextPrefs);
          if (applicable) applyIncomingPrefs(applicable);
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
  }, [applyIncomingPrefs]);

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
      latestPrefsRef.current = resolved;
      applyStackedLayoutFromPrefs(resolved.stackedRowLayout);
      applyConservativeLayout(resolved.conservativeLayout === true);
      try {
        await queueSetSettings(resolved);
        persistedPrefsRef.current = resolved;
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
