import type { ActivityDay, CredentialsStatus } from '../api/contracts';

const CACHE_KEY = 'ol-overview-status-v1';

interface OverviewStatusCache {
  creds: CredentialsStatus | null;
  activity: ActivityDay[] | null;
}

function isCreds(value: unknown): value is CredentialsStatus {
  if (!value || typeof value !== 'object') return false;
  const creds = value as Partial<CredentialsStatus>;
  return typeof creds.asrConfigured === 'boolean' && typeof creds.llmConfigured === 'boolean';
}

function isActivityDay(value: unknown): value is ActivityDay {
  if (!value || typeof value !== 'object') return false;
  const day = value as Partial<ActivityDay>;
  return typeof day.date === 'string' && typeof day.count === 'number';
}

/** Reopened native windows can paint the last confirmed status before IPC is ready. */
export function readOverviewStatusCache(): OverviewStatusCache | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<OverviewStatusCache> | null;
    if (!parsed) return null;
    const creds = isCreds(parsed.creds) ? parsed.creds : null;
    const activity =
      Array.isArray(parsed.activity) && parsed.activity.every(isActivityDay)
        ? parsed.activity
        : null;
    return { creds, activity };
  } catch {
    return null;
  }
}

/** Each successful read replaces only its own part of the last confirmed status. */
export function rememberOverviewStatus(patch: Partial<OverviewStatusCache>): void {
  try {
    const current = readOverviewStatusCache();
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({
        creds: current?.creds ?? null,
        activity: current?.activity ?? null,
        ...patch,
      }),
    );
  } catch {
    // A full disk or blocked storage must not turn a successful IPC read into an error.
  }
}
