import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

// Exercise the production lifecycle functions, with only browser/transport effects
// replaced. Deferred promises and explicit timers make late callbacks deterministic.
const source = readFileSync(new URL('../assets/remote-input/app.js', import.meta.url), 'utf8');
const names = [
  'withTimeout',
  'startRecording',
  'stopRecording',
  'cancelRecording',
  'ensureAudio',
  'buildCaptureGraph',
  'clearPendingPcm',
  'resetRemoteStreamState',
  'teardownAudioCapture',
  'teardownAudio',
  'resetAudioContext',
];
const lifecycle = names
  .map((name) => {
    const start = source.indexOf(`  function ${name}(`);
    const end = source.indexOf('\n  }', start);
    assert.ok(start >= 0 && end > start, `production function ${name} must exist`);
    return source.slice(start, end + '\n  }'.length);
  })
  .join('\n');
const settle = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
const makeStream = () => {
  const track = {
    stopped: false,
    stop() {
      this.stopped = true;
    },
  };
  return { track, getTracks: () => [track] };
};
const makeNode = () => ({
  disconnected: false,
  port: {},
  connect() {},
  disconnect() {
    this.disconnected = true;
  },
});

function harness({ worklet = false, suspended = false } = {}) {
  const calls = { mic: [], resume: [], worklet: [], nodes: [], sent: [], statuses: [], pcm: [] };
  const timers = new Map();
  let timerId = 0;
  let scriptBuilds = 0;
  class AudioContext {
    constructor() {
      this.state = suspended ? 'suspended' : 'running';
      this.sampleRate = 48000;
      this.audioWorklet = worklet ? {} : undefined;
    }
    resume() {
      const d = deferred();
      calls.resume.push(d);
      return d.promise;
    }
    suspend() {
      this.state = 'suspended';
      return Promise.resolve();
    }
    close() {
      this.state = 'closed';
      return Promise.resolve();
    }
    createMediaStreamSource() {
      return makeNode();
    }
  }
  const state = {
    recording: false,
    startSent: false,
    awaitingResult: false,
    ws: { readyState: 1 },
    audioGen: 0,
    audioCtx: null,
    mediaStream: null,
    sourceNode: null,
    workletNode: null,
    scriptNode: null,
    usingWorklet: false,
    remoteSessionId: '',
    remoteSequence: 0,
    finishAfterStarted: '',
    pendingPcm: [],
    pendingPcmBytes: 0,
    resampleState: { phase: 0, last: 0, hasLast: false },
    wakeLockHint: null,
    TARGET_SR: 16000,
    MIC_PREP_TIMEOUT_MS: 10000,
    L: { preparingMic: 'mic', preparingBackend: 'backend', micTimeout: 'timeout', ready: 'ready' },
    window: { AudioContext },
    navigator: {
      mediaDevices: {
        getUserMedia() {
          const d = deferred();
          calls.mic.push(d);
          return d.promise;
        },
      },
    },
    AudioWorkletNode: class {
      constructor() {
        const n = makeNode();
        calls.nodes.push(n);
        return n;
      }
    },
    setTimeout(fn) {
      const id = ++timerId;
      timers.set(id, fn);
      return id;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
    loadWorklet() {
      const d = deferred();
      calls.worklet.push(d);
      return d.promise;
    },
    buildScriptProcessor() {
      scriptBuilds++;
      state.scriptNode = makeNode();
    },
    wsSendJSON(message) {
      calls.sent.push(message.type);
    },
    sendAudio(pcm) {
      calls.pcm.push(pcm);
    },
    setStatus(message) {
      calls.statuses.push(message);
    },
    micErrorText(error) {
      return error.name;
    },
    interruptRecording() {
      throw new Error('unexpected interruption');
    },
  };
  for (const name of [
    'clearRecoveryTimer',
    'acquireWakeLock',
    'releaseWakeLock',
    'clearReadyTimer',
    'clearWorkTimeout',
    'updateRecordBtnUI',
    'clearResult',
    'detachHoldEnd',
    'setLevel',
    'enterTranscribing',
    'armWorkTimeout',
    'saveRecoverySession',
  ])
    state[name] = () => {};
  vm.runInNewContext(lifecycle, state);
  return {
    state,
    calls,
    timers,
    get scriptBuilds() {
      return scriptBuilds;
    },
  };
}

async function startPending(h) {
  h.state.startRecording();
  await settle();
}
async function resolveMic(h, index) {
  const stream = makeStream();
  h.calls.mic[index].resolve(stream);
  await settle();
  return stream;
}
function assertActive(h, stream, node) {
  assert.equal(h.state.recording, true, 'old callback must not stop the new recording');
  assert.equal(h.state.startSent, true);
  assert.equal(h.state.mediaStream, stream, 'old stream must not replace the new microphone');
  assert.equal(h.state.sourceNode, node);
  assert.equal(node.disconnected, false);
  assert.equal(stream.track.stopped, false);
  assert.deepEqual(h.calls.sent, ['start'], 'only the current attempt may send start');
}

for (const end of ['stopRecording', 'cancelRecording']) {
  test(`late microphone success after ${end} cannot replace a retry`, async () => {
    const h = harness();
    await startPending(h);
    h.state[end]();
    await startPending(h);
    const current = await resolveMic(h, 1);
    const node = h.state.sourceNode;
    const stale = await resolveMic(h, 0);
    assert.equal(stale.track.stopped, true, 'cancelled microphone request must release its tracks');
    assertActive(h, current, node);
  });
}

test('late microphone rejection cannot stop a retry', async () => {
  const h = harness();
  await startPending(h);
  h.state.stopRecording();
  await startPending(h);
  const current = await resolveMic(h, 1);
  const node = h.state.sourceNode;
  h.calls.mic[0].reject(Object.assign(new Error('old request'), { name: 'NotAllowedError' }));
  await settle();
  assertActive(h, current, node);
  assert.equal(h.calls.statuses.at(-1), 'backend');
});

test('old preparation timeout cannot close the current AudioContext', async () => {
  const h = harness();
  await startPending(h);
  const oldTimeout = [...h.timers.values()][0];
  h.state.stopRecording();
  await startPending(h);
  const current = await resolveMic(h, 1);
  const node = h.state.sourceNode;
  const context = h.state.audioCtx;
  oldTimeout();
  await settle();
  assertActive(h, current, node);
  assert.equal(h.state.audioCtx, context);
  assert.equal(context.state, 'running');
});

test('late resume cannot start another microphone request after cancellation', async () => {
  const h = harness({ suspended: true });
  await startPending(h);
  h.state.stopRecording();
  h.calls.resume[0].resolve();
  await settle();
  assert.equal(h.calls.mic.length, 0, 'cancelled resume must not request microphone permission');
  assert.deepEqual(h.calls.sent, []);
});

test('late resume cannot borrow a new recording and send a second start', async () => {
  const h = harness({ suspended: true });
  await startPending(h);
  h.state.stopRecording();
  await startPending(h);
  h.state.audioCtx.state = 'running';
  h.calls.resume[1].resolve();
  await settle();
  const current = await resolveMic(h, 0);
  const node = h.state.sourceNode;
  h.calls.resume[0].resolve();
  await settle();
  assertActive(h, current, node);
});

for (const outcome of ['resolve', 'reject']) {
  test(`late worklet ${outcome} cannot rebuild the current capture graph`, async () => {
    const h = harness({ worklet: true });
    await startPending(h);
    const current = await resolveMic(h, 0);
    h.state.stopRecording();
    await startPending(h);
    h.calls.worklet[1].resolve();
    await settle();
    const node = h.state.sourceNode;
    const currentWorklet = h.state.workletNode;
    h.calls.worklet[0][outcome](new Error('old module'));
    await settle();
    assertActive(h, current, node);
    assert.equal(h.state.workletNode, currentWorklet);
    assert.equal(h.calls.nodes.length, 1);
    assert.equal(h.scriptBuilds, 0, 'stale worklet failure must not start fallback capture');
    assert.equal(h.state.usingWorklet, true);
  });
}

test('current worklet failure still falls back to ScriptProcessor', async () => {
  const h = harness({ worklet: true });
  await startPending(h);
  await resolveMic(h, 0);
  h.calls.worklet[0].reject(new Error('worklet unavailable'));
  await settle();
  assert.equal(h.scriptBuilds, 1);
  assert.equal(h.state.recording, true);
  assert.deepEqual(h.calls.sent, ['start']);
});

test('current timeout still resets the context and releases a late microphone', async () => {
  const h = harness();
  await startPending(h);
  [...h.timers.values()][0]();
  await settle();
  assert.equal(h.state.recording, false);
  assert.equal(h.state.audioCtx, null);
  assert.equal(h.calls.statuses.at(-1), 'timeout');
  const stale = await resolveMic(h, 0);
  assert.equal(stale.track.stopped, true);
  assert.deepEqual(h.calls.sent, []);
});

test('normal stop still pairs start/stop and reuses the microphone on the next attempt', async () => {
  const h = harness();
  await startPending(h);
  const stream = await resolveMic(h, 0);
  h.state.remoteSessionId = 'active-session';
  h.state.stopRecording();
  assert.deepEqual(h.calls.sent, ['start', 'stop']);
  assert.equal(h.state.awaitingResult, true);
  assert.equal(stream.track.stopped, false);
  h.state.awaitingResult = false; // Backend completed the preceding recording.
  await startPending(h);
  assert.equal(h.calls.mic.length, 1);
  assert.equal(h.state.mediaStream, stream);
  assert.deepEqual(h.calls.sent, ['start', 'stop', 'start']);
});
