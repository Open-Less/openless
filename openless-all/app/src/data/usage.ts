import { useSyncExternalStore } from 'react';
import { getServiceUsage } from '../lib/ipc/service-usage';
import { SnapshotQuery } from './snapshot-query';

export const serviceUsageQuery = new SnapshotQuery(getServiceUsage, async () => {});
export const refreshServiceUsage = serviceUsageQuery.refresh;

export function useServiceUsage() {
  return useSyncExternalStore(
    serviceUsageQuery.subscribe,
    serviceUsageQuery.getSnapshot,
    serviceUsageQuery.getSnapshot,
  );
}
