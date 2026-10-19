import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../harmony/entry/src/main/ets/inputmethod/InputSession.ets', import.meta.url), 'utf8');
const stripImports = (text) => text.replace(/\r\n/g, '\n').replace(/^import .*;\n/gm, '');
assert.equal(stripImports(source), stripImports(source.replace(/\r?\n/g, '\r\n')));
const compiled = ts.transpileModule(stripImports(source), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

const clipboardExports = {};
vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../harmony/entry/src/main/ets/inputmethod/ClipboardHistory.ets', import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, { exports: clipboardExports });

function host(getEditorAttribute, clientOverrides = {}, queryCloud = async () => [], candidatePages = [], autoClipboard = false) {
  const schemas = [], inserted = [];
  const cloudTimers = [], operations = [];
  let pending = '', acknowledgements = 0;
  let timer, recordings = 0;
  let voiceStartup = Promise.resolve(), voiceResult = Promise.resolve({ rawText: '语音测试', polishedText: '语音结果' }), voiceStops = 0;
  let clipboardText = '本地剪贴板测试', clipboardListener, clipboardFailure;
  let clipboardReply;
  const board = {
    on: (_type, callback) => { clipboardListener = callback; },
    off: (_type, callback) => { if (clipboardListener === callback) clipboardListener = undefined; },
    getData: async () => {
      if (clipboardFailure) throw clipboardFailure;
      if (clipboardReply) return clipboardReply;
      const text = clipboardText;
      return { getPrimaryText: () => text };
    },
  };
  class Storage {
    constructor(initial) { this.values = new Map(Object.entries(initial)); }
    setOrCreate(key, value) { this.values.set(key, value); }
    get(key) { return this.values.get(key); }
  }
  class Voice {
    async start() { recordings++; await voiceStartup; }
    async stop() { voiceStops++; return voiceResult; }
    async cancel() {}
  }
  const exports = {};
  vm.runInNewContext(compiled, {
    exports, console, LocalStorage: Storage, VoiceClient: Voice, ClipboardHistory: clipboardExports.ClipboardHistory,
    pasteboard: {
      getSystemPasteboard: () => board,
    },
    preferences: { getPreferences: async () => ({ get: async (key) => autoClipboard && key === 'clipboard_history' ? '[]' : false, put: async () => {}, flush: async () => {} }) },
    cloudCandidates: queryCloud,
    refreshPublicDictionary: async () => ({ pending: false, revision: 'test', checked: 1 }),
    setTimeout: (callback, delay) => {
      if (delay === 800) return setTimeout(callback, 20);
      if (delay === 150 || delay === 60) return setTimeout(callback, 1);
      if (delay === 180) { cloudTimers.push(callback); return -1; }
      timer = callback; return -1;
    }, clearTimeout: (handle) => { if (handle !== -1) clearTimeout(handle); },
    initializeRime: async () => {}, testRime: async () => {},
    inputMethodEngine: {
      ENTER_KEY_TYPE_UNSPECIFIED: 0, ENTER_KEY_TYPE_SEND: 1, ENTER_KEY_TYPE_SEARCH: 2,
      ENTER_KEY_TYPE_DONE: 3, ENTER_KEY_TYPE_NEXT: 4, ENTER_KEY_TYPE_GO: 5,
      ENTER_KEY_TYPE_NEWLINE: 6,
      PATTERN_PASSWORD: 10, PATTERN_PASSWORD_NUMBER: 11,
      PATTERN_PASSWORD_SCREEN_LOCK: 12, PATTERN_NEW_PASSWORD: 13,
    },
    rimeRequest: async (operation, session, argument) => {
      operations.push([operation, argument]);
      if (candidatePages.length) {
        if (operation === 'select_absolute') pending = candidatePages.flat()[Number(argument.split('@')[0])];
        if (operation === 'ack') { pending = ''; acknowledgements++; }
        const offset = operation === 'candidates' ? Number(argument.split('@')[0]) : 0;
        const rows = operation === 'candidates' ? candidatePages.flat().slice(offset, offset + 32) : candidatePages[0];
        return { session: 'test', revision: 1, input: pending ? '' : 'ni', preedit: 'ni',
          candidates: rows, commit: pending, page: 0, candidateOffset: offset,
          candidateHasNext: offset + rows.length < candidatePages.flat().length, hasNext: candidatePages.length > 1 };
      }
      if (operation === 'create') { schemas.push(argument); pending = ''; }
      if (operation === 'select') pending = '测试';
      if (operation === 'external') pending = argument.slice(argument.indexOf('@') + 1);
      if (operation === 'ack') { acknowledgements++; pending = ''; }
      if (operation === 'clear') pending = '';
      return { session: 'test', revision: 1, input: '', preedit: '', candidates: ['测试'], commit: pending, page: 0, hasNext: false };
    },
  });
  const client = { getEditorAttribute, insertText: async (text) => { inserted.push(text); return true; }, ...clientOverrides };
  const session = exports.inputSession, state = exports.imeStorage;
  session.attach({ filesDir: '/test', resourceManager: { getBooleanByName: () => autoClipboard } }, {}, client);
  const pump = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  const drain = async () => {
    await pump();
    await session.queue;
  };
  return { session, state, schemas, inserted, drain, pump, timeout: () => timer(), recordings: () => recordings,
    voiceStartWith: (promise) => { voiceStartup = promise; }, voiceStopWith: (promise) => { voiceResult = promise; }, voiceStops: () => voiceStops,
    cloudTimers, operations,
    copy: (text) => { clipboardText = text; clipboardListener?.(); },
    watching: () => !!clipboardListener,
    clipboardError: (error) => { clipboardFailure = error; },
    clipboardDelay: (reply) => { clipboardReply = reply; },
    acknowledgements: () => acknowledgements };
}

const normal = host(async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: 'com.huawei.hmos.settings' }));
await normal.drain();
assert.deepEqual(normal.schemas, ['learning']);
assert.equal(normal.state.get('rimeReady'), true);
assert.equal(normal.state.get('editorVerified'), true);
assert.equal(normal.state.get('enterLabel'), '搜索');
assert.equal(normal.watching(), false, 'the default build must not subscribe to clipboard updates');
normal.session.refreshClipboard();
assert.equal(normal.state.get('clipboardAutoRead'), false);
normal.session.selectCandidate(0);
await normal.drain();
assert.equal(normal.acknowledgements(), 1);
assert.equal(normal.state.get('pendingCommit'), false);
assert.equal(normal.state.get('commitNeedsConfirmation'), false);

