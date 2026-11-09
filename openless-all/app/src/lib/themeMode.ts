// Theme preference: localStorage boot cache + Rust UserPreferences.theme_mode authority.
// Applies via html[data-ol-theme] + CSS variables — no React re-render for color changes.

import { getSettings } from './ipc/settings';
import { isTauri } from './ipc/shared';
import { syncWindowsCaptionTheme } from './platform';
import { syncMacOSWindowTheme } from './nativeWindowTheme';
import type { ThemeMode, UserPreferences } from './types';

export type ThemePreference = ThemeMode;
export type ResolvedTheme = 'light' | 'dark';

const THEME_KEY = 'ol.theme';

const SYSTEM_MEDIA = '(prefers-color-scheme: dark)';

let systemListener: MediaQueryList | null = null;
let systemHandler: ((event: MediaQueryListEvent) => void) | null = null;
let activePreference: ThemePreference = 'system';
let preferenceRevision = 0;
let initialized = false;

function readStoredPreference(): ThemePreference {
  try {
    const value = window.localStorage.getItem(THEME_KEY);
    if (value === 'system' || value === 'light' || value === 'dark') return value;
  } catch {
    /* ignore */
  }
  return 'system';
}

function writeStoredPreference(pref: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_KEY, pref);
  } catch {
    /* ignore */
  }
}

export function resolveTheme(pref: ThemePreference): ResolvedTheme {
  if (pref === 'light') return 'light';
  if (pref === 'dark') return 'dark';
  if (typeof window.matchMedia === 'function') {
    return window.matchMedia(SYSTEM_MEDIA).matches ? 'dark' : 'light';
  }
  return 'light';
}

export function readThemePreference(): ThemePreference {
  return activePreference;
}

function attachSystemListener(): void {
  if (typeof window.matchMedia !== 'function') return;
  if (systemListener) return;

  systemListener = window.matchMedia(SYSTEM_MEDIA);
  systemHandler = () => {
    if (activePreference !== 'system') return;
    applyThemeMode(resolveTheme('system'));
  };
  systemListener.addEventListener('change', systemHandler);
}

function detachSystemListener(): void {
  if (systemListener && systemHandler) {
    systemListener.removeEventListener('change', systemHandler);
  }
  systemListener = null;
  systemHandler = null;
}

export function applyThemeMode(resolved: ResolvedTheme): void {
  const root = document.documentElement;
  if (resolved === 'dark') {
    root.dataset.olTheme = 'dark';
  } else {
    delete root.dataset.olTheme;
  }
  root.style.colorScheme = resolved;
  void syncWindowsCaptionTheme(resolved === 'dark');
}

function syncNativeAppearance(preference: ThemePreference): void {
  void syncMacOSWindowTheme(preference).then((applied) => {
    // matchMedia may still reflect the old explicit native appearance until
    // NSApplication's override has been cleared. Re-read it after that change.
    if (applied && preference === 'system' && activePreference === preference) {
      applyThemeMode(resolveTheme(preference));
    }
  });
}

export function applyThemeFromPreference(pref: ThemePreference): void {
  preferenceRevision++;
  activePreference = pref;
  writeStoredPreference(pref);
  if (pref === 'system') {
    attachSystemListener();
  } else {
    detachSystemListener();
  }
  applyThemeMode(resolveTheme(pref));
  syncNativeAppearance(pref);
}

export function setThemePreference(pref: ThemePreference): void {
  applyThemeFromPreference(pref);
}

export function initThemeMode(): void {
  if (initialized) return;
  initialized = true;
  activePreference = readStoredPreference();
  if (activePreference === 'system') {
    attachSystemListener();
  }
  applyThemeMode(resolveTheme(activePreference));

  if (!isTauri) return;
  const requestedRevision = preferenceRevision;
  // This subscription belongs to the WebView lifetime, including hidden capsules
  // and transcript mirrors that never mount HotkeySettingsProvider.
  void (async () => {
    try {
      const { listen } = await import('@tauri-apps/api/event');
      await listen<Partial<UserPreferences>>('prefs:changed', ({ payload }) => {
        const preference = payload?.themeMode;
        if (preference === 'light' || preference === 'dark' || preference === 'system') {
          applyThemeFromPreference(preference);
        }
      });
    } catch (error) {
      console.warn('[theme] failed to subscribe to preference changes', error);
    }
    // Subscribe before the boot read. A newer event or local choice invalidates
    // this response even if a hidden WebView's initial IPC finishes late.
    return getSettings();
  })()
    .then((prefs) => {
      if (requestedRevision !== preferenceRevision) return;
      const rustPref = prefs.themeMode ?? 'system';
      // The boot cache paints the page only. Rust has already initialized
      // native appearance, so a stale cache must never overwrite it.
      if (rustPref === activePreference) {
        syncNativeAppearance(rustPref);
      } else {
        applyThemeFromPreference(rustPref);
      }
    })
    .catch((error) => {
      console.warn('[theme] failed to reconcile theme from preferences', error);
    });
}
