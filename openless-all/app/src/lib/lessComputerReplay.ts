import type { LessComputerEvent, LessComputerSyncResult, LessComputerVoiceEvent } from './types';

/** Consume Core presentation snapshots. Keep the idle waterline so a late
 * meter cannot revive a finished session; an explicit snapshot may recover a
 * new owner whose starting event was already evicted from the bounded replay. */
export function reduceLessComputerVoice(
  previous: LessComputerVoiceEvent | null,
  event: LessComputerEvent,
  fromSnapshot = false,
): LessComputerVoiceEvent | null {
  if (event.kind !== 'voice_state') return previous;
  if (previous) {
    if (typeof event.seq === 'number' && typeof previous.seq === 'number' && event.seq <= previous.seq) return previous;
    if (!fromSnapshot && event.sessionId !== previous.sessionId && event.phase !== 'starting') return previous;
    if (event.sessionId === previous.sessionId && (
      previous.phase === 'idle'
      || (previous.phase === 'transcribing' && event.phase === 'recording')
    )) return previous;
  }
  return event;
}

export interface ReconciledLessComputerReplay {
  events: LessComputerEvent[];
  latestAppliedSequence: number;
  reset: boolean;
}

/**
 * Merge a bounded backend replay with events buffered while the listener was
 * being installed. Sequence-bearing duplicates are removed; sequence-less
 * legacy fallback events remain observable. A truncated replay starts from a
 * fresh waterline because the previous derived view can no longer be trusted.
 */
export function reconcileLessComputerReplay(
  appliedSequence: number,
  replay: LessComputerSyncResult,
  pending: readonly LessComputerEvent[],
): ReconciledLessComputerReplay {
  let latestAppliedSequence = replay.truncated
    ? Math.max(0, (replay.oldestSequence ?? 1) - 1)
    : appliedSequence;
  const events: LessComputerEvent[] = [];

  for (const event of [...replay.events, ...pending]) {
    if (typeof event.seq === 'number') {
      if (event.seq <= latestAppliedSequence) continue;
      latestAppliedSequence = event.seq;
    }
    events.push(event);
  }

  latestAppliedSequence = Math.max(latestAppliedSequence, replay.latestSequence);
  return { events, latestAppliedSequence, reset: replay.truncated };
}
