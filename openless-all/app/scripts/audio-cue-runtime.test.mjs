import assert from 'node:assert/strict';
import { mock } from 'node:test';
import { tsImport } from 'tsx/esm/api';
const { playRecordStartCue, stopAudioCue } = await tsImport(
  '../src/lib/audioCue.ts',
  import.meta.url,
);
// Exercise the actual playback lifecycle, including promises that never resolve.
let now = 0;
mock.timers.enable({ apis: ['setTimeout'] });
mock.method(performance, 'now', () => now);
function tick(ms) {
  now += ms;
  mock.timers.tick(ms);
}
class FakeContext {
  get currentTime() {
    return this.frozen ? 0 : now / 1000;
  }
  constructor() {
    this.state = FakeContext.initialState;
    this.frozen = false;
    this.voices = 0;
    this.closeCalls = 0;
    this.resumeImpl = FakeContext.initialResumeImpl;
    this.destination = {};
    FakeContext.instances.push(this);
  }
  resume() {
    return this.resumeImpl();
  }
  close() {
    this.closeCalls++;
    this.state = 'closed';
    return Promise.resolve();
  }
  createOscillator() {
    this.voices++;
    return {
      frequency: { setValueAtTime() {} },
      connect: (gain) => gain,
      start() {},
      stop() {},
      disconnect() {},
    };
  }
  createGain() {
    return {
      gain: {
        setValueAtTime() {},
        exponentialRampToValueAtTime() {},
        cancelScheduledValues() {},
      },
      connect() {},
      disconnect() {},
    };
  }
}
FakeContext.instances = [];
FakeContext.initialState = 'running';
FakeContext.initialResumeImpl = () => new Promise(() => {});
const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: { AudioContext: FakeContext },
});
try {
  FakeContext.initialState = 'suspended';
  playRecordStartCue();
  const stuck = FakeContext.instances[0];
  let releaseOldResume;
  stuck.resumeImpl = () => new Promise((resolve) => (releaseOldResume = resolve));
  playRecordStartCue();
  FakeContext.initialState = 'running';
  tick(250);
  const healthy = FakeContext.instances[1];
  assert.equal(stuck.closeCalls, 1, 'a hung resume must close the old context');
  assert.equal(healthy.voices, 2, 'the same recording recovers its cue');
  stuck.state = 'running';
  releaseOldResume();
  await Promise.resolve();
  assert.equal(stuck.voices, 0, 'late resolution cannot play through a discarded context');
  tick(315);
  // Every request invalidates old work, including a new synchronous playback.
  healthy.state = 'suspended';
  let releaseSuperseded;
  healthy.resumeImpl = () => new Promise((resolve) => (releaseSuperseded = resolve));
  playRecordStartCue();
  healthy.state = 'running';
  playRecordStartCue();
  releaseSuperseded();
  await Promise.resolve();
  assert.equal(healthy.voices, 4, 'a superseded resume must not duplicate the newer cue');
  tick(315);
  healthy.frozen = true;
  playRecordStartCue();
  tick(315);
  const recovered = FakeContext.instances[2];
  assert.equal(healthy.closeCalls, 1, 'running with a frozen clock must recover');
  assert.equal(recovered.voices, 2);
  tick(315);
  // The original stop/time boundary survives recreation, so a late retry stays silent.
  recovered.state = 'suspended';
  playRecordStartCue();
  stopAudioCue();
  tick(1000);
  assert.equal(recovered.closeCalls, 1, 'even a dropped late cue discards its broken context');
  assert.equal(FakeContext.instances.length, 3, 'stopped recordings do not replay late');
  playRecordStartCue();
  const next = FakeContext.instances[3];
  assert.equal(next.voices, 2, 'the next recording still works after a dropped cue');
  tick(315);
  next.state = 'suspended';
  next.resumeImpl = () => {
    throw new Error('audio interrupted');
  };
  FakeContext.initialState = 'suspended';
  assert.doesNotThrow(playRecordStartCue);
  tick(250);
  assert.equal(FakeContext.instances.length, 5, 'recovery retries at most once per request');
  assert.equal(FakeContext.instances[4].closeCalls, 1, 'failed retry releases audio resources');

  // The 400ms deadline belongs to the recording, not to each resume attempt.
  playRecordStartCue();
  let releaseRetry;
  FakeContext.initialResumeImpl = () => new Promise((resolve) => (releaseRetry = resolve));
  tick(250);
  const retry = FakeContext.instances[6];
  stopAudioCue();
  tick(151);
  retry.state = 'running';
  releaseRetry();
  await Promise.resolve();
  assert.equal(retry.voices, 0, 'a recreated context cannot reset the late-cue deadline');
  playRecordStartCue();
  assert.equal(retry.voices, 2, 'dropping the old cue must not disable the next recording');
  tick(315);

  retry.state = 'suspended';
  retry.resumeImpl = () => Promise.reject(new Error('device changed'));
  FakeContext.initialState = 'running';
  playRecordStartCue();
  await Promise.resolve();
  assert.equal(retry.closeCalls, 1, 'asynchronous resume rejection also releases the context');
  assert.equal(FakeContext.instances[7].voices, 2, 'device-change recovery plays only one cue');
  tick(315);
  console.log('[audioCue.runtime.test] playback recovery assertions passed');
} finally {
  stopAudioCue();
  mock.restoreAll();
  mock.timers.reset();
  if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow);
  else Reflect.deleteProperty(globalThis, 'window');
}
