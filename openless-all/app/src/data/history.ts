import { useSyncExternalStore } from 'react';
import type { ActivityDay, DictationSession } from '../api/contracts';
import { getActivityStats, listHistory } from '../lib/ipc/history';
import { previewHistory } from '../preview/history';
import { listenHistoryChanges } from '../runtime/domain-events';
import { isPreviewHost } from '../runtime/environment';
import { SnapshotQuery } from './snapshot-query';

export type YearlyActivityApps =
  | { status: 'available'; rows: [name: string, count: number][] }
  | { status: 'unsupported' };

export function supportsActivityApps(): boolean {
  // The current native activity DTO contains counts, characters, and duration only.
  return isPreviewHost();
}

export async function getActivityApps(year: number): Promise<YearlyActivityApps> {
  return supportsActivityApps()
    ? { status: 'available', rows: previewHistory.getActivityApps(year) }
    : { status: 'unsupported' };
}

interface HistorySource {
  listHistory(): Promise<DictationSession[]>;
  getActivityStats(): Promise<ActivityDay[]>;
  getActivityApps(year: number): Promise<YearlyActivityApps>;
  listen(onChange: (revision?: number) => void): Promise<() => void>;
}

export function createHistoryStore(source: HistorySource) {
  let subscribed: Promise<() => void> | null = null;
  let writes = 0;
  let pendingWrites: Promise<void> | null = null;
  let resolveWrites: (() => void) | undefined;
  const revisions = new Set<number>();
  const years = new Map<number, SnapshotQuery<YearlyActivityApps>>();
  const invalidate = () => {
    records.invalidate();
    activity.invalidate();
    for (const query of years.values()) query.invalidate();
  };
  const start = (): Promise<() => void> => {
    subscribed ??= source.listen((revision) => {
      if (revision !== undefined) {
        // Concurrent producers can emit different revisions out of numeric order.
        if (revisions.has(revision)) return;
        revisions.add(revision);
      }
      invalidate();
    }).catch((error: unknown) => {
      subscribed = null;
      throw error;
    });
    return subscribed;
  };
  const awaitSubscription = async () => { await start(); };
  const records = new SnapshotQuery(source.listHistory, async () => {
    await awaitSubscription();
    while (pendingWrites) await pendingWrites;
  });
  const activity = new SnapshotQuery(source.getActivityStats, awaitSubscription);
  const apps = (year: number) => {
    let query = years.get(year);
    if (!query) {
      query = new SnapshotQuery(() => source.getActivityApps(year), awaitSubscription);
      years.set(year, query);
    }
    return query;
  };

  return {
    start: awaitSubscription,
    records,
    activity,
    apps,
    invalidate,
    refreshRecords: records.refresh,
    refreshActivity: activity.refresh,
    refreshApps: (year: number) => apps(year).refresh(),
    refresh: () => Promise.all([
      records.refresh(),
      activity.refresh(),
      ...Array.from(years.values(), (query) => query.refresh()),
    ]),
    async mutate<T>(command: () => Promise<T>, changesRecords: (result: T) => boolean = () => true): Promise<T> {
      if (writes++ === 0) pendingWrites = new Promise((resolve) => { resolveWrites = resolve; });
      try {
        // Pair any push emitted during the invocation with its post-commit refresh.
        // A listener failure must not substitute for the actual command result.
        await start().catch(() => undefined);
        const result = await command();
        if (changesRecords(result)) records.invalidate();
        return result;
      } finally {
        if (--writes === 0) {
          pendingWrites = null;
          resolveWrites?.();
          resolveWrites = undefined;
        }
      }
    },
  };
}

export const historyStore = createHistoryStore({
  listHistory,
  getActivityStats,
  getActivityApps,
  listen(onChange) {
    return listenHistoryChanges(onChange).then((unlisten) => {
      // Keep one native subscription for this WebView; tab switches don't leave a gap.
      if (typeof window !== 'undefined') {
        const regainFocus = () => {
          if (document.visibilityState !== 'hidden') onChange();
        };
        window.addEventListener('focus', regainFocus);
        window.addEventListener('pageshow', regainFocus);
        document.addEventListener('visibilitychange', regainFocus);
        return () => {
          unlisten();
          window.removeEventListener('focus', regainFocus);
          window.removeEventListener('pageshow', regainFocus);
          document.removeEventListener('visibilitychange', regainFocus);
        };
      }
      return unlisten;
    });
  },
});

function useQuery<T>(query: SnapshotQuery<T>) {
  return useSyncExternalStore(query.subscribe, query.getSnapshot, query.getSnapshot);
}

export function useHistoryRecords() {
  return useQuery(historyStore.records);
}

export function useActivityStats() {
  return useQuery(historyStore.activity);
}

export function useActivityApps(year: number) {
  return useQuery(historyStore.apps(year));
}