for (const [attribute, expectedRecords] of [
  [{ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }, 1],
  [{ inputPattern: 10, enterKeyType: 0, bundleName: 'native.test' }, 0],
  [undefined, 0],
]) {
  const pasted = host(async () => { if (!attribute) throw Error('unknown field'); return attribute; });
  await pasted.drain();
  let recorded = 0;
  pasted.session.clipboard = { add: () => { recorded++; }, list: () => [], serialize: () => '[]' };
  pasted.session.pasteClipboard();
  await pasted.drain();
  assert.equal(recorded, expectedRecords, 'password and unverified fields must not populate clipboard history');
  assert.deepEqual(pasted.inserted, ['本地剪贴板测试'], 'explicit system paste remains available in private fields');
}

const autoClipboard = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [], [], true);
await autoClipboard.drain();
assert.equal(autoClipboard.watching(), true);
assert.equal(autoClipboard.state.get('clipboardItems')[0].text, '本地剪贴板测试');
assert.deepEqual(autoClipboard.inserted, [], 'reading the clipboard must never type into the target editor');
autoClipboard.copy('第二次复制');
await autoClipboard.drain();
assert.equal(autoClipboard.state.get('clipboardItems').length, 2);
autoClipboard.copy('第二次复制');
await autoClipboard.drain();
assert.equal(autoClipboard.state.get('clipboardItems').length, 2, 'update notifications deduplicate clipboard history');
autoClipboard.clipboardError({ code: 201 });
autoClipboard.session.refreshClipboard();
await autoClipboard.drain();
assert.match(autoClipboard.state.get('clipboardStatus'), /授权/);
assert.equal(autoClipboard.state.get('clipboardItems').length, 2, 'permission denial preserves existing history');
autoClipboard.clipboardError(undefined);
let finishClearedClipboard;
autoClipboard.clipboardDelay(new Promise((resolve) => { finishClearedClipboard = resolve; }));
autoClipboard.session.refreshClipboard();
autoClipboard.session.clearClipboardHistory();
finishClearedClipboard({ getPrimaryText: () => '清空前的延迟读取' });
await autoClipboard.drain();
assert.equal(autoClipboard.state.get('clipboardItems').length, 0, 'a delayed clipboard read must not undo clearing history');
autoClipboard.clipboardDelay(undefined);
autoClipboard.copy('清空后新的复制');
await autoClipboard.drain();
assert.equal(autoClipboard.state.get('clipboardItems').length, 1, 'new clipboard notifications still work after clearing');
let finishClipboard;
autoClipboard.clipboardDelay(new Promise((resolve) => { finishClipboard = resolve; }));
autoClipboard.session.refreshClipboard();
autoClipboard.session.detach();
finishClipboard({ getPrimaryText: () => '过期读取不得入库' });
await autoClipboard.drain();
assert.equal(autoClipboard.watching(), false, 'detaching unregisters only this input session listener');
assert.equal(autoClipboard.state.get('clipboardItems').length, 1, 'stale clipboard reads cannot update another editor');
for (const inputPattern of [10, undefined]) {
  const privateClipboard = host(async () => { if (inputPattern === undefined) throw Error('unknown field');
    return { inputPattern, enterKeyType: 0, bundleName: 'native.test' }; }, {}, async () => [], [], true);
  await privateClipboard.drain();
  privateClipboard.session.refreshClipboard();
  privateClipboard.copy('私密输入不记录');
  await privateClipboard.drain();
  assert.equal(privateClipboard.watching(), false);
  assert.equal(privateClipboard.state.get('clipboardItems').length, 0);
}

