// Verify real HarmonyOS ArkTS API & page controllers against strict openless-core wire contracts.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const root = new URL('../../harmony/entry/src/main/ets/', import.meta.url);
const read = (file) => readFileSync(new URL(file, root), 'utf8');
const tick = () => new Promise((resolve) => setImmediate(resolve));

function compile(source, globals = {}) {
  const exports = {};
  const clean = source
    .replace(/^import[\s\S]*?from '[^']+';\n/gm, '')
    .replace(/^@Component\n/gm, '')
    .replace(/@(StorageLink|Watch)\('[^']*'\)\s*/g, '')
    .replace(/@State\s+/g, '');
  const result = ts.transpileModule(clean, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  assert.equal(result.diagnostics?.length ?? 0, 0, 'source must transpile cleanly');
  vm.runInNewContext(result.outputText, { exports, console, ...globals });
  return exports;
}

// 1. Api.ets strict Core wire contracts: retranscribe sessionId, readRecording data URL, writePipelineMode.
{
  const calls = [];
  class HarmonyApi {
    async invokeAsync(command, argsJson) {
      const args = JSON.parse(argsJson);
      calls.push({ command, args });
      if (command === 'retranscribe_recording') {
        assert.equal(typeof args.sessionId, 'string', 'retranscribe_recording requires sessionId');
        assert.equal('id' in args, false, 'retranscribe_recording must not send legacy id key');
        return JSON.stringify({ ok: true, value: { text: 'ok', updatedEntry: null } });
      }
      if (command === 'read_audio_recording') {
        assert.equal(typeof args.sessionId, 'string');
        return JSON.stringify({ ok: true, value: 'data:audio/wav;base64,UklGRg==' });
      }
      if (command === 'create_channel') {
        return JSON.stringify({ ok: true, value: 'created-channel-42' });
      }
      if (command === 'list_channels') {
        return JSON.stringify({
          ok: true,
          value: [
            {
              id: 'created-channel-42',
              name: 'My Bailian',
              providerType: 'bailian',
              enabled: true,
              order: 0,
            },
          ],
        });
      }
      if (command === 'update_setting_fields') {
        return JSON.stringify({
          ok: true,
          value: {
            preferences: {
              pipelineMode: args.edits['/pipelineMode'],
              multimodalPipelineEnabled: args.edits['/multimodalPipelineEnabled'],
            },
            revision: 2,
          },
        });
      }
      return JSON.stringify({ ok: true, value: null });
    }
  }
  const { StyleEditing } = compile(read('data/StyleEditing.ets'));
  const { Api } = compile(read('data/Api.ets'), { HarmonyApi, StyleEditing });

  const retranscribed = await Api.retranscribe('session-123');
  assert.equal(retranscribed.text, 'ok');
  const recording = await Api.readRecording('session-123');
  assert.equal(
    recording.base64,
    'UklGRg==',
    'readRecording must unwrap data:audio/wav;base64, strings',
  );
  const created = await Api.createChannel('asr', 'bailian', 'My Bailian');
  assert.equal(
    created.id,
    'created-channel-42',
    'createChannel must resolve Rust string channel IDs to Channel objects',
  );
  assert.equal(created.providerType, 'bailian');
  const switched = await Api.writePipelineMode('multimodal');
  assert.equal(switched.preferences.pipelineMode, 'multimodal');
  assert.equal(switched.preferences.multimodalPipelineEnabled, true);
  assert.deepEqual(calls.at(-1), {
    command: 'update_setting_fields',
    args: { edits: { '/pipelineMode': 'multimodal', '/multimodalPipelineEnabled': true } },
  });
}

// 2. AudioPlayback strips data URL prefix defensively before Base64 decoding.
{
  const decoded = [];
  let fd = 0;
  const fs = {
    OpenMode: { CREATE: 1, READ_WRITE: 2, TRUNC: 4, READ_ONLY: 8 },
    openSync: (path) => ({ fd: ++fd, path }),
    writeSync: () => {},
    closeSync: () => {},
  };
  const { AudioPlayback } = compile(read('common/AudioPlayback.ets'), {
    fs,
    util: {
      Base64Helper: class {
        decodeSync(value) {
          decoded.push(value);
          return new Uint8Array([1, 2, 3, 4]);
        }
      },
    },
    media: {
      createAVPlayer: async () => ({
        on() {},
        async prepare() {},
        async play() {},
        async stop() {},
        async release() {},
      }),
    },
  });
  const player = new AudioPlayback();
  await player.play('/cache', 's1', 'data:audio/wav;base64,AAAA', () => {});
  assert.deepEqual(decoded, ['AAAA']);
  await player.stop();
}

// 3. StylePage GitHub device flow handles canonical Core expiresInSecs / intervalSecs fields.
{
  const library = read('pages/LibraryPages.ets');
  const start = library.indexOf('@Component\nexport struct StylePage');
  const source =
    library
      .slice(start, library.indexOf('  build()', start))
      .replace('export struct StylePage', 'export class StylePage') + '\n}\n';
  const timers = new Map();
  let nextTimer = 0;
  const commands = [];
  const Api = {
    marketplaceAuthStatus: async () => ({ signedIn: true }),
    marketplace: async () => [],
    githubDeviceFlowStart: async () => ({
      flowId: 'flow-core',
      userCode: 'ABCD-1234',
      verificationUri: 'https://github.com/login/device',
      expiresInSecs: 900,
      intervalSecs: 5,
    }),
    githubDeviceFlowPoll: async () => {
      commands.push('poll');
      return { kind: 'authorized', login: 'harmony-user' };
    },
    githubDeviceFlowCancel: async (id) => {
      commands.push('cancel:' + id);
    },
    copyText: async () => {},
    openExternalUrl: async () => {},
  };
  const { StyleEditing } = compile(read('data/StyleEditing.ets'));
  const { StylePage } = compile(source, {
    Api,
    StyleEditing,
    notify() {},
    SHEET_CREATE: 'create',
    SHEET_EDIT: 'edit',
    SHEET_DETAIL: 'detail',
    SHEET_MARKET: 'market',
    Layout: { LG: 'lg', XL: 'xl' },
    setTimeout: (cb, delay) => {
      const id = ++nextTimer;
      timers.set(id, { cb, delay });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  });
  const page = new StylePage();
  page.viewActive = true;
  const pending = page.startLogin();
  await tick();
  const [[id, entry]] = [...timers.entries()];
  assert.equal(entry.delay, 5000, 'intervalSecs=5 must schedule a 5000ms poll');
  timers.delete(id);
  entry.cb();
  await pending;
  assert.equal(page.signedIn, true);
  assert.equal(page.loginState, '已登录 harmony-user');
  assert.deepEqual(commands, ['poll']);
}

// 4. SettingsPage cloud sync uses marketplaceAuthStatus, real CloudSyncStatus fields, '{}' UI prefs and GitHub login.
{
  const settingsSource = read('pages/SettingsPage.ets');
  const start = settingsSource.indexOf('@Component\nexport struct SettingsPage');
  const controller =
    settingsSource
      .slice(start, settingsSource.indexOf('  // ── View', start))
      .replace('export struct SettingsPage', 'export class SettingsPage') + '\n}\n';
  const uploads = [];
  const timers = new Map();
  let nextTimer = 0;
  let signedIn = false;
  const Api = {
    settings: async () => ({
      preferences: { themeMode: 'system', translationTargetLanguage: 'English' },
      revision: 1,
    }),
    startupSnapshot: async () => ({ contractVersion: '2.0.0' }),
    platformCapabilities: async () => ({ platform: 'harmony', supportsImeInput: true }),
    network: async () => ({ online: true, latencyMs: 12 }),
    microphoneDevices: async () => [],
    history: async () => [],
    raw: async () => JSON.stringify({ ok: true, value: 'granted' }),
    marketplaceAuthStatus: async () => ({ signedIn }),
    cloudSyncStatus: async () => ({
      schemaVersion: 1,
      revision: 7,
      updatedAt: '2026-11-28T08:00:00Z',
      hasSnapshot: true,
      counts: { dictionary: 12, corrections: 3, stylePacks: 4 },
    }),
    cloudSyncUpload: async (baseRevision, uiPreferencesJson) => {
      uploads.push({ baseRevision, uiPreferences: JSON.parse(uiPreferencesJson) });
    },
    getUpdateChannel: async () => 'stable',
    setUpdateChannel: async (channel) => channel,
    checkUpdate: async (currentVersion, channel) => ({
      currentVersion,
      latestVersion: '2.1.0',
      tagName: 'v2.1.0',
      channel,
      hasUpdate: true,
      publishedAt: '2026-11-28T00:00:00Z',
      releaseUrl: 'https://github.com/Open-Less/openless/releases/tag/v2.1.0',
    }),
    githubDeviceFlowStart: async () => ({
      flowId: 'settings-flow',
      userCode: 'WXYZ-9999',
      verificationUri: 'https://github.com/login/device',
      expiresInSecs: 600,
      intervalSecs: 3,
    }),
    githubDeviceFlowPoll: async () => {
      signedIn = true;
      return { kind: 'authorized', login: 'aeoform' };
    },
    githubDeviceFlowCancel: async () => {},
    marketplaceLogout: async () => {
      signedIn = false;
    },
    copyText: async () => {},
    openExternalUrl: async () => {},
  };
  const { SettingsPage } = compile(controller, {
    Api,
    AppStorage: { get: () => '2.0.2' },
    Appearance: { apply() {} },
    Layout: { isWide: () => false },
    THEME_IDS: ['system', 'light', 'dark'],
    THEME_LABELS: ['跟随系统', '浅色', '深色'],
    UPDATE_CHANNELS: ['stable', 'beta'],
    UPDATE_CHANNEL_LABELS: ['正式版', 'Beta'],
    LANG_IDS: ['', '简体中文', 'English', '日本語', '한국어'],
    LANG_LABELS: ['关闭翻译', '简体中文', 'English', '日本語', '한국어'],
    SECTION_ALL: 'all',
    GROUP_SPECS: [],
    notify() {},
    setTimeout: (cb, delay) => {
      const id = ++nextTimer;
      timers.set(id, { cb, delay });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  });
  const page = new SettingsPage();
  page.getUIContext = () => ({ getHostContext: () => undefined });
  page.viewActive = true;
  await page.loadSyncStatus();
  assert.equal(page.syncLoggedIn, false);
  await page.syncUpload();
  assert.equal(uploads.length, 0, 'signed-out upload must be blocked');

  const login = page.startLogin();
  await tick();
  const [[timerId, entry]] = [...timers.entries()];
  assert.equal(entry.delay, 3000);
  timers.delete(timerId);
  entry.cb();
  await login;
  assert.equal(page.syncLoggedIn, true);
  assert.equal(page.syncRevision, 7);
  assert.equal(page.syncHasSnapshot, true);
  assert.equal(page.syncLast, '2026-11-28T08:00:00Z');
  assert.match(page.syncCountsText, /词典 12.*纠错 3.*风格 4/);

  await page.syncUpload();
  assert.deepEqual(uploads, [{ baseRevision: 7, uiPreferences: {} }]);

  await page.switchUpdateChannel(1);
  assert.equal(page.updateChannel, 'beta');
  await page.checkForUpdates();
  assert.equal(page.updateAvailable, true);
  assert.match(page.updateNote, /v2\.1\.0/);

  await page.logoutAccount();
  assert.equal(page.syncLoggedIn, false);
  assert.equal(page.syncHasSnapshot, false);

  // Settings must accurately describe HarmonyOS's shipped OpenLessInputMethod extension.
  assert.doesNotMatch(settingsSource, /鸿蒙端没有第三方键盘扩展/);
  assert.match(settingsSource, /OpenLess 鸿蒙输入法/);
}

// 5. TranslationPage writes canonical native languages and PolishMode values, never /translationEnabled.
{
  const tools = read('pages/ToolPages.ets');
  assert.doesNotMatch(
    tools,
    /\/translationEnabled/,
    'UserPreferences has no /translationEnabled field',
  );
  assert.doesNotMatch(
    tools,
    /'zh-CN'/,
    'translationTargetLanguage must use native names like 简体中文 / English',
  );
  const start = tools.indexOf('@Component\nexport struct TranslationPage');
  const controller =
    tools
      .slice(start, tools.indexOf('  build()', start))
      .replace('export struct TranslationPage', 'export class TranslationPage') + '\n}\n';
  const writes = [];
  const modeCalls = [];
  let prefs = { defaultMode: 'light', translationTargetLanguage: 'English' };
  const Api = {
    settings: async () => ({ preferences: { ...prefs }, revision: 1 }),
    writeString: async (pointer, value) => {
      writes.push([pointer, value]);
      if (pointer === '/translationTargetLanguage')
        prefs = { ...prefs, translationTargetLanguage: value };
      return { preferences: { ...prefs }, revision: 2 };
    },
    setDefaultPolishMode: async (mode) => {
      modeCalls.push(mode);
      prefs = { ...prefs, defaultMode: mode };
      return { preferences: { ...prefs }, revision: 3 };
    },
  };
  const { Labels } = compile(read('common/Labels.ets'));
  const { TranslationPage } = compile(controller, {
    Api,
    Labels,
    notify() {},
    LANGUAGES: ['简体中文', 'English', '日本語', '한국어'],
    LANGUAGE_NAMES: ['简体中文', 'English', '日本語', '한국어'],
    MODES: ['raw', 'light', 'structured', 'formal'],
    Layout: { LG: 'lg', XL: 'xl' },
  });
  const page = new TranslationPage();
  await page.reload();
  assert.equal(page.translationOn(), true);
  assert.equal(page.targetName(), 'English');
  await page.setTranslationEnabled(false);
  assert.deepEqual(writes.at(-1), ['/translationTargetLanguage', '']);
  assert.equal(page.translationOn(), false);
  await page.setTranslationEnabled(true);
  assert.deepEqual(writes.at(-1), ['/translationTargetLanguage', 'English']);
  await page.writePolishMode('formal');
  assert.deepEqual(modeCalls, ['formal']);
  assert.equal(Labels.polishMode('light'), '轻度润色');
  assert.equal(Labels.polishMode('formal'), '正式表达');
  assert.equal(Labels.provider('iflytek'), '讯飞 RTASR');
  assert.equal(Labels.provider('tencent-cloud'), '腾讯云');
}

// 6. CredentialsPage supports iflytek, tencent-cloud, volcengine, omni fields and pipeline switching.
{
  const credentials = read('pages/CredentialsPage.ets');
  const controller =
    credentials
      .slice(
        credentials.indexOf('export struct CredentialsPage'),
        credentials.indexOf('  // ── Page'),
      )
      .replace('export struct CredentialsPage', 'export class CredentialsPage') + '\n}\n';
  const calls = [];
  let pipelineMode = 'traditional';
  const Api = {
    settings: async () => ({
      preferences: { pipelineMode, multimodalPipelineEnabled: pipelineMode === 'multimodal' },
    }),
    credentialStatus: async () => ({ pipelineMode, omniConfigured: true }),
    channels: async (kind) => {
      assert.notEqual(kind, 'omni', 'ChannelKind only supports asr and llm');
      return [];
    },
    providerDescriptors: async (kind) =>
      kind === 'omni'
        ? [
            {
              kind: 'omni',
              providerType: 'gemini',
              labelKey: 'omniGemini',
              authRequirement: 'api_key',
            },
            {
              kind: 'omni',
              providerType: 'not-added',
              labelKey: 'notAdded',
              authRequirement: 'api_key',
            },
          ]
        : [],
    configuredOmniProviders: async () => [],
    writePipelineMode: async (mode) => {
      pipelineMode = mode;
      calls.push(['pipeline', mode]);
      return {
        preferences: { pipelineMode: mode, multimodalPipelineEnabled: mode === 'multimodal' },
        revision: 2,
      };
    },
    activateOmniProvider: async (id) => {
      calls.push(['activateOmni', id]);
    },
    validateChannel: async (kind, id) => {
      calls.push(['validate', kind, id]);
      return true;
    },
    createChannel: async () => {
      throw new Error('omni must not call createChannel');
    },
    renameChannel: async () => {
      throw new Error('omni must not call renameChannel');
    },
    recordChannelTest: async () => {
      throw new Error('omni must not call recordChannelTest');
    },
    deleteChannel: async () => {
      throw new Error('omni must not call deleteChannel');
    },
  };
  const { CredentialsPage } = compile(controller, {
    Api,
    Store: { put() {} },
    AppStorage: { get: () => undefined },
    UNSUPPORTED_PROVIDERS: [],
    Labels: { provider: (id) => id },
    notify() {},
  });
  const page = new CredentialsPage();
  page.pageActive = true;
  await page.reload();
  assert.equal(page.omniChannels.length, 0, 'provider catalogue is not an added-channel list');
  Api.configuredOmniProviders = async () => ['gemini'];
  await page.reload();
  assert.equal(page.omniChannels.length, 1);
  assert.equal(page.omniChannels[0].id, 'gemini');
  assert.equal(page.omniDescriptors.length, 2, 'unconfigured providers remain in the add picker');
  assert.deepEqual(
    Array.from(page.fieldSpecs('asr', 'iflytek'), (s) => s.account),
    ['xfyun.app_id', 'xfyun.api_key', 'asr.endpoint', 'asr.model'],
  );
  assert.deepEqual(
    Array.from(page.fieldSpecs('asr', 'tencent-cloud'), (s) => s.account),
    [
      'tencent_cloud.app_id',
      'tencent_cloud.secret_id',
      'tencent_cloud.secret_key',
      'asr.endpoint',
      'asr.model',
    ],
  );
  assert.deepEqual(
    Array.from(page.fieldSpecs('omni', 'gemini'), (s) => s.account),
    ['omni.api_key', 'omni.endpoint', 'omni.model'],
  );
  await page.switchPipeline(true);
  assert.deepEqual(calls[0], ['pipeline', 'multimodal']);
  assert.equal(page.isMultimodal(), true);
  page.openCreate();
  page.selectKind(2);
  assert.equal(page.editorKind, 'omni');
  await page.chooseProvider('gemini');
  await page.runTest();
  await page.activateEditorChannel();
  await page.deleteEditorChannel();
  assert.ok(calls.some((c) => c[0] === 'validate' && c[1] === 'omni' && c[2] === 'gemini'));
  assert.ok(calls.some((c) => c[0] === 'activateOmni' && c[1] === 'gemini'));
}

// 7. OverviewPage respects multimodal omniConfigured, ModelsPage avoids /localAsrEnabled,
//    InputMethodPanel ships no dead '待接入' tiles, and LibraryPages wires vocab edit/reset/dismiss.
{
  const overview = read('pages/OverviewPage.ets');
  assert.match(
    overview,
    /credentials\.pipelineMode === 'multimodal'[\s\S]{0,80}credentials\.omniConfigured === true/,
  );
  const models = read('pages/ModelsPage.ets');
  assert.doesNotMatch(models, /\/localAsrEnabled/);
  const ime = read('pages/InputMethodPanel.ets');
  assert.doesNotMatch(ime, /待接入/, 'IME toolbar must not expose dead placeholder tiles');
  const library = read('pages/LibraryPages.ets');
  assert.match(library, /Api\.updateVocabulary\(/);
  assert.match(library, /Api\.dismissSuggestions\(/);
  assert.match(library, /Api\.resetBuiltinStylePack\(/);
  const credentials = read('pages/CredentialsPage.ets');
  assert.match(credentials, /Api\.setChannelEnabled\(/);
  assert.match(credentials, /Api\.listModels\(/);
}

console.log(
  'Harmony Core wire contracts: retranscribe sessionId, audio data-URL, GitHub device flow, cloud sync, translation, multi-provider credentials and multimodal pipeline passed',
);
