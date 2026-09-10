export interface TranscriptDelta {
  text: string;
  offset: number;
  isFinal: boolean;
}

export interface BackendEvent {
  sequence: number;
  sessionId: string | null;
  kind: { type: string; payload?: unknown };
}

export interface TranscriptViewState {
  sessionId: string | null;
  sequence: number;
  text: string;
}

export function applyTranscriptEvent(
  state: TranscriptViewState,
  event: BackendEvent,
): TranscriptViewState {
  if (event.sequence <= state.sequence) return state;
  if (event.kind.type === 'dictation_state_changed') {
    const payload = event.kind.payload as { phase?: string; sessionId?: string | null } | undefined;
    if (payload?.phase === 'starting' && payload.sessionId) {
      return { sessionId: payload.sessionId, sequence: event.sequence, text: '' };
    }
    return { ...state, sequence: event.sequence };
  }
  if (event.kind.type !== 'transcript_delta') return { ...state, sequence: event.sequence };
  if (state.sessionId !== null && event.sessionId !== state.sessionId) return state;
  const delta = event.kind.payload as TranscriptDelta | undefined;
  if (!delta || !Number.isSafeInteger(delta.offset) || delta.offset < 0) return state;
  const current = Array.from(state.text);
  if (delta.offset > current.length) return state;
  return {
    sessionId: event.sessionId,
    sequence: event.sequence,
    text: current.slice(0, delta.offset).join('') + delta.text,
  };
}