const scrolling = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [],
  [['甲', '乙'], ['丙', '丁'], ['戊']]);
await scrolling.drain();
scrolling.session.loadMoreCandidates();
scrolling.session.loadMoreCandidates();
await scrolling.drain();
assert.deepEqual(Array.from(scrolling.state.get('candidates')), ['甲', '乙', '丙', '丁', '戊']);
assert.equal(scrolling.operations.filter(([op]) => op === 'candidates').length, 1);
scrolling.session.loadMoreCandidates();
await scrolling.drain();
assert.deepEqual(Array.from(scrolling.state.get('candidates')), ['甲', '乙', '丙', '丁', '戊']);
assert.equal(scrolling.state.get('hasNext'), false);
scrolling.session.loadMoreCandidates();
await scrolling.drain();
assert.equal(scrolling.operations.filter(([op]) => op === 'candidates').length, 1);
scrolling.session.selectCandidate(3, '丁');
await scrolling.drain();
assert(scrolling.operations.some(([op, arg]) => op === 'select_absolute' && arg === '3@1'));
assert.deepEqual(scrolling.inserted, ['丁']);

const batchPages = Array.from({ length: 20 }, (_, page) => ['词' + page * 2, '词' + (page * 2 + 1)]);
const batch = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [], batchPages);
await batch.drain();
batch.session.loadMoreCandidates();
await batch.drain();
assert.equal(batch.state.get('candidates').length, 34, 'one native range adds 32 candidates to the initial pool');
assert.equal(batch.state.get('hasNext'), true);
batch.session.loadMoreCandidates();
await batch.drain();
assert.equal(batch.state.get('candidates').length, 40);
assert.equal(batch.state.get('hasNext'), false);

const oldCandidate = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [],
  [['甲', '乙'], ['丙', '丁']]);
await oldCandidate.drain();
oldCandidate.session.loadMoreCandidates();
await oldCandidate.drain();
oldCandidate.session.selectCandidate(0, '甲');
await oldCandidate.drain();
assert(oldCandidate.operations.some(([op, arg]) => op === 'select_absolute' && arg === '0@1'));
assert.deepEqual(oldCandidate.inserted, ['甲']);

const scrollCloud = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [],
  [['甲', '乙'], ['丙', '丁']]);
await scrollCloud.drain();
scrollCloud.session.cloudChoices = ['云'];
scrollCloud.session.loadMoreCandidates();
await scrollCloud.drain();
assert.deepEqual(Array.from(scrollCloud.state.get('candidates')), ['甲', '云', '乙', '丙', '丁']);
scrollCloud.session.selectCandidate(4, '丁');
await scrollCloud.drain();
assert.deepEqual(scrollCloud.inserted, ['丁'], 'cloud insertion must not shift appended absolute indices');

const staleScroll = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, async () => [],
  [['甲', '乙'], ['丙', '丁']]);
