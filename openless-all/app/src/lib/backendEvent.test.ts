import { applyTranscriptEvent, type TranscriptViewState } from './backendEvent';

function assertState(actual: TranscriptViewState, text: string, sequence: number) {
  if (actual.text !== text || actual.sequence !== sequence) {
    throw new Error(
      `expected ${JSON.stringify({ text, sequence })}, got ${JSON.stringify(actual)}`,
    );
  }
}

let state: TranscriptViewState = { sessionId: null, sequence: 0, text: '' };
state = applyTranscriptEvent(state, {
  sequence: 1,
  sessionId: 'a',
  kind: { type: 'transcript_delta', payload: { text: '你', offset: 0, isFinal: false } },
});
assertState(state, '你', 1);

state = applyTranscriptEvent(state, {
  sequence: 2,
  sessionId: 'a',
  kind: { type: 'transcript_delta', payload: { text: '你好🙂', offset: 0, isFinal: true } },
});
assertState(state, '你好🙂', 2);

state = applyTranscriptEvent(state, {
  sequence: 2,
  sessionId: 'a',
  kind: { type: 'transcript_delta', payload: { text: 'duplicate', offset: 0, isFinal: false } },
});
assertState(state, '你好🙂', 2);

state = applyTranscriptEvent(state, {
  sequence: 3,
  sessionId: 'old',
  kind: { type: 'transcript_delta', payload: { text: 'late', offset: 0, isFinal: false } },
});
assertState(state, '你好🙂', 2);

console.log('backendEvent.test.ts passed');

state = applyTranscriptEvent(state, {
  sequence: 4,
  sessionId: 'a',
  kind: { type: 'polish_delta', payload: { text: '润色结果', offset: 0 } },
});
assertState(state, '你好🙂', 4);
state = applyTranscriptEvent(state, {
  sequence: 5,
  sessionId: 'b',
  kind: { type: 'dictation_state_changed', payload: { phase: 'starting', sessionId: 'b' } },
});
assertState(state, '', 5);
state = applyTranscriptEvent(state, {
  sequence: 6,
  sessionId: 'a',
  kind: { type: 'transcript_delta', payload: { text: '旧会话', offset: 0, isFinal: true } },
});
assertState(state, '', 5);
state = applyTranscriptEvent(state, {
  sequence: 7,
  sessionId: 'b',
  kind: { type: 'transcript_delta', payload: { text: '新会话', offset: 0, isFinal: false } },
});
assertState(state, '新会话', 7);

// Cloud ASR snapshots must retain all preceding words and apply corrections.
let cloudState: TranscriptViewState = { sessionId: null, sequence: 0, text: '' };
for (const [index, text] of ['你', '你好', '您好', '您好。', '您好。世界'].entries()) {
  cloudState = applyTranscriptEvent(cloudState, {
    sequence: index + 1, sessionId: 'cloud',
    kind: { type: 'transcript_delta', payload: { text, offset: 0, isFinal: false } },
  });
  assertState(cloudState, text, index + 1);
}
