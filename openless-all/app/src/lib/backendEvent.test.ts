import {
  applyTranscriptEvent,
  isTranscriptSnapshotCoherent,
  applyTranscriptSnapshot,
  shouldRetryTranscriptSnapshot,
  beginTranscriptGeneration,
  clearTranscriptText,
  createTranscriptViewState,
  type TranscriptViewState,
} from './backendEvent';

function assertState(actual: TranscriptViewState, text: string, sequence: number) {
  if (actual.text !== text || actual.sequence !== sequence) {
    throw new Error(
      `expected ${JSON.stringify({ text, sequence })}, got ${JSON.stringify(actual)}`,
    );
  }
}

let state: TranscriptViewState = createTranscriptViewState();
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

state = applyTranscriptEvent(state, {
  sequence: 8,
  sessionId: 'sv',
  kind: {
    type: 'selection_voice_state_changed',
    payload: { phase: 'recording', sessionId: 'sv' },
  },
});
assertState(state, '', 8);
state = applyTranscriptEvent(state, {
  sequence: 9,
  sessionId: 'b',
  kind: { type: 'transcript_delta', payload: { text: '旧听写', offset: 0, isFinal: true } },
});
assertState(state, '', 8);
state = applyTranscriptEvent(state, {
  sequence: 10,
  sessionId: 'sv',
  kind: { type: 'transcript_delta', payload: { text: '帮我写', offset: 0, isFinal: false } },
});
assertState(state, '帮我写', 10);

// A UI recording edge must not reset the global watermark. Until the matching
// backend start arrives, delayed events from the old session are ignored.
state = clearTranscriptText(state);
state = beginTranscriptGeneration(state);
const generation = state.generation;
const watermark = state.sequence;
state = applyTranscriptEvent(state, {
  sequence: 13,
  sessionId: 'sv',
  kind: { type: 'transcript_delta', payload: { text: '旧会话延迟', offset: 0, isFinal: false } },
});
assertState(state, '', watermark);
if (state.generation !== generation || !state.awaitingSessionStart) {
  throw new Error('new generation must retain the watermark and await its session start');
}
state = applyTranscriptEvent(state, {
  sequence: 13,
  sessionId: 'next',
  kind: { type: 'dictation_state_changed', payload: { phase: 'starting', sessionId: 'next' } },
});
state = applyTranscriptEvent(state, {
  sequence: 14,
  sessionId: 'sv',
  kind: { type: 'transcript_delta', payload: { text: '旧会话仍延迟', offset: 0, isFinal: false } },
});
assertState(state, '', 13);
state = applyTranscriptEvent(state, {
  sequence: 14,
  sessionId: 'next',
  kind: { type: 'transcript_delta', payload: { text: '新会话', offset: 0, isFinal: false } },
});
assertState(state, '新会话', 14);

// Tauri's real order is backend:event(starting) -> capsule:state(recording) ->
// backend:event(transcript_delta). The recording edge must reuse the session
// already established by the first event instead of reopening the gate.
let orderedState: TranscriptViewState = createTranscriptViewState();
orderedState = applyTranscriptEvent(orderedState, {
  sequence: 20,
  sessionId: 'ordered-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'starting', sessionId: 'ordered-session' },
  },
});
orderedState = beginTranscriptGeneration(orderedState);
if (
  orderedState.awaitingSessionStart ||
  orderedState.sessionId !== 'ordered-session' ||
  orderedState.generation !== 1
) {
  throw new Error('recording edge must reuse the session established by dictation starting');
}
orderedState = applyTranscriptEvent(orderedState, {
  sequence: 21,
  sessionId: 'ordered-session',
  kind: { type: 'transcript_delta', payload: { text: '首个字', offset: 0, isFinal: false } },
});
assertState(orderedState, '首个字', 21);
orderedState = applyTranscriptEvent(orderedState, {
  sequence: 22,
  sessionId: 'old-session',
  kind: { type: 'transcript_delta', payload: { text: '旧延迟', offset: 0, isFinal: false } },
});
assertState(orderedState, '首个字', 21);

