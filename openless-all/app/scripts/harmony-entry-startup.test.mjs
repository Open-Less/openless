import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../harmony/entry/src/main/ets/entryability/EntryAbility.ets', import.meta.url),
  'utf8',
)
  .replace(/\r\n/g, '\n')
  .replace(/^import .*;\n/gm, '');
let clipboardAccess = false;
const requests = [],
  pages = [];
class UIAbility {
  context = { resourceManager: { getBooleanByName: () => clipboardAccess } };
}
// `onCreate` records launch parameters for the page (`--ps route`, `--ps layout`)
// instead of touching AppStorage before a UI instance exists.
const boot = { route: '', layout: '' };
const Boot = {
  get route() {
    return boot.route;
  },
  set route(value) {
    boot.route = value;
  },
  get layout() {
    return boot.layout;
  },
  set layout(value) {
    boot.layout = value;
  },
  takeRoute: () => {
    const value = boot.route;
    boot.route = '';
    return value;
  },
  takeLayout: () => {
    const value = boot.layout;
    boot.layout = '';
    return value;
  },
};
const stored = new Map();
const Store = { put: (key, value) => stored.set(key, value) };
const exports = {};
vm.runInNewContext(
  ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText,
  {
    exports,
    UIAbility,
    Boot,
    Store,
    console,
    abilityAccessCtrl: {
      createAtManager: () => ({
        requestPermissionsFromUser: async (_context, permissions) => {
          requests.push(Array.from(permissions));
          return { authResults: [0] };
        },
      }),
    },
  },
);
const stage = {
  loadContent: (page, ready) => {
    pages.push(page);
    ready();
  },
};
for (const want of [undefined, {}, { parameters: {} }]) {
  const ability = new exports.default();
  ability.onCreate(want);
  assert.equal(Boot.route, '', 'a plain launch must not carry a deep link');
  ability.onForeground();
  ability.onWindowStageCreate(stage);
  ability.onNewWant();
  ability.onForeground();
}
assert.deepEqual(pages, ['pages/Index', 'pages/Index', 'pages/Index']);
assert.equal(requests.length, 0, 'normal launches must not request clipboard access');
const ability = new exports.default();
ability.onCreate({ parameters: { requestClipboardPermission: true } });
ability.onWindowStageCreate(stage);
assert.equal(
  requests.length,
  0,
  'the default build must ignore clipboard authorization deep links',
);
clipboardAccess = true;
ability.onNewWant({ parameters: { requestClipboardPermission: true } });
ability.onForeground();
ability.onWindowStageCreate(stage);
assert.deepEqual(
  requests,
  [['ohos.permission.READ_PASTEBOARD']],
  'authorized builds request only once per explicit action',
);
// Launch parameters are forwarded for the page to honour, and never for a plain
// launch: `--ps route`/`--ps layout` are the launcher and QA entry points.
{
  const ability = new exports.default();
  ability.onCreate({ parameters: { route: 'settings', layout: 'sm' } });
  assert.equal(Boot.takeRoute(), 'settings');
  assert.equal(Boot.takeLayout(), 'sm');
  const plain = new exports.default();
  plain.onCreate({ parameters: { route: 'settings' } });
  assert.equal(Boot.takeLayout(), '', 'an unrelated launch must not pin a layout');
}
console.log(
  'Harmony entry startup: absent launch arguments, page loading, deep links and clipboard capability gating passed',
);
