// User-selectable UI font family. Mirrors the localStorage-based approach in
// fontScale.ts. The selected stack is applied as a CSS custom property
// `--ol-font-sans-active` on documentElement; tokens.css falls back to the
// default cascade when that property is unset.
//
// Two modes:
//   - preset id ('auto' | 'notoSansJp' | 'yuGothic' | 'meiryo' | 'hiragino')
//     → applies a curated stack
//   - 'custom' id with a free-form font name → applies that family followed
//     by the rest of the default cascade. Use this together with the
//     Web `window.queryLocalFonts()` API so users can pick any font that's
//     actually installed on their machine.

export type FontFamilyId =
  | 'auto'
  | 'notoSansJp'
  | 'yuGothic'
  | 'meiryo'
  | 'hiragino'
  | 'custom';

const PRESET_STACKS: Record<Exclude<FontFamilyId, 'auto' | 'custom'>, string> = {
  notoSansJp: "'Noto Sans JP', sans-serif",
  yuGothic: "'Yu Gothic UI', 'Yu Gothic', 'Meiryo', sans-serif",
  meiryo: "'Meiryo', sans-serif",
  hiragino: "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', sans-serif",
};

const FALLBACK_TAIL = "'Microsoft YaHei', system-ui, sans-serif";

const FONT_FAMILY_KEY = 'ol-font-family';
const FONT_FAMILY_CUSTOM_KEY = 'ol-font-family-custom';

function escapeFontName(name: string): string {
  // Quote the family name; CSS allows unquoted single-word names but anything
  // with spaces or punctuation must be quoted. Always quote to keep things
  // predictable. Replace any embedded single quotes defensively.
  return `'${name.replace(/'/g, '\\\'')}'`;
}

export function readFontFamily(): FontFamilyId {
  try {
    const v = window.localStorage.getItem(FONT_FAMILY_KEY);
    if (
      v === 'auto' ||
      v === 'notoSansJp' ||
      v === 'yuGothic' ||
      v === 'meiryo' ||
      v === 'hiragino' ||
      v === 'custom'
    ) {
      return v;
    }
  } catch {
    /* localStorage unavailable: ignore */
  }
  return 'auto';
}

export function readFontFamilyCustom(): string {
  try {
    return window.localStorage.getItem(FONT_FAMILY_CUSTOM_KEY) ?? '';
  } catch {
    return '';
  }
}

export function applyFontFamily(id: FontFamilyId, customName?: string): void {
  const root = document.documentElement;
  if (id === 'auto') {
    root.style.removeProperty('--ol-font-sans-active');
    return;
  }
  if (id === 'custom') {
    const name = (customName ?? '').trim();
    if (!name) {
      root.style.removeProperty('--ol-font-sans-active');
      return;
    }
    root.style.setProperty(
      '--ol-font-sans-active',
      `'Inter', ${escapeFontName(name)}, ${FALLBACK_TAIL}`,
    );
    return;
  }
  const stack = PRESET_STACKS[id];
  root.style.setProperty(
    '--ol-font-sans-active',
    `'Inter', ${stack}, ${FALLBACK_TAIL}`,
  );
}

export function setFontFamily(id: FontFamilyId, customName?: string): void {
  applyFontFamily(id, customName);
  try {
    window.localStorage.setItem(FONT_FAMILY_KEY, id);
    if (id === 'custom' && customName !== undefined) {
      window.localStorage.setItem(FONT_FAMILY_CUSTOM_KEY, customName);
    }
  } catch {
    /* ignore */
  }
}

/**
 * Enumerate fonts installed on the host via the Web Local Font Access API.
 * Returns unique family names sorted alphabetically. Returns an empty array
 * (without throwing) when the API is unavailable or the user denied access;
 * the caller should treat that as "feature not available" and keep the
 * default presets.
 *
 * Must be called from a user gesture (button click) — Chromium gates this
 * API behind a one-time permission prompt.
 */
export async function querySystemFonts(): Promise<string[]> {
  type LocalFont = { family: string };
  const w = window as unknown as {
    queryLocalFonts?: () => Promise<LocalFont[]>;
  };
  if (typeof w.queryLocalFonts !== 'function') return [];
  try {
    const fonts = await w.queryLocalFonts();
    const families = new Set<string>();
    for (const f of fonts) {
      if (f.family) families.add(f.family);
    }
    return Array.from(families).sort((a, b) =>
      a.localeCompare(b, 'ja', { sensitivity: 'base' }),
    );
  } catch (err) {
    // Permission denied or API blocked — caller falls back to manual entry.
    // eslint-disable-next-line no-console
    console.warn('[fontFamily] queryLocalFonts failed:', err);
    return [];
  }
}

// Apply the saved choice on module load so the override takes effect before
// the first paint. main.tsx imports this for its side effect.
applyFontFamily(readFontFamily(), readFontFamilyCustom());
