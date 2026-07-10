// Theme preference: localStorage boot cache + Rust UserPreferences.theme_mode authority.
// Applies via html[data-ol-theme] + CSS variables — no React re-render for color changes.

import { getSettings, isTauri } from './ipc';
import { syncWindowsCaptionTheme } from './platform';
import type { ThemeMode } from './types';

export type ThemePreference = ThemeMode;
export type ResolvedTheme = 'light' | 'dark';

const THEME_KEY = 'ol.theme';

const SYSTEM_MEDIA = '(prefers-color-scheme: dark)';

let systemListener: MediaQueryList | null = null;
let systemHandler: ((event: MediaQueryListEvent) => void) | null = null;
let activePreference: ThemePreference = 'system';

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

export function applyThemeFromPreference(pref: ThemePreference): void {
  activePreference = pref;
  writeStoredPreference(pref);
  if (pref === 'system') {
    attachSystemListener();
  } else {
    detachSystemListener();
  }
  applyThemeMode(resolveTheme(pref));
}

export function setThemePreference(pref: ThemePreference): void {
  applyThemeFromPreference(pref);
}

export function initThemeMode(): void {
  activePreference = readStoredPreference();
  if (activePreference === 'system') {
    attachSystemListener();
  }
  applyThemeMode(resolveTheme(activePreference));

  if (!isTauri) return;
  void getSettings()
    .then((prefs) => {
      const rustPref = prefs.themeMode ?? 'system';
      if (rustPref === activePreference) return;
      applyThemeFromPreference(rustPref);
    })
    .catch((error) => {
      console.warn('[theme] failed to reconcile theme from preferences', error);
    });
}
