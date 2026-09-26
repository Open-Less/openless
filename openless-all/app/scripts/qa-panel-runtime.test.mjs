import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
const app = fileURLToPath(new URL('../', import.meta.url)),
  require = createRequire(resolve(app, 'package.json')),
  ts = require('typescript');
const helpers = await import(pathToFileURL(resolve(app, 'src/lib/qaMessage.ts')));
const source = readFileSync(resolve(app, 'src/pages/QaPanel.tsx'), 'utf8');
const parsed = ts.createSourceFile(
  'QaPanel.tsx',
  source,
  ts.ScriptTarget.ES2020,
  true,
  ts.ScriptKind.TSX,
);
const names = parsed.statements
  .filter(ts.isImportDeclaration)
  .flatMap((n) =>
    n.importClause?.namedBindings && ts.isNamedImports(n.importClause.namedBindings)
      ? n.importClause.namedBindings.elements.map((n) => n.name.text)
      : [],
  );
const body = parsed.statements
  .filter((n) => !ts.isImportDeclaration(n))
  .map((n) => n.getFullText(parsed))
  .join('\n')
  .replaceAll("import('@tauri-apps/api/event')", 'Promise.resolve({listen: __listen})');
const compiled = ts.transpileModule(body, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const factory = new Function(
  ...names,
  '__listen',
  'exports',
  'require',
  `${compiled};return {QaPanel,Composer,MessageRow,ErrorContent};`,
);
const tick = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
};
const jsx = (type, props) => ({ type, props: props ?? {} });
function nodes(tree) {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!tree || typeof tree !== 'object' || !tree.props) return [];
  return [tree, ...nodes(tree.props.children)];
}
function find(tree, predicate) {
  const node = nodes(tree).find(predicate);
  assert(node, 'expected control');
  return node;
}
let active;
class Hooks {
  slots = [];
  cursor = 0;
  pending = [];
  state(initial) {
    const i = this.cursor++;
    this.slots[i] ??= { value: typeof initial === 'function' ? initial() : initial };
    return [
      this.slots[i].value,
      (value) => {
        this.slots[i].value = typeof value === 'function' ? value(this.slots[i].value) : value;
      },
    ];
  }
  ref(initial) {
    return this.state({ current: initial })[0];
  }
  effect(fn, deps) {
    const i = this.cursor++,
      prev = this.slots[i];
    if (
      prev &&
      deps &&
      prev.deps?.length === deps.length &&
      deps.every((v, i) => Object.is(v, prev.deps[i]))
    )
      return;
    const slot = { deps };
    this.slots[i] = slot;
    this.pending.push(() => {
      prev?.cleanup?.();
      slot.cleanup = fn();
    });
  }
  render(fn) {
    this.cursor = 0;
    active = this;
    const tree = fn();
    this.pending.splice(0).forEach((fn) => fn());
    return tree;
  }
  unmount() {
    this.slots.forEach((s) => s.cleanup?.());
  }
}
function context(native = true, embedded = false) {
  const events = new Map(),
    keys = new Set(),
    calls = {
      resize: [],
      submit: [],
      submitArgs: [],
      snapshot: 0,
      mic: 0,
      mode: [],
      preview: [],
      confirm: [],
      revert: [],
      dismiss: 0,
      close: 0,
    };
  const pending = {};
  const lifecycle = { enterEpoch: 0, closing: false };
  globalThis.window = {
    location: { search: '?window=qa&demo=1' },
    addEventListener: (name, fn) => {
      if (name === 'keydown') keys.add(fn);
    },
    removeEventListener: (name, fn) => {
      if (name === 'keydown') keys.delete(fn);
    },
  };
  const imports = Object.fromEntries(names.map((n) => [n, n]));
  Object.assign(imports, {
    useState: (i) => active.state(i),
    useRef: (i) => active.ref(i),
    useEffect: (f, d) => active.effect(f, d),
    useTranslation: () => ({ t: (k) => k }),
    useGithubLogin: () => '',
    useChatPanelLifecycle: () => lifecycle,
    isTauri: native,
    ...helpers,
    qaWindowSetExpanded: async (next) => {
      calls.resize.push(next);
      return pending.resize?.(next);
    },
    qaSubmitText: async (text, session) => {
      calls.submit.push(text);
      calls.submitArgs.push([text, session]);
      return pending.submit?.promise;
    },
    qaGetSnapshot: async () => {
      calls.snapshot++;
      return pending.snapshot?.() ?? { kind: 'idle', messages: [] };
    },
    qaToggleRecording: async () => {
      calls.mic++;
      return pending.mic?.promise;
    },
    qaSetEditInstructionMode: async (mode) => {
      calls.mode.push(mode);
      return pending.mode?.promise;
    },
    qaWindowDismiss: async () => {
      calls.dismiss++;
      return pending.dismiss?.promise;
    },
    getSelectionVoicePreview: async (session) => {
      calls.preview.push(session);
      return pending.preview?.promise ?? { text: ' fixture edit ' };
    },
    confirmSelectionVoicePreview: async (...args) => {
      calls.confirm.push(args);
      return pending.confirm?.promise;
    },
    revertSelectionVoicePreview: async (session) => {
      calls.revert.push(session);
      return pending.revert?.promise;
    },
    chatPanelFocusKeyboard: async () => {},
  });
  const api = factory(
    ...names.map((n) => imports[n]),
    async (name, fn) => {
      const listeners = events.get(name) ?? new Set();
      listeners.add(fn);
      events.set(name, listeners);
      return () => listeners.delete(fn);
    },
    {},
    () => ({ jsx, jsxs: jsx, Fragment: 'fragment' }),
  );
  const hooks = new Hooks(),
    render = () =>
      hooks.render(() => api.QaPanel({ embedded, onRequestClose: () => calls.close++ }));
  const emit = (name, payload) => {
    for (const fn of events.get(name) ?? []) fn({ payload });
  };
  const composer = (tree) => find(tree, (n) => n.type === api.Composer).props;
  return { api, hooks, render, emit, calls, pending, lifecycle, events, keys, composer };
}
let passed = 0,
  failed = 0;