await staleScroll.drain();
staleScroll.session.loadMoreCandidates();
staleScroll.session.publish({ revision: 99, input: 'hao', preedit: 'hao', candidates: ['好'], page: 0, hasNext: false, commit: '' });
await staleScroll.drain();
assert.deepEqual(Array.from(staleScroll.state.get('candidates')), ['好']);
assert.equal(staleScroll.state.get('candidatesLoading'), false);
assert.equal(staleScroll.operations.filter(([op]) => op === 'candidates').length, 0,
  'a queued append must not page a new composition');

let asyncWrites = 0, syncWrites = 0, asyncDeletes = 0;
const bridge = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.droi.tong' }), {
  insertText: async () => { asyncWrites++; return true; },
  insertTextSync: () => { syncWrites++; }, // Public sync API returns void on success.
  deleteForward: async () => { asyncDeletes++; return true; },
});
await bridge.drain();
bridge.session.selectCandidate(0);
await bridge.drain();
assert.equal(syncWrites, 0, 'never block the UI through synchronous IPC');
assert.equal(asyncWrites, 1);
assert.equal(bridge.acknowledgements(), 1);
assert.equal(bridge.state.get('pendingCommit'), false);
bridge.session.backspace();
await bridge.drain();
assert.equal(asyncDeletes, 1);

let accidentalRetries = 0, asynchronousAttempts = 0;
const failedWrite = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.huawei.hmos.settings' }), {
  insertText: async () => { asynchronousAttempts++; return false; },
  insertTextSync: () => { accidentalRetries++; },
});
await failedWrite.drain();
failedWrite.session.selectCandidate(0);
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), true);
assert.equal(failedWrite.acknowledgements(), 0);
assert.equal(accidentalRetries, 0, 'an uncertain write must never automatically replay through another transport');
failedWrite.session.acknowledgePendingCommit();
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), false);
assert.equal(failedWrite.acknowledgements(), 1);
assert.equal(asynchronousAttempts, 1, 'manual confirmation of an applied write must not insert it again');
failedWrite.session.selectCandidate(0);
await failedWrite.drain();
failedWrite.session.cancelPendingCommit();
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), false);
assert.equal(failedWrite.acknowledgements(), 1, 'cancel does not claim another failed write succeeded');

const syncFailure = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: async () => { throw { code: 12800003 }; },
  insertTextSync: () => { throw { code: 12800003 }; },
});
await syncFailure.drain();
syncFailure.session.selectCandidate(0);
await syncFailure.drain();
assert.equal(syncFailure.state.get('pendingCommit'), true);
assert.equal(syncFailure.acknowledgements(), 0);

let anonymousSyncWrites = 0;
const anonymousBridge = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  insertText: async () => true,
  insertTextSync: () => { anonymousSyncWrites++; },
});
await anonymousBridge.drain();
anonymousBridge.session.selectCandidate(0);
await anonymousBridge.drain();
assert.equal(anonymousSyncWrites, 0);
assert.equal(anonymousBridge.state.get('pendingCommit'), false);
assert.equal(anonymousBridge.acknowledgements(), 1);

let observedAttempts = 0;
const observed = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => { observedAttempts++; return new Promise(() => {}); },
});
await observed.drain();
observed.session.observeText('原文字');
observed.session.observeSelection(1, 2);
observed.session.selectCandidate(0);
await observed.pump();
observed.session.observeText('原错误字');
await observed.pump();
assert.equal(observed.acknowledgements(), 0, 'unrelated text must not confirm a commit');
observed.session.observeSelection(3, 3);
observed.session.observeText('原测试字');
await observed.drain();
assert.equal(observed.acknowledgements(), 1, 'exact selected-range replacement confirms an applied edit');
assert.equal(observedAttempts, 1, 'observation must not replay the insertion');
assert.equal(observed.state.get('pendingCommit'), false);

const noReceipt = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => new Promise(() => {}),
});
await noReceipt.drain();
noReceipt.session.selectCandidate(0);
await noReceipt.drain();
assert.equal(noReceipt.state.get('writeInFlight'), false, 'unresponsive IPC has a bounded UI wait');
assert.equal(noReceipt.state.get('pendingCommit'), true);
assert.equal(noReceipt.state.get('commitNeedsConfirmation'), true, 'only an unconfirmed completed attempt opens recovery');
noReceipt.session.observeText('测试');
noReceipt.session.observeSelection(2, 2);
await noReceipt.pump();
assert.equal(noReceipt.acknowledgements(), 0, 'without a baseline, text alone is insufficient proof');
noReceipt.session.acknowledgePendingCommit();
await noReceipt.drain();
noReceipt.session.selectCandidate(0);
await noReceipt.pump();
noReceipt.session.observeText('测试测试');
noReceipt.session.observeSelection(4, 4);
await noReceipt.drain();
assert.equal(noReceipt.acknowledgements(), 2, 'manual recovery preserves the active-field baseline for the next word');

