import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';

const source = await readFile(
  new URL('../assets/remote-input/app.js', import.meta.url),
  'utf8',
);
const html = await readFile(
  new URL('../assets/remote-input/index.html', import.meta.url),
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
    addEventListener(type, listener) {
      this.listeners[type] = listener;
    },
    querySelectorAll() {
      return [];
    },
    focus() {},
    select() {},
  };
}

const fixtureRecoveryKey = '40112233-4455-4677-8899-aabbccddeeff';
async function openRemotePage({
  defaultMode,
  savedMode,
  savedWakeLock,
  savedRecovery,
  wakeLockMode = 'supported',
} = {}) {
  const elements = new Map();
  const documentListeners = {};
  const sent = [];
  let socket;
  let worklet;
  let audioContext;
  const windowListeners = {};
  const wakeLocks = [];
  const tracks = [];
  const wakeResolvers = [];
  let wakeRequests = 0;
  const timers = new Map();
  let now = 0;
  let timerId = 0;
  const flush = async () => {
    for (let i = 0; i < 12; i += 1) await Promise.resolve();
  };

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
    send(value) {
      sent.push(value);
    }
    close() {
      this.readyState = 3;
    }
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
      audioContext = this;
      this.state = 'running';
      this.sampleRate = 48_000;
      this.audioWorklet = { addModule: () => Promise.resolve() };
    }
    resume() {
      this.state = 'running';
      return Promise.resolve();
    }
    suspend() {
      this.state = 'suspended';
    }
    createMediaStreamSource() {
      return { connect() {}, disconnect() {} };
    }
  }

  const document = {
    hidden: false,
    title: '',
    body: { appendChild() {}, removeChild() {} },
    getElementById: element,
    querySelectorAll: () => [],
    createElement: fakeElement,
    addEventListener(type, listener) {
      documentListeners[type] = listener;
    },
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
    clearTimeout: (id) => timers.delete(id),
    console,
    document,
    isNaN,
    localStorage: storage([
      ['ol_remote_pin', '123456'],
      ...(savedMode === undefined ? [] : [['ol_remote_mode', savedMode]]),
      ...(savedWakeLock === undefined ? [] : [['ol_remote_wake_lock', savedWakeLock]]),
      ...(savedRecovery === undefined
        ? []
        : [
            [
              'ol_remote_recovery_session',
              JSON.stringify({ sessionId: savedRecovery, key: fixtureRecoveryKey }),
            ],
          ]),
    ]),
    location: { host: 'localhost:8443', origin: 'https://localhost:8443', reload() {} },
    navigator: {
      language: 'zh-CN',
      wakeLock:
        wakeLockMode === 'unsupported'
          ? undefined
          : {
              request: (type) => {
                assert.equal(type, 'screen');
                wakeRequests++;
                if (wakeLockMode === 'rejected') return Promise.reject(new Error('system denied'));
                const sentinel = {
                  released: false,
                  releaseCount: 0,
                  listener: null,
                  addEventListener(type, listener) {
                    assert.equal(type, 'release');
                    this.listener = listener;
                  },
                  release() {
                    this.released = true;
                    this.releaseCount++;
                    this.listener?.();
                    return Promise.resolve();
                  },
                };
                wakeLocks.push(sentinel);
                return wakeLockMode === 'deferred'
                  ? new Promise((resolve) => wakeResolvers.push(() => resolve(sentinel)))
                  : Promise.resolve(sentinel);
              },
            },
      mediaDevices: {
        getUserMedia: () => {
          const track = {
            stopped: false,
            stop() {
              this.stopped = true;
            },
          };
          tracks.push(track);
          return Promise.resolve({ getTracks: () => [track] });
        },
      },
    },
    performance: { now: () => 100 },
    sessionStorage: storage([['ol_reloaded_once', '1']]),
    setTimeout: (callback, delay) => {
      const id = ++timerId;
      timers.set(id, { callback, at: now + delay });
      return id;
    },
    addEventListener: (type, listener) => {
      windowListeners[type] = listener;
    },
  };
  context.window = context;
  // Exercise the embedded HTML script too, so a missing template variable cannot
  // be hidden by setting window properties directly in the test harness.
  const injectedScript = html
    .match(/<script>([\s\S]*?)<\/script>/)[1]
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
    get socket() {
      return socket;
    },
    get audioContext() {
      return audioContext;
    },
    get wakeRequests() {
      return wakeRequests;
    },
    wakeLocks,
    tracks,
    wakeResolvers,
    windowListeners,
    flush,
    async advance(ms) {
      now += ms;
      for (const [id, timer] of [...timers]) {
        if (timer.at <= now && timers.delete(id)) timer.callback();
      }
      await flush();
    },
    storage: context.localStorage,
    async start() {
      if (element('btn-record').style.touchAction === 'none') {
        element('btn-record').listeners.pointerdown({ preventDefault() {} });
      } else {
        element('btn-record').listeners.click();
      }
      await flush();
      assert.ok(worklet?.port.onmessage, 'audio capture must be running');
    },
    pcm(bytes) {
      worklet.port.onmessage({ data: Uint8Array.from(bytes).buffer });
    },
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
  assert.equal(
    page.storage.getItem('ol_remote_mode'),
    savedMode ?? null,
    'inheriting a PC default must not create a phone override',
  );
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
  assert.deepEqual(frames.map(payload), [
    [1, 0, 2, 0],
    [3, 0],
  ]);
}

