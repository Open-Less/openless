// Frame-by-frame parity contract with the iOS demo recording.
//
// Reference: /data/Video_1795451355907_396.mp4, analysed into .refs/ios-demo/
// (147 frames at 10 fps + 15 per-second contact sheets). The assertions below pin
// the structural facts that were read off those frames AND confirmed in the iOS
// sources, so a later refactor cannot silently drift back to a "similar but not
// the same" layout:
//
//   * home has no navigation bar (brand row scrolls with the content);
//   * history / settings / style use the centred 17vp inline title;
//   * dictionary draws its own 34vp large title inside the scroll body;
//   * history cards use the iOS seven-state status mapping;
//   * dictionary chips scroll horizontally and are driven by CapsuleTrack;
//   * the two 52vp orbs own search and add;
//   * section switching lives in the expanded dock, not in a page segmented.
//
// Text-based so it runs in the Linux contract job (no node_modules required).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ets = (path) => readFileSync(join(appRoot, '../harmony/entry/src/main/ets', path), 'utf8');

const overview = ets('pages/OverviewPage.ets');
const history = ets('pages/HistoryPage.ets');
const library = ets('pages/LibraryPages.ets');
const settings = ets('pages/SettingsPage.ets');
const kit = ets('components/Kit.ets');
const nav = ets('components/ProductNavigation.ets');
const shell = ets('components/AppShell.ets');

