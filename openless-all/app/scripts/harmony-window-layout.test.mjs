// Execute the real layout watcher and page lifecycle against deterministic windows.
// This verifies responsive state wiring, not device animation smoothness.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const base = new URL('../../harmony/entry/src/main/ets/', import.meta.url);
const read = (path) => readFileSync(new URL(path, base), 'utf8');
function load(source, globals = {}) {
  const exports = {};
  const compiled = ts.transpileModule(source.replace(/^import .*;\n/gm, ''), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  assert.equal(compiled.diagnostics?.length ?? 0, 0, 'test source must transpile');
  vm.runInNewContext(compiled.outputText, { exports, console, ...globals });
  return exports;
}
function fakeWindow(width, height, density = 2) {
  const listeners = new Set();
  const keyboardListeners = new Set();
  return {
    width,
    height,
    listeners,
    keyboardListeners,
    onCount: 0,
    offCount: 0,
    keyboardModes: [],
    getWindowProperties() {
      return { windowRect: { width: this.width, height: this.height } };
    },
    getUIContext() {
      return {
        px2vp: (px) => px / density,
        setKeyboardAvoidMode: (mode) => this.keyboardModes.push(mode),
      };
    },
    on(name, listener) {
      assert.ok(['windowSizeChange', 'keyboardHeightChange'].includes(name));
      this.onCount++;
      (name === 'keyboardHeightChange' ? keyboardListeners : listeners).add(listener);
    },
    off(name, listener) {
      assert.ok(['windowSizeChange', 'keyboardHeightChange'].includes(name));
      this.offCount++;
      assert.ok(
        (name === 'keyboardHeightChange' ? keyboardListeners : listeners).delete(listener),
        'off must use the exact subscribed callback',
      );
    },
    resize(width, height) {
      this.width = width;
      this.height = height;
      for (const listener of listeners) listener({ width, height });
    },
  };
}
const { Breakpoints, WindowWatcher } = load(read('common/Breakpoint.ets'));
for (const [width, expected] of [
  [0, 'sm'],
  [599, 'sm'],
  [600, 'md'],
  [839, 'md'],
  [840, 'lg'],
  [1439, 'lg'],
  [1440, 'xl'],
]) {
  assert.equal(Breakpoints.of(width), expected);
}
const changes = [];
const watcher = new WindowWatcher();
const tablet = fakeWindow(1600, 2560);
watcher.start(tablet, (...state) => changes.push(state));
assert.deepEqual(changes.at(-1), ['md', false, true]);
tablet.resize(2560, 1600);
assert.deepEqual(changes.at(-1), ['lg', false, false]);
tablet.resize(1600, 2560);
assert.deepEqual(changes.at(-1), ['md', false, true]);
const sameCount = changes.length;
tablet.resize(1600, 2560);
assert.equal(changes.length, sameCount, 'duplicate size state is ignored');
// Orientation must update even when both widths remain in the same size class.
tablet.resize(1800, 2200);
assert.deepEqual(changes.at(-1), ['lg', false, true]);
tablet.resize(2200, 1800);
assert.deepEqual(changes.at(-1), ['lg', false, false]);
tablet.resize(2200, 800);
assert.deepEqual(changes.at(-1), ['lg', true, false]);
const validCount = changes.length;
tablet.resize(0, 0);
assert.equal(changes.length, validCount, 'transient zero bounds are ignored');
const stale = [...tablet.listeners][0];
const dense = fakeWindow(1800, 3000, 3);
watcher.start(dense, (...state) => changes.push(state));
assert.equal(tablet.listeners.size, 0, 'rebinding detaches the old main window');
assert.deepEqual(changes.at(-1), ['md', false, true], 'conversion uses this window density');
const reboundCount = changes.length;
stale({ width: 4000, height: 1000 });
assert.equal(changes.length, reboundCount, 'queued events from the previous window are rejected');
watcher.stop();
assert.equal(dense.listeners.size, 0);
watcher.stop(); // Idempotent teardown.

// Execute Index before its backend and declarative builders; neither is replaced
// in production. Strip decorators only, retaining the actual lifecycle methods.
const index = read('pages/Index.ets');
const pageSource =
  index
    .slice(0, index.indexOf('  private async boot()'))
    .replace(/^@(Entry|Component)\n/gm, '')
    .replace(/@(StorageLink|Watch)\('[^']*'\)\s*/g, '')
    .replace(/@State\s+/g, '')
    .replace('struct Index', 'export class Index') + '  private boot(): void {}\n}\n';
const storage = new Map([['bpOverride', 'xl']]); // Obsolete QA state cannot pin the new UI.
const put = (key, value) => storage.set(key, value);
const main = fakeWindow(1600, 2560);
const Boot = { mainWindow: main, takeRoute: () => 'history' };
let released = 0;
const Nav = { OVERVIEW: 'overview', known: () => true, go: () => {}, back: () => false };
const { Index } = load(pageSource, {
  Boot,
  WindowWatcher,
  Nav,
  KeyboardAvoidMode: { RESIZE: 'resize' },
  Store: { put },
  AppStorage: { get: (key) => storage.get(key), setOrCreate: put },
  Api: { bridgeOf: () => ({ release: () => released++ }) },
});
const page = new Index();
page.aboutToAppear();
assert.equal(storage.get('bp'), 'md');
assert.equal(storage.get('phoneLayout'), true);
assert.deepEqual(
  main.keyboardModes,
  ['resize'],
  'use native RESIZE, never manually pan floating inputs',
);
assert.equal(main.keyboardListeners.size, 1);
const keyboardListener = [...main.keyboardListeners][0];
keyboardListener(500);
assert.equal(storage.get('keyboardVisible'), true);
keyboardListener(0);
assert.equal(storage.get('keyboardVisible'), false);
put('detailId', 'keep-history-detail');
put('route', 'history');
main.resize(2560, 1600);
assert.equal(storage.get('bp'), 'lg');
assert.equal(storage.get('phoneLayout'), false);
assert.equal(storage.get('route'), 'history');
assert.equal(storage.get('detailId'), 'keep-history-detail', 'rotation does not reset navigation');
main.resize(1600, 2560);
assert.equal(storage.get('phoneLayout'), true);
main.resize(3000, 4000);
assert.equal(storage.get('bp'), 'xl');
assert.equal(storage.get('phoneLayout'), true, 'portrait tablets remain single-column even at XL');
page.onPageHide();
assert.equal(storage.get('appForeground'), false);
// Simulate a background window resize for which no callback was delivered.
main.width = 2560;
main.height = 1600;
page.onPageShow();
assert.equal(storage.get('bp'), 'lg');
assert.equal(storage.get('phoneLayout'), false);
assert.equal(main.listeners.size, 1, 'foreground rebinding cannot accumulate listeners');
page.aboutToDisappear();
assert.equal(main.listeners.size, 0);
assert.equal(main.keyboardListeners.size, 0);
keyboardListener(500);
assert.equal(storage.get('keyboardVisible'), false, 'detached keyboard callback must stay inert');
assert.equal(released, 1);

const columns = read('components/AdaptiveLayout.ets');
assert.equal((columns.match(/this\.primary\(\)/g) ?? []).length, 1);
assert.equal((columns.match(/this\.secondary\(\)/g) ?? []).length, 1);
assert.match(columns, /FlexDirection\.Row\s*:\s*FlexDirection\.Column/);
assert.doesNotMatch(
  columns,
  /animateTo\(|\.animation\(|\.transition\(/,
  'rotation must not queue a second full-page animation',
);
assert.doesNotMatch(index, /getLastWindow|bpOverride|takeLayout|onLayoutPin/);
const columnsHead =
  columns
    .slice(0, columns.indexOf('  build()'))
    .replace(/^@Component\n/gm, '')
    .replace(/@Prop\s+|@BuilderParam\s+/g, '')
    .replace('export struct AdaptiveColumns', 'export class AdaptiveColumns') + '\n}\n';
const { AdaptiveColumns } = load(columnsHead, {
  Layout: { SM: 'sm', isWide: (bp) => bp === 'lg' || bp === 'xl' },
});
const adaptive = new AdaptiveColumns();
adaptive.bp = 'xl';
adaptive.singleColumn = true;
assert.equal(adaptive.wide(), false, 'portrait mode must reach the reusable column container');
adaptive.singleColumn = false;
assert.equal(adaptive.wide(), true);
adaptive.bp = 'sm';
assert.equal(adaptive.wide(), false, 'a narrow landscape window stays single-column');
for (const file of ['pages/OverviewPage.ets', 'pages/ModelsPage.ets']) {
  assert.match(read(file), /singleColumn:\s*this\.phoneLayout/);
}
const shell = read('components/AppShell.ets');
assert.doesNotMatch(
  shell,
  /padding\(\{ bottom: DOCK_CLEARANCE/,
  'floating navigation must not shrink the entire content viewport',
);
assert.doesNotMatch(
  shell,
  /Layout\.columnMax/,
  'portrait tablets must not be squeezed into a phone reading column',
);
assert.match(shell, /shell-content-viewport/);
assert.match(shell, /shell-floating-dock/);
assert.match(read('pages/HistoryPage.ets'), /contentEndOffset\(Layout\.dockClearance\(\)\)/);
for (const file of [
  'pages/OverviewPage.ets',
  'pages/SettingsPage.ets',
  'components/Kit.ets',
  'components/history/HistoryDetail.ets',
]) {
  assert.match(
    read(file),
    /bottom: Layout\.dockClearance\(\)/,
    `${file} must reserve space at the scroll tail, not around the viewport`,
  );
}
console.log(
  'Harmony window layout: live rotation, same-breakpoint orientation, density, lifecycle, stale events and state-preserving reflow passed',
);
