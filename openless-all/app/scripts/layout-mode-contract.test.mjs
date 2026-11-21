import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const read = (relativePath) => readFileSync(join(appRoot, relativePath), 'utf8');

const css = read('src/styles/global.css');
const stackedLayout = read('src/lib/stackedLayout.ts');
const toggle = read('src/ui/controls/SettingRow.tsx');
const refinements = read('src/styles/refinements.css');
const pageHeader = read('src/ui/layout/PageHeader.tsx');
const rowCss = read('src/ui/controls/setting-row.css');
const channelList = read('src/features/providers/ChannelList.tsx');
const permissions = read('src/features/input/PermissionsSection.tsx');
const androidPermissions = read('android/frontend/components/AndroidPermissionsPanel.tsx');
const about = read('src/features/settings/sections/AboutSection.tsx');
const updateButton = read('src/features/settings/sections/CheckUpdateButton.tsx');
const updateButtonCss = read('src/features/settings/sections/check-update-button.css');
const mobileSheet = read('src/app/shell/MobileNavigationSheet.tsx');
const mobileSheetCss = read('src/app/shell/navigation.css');

assert.doesNotMatch(css, /\.ol-flex-row\s*>\s*\*/);
assert.doesNotMatch(css, /\[class\*=["']ol-grid["']\]/);
assert.match(css, /\.ol-flex-row\.ol-flex-split/);
assert.match(css, /\.ol-conservative-stack/);

assert.match(stackedLayout, /stackedRowLayout\?: boolean/);
assert.doesNotMatch(stackedLayout, /mobile\s*\|\|/);
// Toggle size contract: the shared .ol-switch locks width 36 with width/min-width/max-width; flex only
// declares no grow/shrink. flex-basis must be auto to avoid stretching the toggle's height to 36px in a
// column-direction container.
const switchRule = refinements.match(/\.ol-switch\s*\{[^}]*\}/)?.[0] ?? '';
assert.match(toggle, /className="ol-switch"/);
assert.match(switchRule, /--ol-switch-width:\s*36px/);
assert.match(switchRule, /flex:\s*0 0 auto/);
assert.match(switchRule, /\swidth:\s*var\(--ol-switch-width\)/);
assert.match(switchRule, /min-width:\s*var\(--ol-switch-width\)/);
assert.match(switchRule, /max-width:\s*var\(--ol-switch-width\)/);
assert.match(pageHeader, /useMobileLayout\(\)/);
assert.doesNotMatch(pageHeader, /useReadableLayout|useConservativeLayout/);
assert.match(rowCss, /@media \(max-width: 719px\)[^]*?grid-template-columns: minmax\(0, 1fr\)/);

assert.match(channelList, /ol-conservative-stack/);
assert.match(channelList, /settings\.channels\.edit/);
assert.match(permissions, /permissionActionsStyle/);
assert.match(permissions, /justifyContent:\s*stackLayout\s*\?\s*'flex-start'\s*:\s*'flex-end'/);
assert.match(androidPermissions, /const baseLayoutStack = useLayoutStack\(\)/);
assert.match(androidPermissions, /const rowJustify = layoutStack \? 'flex-start' : 'flex-end'/);

assert.match(about, /ol-inline-composite/);
assert.match(about, /compact=\{compactLayout\}/);
assert.match(updateButton, /data-compact=\{compact \|\| undefined\}/);
assert.match(updateButtonCss, /\.ol-check-update-button\s*\{[^}]*\bmin-width:\s*160px/);
assert.match(updateButtonCss, /\.ol-check-update-button\[data-compact\]\s*\{[^}]*\bmin-width:\s*32px/);
assert.match(updateButton, /aria-label=\{compact\s*\?\s*label\s*:\s*undefined\}/);
assert.match(updateButtonCss, /\.ol-check-update-button\[data-compact\] \.ol-check-update-label\s*\{[^}]*\bdisplay:\s*none/);
assert.match(mobileSheet, /chevRight/);
assert.match(mobileSheet, /<Modal[^]*?overlayClassName="ol-mobile-navigation-sheet"/);
assert.match(mobileSheetCss, /align-items: end/);
assert.match(mobileSheetCss, /min-width: 0/);

console.log('layout mode contract tests passed');
