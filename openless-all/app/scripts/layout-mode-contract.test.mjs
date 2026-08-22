import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const read = relativePath => readFileSync(join(appRoot, relativePath), 'utf8');

const css = read('src/styles/global.css');
const stackedLayout = read('src/lib/stackedLayout.ts');
const shared = read('src/pages/settings/shared.tsx');
const pageAtoms = read('src/pages/_atoms.tsx');
const row = read('src/components/ui/Row.tsx');
const channelList = read('src/pages/settings/ChannelList.tsx');
const permissions = read('src/pages/settings/PermissionsSection.tsx');
const androidPermissions = read('android/frontend/components/AndroidPermissionsPanel.tsx');
const about = read('src/pages/settings/AboutSection.tsx');
const updateButton = read('src/pages/settings/CheckUpdateButton.tsx');
const moreSheet = read('src/components/MobileMoreSheet.tsx');
const styleSheet = read('src/components/MobileStyleSheet.tsx');

assert.doesNotMatch(css, /\.ol-flex-row\s*>\s*\*/);
assert.doesNotMatch(css, /\[class\*=["']ol-grid["']\]/);
assert.match(css, /\.ol-flex-row\.ol-flex-split/);
assert.match(css, /\.ol-conservative-stack/);

assert.match(stackedLayout, /stackedRowLayout\?: boolean/);
assert.doesNotMatch(stackedLayout, /mobile\s*\|\|/);
assert.match(shared, /flex:\s*"0 0 36px"/);
assert.match(shared, /minWidth:\s*36/);
assert.match(shared, /maxWidth:\s*36/);
assert.match(pageAtoms, /preferenceStack\s*\?/);
assert.match(row, /stackLayout\s*\?\s*'minmax\(0, 1fr\)'/);

assert.match(channelList, /ol-conservative-stack/);
assert.match(channelList, /settings\.channels\.edit/);
assert.match(permissions, /permissionActionsStyle/);
assert.match(permissions, /justifyContent:\s*stackLayout\s*\?\s*'flex-start'\s*:\s*'flex-end'/);
assert.match(androidPermissions, /const baseLayoutStack = useLayoutStack\(\)/);
assert.match(androidPermissions, /const rowJustify = layoutStack \? 'flex-start' : 'flex-end'/);

assert.match(about, /ol-inline-composite/);
assert.match(about, /compact=\{compactLayout\}/);
assert.match(updateButton, /minWidth:\s*compact\s*\?\s*32\s*:\s*160/);
assert.match(updateButton, /aria-label=\{compact\s*\?\s*label\s*:\s*undefined\}/);
assert.match(updateButton, /display:\s*compact\s*\?\s*'none'/);
assert.match(moreSheet, /chevRight/);
assert.match(styleSheet, /chevRight/);
assert.match(moreSheet, /flexWrap:\s*'nowrap'/);
assert.match(styleSheet, /flexWrap:\s*'nowrap'/);
assert.match(moreSheet, /position:\s*'fixed'/);
assert.match(styleSheet, /position:\s*'fixed'/);
assert.match(moreSheet, /boxSizing:\s*'border-box'/);
assert.match(styleSheet, /boxSizing:\s*'border-box'/);

console.log('layout mode contract tests passed');
