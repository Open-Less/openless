import { readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';

const root = new URL('../', import.meta.url);

async function read(relPath) {
  return readFile(new URL(relPath, root), 'utf8');
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function extractCssBlock(css, selector) {
  const escapedSelector = escapeRegExp(selector);
  const match = css.match(new RegExp(`${escapedSelector}\\s*\\{([\\s\\S]*?)\\n\\}`));
  assert.ok(match, `tokens.css must contain ${selector} block`);
  return match[1];
}

function parseCustomProperties(block) {
  const tokens = new Map();
  const re = /^\s*(--[\w-]+)\s*:\s*([^;]+);/gm;
  let match;
  while ((match = re.exec(block)) !== null) {
    tokens.set(match[1], match[2].trim());
  }
  return tokens;
}

function resolveTokenValue(name, tokens, stack = new Set()) {
  const raw = tokens.get(name);
  assert.ok(raw, `missing token ${name}`);
  if (!raw.startsWith('var(')) {
    return raw;
  }
  const inner = raw.slice(4, -1).trim();
  const refName = inner.split(',')[0].trim();
  assert.ok(refName.startsWith('--'), `unsupported var() reference in ${name}: ${raw}`);
  if (stack.has(refName)) {
    throw new Error(`circular var() reference: ${[...stack, refName].join(' -> ')}`);
  }
  stack.add(refName);
  return resolveTokenValue(refName, tokens, stack);
}

function parseCssColor(value) {
  const hexMatch = value.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hexMatch) {
    let hex = hexMatch[1];
    if (hex.length === 3) {
      hex = hex
        .split('')
        .map((ch) => ch + ch)
        .join('');
    }
    return {
      r: Number.parseInt(hex.slice(0, 2), 16),
      g: Number.parseInt(hex.slice(2, 4), 16),
      b: Number.parseInt(hex.slice(4, 6), 16),
      a: 1,
    };
  }

  const rgbaMatch = value.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/i);
  if (rgbaMatch) {
    return {
      r: Number(rgbaMatch[1]),
      g: Number(rgbaMatch[2]),
      b: Number(rgbaMatch[3]),
      a: rgbaMatch[4] === undefined ? 1 : Number(rgbaMatch[4]),
    };
  }

  throw new Error(`unsupported color format: ${value}`);
}