let dispatchedDeletes = 0, dispatchedWrites = 0;
const dispatchedReplies = [];
const continuous = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  insertText: () => { dispatchedWrites++; return new Promise((resolve) => dispatchedReplies.push(resolve)); },
  deleteForward: async () => { dispatchedDeletes++; return new Promise(() => {}); },
});
await continuous.drain();
continuous.session.selectCandidate(0);
await continuous.drain();
assert.equal(continuous.state.get('pendingCommit'), false, 'missing bridge replies must not lock the first commit');
assert.equal(continuous.state.get('commitNeedsConfirmation'), false);
for (let i = 0; i < 3; i++) continuous.session.selectCandidate(0);
await continuous.drain();
assert.equal(dispatchedWrites, 4, 'one and only one dispatch per selected word');
assert.equal(continuous.state.get('pendingCommit'), false);
for (let i = 0; i < 5; i++) continuous.session.backspace();
await continuous.drain();
assert.equal(dispatchedDeletes, 5, 'missing delete replies must not serialize repeat keys behind timeouts');
dispatchedReplies[0](false);
await continuous.pump();
assert.equal(dispatchedWrites, 4, 'a late false reply must never resend committed text');
assert.equal(continuous.state.get('pendingCommit'), false, 'late bridge replies must not lock the next composition');
continuous.session.attach({ filesDir: '/test', resourceManager: {} }, {}, {
  getEditorAttribute: async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }),
  insertText: () => new Promise(() => {}),
});
await continuous.drain();
continuous.session.selectCandidate(0);
await continuous.drain();
assert.equal(continuous.state.get('pendingCommit'), false, 'new compatibility fields also allow continuous composition');
dispatchedReplies[1](false);
await continuous.pump();
assert.equal(continuous.state.get('status'), 'Rime 拼音', 'old editor failures must not affect the new field');

let actionCalls = 0, postActionWrites = 0;
const actions = host(async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: '' }), {
  sendKeyFunction: () => { actionCalls++; return new Promise(() => {}); },
  insertText: () => { postActionWrites++; return new Promise(() => {}); },
});
await actions.drain();
actions.session.enter();
actions.session.selectCandidate(0);
await actions.drain();
assert.equal(actionCalls, 1, 'editor action must be dispatched once');
assert.equal(postActionWrites, 1, 'a missing action reply must not block further input');

let releaseOldWrite, hangingAttempts = 0;
const hanging = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => {
    hangingAttempts++;
    return hangingAttempts === 1 ? new Promise((resolve) => { releaseOldWrite = resolve; }) : Promise.resolve(true);
  },
});
await hanging.drain();
hanging.session.selectCandidate(0);
await hanging.pump();
assert.equal(hanging.state.get('pendingCommit'), true);
assert.equal(hanging.state.get('writeInFlight'), true);
assert.equal(hanging.state.get('commitNeedsConfirmation'), false, 'normal pending IPC must not flash the recovery controls');
// Recovery must work while the original insertion promise is still unresolved.
hanging.session.acknowledgePendingCommit();
await hanging.drain();
assert.equal(hanging.state.get('pendingCommit'), false);
assert.equal(hanging.state.get('commitNeedsConfirmation'), false);
hanging.session.selectCandidate(0);
await hanging.drain();
assert.equal(hangingAttempts, 2);
assert.equal(hanging.acknowledgements(), 2);
releaseOldWrite(false);
await hanging.pump();
assert.equal(hanging.state.get('status'), 'Rime 拼音', 'late failure must not replace recovered state');
assert.equal(hanging.acknowledgements(), 2);

let lateFieldWrite;
const rebound = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => new Promise((resolve) => { lateFieldWrite = resolve; }),
});
await rebound.drain();
rebound.session.selectCandidate(0);
await rebound.pump();
rebound.session.attach({ filesDir: '/test', resourceManager: {} }, {}, {
  getEditorAttribute: async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: 'native.other' }),
  insertText: async () => true,
});
await rebound.drain();
assert.equal(rebound.state.get('rimeReady'), true, 'new field initialization must not wait for old IPC');
assert.equal(rebound.state.get('enterLabel'), '搜索');
assert.equal(rebound.state.get('pendingCommit'), false);
lateFieldWrite(true);
await rebound.pump();
assert.equal(rebound.acknowledgements(), 0, 'late old-field success must not acknowledge the new session');

