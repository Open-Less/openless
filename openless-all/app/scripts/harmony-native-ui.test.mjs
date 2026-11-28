// Contract test for the native HarmonyOS UI (v2 design system).
//
// v2 replaced the glass/glow "Common.ets + Tokens.ets" stack with a small,
// closed vocabulary: `common/Theme.ets` (semantic tokens) + `components/Kit.ets`
// (one way to draw a card, row, button, chip or input). This test guards the
// invariants that actually broke or drifted during the migration:
//
//   * a WebView shell creeping back in (dead bundle + two UIs drifting apart);
//   * destination ownership drifting between system back and navigation;
//   * a route that exists in the dock/rail but has no page behind it;
//   * pages drifting back to hard-coded hex colors or font sizes;
//   * a light-mode token without a dark counterpart;
//   * the v1 files resurrecting.
//
// Text-based so it runs in the Linux contract job (no node_modules required).
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const harmonyRoot = resolve(appRoot, '../harmony');
const ets = (path) => readFileSync(join(harmonyRoot, 'entry/src/main/ets', path), 'utf8');
const read = (path) => readFileSync(join(harmonyRoot, path), 'utf8');
const has = (path) => existsSync(join(harmonyRoot, path));

const index = ets('pages/Index.ets');
const shell = ets('components/AppShell.ets');
const nav = ets('components/Nav.ets');
const navigation = ets('components/ProductNavigation.ets');
const theme = ets('common/Theme.ets');
const kit = ets('components/Kit.ets');
const moduleJson = read('entry/src/main/module.json5');
const buildScript = read('scripts/build-harmony-app.sh');
const packaging = read('scripts/package-harmony-hap.sh');

const PAGE_FILES = [
  'pages/OverviewPage.ets',
  'pages/HistoryPage.ets',
  'pages/LibraryPages.ets',
  'pages/SettingsPage.ets',
  'pages/ToolPages.ets',
  'pages/ModelsPage.ets',
  'pages/CredentialsPage.ets',
  'pages/SelectionPage.ets',
];
const pages = PAGE_FILES.map(ets).join('\n');

