import { useEffect, useSyncExternalStore } from 'react';
import { getServiceUsage } from '../lib/ipc/service-usage';
import { SnapshotQuery } from './snapshot-query';

export const serviceUsageQuery = new SnapshotQuery(getServiceUsage, async () => {});
export const refreshServiceUsage = serviceUsageQuery.refresh;

export function useServiceUsage() {
  useEffect(() => {
    const refresh = () => {
      if (document.visibilityState !== 'hidden') void refreshServiceUsage();
    };
    refresh();
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, []);
  return useSyncExternalStore(
    serviceUsageQuery.subscribe,
    serviceUsageQuery.getSnapshot,
    serviceUsageQuery.getSnapshot,
  );
}
