import { useEffect, useRef, useState } from 'react';
import { getRuntimeOS, isTauriHost } from '../../runtime/environment';
import {
  cancelFoundryLocalAsrPrepare,
  cancelLocalAsrDownload,
  cancelSherpaOnnxAsrDownload,
  type FoundryPrepareProgress,
  type LocalAsrDownloadProgress,
} from '../../api/local-asr';
import {
  downloadPercent,
  experimentalAsrModelFromDownloadId,
  experimentalAsrName,
  type ExperimentalAsrModel,
} from '../../lib/experimentalAsr';
import { useExperimentalAsr } from '../../lib/useExperimentalAsr';

type Engine = 'qwen3' | 'sherpa' | 'foundry' | 'experimental';
export interface ProgressItem {
  key: string;
  id: string;
  name: string;
  percent: number | null;
  engine: Engine;
  model?: ExperimentalAsrModel;
  phase?: string;
}
const TERMINAL = new Set(['finished', 'cancelled', 'failed']);

export function useDownloadJobs() {
  const experimental = useExperimentalAsr(getRuntimeOS() === 'mac');
  const [items, setItems] = useState<Record<string, ProgressItem>>({});
  const [pending, setPending] = useState<string[]>([]);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const cancelledJobs = useRef(new Set<string>());
  const generations = useRef(new Map<string, number>());
  const cancelling = useRef(new Map<string, symbol>());

  useEffect(() => {
    if (!isTauriHost()) return;
    const unlistens: Array<() => void> = [];
    let cancelled = false;
    const update = (item: ProgressItem, phase: string) => {
      if (cancelled) return;
      if (phase === 'started') {
        generations.current.set(item.key, (generations.current.get(item.key) ?? 0) + 1);
        if (cancelling.current.delete(item.key)) {
          setPending((previous) => previous.filter((key) => key !== item.key));
        }
      }
      if (phase === 'started' || TERMINAL.has(phase)) cancelledJobs.current.delete(item.key);
      setItems((previous) => {
        if (TERMINAL.has(phase)) {
          const next = { ...previous };
          delete next[item.key];
          return next;
        }
        if (cancelledJobs.current.has(item.key)) return previous;
        return { ...previous, [item.key]: item };
      });
    };
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const subscribe = async <T>(event: string, handler: (payload: T) => void) => {
        const off = await listen<T>(event, ({ payload }) => {
          if (!cancelled) handler(payload);
        });
        if (cancelled) off();
        else unlistens.push(off);
      };
      const download = (engine: 'qwen3' | 'sherpa') => (p: LocalAsrDownloadProgress) => {
        // Generic model-store progress also includes experiments. Their IDs are
        // not legacy model IDs and must never reach cancelLocalAsrDownload().
        if (experimentalAsrModelFromDownloadId(p.modelId)) return;
        update(
          {
            key: `${engine}:${p.modelId}`,
            id: p.modelId,
            name: p.modelId,
            percent: p.bytesTotal > 0 ? (p.bytesDownloaded / p.bytesTotal) * 100 : null,
            engine,
          },
          p.phase,
        );
      };
      const results = await Promise.allSettled([
        subscribe<LocalAsrDownloadProgress>('local-asr-download-progress', download('qwen3')),
        subscribe<LocalAsrDownloadProgress>(
          'sherpa-onnx-asr-download-progress',
          download('sherpa'),
        ),
        subscribe<FoundryPrepareProgress>('foundry-local-asr-prepare-progress', (p) => {
          update(
            {
              key: `foundry:${p.modelAlias}`,
              id: p.modelAlias,
              name: p.label || p.modelAlias,
              percent: p.percent ?? null,
              engine: 'foundry',
            },
            p.phase,
          );
        }),
      ]);
      for (const result of results)
        if (result.status === 'rejected')
          console.warn('[global-download-progress] subscribe failed', result.reason);
    })().catch((error) => console.warn('[global-download-progress] subscribe failed', error));
    return () => {
      cancelled = true;
      for (const off of unlistens) off();
    };
  }, []);

  const handleCancel = async (item: ProgressItem) => {
    if (cancelling.current.has(item.key)) return;
    const token = Symbol(item.key);
    cancelling.current.set(item.key, token);
    const generation = generations.current.get(item.key);
    const isCurrent = () =>
      cancelling.current.get(item.key) === token &&
      generations.current.get(item.key) === generation;
    setPending((previous) => [...previous, item.key]);
    setErrors((previous) => {
      const next = { ...previous };
      delete next[item.key];
      return next;
    });
    // Generic/Sherpa events have a Started boundary to admit a later retry.
    // Foundry has no such event; keep its card until the native terminal event
    // rather than fencing all future prepares for the same model alias.
    if (item.engine === 'qwen3' || item.engine === 'sherpa') cancelledJobs.current.add(item.key);
    try {
      if (item.engine === 'experimental' && item.model)
        await experimental.toggle(item.model, false);
      else if (item.engine === 'qwen3') await cancelLocalAsrDownload(item.id);
      else if (item.engine === 'sherpa') await cancelSherpaOnnxAsrDownload(item.id);
      else await cancelFoundryLocalAsrPrepare();
      if (isCurrent() && item.engine !== 'foundry') {
        setItems((previous) => {
          const next = { ...previous };
          delete next[item.key];
          return next;
        });
      }
    } catch (error) {
      if (!isCurrent()) return;
      cancelledJobs.current.delete(item.key);
      setErrors((previous) => ({ ...previous, [item.key]: String(error) }));
    } finally {
      if (isCurrent()) {
        cancelling.current.delete(item.key);
        setPending((previous) => previous.filter((key) => key !== item.key));
      }
    }
  };
  const visible: ProgressItem[] = [
    ...Object.values(items),
    ...experimental.models
      .filter(
        (status) =>
          status.enabled && (status.phase === 'downloading' || status.phase === 'preparing'),
      )
      .map((status): ProgressItem => ({
        key: `experimental:${status.model}`,
        id: status.model,
        model: status.model,
        name: experimentalAsrName(status.model),
        engine: 'experimental',
        phase: status.phase,
        percent:
          status.phase === 'downloading' && status.downloadedBytes > 0
            ? downloadPercent(status)
            : null,
      })),
  ];

  return {
    visible,
    errors,
    pending: [...pending, ...experimental.pending.map((model) => `experimental:${model}`)],
    handleCancel,
  };
}
