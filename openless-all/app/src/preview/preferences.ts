import type { ThemeMode } from '../api/contracts';
import { isPreviewHost } from '../runtime/environment';

export function readPreviewThemePreference(): ThemeMode {
  if (!isPreviewHost()) return 'system';
  try {
    const value = globalThis.window?.localStorage.getItem('ol.theme');
    if (value === 'light' || value === 'dark' || value === 'system') return value;
  } catch {
    // Browser storage may be unavailable; the system color scheme still works.
  }
  return 'system';
}