{
  const page = await openRemotePage();
  await page.start();
  page.pcm([4, 0, 5, 0]);
  page.element('btn-record').listeners.click();
  assert.equal(binaryFrames(page.sent).length, 0);
  assert.equal(
    page.sent.some((value) => typeof value === 'string' && JSON.parse(value).type === 'stop'),
    false,
  );

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
    page.element('mode-switch').listeners.click({
      target: { closest: () => ({ getAttribute: () => 'hold' }) },
    });
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
  assert.ok(
    page.sent.some((value) => typeof value === 'string' && JSON.parse(value).type === 'cancel'),
  );
  assert.equal(binaryFrames(page.sent).length, 0);
}

const controls = (page, type) =>
  page.sent
    .filter((value) => typeof value === 'string')
    .map((value) => JSON.parse(value))
    .filter((value) => value.type === type);
const recordingId = '30112233-4455-4677-8899-aabbccddeeff';
const acknowledge = (page) =>
  page.socket.onmessage({
    data: JSON.stringify({
      type: 'started',
      sessionId: recordingId,
      recoveryKey: fixtureRecoveryKey,
    }),
  });

// 息屏结束两分钟录音，已发送的帧仍在 stop 之前；重复生命周期事件不会重复结束。
for (const defaultMode of ['toggle', 'hold']) {
  const page = await openRemotePage({ defaultMode });
  assert.equal(page.element('wake-lock-switch').checked, true);
  assert.equal(page.storage.getItem('ol_remote_wake_lock'), null);
  await page.start();
  acknowledge(page);
  for (let second = 0; second < 120; second++) page.pcm(new Uint8Array(32_000).fill(second));
  page.document.hidden = true;
  page.documentListeners.visibilitychange();
  page.windowListeners.pagehide();
  assert.equal(controls(page, 'stop').length, 1);
  assert.equal(controls(page, 'cancel').length, 0);
  assert.equal(
    binaryFrames(page.sent).reduce((bytes, frame) => bytes + frame.byteLength - 28, 0),
    120 * 32_000,
  );
  assert.equal(JSON.parse(page.sent.at(-1)).type, 'stop');
  assert.equal(page.wakeLocks[0].released, true);
  assert.deepEqual(JSON.parse(page.storage.getItem('ol_remote_recovery_session')), {
    sessionId: recordingId,
    key: fixtureRecoveryKey,
  });
  page.document.hidden = false;
  page.documentListeners.visibilitychange();
  assert.equal(controls(page, 'start').length, 1, '恢复前台不会自动打开麦克风');
  assert.equal(page.wakeRequests, 1);
}

{
  const page = await openRemotePage();
  await page.start();
  page.pcm([7, 0, 8, 0]);
  page.document.hidden = true;
  page.documentListeners.visibilitychange();
  assert.equal(controls(page, 'cancel').length, 0);
  acknowledge(page);
  assert.deepEqual(payload(binaryFrames(page.sent)[0]), [7, 0, 8, 0]);
  assert.equal(JSON.parse(page.sent.at(-1)).type, 'stop', 'ACK 迟到时保留首段音频并按序结束');
}

