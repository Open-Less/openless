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
const latestContext = () => FakeContext.instances.at(-1);
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: { AudioContext: FakeContext },
});
try {
  stopAudioCue();
  disposeAudioCue();
  assert.equal(FakeContext.instances.length, 0, 'idle cleanup never opens audio output');

  playRecordStartCue();
  const natural = latestContext();
  natural.oscillators[0].onended();
  assert.equal(natural.closeCalls, 0, 'keep the output open for the second tone');
  natural.oscillators[1].onended();
  tick(315);
  assert.equal(natural.closeCalls, 1, 'the last tone closes output and cancels the fallback');

  playRecordStartCue();
  const stopped = latestContext();
  const oldOscillators = [...stopped.oscillators];
  stopAudioCue();
  tick(10);
  playRecordStartCue();
  assert.equal(latestContext(), stopped, 'a quick new cue can reuse the releasing context');
  for (const oscillator of oldOscillators) oscillator.onended();
  tick(30);
  assert.equal(stopped.closeCalls, 0, 'old callbacks cannot close a newer cue');
  stopAudioCue();
  tick(30);
  assert.equal(stopped.closeCalls, 1, 'ending recording releases the output after the tail');

  FakeContext.initialState = 'suspended';
  playRecordStartCue();
  const stuck = latestContext();
  let releaseOldResume;
  stuck.resumeImpl = () => new Promise((resolve) => (releaseOldResume = resolve));
  playRecordStartCue();
  FakeContext.initialState = 'running';
  tick(250);
  const healthy = latestContext();
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
  tick(315);
  assert.equal(healthy.closeCalls, 1, 'missing onended still releases the output');

  playRecordStartCue();
  const frozen = latestContext();
  frozen.frozen = true;
  tick(315);
  const recovered = latestContext();
  assert.equal(frozen.closeCalls, 1, 'running with a frozen clock must recover');
  assert.equal(recovered.voices, 2);
  tick(315);

  // The 400ms deadline belongs to the recording, not to each resume attempt.
  FakeContext.initialState = 'suspended';
  FakeContext.initialResumeImpl = () => new Promise(() => {});
  playRecordStartCue();
  const beforeRetry = latestContext();
  let releaseRetry;
  FakeContext.initialResumeImpl = () => new Promise((resolve) => (releaseRetry = resolve));
  tick(250);
  const retry = latestContext();
  assert.notEqual(retry, beforeRetry);
  stopAudioCue();
  tick(151);
  retry.state = 'running';
  releaseRetry();
  await Promise.resolve();
  assert.equal(retry.voices, 0, 'a recreated context cannot reset the late-cue deadline');
  assert.equal(retry.closeCalls, 1, 'a late successful resume still releases the output');

  let releaseQuick;
  FakeContext.initialResumeImpl = () => new Promise((resolve) => (releaseQuick = resolve));
  playRecordStartCue();
  const quick = latestContext();
  stopAudioCue();
  tick(50);
  quick.state = 'running';
  releaseQuick();
  await Promise.resolve();
  assert.equal(quick.voices, 2, 'a quick recording keeps its deferred feedback');
  tick(315);

  playRecordStartCue();
  const disabled = latestContext();
  const beforeDisable = FakeContext.instances.length;
  disposeAudioCue();
  assert.equal(disabled.closeCalls, 1, 'disabling closes a pending context immediately');
  disabled.state = 'running';
  releaseQuick();
  await Promise.resolve();
  tick(500);
  assert.equal(disabled.voices, 0, 'a late resume cannot play after disable');
  assert.equal(disabled.state, 'closed', 'a late resume cannot reopen the discarded output');
  assert.equal(FakeContext.instances.length, beforeDisable, 'disable also cancels recovery');

  FakeContext.initialResumeImpl = () => Promise.reject(new Error('device changed'));
  playRecordStartCue();
  const rejected = latestContext();
  FakeContext.initialState = 'running';
  await Promise.resolve();
  assert.equal(rejected.closeCalls, 1, 'asynchronous resume rejection releases the context');
  assert.equal(latestContext().voices, 2, 'device-change recovery plays one cue');
  tick(315);

  FakeContext.initialState = 'suspended';
  FakeContext.initialResumeImpl = () => {
    throw new Error('audio interrupted');
  };
  const beforeFailure = FakeContext.instances.length;
  assert.doesNotThrow(playRecordStartCue);
  assert.equal(FakeContext.instances.length, beforeFailure + 2, 'retry at most once per request');
  assert.equal(latestContext().closeCalls, 1, 'failed retry releases audio resources');
  console.log('[audioCue.runtime.test] playback recovery and output release assertions passed');
} finally {
  disposeAudioCue();
  mock.restoreAll();
  mock.timers.reset();
  if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow);
  else Reflect.deleteProperty(globalThis, 'window');
}