const rejected = host(async () => { throw { code: 43 }; });
await rejected.drain();
assert.deepEqual(rejected.schemas, ['private']);
assert.equal(rejected.state.get('rimeReady'), true);
assert.equal(rejected.state.get('editorVerified'), false);
assert.equal(rejected.state.get('enterLabel'), '换行');
await rejected.session.startVoice();
assert.equal(rejected.recordings(), 0);
rejected.session.toggleLanguage();
rejected.session.insert('a');
await rejected.drain();
assert.deepEqual(rejected.inserted, ['a'], 'attribute failure must not block ordinary input');

let lateResolve;
const stalled = host(() => new Promise((resolve) => { lateResolve = resolve; }));
await stalled.drain();
assert.equal(stalled.state.get('rimeReady'), false);
stalled.timeout();
await stalled.drain();
assert.deepEqual(stalled.schemas, ['private']);
lateResolve({ inputPattern: 0, enterKeyType: 1 });
await stalled.drain();
assert.deepEqual(stalled.schemas, ['private'], 'late attributes must not upgrade unknown fields to learning');

const password = host(async () => ({ inputPattern: 10, enterKeyType: 0 }));
await password.drain();
assert.equal(password.state.get('password'), true);
assert.equal(password.state.get('chinese'), false);
assert.deepEqual(password.schemas, []);
password.session.observeText('private');
password.session.observeSelection(7, 7);
assert.equal(password.session.observedText, undefined);
assert.equal(password.session.selectionBegin, -1);
await password.session.startVoice();
assert.equal(password.recordings(), 0);

const detached = host(() => new Promise(() => {}));
detached.session.detach();
detached.timeout();
await detached.drain();
assert.deepEqual(detached.schemas, []);
assert.equal(detached.state.get('connected'), false);
console.log('Harmony editor compatibility: transports, commit acknowledgements, cancellation, attributes and privacy passed');

let resolveCloud, cloudQueries = 0;
const cloud = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, () => {
  cloudQueries++; return new Promise((resolve) => { resolveCloud = resolve; });
});
await cloud.drain();
const composition = { session: 'test', revision: 1, input: 'nihao', preedit: 'ni hao',
  candidates: ['你好', '你号'], commit: '', page: 0, hasNext: true };
cloud.session.publish(composition);
assert.equal(cloud.cloudTimers.length, 0, 'cloud is off by default');
cloud.state.setOrCreate('cloudEnabled', true);
cloud.session.publish(composition);
cloud.cloudTimers.shift()();
assert.equal(cloudQueries, 1);
resolveCloud(['你好', '你好世界']);
await cloud.pump();
assert.deepEqual(Array.from(cloud.state.get('candidates')), ['你好', '你好世界', '你号']);
cloud.session.selectCandidate(1, '你号');
await cloud.drain();
assert(cloud.operations.some(([op, arg]) => op === 'select' && arg === '1@1'), 'cloud insertion must not shift the native selection index');
cloud.session.publish(composition);
cloud.cloudTimers.shift()();
resolveCloud(['你好', '你好世界']);
await cloud.pump();
cloud.session.selectCandidate(1);
await cloud.drain();
assert.deepEqual(cloud.inserted, ['测试', '你好世界']);
assert(cloud.operations.some(([op, arg]) => op === 'external' && arg === '1@你好世界'));
cloud.session.publish(composition);
cloud.cloudTimers.shift()();
cloud.session.detach();
resolveCloud(['迟到的候选']);
await cloud.pump();
assert.equal(cloud.state.get('candidates').length, 0, 'late cloud replies cannot populate a different field');
password.state.setOrCreate('cloudEnabled', true);
password.session.publish(composition);
assert.equal(password.cloudTimers.length, 0, 'password input never schedules cloud queries');

