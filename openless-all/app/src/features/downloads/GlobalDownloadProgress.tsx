// Host jobs outlive settings and are mirrored to the same scoped Toast API as other operations.
import { useLayoutEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { NotificationHandle } from '../../api/interaction';
import { useToast } from '../../ui/overlays/InteractionProvider';
import { useDownloadJobs } from './useDownloadJobs';
export type { ProgressItem } from './useDownloadJobs';

export function GlobalDownloadProgress() {
  const { t } = useTranslation();
  const toast = useToast('model-downloads');
  const { visible, pending, errors, handleCancel } = useDownloadJobs((item) => {
    toast.error(t('localAsr.downloadFailed'), {
      key: `download-failed:${item.key}`,
      description: item.name,
    });
  });
  const handles = useRef(
    new Map<string, { handle: NotificationHandle; generation: number | undefined }>(),
  );

  useLayoutEffect(() => {
    const active = new Set(visible.map((item) => item.key));
    for (const [key, entry] of handles.current) {
      if (active.has(key)) continue;
      entry.handle.dismiss();
      handles.current.delete(key);
    }
    for (const item of visible) {
      const cancelling = pending.includes(item.key);
      const status = item.phase
        ? t(`experimentalAsr.phases.${item.phase}`)
        : t('localAsr.downloading');
      const description = cancelling
        ? t('localAsr.cancellingDownload')
        : errors[item.key]
          ? t('common.operationFailed')
          : item.percent === null
            ? status
            : `${status} · ${item.engine === 'experimental' && item.percent < 1 ? '<1' : Math.round(item.percent)}%`;
      const options = {
        description,
        progress: true,
        progressValue: item.percent ?? undefined,
        onAction: () => void handleCancel(item),
        actionLabel: t('localAsr.cancelDownload'),
        actionTitle: t('localAsr.cancelDownloadTitle', { name: item.name }),
        actionStyle: 'below' as const,
        actionDisabled: cancelling,
      };
      const entry = handles.current.get(item.key);
      // Dismissing the presentation keeps its handle for this job; only a fresh job reopens it.
      if (entry && entry.generation === item.generation) entry.handle.update(options);
      else {
        entry?.handle.dismiss();
        handles.current.set(item.key, {
          handle: toast.loading(item.name, { ...options, key: `download:${item.key}` }),
          generation: item.generation,
        });
      }
    }
  }, [toast, t, visible, pending, errors, handleCancel]);
  useLayoutEffect(() => {
    const active = handles.current;
    return () => active.clear();
  }, []);
  return null;
}
