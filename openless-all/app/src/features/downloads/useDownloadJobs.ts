import { useLayoutEffect, useRef, useState } from 'react';
import {
  cancelFoundryLocalAsrPrepare,
  cancelLocalAsrDownload,
  cancelSherpaOnnxAsrDownload,
  type LocalAsrDownloadProgress,
} from '../../api/local-asr';
import { listenModelProgress } from '../../data/model-progress';
import { experimentalAsrModelFromDownloadId, experimentalAsrName } from '../../lib/experimentalAsr';
import { observeExperimentalAsr, toggleExperimentalAsr } from '../../lib/useExperimentalAsr';
import { getRuntimeOS, isTauriHost } from '../../runtime/environment';
import { useAppViewport } from '../../ui/layout/AppViewport';
import { useExperimentalDownloadOwners } from './experimentalDownloadOwner';
import type { ExperimentalAsrModel } from '../../lib/experimentalAsr';

type Target =
  | { engine: 'qwen3' | 'sherpa' | 'foundry'; id: string }
  | { engine: 'experimental'; id: ExperimentalAsrModel };
type Progress = Target & {
  key: string;
  name: string;
  percent: number | null;
  preparing: boolean;
};
export type ProgressItem = Progress & {
  generation: number;
  switching?: boolean;
  cancelling?: boolean;
  cancelFailed?: boolean;
};
type Outcome = 'finished' | 'failed';

function percentFromBytes(downloaded: number, total: number) {
  return total > 0 ? Math.min(100, Math.max(0, (downloaded / total) * 100)) : null;
}

export function useDownloadJobs(onResult: (item: ProgressItem, outcome: Outcome) => void) {
  const { modalStack } = useAppViewport();
  const getExperimentalOwner = useExperimentalDownloadOwners();
  const active = useRef(new Map<string, ProgressItem>());
  const generation = useRef(0);
  const [visible, setVisible] = useState<ProgressItem[]>([]);
  const reportResult = useRef(onResult);
  useLayoutEffect(() => {
    reportResult.current = onResult;
  }, [onResult]);

  useLayoutEffect(() => {
    const jobs = active.current;
    const closedDownloads = new Set<string>();
    const unlistens: Array<() => void> = [];
    let disposed = false;
    let foundry: string | undefined;
    const publish = () => setVisible([...jobs.values()]);
    const track = (progress: Progress, current = jobs.get(progress.key), switching = false) => {
      jobs.set(progress.key, {
        ...current,
        ...progress,
        generation: current?.generation ?? ++generation.current,
        switching,
      });
    };
    const finish = (key: string, outcome: Outcome | 'cancelled') => {
      const job = jobs.get(key);
      // Retire before publishing a receipt: the previous card may still be clickable.
      jobs.delete(key);
      if (job && !job.cancelling && outcome !== 'cancelled') reportResult.current(job, outcome);
    };

    if (getRuntimeOS() === 'mac') {
      unlistens.push(
        observeExperimentalAsr(({ models, pending }) => {
          for (const status of models) {
            const key = `experimental:${status.model}`;
            const preparing = status.phase === 'preparing';
            if (status.enabled && (status.phase === 'downloading' || preparing)) {
              track(
                {
                  key,
                  id: status.model,
                  name: experimentalAsrName(status.model),
                  engine: 'experimental',
                  preparing,
                  percent: preparing
                    ? null
                    : percentFromBytes(status.downloadedBytes, status.totalBytes),
                },
                jobs.get(key),
                pending.includes(status.model),
              );
            } else if (status.enabled && status.phase === 'ready' && status.channelId) {
              finish(key, 'finished');
            } else if (status.enabled && status.phase === 'failed') {
              finish(key, 'failed');
            } else {
              finish(key, 'cancelled');
            }
          }
          for (const job of jobs.values()) {
            if (job.engine === 'experimental' && !models.some((status) => status.model === job.id))
              finish(job.key, 'cancelled');
          }
          publish();
        }),
      );
    }

    if (isTauriHost()) {
      const keep = async (registration: Promise<() => void>) => {
        try {
          const off = await registration;
          if (disposed) off();
          else unlistens.push(off);
        } catch (error) {
          if (!disposed) console.warn('[global-download-progress] subscribe failed', error);
        }
      };
      const download = (engine: 'qwen3' | 'sherpa') => (p: LocalAsrDownloadProgress) => {
        // Experimental jobs finish only after installation, not at the model-store byte event.
        if (disposed || experimentalAsrModelFromDownloadId(p.modelId)) return;
        const key = `${engine}:${p.modelId}`;
        if (p.phase === 'finished' || p.phase === 'failed' || p.phase === 'cancelled') {
          closedDownloads.add(key);
          finish(key, p.phase);
        } else {
          if (p.phase === 'started') {
            jobs.delete(key);
            closedDownloads.delete(key);
          } else if (closedDownloads.has(key)) return;
          track({
            key,
            id: p.modelId,
            name: p.modelId,
            percent: percentFromBytes(p.bytesDownloaded, p.bytesTotal),
            engine,
            preparing: false,
          });
        }
        publish();
      };
      void keep(listenModelProgress('local-asr-download-progress', download('qwen3')));
      void keep(listenModelProgress('sherpa-onnx-asr-download-progress', download('sherpa')));
      void keep(
        listenModelProgress('foundry-local-asr-prepare-progress', (p) => {
          if (disposed) return;
          const key = `foundry:${p.modelAlias}`;
          if (p.phase === 'finished' || p.phase === 'failed') {
            if (foundry !== key) return;
            foundry = undefined;
            finish(key, p.phase);
          } else {
            // The host serializes Foundry work and its cancel command has no model argument.
            if (foundry && foundry !== key) finish(foundry, 'cancelled');
            foundry = key;
            track({
              key,
              id: p.modelAlias,
              name: p.modelAlias,
              percent: p.percent,
              engine: 'foundry',
              preparing: p.phase !== 'model',
            });
          }
          publish();
        }),
      );
    }
    return () => {
      disposed = true;
      for (const off of unlistens) off();
      jobs.clear();
    };
  }, []);

  const getActionOwner = (item: ProgressItem) =>
    item.engine === 'experimental' ? getExperimentalOwner(item.id) : null;

  const handleCancel = async (item: ProgressItem) => {
    const overlay = modalStack.getTopOverlay();
    if (overlay && !overlay.contains(getActionOwner(item))) return;
    const jobs = active.current;
    const current = jobs.get(item.key);
    if (current?.generation !== item.generation || current.cancelling || current.switching) return;

    const amend = (change: Pick<ProgressItem, 'cancelling' | 'cancelFailed'>) => {
      const job = jobs.get(item.key);
      if (job?.generation !== item.generation) return;
      jobs.set(item.key, { ...job, ...change });
      setVisible([...jobs.values()]);
    };
    amend({ cancelling: true, cancelFailed: false });
    try {
      if (item.engine === 'experimental') await toggleExperimentalAsr(item.id, false);
      else if (item.engine === 'qwen3') await cancelLocalAsrDownload(item.id);
      else if (item.engine === 'sherpa') await cancelSherpaOnnxAsrDownload(item.id);
      else await cancelFoundryLocalAsrPrepare();
    } catch {
      amend({ cancelling: false, cancelFailed: true });
    }
  };

  return { visible, handleCancel, getActionOwner };
}
