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
assert.equal(h.state.get('keyboardLayout'), 'standard');
assert.equal(h.state.get('showNumberRow'), true);
const initialResizes = h.panel.resizes;
await Promise.all([h.layout.refresh(), h.layout.refresh(), h.layout.refresh()]);
assert.equal(h.panel.resizes, initialResizes, 'native resize notifications must not cause another resize');
h.layout.select('split');
await h.layout.queue;
assert.equal(h.state.get('keyboardLayout'), 'split');
assert.equal(h.values.get('mode'), 'split');
h.layout.select('floating');
await h.layout.queue;
assert.equal(h.panel.flag, 1);
assert.ok(h.panel.width < h.screen.width);
assert.equal(h.state.get('showNumberRow'), false);
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

// Landscape must fit keys without retaining an oversized portrait number row.
Object.assign(h.screen, { width: 2560, height: 720 });
h.layout.select('standard');
await h.layout.queue;
assert.equal(h.panel.flag, 0);
assert.equal(h.state.get('showNumberRow'), false);
assert.ok(h.state.get('keyHeight') * 4 + 18 + 66 <= h.panel.height / h.screen.densityPixels);

// A native mode failure keeps the previous working mode and stored preference.
h.panel.failFloating = true;
h.layout.select('floating');
await h.layout.queue;
assert.equal(h.state.get('keyboardLayout'), 'standard');
assert.equal(h.values.get('mode'), 'standard');
assert.match(h.state.get('status'), /切换失败/);

const invalid = await host({ mode: 'unknown', scale: Infinity, x: -20, y: 99 });
assert.equal(invalid.state.get('keyboardLayout'), 'standard');
console.log('Harmony panel layout: modes, persistence, resize, drag bounds, landscape and native failure passed');