{
  const page = await openRemotePage({ savedWakeLock: '0' });
  assert.equal(page.element('wake-lock-switch').checked, false);
  await page.start();
  assert.equal(page.wakeRequests, 0);
  const toggle = page.element('wake-lock-switch');
  toggle.checked = true;
  toggle.listeners.change();
  await page.flush();
  assert.equal(page.wakeRequests, 1);
  assert.equal(page.storage.getItem('ol_remote_wake_lock'), '1');
  toggle.checked = false;
  toggle.listeners.change();
  assert.equal(page.wakeLocks[0].releaseCount, 1);
  assert.equal(page.storage.getItem('ol_remote_wake_lock'), '0');
}

for (const wakeLockMode of ['unsupported', 'rejected']) {
  const page = await openRemotePage({ wakeLockMode });
  await page.start();
  assert.equal(controls(page, 'start').length, 1, '保持亮屏失败不阻止录音');
  assert.match(page.element('wake-lock-hint').textContent, /未允许/);
  acknowledge(page);
  page.audioContext.state = 'interrupted';
  page.audioContext.onstatechange();
  assert.equal(controls(page, 'stop').length, 1);
  assert.equal(controls(page, 'cancel').length, 0);
}

{
  const page = await openRemotePage();
  await page.start();
  acknowledge(page);
  page.tracks[0].onended();
  assert.equal(controls(page, 'stop').length, 1);
  assert.equal(page.tracks[0].stopped, true);
  page.socket.onmessage({ data: JSON.stringify({ type: 'status', kind: 'done' }) });
  await page.start();
  assert.equal(page.tracks.length, 2, '系统中断后的下一次录音必须重新获取麦克风');
  assert.equal(page.tracks[1].stopped, false);
}

{
  const page = await openRemotePage({ wakeLockMode: 'deferred' });
  await page.start();
  acknowledge(page);
  page.element('btn-record').listeners.click();
  page.wakeResolvers[0]();
  await page.flush();
  assert.equal(page.wakeLocks[0].releaseCount, 1, '录音结束后迟到的请求必须释放');
}

{
  const page = await openRemotePage();
  await page.start();
  await page.wakeLocks[0].release();
  assert.match(page.element('wake-lock-hint').textContent, /未允许/);
  assert.equal(page.wakeRequests, 1, '系统释放后不能循环申请');
  acknowledge(page);
  page.socket.readyState = 3;
  page.socket.onclose();
  page.documentListeners.visibilitychange();
  page.socket.onopen();
  page.socket.onmessage({ data: JSON.stringify({ type: 'auth', ok: true }) });
  assert.equal(controls(page, 'recover').at(-1).sessionId, recordingId);
  assert.equal(controls(page, 'recover').at(-1).recoveryKey, fixtureRecoveryKey);
  page.socket.onmessage({
    data: JSON.stringify({
      type: 'recovery',
      sessionId: recordingId,
      recovery: { kind: 'pending' },
    }),
  });
  assert.equal(page.element('btn-record').disabled, true);
  await page.advance(1500);
  assert.equal(controls(page, 'recover').length, 2);
  page.socket.onmessage({
    data: JSON.stringify({
      type: 'recovery',
      sessionId: recordingId,
      recovery: { kind: 'completed', text: '保留下来的两分钟录音' },
    }),
  });
  assert.equal(page.element('result-text').textContent, '保留下来的两分钟录音');
  assert.equal(page.element('btn-record').disabled, false);
}

{
  const page = await openRemotePage({ savedRecovery: recordingId });
  const oldSocket = page.socket;
  await page.advance(8000);
  assert.notEqual(page.socket, oldSocket, '半开连接恢复超时后重新认证');
  assert.equal(oldSocket.readyState, 3);
}

for (const recovery of [{ kind: 'failed', hasAudioRecording: true }, { kind: 'unavailable' }]) {
  const page = await openRemotePage({ savedRecovery: recordingId });
  page.socket.onmessage({
    data: JSON.stringify({ type: 'recovery', sessionId: recordingId, recovery }),
  });
  assert.match(page.element('status-text').textContent, /历史记录/);
  assert.equal(page.element('btn-record').disabled, false);
  if (recovery.kind === 'unavailable')
    assert.equal(page.storage.getItem('ol_remote_recovery_session'), null);
}

console.log('remote-input-audio-queue.test.mjs passed');