// A delayed capsule:state(recording) can arrive after the backend has already
// announced starting and delivered the first delta. The same session must be
// reused without clearing the confirmed prefix.
let lateRecordingState: TranscriptViewState = createTranscriptViewState();
lateRecordingState = applyTranscriptEvent(lateRecordingState, {
  sequence: 30,
  sessionId: 'late-recording-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'starting', sessionId: 'late-recording-session' },
  },
});
lateRecordingState = applyTranscriptEvent(lateRecordingState, {
  sequence: 31,
  sessionId: 'late-recording-session',
  kind: { type: 'transcript_delta', payload: { text: '已确认', offset: 0, isFinal: false } },
});
lateRecordingState = beginTranscriptGeneration(lateRecordingState);
assertState(lateRecordingState, '已确认', 31);
if (lateRecordingState.awaitingSessionStart || lateRecordingState.sessionId !== 'late-recording-session') {
  throw new Error('late recording edge must preserve the already-started session transcript');
}

// The rail ready handshake can resolve after a newer live event. Its older
// snapshot must not roll that event back, while a current snapshot replays the
// complete transcript and leaves the session open for a later recording edge.
let replayState = applyTranscriptEvent(createTranscriptViewState(), {
  sequence: 40,
  sessionId: 'replay-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'starting', sessionId: 'replay-session' },
  },
});
replayState = applyTranscriptEvent(replayState, {
  sequence: 41,
  sessionId: 'replay-session',
  kind: { type: 'transcript_delta', payload: { text: '更新后的原文', offset: 0, isFinal: false } },
});
const staleReplay = applyTranscriptSnapshot(replayState, {
  transcript: '旧快照', sessionId: 'replay-session', sequence: 40, revision: 40,
});
assertState(staleReplay, '更新后的原文', 41);
const tornReplay = applyTranscriptSnapshot(replayState, {
  transcript: '撕裂帧', sessionId: 'replay-session', sequence: 42, revision: 42,
  payloadRevision: 41,
});
assertState(tornReplay, '更新后的原文', 41);
if (
  !shouldRetryTranscriptSnapshot(replayState, {
    transcript: '撕裂帧', sessionId: 'replay-session', sequence: 42, revision: 42,
    payloadRevision: 41,
  }) ||
  !isTranscriptSnapshotCoherent({
    transcript: '一致帧', sessionId: 'replay-session', sequence: 42, revision: 42,
    payloadRevision: 42,
  })
) {
  throw new Error('rail must retry a current torn snapshot and accept a coherent one');
}
// Deterministic snapshot transaction ordering:
// 1. payload commit before the backend event is already coherent;
// 2. backend event before a queued payload is rejected only temporarily;
// 3. the matching payload commit at the same revision is accepted and must
//    not permanently lose a legal first frame.
const payloadBeforeBackend = applyTranscriptSnapshot(replayState, {
  transcript: 'payload先提交', sessionId: 'replay-session', sequence: 60, revision: 60,
  payloadRevision: 60,
});
assertState(payloadBeforeBackend, 'payload先提交', 60);
const backendBeforeQueuedPayload = applyTranscriptSnapshot(payloadBeforeBackend, {
  transcript: 'backend先到', sessionId: 'replay-session', sequence: 61, revision: 61,
  payloadRevision: 60,
});
assertState(backendBeforeQueuedPayload, 'payload先提交', 60);
const queuedPayloadCommit = applyTranscriptSnapshot(backendBeforeQueuedPayload, {
  transcript: 'backend先到', sessionId: 'replay-session', sequence: 61, revision: 61,
  payloadRevision: 61,
});
assertState(queuedPayloadCommit, 'backend先到', 61);

// A Recording -> Recording state update is the first-PCM path. It must advance
// the watermark without treating the previous Recording payload as the new
// frame; the first delta after the update is still accepted for the same session.
let repeatedRecording = applyTranscriptEvent(createTranscriptViewState(), {
  sequence: 70,
  sessionId: 'pcm-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'starting', sessionId: 'pcm-session' },
  },
});
repeatedRecording = applyTranscriptEvent(repeatedRecording, {
  sequence: 71,
  sessionId: 'pcm-session',
  kind: { type: 'transcript_delta', payload: { text: '已有前缀', offset: 0, isFinal: false } },
});
repeatedRecording = applyTranscriptEvent(repeatedRecording, {
  sequence: 72,
  sessionId: 'pcm-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'recording', sessionId: 'pcm-session' },
  },
});
assertState(repeatedRecording, '已有前缀', 72);
repeatedRecording = applyTranscriptEvent(repeatedRecording, {
  sequence: 73,
  sessionId: 'pcm-session',
  kind: { type: 'transcript_delta', payload: { text: '首个 PCM 字', offset: 0, isFinal: false } },
});
assertState(repeatedRecording, '首个 PCM 字', 73);

