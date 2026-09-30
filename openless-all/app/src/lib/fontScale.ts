// Global font-size tiers — scales the whole UI via documentElement.style.zoom (supported by
// WebKit/Tauri). localStorage is the single source of truth; it's read once at window
// startup, and changes made in Settings' "personalization" overwrite it directly.

export type FontScaleId = 'small' | 'medium' | 'large';

export const FONT_SCALE_VALUES: Record<FontScaleId, number> = {
  small: 0.9,
  medium: 1.0,
  large: 1.1,
};

const FONT_SCALE_KEY = 'ol-font-scale';

export function readFontScale(): FontScaleId {
  try {
    const v = window.localStorage.getItem(FONT_SCALE_KEY);
    if (v === 'small' || v === 'medium' || v === 'large') return v;
  } catch {
    /* localStorage unavailable: ignore, fall back to the default */
  }
  // Windows defaults to 'large' (user feedback: medium reads too small on Windows); other
  // platforms keep medium.
  if (typeof navigator !== 'undefined') {
    const hint = `${navigator.userAgent || ''} ${navigator.platform || ''}`;
    if (/Windows|Win32|Win64/.test(hint)) return 'large';
  }
  return 'medium';
}

export function applyFontScale(id: FontScaleId): void {
  const scale = FONT_SCALE_VALUES[id];
  // CSS zoom isn't in the W3C standard but WebKit/Blink both support it; Tauri desktop runs
  // Wry/WebKit, so it's fine.
  (document.documentElement.style as CSSStyleDeclaration & { zoom?: string }).zoom = String(scale);
}

export function setFontScale(id: FontScaleId, source: 'user' | 'sync-restore' = 'user'): void {
  applyFontScale(id);
  try {
    window.localStorage.setItem(FONT_SCALE_KEY, id);
    window.dispatchEvent(new CustomEvent('openless:ui-preferences-changed', { detail: { source, key: 'fontScale' } }));
  } catch {
    /* ignore */
  }
}
