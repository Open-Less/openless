// Preserve the local preference for both early startup and cloud restore. The viewport
// applies it to content and popup surfaces without changing the root coordinates.

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
  // A fresh install opens at 110% (user request: 100% read too small), the same on every
  // platform. An explicit choice above, including one restored by cloud sync, always wins.
  return 'large';
}

export function applyFontScale(id: FontScaleId): void {
  const scale = FONT_SCALE_VALUES[id];
  document.documentElement.style.setProperty('--ol-interface-scale', String(scale));
}

export function setFontScale(id: FontScaleId, source: 'user' | 'sync-restore' = 'user'): void {
  applyFontScale(id);
  try {
    window.localStorage.setItem(FONT_SCALE_KEY, id);
    window.dispatchEvent(
      new CustomEvent('openless:ui-preferences-changed', { detail: { source, key: 'fontScale' } }),
    );
  } catch {
    /* ignore */
  }
}
