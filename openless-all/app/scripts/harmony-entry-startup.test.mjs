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
const exports = {};
vm.runInNewContext(
  ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText,
  {
    exports,
    UIAbility,
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
console.log(
  'Harmony entry startup: absent launch arguments, page loading and clipboard capability gating passed',
);
