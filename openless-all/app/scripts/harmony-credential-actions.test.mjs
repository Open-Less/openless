// Execute real credential/navigation methods with deferred effects; no device or real keys.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const base = new URL('../../harmony/entry/src/main/ets/', import.meta.url);
const read = (name) => readFileSync(new URL(name, base), 'utf8');
const credentials = read('pages/CredentialsPage.ets');
const shell = read('components/AppShell.ets');
const index = read('pages/Index.ets');
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
const tick = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};
function compile(source, globals) {
  const exports = {};
  const compiled = ts.transpileModule(source.replace(/^import .*;\n/gm, ''), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  assert.equal(compiled.diagnostics.length, 0, 'source must transpile');
  vm.runInNewContext(compiled.outputText, { exports, console, ...globals });
  return exports;
}
function method(source, name) {
  const match = new RegExp('(?:private )?' + name + '\\([^)]*\\)(?::[^\\{]+)?\\{').exec(source);
  assert.ok(match, name);
  let end = match.index + match[0].length,
    depth = 1;
  for (; depth > 0; end++) {
    if (source[end] === '{') depth++;
    if (source[end] === '}') depth--;
  }
  return source.slice(match.index, end).replace(/^private /, '');
}
const controllerSource =
  credentials
    .slice(
      credentials.indexOf('export struct CredentialsPage'),
      credentials.indexOf('  // ── Page'),
    )
    .replace('export struct CredentialsPage', 'export class CredentialsPage')
    .replace(/@(StorageLink|Watch)\('[^']*'\)\s*/g, '')
    .replace(/@State\s*/g, '') + '\n}\n';
function harness() {
  const storage = new Map([['route', 'style']]);
  const calls = [],
    pending = new Map();
  let page;
  const Store = {
    put(key, value) {
      storage.set(key, value);
      if (key === 'overlayBackRequest') {
        page.overlayBackRequest = value;
        page.onOverlayBack();
      }
    },
  };
  const AppStorage = { get: (key) => storage.get(key) };
  const Nav = compile(read('components/Nav.ets'), { Store, AppStorage }).Nav;
  const active = { llm: 'old-llm', asr: 'old-asr' };
  const Api = {
    settings: async () => ({
      preferences: { activeLlmProvider: active.llm, activeAsrProvider: active.asr },
    }),
    credentialStatus: async () => ({ llmConfigured: true }),
    channels: async () => [],
    providerDescriptors: async () => [],
    configuredOmniProviders: async () => [],
  };
  for (const name of [
    'createChannel',
    'deleteChannel',
    'renameChannel',
    'setCredential',
    'activateLlmProvider',
    'activateAsrProvider',
    'validateChannel',
    'recordChannelTest',
    'listModels',
  ]) {
    Api[name] = async (...args) => {
      calls.push([name, ...args]);
      if (name.startsWith('activate'))
        active[name === 'activateLlmProvider' ? 'llm' : 'asr'] = args[0];
      const wait = pending.get(name);
      if (wait) return wait.promise;
      if (name === 'createChannel')
        return { id: 'draft-channel', providerType: args[1], name: args[2] };
      if (name === 'validateChannel') return true;
    };
  }
  const { CredentialsPage } = compile(controllerSource, {
    Store,
    AppStorage,
    Api,
    UNSUPPORTED_PROVIDERS: [],
    Labels: { provider: (id) => id },
    notify() {},
  });
  page = new CredentialsPage();
  page.aboutToAppear();
  const { Index } = compile('export class Index {' + method(index, 'onBackPress') + '}', { Nav });
  const keys = { KEYCODE_1: 1, KEYCODE_2: 2, KEYCODE_3: 3, KEYCODE_4: 4, KEYCODE_5: 5 };
  const { AppShell } = compile('export class AppShell {' + method(shell, 'handleKey') + '}', {
    Nav,
    KeyCode: keys,
    KeyType: { Down: 0 },
  });
  const event = { type: 0, keyCode: 1, metaKey: 1, stopPropagation() {} };
  return {
    page,
    Nav,
    storage,
    calls,
    active,
    pending,
    back: () => new Index().onBackPress(),
    shortcut: () => new AppShell().handleKey(event),
    count: (name) => calls.filter((call) => call[0] === name).length,
    openEdit: () =>
      page.openEdit('llm', { id: 'existing', name: 'Existing', providerType: 'openai' }),
    wait(name) {
      const wait = deferred();
      pending.set(name, wait);
      return wait;
    },
  };
}

// X/cancel/scrim all share closeEditor; a stale global flag must not swallow Back.
for (const close of ['close', 'back', 'disappear']) {
  const h = harness();
  await tick();
  h.Nav.go(h.Nav.CREDENTIALS);
  h.page.openCreate();
  assert.equal(h.storage.get('overlayOpen'), true);
  if (close === 'close') h.page.closeEditor();
  else if (close === 'back') assert.equal(h.back(), true);
  else h.page.aboutToDisappear();
  assert.equal(h.page.editorOpen, false, close);
  assert.equal(h.storage.get('overlayOpen'), false, close);
  if (close !== 'disappear') {
    h.back();
    assert.equal(h.storage.get('route'), 'style');
  }
}

// Double Back, dock/deep-link navigation and keyboard shortcuts cannot escape a busy editor.
{
  const h = harness();
  await tick();
  h.Nav.go(h.Nav.CREDENTIALS);
  h.openEdit();
  h.page.drafts.set('ark.api_key', 'fixture-key');
  const saving = h.wait('setCredential');
  const work = h.page.saveEditor();
  await tick();
  assert.equal(h.count('setCredential'), 1);
  assert.equal(h.back(), true);
  assert.equal(h.back(), true);
  h.Nav.openPrimary(h.Nav.HISTORY);
  h.Nav.go(h.Nav.OVERVIEW);
  h.shortcut();
  assert.equal(h.storage.get('route'), 'credentials');
  assert.equal(h.storage.get('overlayOpen'), true);
  assert.equal(h.page.editorOpen, true);
  await h.page.deleteEditorChannel();
  await h.page.activateEditorChannel();
  assert.equal(h.count('deleteChannel'), 0);
  assert.equal(h.count('activateLlmProvider'), 0);
  saving.resolve();
  await work;
  assert.equal(h.storage.get('overlayOpen'), false);
  h.back();
  assert.equal(h.storage.get('route'), 'style');
}

// Freeze provider identity, reject duplicate create/type changes, and never auto-activate drafts.
{
  const h = harness();
  await tick();
  h.page.openCreate();
  h.page.editorName = 'Draft';
  const creating = h.wait('createChannel');
  const work = h.page.chooseProvider('openai');
  h.page.selectKind(1);
  assert.equal(h.page.editorKind, 'llm');
  await h.page.chooseProvider('another');
  assert.equal(h.count('createChannel'), 1);
  h.page.editorKind = 'asr'; // Even an external mutation must not retarget the awaited result.
  h.back();
  h.back();
  assert.equal(h.storage.get('overlayOpen'), true);
  creating.resolve({ id: 'draft-channel', providerType: 'openai', name: 'Draft' });
  await work;
  assert.equal(h.page.editorKind, 'llm');
  assert.equal(h.page.editorActive, false);
  assert.equal(h.count('activateLlmProvider') + h.count('activateAsrProvider'), 0);
  h.page.closeEditor();
  await tick();
  assert.deepEqual(
    h.calls.filter((call) => call[0] === 'deleteChannel'),
    [['deleteChannel', 'llm', 'draft-channel']],
  );
  assert.equal(h.active.llm, 'old-llm');
  assert.equal(h.active.asr, 'old-asr');
  assert.equal(h.storage.get('overlayOpen'), false);
}

// A create finishing after teardown cleans its uncommitted channel, never revives an overlay.
{
  const h = harness();
  await tick();
  h.page.openCreate();
  const creating = h.wait('createChannel');
  const work = h.page.chooseProvider('openai');
  h.page.aboutToDisappear();
  creating.resolve({ id: 'late-draft', providerType: 'openai', name: 'Late' });
  await work;
  await tick();
  assert.equal(h.page.editorOpen, false);
  assert.equal(h.storage.get('overlayOpen'), false);
  assert.ok(h.calls.some((call) => call[0] === 'deleteChannel' && call[2] === 'late-draft'));
  assert.equal(h.active.llm, 'old-llm');
}

// Testing blocks repeated test/save/activation; late results cannot alter a later editor.
{
  const h = harness();
  await tick();
  h.Nav.go(h.Nav.CREDENTIALS);
  h.openEdit();
  const testing = h.wait('validateChannel');
  const work = h.page.runTest();
  await tick();
  await h.page.runTest();
  await h.page.saveEditor();
  h.back();
  h.back();
  assert.equal(h.count('validateChannel'), 1);
  assert.equal(h.storage.get('overlayOpen'), true);
  h.page.aboutToDisappear();
  h.page.aboutToAppear();
  h.page.openCreate();
  testing.resolve(true);
  await work;
  assert.equal(h.count('recordChannelTest'), 0);
  assert.equal(h.page.editorStep, 'pick');
  assert.equal(h.page.editorTest, 'idle');
  assert.equal(h.storage.get('overlayOpen'), true);
}

// Only explicit activation changes routing; cancellation after use must retain the channel.
{
  const h = harness();
  await tick();
  h.page.openCreate();
  await h.page.chooseProvider('openai');
  await h.page.activateEditorChannel();
  assert.equal(h.active.llm, 'draft-channel');
  h.page.closeEditor();
  await tick();
  assert.equal(h.count('deleteChannel'), 0);
}
{
  const h = harness();
  await tick();
  h.openEdit();
  const deleting = h.wait('deleteChannel');
  const work = h.page.deleteEditorChannel();
  await h.page.deleteEditorChannel();
  h.page.closeEditor();
  assert.equal(h.page.editorOpen, true);
  deleting.resolve();
  await work;
  assert.equal(h.count('deleteChannel'), 1);
  assert.equal(h.storage.get('overlayOpen'), false);
}

// Return origins are transient, while the styles family remembers Translation targets.
{
  const h = harness();
  await tick();
  h.Nav.go(h.Nav.CREDENTIALS);
  h.Nav.back();
  assert.equal(h.storage.get('route'), 'style');
  h.Nav.go(h.Nav.SETTINGS);
  h.Nav.go(h.Nav.CREDENTIALS);
  h.Nav.go(h.Nav.HISTORY);
  h.Nav.openPrimary(h.Nav.SETTINGS);
  assert.equal(h.storage.get('route'), 'settings');
  h.Nav.go(h.Nav.TRANSLATION);
  h.Nav.go(h.Nav.VOCAB);
  h.Nav.openPrimary(h.Nav.STYLE);
  assert.equal(h.storage.get('route'), 'translation');
  h.Nav.go(h.Nav.STYLE);
  assert.equal(h.storage.get('route'), 'style', 'explicit expanded section still wins');
  h.Nav.go(h.Nav.VOCAB);
  h.Nav.openPrimary(h.Nav.STYLE);
  assert.equal(h.storage.get('route'), 'style');
}
assert.match(shell, /\.enabled\(!this\.overlayOpen\)/, 'back affordance respects modal state');
assert.match(
  shell,
  /\.enabled\(!this\.overlayOpen && !this\.keyboardVisible\)/,
  'dock cannot receive hits while a modal or keyboard is visible',
);
assert.match(
  shell,
  /\.visibility\(this\.overlayOpen \|\| this\.keyboardVisible \? Visibility\.Hidden : Visibility\.Visible\)/,
);
// Main-window keyboard subscription has stable identity, paired removal and late-event guards.
{
  const storage = new Map();
  const { Keyboard } = compile(
    'export class Keyboard { keyboardWindow; keyboardListener; ' +
      method(index, 'watchKeyboard') +
      method(index, 'stopKeyboardWatch') +
      '}',
    { Store: { put: (key, value) => storage.set(key, value) } },
  );
  const makeWindow = () => ({
    onCount: 0,
    offCount: 0,
    listener: undefined,
    on(name, callback) {
      assert.equal(name, 'keyboardHeightChange');
      this.onCount++;
      this.listener = callback;
    },
    off(name, callback) {
      assert.equal(name, 'keyboardHeightChange');
      assert.equal(callback, this.listener);
      this.offCount++;
    },
  });
  const page = new Keyboard(),
    first = makeWindow(),
    second = makeWindow();
  page.watchKeyboard(first);
  page.watchKeyboard(first);
  assert.equal(first.onCount, 1);
  first.listener(400);
  assert.equal(storage.get('keyboardVisible'), true);
  first.listener(0);
  assert.equal(storage.get('keyboardVisible'), false);
  const stale = first.listener;
  page.watchKeyboard(second);
  assert.equal(first.offCount, 1);
  stale(500);
  assert.equal(storage.get('keyboardVisible'), false);
  second.listener(500);
  assert.equal(storage.get('keyboardVisible'), true);
  page.stopKeyboardWatch();
  assert.equal(second.offCount, 1);
  assert.equal(storage.get('keyboardVisible'), false);
  second.listener(500);
  assert.equal(storage.get('keyboardVisible'), false);
  page.stopKeyboardWatch();
  assert.equal(second.offCount, 1);
}
assert.match(
  read('components/ProductNavigation.ets'),
  /this\.animateDock\(\(\) => \{\s*Nav\.openPrimary\(Nav\.STYLE\)/,
);
console.log(
  'Harmony credential actions: modal/back, busy guards, deferred identities, draft cleanup, explicit activation, origin routing and style restoration passed',
);

assert.match(shell, /barHeight\(this\.overlayOpen \|\| this\.keyboardVisible \? 0 : 78\)/);
assert.match(credentials, /credentials-editor-scrim/);
assert.match(credentials, /maxWidth: Layout\.contentMax\(this\.bp\)/);

// The catalogue stays in a native dropdown: no chip wall, no silent 16-item truncation.
{
  const h = harness();
  await tick();
  h.openEdit();
  h.page.llmDescriptors = [
    { providerType: 'openai', staticModels: [' model-1 ', 'preset'], defaultModel: 'default' },
  ];
  h.page.editorModels = [' ', ...Array.from({ length: 64 }, (_, i) => `model-${i}`), 'model-1'];
  const models = Array.from(h.page.availableModels());
  assert.equal(models.length, 66);
  assert.equal(models.at(-1), 'default');
  assert.equal(models.filter((m) => m === 'model-1').length, 1);
  assert.deepEqual(
    Array.from(h.page.modelOptions(), (option) => option.value),
    models,
  );
  assert.equal(h.page.selectedModelIndex(), -1, 'no implicit first selection');
  h.page.selectModel(40);
  assert.equal(h.page.editorModelSeed, 'model-40');
  assert.equal(h.page.drafts.get('ark.model_id'), 'model-40');
  assert.equal(h.page.selectedModelIndex(), 40);
  h.page.updateDraft('ark.model_id', 'manual-model');
  assert.equal(h.page.editorModelSeed, 'manual-model');
  assert.equal(h.page.selectedModelIndex(), -1, 'manual ids remain supported');
  h.page.selectModel(-1);
  h.page.selectModel(999);
  assert.equal(h.page.drafts.get('ark.model_id'), 'manual-model');
  assert.equal(h.count('setCredential'), 0, 'selecting does not save or activate implicitly');
  assert.equal(h.count('activateLlmProvider'), 0);
}
// Fetch serializes writes, preserves a manual selection and ignores late editor results.
{
  const h = harness();
  await tick();
  h.openEdit();
  h.page.updateDraft('ark.model_id', 'manual-model');
  const fetching = h.wait('listModels');
  const work = h.page.fetchModels();
  await tick();
  assert.equal(h.count('listModels'), 1);
  await h.page.fetchModels();
  await h.page.saveEditor();
  await h.page.runTest();
  await h.page.activateEditorChannel();
  await h.page.deleteEditorChannel();
  assert.equal(h.count('listModels'), 1);
  assert.equal(
    h.count('validateChannel') + h.count('activateLlmProvider') + h.count('deleteChannel'),
    0,
  );
  fetching.resolve(['fetched-model']);
  await work;
  assert.deepEqual(Array.from(h.page.editorModels), ['fetched-model']);
  assert.equal(h.page.editorModelSeed, 'manual-model', 'fetching does not select the first result');
  assert.equal(h.page.editorLoadingModels, false);
  h.page.selectModel(0);
  assert.equal(h.page.drafts.get('ark.model_id'), 'fetched-model');
}
{
  const h = harness();
  await tick();
  h.openEdit();
  const fetching = h.wait('listModels');
  const work = h.page.fetchModels();
  await tick();
  h.page.closeEditor();
  h.page.openCreate();
  fetching.resolve(['late-result']);
  await work;
  assert.equal(h.page.editorStep, 'pick');
  assert.deepEqual(Array.from(h.page.editorModels), []);
  assert.equal(h.page.editorLoadingModels, false);
  assert.equal(h.page.editorModelSeed, '');
}
{
  const h = harness();
  await tick();
  h.openEdit();
  const fetching = h.wait('listModels');
  const work = h.page.fetchModels();
  await tick();
  fetching.reject(new Error('fixture unavailable'));
  await work;
  assert.equal(h.page.editorLoadingModels, false);
  assert.equal(h.page.editorError, 'fixture unavailable');
  h.page.updateDraft('ark.model_id', 'still-editable');
  assert.equal(h.page.drafts.get('ark.model_id'), 'still-editable');
}
assert.match(credentials, /Select\(this\.modelOptions\(\)\)/);
assert.match(credentials, /credentials-model-fetch/);
assert.match(credentials, /credentials-model-select/);
assert.doesNotMatch(credentials, /可选模型（点按填入）|out\.length < 16/);
console.log(
  'Harmony model selector: full catalogue, dedup, manual ids, no implicit activation, busy guards and late fetch isolation passed',
);

// Provider slots have no persisted Core channel-test history or custom names.
{
  const h = harness();
  await tick();
  const channel = { id: 'gemini', providerType: 'gemini', name: 'gemini' };
  assert.match(h.page.channelSubtitle(channel, 'omni'), /配置已保存/);
  assert.doesNotMatch(h.page.channelSubtitle(channel, 'omni'), /还没测试过/);
  assert.match(h.page.channelSubtitle(channel, 'llm'), /还没测试过/);
}
assert.match(credentials, /if \(this\.editorKind !== 'omni'\) \{\s*InputField\(\{\s*label: '名称'/);
assert.match(credentials, /拉取前会先保存已填写配置/);