async function test(name, fn) {
  try {
    await fn();
    console.log('PASS ' + name);
    passed++;
  } catch (error) {
    console.log('FAIL ' + name + '\n' + error.stack);
    failed++;
  }
}
await test('browser compact has no demo conversations and cannot execute native actions', async () => {
  const c = context(false);
  let tree = c.render();
  assert(!tree.props.className.includes('is-expanded'));
  assert(!nodes(tree).some((n) => n.type === c.api.MessageRow));
  let p = c.composer(tree);
  p.onChange('fixture');
  p = c.composer(c.render());
  await p.onSubmit();
  await p.onToggleRecording();
  await p.onEditInstructionModeChange(true);
  await tick();
  assert.deepEqual(c.calls.resize, []);
  assert.deepEqual(c.calls.submit, []);
  assert.equal(c.calls.mic, 0);
  assert.deepEqual(c.calls.mode, []);
  const h = new Hooks(),
    ui = h.render(() => c.api.Composer(p));
  for (const button of nodes(ui).filter((n) => n.type === 'button'))
    assert.equal(button.props.disabled, true);
  c.hooks.unmount();
});
await test('native text waits for expansion and excludes repeated clicks; keeps a newer draft', async () => {
  const c = context();
  c.render();
  await tick();
  assert.deepEqual(c.calls.resize, [false]);
  c.composer(c.render()).onChange('question');
  let p = c.composer(c.render());
  const resize = deferred(),
    send = deferred();
  c.pending.resize = (next) => (next ? resize.promise : undefined);
  c.pending.submit = send;
  const first = p.onSubmit();
  void p.onSubmit();
  c.render();
  await tick();
  assert.deepEqual(c.calls.resize, [false, true]);
  assert.deepEqual(c.calls.submit, []);
  resize.resolve();
  await tick();
  assert.deepEqual(c.calls.submit, ['question']);
  c.composer(c.render()).onChange('next draft');
  send.resolve();
  await first;
  assert.equal(c.composer(c.render()).value, 'next draft');
  c.hooks.unmount();
});
await test('send failure preserves draft and exposes error without a fake user message', async () => {
  const c = context();
  c.render();
  await tick();
  c.composer(c.render()).onChange('keep this');
  const d = deferred();
  c.pending.submit = d;
  const result = c.composer(c.render()).onSubmit();
  await tick();
  d.reject(new Error('fixture'));
  await result;
  const tree = c.render();
  assert.equal(c.composer(tree).value, 'keep this');
  assert.equal(c.composer(tree).status, 'error');
  assert(!nodes(tree).some((n) => n.type === c.api.MessageRow));
  c.hooks.unmount();
});
await test('cancel during resize prevents queued question and late layout failures do not overwrite reopened compact state', async () => {
  const c = context();
  c.render();
  await tick();
  c.composer(c.render()).onChange('do not send');
  const d = deferred();
  c.pending.resize = (next) => (next ? d.promise : undefined);
  const submission = c.composer(c.render()).onSubmit();
  c.render();
  await tick();
  await c.composer(c.render()).onClose();
  c.lifecycle.closing = true;
  c.render();
  c.lifecycle.closing = false;
  c.lifecycle.enterEpoch++;
  c.render();
  assert.deepEqual(c.calls.resize, [false, true]);
  d.reject(new Error('old geometry'));
  await submission;
  await tick();
  c.render();
  await tick();
  assert.deepEqual(c.calls.submit, []);
  assert.deepEqual(c.calls.resize, [false, true, false]);
  assert.equal(c.composer(c.render()).status, 'idle');
  assert(!nodes(c.render()).some((n) => n.props.className === 'qa-layout-error'));
  c.hooks.unmount();
});
await test('recording level requires active session; no fake recording before backend phase', async () => {
  const c = context();
  c.render();
  await tick();
  const d = deferred();
  c.pending.mic = d;
  let p = c.composer(c.render());
  const first = p.onToggleRecording();
  void p.onToggleRecording();
  assert.equal(c.calls.mic, 1);
  p = c.composer(c.render());
  assert.equal(p.status, 'idle');
  assert.equal(p.micBusy, true);
  assert.equal(p.level, 0);
  c.emit('qa:state', {
    kind: 'recording',
    sessionId: 'a',
    selectionPreview: 'selected',
    messages: [],
  });
  c.emit('qa:level', { sessionId: 'other', level: 0.9 });
  assert.equal(c.composer(c.render()).level, 0);
  c.emit('qa:level', { sessionId: 'a', level: 0.6 });
  p = c.composer(c.render());
  assert.equal(p.level, 0.6);
  assert.equal(p.status, 'recording');
  assert.equal(p.voiceActive, true);
  assert.equal(p.selectionPreview, 'selected');
  d.resolve();
  await first;
  c.emit('qa:state', {
    kind: 'thinking',
    sessionId: 'a',
    messages: [{ role: 'user', content: 'actual question' }],
  });
  c.emit('qa:level', { sessionId: 'a', level: 0.9 });
  p = c.composer(c.render());
  assert.equal(p.level, 0);
  assert.equal(p.voiceActive, false);
  c.hooks.unmount();
});
await test('stale answer is ignored and final answer replaces streaming buffer exactly once', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', {
    kind: 'thinking',
    sessionId: 'new',
    messages: [{ role: 'user', content: 'question' }],
  });
  c.emit('qa:state', { kind: 'answer_delta', sessionId: 'old', chunk: 'stale' });
  c.emit('qa:state', { kind: 'answer_delta', sessionId: 'new', chunk: 'real ' });
  c.emit('qa:state', { kind: 'answer_delta', sessionId: 'new', chunk: 'answer' });
  let tree = c.render();
  assert.equal(find(tree, (n) => n.type === 'AssistantMarkdown').props.markdown, 'real answer');
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'new',
    messages: [
      { role: 'user', content: 'question' },
      { role: 'assistant', content: 'real answer' },
    ],
  });
  tree = c.render();
  assert(!nodes(tree).some((n) => n.type === 'AssistantMarkdown'));
  assert.equal(nodes(tree).filter((n) => n.type === c.api.MessageRow).length, 2);
  c.hooks.unmount();
});
await test('edit preview captured in old session cannot be applied after a new recording starts', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'a',
    messages: [{ role: 'assistant', content: 'preview' }],
    editApplyAvailable: true,
    editRevertAvailable: true,
  });
  let tree = c.render();
  const d = deferred();
  c.pending.preview = d;
  find(tree, (n) => n.type === 'button' && n.props.className === 'qa-apply').props.onClick();
  assert.deepEqual(c.calls.preview, ['a']);
  c.emit('qa:state', { kind: 'recording', sessionId: 'b', messages: [] });
  d.resolve({ text: 'must not apply' });
  await tick();
  assert.deepEqual(c.calls.confirm, []);
  assert.equal(c.composer(c.render()).status, 'recording');
  c.hooks.unmount();
});
await test('an old rendered edit button cannot approve a newer answer before React presents it', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'a',
    messages: [{ role: 'assistant', content: 'old preview' }],
    editApplyAvailable: true,
  });
  const oldTree = c.render();
  c.emit('qa:state', { kind: 'thinking', sessionId: 'b', messages: [] });
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'b',
    messages: [{ role: 'assistant', content: 'new unreviewed preview' }],
    editApplyAvailable: true,
  });
  find(oldTree, (n) => n.type === 'button' && n.props.className === 'qa-apply').props.onClick();
  await tick();
  assert.deepEqual(c.calls.preview, []);
  assert.deepEqual(c.calls.confirm, []);
  c.hooks.unmount();
});
await test('edit apply/revert are single-flight and send exact text plus captured session', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'a',
    messages: [{ role: 'assistant', content: 'preview' }],
    editApplyAvailable: true,
    editRevertAvailable: true,
  });
  let tree = c.render();
  const d = deferred();
  c.pending.confirm = d;
  const button = find(tree, (n) => n.type === 'button' && n.props.className === 'qa-apply');
  button.props.onClick();
  button.props.onClick();
  await tick();
  assert.deepEqual(c.calls.confirm, [['fixture edit', 'a']]);
  d.resolve();
  await tick();
  assert(!nodes(c.render()).some((n) => n.props.className === 'qa-edit-actions'));
  c.emit('qa:state', {
    kind: 'answer',
    sessionId: 'a',
    messages: [{ role: 'assistant', content: 'preview' }],
    editApplyAvailable: true,
    editRevertAvailable: true,
  });
  tree = c.render();
  const r = deferred();
  c.pending.revert = r;
  const back = find(
    tree,
    (n) => n.type === 'button' && n.props.children === 'qa.editRevertPrevious',
  );
  back.props.onClick();
  back.props.onClick();
  assert.deepEqual(c.calls.revert, ['a']);
  r.resolve();
  await tick();
  tree = c.render();
  assert(nodes(tree).some((n) => n.props.className === 'qa-apply'));
  assert(!nodes(tree).some((n) => n.props.children === 'qa.editRevertPrevious'));
  c.hooks.unmount();
});
await test('mode update waits for backend and repeated click cannot flip twice', async () => {
  const c = context();
  c.render();
  await tick();
  const d = deferred();
  c.pending.mode = d;
  let p = c.composer(c.render());
  const first = p.onEditInstructionModeChange(true);
  void p.onEditInstructionModeChange(false);
  p = c.composer(c.render());
  assert.equal(p.editInstructionMode, false);
  assert.deepEqual(c.calls.mode, [true]);
  d.reject(new Error('fixture'));
  await first;
  p = c.composer(c.render());
  assert.equal(p.editInstructionMode, false);
  assert.equal(p.status, 'error');
  c.hooks.unmount();
});
await test('embedded skip native resize, preserve existing parent close path and event cleanup', async () => {
  const c = context(true, true);
  c.render();
  await tick();
  c.composer(c.render()).onChange('embedded');
  await c.composer(c.render()).onSubmit();
  c.render();
  await tick();
  assert.deepEqual(c.calls.resize, []);
  c.emit('qa:dismiss', {});
  assert.equal(c.calls.close, 1);
  c.hooks.unmount();
  assert.equal(
    [...c.events.values()].reduce((n, set) => n + set.size, 0),
    0,
  );
  assert.equal(c.keys.size, 0);
});
await test('composer IME enter, WebKit 229 and form submission guards; genuine enter sends', async () => {
  const c = context();
  const h = new Hooks();
  let sent = 0;
  const p = {
    value: '中文',
    status: 'idle',
    level: 0,
    voiceActive: false,
    selectionPreview: '',
    busy: false,
    micBusy: false,
    embedded: false,
    editInstructionMode: false,
    onEditInstructionModeChange() {},
    onChange() {},
    onSubmit() {
      sent++;
    },
    onToggleRecording() {},
    onClose() {},
    t: (k) => k,
  };
  const tree = h.render(() => c.api.Composer(p));
  const input = find(tree, (n) => n.type === 'input');
  const key = (extra = {}) => ({
    key: 'Enter',
    keyCode: 13,
    nativeEvent: { isComposing: false },
    preventDefault() {},
    ...extra,
  });
  input.props.onCompositionStart();
  input.props.onKeyDown(key());
  find(tree, (n) => n.type === 'form').props.onSubmit({ preventDefault() {} });
  input.props.onCompositionEnd();
  input.props.onKeyDown(key({ nativeEvent: { isComposing: true } }));
  input.props.onKeyDown(key({ keyCode: 229 }));
  assert.equal(sent, 0);
  input.props.onKeyDown(key());
  assert.equal(sent, 1);
});

