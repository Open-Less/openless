import type { CapsuleState } from './types';

export function capsuleTranscriptFontSize(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(20, Math.max(12, value))
    : 14;
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
