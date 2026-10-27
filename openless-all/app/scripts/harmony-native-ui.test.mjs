// Contract test for the native HarmonyOS UI.
//
// The HAP used to be an ArkWeb shell around the shared web bundle: the whole app
// was one `Web({ src: $rawfile('web/index.html') })`. It is now a native ArkUI
// app built for phone, tablet and 2-in-1, and the risks this test guards are the
// ones that actually bit during the migration:
//
//   * a `webview` import creeping back in (it would ship 40 MB of dead assets and
//     two UIs that drift apart);
//   * `AppStorage.setOrCreate` used as an assignment - it silently ignores an
//     existing key, which pinned navigation to the first destination forever;
//   * `deviceTypes`/`orientation` losing 2-in-1 or rotation support;
//   * a route that exists in the side bar but has no page behind it;
//   * a light-mode token without a dark-mode counterpart.
//
// Text-based so it also runs in the Linux contract job (no node_modules).
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(appRoot, '../..');
const ets = (path) => readFileSync(join(appRoot, 'harmony/entry/src/main/ets', path), 'utf8');
const read = (path) => readFileSync(join(appRoot, path), 'utf8');

const index = ets('pages/Index.ets');
const shell = ets('components/AppShell.ets');
const nav = ets('components/Nav.ets');
const tokens = ets('common/Tokens.ets');
const breakpoints = ets('common/Breakpoint.ets');
const moduleJson = read('harmony/entry/src/main/module.json5');
const buildScript = read('scripts/build-harmony-app.sh');
const packaging = read('scripts/package-harmony-hap.sh');