await test('session drift during delayed native resize cancels old draft', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', { kind: 'idle', sessionId: 'a', messages: [] });
  c.composer(c.render()).onChange('belongs to a');
  const d = deferred();
  c.pending.resize = (next) => (next ? d.promise : undefined);
  const result = c.composer(c.render()).onSubmit();
  await tick();
  c.emit('qa:state', { kind: 'idle', sessionId: 'b', messages: [] });
  d.resolve();
  await result;
  await tick();
  assert.deepEqual(c.calls.submit, []);
  c.hooks.unmount();
});
await test('text always carries the exact session captured before resize', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', { kind: 'idle', sessionId: 'a', messages: [] });
  c.composer(c.render()).onChange('question a');
  await c.composer(c.render()).onSubmit();
  assert.deepEqual(c.calls.submitArgs, [['question a', 'a']]);
  c.hooks.unmount();
});
for (const embedded of [false, true])
  await test(`cold ${embedded ? 'embedded' : 'desktop'} snapshot recovers initial recording and accepts its levels`, async () => {
    const c = context(true, embedded);
    c.pending.snapshot = () => ({
      kind: 'recording',
      sessionId: 'cold',
      messages: [],
      selectionPreview: 'captured text',
    });
    c.render();
    await tick();
    c.emit('qa:level', { sessionId: 'cold', level: 0.4 });
    const p = c.composer(c.render());
    assert.equal(p.status, 'recording');
    assert.equal(p.level, 0.4);
    assert.equal(p.selectionPreview, 'captured text');
    assert.equal(c.calls.snapshot, 1);
    c.hooks.unmount();
  });
