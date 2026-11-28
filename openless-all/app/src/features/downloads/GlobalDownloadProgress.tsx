// Host jobs outlive settings and are mirrored to the same scoped Toast API as other operations.
import { useLayoutEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { DownloadStatusContent, NotificationHandle } from '../../api/interaction';
import { useToast } from '../../ui/overlays/InteractionProvider';
import { downloadStatusMessage } from '../../ui/feedback/DownloadStatusText';
import { useDownloadJobs } from './useDownloadJobs';
export type { ProgressItem } from './useDownloadJobs';

export function GlobalDownloadProgress() {
  const { t, i18n } = useTranslation();
  const toast = useToast('model-downloads');
  const handles = useRef(new Map<string, { handle: NotificationHandle; generation: number }>());
  const results = useRef(new Map<string, NotificationHandle>());
  const withName = (
    phase: 'downloading' | 'preparing' | 'ready',
    name: string,
  ): DownloadStatusContent => {
    // Keep the model's slot mounted even when the wording changes around it.
    const [beforeName, afterName] = t(`modelDownloads.${phase}`, { name: '\0' }).split('\0');
    return { beforeName, modelName: name, afterName, locale: i18n.resolvedLanguage };
  };
  const { visible, handleCancel, getActionOwner } = useDownloadJobs((item, outcome) => {
    const finished = outcome === 'finished';
    const downloadStatus = finished
      ? withName('ready', item.name)
      : { beforeName: t('localAsr.downloadFailed') };
    const message = downloadStatusMessage(downloadStatus);
    const description = finished ? undefined : item.name;
    const entry = handles.current.get(item.key);
    if (entry?.generation === item.generation) {
      // Transfer the live handle before removing the job from the active projection.
      handles.current.delete(item.key);
      entry.handle.update({
        variant: finished ? 'success' : 'warning',
        message,
        description,
        downloadStatus,
        progress: false,
        progressValue: undefined,
        actionLabel: undefined,
        onAction: undefined,
      });
      results.current.set(item.key, entry.handle);
    } else {
      const options = { key: `download:${item.key}`, downloadStatus, description };
      results.current.set(
        item.key,
        finished ? toast.success(message, options) : toast.warning(message, options),
      );
    }
  });

  useLayoutEffect(() => {
    const active = new Set(visible.map((item) => item.key));
    for (const [key, entry] of handles.current) {
      if (active.has(key)) continue;
      entry.handle.dismiss();
      handles.current.delete(key);
    }
    for (const item of visible) {
      const percent = item.percent;
      const downloadStatus: DownloadStatusContent = item.cancelling
        ? { beforeName: t('localAsr.cancellingDownload') }
        : {
            ...withName(item.preparing ? 'preparing' : 'downloading', item.name),
            percent: item.preparing ? (percent ?? undefined) : percent,
          };
      const message = downloadStatusMessage(downloadStatus);
      const options = {
        description: item.cancelFailed ? t('common.operationFailed') : undefined,
        downloadStatus,
        align: 'center' as const,
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
  }, [toast, t, i18n.resolvedLanguage, visible, handleCancel, getActionOwner]);
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