function srgbChannel(value) {
  const normalized = value / 255;
  return normalized <= 0.03928 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance({ r, g, b }) {
  const R = srgbChannel(r);
  const G = srgbChannel(g);
  const B = srgbChannel(b);
  return 0.2126 * R + 0.7152 * G + 0.0722 * B;
}

function contrastRatio(foreground, background) {
  const fg = parseCssColor(foreground);
  const bg = parseCssColor(background);
  assert.equal(fg.a, 1, `foreground must be opaque for contrast checks: ${foreground}`);
  assert.equal(bg.a, 1, `background must be opaque for contrast checks: ${background}`);
  const lighter = Math.max(relativeLuminance(fg), relativeLuminance(bg));
  const darker = Math.min(relativeLuminance(fg), relativeLuminance(bg));
  return (lighter + 0.05) / (darker + 0.05);
}

function compositeOverBackground(fgValue, bgValue) {
  const fg = parseCssColor(fgValue);
  const bg = parseCssColor(bgValue);
  if (fg.a === 1) {
    return fgValue;
  }
  const alpha = fg.a;
  const r = Math.round(fg.r * alpha + bg.r * (1 - alpha));
  const g = Math.round(fg.g * alpha + bg.g * (1 - alpha));
  const b = Math.round(fg.b * alpha + bg.b * (1 - alpha));
  return `rgb(${r}, ${g}, ${b})`;
}

function contrastRatioOverBackground(foreground, background) {
  const effectiveFg = compositeOverBackground(foreground, background);
  return contrastRatio(effectiveFg, background);
}

function assertSolidContrast(tokens, label, bgToken, inkToken, minRatio = 4.5) {
  const bg = resolveTokenValue(bgToken, tokens);
  const ink = resolveTokenValue(inkToken, tokens);
  const ratio = contrastRatio(ink, bg);
  assert.ok(
    ratio >= minRatio,
    `${label}: ${inkToken} on ${bgToken} must meet WCAG AA (${ratio.toFixed(2)}:1 < ${minRatio}:1)`,
  );
  return ratio;
}

function assertMutedContrast(tokens, label, bgToken, inkToken, minRatio = 4.5) {
  const bg = resolveTokenValue(bgToken, tokens);
  const ink = resolveTokenValue(inkToken, tokens);
  const ratio = contrastRatioOverBackground(ink, bg);
  assert.ok(
    ratio >= minRatio,
    `${label}: ${inkToken} on ${bgToken} must meet WCAG AA (${ratio.toFixed(2)}:1 < ${minRatio}:1)`,
  );
  return ratio;
}

function assertUsesClassName(source, className, message) {
  const escapedClassName = escapeRegExp(className);
  const patterns = [
    new RegExp(`className\\s*=\\s*(?:\\{\\s*)?"(?:[^"]*\\s)?${escapedClassName}(?:\\s[^"]*)?"(?:\\s*\\})?`),
    new RegExp(`className\\s*=\\s*(?:\\{\\s*)?'(?:[^']*\\s)?${escapedClassName}(?:\\s[^']*)?'(?:\\s*\\})?`),
    new RegExp(`className\\s*=\\s*(?:\\{\\s*)?\`(?:[^\`]*\\s)?${escapedClassName}(?:\\s[^\`]*)?\`(?:\\s*\\})?`),
  ];

  assert.ok(patterns.some((pattern) => pattern.test(source)), message);
}

const srcRoot = fileURLToPath(new URL('src/', root));

async function walkSourceFiles(dirPath, files = []) {
  const entries = await readdir(dirPath, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      await walkSourceFiles(entryPath, files);
      continue;
    }
    if (/\.(tsx?|css)$/.test(entry.name)) {
      files.push(path.relative(srcRoot, entryPath).replace(/\\/g, '/'));
    }
  }
  return files;
}

assert.throws(
  () => assertUsesClassName('<div>ol-app-shell-bg</div>', 'ol-app-shell-bg', 'sample must require className usage'),
  /sample must require className usage/,
);
assert.throws(
  () =>
    assertUsesClassName(
      '<div className="foo-ol-app-shell-bg-bar" />',
      'ol-app-shell-bg',
      'sample must require an exact class token',
    ),
  /sample must require an exact class token/,
);
assertUsesClassName(
  '<div className="foo ol-app-shell-bg bar" />',
  'ol-app-shell-bg',
  'sample should accept className usage',
);

const [tokens, globalCss, shell, settingsModal, overview, settingsTabs, themeMode, stylePage, sourceFiles, remoteStyle, selectLite, translationPage] = await Promise.all([
  read('src/styles/tokens.css'),
  read('src/styles/global.css'),
  read('src/components/FloatingShell.tsx'),
  read('src/components/SettingsModal.tsx'),
  read('src/pages/Overview.tsx'),
  read('src/pages/settings/tabs.tsx'),
  read('src/lib/themeMode.ts'),
  read('src/pages/Style.tsx'),
  walkSourceFiles(srcRoot),
  read('src-tauri/src/remote_server/assets/style.css'),
  read('src/components/ui/SelectLite.tsx'),
  read('src/pages/Translation.tsx'),
]);

