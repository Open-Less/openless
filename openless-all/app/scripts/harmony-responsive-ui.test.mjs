import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const stored = new Map();
const storage = {
  get: (key) => stored.get(key),
  setOrCreate: (key, value) => stored.set(key, value),
};
const Store = { put: (key, value) => stored.set(key, value) };
function load(path) {
  const source = readFileSync(
    new URL(`../../harmony/entry/src/main/ets/${path}`, import.meta.url),
    'utf8',
  ).replace(/^import .*;\n/gm, '');
  const exports = {};
  const result = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  });
  vm.runInNewContext(result.outputText, { exports, AppStorage: storage, Store, console });
  return exports;
}

const { Breakpoints, WindowWatcher } = load('common/Breakpoint.ets');
const { Layout } = load('common/Theme.ets');
const { Nav } = load('components/Nav.ets');
assert.deepEqual([0, 319, 320, 599, 600, 839, 840, 1080, 1439, 1440, 1920].map(Breakpoints.of), [
  'sm',
  'sm',
  'sm',
  'sm',
  'md',
  'md',
  'lg',
  'lg',
  'lg',
  'xl',
  'xl',
]);

// Read real window dimensions, not device type or cached default-display density.
let density = 2;
let resize;
let detached = 0;
const window = {
  getWindowProperties: () => ({ windowRect: { width: 780, height: 1688 } }),
  getUIContext: () => ({ px2vp: (value) => value / density }),
  on: (event, listener) => {
    assert.equal(event, 'windowSizeChange');
    resize = listener;
  },
  off: (event, listener) => {
    assert.equal(event, 'windowSizeChange');
    assert.equal(listener, resize);
    detached++;
  },
};
const changes = [];
const watcher = new WindowWatcher();
watcher.start(window, (bp, short) => changes.push([bp, short]));
assert.deepEqual(changes, [['sm', false]], 'initial compact dimensions must publish too');
resize({ width: 800, height: 1700 });
assert.equal(changes.length, 1, 'resize inside a class must not duplicate publications');
resize({ width: 1688, height: 780 });
assert.deepEqual(changes.at(-1), ['lg', true], 'landscape uses the wider body and short headers');
resize({ width: 1688, height: 1600 });
assert.deepEqual(changes.at(-1), ['lg', false], 'height changes also update chrome');
density = 1.5;
resize({ width: 2250, height: 1500 });
assert.deepEqual(changes.at(-1), ['xl', false], 'conversion follows the active UIContext');
watcher.stop();
assert.equal(detached, 1, 'remove exactly the registered listener');
const count = changes.length;
resize({ width: 600, height: 800 });
assert.equal(changes.length, count, 'late callbacks after detach must be inert');

// Orientation decides content presentation: a portrait tablet keeps one column,
// while a wide landscape/split window can spread into panes. Both keep the dock.
{
  let portraitCb;
  const portraitWindow = {
    getWindowProperties: () => ({ windowRect: { width: 1600, height: 2560 } }),
    getUIContext: () => ({ px2vp: (value) => value / 2 }),
    on: (_event, listener) => {
      portraitCb = listener;
    },
    off: () => {},
  };
  const seen = [];
  const portraitWatcher = new WindowWatcher();
  portraitWatcher.start(portraitWindow, (bp, short, portrait) => seen.push([bp, short, portrait]));
  assert.deepEqual(seen.at(-1), ['md', false, true], 'a portrait tablet reports the phone layout');
  portraitCb({ width: 2560, height: 1600 });
  assert.deepEqual(seen.at(-1), ['lg', false, false], 'landscape enables a multi-pane body');
  portraitWatcher.stop();
}

// A bottom dock does not consume horizontal space. Reading columns and wider
// workspaces retain separate budgets rather than reserving a retired side rail.
for (const width of [320, 390, 600, 839, 840, 1080, 1439, 1440, 1920]) {
  const bp = Breakpoints.of(width);
  const body = Math.min(width, Layout.bodyMax(bp, 'history')) - 2 * Layout.gutter(bp);
  if (width >= 840) {
    assert.ok(
      body - 360 - Layout.columnGap(bp) >= 320,
      `history detail needs at least 320vp after list/gutter at ${width}vp`,
    );
    assert.ok(Layout.bodyMax(bp, 'overview') > Layout.contentMax(bp));
  }
  assert.equal(
    Layout.bodyMax(bp, 'credentials'),
    Layout.contentMax(bp),
    'forms retain readable width',
  );
}
assert.deepEqual(Array.from(Nav.PRIMARY), ['overview', 'history', 'vocab', 'style', 'settings']);
assert.equal(Nav.ownerOf(Nav.CORRECTIONS), Nav.VOCAB);
assert.equal(
  Nav.ownerOf(Nav.TRANSLATION),
  Nav.TRANSLATION,
  'translation targets is a dock destination',
);
assert.equal(Nav.rootOf(Nav.LOCALASR), Nav.SETTINGS);
stored.set('route', Nav.HISTORY);
stored.set('detailId', 'history-42');
Nav.go(Nav.HISTORY);
assert.equal(
  stored.get('detailId'),
  'history-42',
  'reselecting a tab must not erase its selection',
);
Nav.go(Nav.VOCAB);
assert.equal(stored.get('detailId'), '');
Nav.go(Nav.CORRECTIONS);
Nav.go(Nav.SETTINGS);
Nav.openPrimary(Nav.VOCAB);
assert.equal(stored.get('route'), Nav.CORRECTIONS, 'restore each destination navigation path');
assert.equal(Nav.back(), true);
assert.equal(stored.get('route'), Nav.VOCAB, 'system back returns to owning primary, not home');
Nav.openPrimary(Nav.HISTORY);
assert.equal(
  stored.get('detailId'),
  'history-42',
  'restore the history selection across destinations',
);
assert.equal(Nav.back(), true);
assert.equal(stored.get('detailId'), '');
Nav.go(Nav.TRANSLATION);
assert.equal(Nav.back(), true, 'translation is its own dock destination, so back leaves it');
assert.equal(stored.get('route'), Nav.OVERVIEW);
Nav.go(Nav.OVERVIEW);
assert.equal(Nav.back(), false, 'root home delegates exit to the system');
console.log(
  'Harmony responsive UI: window reflow, density, height, width budgets, navigation restoration and back passed',
);
