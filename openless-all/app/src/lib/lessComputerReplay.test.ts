import { reconcileLessComputerReplay, reduceLessComputerVoice } from './lessComputerReplay';
import type { LessComputerEvent, LessComputerSyncResult } from './types';
import contract from '../../contract/backend-2.0.json';

function assertDeepEqual(actual: unknown, expected: unknown, name: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${name}: expected ${expectedJson}, got ${actualJson}`);
  }
}

const replay: LessComputerSyncResult = {
  events: [
    { kind: 'user', text: 'question', fresh: true, seq: 4 },
    { kind: 'started', seq: 5 },
  ],
  oldestSequence: 2,
  latestSequence: 5,
  truncated: false,
};
const pending: LessComputerEvent[] = [
  { kind: 'started', seq: 5 },
  { kind: 'delta', text: 'answer', seq: 6 },
];

assertDeepEqual(
  reconcileLessComputerReplay(3, replay, pending),
  {
    events: [
      { kind: 'user', text: 'question', fresh: true, seq: 4 },
      { kind: 'started', seq: 5 },
      { kind: 'delta', text: 'answer', seq: 6 },
    ],
    latestAppliedSequence: 6,
    reset: false,
  },
  'merges pending events after replay without duplicates',
);

assertDeepEqual(
  reconcileLessComputerReplay(99, { ...replay, truncated: true }, [
    { kind: 'delta', text: 'tail without sequence' },
  ]),
  {
    events: [
      { kind: 'user', text: 'question', fresh: true, seq: 4 },
      { kind: 'started', seq: 5 },
      { kind: 'delta', text: 'tail without sequence' },
    ],
    latestAppliedSequence: 5,
    reset: true,
  },
  'rebuilds state from a truncated replay',
);

const oldRecording = {
  kind: 'voice_state',
  sessionId: 'old',
  phase: 'recording',
  level: 0.6,
  elapsedMs: 100,
  seq: 10,
} as const;
const newStarting = {
  kind: 'voice_state',
  sessionId: 'new',
  phase: 'starting',
  level: 0,
  elapsedMs: 0,
  seq: 11,
} as const;
let voice = reduceLessComputerVoice(null, oldRecording);
assertDeepEqual(voice, oldRecording, 'recording feedback is consumed from the Core event');
voice = reduceLessComputerVoice(voice, newStarting);
voice = reduceLessComputerVoice(voice, { ...oldRecording, phase: 'idle', seq: 12 });
assertDeepEqual(voice, newStarting, 'late terminal event cannot clear another session');
voice = reduceLessComputerVoice(voice, {
  ...newStarting,
  phase: 'recording',
  level: 0.4,
  elapsedMs: 50,
  seq: 13,
});
voice = reduceLessComputerVoice(voice, newStarting);
assertDeepEqual(voice?.phase, 'recording', 'replayed older phase cannot rewind live feedback');
voice = reduceLessComputerVoice(voice, { ...newStarting, phase: 'idle', seq: 14 });
voice = reduceLessComputerVoice(voice, { ...newStarting, phase: 'recording', seq: 15 });
assertDeepEqual(voice?.phase, 'idle', 'late meter cannot revive a terminal session');
assertDeepEqual(
  reduceLessComputerVoice(null, contract.lessComputerVoice.sample as LessComputerEvent),
  {
    seq: 3,
    kind: 'voice_state',
    sessionId: '00000000-0000-4000-8000-000000000000',
    phase: 'recording',
    level: 0.5,
    elapsedMs: 120,
  },
  'React consumes the same serialized Core feedback fixture',
);

const truncated: LessComputerSyncResult = {
  events: [],
  oldestSequence: 200,
  latestSequence: 250,
  truncated: true,
  voiceState: {
    kind: 'voice_state',
    sessionId: 'long-transcription',
    phase: 'transcribing',
    level: 0,
    elapsedMs: 4000,
    seq: 100,
  },
};
const restored = reconcileLessComputerReplay(0, truncated, []);
let restoredVoice = restored.reset ? null : voice;
if (truncated.voiceState)
  restoredVoice = reduceLessComputerVoice(restoredVoice, truncated.voiceState, true);
for (const event of restored.events) restoredVoice = reduceLessComputerVoice(restoredVoice, event);
assertDeepEqual(
  restoredVoice?.phase,
  'transcribing',
  'an evicted phase snapshot restores a long transcription after reload',
);
assertDeepEqual(
  restored.latestAppliedSequence,
  250,
  'restoring an older projection does not rewind the conversation waterline',
);

console.log('lessComputerReplay.test.ts passed');
