export const FONT_CATALOG = [
  {
    id: 'system',
    name: 'System · 系统字体',
    category: 'sans',
    family: 'system-ui, -apple-system, sans-serif',
  },
  {
    id: 'humanist',
    name: 'Avenir Next · Aptos',
    category: 'sans',
    family: '"Avenir Next", Aptos, system-ui, sans-serif',
  },
  {
    id: 'cjk',
    name: 'PingFang · 苹方 / 微软雅黑',
    category: 'sans',
    family: '"PingFang SC", "Microsoft YaHei", system-ui, sans-serif',
  },
  {
    id: 'serif',
    name: 'New York · Georgia',
    category: 'serif',
    family: 'ui-serif, "New York", Georgia, "Songti SC", serif',
  },
  {
    id: 'song',
    name: 'Songti · 宋体',
    category: 'serif',
    family: '"Songti SC", SimSun, ui-serif, serif',
  },
  {
    id: 'mono',
    name: 'SF Mono · Cascadia Code',
    category: 'mono',
    family: '"SF Mono", "Cascadia Code", Consolas, monospace',
  },
  { id: 'menlo', name: 'Menlo · Consolas', category: 'mono', family: 'Menlo, Consolas, monospace' },
] as const;
export type FontId = (typeof FONT_CATALOG)[number]['id'];
export const FONT_ROLES = ['ui', 'body', 'title', 'code', 'numbers'] as const;
export type FontRole = (typeof FONT_ROLES)[number];
export type Typography = {
  version: 1;
  fonts: Record<FontRole, FontId>;
  width: 'narrow' | 'comfortable' | 'wide';
};
export const DEFAULT_TYPOGRAPHY: Typography = {
  version: 1,
  fonts: { ui: 'system', body: 'system', title: 'system', code: 'mono', numbers: 'system' },
  width: 'comfortable',
};
export const TYPOGRAPHY_PRESETS: Record<string, Typography> = {
  native: DEFAULT_TYPOGRAPHY,
  reading: { version: 1, fonts: { ...DEFAULT_TYPOGRAPHY.fonts, body: 'serif' }, width: 'narrow' },
  technical: {
    version: 1,
    fonts: { ...DEFAULT_TYPOGRAPHY.fonts, code: 'menlo', numbers: 'mono' },
    width: 'wide',
  },
};
const KEY = 'ol.typography.v1';
const RECENT = 'ol.typography.recent';
export function parseTypography(text: string): Typography {
  if (text.length > 16_384) throw new Error('typography-too-large');
  const value = JSON.parse(text);
  if (
    !value ||
    value.version !== 1 ||
    !['narrow', 'comfortable', 'wide'].includes(value.width) ||
    !FONT_ROLES.every((role) => FONT_CATALOG.some((font) => font.id === value.fonts?.[role]))
  )
    throw new Error('invalid-typography');
  return {
    version: 1,
    fonts: Object.fromEntries(
      FONT_ROLES.map((role) => [role, value.fonts[role]]),
    ) as Typography['fonts'],
    width: value.width,
  };
}
export function readTypography(): Typography {
  try {
    return parseTypography(localStorage.getItem(KEY) || '');
  } catch {
    return DEFAULT_TYPOGRAPHY;
  }
}
export function fontFamily(id: FontId) {
  return FONT_CATALOG.find((font) => font.id === id)!.family;
}
export function applyTypography(value: Typography) {
  const tokens = { ui: 'sans', body: 'body', title: 'display', code: 'mono', numbers: 'numeric' };
  for (const role of FONT_ROLES) {
    const token = `--ol-font-${tokens[role]}`;
    if (value.fonts[role] === DEFAULT_TYPOGRAPHY.fonts[role])
      document.documentElement.style.removeProperty(token);
    else document.documentElement.style.setProperty(token, fontFamily(value.fonts[role]));
  }
  document.documentElement.style.setProperty(
    '--ol-reading-width',
    { narrow: '56ch', comfortable: '72ch', wide: '88ch' }[value.width],
  );
}
export function saveTypography(value: Typography) {
  applyTypography(value);
  localStorage.setItem(KEY, JSON.stringify(value));
  window.dispatchEvent(new Event('openless:typography'));
}
export function recentFonts(): FontId[] {
  try {
    const value = JSON.parse(localStorage.getItem(RECENT) || '[]');
    return Array.isArray(value)
      ? value.filter((id) => FONT_CATALOG.some((font) => font.id === id)).slice(0, 5)
      : [];
  } catch {
    return [];
  }
}
export function rememberFont(id: FontId) {
  try {
    localStorage.setItem(
      RECENT,
      JSON.stringify([id, ...recentFonts().filter((item) => item !== id)].slice(0, 5)),
    );
  } catch {
    /* optional history */
  }
}
export function initTypography() {
  applyTypography(readTypography());
  window.addEventListener('storage', (event) => {
    if (event.key === KEY || event.key === null) applyTypography(readTypography());
  });
}
