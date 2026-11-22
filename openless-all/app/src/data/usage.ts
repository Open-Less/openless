import { useSyncExternalStore } from 'react';
import { getServiceUsage } from '../lib/ipc/service-usage';
import { listenServiceUsageChanges } from '../runtime/domain-events';
import { SnapshotQuery } from './snapshot-query';

let subscribers = 0;
let subscription: Promise<() => void> | null = null;
let stopSubscription: (() => void) | undefined;
let generation = 0;
let dirty = true;
const foreground = () => document.visibilityState !== 'hidden' && document.hasFocus();
const canRead = () => subscribers > 0 && foreground();

const invalidate = () => {
  dirty = true;
  flush();
};
const flush = () => {
  if (!dirty || !canRead()) return;
  dirty = false;
  void serviceUsageQuery.refresh();
};
const subscribeBeforeRead = async () => {
  if (!subscription) {
    const current = generation;
    const pending = listenServiceUsageChanges(() => {
      if (generation === current) invalidate();
    });
    subscription = pending;
    try {
      const stop = await pending;
      if (generation !== current) {
        stop();
        if (subscribers > 0) await subscribeBeforeRead();
        return;
      }
      stopSubscription = stop;
    } catch (error) {
      if (subscription === pending) subscription = null;
      throw error;
    }
  } else {
    await subscription;
  }
};

export const serviceUsageQuery = new SnapshotQuery(
  getServiceUsage,
  subscribeBeforeRead,
  {},
  canRead,
);
export const refreshServiceUsage = serviceUsageQuery.refresh;

const subscribe = (listener: () => void) => {
  if (subscribers++ === 0) {
    document.addEventListener('visibilitychange', invalidate);
    window.addEventListener('focus', invalidate);
    dirty = true;
  }
  const stop = serviceUsageQuery.subscribe(listener);
  flush();
  return () => {
    stop();
    if (--subscribers === 0) {
      generation++;
      stopSubscription?.();
      stopSubscription = undefined;
      subscription = null;
      dirty = true;
      document.removeEventListener('visibilitychange', invalidate);
      window.removeEventListener('focus', invalidate);
    }
  };
};

export function useServiceUsage() {
  return useSyncExternalStore(subscribe, serviceUsageQuery.getSnapshot, serviceUsageQuery.getSnapshot);
}