await test('snapshot reply overtaken by a live session event is discarded and reread', async () => {
  const c = context();
  const first = deferred();
  c.pending.snapshot = () =>
    c.calls.snapshot === 1
      ? first.promise
      : { kind: 'recording', sessionId: 'new', messages: [], selectionPreview: 'new' };
  c.render();
  await tick();
  c.emit('qa:state', {
    kind: 'recording',
    sessionId: 'new',
    messages: [],
    selectionPreview: 'new',
  });
  first.resolve({ kind: 'recording', sessionId: 'old', messages: [], selectionPreview: 'old' });
  await tick();
  c.emit('qa:level', { sessionId: 'new', level: 0.7 });
  const p = c.composer(c.render());
  assert.equal(p.selectionPreview, 'new');
  assert.equal(p.level, 0.7);
  assert.equal(c.calls.snapshot, 2);
  c.hooks.unmount();
});
await test('snapshot completion after dismiss cannot reopen old recording state', async () => {
  const c = context();
  const first = deferred();
  c.pending.snapshot = () => first.promise;
  c.render();
  await tick();
  c.emit('qa:dismiss', {});
  c.lifecycle.closing = true;
  c.render();
  first.resolve({ kind: 'recording', sessionId: 'old', messages: [], selectionPreview: 'old' });
  await tick();
  assert.equal(c.composer(c.render()).status, 'idle');
  c.hooks.unmount();
});
await test('a snapshot read failure does not silently remove all established live subscriptions', async () => {
  const c = context();
  c.pending.snapshot = () => Promise.reject(new Error('transient snapshot unavailable'));
  c.render();
  await tick();
  c.emit('qa:state', { kind: 'recording', sessionId: 'live', messages: [] });
  c.emit('qa:level', { sessionId: 'live', level: 0.4 });
  const p = c.composer(c.render());
  assert.equal(p.status, 'recording');
  assert.equal(p.level, 0.4);
  c.hooks.unmount();
});
await test('old microphone RPC failure cannot overwrite a newer native recording session', async () => {
  const c = context();
  c.render();
  await tick();
  c.emit('qa:state', { kind: 'idle', sessionId: 'before', messages: [] });
  const old = deferred();
  c.pending.mic = old;
  const request = c.composer(c.render()).onToggleRecording();
  c.emit('qa:state', { kind: 'recording', sessionId: 'a', messages: [] });
  c.emit('qa:state', {
    kind: 'error',
    sessionId: 'a',
    messages: [],
    error: 'old recording failed',
  });
  c.emit('qa:state', { kind: 'recording', sessionId: 'b', messages: [] });
  old.reject(new Error('old RPC completed late'));
  await request;
  const p = c.composer(c.render());
  assert.equal(p.status, 'recording');
  c.emit('qa:level', { sessionId: 'b', level: 0.8 });
  assert.equal(c.composer(c.render()).level, 0.8);
  c.hooks.unmount();
});
console.log(JSON.stringify({ passed, failed }));
process.exitCode = failed ? 1 : 0;
