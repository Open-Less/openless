// Host jobs outlive settings and are mirrored to the same scoped Toast API as other operations.
import { useLayoutEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { NotificationHandle } from '../../api/interaction';
import { useToast } from '../../ui/overlays/InteractionProvider';
import { useDownloadJobs } from './useDownloadJobs';
export type { ProgressItem } from './useDownloadJobs';

export function GlobalDownloadProgress() {
  const { t, i18n } = useTranslation();
  const toast = useToast('model-downloads');
  const handles = useRef(new Map<string, { handle: NotificationHandle; generation: number }>());
  const results = useRef(new Map<string, NotificationHandle>());
  const { visible, handleCancel, getActionOwner } = useDownloadJobs((item, outcome) => {
    const key = `download-result:${item.key}`;
    const entry = handles.current.get(item.key);
    if (entry?.generation === item.generation) entry.handle.dismiss();
    results.current.set(
      item.key,
      outcome === 'finished'
        ? toast.success(t('modelDownloads.ready', { name: item.name }), { key })
        : toast.warning(t('localAsr.downloadFailed'), { key, description: item.name }),
    );
  });
  const formatPercent = useMemo(
    () => new Intl.NumberFormat(i18n.resolvedLanguage, { style: 'percent' }),
    [i18n.resolvedLanguage],
  );

  useLayoutEffect(() => {
    const active = new Set(visible.map((item) => item.key));
    for (const [key, entry] of handles.current) {
      if (active.has(key)) continue;
      entry.handle.dismiss();
      handles.current.delete(key);
    }
    for (const item of visible) {
      const label = t(item.preparing ? 'modelDownloads.preparing' : 'modelDownloads.downloading', {
        name: item.name,
      });
      const percent = item.percent;
      const belowOnePercent = percent !== null && percent > 0 && percent < 1;
      const progressLabel =
        percent === null
          ? undefined
          : `${belowOnePercent ? '<' : ''}${formatPercent.format((belowOnePercent ? 1 : Math.floor(percent)) / 100)}`;
      const message = item.cancelling
        ? t('localAsr.cancellingDownload')
        : progressLabel === undefined
          ? label
          : `${label} · ${progressLabel}`;
      const options = {
        description: item.cancelFailed ? t('common.operationFailed') : undefined,
        progress: true,
        progressValue: percent ?? undefined,
        onAction: () => void handleCancel(item),
        actionLabel: t('localAsr.cancelDownload'),
        actionTitle: t('localAsr.cancelDownloadTitle', { name: item.name }),
        actionStyle: 'cancel-icon' as const,
        actionDisabled: item.cancelling || item.switching,
        actionOwner: () => getActionOwner(item),
      };
      const entry = handles.current.get(item.key);
      // Dismissing the presentation keeps its handle for this job; only a fresh job reopens it.
      if (entry?.generation === item.generation) entry.handle.update({ ...options, message });
      else {
        entry?.handle.dismiss();
        results.current.get(item.key)?.dismiss();
        results.current.delete(item.key);
        handles.current.set(item.key, {
          handle: toast.loading(message, { ...options, key: `download:${item.key}` }),
          generation: item.generation,
        });
      }
    }
  }, [toast, t, visible, handleCancel, getActionOwner, formatPercent]);
  useLayoutEffect(() => {
    const active = handles.current;
    const receipts = results.current;
    return () => {
      active.clear();
      receipts.clear();
    };
  }, []);
  return null;
}
