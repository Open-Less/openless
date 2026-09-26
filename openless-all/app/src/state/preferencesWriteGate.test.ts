import { PreferencesWriteGate } from './preferencesWriteGate';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const gate = new PreferencesWriteGate<string>();
assert(gate.shouldApplyIncoming(), 'remote preference events should apply while idle');
const idleIncoming = gate.receiveIncoming('idle');
assert(idleIncoming.value === 'idle', 'idle events should be returned immediately');
assert(!idleIncoming.isOwnWrite, 'an unrelated idle event is not a local echo');

const finishFirst = gate.beginWrite('first');
assert(!gate.shouldApplyIncoming(), 'an older event must not replace an optimistic local update');
const externalIncoming = gate.receiveIncoming('latest');
assert(externalIncoming.wasPending, 'events during a write must be marked as pending');
assert(!externalIncoming.isOwnWrite, 'external events must not be discarded as local echoes');
const ownIncoming = gate.receiveIncoming('first');
assert(ownIncoming.isOwnWrite, 'the write payload must be recognized as its own echo');

const finishSecond = gate.beginWrite('second');
assert(!gate.receiveIncoming('old').isOwnWrite, 'unrelated queued events must remain visible to the caller');
assert(finishFirst('canonical-first') === false, 'only the final queued write may apply its canonical preference payload');
assert(!gate.shouldApplyIncoming(), 'all queued local writes must finish before events resume');

assert(finishSecond('canonical-second'), 'the final queued write must apply its canonical preference payload');
assert(gate.shouldApplyIncoming(), 'remote preference events should resume after local writes finish');
assert(gate.receiveIncoming('canonical-second').isOwnWrite, 'delayed canonical echoes must be recognized');

const objectGate = new PreferencesWriteGate<{ value: string }>(
  (left, right) => left.value === right.value,
);
const finishObjectWrite = objectGate.beginWrite({ value: 'saved' });
assert(
  objectGate.receiveIncoming({ value: 'saved' }).isOwnWrite,
  'structurally equal event payloads must be recognized as local echoes',
);
finishObjectWrite();

// Completion callbacks can be reached from defensive cleanup paths; they must be idempotent.
assert(finishSecond() === false, 'a completion callback must only release its event once');
assert(gate.shouldApplyIncoming(), 'finishing one write twice must not corrupt the gate');

console.log('preference write gate tests passed');
