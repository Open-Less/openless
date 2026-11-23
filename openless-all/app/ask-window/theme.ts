import type { AskTheme } from './client';

const THEME_KEY = 'ol.theme';

export function watchTheme() {
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)');
  let preferred: AskTheme = 'system';
  const setTheme = (preference: string | null) => {
    preferred = preference === 'light' || preference === 'dark' ? preference : 'system';
    paint();
  };
  function paint() {
    const dark = preferred === 'dark' || (preferred === 'system' && prefersDark.matches);
    const root = document.documentElement;
    root.classList.toggle('dark', dark);
    if (dark) root.dataset.olTheme = 'dark';
    else delete root.dataset.olTheme;
    root.style.colorScheme = dark ? 'dark' : 'light';
  }
  try {
    setTheme(window.localStorage.getItem(THEME_KEY));
  } catch {
    paint();
  }
  function onStorage(event: StorageEvent) {
    if (event.key === THEME_KEY) setTheme(event.newValue);
  }
  prefersDark.addEventListener('change', paint);
  window.addEventListener('storage', onStorage);
  return {
    setTheme,
    stop() {
      prefersDark.removeEventListener('change', paint);
      window.removeEventListener('storage', onStorage);
    },
  };
}
