// Liked-pack identity survives a failed status or likes read. Only an explicit
// signed-out status replaces the set, so a restarted effect cannot blank hearts.

export type MarketplaceLikesEvent =
  | { type: 'signed-out' }
  | { type: 'status-failed' }
  | { type: 'fetched'; ids: readonly string[] }
  | { type: 'fetch-failed' };

export function reduceMarketplaceLikes(
  current: ReadonlySet<string>,
  event: MarketplaceLikesEvent,
): Set<string> {
  switch (event.type) {
    case 'signed-out':
      return new Set();
    case 'status-failed':
    case 'fetch-failed':
      return new Set(current);
    case 'fetched':
      return new Set(event.ids);
  }
}

const LIKES_CACHE_KEY = 'ol.marketplace.likes.v1';

export function readCachedLikes(login: string): string[] {
  if (!login || typeof sessionStorage === 'undefined') return [];
  try {
    const raw = sessionStorage.getItem(`${LIKES_CACHE_KEY}:${login}`);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((id) => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

export function writeCachedLikes(login: string, ids: Iterable<string>): void {
  if (!login || typeof sessionStorage === 'undefined') return;
  try {
    sessionStorage.setItem(`${LIKES_CACHE_KEY}:${login}`, JSON.stringify([...ids]));
  } catch {
    // Quota or a private-mode store must not drop the in-memory hearts.
  }
}