assert.match(tokens, /--ol-shell-radius:/, 'tokens.css must define --ol-shell-radius');
assert.match(tokens, /--ol-panel-radius:/, 'tokens.css must define --ol-panel-radius');
assert.match(tokens, /--ol-aura-shadow:/, 'tokens.css must define --ol-aura-shadow');
assert.match(tokens, /--ol-font-display:/, 'tokens.css must define --ol-font-display');
assert.match(tokens, /--ol-on-accent:/, 'tokens.css must define --ol-on-accent');
assert.match(tokens, /--ol-primary-solid-bg:/, 'tokens.css must define --ol-primary-solid-bg');
assert.match(tokens, /--ol-primary-solid-ink:/, 'tokens.css must define --ol-primary-solid-ink');
assert.match(tokens, /--ol-control-radius:/, 'tokens.css must define --ol-control-radius');
assert.match(tokens, /--ol-accent-solid-bg:/, 'tokens.css must define --ol-accent-solid-bg');
assert.match(tokens, /--ol-accent-solid-bg-hover:/, 'tokens.css must define --ol-accent-solid-bg-hover');
assert.match(tokens, /--ol-accent-solid-ink:/, 'tokens.css must define --ol-accent-solid-ink');
assert.match(tokens, /--ol-danger-solid-bg:/, 'tokens.css must define --ol-danger-solid-bg');
assert.match(tokens, /--ol-danger-solid-ink:/, 'tokens.css must define --ol-danger-solid-ink');

assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-accent-solid-bg:/,
  'tokens.css must define --ol-accent-solid-bg in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-accent-solid-bg-hover:/,
  'tokens.css must define --ol-accent-solid-bg-hover in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-accent-solid-ink:/,
  'tokens.css must define --ol-accent-solid-ink in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-danger-solid-bg:/,
  'tokens.css must define --ol-danger-solid-bg in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-danger-solid-ink:/,
  'tokens.css must define --ol-danger-solid-ink in dark theme',
);

assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-primary-solid-bg:/,
  'tokens.css must define --ol-primary-solid-bg in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-primary-solid-ink:/,
  'tokens.css must define --ol-primary-solid-ink in dark theme',
);

const lightTokens = parseCustomProperties(extractCssBlock(tokens, ':root'));
const darkTokens = parseCustomProperties(extractCssBlock(tokens, "[data-ol-theme='dark']"));

const contrastPairs = [
  {
    label: 'light accent-solid',
    tokens: lightTokens,
    bg: '--ol-accent-solid-bg',
    ink: '--ol-accent-solid-ink',
  },
  {
    label: 'light primary-solid',
    tokens: lightTokens,
    bg: '--ol-primary-solid-bg',
    ink: '--ol-primary-solid-ink',
  },
  {
    label: 'dark accent-solid',
    tokens: darkTokens,
    bg: '--ol-accent-solid-bg',
    ink: '--ol-accent-solid-ink',
  },
  {
    label: 'dark primary-solid',
    tokens: darkTokens,
    bg: '--ol-primary-solid-bg',
    ink: '--ol-primary-solid-ink',
  },
  {
    label: 'light danger-solid',
    tokens: lightTokens,
    bg: '--ol-danger-solid-bg',
    ink: '--ol-danger-solid-ink',
  },
  {
    label: 'dark danger-solid',
    tokens: darkTokens,
    bg: '--ol-danger-solid-bg',
    ink: '--ol-danger-solid-ink',
  },
];

const contrastRatios = {};
for (const pair of contrastPairs) {
  contrastRatios[pair.label] = assertSolidContrast(pair.tokens, pair.label, pair.bg, pair.ink);
}

const mutedContrastPairs = [
  {
    label: 'light ink-4 on surface',
    tokens: lightTokens,
    bg: '--ol-surface',
    ink: '--ol-ink-4',
  },
  {
    label: 'light ink-4 on surface-2',
    tokens: lightTokens,
    bg: '--ol-surface-2',
    ink: '--ol-ink-4',
  },
];

const mutedContrastRatios = {};
for (const pair of mutedContrastPairs) {
  mutedContrastRatios[pair.label] = assertMutedContrast(pair.tokens, pair.label, pair.bg, pair.ink);
}

const remoteTokens = parseCustomProperties(extractCssBlock(remoteStyle, ':root'));

const remoteMutedContrastPairs = [
  {
    label: 'remote ink-4 on surface',
    tokens: remoteTokens,
    bg: '--surface',
    ink: '--ink-4',
  },
  {
    label: 'remote ink-4 on surface-2',
    tokens: remoteTokens,
    bg: '--surface-2',
    ink: '--ink-4',
  },
];

