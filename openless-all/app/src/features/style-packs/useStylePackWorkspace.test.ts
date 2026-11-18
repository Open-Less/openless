// @ts-nocheck — exercise the workspace handlers with controlled store responses.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { createInteractionController } from '../../ui/overlays/interactionController';

const source = ts.createSourceFile(
  'useStylePackWorkspace.ts',
  readFileSync(new URL('./useStylePackWorkspace.ts', import.meta.url), 'utf8'),
  ts.ScriptTarget.ES2022,
  true,
  ts.ScriptKind.TS,
);
const workspace = source.statements.find(
  (node) => ts.isFunctionDeclaration(node) && node.name?.text === 'useStylePackWorkspace',
);
const names = ['showSaveStatus', 'loadPacks', 'handleDeleteImportedPack'];
const handlers = workspace.body.statements.filter(
  (node) =>
    ts.isVariableStatement(node) &&
    node.declarationList.declarations.some((value) => names.includes(value.name.getText(source))),
);
assert.equal(handlers.length, names.length);
const compiled = ts.transpileModule(handlers.map((node) => node.getText(source)).join('\n'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText;
const instantiate = new Function(
  'deps',
  `const {loadSequence,packsLoaded,deleteRequest,listStylePacks,setPacks,setSelectedId,selectedIdRef,
    setBusy,notify,confirm,t,deleteStylePack,editorOpen,startEditorClose,getStylePackPresentation,mounted}=deps;
    ${compiled}; return {${names.join(',')}};`,
);

const pack = { id: 'imported', name: 'Fixture', kind: 'imported', active: true };
const other = { id: 'builtin', name: 'Builtin', kind: 'builtin', active: false };
let rows = [pack, other];
const deleted = [];
const controller = createInteractionController();
const scope = controller.createScope();
const selectedIdRef = { current: pack.id };
let listed = [pack, other];
let fail = true;
let release;
const api = instantiate({
  ...scope.interaction,
  loadSequence: { current: 0 },
  packsLoaded: { current: true },
  deleteRequest: { current: false },
  mounted: { current: true },
  listStylePacks: async () => listed,
  deleteStylePack: async (id) => {
    deleted.push(id);
    if (fail) throw new Error('fixture failure');
    await new Promise((resolve) => {
      release = resolve;
    });
    listed = [other];
  },
  setPacks: (value) => {
    rows = typeof value === 'function' ? value(rows) : value;
  },
  selectedIdRef,
  setSelectedId: (value) => {
    selectedIdRef.current = value;
  },
  setBusy: () => {},
  t: (key) => key,
  editorOpen: false,
  startEditorClose: () => {},
  getStylePackPresentation: (value) => ({ name: value.name }),
});

await api.handleDeleteImportedPack(other);
assert.equal(controller.getRequest(), null, 'Built-in packs cannot enter the destructive flow');
let finished = api.handleDeleteImportedPack(pack);
controller.getRequest().cancel();
await finished;
assert.deepEqual(deleted, [], 'Cancellation performs no deletion');
assert.deepEqual(rows, [pack, other]);

finished = api.handleDeleteImportedPack(pack);
const request = controller.getRequest();
assert.equal(request.options.tone, 'danger');
await assert.rejects(request.options.onConfirm(), /fixture failure/);
assert.deepEqual(rows, [pack, other], 'The row is kept when the business action rejects');
assert.deepEqual(controller.getNotifications(), []);

fail = false;
const committed = request.options.onConfirm();
assert.deepEqual(rows, [pack, other], 'The row remains visible while native deletion is pending');
assert.deepEqual(controller.getNotifications(), []);
release();
await committed;
request.accept();
await finished;
assert.deepEqual(rows, [other]);
assert.equal(selectedIdRef.current, other.id);
assert.deepEqual(deleted, [pack.id, pack.id]);
assert.equal(controller.getNotifications()[0].state, 'deleted');
assert.equal(controller.getNotifications()[0].actionLabel, undefined);
scope.dispose();