// 1. Native UI: no ArkWeb shell, no web bundle.
for (const [label, source] of [['pages/Index.ets', index], ['components/AppShell.ets', shell]]) {
  assert.doesNotMatch(source, /@kit\.ArkWeb|webview\.|WebviewController/, `${label} must not import ArkWeb`);
  assert.doesNotMatch(source, /rawfile\('web\/|Web\(\{/, `${label} must not host the web bundle`);
}
assert.ok(
  !existsSync(join(appRoot, 'harmony/entry/src/main/resources/rawfile/web')),
  'the web bundle must not be staged into the HAP',
);
assert.doesNotMatch(buildScript, /npm run build|web-dist|rawfile\/web'\)/, 'the build must not produce a web bundle');
assert.match(packaging, /build-harmony-app\.sh/, 'packaging drives the native build script');
assert.ok(
  !existsSync(join(appRoot, 'scripts/build-harmony-frontend.sh')),
  'the old frontend-bundle script must be gone',
);

// 2. `AppStorage.setOrCreate` only creates; every writer needs an assignment.
assert.match(nav, /Store\.put<string>\('route'/, 'navigation must assign the route, not only create it');
assert.doesNotMatch(
  nav + index,
  /AppStorage\.setOrCreate<string>\('route'/,
  'setOrCreate silently ignores an existing route key and freezes navigation',
);
assert.match(ets('common/Store.ets'), /AppStorage\.set<T>\(key, value\)/, 'Store.put must assign after creating');

// 3. One UI for phone, tablet and PC.
// deviceTypes must stay inside what the signing profile grants: declaring `2in1`
// without a 2-in-1 profile makes the system reject the app outright.
assert.match(moduleJson, /"deviceTypes":\s*\[\s*\n\s*"phone",\s*\n\s*"tablet"\s*\n\s*\]/, 'deviceTypes must be phone + tablet');
assert.doesNotMatch(moduleJson, /"2in1"/, 'a 2in1 device type needs a 2-in-1 signing profile first');
assert.match(moduleJson, /"orientation": "auto_rotation"/, 'the window must be free to rotate');
assert.doesNotMatch(moduleJson, /"supportWindowMode":\s*\[[^\]]*"floating"/, 'free-form windows are a 2-in-1 capability the profile does not cover');
assert.match(moduleJson, /"supportWindowMode":\s*\[\s*\n\s*"fullscreen",\s*\n\s*"split"\s*\n\s*\]/, 'tablets may still split');

// 4. Size classes come from the window rectangle, not from `mediaquery`.
assert.doesNotMatch(breakpoints, /mediaquery\.matchMediaSync/, 'the size class must not depend on mediaquery ranges');
assert.match(breakpoints, /getWindowProperties\(\)/, 'the size class must use the window rectangle');
assert.match(breakpoints, /'windowSizeChange'/, 'rotation and free-form resize must re-flow through windowSizeChange');
{
  const source = breakpoints
    .replace(/\r\n/g, '\n')
    .replace(/^import .*;\n/gm, '');
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  const exports = {};
  vm.runInNewContext(compiled, { exports, console });
  const of = exports.Breakpoints.of;
  const cases = [
    [320, 'sm'], // phone portrait
    [599, 'sm'],
    [600, 'md'], // phone landscape / small tablet
    [839, 'md'],
    [840, 'lg'], // large tablet / folded phone
    [1079, 'lg'],
    [1080, 'xl'], // 2-in-1 / PC
    [1600, 'xl'],
  ];
  for (const [width, expected] of cases) assert.equal(of(width), expected, `${width}vp must be ${expected}`);
}

// 5. Every navigation destination has a page behind it.
const destinations = [...nav.matchAll(/static readonly [A-Z]+: string = '([a-z]+)'/g)].map((match) => match[1]);
assert.ok(destinations.length >= 12, `expected at least 12 routes, got ${destinations.length}`);
for (const id of destinations) {
  if (id === 'overview') continue;
  assert.match(shell, new RegExp(`Nav\\.${id.toUpperCase()}\\)|Nav\\.${id.toUpperCase()}\\b`), `AppShell must route ${id}`);
}
// The overview is the fallback: an unknown route must still land somewhere sane.
assert.match(shell, /else \{\s*\n\s*OverviewPage\(\)/, 'unknown routes must fall back to the overview');

// Navigation stays short: five primary destinations, everything else under 设置.
const primary = nav.match(/static readonly PRIMARY: string\[\] = \[([^\]]+)\]/)?.[1] ?? '';
const primaryIds = [...primary.matchAll(/Nav\.([A-Z]+)/g)].map((match) => match[1]);
assert.equal(primaryIds.length, 5, `the tab bar may hold five destinations, got ${primaryIds.length}`);
for (const id of ['CORRECTIONS', 'LOCALASR', 'CREDENTIALS', 'MARKETPLACE', 'TRANSLATION', 'QA', 'QUICKNOTE']) {
  assert.ok(!primaryIds.includes(id), `${id} belongs in 设置, not in the tab bar`);
  assert.match(nav, new RegExp(`\\{ id: Nav\\.${id}, label: `), `${id} must be listed in Nav.tools()`);
}
assert.match(ets('pages/SettingsPage.ets'), /Nav\.tools\(\)/, '设置 must render the tool list');
assert.ok(!existsSync(join(appRoot, 'harmony/entry/src/main/ets/components/MorePage.ets')), 'the 更多 sheet is gone with the old 11-item bar');

// Portrait stays phone-shaped: only the widest class gets a side bar.
assert.match(shell, /if \(Breakpoints\.isWide\(this\.bp\)\) \{\s*\n\s*this\.sideBar\(\)/, 'only lg may draw a side bar');
assert.match(shell, /if \(!Breakpoints\.isWide\(this\.bp\)\) \{\s*\n\s*this\.tabBar\(\)/, 'md and sm keep the bottom tab bar');

// HarmonyOS design baseline: 8vp grid, spec type scale, 16vp card radius.
assert.match(tokens, /static readonly xxs: number = 4;/, 'spacing starts at 4vp');
assert.match(tokens, /static readonly xxxl: number = 64;/, 'spacing follows the 8vp grid up to 64vp');
assert.match(tokens, /static readonly body: number = 16;/, 'Body M is 16fp');
assert.match(tokens, /static readonly caption: number = 12;/, 'captions bottom out at 12fp');
assert.match(tokens, /static readonly touch: number = 44;/, 'touch targets stay above 40vp');
assert.match(tokens, /static gutter\(bp: string\): number \{[\s\S]{0,200}return 32;/, 'XL margins are 32vp');

// Immersive material: translucent glass + blur over an ambient glow.
assert.match(ets('common/Tokens.ets'), /static glass\(\)/, 'cards need a translucent glass token');
assert.match(ets('components/Common.ets'), /backgroundBlurStyle\(BlurStyle\.COMPONENT_THICK\)/, 'cards must use the system blur material');
assert.match(shell, /backgroundBlurStyle\(BlurStyle\.COMPONENT_ULTRA_THICK\)/, 'navigation chrome must use the system blur material');
assert.match(index, /ambient\(\)/, 'the shell must paint an ambient background');
assert.match(index, /linearGradient\(/, 'the ambient wash is a single gradient, not blurred colour blobs');
assert.match(index, /C\.glowA\(\)/, 'the wash must use the brand glow token');
const pageFiles = [
  'pages/CredentialsPage.ets',
  'pages/OverviewPage.ets',
  'pages/HistoryPage.ets',
  'pages/SettingsPage.ets',
  'pages/LibraryPages.ets',
  'pages/ToolPages.ets',
];
for (const file of pageFiles) assert.ok(existsSync(join(appRoot, 'harmony/entry/src/main/ets', file)), `${file} must exist`);
const pages = pageFiles.map((file) => ets(file)).join('\n');
for (const struct of ['OverviewPage', 'HistoryPage', 'VocabPage', 'CorrectionsPage', 'StylePage',
  'MarketplacePage', 'TranslationPage', 'QaPage', 'QuickNotePage', 'LocalAsrPage', 'SettingsPage',
  'CredentialsPage']) {
  assert.ok(pages.includes(`export struct ${struct}`), `missing native page ${struct}`);
  assert.ok(shell.includes(`${struct}()`), `AppShell must render ${struct}`);
}

// 6. Everything goes through the one bridge to the shared Rust core.
for (const file of ['pages/OverviewPage.ets', 'pages/HistoryPage.ets', 'pages/SettingsPage.ets']) {
  assert.match(ets(file), /from '\.\.\/data\/Api'/, `${file} must use the typed Api façade`);
}
assert.doesNotMatch(pages, /nativeHost\.invoke/, 'pages must not call the native module directly');
// Secrets never travel through the preference document.
assert.match(ets('pages/CredentialsPage.ets'), /Api\.setCredential/, 'credentials must go through set_credential');
assert.doesNotMatch(ets('pages/CredentialsPage.ets'), /writeString\('\/.*apiKey/i, 'a key must never be a preference');

// 7. Light and dark palettes stay in step.
const colors = (theme) =>
  JSON.parse(read(`harmony/entry/src/main/resources/${theme}/element/color.json`)).color.map((entry) => entry.name);
const light = colors('base');
const dark = colors('dark');
assert.ok(light.length >= 20, 'the palette must define the shared tokens');
for (const name of light) assert.ok(dark.includes(name), `dark palette is missing ${name}`);
assert.match(tokens, /ol_accent/, 'tokens must expose the brand accent');

// 8. Feature surfaces stay wired to the shared core.
const pagesAll = pages;
for (const [label, source, api] of [
  ['history playback', ets('pages/HistoryPage.ets'), /Api\.readRecording/],
  ['history re-polish', ets('pages/HistoryPage.ets'), /Api\.repolish/],
  ['channel test', ets('pages/CredentialsPage.ets'), /Api\.validateChannel/],
  ['channel CRUD', ets('pages/CredentialsPage.ets'), /Api\.createChannel/],
  ['model list', ets('pages/CredentialsPage.ets'), /Api\.listModels/],
  ['style editing', ets('pages/LibraryPages.ets'), /Api\.saveStylePack/],
  ['marketplace like', ets('pages/LibraryPages.ets'), /Api\.likePack/],
  ['marketplace login', ets('pages/LibraryPages.ets'), /Api\.githubDeviceFlowStart/],
  ['microphone test', ets('pages/SettingsPage.ets'), /Api\.startMicrophoneMonitor/],
  ['log export', ets('pages/SettingsPage.ets'), /Api\.exportErrorLog/],
  ['onboarding', ets('pages/OverviewPage.ets'), /Api\.credentialStatus/],
]) {
  assert.match(source, api, `${label} must call the core through Api`);
}
assert.match(ets('common/AudioPlayback.ets'), /AVPlayer/, 'recording playback must use AVPlayer');
assert.match(ets('common/AudioPlayback.ets'), /fdSrc/, 'AVPlayer reads the cached wav through fdSrc');

// 9. The layout override used for single-device QA stays opt-in.
assert.match(index, /bpOverride/, 'the size class override must be readable for QA');
assert.match(ets('entryability/EntryAbility.ets'), /--ps layout/, 'the launcher form is documented next to the parser');

console.log('harmony-native-ui contract passed');