const remoteMutedContrastRatios = {};
for (const pair of remoteMutedContrastPairs) {
  remoteMutedContrastRatios[pair.label] = assertMutedContrast(pair.tokens, pair.label, pair.bg, pair.ink);
}

const olFrostBlock = extractCssBlock(globalCss, '.ol-frost');
assert.doesNotMatch(
  olFrostBlock,
  /rgba\(\s*255\s*,\s*255\s*,\s*255/i,
  '.ol-frost in global.css must not hardcode white rgba background gradients',
);
assert.match(globalCss, /background:\s*var\(--ol-frost-bg\)/, '.ol-frost must use --ol-frost-bg token');

assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-input-ink:/,
  'tokens.css must define --ol-input-ink in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-capsule-confirm-bg:/,
  'tokens.css must define --ol-capsule-confirm-bg in dark theme',
);
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-capsule-confirm-ink:/,
  'tokens.css must define --ol-capsule-confirm-ink in dark theme',
);

assert.match(tokens, /--ol-select-trigger-bg:/, 'tokens.css must define --ol-select-trigger-bg');
assert.match(tokens, /--ol-select-popover-bg:/, 'tokens.css must define --ol-select-popover-bg');
assert.match(
  tokens,
  /\[data-ol-theme='dark'\][\s\S]*--ol-select-popover-bg:/,
  'tokens.css must define --ol-select-popover-bg in dark theme',
);

assert.match(selectLite, /--ol-select-trigger-bg/, 'SelectLite must use --ol-select-trigger-bg');
assert.match(selectLite, /--ol-select-popover-bg/, 'SelectLite must use --ol-select-popover-bg');
assert.match(selectLite, /--ol-select-option-hover-bg/, 'SelectLite must use --ol-select-option-hover-bg');
assert.doesNotMatch(
  selectLite,
  /rgba\(\s*252\s*,/,
  'SelectLite must not hardcode light popover rgba backgrounds',
);
assert.doesNotMatch(
  translationPage,
  /background:\s*'#fff'/,
  'Translation.tsx must not override SelectLite with hardcoded #fff background',
);

assert.match(globalCss, /\.ol-app-shell-bg\b/, 'global.css must expose .ol-app-shell-bg');
assert.match(globalCss, /\.ol-aura-panel\b/, 'global.css must expose .ol-aura-panel');
assert.doesNotMatch(globalCss, /@keyframes ol-aura-halo/, 'global.css must not add an animated halo');
assert.match(globalCss, /\.ol-aura-card\b/, 'global.css must expose .ol-aura-card');
assert.match(
  globalCss,
  /\.ol-aura-settings\[data-ol-mobile="true"\]/,
  'global.css must scope mobile settings radius to the settings element itself',
);

assertUsesClassName(shell, 'ol-app-shell-bg', 'FloatingShell must use the app shell background class');
assertUsesClassName(shell, 'ol-aura-sidebar', 'FloatingShell must expose an Aura sidebar hook');
assertUsesClassName(shell, 'ol-aura-panel', 'FloatingShell must expose an Aura panel hook');

assertUsesClassName(settingsModal, 'ol-aura-settings', 'SettingsModal must expose an Aura settings wrapper');
assertUsesClassName(overview, 'ol-overview-hero', 'Overview must expose a high-visibility overview surface hook');

assert.match(
  settingsTabs,
  /import\s+\{[^}]*ThemeSection[^}]*\}\s+from\s+['"]\.\/ThemeSection['"]/,
  'tabs.tsx GeneralTab must import ThemeSection',
);
assert.match(settingsTabs, /<ThemeSection\s*\/?>/, 'tabs.tsx GeneralTab must render ThemeSection');

assert.match(
  themeMode,
  /prefers-color-scheme:\s*dark/,
  'themeMode.ts must listen for prefers-color-scheme: dark',
);
assert.match(
  themeMode,
  /data-ol-theme|olTheme|dataset\.olTheme/,
  'themeMode.ts must apply theme via data-ol-theme / dataset.olTheme',
);

