// @ts-nocheck — exercise the real page handlers with fake stores and controlled timers.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = ts.createSourceFile(
  'Style.tsx',
  readFileSync(new URL('./Style.tsx', import.meta.url), 'utf8'),
  ts.ScriptTarget.ES2022,
  true,
  ts.ScriptKind.TSX,
);
const page = source.statements.find(
  (node) => ts.isFunctionDeclaration(node) && node.name?.text === 'Style',
);
const names = ['loadPacks', 'restoreDeletedPack', 'commitDeletedPack', 'handleDeleteImportedPack'];
const handlers = page.body.statements.filter(
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
  `const {loadSequence,packsLoaded,pendingDeletes,listStylePacks,setPacks,setSelectedId,
    setBusy,showSaveStatus,t,window,setUndoDelete,deleteStylePack,packs,editorOpen,
    selectedId,startEditorClose,getStylePackPresentation}=deps;
    ${compiled}; return {${names.join(',')}};`,
);
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function context() {
  const pack = { id: 'imported', name: 'Fixture', kind: 'imported', active: true };
  const other = { id: 'builtin', name: 'Builtin', kind: 'builtin', active: false };
  const state = { rows: [pack, other], selected: pack.id, undo: null, errors: [], deleted: [] };
  const pendingDeletes = { current: new Map() };
  const timers = new Map();
  let timerId = 0;
  let listing = () => Promise.resolve([pack, other]);
  let deletion = () => Promise.resolve();
  const apply = (field, value) => {
    state[field] = typeof value === 'function' ? value(state[field]) : value;
  };
  const api = instantiate({
    loadSequence: { current: 0 },
    packsLoaded: { current: true },
    pendingDeletes,
    listStylePacks: () => listing(),
    deleteStylePack: (id) => {
      state.deleted.push(id);
      return deletion();
    },
    setPacks: (value) => apply('rows', value),
    setSelectedId: (value) => apply('selected', value),
    setUndoDelete: (value) => apply('undo', value),
    setBusy: () => {},
    showSaveStatus: (...value) => state.errors.push(value),
    t: (key) => key,
    window: {
      setTimeout: (fn) => {
        timers.set(++timerId, fn);
        return timerId;
      },
      clearTimeout: (id) => timers.delete(id),
    },
    packs: state.rows,
    editorOpen: false,
    selectedId: pack.id,
    startEditorClose: () => {},
    getStylePackPresentation: (value) => ({ name: value.name }),
  });
  return {
    api,
    state,
    pack,
    other,
    pendingDeletes,
    timers,
    listing: (fn) => {
      listing = fn;
    },
    deletion: (fn) => {
      deletion = fn;
    },
  };
}

// Refresh/prefs:changed cannot resurrect or select an optimistically removed row.
{
  const c = context();
  c.api.handleDeleteImportedPack(c.pack);
  await c.api.loadPacks(c.pack.id);
  assert.deepEqual(c.state.rows, [c.other]);
  assert.equal(c.state.selected, c.other.id);
  assert.deepEqual(c.state.deleted, []);
  c.api.restoreDeletedPack(c.pack.id);
  assert.deepEqual(c.state.rows, [c.pack, c.other]);
  assert.equal(c.timers.size, 0);
}

// Committing rows remain hidden; stale Undo and repeated timer callbacks cannot race the RPC.
{
  const c = context();
  const done = deferred();
  c.deletion(() => done.promise);
  c.api.handleDeleteImportedPack(c.pack);
  c.api.commitDeletedPack(c.pack.id);
  c.api.commitDeletedPack(c.pack.id);
  c.api.restoreDeletedPack(c.pack.id);
  await c.api.loadPacks(c.pack.id);
  assert.deepEqual(c.state.rows, [c.other]);
  assert.deepEqual(c.state.deleted, [c.pack.id]);
  assert(c.pendingDeletes.current.has(c.pack.id));
  const stale = deferred();
  c.listing(() => stale.promise);
  const refresh = c.api.loadPacks(c.pack.id);
  c.listing(() => Promise.resolve([c.other]));
  done.resolve();
  await tick();
  stale.resolve([c.pack, c.other]);
  await refresh;
  assert.deepEqual(c.state.rows, [c.other]);
  assert.equal(c.pendingDeletes.current.size, 0);
}

// A failed delete restores exactly one copy and reports the failure.
{
  const c = context();
  const done = deferred();
  c.deletion(() => done.promise);
  c.api.handleDeleteImportedPack(c.pack);
  c.api.commitDeletedPack(c.pack.id);
  await c.api.loadPacks();
  done.reject(new Error('fixture failure'));
  await tick();
  assert.deepEqual(c.state.rows, [c.pack, c.other]);
  assert.equal(c.pendingDeletes.current.size, 0);
  assert.equal(c.state.errors[0][0], 'failed');
}
console.log('Style deletion lifecycle regressions passed');
