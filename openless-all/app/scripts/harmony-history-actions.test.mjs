// Execute actual history action methods and player lifecycle with delayed ports.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const ets = (name) =>
  readFileSync(new URL('../../harmony/entry/src/main/ets/' + name, import.meta.url), 'utf8');
function load(source, globals) {
  const exports = {};
  const clean = source
    .replace(/^import[\s\S]*?from '[^']+';\n/gm, '')
    .replace(/^@Component\n/gm, '')
    .replace(/@(StorageLink|Watch)\('[^']*'\)\s*/g, '')
    .replace(/@State\s+/g, '')
    .replace('export struct HistoryPage', 'export class HistoryPage');
  const result = ts.transpileModule(clean, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  });
  vm.runInNewContext(result.outputText, { exports, console, ...globals });
  return exports;
}
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
const source = ets('pages/HistoryPage.ets');
const methods = source.slice(0, source.indexOf('  // ── Build')) + '\n}\n';
const A = { id: 'A', finalText: 'A final', rawTranscript: 'A raw' };
const B = { id: 'B', finalText: 'B final', rawTranscript: 'B raw' };
function history() {
  const polish = [],
    reads = [],
    played = [],
    notices = [];
  const audio = {
    stops: 0,
    stop: async () => {
      audio.stops++;
    },
    play: async (...args) => {
      played.push(args);
      args.at(-1)(true);
    },
  };
  const Api = {
    repolish: (text, style) => {
      const d = deferred();
      polish.push({ ...d, text, style });
      return d.promise;
    },
    readRecording: (id) => {
      const d = deferred();
      reads.push({ ...d, id });
      return d.promise;
    },
  };
  const { HistoryPage } = load(methods, {
    AudioPlayback: class {
      constructor() {
        return audio;
      }
    },
    Api,
    notify: (text) => notices.push(text),
    clearTimeout,
    setTimeout,
  });
  const page = new HistoryPage();
  page.pageActive = true;
  page.entries = [A, B];
  page.detailId = 'A';
  page.getUIContext = () => ({ getHostContext: () => ({ cacheDir: '/cache' }) });
  page.flushDraft = () => {};
  page.cancelSearch = () => {};
  return {
    page,
    polish,
    reads,
    played,
    notices,
    audio,
    select: (id) => {
      page.detailId = id;
      page.onDetailChange();
    },
  };
}
{
  const h = history();
  const pending = h.page.repolish(A, { id: 'style-a', name: 'Style A' });
  h.select('B');
  const next = h.page.repolish(B, { id: 'style-b', name: 'Style B' });
  h.polish[0].resolve('late A output');
  await pending;
  assert.equal(h.page.repolished, '');
  assert.equal(h.page.repolishedStyle, 'Style B');
  assert.equal(h.page.busy, true, 'old finally must not enable a newer action');
  h.polish[1].resolve('B output');
  await next;
  assert.equal(h.page.repolished, 'B output');
  assert.equal(h.page.busy, false);
}
{
  const h = history();
  const pending = h.page.repolish(A, { id: 'style', name: 'Style' });
  h.select('B');
  h.polish[0].reject(new Error('late A failure'));
  await pending;
  assert.equal(h.page.detailError, '');
}
for (const action of ['select', 'leave']) {
  const h = history();
  const pending = h.page.togglePlayback(A);
  if (action === 'select') h.select('B');
  else h.page.aboutToDisappear();
  h.reads[0].resolve({ base64: 'old audio' });
  await pending;
  assert.equal(h.played.length, 0, action + ' must prevent delayed playback');
  assert.equal(h.page.playing, false);
}
{
  const h = history();
  const first = h.page.togglePlayback(A),
    second = h.page.togglePlayback(A);
  h.reads[0].resolve({ base64: 'first' });
  await first;
  assert.equal(h.played.length, 0, 'only the latest pending playback owns audio');
  h.reads[1].resolve({ base64: 'second' });
  await second;
  assert.equal(h.played.length, 1);
  assert.equal(h.page.playing, true);
  h.select('B');
  h.played[0].at(-1)(true);
  assert.equal(h.page.playing, false, 'old player callback cannot follow the selection');
}

const playback = ets('common/AudioPlayback.ets');
function playerHarness() {
  const create = [],
    closed = [],
    writes = [],
    events = [];
  let fd = 0;
  const fs = {
    OpenMode: { CREATE: 1, READ_WRITE: 2, TRUNC: 4, READ_ONLY: 8 },
    openSync: (path) => ({ fd: ++fd, path }),
    writeSync: (descriptor) => writes.push(descriptor),
    closeSync: (file) => closed.push(file.fd),
  };
  const { AudioPlayback } = load(playback, {
    fs,
    util: {
      Base64Helper: class {
        decodeSync() {
          return new Uint8Array([1, 2, 3]);
        }
      },
    },
    media: {
      createAVPlayer: () => {
        const d = deferred();
        create.push(d);
        return d.promise;
      },
    },
  });
  return { controller: new AudioPlayback(), create, closed, writes, events };
}
const tick = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};
function player({ prepare, stopThrows = false } = {}) {
  return {
    releases: 0,
    starts: 0,
    listeners: {},
    on(name, fn) {
      this.listeners[name] = fn;
    },
    prepare: () => (prepare ? prepare.promise : Promise.resolve()),
    async play() {
      this.starts++;
    },
    async stop() {
      if (stopThrows) throw new Error('not prepared');
    },
    async release() {
      this.releases++;
    },
  };
}
{
  const h = playerHarness();
  const pending = h.controller.play('/cache', 'A', 'abc', (value) => h.events.push(value));
  await tick();
  await h.controller.stop();
  const late = player();
  h.create[0].resolve(late);
  await pending;
  assert.equal(late.starts, 0);
  assert.equal(late.releases, 1);
  assert.deepEqual(h.events, []);
  assert.equal(h.controller.isPlaying(), false);
}
{
  const h = playerHarness(),
    preparation = deferred();
  const pending = h.controller.play('/cache', 'A', 'abc', (value) => h.events.push(value));
  await tick();
  const p = player({ prepare: preparation, stopThrows: true });
  h.create[0].resolve(p);
  await tick();
  await h.controller.stop();
  preparation.resolve();
  await pending;
  assert.equal(p.starts, 0);
  assert.equal(p.releases, 1, 'stop failure must not skip release');
  assert.equal(h.closed.length, 2, 'both write and player source descriptors close');
}
console.log(
  'Harmony history actions: selection-bound async results, playback cancellation, obsolete callbacks and descriptor cleanup passed',
);