const forbiddenStyleCardLightBackgrounds = [
  /rgba\(\s*255\s*,\s*255\s*,\s*255/i,
  /rgba\(\s*248\s*,\s*250\s*,\s*252/i,
  /rgba\(\s*239\s*,\s*246\s*,\s*255/i,
];

for (const pattern of forbiddenStyleCardLightBackgrounds) {
  assert.doesNotMatch(
    stylePage,
    pattern,
    'Style.tsx must not hardcode light style-card backgrounds (use --ol-style-* tokens)',
  );
}

assert.match(
  stylePage,
  /--ol-style-card-bg/,
  'Style.tsx must reference --ol-style-card-bg for style pack surfaces',
);
assert.match(
  stylePage,
  /--ol-style-card-ink/,
  'Style.tsx must reference --ol-style-card-ink for style pack text',
);
assert.match(
  stylePage,
  /--ol-style-subtle-bg/,
  'Style.tsx must reference --ol-style-subtle-bg for editor subtle surfaces',
);

const illegalCssStringPatterns = [
  /color:\s*'var\([^)]+\)';/,
  /background:\s*'var\([^)]+\)';/,
];

const forbiddenInlineInkBackground =
  /background:\s*'var\(--ol-ink\)'|background:\s*[^,\n;{]+?\?\s*'var\(--ol-ink\)'/;

const forbiddenBlueOnAccentCombo =
  /background:[\s\S]{0,200}var\(--ol-blue\)[\s\S]{0,500}?color:[\s\S]{0,200}var\(--ol-on-accent\)|color:[\s\S]{0,200}var\(--ol-on-accent\)[\s\S]{0,500}?background:[\s\S]{0,200}var\(--ol-blue\)/;

const forbiddenErrOnAccentCombo =
  /background:[\s\S]{0,200}var\(--ol-err[^)]*\)[\s\S]{0,500}?color:[\s\S]{0,200}var\(--ol-on-accent\)|background:[\s\S]{0,200}var\(--ol-err[^)]*\)[\s\S]{0,500}?color:[\s\S]{0,200}var\(--ol-accent-solid-ink\)|color:[\s\S]{0,200}var\(--ol-on-accent\)[\s\S]{0,500}?background:[\s\S]{0,200}var\(--ol-err[^)]*\)|color:[\s\S]{0,200}var\(--ol-accent-solid-ink\)[\s\S]{0,500}?background:[\s\S]{0,200}var\(--ol-err[^)]*\)/;

for (const relPath of sourceFiles) {
  const source = await read(`src/${relPath}`);
  for (const pattern of illegalCssStringPatterns) {
    assert.doesNotMatch(
      source,
      pattern,
      `src/${relPath} must not use quoted CSS custom properties inside injected CSS strings`,
    );
  }
  if (relPath.endsWith('.tsx')) {
    assert.doesNotMatch(
      source,
      forbiddenInlineInkBackground,
      `src/${relPath} must not use --ol-ink as a button/solid background`,
    );
    assert.doesNotMatch(
      source,
      forbiddenBlueOnAccentCombo,
      `src/${relPath} must not pair background var(--ol-blue) with color var(--ol-on-accent) (use --ol-accent-solid-* tokens)`,
    );
    assert.doesNotMatch(
      source,
      forbiddenErrOnAccentCombo,
      `src/${relPath} must not pair background var(--ol-err) with color var(--ol-on-accent) or var(--ol-accent-solid-ink) (use --ol-danger-solid-* tokens)`,
    );
  }
}

console.log('Aura skin contract OK');
console.log(
  'Solid contrast ratios:',
  Object.fromEntries(
    Object.entries(contrastRatios).map(([label, ratio]) => [label, `${ratio.toFixed(2)}:1`]),
  ),
);
console.log(
  'Muted contrast ratios:',
  Object.fromEntries(
    Object.entries({ ...mutedContrastRatios, ...remoteMutedContrastRatios }).map(([label, ratio]) => [
      label,
      `${ratio.toFixed(2)}:1`,
    ]),
  ),
);