// SessionId, rather than a Recording -> Recording state edge, is the
// generation boundary. A new session arriving on the same state must clear
// the old text, and a delayed old-session delta must remain rejected.
let sameStateNewSession = applyTranscriptEvent(createTranscriptViewState(), {
  sequence: 80,
  sessionId: 'first-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'recording', sessionId: 'first-session' },
  },
});
sameStateNewSession = applyTranscriptEvent(sameStateNewSession, {
  sequence: 81,
  sessionId: 'first-session',
  kind: { type: 'transcript_delta', payload: { text: '旧文本', offset: 0, isFinal: false } },
});
const firstGeneration = sameStateNewSession.generation;
sameStateNewSession = applyTranscriptEvent(sameStateNewSession, {
  sequence: 82,
  sessionId: 'second-session',
  kind: {
    type: 'dictation_state_changed',
    payload: { phase: 'recording', sessionId: 'second-session' },
  },
});
assertState(sameStateNewSession, '', 82);
if (
  sameStateNewSession.generation !== firstGeneration + 1 ||
  sameStateNewSession.sessionId !== 'second-session'
) {
  throw new Error('Recording -> Recording with a new sessionId must start a new transcript generation');
}
sameStateNewSession = applyTranscriptEvent(sameStateNewSession, {
  sequence: 83,
  sessionId: 'first-session',
  kind: { type: 'transcript_delta', payload: { text: '延迟旧文本', offset: 0, isFinal: false } },
});
assertState(sameStateNewSession, '', 82);
sameStateNewSession = applyTranscriptEvent(sameStateNewSession, {
  sequence: 84,
  sessionId: 'second-session',
  kind: { type: 'transcript_delta', payload: { text: '新文本', offset: 0, isFinal: false } },
});
assertState(sameStateNewSession, '新文本', 84);
const currentReplay = applyTranscriptSnapshot(replayState, {
  transcript: '完整原文回放', sessionId: 'replay-session', sequence: 42, revision: 42,
});
assertState(currentReplay, '完整原文回放', 42);
if (
  currentReplay.revision !== 42 ||
  !currentReplay.sessionStartObserved ||
  currentReplay.awaitingSessionStart
) {
  throw new Error('current rail snapshot must keep its session eligible for the recording edge');
}

// Selection-voice can mount the rail midway through its session as well. Its
// state replay must advance the same session/revision watermark so subsequent
// deltas are accepted, while a delayed dictation delta is still rejected.
let selectionReplay = applyTranscriptEvent(createTranscriptViewState(), {
  sequence: 50,
  sessionId: 'selection-session',
  kind: {
    type: 'selection_voice_state_changed',
    payload: { phase: 'recording', sessionId: 'selection-session' },
  },
});
if (
  selectionReplay.sessionId !== 'selection-session' ||
  selectionReplay.sequence !== 50 ||
  selectionReplay.revision !== 50
) {
  throw new Error('selection voice replay must advance session and revision watermarks');
}
selectionReplay = applyTranscriptSnapshot(selectionReplay, {
  transcript: '选区语音已回放',
  sessionId: 'selection-session',
  sequence: 51,
  revision: 51,
});
selectionReplay = applyTranscriptEvent(selectionReplay, {
  sequence: 52,
  sessionId: 'selection-session',
  kind: { type: 'transcript_delta', payload: { text: '选区语音后续', offset: 0, isFinal: false } },
});
assertState(selectionReplay, '选区语音后续', 52);
selectionReplay = applyTranscriptEvent(selectionReplay, {
  sequence: 53,
  sessionId: 'dictation-session',
  kind: { type: 'transcript_delta', payload: { text: '旧听写延迟', offset: 0, isFinal: false } },
});
assertState(selectionReplay, '选区语音后续', 52);

// Cloud ASR snapshots must retain all preceding words and apply corrections.
let cloudState: TranscriptViewState = createTranscriptViewState();
for (const [index, text] of ['你', '你好', '您好', '您好。', '您好。世界'].entries()) {
  cloudState = applyTranscriptEvent(cloudState, {
    sequence: index + 1, sessionId: 'cloud',
    kind: { type: 'transcript_delta', payload: { text, offset: 0, isFinal: false } },
  });
  assertState(cloudState, text, index + 1);
}
