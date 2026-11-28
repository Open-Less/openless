import type { CapsulePayload } from '../../lib/types';

export const REFERENCE_NOTICE_MS = 4_000;
type ReferenceNotice = NonNullable<CapsulePayload['referenceNotice']>;

export interface ReferenceBadgeState {
  sessionId: CapsulePayload['sessionId'];
  count: number;
  notice: ReferenceNotice | null;
  noticeSince: number;
  incomingNotice: ReferenceNotice | null;
}

export function updateReferenceBadge(
  previous: ReferenceBadgeState | null,
  payload: Pick<CapsulePayload, 'state' | 'sessionId' | 'referenceCount'> & { referenceNotice?: string | null },
  now: number,
): ReferenceBadgeState | null {
  if (payload.state !== 'recording' && payload.state !== 'transcribing' && payload.state !== 'polishing') {
    return null;
  }

  const sessionId = payload.sessionId ?? undefined;
  const current = previous?.sessionId === sessionId ? previous : null;
  const { referenceNotice } = payload;
  const incomingNotice = referenceNotice === 'tooLarge' || referenceNotice === 'captureFailed'
    ? referenceNotice
    : null;
  const count = payload.referenceCount ?? 0;
  if (!current?.notice && !incomingNotice && count === 0) return null;

  const newNotice = incomingNotice && incomingNotice !== current?.incomingNotice &&
    (!current || incomingNotice !== current.notice || now - current.noticeSince >= REFERENCE_NOTICE_MS);
  return {
    sessionId,
    count,
    incomingNotice,
    notice: newNotice ? incomingNotice : current?.notice ?? null,
    noticeSince: newNotice ? now : current?.noticeSince ?? now,
  };
}

export function referenceBadgeView(badge: ReferenceBadgeState | null, now: number) {
  if (!badge) return null;
  const notice = now - badge.noticeSince < REFERENCE_NOTICE_MS ? badge.notice : null;
  if (notice) return { kind: 'notice', notice } as const;
  return badge.count > 0 ? { kind: 'count', count: badge.count } as const : null;
}
