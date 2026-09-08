import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';

const source = await readFile(
  new URL('../src-tauri/src/remote_server/assets/app.js', import.meta.url),
  'utf8',
);
const html = await readFile(
  new URL('../src-tauri/src/remote_server/assets/index.html', import.meta.url),
  'utf8',
);

function fakeElement() {
  const classes = new Set();
  return {
    listeners: {},
    style: {},
    hidden: false,
    checked: true,
    value: '',
    textContent: '',
    classList: {
      add: (...names) => names.forEach((name) => classes.add(name)),
      remove: (...names) => names.forEach((name) => classes.delete(name)),
      toggle: (name, enabled) => (enabled ? classes.add(name) : classes.delete(name)),
      contains: (name) => classes.has(name),
    },
    addEventListener(type, listener) { this.listeners[type] = listener; },
    querySelectorAll() { return []; },
    focus() {},
    select() {},
  };
}

async function openRemotePage({ defaultMode, savedMode } = {}) {
  const elements = new Map();
  const documentListeners = {};
  const sent = [];
  let socket;
  let worklet;

  const element = (id) => {
    if (!elements.has(id)) elements.set(id, fakeElement());
    return elements.get(id);
  };
  const storage = (entries = []) => {
    const values = new Map(entries);
    return {
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => values.set(key, String(value)),
      removeItem: (key) => values.delete(key),
    };
  };

  class FakeWebSocket {
    constructor() {
      this.readyState = 1;
      socket = this;
    }
    send(value) { sent.push(value); }
    close() { this.readyState = 3; }
  }

  class FakeAudioWorkletNode {
    constructor() {
      this.port = { onmessage: null };
      worklet = this;
    }
    connect() {}
    disconnect() {}
  }

  class FakeAudioContext {
    constructor() {
      this.state = 'running';
      this.sampleRate = 48_000;
      this.audioWorklet = { addModule: () => Promise.resolve() };
    }
    resume() { return Promise.resolve(); }
    suspend() { this.state = 'suspended'; }
    createMediaStreamSource() { return { connect() {}, disconnect() {} }; }
  }

  const document = {
    hidden: false,
    title: '',
    body: { appendChild() {}, removeChild() {} },
    getElementById: element,
    querySelectorAll: () => [],
    createElement: fakeElement,
    addEventListener(type, listener) { documentListeners[type] = listener; },
    removeEventListener() {},
    execCommand() {},
  };
  const context = {
    ArrayBuffer,
    Blob,
    DataView,
    Error,
    Math,
    Promise,
    Uint8Array,
    URL: { createObjectURL: () => 'blob:worklet' },
    AudioContext: FakeAudioContext,
    AudioWorkletNode: FakeAudioWorkletNode,
    WebSocket: FakeWebSocket,
    clearTimeout,
    console,
    document,
    isNaN,
    localStorage: storage([
      ['ol_remote_pin', '123456'],
      ...(savedMode === undefined ? [] : [['ol_remote_mode', savedMode]]),
    ]),
    location: { host: 'localhost:8443', origin: 'https://localhost:8443', reload() {} },
    navigator: {
      language: 'zh-CN',
      mediaDevices: {
        getUserMedia: () => Promise.resolve({ getTracks: () => [{ stop() {} }] }),
      },
    },
    performance: { now: () => 100 },
    sessionStorage: storage([['ol_reloaded_once', '1']]),
    setTimeout,
  };
  context.window = context;
  // Exercise the embedded HTML script too, so a missing template variable cannot
  // be hidden by setting window properties directly in the test harness.
  const injectedScript = html.match(/<script>([\s\S]*?)<\/script>/)[1]
    .replaceAll('%%OL_LANG%%', 'zh-CN')
    .replaceAll('%%OL_DEFAULT_MODE%%', defaultMode ?? '');
  runInNewContext(injectedScript, context, { filename: 'remote-server/assets/index.html' });
  runInNewContext(source, context, { filename: 'remote-server/assets/app.js' });

  socket.onopen();
  socket.onmessage({ data: JSON.stringify({ type: 'auth', ok: true }) });

  return {
    document,
    documentListeners,
    element,
    sent,
    socket,
    storage: context.localStorage,
    async start() {
      element('btn-record').listeners.click();
      for (let i = 0; i < 8; i += 1) await Promise.resolve();
      assert.ok(worklet?.port.onmessage, 'audio capture must be running');
    },
    pcm(bytes) { worklet.port.onmessage({ data: Uint8Array.from(bytes).buffer }); },
  };
}