// 1. Native UI only: no ArkWeb shell, no web bundle.
for (const [label, source] of [
  ['pages/Index.ets', index],
  ['components/AppShell.ets', shell],
]) {
  assert.doesNotMatch(
    source,
    /@kit\.ArkWeb|webview\.|WebviewController/,
    `${label} must not import ArkWeb`,
  );
  assert.doesNotMatch(source, /rawfile\('web\/|Web\(\{/, `${label} must not host the web bundle`);
}
assert.ok(
  !existsSync(join(harmonyRoot, 'entry/src/main/resources/rawfile/web')),
  'the web bundle must not be staged',
);
for (const file of [
  'build-profile.json5',
  'entry/src/main/module.json5',
  'entry/src/main/ets/pages/Index.ets',
]) {
  assert.ok(
    !existsSync(join(appRoot, 'harmony', file)),
    'the retired app-local project must not return: ' + file,
  );
}
assert.ok(
  !existsSync(join(appRoot, 'scripts/build-harmony-app.sh')),
  'the retired app-local builder must stay deleted',
);
assert.doesNotMatch(
  buildScript,
  /npm run build|web-dist|rawfile\/web'\)/,
  'the build must not produce a web bundle',
);
assert.match(packaging, /build-harmony-app\.sh/, 'packaging drives the native build script');
assert.ok(
  !has('scripts/build-harmony-frontend.sh'),
  'the old frontend-bundle script must stay gone',
);

// 2. v1 design system is gone for good.
for (const stale of [
  'entry/src/main/ets/components/Common.ets',
  'entry/src/main/ets/components/Material.ets',
  'entry/src/main/ets/common/Tokens.ets',
]) {
  assert.ok(!has(stale), `${stale} must stay deleted`);
}

// 3. Shared navigation model + five product destinations.
assert.match(
  ets('common/Store.ets'),
  /AppStorage\.setOrCreate<T>\(key, value\)/,
  'Store.put uses the documented create-or-update operation',
);
const primary = nav.match(/static readonly PRIMARY: string\[\] = \[([\s\S]*?)\];/)?.[1] ?? '';
const primaryIds = [...primary.matchAll(/Nav\.([A-Z]+)/g)].map((m) => m[1]);
assert.deepEqual(
  primaryIds,
  ['OVERVIEW', 'HISTORY', 'VOCAB', 'STYLE', 'SETTINGS'],
  'five product destinations stay available across layouts',
);
for (const id of ['SELECTION', 'TRANSLATION', 'CORRECTIONS', 'QA', 'CREDENTIALS', 'LOCALASR']) {
  assert.match(nav, new RegExp(`id: Nav\\.${id}, label:`), `${id} must be listed in Nav.tools()`);
}
assert.ok(
  !/Nav\.MARKETPLACE|Nav\.QUICKNOTE/.test(nav),
  'marketplace and quicknote routes are merged away',
);
assert.match(
  nav,
  /static rootOf\(id: string\): string/,
  'secondary pages need a top-level owner for the dock',
);
assert.match(
  navigation,
  /Nav\.rootOf\(this\.route\)/,
  'navigation must highlight the owning destination',
);
assert.match(navigation, /backgroundBlurStyle/, 'the dock uses native Harmony blur');
assert.match(
  navigation,
  /Nav\.DOCK_MAIN/,
  'the compact dock groups primary items separately from styles',
);
assert.match(shell, /ProductDock\(/, 'shell delegates compact navigation');
assert.doesNotMatch(shell, /ProductRail\(/, 'iOS keeps the tab bar on tablets, so no side rail');
assert.doesNotMatch(shell + theme, /NO_RAIL/, 'wide navigation must not be forced off');
assert.match(shell, /Layout\.bodyMax/, 'split pages must not be limited to single-column width');
assert.match(index, /return Nav\.back\(\)/, 'system back follows the shared navigation model');

// 3b. iOS dock: fixed circular Dictionary platter + three-item Styles platter when expanded.
for (const label of ['我的风格', '风格市场', '翻译目标']) {
  assert.ok(navigation.includes(`'${label}'`), `expanded dock must offer ${label}`);
}
assert.match(
  navigation,
  /this\.item\(Nav\.VOCAB, '词典'/,
  'expanded dock keeps 词典 as the way back',
);
assert.match(navigation, /Motion\.dock/, 'dock expansion uses the iOS 0.28s timing');
assert.doesNotMatch(shell, /DockScrim\(/, 'the dock material must not get an extra opaque scrim');
assert.doesNotMatch(navigation, /icon2\(\)\)\]/g, 'unselected dock glyphs use the primary ink');

// 4. Every page the shell routes to exists and is exported.
for (const struct of [
  'OverviewPage',
  'HistoryPage',
  'StylePage',
  'VocabPage',
  'CorrectionsPage',
  'SettingsPage',
  'CredentialsPage',
  'SelectionPage',
  'TranslationPage',
  'QaPage',
  'LocalAsrPage',
]) {
  assert.ok(pages.includes(`export struct ${struct}`), `missing native page ${struct}`);
  assert.ok(shell.includes(`${struct}()`), `AppShell must render ${struct}`);
}
assert.match(
  shell,
  /else \{\s*\n\s*OverviewPage\(\)/,
  'unknown routes must fall back to the overview',
);

// 5. Design tokens: one closed scale, resource-backed colors only.
assert.match(theme, /static readonly xxs: number = 4;/, 'spacing starts at 4vp');
assert.match(
  theme,
  /static readonly xxl: number = 40;/,
  'spacing tops out at 40vp in the 4-step grid',
);
assert.match(theme, /static readonly body: number = 17;/, 'Body follows the iOS 17pt body style');
assert.match(
  theme,
  /static readonly caption: number = 13;/,
  'captions follow the iOS 13pt footnote',
);
assert.match(theme, /static readonly caption2: number = 11;/, 'the 11pt caption2 tier exists');
assert.match(theme, /static readonly card: number = 16;/, 'cards use the product 16vp radius');
assert.match(
  theme,
  /static contentMax\(bp: string\): number \{[\s\S]{0,400}return 880;/,
  'XL content is capped',
);
assert.doesNotMatch(
  theme,
  /backgroundBlurStyle|linearGradient/,
  'tokens must not paint material themselves',
);
assert.match(kit, /export struct PageScaffold/, 'pages share one scaffold');

// 6. Pages only use tokens: no hex colors, no literal font sizes.
const hex = /#[0-9A-Fa-f]{6,8}\b/;
for (const file of PAGE_FILES) {
  const source = ets(file);
  assert.ok(!hex.test(source), `${file} must not hard-code a color`);
  assert.ok(!/fontSize\(\s*\d/.test(source), `${file} must not hard-code a font size`);
  assert.ok(!/backgroundBlurStyle\(/.test(source), `${file} must not paint its own material`);
  assert.match(source, /from '\.\.\/components\/Kit'/, `${file} must use the shared kit`);
}
assert.ok(!hex.test(kit), 'the kit must not hard-code a color');

// 7. Light and dark palettes stay in step, and both carry the v2 roles.
const colors = (theme) =>
  JSON.parse(read(`entry/src/main/resources/${theme}/element/color.json`)).color.map((e) => e.name);
const light = colors('base');
const dark = colors('dark');
for (const name of light) assert.ok(dark.includes(name), `dark palette is missing ${name}`);
assert.ok(
  light.filter((name) => name.startsWith('c_')).length >= 20,
  'the v2 palette defines the shared roles',
);
for (const role of ['c_bg', 'c_card', 'c_line', 'c_text_1', 'c_accent', 'c_danger']) {
  assert.ok(light.includes(role), `missing color role ${role}`);
}

// 8. Merged surfaces keep their behaviour.
assert.ok(
  !/export struct MarketplacePage/.test(pages),
  'the marketplace is a section of StylePage',
);
assert.ok(!/export struct QuickNotePage/.test(pages), 'quick note lives inside the history page');
assert.match(
  ets('pages/LibraryPages.ets'),
  /风格市场/,
  'StylePage must expose the marketplace section',
);
assert.match(ets('pages/HistoryPage.ets'), /速记/, 'HistoryPage must expose the notes section');
assert.match(
  ets('pages/OverviewPage.ets'),
  /Dashboard/,
  'the overview composes user-selectable cards',
);
assert.match(
  ets('common/Dashboard.ets'),
  /static readonly CARDS/,
  'the dashboard keeps a card registry',
);

// 9. Everything still goes through the one bridge to the shared Rust core.
for (const file of ['pages/OverviewPage.ets', 'pages/HistoryPage.ets', 'pages/SettingsPage.ets']) {
  assert.match(ets(file), /from '\.\.\/data\/Api'/, `${file} must use the typed Api façade`);
  assert.doesNotMatch(
    ets(file),
    /nativeHost\.invoke/,
    `${file} must not call the native module directly`,
  );
}
for (const [label, file, api] of [
  ['history playback', 'pages/HistoryPage.ets', /Api\.readRecording/],
  ['history re-polish', 'pages/HistoryPage.ets', /Api\.repolish/],
  ['dictation', 'pages/OverviewPage.ets', /Api\.startDictation/],
  ['settings writes', 'pages/SettingsPage.ets', /Api\.writeBool/],
  ['cloud sync', 'pages/SettingsPage.ets', /Api\.cloudSyncUpload/],
  ['credentials', 'pages/CredentialsPage.ets', /Api\.setCredential/],
  ['channel test', 'pages/CredentialsPage.ets', /Api\.validateChannel/],
  ['channel create', 'pages/CredentialsPage.ets', /Api\.createChannel/],
  ['marketplace install', 'pages/LibraryPages.ets', /Api\.installMarketplacePack/],
  ['marketplace like', 'pages/LibraryPages.ets', /Api\.likePack/],
]) {
  assert.match(ets(file), api, `${label} must stay wired`);
}

// 10. One UI for phone, tablet and 2-in-1 within the signing profile.
assert.match(
  moduleJson,
  /"deviceTypes":\s*\[\s*\n\s*"phone",\s*\n\s*"tablet"\s*\n\s*\]/,
  'deviceTypes must be phone + tablet',
);
assert.doesNotMatch(
  moduleJson,
  /"2in1"/,
  'a 2in1 device type needs a 2-in-1 signing profile first',
);
assert.match(moduleJson, /"orientation": "auto_rotation"/, 'the window must be free to rotate');
assert.doesNotMatch(
  ets('common/Breakpoint.ets'),
  /mediaquery\.matchMediaSync/,
  'the size class must not depend on mediaquery',
);
assert.match(
  ets('common/Breakpoint.ets'),
  /getWindowProperties\(\)/,
  'the size class must use the window rectangle',
);

// 11. Presentational modules must not duplicate backend or recording state.
for (const file of [
  'VoiceSessionCard',
  'TranscriptCard',
  'TodayMetricsCard',
  'ActivityTrendCard',
]) {
  const source = ets(`components/overview/${file}.ets`);
  assert.doesNotMatch(
    source,
    /@State|@StorageLink|Api\./,
    `${file} must receive props and callbacks`,
  );
  assert.ok(!hex.test(source), `${file} uses semantic colors`);
}
assert.match(
  ets('pages/OverviewPage.ets'),
  /AdaptiveColumns/,
  'overview reuses the common reflow container',
);
assert.match(
  ets('components/AdaptiveLayout.ets'),
  /@BuilderParam primary/,
  'layout separates content slots',
);
assert.match(
  ets('pages/ModelsPage.ets'),
  /SystemSpeechCard/,
  'system offline trial is distinct from Core model support',
);
assert.match(
  ets('pages/LibraryPages.ets'),
  /@StorageLink\('styleSection'\)/,
  'style tabs and dock share selection',
);
console.log(
  'Harmony native UI: five destinations, modular pages, adaptive shell and native model trial passed',
);
