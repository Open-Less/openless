import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../harmony/entry/src/main/ets/inputmethod/PanelLayout.ets', import.meta.url), 'utf8');
const stripImports = (text) => text.replace(/\r\n/g, '\n').replace(/^import .*;\n/gm, '');
assert.equal(stripImports(source), stripImports(source.replace(/\r?\n/g, '\r\n')));
const compiled = ts.transpileModule(stripImports(source), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

async function host(saved = {}) {
  const state = new Map();
  const values = new Map(Object.entries(saved));
  const screen = { width: 1600, height: 2560, densityPixels: 2.25 };
  const panel = {
    flag: 0, width: 0, height: 0, x: 0, y: 0, failFloating: false, resizes: 0,
    changeFlag(flag) { if (flag === 1 && this.failFloating) throw Error('unsupported'); this.flag = flag; },
    async resize(width, height) {
      this.resizes++;
      assert.ok(width > 0 && width <= screen.width);
      assert.ok(height > 0 && height <= screen.height * 0.7);
      this.width = width; this.height = height;
    },
    async moveTo(x, y) {
      assert.equal(this.flag, 1);
      assert.ok(x >= 0 && x + this.width <= screen.width);
      assert.ok(y >= 0 && y + this.height <= screen.height);
      this.x = x; this.y = y;
    },
  };
  const exports = {};
  vm.runInNewContext(compiled, {
    exports, console,
    inputMethodEngine: { PanelFlag: { FLG_FIXED: 0, FLG_FLOATING: 1 } },
    display: { getDefaultDisplaySync: () => screen },
    imeStorage: { setOrCreate: (key, value) => state.set(key, value) },
    preferences: { getPreferences: async () => ({
      get: async (key, fallback) => values.has(key) ? values.get(key) : fallback,
      put: async (key, value) => values.set(key, value), flush: async () => {},
    }) },
  });
  const layout = exports.panelLayout;
  await layout.attach(panel, {});
  return { layout, panel, screen, state, values };
}

const h = await host();
function adaptiveGeometry(h) {
  const size = h.state.get('keyHeight');
  const keyWidth = h.state.get('keyWidth');
  const split = h.state.get('keyboardLayout') === 'split';
  const gap = h.state.get('keyboardLayout') === 'standard' ? 7 : 5;
  const width = h.state.get('keyboardWidth');
  assert.equal(width, (split ? 12 : 10) * keyWidth + (split ? 10 : 9) * gap);
  const expectedWidth = h.state.get('keyboardLayout') === 'onehand' ?
    Math.min(420, h.panel.width / h.screen.densityPixels * 0.72) : h.panel.width / h.screen.densityPixels;
  assert.ok(Math.abs(width + 12 - expectedWidth) < 0.01, 'keys must fill the selected layout width');
  if (h.state.get('keyboardLayout') === 'floating') assert.ok(size <= keyWidth + 0.01);
  assert.ok(size >= 12);
  const rows = h.state.get('showNumberRow') ? 5 : 4;
  const fitted = rows * size + (rows - 1) * 7 + 66 + (h.state.get('keyboardLayout') === 'floating' ? 26 : 0);
  assert.ok(Math.abs(h.panel.height / h.screen.densityPixels - fitted) < 0.5,
    'panel height must follow the fitted rows rather than leaving an empty band');
}
adaptiveGeometry(h);
assert.equal(h.state.get('keyboardLayout'), 'standard');
assert.equal(h.state.get('showNumberRow'), true);
const standardHeight = h.panel.height;
const initialResizes = h.panel.resizes;
await Promise.all([h.layout.refresh(), h.layout.refresh(), h.layout.refresh()]);
assert.equal(h.panel.resizes, initialResizes, 'native resize notifications must not cause another resize');
h.layout.select('split');
await h.layout.queue;
assert.equal(h.state.get('keyboardLayout'), 'split');
assert.equal(h.values.get('mode'), 'split');
assert.equal(h.panel.height, standardHeight);
adaptiveGeometry(h);
h.layout.select('onehand');
await h.layout.queue;
assert.equal(h.state.get('keyboardLayout'), 'onehand');
assert.equal(h.panel.height, standardHeight);
assert.equal(h.panel.flag, 0);
assert.ok(h.state.get('keyboardWidth') < h.panel.width / h.screen.densityPixels - 12);
adaptiveGeometry(h);
const restoredOnehand = await host(Object.fromEntries(h.values));
assert.equal(restoredOnehand.state.get('keyboardLayout'), 'onehand');
adaptiveGeometry(restoredOnehand);
h.layout.select('floating');
await h.layout.queue;
assert.equal(h.panel.flag, 1);
assert.ok(h.panel.width < h.screen.width);
assert.equal(h.state.get('showNumberRow'), false);
adaptiveGeometry(h);
const originalWidth = h.panel.width;
h.layout.resize(0.1);
await h.layout.queue;
assert.ok(h.panel.width > originalWidth);
h.layout.beginMove();
h.layout.drag(-10000, 10000);
h.layout.endMove();
await h.layout.queue;
assert.equal(h.panel.x, 0);
assert.equal(h.panel.y + h.panel.height, h.screen.height);

// Restore the actual mode, size, and position in a new extension instance.
const restored = await host(Object.fromEntries(h.values));
assert.equal(restored.panel.flag, 1);
assert.equal(restored.panel.width, h.panel.width);
assert.equal(restored.panel.x, h.panel.x);
assert.equal(restored.panel.y, h.panel.y);
adaptiveGeometry(restored);

// Landscape must fit keys without retaining an oversized portrait number row.
Object.assign(h.screen, { width: 2560, height: 720 });
h.layout.select('standard');
await h.layout.queue;
adaptiveGeometry(h);
assert.equal(h.panel.flag, 0);
assert.equal(h.state.get('showNumberRow'), false);
assert.ok(h.state.get('keyHeight') * 4 + 18 + 66 <= h.panel.height / h.screen.densityPixels);

const landscapeHeight = h.panel.height;
h.layout.select('split');
await h.layout.queue;
adaptiveGeometry(h);
assert.equal(h.panel.height, landscapeHeight);
h.layout.select('standard');
await h.layout.queue;

// A native mode failure keeps the previous working mode and stored preference.
h.panel.failFloating = true;
h.layout.select('floating');
await h.layout.queue;
assert.equal(h.state.get('keyboardLayout'), 'standard');
assert.equal(h.values.get('mode'), 'standard');
assert.match(h.state.get('status'), /切换失败/);

const narrow = await host();
Object.assign(narrow.screen, { width: 720, height: 1600 });
await narrow.layout.refresh();
const narrowHeight = narrow.panel.height;
narrow.layout.select('split');
await narrow.layout.queue;
adaptiveGeometry(narrow);
assert.equal(narrow.panel.height, narrowHeight);

const invalid = await host({ mode: 'unknown', scale: Infinity, x: -20, y: 99 });
assert.equal(invalid.state.get('keyboardLayout'), 'standard');
console.log('Harmony panel layout: modes, persistence, resize, drag bounds, landscape and native failure passed');
