import assert from 'node:assert/strict';
import { mock } from 'node:test';
import { tsImport } from 'tsx/esm/api';
const { disposeAudioCue, playRecordStartCue, stopAudioCue } = await tsImport(
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
    this.oscillators = [];
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
    const oscillator = {
      frequency: { setValueAtTime() {} },
      connect: (gain) => gain,
      start() {},
      stop() {},
      disconnect() {},
    };
    this.oscillators.push(oscillator);
    return oscillator;
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
  healthy.frozen = true;
  playRecordStartCue();
  tick(315);
  const recovered = FakeContext.instances[2];
  assert.equal(healthy.closeCalls, 1, 'running with a frozen clock must recover');
  assert.equal(recovered.voices, 2);
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
  assert.equal(retry.closeCalls, 1, 'a late successful resume must release its silent output');
  FakeContext.initialState = 'running';
  playRecordStartCue();
  const fresh = FakeContext.instances[7];
  assert.equal(fresh.voices, 2, 'dropping the old cue must not disable the next recording');

  fresh.state = 'suspended';
  fresh.resumeImpl = () => Promise.reject(new Error('device changed'));
  FakeContext.initialState = 'running';
  playRecordStartCue();
  await Promise.resolve();
  assert.equal(fresh.closeCalls, 1, 'asynchronous resume rejection also releases the context');
  const finalCue = FakeContext.instances[8];
  assert.equal(finalCue.voices, 2, 'device-change recovery plays only one cue');
  tick(315);
  assert.equal(finalCue.closeCalls, 1, 'natural cue completion releases the output device');

  // No startup priming: import/disposal does not create a context. Every new preview or
  // recording can still play after the previous context was closed.
  const idleCount = FakeContext.instances.length;
  disposeAudioCue();
  tick(1000);
  assert.equal(FakeContext.instances.length, idleCount, 'idle cleanup never creates audio');
  playRecordStartCue();
  const stopped = FakeContext.instances.at(-1);
  assert.equal(stopped.voices, 2);
  stopAudioCue();
  tick(30);
  assert.equal(stopped.closeCalls, 1, 'stopping a playing cue releases output after its ramp');

  playRecordStartCue();
  const rapid = FakeContext.instances.at(-1);
  stopAudioCue();
  tick(10);
  playRecordStartCue();
  tick(20);
  assert.equal(rapid.closeCalls, 0, 'old stop cleanup cannot close a newer cue');
  tick(295);
  assert.equal(rapid.closeCalls, 1, 'the newer cue also releases output when finished');

  // Disabling the preference / unmounting is stronger than a recording stop: a pending
  // resume must never revive audio or start a recovery retry after disposal.
  let releaseDisposed;
  FakeContext.initialState = 'suspended';
  FakeContext.initialResumeImpl = () => new Promise((resolve) => (releaseDisposed = resolve));
  playRecordStartCue();
  const disposed = FakeContext.instances.at(-1);
  disposeAudioCue();
  const disposedCount = FakeContext.instances.length;
  disposed.state = 'running';
  releaseDisposed();
  await Promise.resolve();
  tick(1000);
  assert.equal(disposed.closeCalls, 1);
  assert.equal(disposed.voices, 0, 'disabled/unmounted listeners cannot play a late cue');
  assert.equal(FakeContext.instances.length, disposedCount, 'disposal cancels recovery timers');

  // A quick recording still gets feedback when resume settles within the original deadline.
  let releaseQuick;
  FakeContext.initialResumeImpl = () => new Promise((resolve) => (releaseQuick = resolve));
  playRecordStartCue();
  const quick = FakeContext.instances.at(-1);
  stopAudioCue();
  tick(100);
  quick.state = 'running';
  releaseQuick();
  await Promise.resolve();
  assert.equal(quick.voices, 2, 'quick recordings keep start feedback without startup priming');
  tick(315);
  assert.equal(quick.closeCalls, 1);

  FakeContext.initialState = 'running';
  playRecordStartCue();
  const ended = FakeContext.instances.at(-1);
  ended.oscillators[0].onended();
  assert.equal(ended.closeCalls, 0, 'one ending tone cannot cut off the remaining tone');
  ended.oscillators[1].onended();
  assert.equal(ended.closeCalls, 1, 'the final tone releases output without waiting for a timer');

  playRecordStartCue();
  const superseded = FakeContext.instances.at(-1);
  const oldVoices = [...superseded.oscillators];
  playRecordStartCue();
  for (const voice of oldVoices) voice.onended();
  assert.equal(superseded.closeCalls, 0, 'old onended callbacks cannot close a newer cue');
  tick(315);
  assert.equal(superseded.closeCalls, 1);

  playRecordStartCue();
  const disabledWhilePlaying = FakeContext.instances.at(-1);
  disposeAudioCue();
  assert.equal(disabledWhilePlaying.closeCalls, 1, 'disabling an active cue releases immediately');
  tick(1000);
  assert.equal(disabledWhilePlaying.closeCalls, 1, 'stale timers cannot dispose twice');

  const originalCreateOscillator = FakeContext.prototype.createOscillator;
  FakeContext.prototype.createOscillator = () => {
    throw new Error('audio node creation failed');
  };
  try {
    assert.doesNotThrow(playRecordStartCue);
    const failedSynthesis = FakeContext.instances.at(-1);
    tick(315);
    assert.equal(failedSynthesis.closeCalls, 1, 'failed synthesis cannot leak a silent output');
  } finally {
    FakeContext.prototype.createOscillator = originalCreateOscillator;
  }
  console.log('[audioCue.runtime.test] playback recovery assertions passed');
} finally {
  disposeAudioCue();
  mock.restoreAll();
  mock.timers.reset();
  if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow);
  else Reflect.deleteProperty(globalThis, 'window');
}
