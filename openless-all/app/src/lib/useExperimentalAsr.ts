import { useCallback, useSyncExternalStore } from 'react';
import {
  acknowledgeExperimentalAsr,
  deleteExperimentalAsr,
  getExperimentalAsrStatus,
  setExperimentalAsrEnabled,
  type ExperimentalAsrModel,
  type ExperimentalAsrStatus,
} from './experimentalAsr';

type Snapshot = {
  models: ExperimentalAsrStatus[];
  error: string | null;
  pending: ExperimentalAsrModel[];
};
const EMPTY: Snapshot = { models: [], error: null, pending: [] };
let snapshot = EMPTY;
const listeners = new Set<() => void>();
const requests = new Set<ExperimentalAsrModel>();
let sequence = 0;
let pollGeneration = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

function update(next: Snapshot) {
  if (JSON.stringify(snapshot) === JSON.stringify(next)) return;
  const channelsBefore = snapshot.models
    .map((s) => `${s.model}:${s.phase}:${s.channelId ?? ''}`)
    .join('|');
  const channelsAfter = next.models
    .map((s) => `${s.model}:${s.phase}:${s.channelId ?? ''}`)
    .join('|');
  snapshot = next;
  for (const listener of listeners) listener();
  if (channelsBefore !== channelsAfter)
    window.dispatchEvent(new CustomEvent('ol-experimental-asr-channels-changed'));
}

async function refresh() {
  const request = ++sequence;
  try {
    const models = await getExperimentalAsrStatus();
    if (request === sequence) update({ ...snapshot, models, error: null });
  } catch (error) {
    if (request === sequence) update({ ...snapshot, error: String(error) });
  }
}

// Settings and the global download indicator share one polling job and one
// authoritative snapshot. Closing/reopening settings cannot lose a cancellation.
function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    const generation = ++pollGeneration;
    const poll = async () => {
      await refresh();
      if (generation === pollGeneration && listeners.size)
        timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      ++pollGeneration;
      clearTimeout(timer);
    }
  };
}

async function mutate(model: ExperimentalAsrModel, action: () => Promise<void>, enabled?: boolean) {
  if (requests.has(model)) return;
  requests.add(model);
  ++sequence;
  update({ ...snapshot, pending: [...requests], error: null });
  try {
    await action();
    if (enabled !== undefined) {
      // The host acknowledged the switch even if the following status read fails.
      // A successfully cancelled job must not linger as a downloading card.
      update({
        ...snapshot,
        models: snapshot.models.map((status) =>
          status.model === model
            ? { ...status, enabled, phase: enabled ? 'downloading' : 'removing', error: null }
            : status,
        ),
      });
    }
    await refresh();
  } catch (error) {
    update({ ...snapshot, error: String(error) });
    // Propagate failure to the cancel button instead of silently pretending it worked.
    throw error;
  } finally {
    requests.delete(model);
    update({ ...snapshot, pending: [...requests] });
  }
}
const toggle = (model: ExperimentalAsrModel, enabled: boolean) =>
  mutate(model, () => setExperimentalAsrEnabled(model, enabled), enabled);
const remove = (model: ExperimentalAsrModel) => mutate(model, () => deleteExperimentalAsr(model));
const acknowledge = async (ready: ExperimentalAsrStatus[]) => {
  await Promise.all(ready.map((status) => acknowledgeExperimentalAsr(status.model)));
  await refresh();
};
const getSnapshot = () => snapshot;
const getEmptySnapshot = () => EMPTY;

export function useExperimentalAsr(enabled: boolean) {
  const listen = useCallback(
    (listener: () => void) => (enabled ? subscribe(listener) : () => {}),
    [enabled],
  );
  const state = useSyncExternalStore(
    listen,
    enabled ? getSnapshot : getEmptySnapshot,
    getEmptySnapshot,
  );
  return { ...state, toggle, remove, acknowledge, refresh };
}
export type ExperimentalAsrController = ReturnType<typeof useExperimentalAsr>;
