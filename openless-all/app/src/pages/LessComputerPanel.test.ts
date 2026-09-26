// @ts-nocheck — Node-only handler replay; production UI remains strictly typed.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const app = process.argv[2] || process.cwd();
const require = createRequire(resolve(app, 'package.json'));
const ts = require('typescript');
const replayModule = await import(
  new URL('file://' + resolve(app, 'src/lib/lessComputerReplay.ts'))
);
const activityModule = await import(
  new URL('file://' + resolve(app, 'src/lib/lessComputerToolActivity.ts'))
);
const composerModule = await import(
  new URL('file://' + resolve(app, 'src/lib/lessComputerComposer.ts'))
);
const source = readFileSync(resolve(app, 'src/pages/LessComputerPanel.tsx'), 'utf8');
const parsed = ts.createSourceFile(
  'LessComputerPanel.tsx',
  source,
  ts.ScriptTarget.ES2020,
  true,
  ts.ScriptKind.TSX,
);
const names = parsed.statements
  .filter(ts.isImportDeclaration)
  .flatMap((node) =>
    node.importClause?.namedBindings && ts.isNamedImports(node.importClause.namedBindings)
      ? node.importClause.namedBindings.elements.map((item) => item.name.text)
      : [],
  );
const body = parsed.statements
  .filter((node) => !ts.isImportDeclaration(node))
  .map((node) => node.getFullText(parsed))
  .join('\n')
  .replaceAll("import('@tauri-apps/api/event')", 'Promise.resolve({listen: __listen})')
  .replaceAll(
    "import('@tauri-apps/api/window')",
    'Promise.resolve({getCurrentWindow: __getWindow})',
  );
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
  '__getWindow',
  'exports',
  'require',
  `${compiled};return {LessComputerPanel,Composer,ApprovalCard,TurnView,ToolProcess,voiceTime};`,
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
function nodes(value) {
  if (Array.isArray(value)) return value.flatMap(nodes);
  if (!value || typeof value !== 'object' || !value.props) return [];
  return [value, ...nodes(value.props.children)];
}
function find(tree, predicate) {
  const result = nodes(tree).find(predicate);
  assert(result, 'control exists');
  return result;
}
let active;
class Hooks {
  slots = [];
  cursor = 0;
  pending = [];
  state(initial) {
    const index = this.cursor++;
    this.slots[index] ??= { value: typeof initial === 'function' ? initial() : initial };
    return [
      this.slots[index].value,
      (value) => {
        this.slots[index].value =
          typeof value === 'function' ? value(this.slots[index].value) : value;
      },
    ];
  }
  ref(initial) {
    return this.state({ current: initial })[0];
  }
  effect(fn, deps) {
    const index = this.cursor++;
    const prev = this.slots[index];
    if (
      prev &&
      deps &&
      prev.deps?.length === deps.length &&
      deps.every((v, i) => Object.is(v, prev.deps[i]))
    )
      return;
    const slot = { deps };
    this.slots[index] = slot;
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
    this.slots.forEach((slot) => slot.cleanup?.());
  }
}
function context(native, replay) {
  const events = new Map();
  const keys = new Set();
  const calls = {
    approve: [],
    submit: [],
    windows: [],
    voiceStart: [],
    voiceStop: [],
    voiceCancel: [],
    taskCancel: 0,
    focus: 0,
  };
  let approvalResult, submitResult, voiceStartResult;
  globalThis.window = {
    location: { search: '?window=less-computer&demo=1' },
    addEventListener: (name, fn) => {
      if (name === 'keydown') keys.add(fn);
    },
    removeEventListener: (name, fn) => {
      if (name === 'keydown') keys.delete(fn);
    },
  };
  const imports = Object.fromEntries(names.map((name) => [name, name]));
  Object.assign(imports, {
    useState: (initial) => active.state(initial),
    useRef: (initial) => active.ref(initial),
    useEffect: (fn, deps) => active.effect(fn, deps),
    useTranslation: () => ({ t: (key) => key }),
    useChatPanelLifecycle: () => ({ enterEpoch: 0, closing: false }),
    useExitMount: (open) => ({ mounted: open, closing: false }),
    applyThemeFromPreference: () => {},
    formatComboLabel: (binding) => binding.primary,
    isTauri: native,
    getSettings: async () => ({ codingAgentProvider: 'codex-cli' }),
    marketplaceAuthStatus: async () => ({ signedIn: false }),
    lessComputerSync: () =>
      replay?.promise ?? Promise.resolve({ events: [], latestSequence: 0, truncated: false }),
    lessComputerApprove: async (...args) => {
      calls.approve.push(args);
      return approvalResult?.promise;
    },
    lessComputerSubmitText: async (text) => {
      calls.submit.push(text);
      return submitResult?.promise;
    },
    lessComputerWindowDismiss: async () => {
      calls.windows.push('hide');
    },
    lessComputerVoiceStart: async (mode) => {
      calls.voiceStart.push(mode);
      return voiceStartResult?.promise;
    },
    lessComputerVoiceStop: async (sessionId) => {
      calls.voiceStop.push(sessionId);
    },
    lessComputerVoiceCancel: async (sessionId) => {
      calls.voiceCancel.push(sessionId);
    },
    lessComputerTaskCancel: async () => {
      calls.taskCancel += 1;
    },
    chatPanelFocusKeyboard: async () => {
      calls.focus += 1;
    },
    ...replayModule,
    ...activityModule,
    ...composerModule,
  });
  const api = factory(
    ...names.map((n) => imports[n]),
    async (name, fn) => {
      const set = events.get(name) ?? new Set();
      set.add(fn);
      events.set(name, set);
      return () => set.delete(fn);
    },
    () => ({
      minimize: async () => calls.windows.push('minimize'),
      toggleMaximize: async () => calls.windows.push('maximize'),
    }),
    {},
    () => ({ jsx, jsxs: jsx, Fragment: 'fragment' }),
  );
  const hooks = new Hooks();
  const render = () => hooks.render(api.LessComputerPanel);
  const emit = (ev) => {
    for (const fn of events.get('less-computer:event') ?? []) fn({ payload: ev });
  };
  return {
    api,
    hooks,
    render,
    emit,
    calls,
    events,
    keys,
    imports,
    approval: (d) => {
      approvalResult = d;
    },
    submission: (d) => {
      submitResult = d;
    },
    voiceStartResult: (d) => {
      voiceStartResult = d;
    },
    turns: (tree) =>
      nodes(tree)
        .filter((node) => node.type === api.TurnView)
        .map((node) => node.props),
    voice: (tree) => find(tree, (node) => node.type === api.Composer).props.voice,
  };
}
let passed = 0;
async function test(name, fn) {
  await fn();
  console.log(`PASS ${name}`);
  passed++;
}
await test('browser has no demo state and cannot submit, approve, open OAuth or control native windows', async () => {
  const c = context(false);
  let tree = c.render();
  assert.equal(c.turns(tree).length, 0);
  const windowButtons = nodes(tree).filter(
    (n) => n.type === 'button' && n.props.className?.startsWith('lc-window-'),
  );
  for (const button of windowButtons) {
    assert.equal(button.props.disabled, true);
    await button.props.onClick();
  }
  assert.deepEqual(c.calls.windows, []);
  const hooks = new Hooks();
  let composer = hooks.render(() => c.api.Composer({ working: false, voice: null, t: (k) => k }));
  find(composer, (n) => n.type === 'textarea').props.onChange({
    currentTarget: { value: 'never execute' },
  });
  composer = hooks.render(() => c.api.Composer({ working: false, voice: null, t: (k) => k }));
  find(composer, (n) => n.type === 'form').props.onSubmit({ preventDefault() {} });
  await tick();
  assert.deepEqual(c.calls.submit, []);
  for (const button of nodes(composer).filter(
    (n) => n.type === 'button' && n.props.className?.startsWith('lc-round'),
  )) {
    assert.equal(button.props.disabled, true, 'browser previews cannot open the microphone');
    await button.props.onClick?.();
  }
  assert.deepEqual(c.calls.voiceStart, []);
  find(tree, (n) => n.type === 'button' && n.props.className === 'lc-github').props.onClick();
  tree = c.render();
  assert(!nodes(tree).some((n) => n.type === 'GithubLoginModal'));
  c.hooks.unmount();
  assert.equal(c.keys.size, 0);
});
await test('replay/live dedup, ordered deltas and fresh session reset', async () => {
  const replay = deferred(),
    c = context(true, replay);
  c.render();
  await tick();
  c.emit({ kind: 'tool', name: 'Read', seq: 2 });
  c.emit({ kind: 'delta', text: 'A', seq: 3 });
  replay.resolve({
    events: [
      { kind: 'user', text: 'task', fresh: true, seq: 1 },
      { kind: 'tool', name: 'Read', seq: 2 },
    ],
    latestSequence: 2,
    truncated: false,
  });
  await tick();
  let turns = c.turns(c.render());
  assert.equal(turns.length, 1);
  assert.equal(turns[0].turn.user, 'task');
  assert.deepEqual(turns[0].turn.segments, [
    { kind: 'tool', name: 'Read', running: false },
    { kind: 'text', content: 'A' },
  ]);
  c.emit({ kind: 'delta', text: 'duplicate', seq: 3 });
  c.emit({ kind: 'delta', text: 'B', seq: 4 });
  turns = c.turns(c.render());
  assert.equal(turns[0].turn.segments[1].content, 'AB');
  c.emit({ kind: 'user', text: 'new', fresh: true, seq: 5 });
  turns = c.turns(c.render());
  assert.equal(turns.length, 1);
  assert.equal(turns[0].turn.user, 'new');
  c.hooks.unmount();
  assert.equal(
    [...c.events.values()].reduce((a, s) => a + s.size, 0),
    0,
  );
});
await test('cold missing-user stream self-heals; completed text fallback preserves tools and real cost', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'tool', name: 'Read', seq: 1 });
  c.emit({ kind: 'completed', text: 'result', costUsd: 0.005, seq: 2 });
  const turn = c.turns(c.render())[0].turn;
  assert.equal(turn.user, '');
  assert.deepEqual(turn.segments, [
    { kind: 'tool', name: 'Read', running: false },
    { kind: 'text', content: 'result' },
  ]);
  assert.equal(turn.costUsd, 0.005);
  assert.equal(turn.status, 'done');
  c.hooks.unmount();
});
await test('approval waits for IPC, rejects duplicate clicks, retains retry on failure, confirms only success', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'user', text: 'task', fresh: true, seq: 1 });
  c.emit({ kind: 'approval', token: 'one', command: 'fixture', reason: 'fixture', seq: 2 });
  let turn = c.turns(c.render())[0];
  let d = deferred();
  c.approval(d);
  const first = turn.onApproval('one', true);
  void turn.onApproval('one', false);
  turn = c.turns(c.render())[0];
  assert.equal(c.calls.approve.length, 1);
  assert.equal(turn.turn.segments[0].pending, true);
  assert.equal(turn.turn.segments[0].decision, undefined);
  d.reject(new Error('fixture rejection'));
  await first;
  turn = c.turns(c.render())[0];
  assert.equal(turn.turn.segments[0].pending, false);
  assert.equal(turn.turn.segments[0].failed, true);
  assert.equal(turn.turn.segments[0].decision, undefined);
  d = deferred();
  c.approval(d);
  const second = turn.onApproval('one', true);
  d.resolve();
  await second;
  turn = c.turns(c.render())[0];
  assert.equal(turn.turn.segments[0].decision, 'approved');
  assert.equal(turn.turn.segments[0].failed, false);
  c.hooks.unmount();
});
await test('late approval from old turn cannot mutate a fresh session or remove its pending request', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'user', text: 'old', fresh: true, seq: 1 });
  c.emit({ kind: 'approval', token: 'same', command: 'old', reason: 'x', seq: 2 });
  let d = deferred();
  c.approval(d);
  const old = c.turns(c.render())[0].onApproval('same', true);
  c.emit({ kind: 'user', text: 'new', fresh: true, seq: 3 });
  c.emit({ kind: 'approval', token: 'same', command: 'new', reason: 'x', seq: 4 });
  const newer = deferred();
  c.approval(newer);
  let turn = c.turns(c.render())[0];
  const current = turn.onApproval('same', false);
  d.resolve();
  await old;
  turn = c.turns(c.render())[0];
  assert.equal(turn.turn.segments[0].decision, undefined);
  assert.equal(turn.turn.segments[0].pending, true);
  void turn.onApproval('same', true);
  assert.equal(c.calls.approve.length, 2);
  newer.resolve();
  await current;
  turn = c.turns(c.render())[0];
  assert.equal(turn.turn.segments[0].decision, 'denied');
  c.hooks.unmount();
});
await test('terminal cancellation blocks approvals and removes tool running indicator', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'user', text: 'task', fresh: true, seq: 1 });
  c.emit({ kind: 'tool', name: 'Read', seq: 2 });
  c.emit({ kind: 'approval', token: 'old', command: 'fixture', reason: 'x', seq: 3 });
  c.emit({ kind: 'cancelled', seq: 4 });
  let turn = c.turns(c.render())[0];
  await turn.onApproval('old', true);
  assert.equal(c.calls.approve.length, 0);
  assert.equal(turn.turn.segments[0].running, false);
  assert.equal(turn.turn.status, 'cancelled');
  c.hooks.unmount();
});
await test('voice projection preserves session ownership and exposes only actual level/phase/elapsed', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({
    kind: 'voice_state',
    sessionId: 'a',
    phase: 'recording',
    level: 0.5,
    elapsedMs: 1000,
    seq: 1,
  });
  c.emit({
    kind: 'voice_state',
    sessionId: 'b',
    phase: 'starting',
    level: 0,
    elapsedMs: 0,
    seq: 2,
  });
  c.emit({ kind: 'voice_state', sessionId: 'a', phase: 'idle', level: 0, elapsedMs: 5000, seq: 3 });
  assert.equal(c.voice(c.render()).sessionId, 'b');
  c.emit({
    kind: 'voice_state',
    sessionId: 'b',
    phase: 'recording',
    level: 0.7,
    elapsedMs: 65000,
    seq: 4,
  });
  const props = find(c.render(), (n) => n.type === c.api.Composer).props;
  const hooks = new Hooks();
  const tree = hooks.render(() => c.api.Composer(props));
  const waveform = find(tree, (n) => n.type === 'LiveWaveform');
  assert.equal(waveform.props.level, 0.7);
  assert.equal(waveform.props.processing, false);
  assert.equal(find(tree, (n) => n.type === 'time').props.children, '1:05');
  assert(find(tree, (n) => n.type === 'textarea').props.disabled);
  assert.equal(c.api.voiceTime(NaN), '0:00');
  assert.equal(c.api.voiceTime(-8), '0:00');
  c.hooks.unmount();
});
await test('composer IME/229/Shift+Enter guards; one pending send; RPC failure retains draft', async () => {
  const c = context(true);
  const h = new Hooks();
  const render = () => h.render(() => c.api.Composer({ working: false, voice: null, t: (k) => k }));
  let tree = render();
  find(tree, (n) => n.type === 'textarea').props.onChange({ currentTarget: { value: '你好' } });
  tree = render();
  let input = find(tree, (n) => n.type === 'textarea');
  const key = (overrides = {}) => ({
    key: 'Enter',
    shiftKey: false,
    keyCode: 13,
    nativeEvent: { isComposing: false },
    preventDefault() {
      this.prevented = true;
    },
    ...overrides,
  });
  input.props.onCompositionStart();
  input.props.onKeyDown(key());
  find(tree, (n) => n.type === 'form').props.onSubmit({ preventDefault() {} });
  input.props.onCompositionEnd();
  input.props.onKeyDown(key({ nativeEvent: { isComposing: true } }));
  input.props.onKeyDown(key({ keyCode: 229 }));
  let shift = key({ shiftKey: true });
  input.props.onKeyDown(shift);
  assert.equal(shift.prevented, undefined);
  assert.equal(c.calls.submit.length, 0);
  let d = deferred();
  c.submission(d);
  input.props.onKeyDown(key());
  input.props.onKeyDown(key());
  assert.equal(c.calls.submit.length, 1);
  tree = render();
  assert(find(tree, (n) => n.type === 'button' && n.props.type === 'submit').props.disabled);
  d.reject(new Error('fixture'));
  await tick();
  tree = render();
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, '你好');
  assert(nodes(tree).some((n) => n.props.role === 'alert'));
  d = deferred();
  c.submission(d);
  find(tree, (n) => n.type === 'textarea').props.onKeyDown(key());
  tree = render();
  find(tree, (n) => n.type === 'textarea').props.onChange({
    currentTarget: { value: 'next draft' },
  });
  d.resolve();
  await tick();
  tree = render();
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, 'next draft');
});
await test('native window actions use hide/minimize/toggleMaximize; Escape closes OAuth without hiding panel', async () => {
  const c = context(true);
  let tree = c.render();
  await tick();
  for (const button of nodes(tree).filter(
    (n) => n.type === 'button' && n.props.className?.startsWith('lc-window-'),
  ))
    await button.props.onClick();
  await tick();
  assert.deepEqual(c.calls.windows, ['hide', 'minimize', 'maximize']);
  find(tree, (n) => n.type === 'button' && n.props.className === 'lc-github').props.onClick();
  tree = c.render();
  assert(nodes(tree).some((n) => n.type === 'GithubLoginModal'));
  let prevented = false,
    stopped = false;
  for (const key of c.keys)
    key({
      key: 'Escape',
      isComposing: false,
      keyCode: 27,
      preventDefault() {
        prevented = true;
      },
      stopPropagation() {
        stopped = true;
      },
    });
  tree = c.render();
  assert.equal(prevented, true);
  assert.equal(stopped, true);
  assert(!nodes(tree).some((n) => n.type === 'GithubLoginModal'));
  assert.equal(c.calls.windows.length, 3);
  for (const key of c.keys)
    key({
      key: 'Escape',
      isComposing: true,
      keyCode: 229,
      preventDefault() {
        throw new Error('IME Escape must be left alone');
      },
    });
  assert.equal(c.calls.windows.length, 3);
  c.hooks.unmount();
});
await test('tool activity is collapsed, grouped and loses shimmer on every terminal state', async () => {
  const c = context(false);
  const tools = [
    { kind: 'tool', name: 'Read', running: false },
    { kind: 'tool', name: 'Bash', running: false },
    { kind: 'tool', name: 'Bash', running: true },
  ];
  const hooks = new Hooks();
  const renderActive = () =>
    hooks.render(() =>
      c.api.ToolProcess({ tools, working: true, interrupted: false, t: (k) => k }),
    );
  let active = renderActive();
  const summary = () => find(active, (n) => n.props.className === 'lc-tool-summary');
  assert.equal(summary().props['aria-expanded'], false, 'tool activity starts collapsed');
  assert.equal(
    find(active, (n) => n.props.className === 'lc-tool-body').props['aria-hidden'],
    true,
  );
  summary().props.onClick();
  active = renderActive();
  assert.equal(summary().props['aria-expanded'], true);
  assert(active.props.className.includes('is-open'), 'expansion animates via the open class');
  assert.equal(
    nodes(active).filter((n) => n.props.className?.startsWith('lc-process-step is-')).length,
    2,
  );
  assert(nodes(active).some((n) => n.props.children === 'lessComputer.activity.commandRunning'));
  assert(nodes(active).some((n) => n.props.className === 'lc-process-label is-running'));
  assert(nodes(active).some((n) => n.props.className === 'lc-process-phase is-running'));
  assert(nodes(active).some((n) => n.props.children === 'Read'));
  assert(nodes(active).some((n) => n.props.children === 'Bash'));
  for (const interrupted of [false, true]) {
    const ended = new Hooks().render(() =>
      c.api.ToolProcess({ tools, working: false, interrupted, t: (k) => k }),
    );
    assert(!nodes(ended).some((n) => n.props.className?.includes('is-running')));
    assert.equal(
      nodes(ended).some((n) => n.props.className === 'lc-process-step is-stopped'),
      interrupted,
    );
  }
});
await test('approval stays directly visible between separate folded tool blocks', async () => {
  const c = context(false);
  const tree = c.api.TurnView({
    index: 0,
    actionable: true,
    onApproval() {},
    t: (k) => k,
    turn: {
      user: 'fixture',
      status: 'working',
      errorMsg: '',
      costUsd: null,
      segments: [
        { kind: 'tool', name: 'Read', running: false },
        {
          kind: 'approval',
          token: 'real-token',
          command: 'fixture command',
          reason: 'fixture reason',
        },
        { kind: 'tool', name: 'Bash', running: true },
      ],
    },
  });
  assert.equal(nodes(tree).filter((n) => n.type === c.api.ToolProcess).length, 2);
  assert.equal(nodes(tree).filter((n) => n.type === c.api.ApprovalCard).length, 1);
});
await test('window controls lead the sidebar and the inspector only summarizes the turn', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'user', text: 'task', fresh: true, seq: 1 });
  c.emit({ kind: 'tool', name: 'Read', seq: 2 });
  c.emit({ kind: 'tool', name: 'Bash', seq: 3 });
  c.emit({ kind: 'approval', token: 'one', command: 'rm -rf build', reason: 'x', seq: 4 });
  const tree = c.render();
  const shell = find(tree, (n) => n.props.className?.startsWith('lc-desktop'));
  const sidebar = shell.props.children.find(Boolean);
  assert.equal(sidebar.type, 'aside');
  const head = sidebar.props.children.find(Boolean);
  assert.equal(head.props.className, 'lc-sidebar-head');
  assert.equal(head.props.children[0].props.className, 'lc-window-controls');
  assert(!nodes(tree).some((n) => n.type === 'AgentBuddy'));
  assert(!nodes(tree).some((n) => n.props.className === 'lc-activity'));
  const inspector = find(tree, (n) => n.type === 'LessComputerInspector');
  assert.deepEqual(
    {
      tone: inspector.props.summary.tone,
      toolCount: inspector.props.summary.toolCount,
      pendingApprovals: inspector.props.summary.pendingApprovals,
    },
    { tone: 'waiting', toolCount: 2, pendingApprovals: 1 },
  );
  const serialized = JSON.stringify(inspector.props.summary);
  assert(!serialized.includes('Read') && !serialized.includes('rm -rf'), 'no second activity feed');
  const current = find(tree, (n) => n.type === 'li' && n.props['aria-current'] === 'true');
  assert(nodes(current).some((n) => n.props.children === 'Codex'));
  c.hooks.unmount();
});
await test('finished dictation lands in the draft exactly once; silence shows a notice', async () => {
  const c = context(true);
  const committed = {
    kind: 'voice_state',
    sessionId: `dictation-${Date.now()}`,
    phase: 'idle',
    level: 0,
    elapsedMs: 900,
    mode: 'dictate',
    transcript: 'open settings',
    outcome: 'committed',
    seq: 9,
  };
  const h = new Hooks();
  const render = (voice) => h.render(() => c.api.Composer({ working: false, voice, t: (k) => k }));
  let tree = render(null);
  find(tree, (n) => n.type === 'textarea').props.onChange({ currentTarget: { value: 'please' } });
  render(committed);
  tree = render(committed);
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, 'please open settings');
  tree = render({ ...committed });
  assert.equal(
    find(tree, (n) => n.type === 'textarea').props.value,
    'please open settings',
    'a replayed idle snapshot is not inserted twice',
  );
  const remount = new Hooks();
  tree = remount.render(() => c.api.Composer({ working: false, voice: committed, t: (k) => k }));
  tree = remount.render(() => c.api.Composer({ working: false, voice: committed, t: (k) => k }));
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, '');
  const silent = {
    ...committed,
    sessionId: `${committed.sessionId}-silent`,
    transcript: '',
    outcome: 'empty',
  };
  const quiet = new Hooks();
  quiet.render(() => c.api.Composer({ working: false, voice: silent, t: (k) => k }));
  tree = quiet.render(() => c.api.Composer({ working: false, voice: silent, t: (k) => k }));
  assert(
    nodes(tree).some(
      (n) => n.props.role === 'alert' && n.props.children === 'lessComputer.voice.empty',
    ),
  );
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, '');
});
await test('composer voice controls call their own commands and report start failures inline', async () => {
  const c = context(true);
  const h = new Hooks();
  const props = { working: false, voice: null, t: (k) => k };
  const render = (overrides = {}) => h.render(() => c.api.Composer({ ...props, ...overrides }));
  let tree = render();
  const byClass = (cls) =>
    find(tree, (n) => n.type === 'button' && n.props.className?.split(' ').includes(cls));
  byClass('lc-mic').props.onClick();
  await tick();
  tree = render();
  byClass('lc-voice').props.onClick();
  await tick();
  assert.deepEqual(c.calls.voiceStart, ['dictate', 'submit']);
  const failure = deferred();
  c.voiceStartResult(failure);
  tree = render();
  byClass('lc-mic').props.onClick();
  tree = render();
  assert.equal(byClass('lc-mic').props.disabled, true, 'one start request at a time');
  byClass('lc-mic').props.onClick();
  assert.equal(c.calls.voiceStart.length, 3, 'a second click while starting is ignored');
  failure.reject(new Error('mic denied'));
  await tick();
  tree = render();
  assert(
    nodes(tree).some(
      (n) => n.props.role === 'alert' && n.props.children === 'lessComputer.voice.startFailed',
    ),
  );
  c.voiceStartResult(undefined);

  tree = render({ working: true });
  await byClass('lc-stop').props.onClick();
  assert.equal(c.calls.taskCancel, 1);

  const recording = {
    kind: 'voice_state',
    sessionId: 'live',
    phase: 'recording',
    level: 0.4,
    elapsedMs: 1200,
    mode: 'dictate',
    transcript: 'hello wor',
  };
  tree = render({ voice: recording });
  assert(
    nodes(tree).some((n) => n.props.children === 'hello wor'),
    'live transcript is visible',
  );
  const stage = find(tree, (n) => n.props.className === 'lc-voice-stage');
  const [cancel, confirm] = nodes(stage).filter((n) => n.type === 'button');
  cancel.props.onClick();
  confirm.props.onClick();
  assert.deepEqual(c.calls.voiceCancel, ['live']);
  assert.deepEqual(c.calls.voiceStop, ['live'], 'confirm stops only the displayed recording');
  tree = render({ voice: { ...recording, phase: 'transcribing' } });
  const [, finishing] = nodes(find(tree, (n) => n.props.className === 'lc-voice-stage')).filter(
    (n) => n.type === 'button',
  );
  assert.equal(finishing.props.disabled, true, 'a finishing capture cannot be stopped twice');
  tree = render({ voice: { ...recording, sessionId: 'next-recording' } });
  confirm.props.onClick();
  const nextConfirm = nodes(find(tree, (n) => n.props.className === 'lc-voice-stage')).filter(
    (n) => n.type === 'button',
  )[1];
  nextConfirm.props.onClick();
  assert.deepEqual(
    c.calls.voiceStop,
    ['live', 'live', 'next-recording'],
    'a delayed control keeps its original session ID instead of targeting the next capture',
  );
});
await test('voice startup and text submission exclude each other before React rerenders', async () => {
  const c = context(true);
  const h = new Hooks();
  const render = () => h.render(() => c.api.Composer({ working: false, voice: null, t: (k) => k }));
  let tree = render();
  find(tree, (n) => n.type === 'textarea').props.onChange({
    currentTarget: { value: 'keep this draft' },
  });
  tree = render();
  const mic = find(tree, (n) => n.props.className === 'lc-round lc-mic');
  const input = find(tree, (n) => n.type === 'textarea');
  const form = find(tree, (n) => n.type === 'form');
  const pending = deferred();
  c.voiceStartResult(pending);
  mic.props.onClick();
  input.props.onKeyDown({
    key: 'Enter',
    shiftKey: false,
    keyCode: 13,
    nativeEvent: { isComposing: false },
    preventDefault() {},
  });
  form.props.onSubmit({ preventDefault() {} });
  await tick();
  assert.deepEqual(c.calls.submit, [], 'same-render send cannot race an accepted microphone start');
  tree = render();
  assert.equal(
    find(tree, (n) => n.type === 'button' && n.props.type === 'submit').props.disabled,
    true,
  );
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, 'keep this draft');
  pending.reject(new Error('fixture microphone failed'));
  await tick();
  tree = render();
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, 'keep this draft');

  const sending = deferred();
  c.submission(sending);
  const currentMic = find(tree, (n) => n.props.className === 'lc-round lc-mic');
  find(tree, (n) => n.type === 'form').props.onSubmit({ preventDefault() {} });
  currentMic.props.onClick();
  assert.deepEqual(
    c.calls.voiceStart,
    ['dictate'],
    'pending text send also blocks same-render microphone start',
  );
  sending.resolve();
  await tick();
  h.unmount();
});
await test('dictation updates a blocked composer without stealing DOM or native focus', async () => {
  const c = context(true);
  const h = new Hooks();
  let domFocus = 0;
  const render = (voice, focusAllowed) =>
    h.render(() => c.api.Composer({ working: false, voice, focusAllowed, t: (k) => k }));
  let tree = render(null, false);
  const input = find(tree, (n) => n.type === 'textarea');
  input.props.ref.current = {
    style: {},
    scrollHeight: 36,
    value: 'fixture dictation',
    focus() {
      domFocus += 1;
    },
    setSelectionRange() {},
  };
  input.props.onFocus();
  input.props.onPointerDown();
  assert.equal(c.calls.focus, 0, 'blocked input handlers cannot request native focus either');
  const terminal = {
    kind: 'voice_state',
    sessionId: `focus-blocked-${Date.now()}`,
    phase: 'idle',
    mode: 'dictate',
    outcome: 'committed',
    transcript: 'fixture dictation',
    level: 0,
    elapsedMs: 500,
  };
  render(terminal, false);
  tree = render(terminal, false);
  assert.equal(find(tree, (n) => n.type === 'textarea').props.value, 'fixture dictation');
  assert.equal(domFocus, 0);
  assert.equal(c.calls.focus, 0);
  render(terminal, true);
  assert.equal(domFocus, 0, 'closing a dialog does not replay an old focus request');
  assert.equal(c.calls.focus, 0);
  const next = { ...terminal, sessionId: `${terminal.sessionId}-next`, transcript: 'next' };
  render(next, true);
  render(next, true);
  assert.equal(domFocus, 1, 'a subsequent foreground dictation still focuses the input');
  assert.equal(c.calls.focus, 1);
  h.unmount();
});
await test('an open login dialog disables composer focus', async () => {
  const c = context(true);
  let tree = c.render();
  assert.equal(find(tree, (n) => n.type === c.api.Composer).props.focusAllowed, true);
  find(tree, (n) => n.props.className === 'lc-github').props.onClick();
  tree = c.render();
  assert.equal(find(tree, (n) => n.type === c.api.Composer).props.focusAllowed, false);
  c.hooks.unmount();
});
await test('abandoned captures never relabel a finished turn; capture errors get their own row', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({ kind: 'user', text: 'task', fresh: true, seq: 1 });
  c.emit({ kind: 'completed', text: 'answer', costUsd: null, seq: 2 });
  c.emit({ kind: 'cancelled', seq: 3 });
  let turns = c.turns(c.render());
  assert.equal(turns.length, 1);
  assert.equal(turns[0].turn.status, 'done');
  c.emit({ kind: 'error', message: 'Less Computer voice input failed. Please try again.', seq: 4 });
  turns = c.turns(c.render());
  assert.equal(turns.length, 2);
  assert.equal(turns[0].turn.status, 'done');
  assert.equal(turns[1].turn.status, 'error');
  assert.equal(turns[1].turn.user, '');
  c.hooks.unmount();
});
await test('Escape during a recording cancels only that recording', async () => {
  const c = context(true);
  c.render();
  await tick();
  c.emit({
    kind: 'voice_state',
    sessionId: 'rec',
    phase: 'recording',
    level: 0.2,
    elapsedMs: 300,
    mode: 'submit',
    seq: 1,
  });
  c.render();
  for (const key of c.keys)
    key({
      key: 'Escape',
      isComposing: false,
      keyCode: 27,
      preventDefault() {},
      stopPropagation() {},
    });
  await tick();
  assert.deepEqual(c.calls.voiceCancel, ['rec']);
  assert.deepEqual(c.calls.windows, []);
  c.hooks.unmount();
});
console.log(`${passed} actual-component behavior tests passed`);
