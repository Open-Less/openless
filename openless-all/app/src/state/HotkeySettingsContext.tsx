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
import { getHotkeyCapability, getSettingsSnapshot, isTauri, updateSettingFields } from '../lib/ipc';
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
  const latestPrefsRef = useRef<UserPreferences | null>(null);
  const persistQueueRef = useRef<Promise<void>>(Promise.resolve());
  const gate = useRef(new PreferencesWriteGate<UserPreferences>());
  const readRequest = useRef<Promise<void> | null>(null);
  const readAgain = useRef(false);

  const applyPrefs = useCallback((value: UserPreferences) => {
    latestPrefsRef.current = value;
    setPrefs(value);
    applyThemeFromPreference(value.themeMode ?? 'system');
    applyStackedLayoutFromPrefs(value.stackedRowLayout);
    applyConservativeLayout(value.conservativeLayout === true);
  }, []);

  // Events invalidate the snapshot; values alone cannot identify who wrote them.
  const reloadPreferences = useCallback((): Promise<void> => {
    readAgain.current = true;
    if (readRequest.current) return readRequest.current;
    const task = (async () => {
      do {
        readAgain.current = false;
        applyPrefs(gate.current.receiveIncoming(await getSettingsSnapshot()));
      } while (readAgain.current);
    })().finally(() => {
      readRequest.current = null;
    });
    readRequest.current = task;
    return task;
  }, [applyPrefs]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [, nextCapability] = await Promise.all([reloadPreferences(), getHotkeyCapability()]);
      setCapability(nextCapability);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, [reloadPreferences]);

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
        const stop = await listen('prefs:changed', () => {
          void reloadPreferences().catch((error) => setError(errorMessage(error)));
        });
        if (cancelled) stop();
        else unlisten = stop;
      } catch (error) {
        setError(errorMessage(error));
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [reloadPreferences]);

  const updatePrefs = useCallback(
    async (next: UserPreferences | ((current: UserPreferences) => UserPreferences)) => {
      const previous = latestPrefsRef.current;
      if (!previous) return;
      const resolved = typeof next === 'function' ? next(previous) : next;
      const write = gate.current.beginWrite(previous, resolved);
      if (Object.keys(write.edits).length === 0) {
        gate.current.finishWrite(write.id);
        return;
      }
      applyPrefs(gate.current.current());
      const task = persistQueueRef.current
        .catch(() => undefined)
        .then(async () => {
          try {
            const saved = await updateSettingFields(write.edits);
            applyPrefs(gate.current.finishWrite(write.id, saved));
          } catch (error) {
            applyPrefs(gate.current.finishWrite(write.id));
            emitSaved('failed', errorMessage(error));
            throw error;
          }
        });
      persistQueueRef.current = task;
      await task;
    },
    [applyPrefs],
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
