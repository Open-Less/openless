// Each UI locale is a bundled local chunk. main.tsx waits for i18nReady so the
// first frame already uses the saved language, without briefly showing fallback text.
// The UI preference is independent from recognition and translation languages.

import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { zhCN } from './zh-CN';
import { setRemoteLocale } from '../lib/ipc';

export const SUPPORTED_LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'es', 'fr', 'de'] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const LOCALE_STORAGE_KEY = 'ol.locale';
const FOLLOW_SYSTEM_VALUE = 'system';

function detectSystemLocale(): SupportedLocale {
  if (typeof navigator === 'undefined') return 'zh-CN';
  const nav = (navigator.language || '').toLowerCase();
  if (nav.startsWith('zh')) {
    if (nav.includes('hant') || nav.includes('tw') || nav.includes('hk') || nav.includes('mo'))
      return 'zh-TW';
    return 'zh-CN';
  }
  const primary = nav.split('-')[0];
  if (SUPPORTED_LOCALES.some((locale) => locale === primary)) return primary as SupportedLocale;
  return 'en';
}

function resolveLocalePreference(
  pref: SupportedLocale | typeof FOLLOW_SYSTEM_VALUE,
): SupportedLocale {
  if (pref === FOLLOW_SYSTEM_VALUE) return detectSystemLocale();
  return pref;
}

function getStoredLocale(): SupportedLocale | null {
  if (typeof window === 'undefined') return null;
  const raw = window.localStorage.getItem(LOCALE_STORAGE_KEY);
  return SUPPORTED_LOCALES.some((locale) => locale === raw) ? (raw as SupportedLocale) : null;
}

const initialLng: SupportedLocale = getStoredLocale() ?? detectSystemLocale();

// Language files remain separate local chunks; Node tests use the same explicit loaders.
const localeLoaders: Record<Exclude<SupportedLocale, 'zh-CN'>, () => Promise<typeof zhCN>> = {
  'zh-TW': () => import('./zh-TW').then((module) => module.zhTW),
  en: () => import('./en').then((module) => module.en),
  ja: () => import('./ja').then((module) => module.ja),
  ko: () => import('./ko').then((module) => module.ko),
  es: () => import('./es').then((module) => module.es),
  fr: () => import('./fr').then((module) => module.fr),
  de: () => import('./de').then((module) => module.de),
};

async function ensureLocaleLoaded(lng: SupportedLocale): Promise<void> {
  if (lng === 'zh-CN' || i18n.hasResourceBundle(lng, 'translation')) return;
  try {
    i18n.addResourceBundle(lng, 'translation', await localeLoaders[lng]());
  } catch (e) {
    console.warn('[i18n] load locale failed, staying on zh-CN fallback:', lng, e);
  }
}

const initialReady = i18n.use(initReactI18next).init({
  // Simplified Chinese is the canonical fallback; the selected chunk loads before rendering.
  resources: { 'zh-CN': { translation: zhCN } },
  lng: initialLng,
  fallbackLng: 'zh-CN',
  supportedLngs: SUPPORTED_LOCALES as unknown as string[],
  partialBundledLanguages: true, // tell i18next the inline resources are complete; no backend fetch needed
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
});

function applyDocumentLocale(locale: string) {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = locale;
  document.documentElement.dir = 'ltr';
}
i18n.on('languageChanged', applyDocumentLocale);
let localeRequest = 0;

async function applyLocale(resolved: SupportedLocale): Promise<void> {
  const request = ++localeRequest;
  await ensureLocaleLoaded(resolved);
  if (request !== localeRequest) return;
  await i18n.changeLanguage(resolved);
  syncRemoteLocale(resolved);
}

export const i18nReady = initialReady.then(async () => {
  await applyLocale(getStoredLocale() ?? detectSystemLocale());
});

// Auxiliary WebViews stay in the same language when the main window changes it.
if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.key === null || event.key === LOCALE_STORAGE_KEY) {
      void applyLocale(resolveLocalePreference(getLocalePreference()));
    }
  });
}

export default i18n;

/**
 * The current persisted preference. 'system' means follow the system; a concrete
 * language tag means the user chose explicitly. Differs from i18n.language, which is
 * always the resolved concrete language.
 */
export function getLocalePreference(): SupportedLocale | typeof FOLLOW_SYSTEM_VALUE {
  return getStoredLocale() ?? FOLLOW_SYSTEM_VALUE;
}

/**
 * Writes the user preference and switches the i18n language immediately.
 * pref === 'system' clears the stored item and re-detects via navigator.
 */
export async function setLocalePreference(
  pref: SupportedLocale | typeof FOLLOW_SYSTEM_VALUE,
  source: 'user' | 'sync-restore' = 'user',
): Promise<SupportedLocale> {
  const resolved = resolveLocalePreference(pref);
  if (pref === FOLLOW_SYSTEM_VALUE) {
    window.localStorage.removeItem(LOCALE_STORAGE_KEY);
  } else {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, pref);
  }
  window.dispatchEvent(new CustomEvent('openless:ui-preferences-changed', { detail: { source, key: 'locale' } }));
  await applyLocale(resolved);
  return resolved;
}

// The remote-input H5 recording page follows the PC UI language: push the resolved
// locale to the backend (it keeps only an in-memory mirror, used when the H5 page
// requests its index). Non-Tauri (browser dev) hits a mock no-op; failures are silent.
function syncRemoteLocale(resolved: SupportedLocale): void {
  void setRemoteLocale(resolved).catch(() => {});
}

export const FOLLOW_SYSTEM = FOLLOW_SYSTEM_VALUE;