let finishQuery, queryCount = 0;
const serialCloud = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {}, () => {
  queryCount++; return new Promise((resolve) => { finishQuery = resolve; });
});
await serialCloud.drain();
serialCloud.state.setOrCreate('cloudEnabled', true);
serialCloud.session.publish(composition);
serialCloud.cloudTimers.shift()();
serialCloud.session.publish({ ...composition, revision: 2, input: 'nihaoma', candidates: ['你好吗'] });
serialCloud.cloudTimers.shift()();
assert.equal(queryCount, 1, 'only one cloud IPC may occupy the worker pool');
finishQuery(['过期回复']);
await serialCloud.pump();
assert.deepEqual(Array.from(serialCloud.state.get('candidates')), ['你好吗']);
serialCloud.cloudTimers.shift()();
assert.equal(queryCount, 2, 'after a slow reply, query the latest composition only');
serialCloud.session.toggleCloudCandidates();
finishQuery(['你好么']);
await serialCloud.pump();
assert.deepEqual(Array.from(serialCloud.state.get('candidates')), ['你好吗'], 'switching cloud off discards in-flight results');
rejected.state.setOrCreate('cloudEnabled', true);
rejected.session.publish(composition);
assert.equal(rejected.cloudTimers.length, 0, 'unverified editors never query cloud');
console.log('Harmony cloud candidates: opt-in, privacy, serial queries, stale replies and native/external selection passed');

normal.session.insertSymbol('，');
assert.equal(normal.state.get('symbolFeedback'), '正在输入「，」');
await normal.drain();
assert.equal(normal.state.get('symbolFeedback'), '已输入「，」');
anonymousBridge.session.insertSymbol('！');
await anonymousBridge.drain();
assert.equal(anonymousBridge.state.get('symbolFeedback'), '已发送「！」', 'bridge dispatch is not a confirmed editor write');
failedWrite.session.insertSymbol('。');
await failedWrite.drain();
assert.equal(failedWrite.state.get('symbolFeedback'), '未能确认，请检查输入框');
normal.session.detach();
assert.equal(normal.state.get('symbolFeedback'), '', 'editor feedback must not leak across bindings');

const voiceEditor = () => host(async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: 'com.huawei.hmos.settings' }));
const held = voiceEditor();
await held.drain();
await held.session.startVoice(true);
assert.equal(held.state.get('voiceHolding'), true);
assert.equal(held.state.get('voicePhase'), 'recording');
held.session.releaseVoiceHold();
held.session.releaseVoiceHold();
held.session.cancelVoiceHold(); // Gesture teardown after release must not cancel recognition.
await held.drain();
assert.equal(held.voiceStops(), 1);
assert.deepEqual(held.inserted, ['语音结果'], 'release inserts once without a confirmation step');
assert.equal(held.state.get('voicePhase'), 'idle');

const earlyRelease = voiceEditor();
await earlyRelease.drain();
let finishStartup;
earlyRelease.voiceStartWith(new Promise((resolve) => { finishStartup = resolve; }));
const starting = earlyRelease.session.startVoice(true);
await earlyRelease.pump();
earlyRelease.session.releaseVoiceHold();
assert.equal(earlyRelease.state.get('voiceHolding'), false);
finishStartup();
await starting;
await earlyRelease.drain();
assert.equal(earlyRelease.voiceStops(), 1, 'release while the microphone prepares is retained');
assert.deepEqual(earlyRelease.inserted, ['语音结果']);

const cancelledHold = voiceEditor();
await cancelledHold.drain();
await cancelledHold.session.startVoice(true);
cancelledHold.session.cancelVoiceHold();
cancelledHold.session.releaseVoiceHold();
await cancelledHold.drain();
assert.deepEqual(cancelledHold.inserted, []);
assert.equal(cancelledHold.voiceStops(), 0);

const lateVoice = voiceEditor();
await lateVoice.drain();
let finishRecognition;
lateVoice.voiceStopWith(new Promise((resolve) => { finishRecognition = resolve; }));
await lateVoice.session.startVoice(true);
lateVoice.session.releaseVoiceHold();
await lateVoice.pump();
lateVoice.session.detach();
finishRecognition({ rawText: '过期结果', polishedText: '' });
await lateVoice.drain();
assert.deepEqual(lateVoice.inserted, [], 'late recognition cannot write after editor detaches');

const emptyVoice = voiceEditor();
await emptyVoice.drain();
emptyVoice.voiceStopWith(Promise.resolve({ rawText: '', polishedText: '' }));
await emptyVoice.session.startVoice(true);
emptyVoice.session.releaseVoiceHold();
await emptyVoice.drain();
assert.deepEqual(emptyVoice.inserted, []);
assert.equal(emptyVoice.state.get('voicePhase'), 'idle');
console.log('Harmony hold-to-talk: release, early release, cancellation, late results and empty recognition passed');
