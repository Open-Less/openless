import type { ActivityDay, CredentialsStatus } from './types';

const CACHE_KEY = 'ol-overview-status-v1';

export interface OverviewStatusCache {
  creds: CredentialsStatus;
  activity: ActivityDay[] | null;
}

function isCreds(value: unknown): value is CredentialsStatus {
  if (!value || typeof value !== 'object') return false;
  const creds = value as Partial<CredentialsStatus>;
  return typeof creds.asrConfigured === 'boolean' && typeof creds.llmConfigured === 'boolean';
}

/** Last overview status, so a reopened window can paint before the next read. */
export function readOverviewStatusCache(): OverviewStatusCache | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<OverviewStatusCache>;
    if (!isCreds(parsed.creds)) return null;
    const activity = Array.isArray(parsed.activity) ? parsed.activity : null;
    return { creds: parsed.creds, activity };
  } catch {
    return null;
  }
}

export function writeOverviewStatusCache(next: OverviewStatusCache): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(next));
  } catch {
    /* Private mode or a full disk leaves the previous paint in place. */
  }
}

let remembered: OverviewStatusCache | null = null;

/** Merge one fresh read into the last status without dropping the other half. */
export function rememberOverviewStatus(patch: Partial<OverviewStatusCache>): void {
  const current = remembered ?? readOverviewStatusCache();
  const creds = patch.creds ?? current?.creds;
  if (!creds) return;
  const next = {
    creds,
    activity: patch.activity !== undefined ? patch.activity : (current?.activity ?? null),
  };
  remembered = next;
  writeOverviewStatusCache(next);
}

/** A history read is valid only after the row is stored. `history_changed` is
 *  published from the write that just returned. `dictation_completed` is earlier:
 *  the gesture and the transcript can both finish while the row is still absent. */
export function overviewFollowsBackendEvent(type: string | undefined): boolean {
  return type === 'history_changed';
}
