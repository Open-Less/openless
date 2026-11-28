import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../../harmony/entry/src/main/ets/entryability/EntryAbility.ets', import.meta.url),
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
// Launch hints are routes only; window geometry is owned by the stage.
const Boot = {
  route: '',
  mainWindow: undefined,
  takeRoute() {
    const value = this.route;
    this.route = '';
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
const mainWindow = { name: 'owning-main-window' };
const stage = {
  getMainWindowSync: () => mainWindow,
  loadContent: (page, ready) => {
    assert.equal(Boot.mainWindow, mainWindow, 'window is available before loading Index');
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
// Old QA layout arguments are deliberately ignored, including on a warm launch.
{
  const ability = new exports.default();
  ability.onCreate({ parameters: { route: 'settings', layout: 'sm' } });
  assert.equal(Boot.takeRoute(), 'settings');
  stored.delete('bpOverride');
  ability.onNewWant({ parameters: { route: 'history', layout: 'xl' } });
  assert.equal(stored.get('deepLinkRoute'), 'history');
  assert.equal(stored.has('bpOverride'), false, 'launch arguments cannot lock responsive layout');
  ability.onWindowStageCreate(stage);
  assert.equal(Boot.mainWindow, mainWindow);
  ability.onWindowStageDestroy();
  assert.equal(Boot.mainWindow, undefined, 'destroyed window references must be released');
}
console.log(
  'Harmony entry startup: absent launch arguments, page loading, deep links and clipboard capability gating passed',
);
