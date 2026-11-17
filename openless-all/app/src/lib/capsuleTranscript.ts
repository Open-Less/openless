import type { CapsuleState } from './types';

export function capsuleTranscriptFontSize(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(20, Math.max(12, value))
    : 14;
}

/**
 * Siri enhanced shows its band for the whole dictation, not only while there is transcript text:
 * it rises out of the wave as soon as the microphone is ready (the orb leaves its warming form and
 * opens into the wave), before any words.
 */
export function capsuleTranscriptBandPresent(
  enabled: boolean,
  state: CapsuleState,
  selectionPolish: boolean,
  warming: boolean,
): boolean {
  if (!enabled || selectionPolish) return false;
  if (state === 'recording') return !warming;
  return state === 'transcribing' || state === 'polishing';
}

export function visibleCapsuleTranscript(
  text: string,
  enabled: boolean,
  state: CapsuleState,
  selectionPolish: boolean,
): string {
  return enabled && !selectionPolish && ['recording', 'transcribing', 'polishing'].includes(state)
    ? text.trim()
    : '';
}