// 1. Home: `toolbar(.hidden)` + self-drawn brand row inside the scroll body.
assert.match(overview, /BrandBar\(/, 'home draws its own brand row');
assert.doesNotMatch(overview, /PageScaffold\(/, 'home has no navigation bar');
assert.match(overview, /Scroll\(\)/, 'home body scrolls');

// 2. Centred inline titles for history, settings and the style sections.
for (const [label, source, title] of [
  ['history', history, "title: this.detailOpen() ? '记录详情' : '历史'"],
  ['settings', settings, "title: '设置'"],
  ['style', library, "this.segment === 0 ? '我的风格' : '风格市场'"],
]) {
  assert.ok(source.includes(title), `${label} uses its inline title`);
  assert.match(source, /PageScaffold\(/, `${label} keeps the shared scaffold`);
  assert.doesNotMatch(source, /largeTitle: true/, `${label} must not draw a large title`);
}

// 3. Dictionary: 34vp large title drawn in the body, not by the scaffold.
assert.match(
  library,
  /Text\('词典'\)[\s\S]{0,160}fontSize\(Type\.pageTitle\)/,
  'dictionary draws its own large title',
);
assert.match(
  library,
  /fontWeight\(FontWeight\.Bold\)/,
  'the large title is bold like .largeTitle.bold()',
);

// 4. History status mapping is the iOS seven-state switch.
for (const status of [
  '已完成',
  '未完成录音',
  '已取消',
  '识别未完成',
  '未识别到文字',
  '润色未完成，原文已保留',
  '文字已保留，输入未完成',
]) {
  assert.ok(history.includes(status), `history status "${status}" must exist`);
}
assert.doesNotMatch(
  history,
  /Segmented\(\{\s*items: TAB_LABELS/,
  'the 记录/速记 segment lives in the menu, not on the page',
);
assert.match(history, /速记/, 'the scratchpad entry stays reachable');
assert.match(history, /PinnedSearchBar\(/, 'history pins its search bar');

// 5. Dictionary chips + orbs.
assert.match(kit, /export struct CapsuleTrack/, 'the capsule track is shared, not page-local');
assert.match(
  kit,
  /scrollable\(ScrollDirection\.Horizontal\)/,
  'the chip track scrolls horizontally',
);
assert.match(library, /CapsuleTrack\(\{/, 'dictionary uses the capsule track');
assert.match(library, /bottomActions\(\)/, 'dictionary keeps the floating action area');
assert.match(library, /\.width\(52\)/, 'orbs are 52vp like the video');
assert.match(library, /searchOpen/, 'search expands from the orb');

// 6. Style sections come from the dock; no page-level section segmented.
assert.doesNotMatch(library, /STYLE_SEGMENTS/, 'style sections are not a page segmented');
assert.match(library, /@StorageLink\('styleSection'\)/, 'style section is shared with the dock');
assert.match(library, /设置文字模型/, 'the model entry row is present');
assert.match(
  nav,
  /我的风格[\s\S]{0,200}风格市场[\s\S]{0,200}翻译目标/,
  'expanded dock lists all three sections',
);

// 7. Settings uses the pinned search bar and the grouped section helper.
assert.match(settings, /PinnedSearchBar\(\{/, 'settings pins its search bar');
assert.match(settings, /SettingsSection\(\{/, 'settings uses the inset-grouped helper');

// 8. Shared motion tokens and the native dock material remain, without an
// extra page-wide scrim that would flatten the immersive translucent surface.
assert.match(kit, /export struct PinnedSearchBar/, 'the pinned bar is a shared primitive');
assert.match(
  ets('common/Theme.ets'),
  /static readonly dock: number = 280;/,
  'dock expansion uses the iOS 0.28s',
);
assert.doesNotMatch(shell, /DockScrim\(|ProductRail\(/, 'the shell mounts only the floating dock');
assert.match(nav, /ImmersiveMaterial/, 'the dock paints the system immersive material');
assert.match(nav, /pillGradient\(\)/, 'older devices keep the painted glass fallback');

// 9. The isolated dock platter is a fixed 62vp circle, not a tablet-width ratio.
assert.match(nav, /const DOCK_HEIGHT: number = 62;/);
assert.match(nav, /const DOCK_GAP: number = 10;/);
assert.match(nav, /\.width\(this\.expanded\(\) \? DOCK_HEIGHT : undefined\)/);
assert.match(nav, /\.width\(this\.expanded\(\) \? undefined : DOCK_HEIGHT\)/);
assert.doesNotMatch(nav, /layoutWeight\(this\.expanded\(\) \? (?:1 : 4|3 : 1)\)/);
const metrics = ets('components/overview/TodayMetricsCard.ets');
assert.match(
  metrics,
  /\.id\(this\.metricId\)[\s\S]{0,40}\.width\('100%'\)/,
  'metric tile contents and their border must fill the allocated half-row',
);
for (const id of ['today-chars', 'today-duration', 'average-duration', 'total-records']) {
  assert.ok(metrics.includes(`'metric-${id}'`));
}
assert.match(
  ets('components/overview/BrandBar.ets'),
  /Image\(\$r\('app\.media\.openless_icon'\)\)/,
);
assert.doesNotMatch(ets('components/overview/BrandBar.ets'), /Text\('OL'\)/);

// 10. Every sheet sends an SDK-typed radius, including orientation updates.
for (const [label, source] of [
  ['overview', overview],
  ['history', history],
  ['library', library],
  ['settings', settings],
]) {
  assert.equal(
    (source.match(/radius: LengthMetrics\.vp\(Radius\.sheet\)/g) ?? []).length,
    (source.match(/\.bindSheet\(/g) ?? []).length,
    `${label} must type every sheet radius`,
  );
  assert.doesNotMatch(source, /radius:\s*\{\s*topStart:/);
}

// 11. Product silhouettes keep the iOS meanings: chart, closed dictionary,
// three-slider settings and sparkles instead of house/calendar/wand glyphs.
const icons = ets('components/Icons.ets');
for (const name of ['dock_overview', 'dock_dictionary', 'dock_settings', 'dock_style']) {
  const svg = readFileSync(
    join(appRoot, '../harmony/entry/src/main/resources/base/media', name + '.svg'),
    'utf8',
  );
  assert.match(svg, /viewBox="0 0 24 24"/);
  assert.match(svg, /<path\s/);
  assert.doesNotMatch(svg, /<(?:script|image|text|foreignObject)\b|https?:\/\/(?!www\.w3\.org)/);
  assert.ok(svg.length < 1500, 'dock vectors must stay small and self-contained');
  assert.ok(icons.includes(`'app.media.${name}'`));
}
assert.match(nav, /Image\(dockImageOf\(id\)\)/);
assert.match(
  nav,
  /fillColor\(this\.primaryActive\(id\) \? Palette\.accent\(\) : Palette\.text1\(\)\)/,
);
assert.match(
  nav,
  /fillColor\(this\.styleActive\(section\) \? Palette\.accent\(\) : Palette\.text1\(\)\)/,
);

// 12. No hard-coded palette or font sizes leaked into any of these files.
const hex = /#[0-9A-Fa-f]{6,8}\b/;
for (const [label, source] of [
  ['history', history],
  ['library', library],
  ['settings', settings],
  ['overview', overview],
  ['kit', kit],
  ['navigation', nav],
]) {
  assert.ok(!hex.test(source), `${label} must not hard-code a colour`);
}
// Pages must not pin text sizes; the kit may size glyphs (icons are not text).
for (const [label, source] of [
  ['history', history],
  ['library', library],
  ['settings', settings],
  ['overview', overview],
  ['navigation', nav],
]) {
  assert.ok(!/fontSize\(\s*\d/.test(source), `${label} must not hard-code a text size`);
}

console.log(
  'Harmony/iOS parity: nav-bar shapes, status mapping, chips, orbs, dock sections and motion passed',
);
