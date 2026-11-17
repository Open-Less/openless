// @ts-nocheck — exercise the real workspace handlers with fake stores and controlled timers.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { createInteractionController } from '../ui/overlays/interactionController';

const source = ts.createSourceFile(
  'useStylePackWorkspace.ts',
  readFileSync(
    new URL('../features/style-packs/useStylePackWorkspace.ts', import.meta.url),
    'utf8',
  ),
  ts.ScriptTarget.ES2022,
  true,
  ts.ScriptKind.TS,
);
const workspace = source.statements.find(
  (node) => ts.isFunctionDeclaration(node) && node.name?.text === 'useStylePackWorkspace',
);
const names = [
  'showSaveStatus',
  'loadPacks',
  'restoreDeletedPack',
  'reinsertPack',
  'commitDeletedPack',
  'handleDeleteImportedPack',
];
const undoDuration = source.statements.find(
  (node) =>
    ts.isVariableStatement(node) &&
    node.declarationList.declarations.some(
      (value) => value.name.getText(source) === 'DELETE_UNDO_MS',
    ),
);
const handlers = workspace.body.statements.filter(
  (node) =>
    ts.isVariableStatement(node) &&
    node.declarationList.declarations.some((value) => names.includes(value.name.getText(source))),
);
assert.equal(handlers.length, names.length);
const compiled = ts.transpileModule(
  [undoDuration, ...handlers].map((node) => node.getText(source)).join('\n'),
  { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } },
).outputText;
const instantiate = new Function(
  'deps',
  `const {loadSequence,packsLoaded,pendingDeletes,listStylePacks,setPacks,setSelectedId,selectedIdRef,
    setBusy,notify,t,window,deleteStylePack,packs,editorOpen,
    selectedId,startEditorClose,getStylePackPresentation,cardRefs,cardDeparture,mounted}=deps;
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
  const state = { rows: [pack, other], selected: pack.id, deleted: [] };
  const pendingDeletes = { current: new Map() };
  const notifications = createInteractionController();
  const scope = notifications.createScope();
  const selectedIdRef = { current: pack.id };
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
    cardRefs: { current: new Map() },
    cardDeparture: () => Promise.resolve(),
    mounted: { current: true },
    listStylePacks: () => listing(),
    deleteStylePack: (id) => {
      state.deleted.push(id);
      return deletion();
    },
    setPacks: (value) => apply('rows', value),
    selectedIdRef,
    setSelectedId: (value) => {
      apply('selected', value);
      selectedIdRef.current = state.selected;
    },
    setBusy: () => {},
    notify: scope.interaction.notify,
    t: (key) => key,
    window: {
      setTimeout: (fn, delay) => {
        const id = ++timerId;
        timers.set(id, {
          delay,
          run: () => {
            timers.delete(id);
            fn();
          },
        });
        return id;
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
    notices: notifications.getNotifications,
    dispose: scope.dispose,
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
  await c.api.handleDeleteImportedPack(c.pack);
  const [notice] = c.notices();
  assert.equal(c.notices().length, 1);
  assert.equal(notice.state, 'deleting');
  assert.equal(notice.message, 'style.pack.deleteImported: Fixture');
  assert.equal(notice.actionLabel, 'style.pack.undoDelete');
  assert.equal([...c.timers.values()][0].delay, 6000);
  await c.api.loadPacks(c.pack.id);
  assert.deepEqual(c.state.rows, [c.other]);
  assert.equal(c.state.selected, c.other.id);
  assert.deepEqual(c.state.deleted, []);
  notice.onAction();
  assert.deepEqual(c.state.rows, [c.pack, c.other]);
  assert.deepEqual(c.state.deleted, [], 'Undo cancels before calling the native delete');
  assert.deepEqual(c.notices(), []);
  assert.equal(c.timers.size, 0);
  c.dispose();
}

// Committing rows remain hidden; stale Undo and repeated timer callbacks cannot race the RPC.
{
  const c = context();
  const done = deferred();
  c.deletion(() => done.promise);
  await c.api.handleDeleteImportedPack(c.pack);
  [...c.timers.values()][0].run();
  c.api.commitDeletedPack(c.pack.id);
  c.api.restoreDeletedPack(c.pack.id);
  await c.api.loadPacks(c.pack.id);
  assert.deepEqual(c.state.rows, [c.other]);
  assert.deepEqual(c.state.deleted, [c.pack.id]);
  assert.deepEqual(
    c.notices(),
    [],
    'the Undo window closed but the native delete has not succeeded',
  );
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
  assert.deepEqual(
    c.notices().map(({ state, message }) => ({ state, message })),
    [{ state: 'deleted', message: 'style.pack.deleteSuccess' }],
  );
  c.dispose();
}

// A failed delete restores exactly one copy and reports the failure.
{
  const c = context();
  const done = deferred();
  c.deletion(() => done.promise);
  await c.api.handleDeleteImportedPack(c.pack);
  c.api.commitDeletedPack(c.pack.id);
  await c.api.loadPacks();
  done.reject(new Error('fixture failure'));
  await tick();
  assert.deepEqual(c.state.rows, [c.pack, c.other]);
  assert.equal(c.pendingDeletes.current.size, 0);
  assert.deepEqual(
    c.notices().map(({ state, message }) => ({ state, message })),
    [{ state: 'failed', message: 'style.pack.deleteFailed' }],
  );
  c.dispose();
}
console.log('Style deletion lifecycle regressions passed');
