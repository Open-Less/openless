import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(
  new URL('../src/pages/settings/RecordingInputSection.tsx', import.meta.url),
  'utf8',
);
const ast = ts.createSourceFile(
  'RecordingInputSection.tsx',
  source,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX,
);
const notice = ast.statements.find(
  (node) => ts.isFunctionDeclaration(node) && node.name?.text === 'LowMemoryRuntimeNotice',
);
let effect;
function visit(node) {
  if (ts.isCallExpression(node) && node.expression.getText(ast) === 'useEffect')
    effect = node.arguments[0].getText(ast);
  ts.forEachChild(node, visit);
}
visit(notice);
assert.ok(effect, 'execute the production health subscription');
const code = ts.transpileModule('globalThis.effect = (' + effect + ');', {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const tick = () => new Promise((resolve) => setImmediate(resolve));
function mount() {
  const requests = [];
  const writes = [];
  let listener;
  let disposed = 0;
  const context = {
    isTauri: true,
    console,
    setRuntime: (value) => writes.push(value),
    getNativeCapsuleStatus: () => new Promise((resolve) => requests.push(resolve)),
    require: () => ({
      listen: async (name, callback) => {
        assert.equal(name, 'native-capsule:health-changed');
        listener = callback;
        return () => {
          disposed += 1;
        };
      },
    }),
  };
  vm.runInNewContext(code, context);
  const cleanup = context.effect();
  return { requests, writes, cleanup, emit: () => listener(), disposed: () => disposed };
}
const healthy = { active: true, available: true, error: null };
const failed = { active: true, available: false, error: 'capsule handshake timed out' };
{
  const view = mount();
  await tick();
  view.requests[0](healthy);
  await tick();
  view.emit();
  view.requests[1](failed);
  await tick();
  assert.equal(view.writes.at(-1), failed);
  view.emit();
  view.requests[2](healthy);
  await tick();
  assert.equal(view.writes.at(-1), healthy);
  view.cleanup();
  assert.equal(view.disposed(), 1);
}
{
  const view = mount();
  await tick();
  view.emit();
  view.requests[1](failed);
  await tick();
  view.requests[0](healthy);
  await tick();
  assert.equal(view.writes.at(-1), failed, 'old healthy reply must not hide a new failure');
  view.emit();
  view.cleanup();
  view.requests[2](healthy);
  await tick();
  assert.equal(view.writes.length, 1, 'unmounted notice must ignore replies');
}
{
  const view = mount();
  view.cleanup();
  await tick();
  assert.equal(view.disposed(), 1);
  assert.equal(view.requests.length, 0);
}
console.log('native capsule health: live failure, recovery, response ordering and cleanup passed');
