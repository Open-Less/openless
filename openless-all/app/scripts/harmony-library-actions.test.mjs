// Execute real native library controllers/API against isolated, in-memory Core wire fixtures.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const root = new URL('../../harmony/entry/src/main/ets/', import.meta.url);
const source = (file) => readFileSync(new URL(file, root), 'utf8');
const clone = (value) => JSON.parse(JSON.stringify(value));
function load(text, globals = {}) {
  const exports = {};
  const clean = text.replace(/^import[\s\S]*?;\n/gm, '');
  const result = ts.transpileModule(clean, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  assert.equal(result.diagnostics?.length ?? 0, 0);
  vm.runInNewContext(result.outputText, { exports, console, ...globals });
  return exports;
}
const deferred = () => {
  let resolve;
  const promise = new Promise((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
};
const tick = () => new Promise((resolve) => setImmediate(resolve));
const { StyleEditing } = load(source('data/StyleEditing.ets'));
const original = {
  id: 'imported.fixture',
  name: 'Do not rename',
  description: 'Do not erase',
  author: 'Author',
  version: '7.8.9',
  kind: 'imported',
  baseMode: 'formal',
  prompt: 'speech original',
  selectionPrompt: 'independent written rules',
  voiceEditPrompt: 'independent EditPlan rules',
  examples: [{ title: 'Example', input: 'a', output: 'b' }],
  tags: ['retain'],
  iconPath: '/fixture/icon.png',
  createdAt: 'created',
  updatedAt: 'updated',
  enabled: false,
  active: false,
  recommendedModel: 'keep-model',
  compatibleAppVersion: 'keep-version',
  originPackId: 'remote-1',
  originAuthorLogin: 'owner',
  futureMetadata: { keep: 'unknown fields too' },
};
const edited = StyleEditing.withPrompt(original, 'new speech only');
assert.deepEqual(clone(edited), { ...original, prompt: 'new speech only' });
assert.equal(original.prompt, 'speech original', 'editor cannot mutate the loaded snapshot');
assert.throws(
  () => StyleEditing.withPrompt({ id: 'partial', name: 'x', prompt: 'x' }, 'unsafe'),
  /不完整/,
);
for (const field of ['author', 'recommendedModel', 'compatibleAppVersion']) {
  const partial = clone(original);
  delete partial[field];
  assert.throws(
    () => StyleEditing.withPrompt(partial, 'unsafe'),
    /不完整/,
    'missing nullable metadata must fail closed, not silently default in Rust: ' + field,
  );
}
const created = StyleEditing.create('  New style  ', '  literal prompt\n');
assert.equal(created.name, 'New style');
assert.equal(created.prompt, '  literal prompt\n', 'prompt whitespace is authored content');
assert.equal(created.kind, 'imported');
assert.equal(created.version, '1.0.0');
assert.equal(created.selectionPrompt, '');
assert.equal('systemPrompt' in created, false);
assert.throws(() => StyleEditing.create('  ', 'x'), /名称/);
const remote = { id: 'remote-1', authorLogin: 'owner', likeCount: 3, downloadCount: 8 };
assert.equal(StyleEditing.installed(remote, [original]), true);
assert.equal(
  StyleEditing.installed({ ...remote, authorLogin: 'different-owner' }, [original]),
  false,
);

const calls = [];
let listed = [clone(original)];
class HarmonyApi {
  async invokeAsync(command, argsJson) {
    const args = JSON.parse(argsJson);
    calls.push({ command, args });
    let value = null;
    if (command === 'list_style_packs') value = listed;
    if (command === 'save_style_pack') value = args.stylePack;
    if (command === 'create_style_pack_from_template') value = { ...args.template, id: 'new-id' };
    if (command === 'marketplace_like') value = { alreadyLiked: false, likeCount: 2 };
    if (command === 'marketplace_auth_status') value = { signedIn: true };
    if (command === 'github_device_flow_poll') value = { kind: 'authorized', login: 'fixture' };
    return JSON.stringify({ ok: true, value });
  }
}
const { Api } = load(source('data/Api.ets'), { HarmonyApi, StyleEditing });
await Api.createStylePack('New', 'speech');
assert.equal(calls.at(-1).command, 'create_style_pack_from_template');
assert.equal(calls.at(-1).args.template.prompt, 'speech');
await Api.updateStylePrompt(original.id, 'edited from current Core snapshot');
assert.equal(calls.at(-2).command, 'list_style_packs');
assert.equal(calls.at(-1).command, 'save_style_pack');
assert.deepEqual(calls.at(-1).args.stylePack, {
  ...original,
  prompt: 'edited from current Core snapshot',
});
listed = [];
const beforeMissing = calls.length;
await assert.rejects(Api.updateStylePrompt(original.id, 'lost target'), /不存在/);
assert.equal(calls.length, beforeMissing + 1, 'missing targets never send save');
assert.deepEqual(clone(await Api.likePack('remote-1')), { alreadyLiked: false, likeCount: 2 });
assert.equal((await Api.marketplaceAuthStatus()).signedIn, true);
assert.equal((await Api.githubDeviceFlowPoll('flow')).kind, 'authorized');
await Api.githubDeviceFlowCancel('flow');
assert.deepEqual(calls.at(-1), { command: 'github_device_flow_cancel', args: { flowId: 'flow' } });
await Api.openExternalUrl('https://github.com/login/device');
assert.equal(calls.at(-1).command, 'open_external_url');

const library = source('pages/LibraryPages.ets');
const start = library.indexOf('@Component\nexport struct StylePage');
const controllerSource =
  library
    .slice(start, library.indexOf('  build()', start))
    .replace('@Component\n', '')
    .replace('export struct StylePage', 'export class StylePage')
    .replace(/@(StorageLink|Watch)\('[^']*'\)\s*/g, '')
    .replace(/@State\s+/g, '') + '\n}\n';
function harness() {
  const messages = [],
    commands = [],
    timers = new Map();
  let nextTimer = 0;
  const backend = {
    packs: [clone(original)],
    signedIn: false,
    poll: { kind: 'authorized', login: 'fixture-user' },
    like: { alreadyLiked: true, likeCount: 4 },
    market: [clone(remote)],
  };
  const mock = {
    stylePacks: async () => clone(backend.packs),
    settings: async () => ({ preferences: { activeStylePackId: 'other' } }),
    createStylePack: async (name, prompt) => {
      commands.push(['create', name, prompt]);
    },
    updateStylePrompt: async (id, prompt) => {
      commands.push(['edit', id, prompt]);
    },
    marketplaceAuthStatus: async () => ({ signedIn: backend.signedIn }),
    marketplace: async (query, sort) => {
      commands.push(['search', query, sort]);
      return clone(backend.market);
    },
    githubDeviceFlowStart: async () => ({
      flowId: 'flow',
      userCode: 'PUBLIC',
      verificationUri: 'https://github.com/login/device',
      interval: 2,
      expiresIn: 600,
    }),
    githubDeviceFlowPoll: async () => {
      commands.push(['poll']);
      if (backend.poll.kind === 'authorized') backend.signedIn = true;
      return clone(backend.poll);
    },
    githubDeviceFlowCancel: async (id) => {
      commands.push(['cancel', id]);
    },
    copyText: async () => {},
    openExternalUrl: async (url) => {
      commands.push(['browser', url]);
    },
    likePack: async (id) => {
      commands.push(['like', id]);
      return clone(backend.like);
    },
    installMarketplacePack: async (id) => {
      commands.push(['install', id]);
    },
  };
  const { StylePage } = load(controllerSource, {
    Api: mock,
    StyleEditing,
    SHEET_CREATE: 'create',
    SHEET_EDIT: 'edit',
    SHEET_DETAIL: 'detail',
    SHEET_MARKET: 'market',
    Layout: { LG: 'lg', XL: 'xl' },
    notify: (message) => messages.push(message),
    setTimeout: (callback, delay) => {
      const id = ++nextTimer;
      timers.set(id, { callback, delay });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  });
  const page = new StylePage();
  page.viewActive = true;
  page.packs = clone(backend.packs);
  return {
    page,
    mock,
    backend,
    messages,
    commands,
    timers,
    fire() {
      const [id, timer] = timers.entries().next().value ?? [];
      assert.ok(timer, 'a pending timer is required');
      timers.delete(id);
      timer.callback();
      return timer.delay;
    },
  };
}
const create = harness();
create.page.newName = 'New';
create.page.newPrompt = 'literal';
await create.page.createPack();
assert.deepEqual(
  create.commands.find((entry) => entry[0] === 'create'),
  ['create', 'New', 'literal'],
);
const edit = harness();
edit.page.selectedId = original.id;
edit.page.startEdit();
assert.equal(edit.page.draftPrompt, original.prompt);
edit.page.draftPrompt = 'changed';
edit.page.selectedId = 'some-other-selection'; // Frozen editor target, not mutable UI selection.
await edit.page.savePrompt();
assert.deepEqual(
  edit.commands.find((entry) => entry[0] === 'edit'),
  ['edit', original.id, 'changed'],
);
const duplicate = harness();
const gate = deferred();
duplicate.mock.createStylePack = async () => {
  duplicate.commands.push(['create']);
  await gate.promise;
};
duplicate.page.newName = 'New';
const first = duplicate.page.createPack();
await duplicate.page.createPack();
assert.equal(duplicate.commands.filter((entry) => entry[0] === 'create').length, 1);
gate.resolve();
await first;

const login = harness();
const authorization = login.page.startLogin();
await tick();
assert.deepEqual(
  login.commands.find((entry) => entry[0] === 'browser'),
  ['browser', 'https://github.com/login/device'],
);
assert.equal(login.fire(), 2000);
await authorization;
assert.equal(login.page.signedIn, true);
assert.equal(login.page.loginState, '已登录 fixture-user');
assert.equal(
  login.commands.filter((entry) => entry[0] === 'poll').length,
  1,
  'authorized kind must stop polling',
);
assert.equal(login.timers.size, 0);
const cancelled = harness();
const pendingLogin = cancelled.page.startLogin();
await tick();
cancelled.page.aboutToDisappear();
await pendingLogin;
assert.equal(
  cancelled.commands.some((entry) => entry[0] === 'poll'),
  false,
);
assert.ok(cancelled.commands.some((entry) => entry[0] === 'cancel'));
assert.equal(cancelled.timers.size, 0);
const slow = harness();
slow.backend.poll = { kind: 'slowDown' };
const slowLogin = slow.page.startLogin();
await tick();
slow.fire();
await tick();
assert.equal(
  [...slow.timers.values()][0].delay,
  7000,
  'slowDown increases the next server poll interval',
);
slow.page.cancelLogin();
await slowLogin;

const search = harness();
const searches = [];
search.mock.marketplace = (query) => {
  const item = deferred();
  searches.push({ query, ...item });
  return item.promise;
};
search.page.marketQuery = 'a';
const a = search.page.reloadMarket();
await tick();
search.page.marketQuery = 'ab';
const ab = search.page.reloadMarket();
await tick();
searches[1].resolve([{ ...remote, id: 'AB' }]);
await ab;
searches[0].resolve([{ ...remote, id: 'A' }]);
await a;
assert.equal(search.page.marketPacks[0].id, 'AB', 'older search cannot overwrite newer results');
const likes = harness();
likes.page.marketPacks = [clone(remote)];
await likes.page.like(remote.id);
assert.equal(likes.page.marketPacks[0].likeCount, 4);
assert.equal(likes.page.marketLiked(remote.id), true);
likes.backend.like = { alreadyLiked: false, likeCount: 3 };
await likes.page.like(remote.id);
assert.equal(likes.page.marketLiked(remote.id), false);
assert.equal(likes.messages.at(-1), '已取消点赞');
const installed = harness();
installed.page.marketPacks = [clone(remote)];
await installed.page.install(remote.id);
assert.equal(
  installed.commands.some((entry) => entry[0] === 'install'),
  false,
  'installed origin-matched pack cannot be accidentally installed twice',
);
assert.match(library, /singleColumn|phoneLayout/);
assert.doesNotMatch(
  library,
  /StylePackPayload|\.builtIn|\.systemPrompt|poll\.state|auth\.loggedIn|pack\.installed/,
);
assert.match(
  library,
  /Layout\.dockClearance\(\) \+ 76/,
  'vocabulary floating actions keep existing layout clearance',
);
console.log(
  'Harmony library actions: Core creation, lossless prompt-only edits, missing-target refusal, duplicate guard, OAuth kind/cancellation/slowDown, last-query ownership, install origins and like toggles passed',
);
