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
  /** Native event revision used to reject a rail snapshot captured before a live event. */
  revision: number;
  /** Generation increments at the UI recording edge without lowering the sequence watermark. */
  generation: number;
  /** Reject deltas until the backend announces the matching session start. */
  awaitingSessionStart: boolean;
  /** True only after the start event for the session that will own the next recording edge. */
  sessionStartObserved: boolean;
}

export function createTranscriptViewState(): TranscriptViewState {
  return {
    sessionId: null,
    sequence: 0,
    text: '',
    revision: 0,
    generation: 0,
    awaitingSessionStart: false,
    sessionStartObserved: false,
  };
}

/**
 * Start a new UI transcript generation while retaining the global event watermark.
 * A delayed delta from the previous session is rejected until a backend start event
 * supplies the new session id, so clearing text never re-opens the old-session gate.
 */
export function beginTranscriptGeneration(state: TranscriptViewState): TranscriptViewState {
  const reuseStartedSession =
    state.sessionStartObserved && state.sessionId !== null && !state.awaitingSessionStart;
  return {
    ...state,
    sessionId: reuseStartedSession ? state.sessionId : null,
    // Tauri can deliver backend:event(starting) and transcript_delta before the
    // capsule:state(recording) edge. Reusing that observed session must not
    // replay the whole transcript as a new generation or erase its prefix.
    text: reuseStartedSession ? state.text : '',
    generation: state.generation + 1,
    awaitingSessionStart: !reuseStartedSession,
    sessionStartObserved: false,
  };
}

export function clearTranscriptText(state: TranscriptViewState): TranscriptViewState {
  return { ...state, text: '', sessionStartObserved: false };
}

export interface TranscriptSnapshot {
  transcript: string;
  sessionId: string | null;
  sequence: number;
  revision: number;
  /** Native payload commit revision; absent only for legacy test/mock snapshots. */
  payloadRevision?: number;
}

/** A snapshot is replayable only when its payload and text share one revision. */
export function isTranscriptSnapshotCoherent(snapshot: TranscriptSnapshot): boolean {
  return (snapshot.payloadRevision ?? snapshot.revision) === snapshot.revision;
}

/**
 * A torn snapshot whose watermark is at least as new as the live state may
 * become coherent on the next native frame. Older torn snapshots are already
 * superseded and should be ignored without spinning the ready handshake.
 */
export function shouldRetryTranscriptSnapshot(
  state: TranscriptViewState,
  snapshot: TranscriptSnapshot,
): boolean {
  if (isTranscriptSnapshotCoherent(snapshot)) return false;
  return (
    snapshot.revision >= state.revision &&
    !(snapshot.revision === state.revision && snapshot.sequence < state.sequence)
  );
}

/** Apply a session watermark carried by capsule:state before backend:event. */
export function applyTranscriptSessionId(
  state: TranscriptViewState,
  sessionId: string | null | undefined,
): TranscriptViewState {
  if (!sessionId || sessionId === state.sessionId) return state;
  return {
    ...state,
    sessionId,
    text: '',
    generation: state.generation + 1,
    awaitingSessionStart: false,
    sessionStartObserved: true,
  };
}

/**
 * Apply the native rail replay only when its two-part watermark is not older
 * than the live event stream. Keeping this predicate beside event reduction
 * makes the ready-handshake race deterministic and directly testable.
 */
export function applyTranscriptSnapshot(
  state: TranscriptViewState,
  snapshot: TranscriptSnapshot,
): TranscriptViewState {
  const payloadRevision = snapshot.payloadRevision ?? snapshot.revision;
  // Native payload and transcript are committed by different Tauri/main-thread
  // callbacks. Never let a torn frame (new text + old idle/terminal payload, or
  // the reverse) clear/replace the rail; the next ready replay/event will carry
  // a matching commit revision.
  if (payloadRevision !== snapshot.revision) return state;
  const stale =
    snapshot.revision < state.revision ||
    (snapshot.revision === state.revision && snapshot.sequence < state.sequence);
  if (stale) return state;
  const sessionChanged =
    snapshot.sessionId !== null && snapshot.sessionId !== state.sessionId;
  return {
    ...state,
    sessionId: snapshot.sessionId,
    sequence: snapshot.sequence,
    revision: snapshot.revision,
    text: snapshot.transcript,
    generation: sessionChanged ? state.generation + 1 : state.generation,
    awaitingSessionStart: false,
    // A later capsule:state(recording) edge may still need to reuse this
    // session after the rail snapshot has replayed it.
    sessionStartObserved: snapshot.sessionId !== null,
  };
}

export function applyTranscriptEvent(
  state: TranscriptViewState,
  event: BackendEvent,
): TranscriptViewState {
  if (event.sequence <= state.sequence) return state;
  if (event.kind.type === 'dictation_state_changed') {
    const payload = event.kind.payload as { phase?: string; sessionId?: string | null } | undefined;
    const sessionId = payload?.sessionId ?? event.sessionId;
    const sessionChanged = Boolean(sessionId && sessionId !== state.sessionId);
    if (sessionChanged) {
      return {
        ...state,
        sessionId,
        sequence: event.sequence,
        revision: Math.max(state.revision, event.sequence),
        text: '',
        awaitingSessionStart: false,
        sessionStartObserved: true,
        // The normal starting -> capsule:state(recording) path increments at
        // the UI recording edge. Do not count that same generation twice;
        // Recording -> Recording with a different sessionId still increments
        // below because it has no separate start edge to rely on.
        generation: payload?.phase === 'starting' ? state.generation : state.generation + 1,
      };
    }
    return {
      ...state,
      sessionId: sessionId ?? state.sessionId,
      sequence: event.sequence,
      revision: Math.max(state.revision, event.sequence),
      awaitingSessionStart: payload?.phase === 'starting' ? false : state.awaitingSessionStart,
      sessionStartObserved: payload?.phase === 'starting' ? true : state.sessionStartObserved,
    };
  }
  if (event.kind.type === 'selection_voice_state_changed') {
    const payload = event.kind.payload as { phase?: string; sessionId?: string | null } | undefined;
    const sessionId = payload?.sessionId ?? event.sessionId;
    const sessionChanged = Boolean(sessionId && sessionId !== state.sessionId);
    if (sessionChanged) {
      return {
        ...state,
        sessionId,
        sequence: event.sequence,
        revision: Math.max(state.revision, event.sequence),
        text: '',
        awaitingSessionStart: false,
        sessionStartObserved: true,
        generation: state.generation + 1,
      };
    }
    return {
      ...state,
      sessionId: sessionId ?? state.sessionId,
      sequence: event.sequence,
      revision: Math.max(state.revision, event.sequence),
      awaitingSessionStart:
        payload?.phase === 'recording' ? false : state.awaitingSessionStart,
      sessionStartObserved:
        payload?.phase === 'recording' ? true : state.sessionStartObserved,
    };
  }
  if (event.kind.type !== 'transcript_delta') {
    return { ...state, sequence: event.sequence, revision: Math.max(state.revision, event.sequence) };
  }
  if (state.awaitingSessionStart) return state;
  if (state.sessionId !== null && event.sessionId !== state.sessionId) return state;
  const delta = event.kind.payload as TranscriptDelta | undefined;
  if (!delta || !Number.isSafeInteger(delta.offset) || delta.offset < 0) return state;
  const current = Array.from(state.text);
  if (delta.offset > current.length) return state;
  return {
    ...state,
    sessionId: event.sessionId,
    sequence: event.sequence,
    revision: Math.max(state.revision, event.sequence),
    text: current.slice(0, delta.offset).join('') + delta.text,
  };
}
