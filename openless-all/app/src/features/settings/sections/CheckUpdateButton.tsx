// Callers select a release channel explicitly; transport selection stays within that channel.

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../../components/Icon';
import { UpdateDialog } from '../../updates/UpdateDialog';
import { useAutoUpdate } from '../../updates/useAutoUpdate';
import { useInteraction } from '../../../ui/overlays/InteractionProvider';
import type { UpdateChannel } from '../../../api/contracts';
import './check-update-button.css';

export function CheckUpdateButton({
  channel,
  compact = false,
  variant = 'outlined',
  autoCheckChannel,
}: {
  channel: UpdateChannel;
  compact?: boolean;
  variant?: 'outlined' | 'flat';
  autoCheckChannel?: UpdateChannel | null;
}) {
  const { t } = useTranslation();
  const updater = useAutoUpdate();
  const { status, checking, busy, errorMessage, dismissDialog, checkForUpdates } = updater;
  const { notify } = useInteraction('update-check');

  useEffect(() => {
    if (status !== 'none' && status !== 'error') return;
    notify({
      key: 'update-check-result',
      state: status === 'error' ? 'failed' : 'saved',
      message:
        status === 'error'
          ? (errorMessage ?? t('settings.about.updateError'))
          : t('settings.about.upToDate'),
      durationMs: status === 'error' ? 5000 : undefined,
    });
    const timer = window.setTimeout(() => void dismissDialog(), 2500);
    return () => window.clearTimeout(timer);
  }, [status, errorMessage, dismissDialog, notify, t]);

  const upToDate = status === 'none';
  const failed = status === 'error';
  const iconName = upToDate ? 'check' : 'refresh';
  const labelKey =
    channel === 'beta'
      ? 'settings.about.checkBetaUpdateBtn'
      : 'settings.about.checkStableUpdateBtn';
  const label = checking ? t('settings.about.checkingUpdate') : t(labelKey);

  useEffect(() => {
    if (autoCheckChannel) void checkForUpdates(autoCheckChannel);
  }, [autoCheckChannel, checkForUpdates]);

  return (
    <>
      <button
        type="button"
        className="ol-check-update-button"
        data-variant={variant}
        data-compact={compact || undefined}
        data-result={upToDate ? 'current' : failed ? 'error' : undefined}
        onClick={() => void updater.checkForUpdates(channel)}
        disabled={checking || busy}
        aria-label={compact ? label : undefined}
        title={
          failed
            ? (updater.errorMessage ?? t('settings.about.updateError'))
            : upToDate
              ? t('settings.about.upToDate')
              : compact
                ? label
                : undefined
        }
      >
        <span className="ol-check-update-icon">
          <Icon
            name={iconName}
            size={12}
            style={{
              // Color transition when the status icon (check ↔ refresh ↔ error) switches;
              // spin while checking (ol-spin) with the pivot at the icon container's center
              // (width locked to 14).
              transition: 'color 0.18s var(--ol-motion-quick)',
              animation: checking ? 'ol-spin 0.8s linear infinite' : undefined,
            }}
          />
        </span>
        <span key={label} className="ol-check-update-label">
          {label}
        </span>
      </button>
      <UpdateDialog
        status={status}
        currentVersion={updater.currentVersion}
        version={updater.version}
        progress={updater.progress}
        downloaded={updater.downloaded}
        contentLength={updater.contentLength}
        errorMessage={updater.errorMessage}
        onInstall={() => void updater.installUpdate()}
        onClose={() => void updater.dismissDialog()}
      />
    </>
  );
}
