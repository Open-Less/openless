import { PreferencesWriteGate } from './preferencesWriteGate';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const gate = new PreferencesWriteGate<string>();
assert(gate.shouldApplyIncoming(), 'remote preference events should apply while idle');
assert(gate.receiveIncoming('idle') === 'idle', 'idle events should be returned immediately');

const finishFirst = gate.beginWrite();
assert(!gate.shouldApplyIncoming(), 'an older event must not replace an optimistic local update');
assert(
  gate.receiveIncoming('latest') === null,
  'events during a write must not replace an optimistic local update',
);

const finishSecond = gate.beginWrite();
assert(
  gate.receiveIncoming('old') === null,
  'a delayed older event must not be replayed after local writes finish',
);
assert(
  finishFirst() === false,
  'only the final queued write may apply its canonical preference payload',
);
assert(!gate.shouldApplyIncoming(), 'all queued local writes must finish before events resume');

assert(finishSecond(), 'the final queued write must apply its canonical preference payload');
assert(
  gate.shouldApplyIncoming(),
  'remote preference events should resume after local writes finish',
);

// Completion callbacks can be reached from defensive cleanup paths; they must be idempotent.
assert(finishSecond() === false, 'a completion callback must only release its event once');
assert(gate.shouldApplyIncoming(), 'finishing one write twice must not corrupt the gate');

console.log('preference write gate tests passed');
