import { detectOS } from '../components/WindowChrome';
import type { ThemeMode } from './types';

let revision = 0;
let pending: Promise<boolean> = Promise.resolve(false);

/** macOS appearance is application-wide. Only the main window may change it. */
export function syncMacOSWindowTheme(preference: ThemeMode): Promise<boolean> {
  if (
    detectOS() !== 'mac' ||
    (typeof navigator !== 'undefined' && /iPhone|iPad|iPod/.test(navigator.userAgent)) ||
    typeof window === 'undefined' ||
    !('__TAURI_INTERNALS__' in window)
  ) {
    return Promise.resolve(false);
  }

  const requestedRevision = ++revision;
  pending = pending
    .then(async () => {
      if (requestedRevision !== revision) return false;
      const [{ getCurrentWindow }, { setTheme }] = await Promise.all([
        import('@tauri-apps/api/window'),
        import('@tauri-apps/api/app'),
      ]);
      if (getCurrentWindow().label !== 'main' || requestedRevision !== revision) return false;
      // Passing the resolved system color would pin NSApplication's appearance
      // and prevent future OS changes from reaching prefers-color-scheme.
      await setTheme(preference === 'system' ? null : preference);
      return requestedRevision === revision;
    })
    .catch((error) => {
      console.warn('[theme] failed to synchronize macOS appearance', error);
      return false;
    });
  return pending;
}
