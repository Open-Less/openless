import type { DictationSession, StylePack } from './types';

/**
 * Style pack id used by "retry with the original style".
 *
 * Prefers the pack that produced the record (session.stylePackId) — the retry is an A/B
 * comparison against the previous result, so it must use the same style, otherwise model
 * jitter can't be told apart from a style difference. Returns undefined when the pack was
 * deleted, the old history has no stylePackId, or the top-level pack list hasn't loaded
 * (allPacks is null); the caller then falls back to the active pack (repolish's behavior
 * when stylePackId is omitted).
 *
 * Note the lookup checks allPacks (including disabled packs): history may come from a pack
 * disabled later on; retry works as long as the pack still exists.
 */
export function resolveRepolishRetryPackId(
  session: Pick<DictationSession, 'stylePackId'>,
  allPacks: StylePack[] | null,
): string | undefined {
  if (!session.stylePackId || !allPacks) return undefined;
  return allPacks.some((pack) => pack.id === session.stylePackId) ? session.stylePackId : undefined;
}

// History shares the card label policy, including user-renamed builtins.
export { stylePackDisplayName as packDisplayName } from './stylePackPresentation';

/** Default selection for the "change style" dropdown: the active pack first, else the first
    available pack; '' for an empty list. */
export function defaultPackId(packs: StylePack[]): string {
  return packs.find((pack) => pack.active)?.id || packs[0]?.id || '';
}

/**
 * The style pack id "retry with the original style" actually uses: prefers the original
 * pack that produced the record; when it was deleted, the old history has no stylePackId,
 * or the pack list hasn't loaded, fall back explicitly to the active pack (else the first
 * available) — passing the id explicitly keeps the frontend's labeling consistent with
 * actual execution instead of relying on the backend's None fallback chain.
 */
export function resolveRepolishRetryPackIdWithFallback(
  session: Pick<DictationSession, 'stylePackId'>,
  allPacks: StylePack[] | null,
  enabledPacks: StylePack[],
): string | undefined {
  return (
    (resolveRepolishRetryPackId(session, allPacks) ?? defaultPackId(enabledPacks)) || undefined
  );
}