// Exercise the displayed page, not a copied mode resolver: a new phone follows
// the PC setting, while a mode explicitly saved on that phone takes priority.
for (const [defaultMode, savedMode, expected] of [
  ['hold', undefined, 'hold'],
  ['toggle', undefined, 'toggle'],
  ['hold', 'toggle', 'toggle'],
  ['toggle', 'hold', 'hold'],
  ['hold', 'invalid', 'hold'],
  ['invalid', undefined, 'toggle'],
  [undefined, undefined, 'toggle'],
]) {
  const page = await openRemotePage({ defaultMode, savedMode });
  assert.equal(
    page.element('btn-record').style.touchAction,
    expected === 'hold' ? 'none' : 'manipulation',
    `PC default ${defaultMode}, phone choice ${savedMode} must use ${expected}`,
  );
  assert.equal(page.storage.getItem('ol_remote_mode'), savedMode ?? null,
    'inheriting a PC default must not create a phone override');
}

{
  const page = await openRemotePage({ defaultMode: 'hold' });
  page.element('mode-switch').listeners.click({
    target: { closest: () => ({ getAttribute: () => 'toggle' }) },
  });
  assert.equal(page.storage.getItem('ol_remote_mode'), 'toggle');
  assert.equal(page.element('btn-record').style.touchAction, 'manipulation');
}

const binaryFrames = (sent) => sent.filter((value) => value instanceof ArrayBuffer);
const payload = (frame) => Array.from(new Uint8Array(frame, 28));
const sequence = (frame) => {
  const view = new DataView(frame);
  return view.getUint32(20, false) * 0x100000000 + view.getUint32(24, false);
};

{
  const page = await openRemotePage();
  await page.start();
  page.pcm([1, 0, 2, 0]);
  page.pcm([3, 0]);
  assert.equal(binaryFrames(page.sent).length, 0, 'PCM must wait for the start ACK');
  assert.equal(page.element('status-text').textContent, '后端准备中…');

  page.socket.onmessage({
    data: JSON.stringify({ type: 'started', sessionId: '00112233-4455-6677-8899-aabbccddeeff' }),
  });
  const frames = binaryFrames(page.sent);
  assert.deepEqual(frames.map(sequence), [0, 1]);
  assert.deepEqual(frames.map(payload), [[1, 0, 2, 0], [3, 0]]);
}

{
  const page = await openRemotePage();
  await page.start();
  page.pcm([4, 0, 5, 0]);
  page.element('btn-record').listeners.click();
  assert.equal(binaryFrames(page.sent).length, 0);
  assert.equal(page.sent.some((value) => typeof value === 'string' && JSON.parse(value).type === 'stop'), false);

  page.socket.onmessage({
    data: JSON.stringify({ type: 'started', sessionId: '10112233-4455-6677-8899-aabbccddeeff' }),
  });
  const frameIndex = page.sent.findIndex((value) => value instanceof ArrayBuffer);
  const stopIndex = page.sent.findIndex(
    (value) => typeof value === 'string' && JSON.parse(value).type === 'stop',
  );
  assert.ok(frameIndex >= 0 && stopIndex > frameIndex, 'queued PCM must be sent before stop');
  assert.deepEqual(payload(page.sent[frameIndex]), [4, 0, 5, 0]);
  assert.equal(page.element('status-text').textContent, '识别中');
  page.socket.onmessage({ data: JSON.stringify({ type: 'status', kind: 'error' }) });
}

for (const terminal of ['cancel', 'busy', 'disconnect']) {
  const page = await openRemotePage();
  await page.start();
  page.pcm([6, 0]);
  if (terminal === 'cancel') {
    page.document.hidden = true;
    page.documentListeners.visibilitychange();
  } else if (terminal === 'busy') {
    page.socket.onmessage({ data: JSON.stringify({ type: 'busy', reason: 'test' }) });
  } else {
    page.socket.onclose();
  }
  page.socket.onmessage({
    data: JSON.stringify({ type: 'started', sessionId: '20112233-4455-6677-8899-aabbccddeeff' }),
  });
  assert.equal(binaryFrames(page.sent).length, 0, `${terminal} must discard queued PCM`);
  if (terminal === 'busy') {
    page.socket.onmessage({ data: JSON.stringify({ type: 'status', kind: 'error' }) });
  }
}

{
  const page = await openRemotePage();
  await page.start();
  page.pcm(new Uint8Array(64 * 1024));
  page.pcm(new Uint8Array(64 * 1024));
  assert.equal(page.element('status-text').textContent, '后端准备中…');
  page.pcm([0, 0]);
  assert.match(page.element('status-text').textContent, /音频缓存已满/);
  assert.ok(page.sent.some((value) => typeof value === 'string' && JSON.parse(value).type === 'cancel'));
  assert.equal(binaryFrames(page.sent).length, 0);
}

console.log('remote-input-audio-queue.test.mjs passed');
