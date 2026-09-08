import { applyTranscriptEvent, type TranscriptViewState } from './backendEvent';

function assertState(actual: TranscriptViewState, text: string, sequence: number) {
  if (actual.text !== text || actual.sequence !== sequence) {
    throw new Error(`expected ${JSON.stringify({ text, sequence })}, got ${JSON.stringify(actual)}`);
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
