// Check-update button — the About page checks the stable channel (channel='stable') and the
// Advanced page's Beta section checks the test channel (channel='beta'); both share this
// component. channel is passed explicitly and is not affected by prefs.updateChannel.

import { useEffect } from 'react';
import { btnGhostStyle } from './shared';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { UpdateDialog, useAutoUpdate } from '../../components/AutoUpdate';
import type { UpdateChannel } from '../../lib/ipc';

export function CheckUpdateButton({
  channel,
  compact = false,
  autoCheckChannel,
}: {
  channel: UpdateChannel;
  compact?: boolean;
  autoCheckChannel?: UpdateChannel | null;
}) {
  const { t } = useTranslation();
  const updater = useAutoUpdate();
  const { status, checking, busy } = updater;

  useEffect(() => {
    if (status === 'none' || status === 'error') {
      const id = window.setTimeout(() => {
        void updater.dismissDialog();
      }, 2500);
      return () => window.clearTimeout(id);
    }
    return undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const upToDate = status === 'none';
  const failed = status === 'error';
  const iconName = upToDate ? 'check' : 'refresh';
  const color = upToDate ? 'var(--ol-ok)' : failed ? 'var(--ol-err)' : 'var(--ol-ink-2)';
  const labelKey =
    channel === 'beta'
      ? 'settings.about.checkBetaUpdateBtn'
      : 'settings.about.checkStableUpdateBtn';
  const label = checking ? t('settings.about.checkingUpdate') : t(labelKey);

  useEffect(() => {
    if (autoCheckChannel) void updater.checkForUpdates(autoCheckChannel);
    // checkForUpdates changes with updater state; this effect is driven only by a channel switch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoCheckChannel]);

  return (
    <>
      <button
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
        // Desktop keeps a stable text width; compact layout uses an icon button with an accessible name.
        style={{
          ...btnGhostStyle,
          color,
          opacity: checking || busy ? 0.7 : 1,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 6,
          boxSizing: 'border-box',
          width: compact ? 32 : undefined,
          minWidth: compact ? 32 : 160,
          maxWidth: compact ? 32 : '100%',
          minHeight: compact ? 32 : undefined,
          padding: compact ? 5 : btnGhostStyle.padding,
          flexShrink: 0,
        }}
      >
        <span
          style={{
            display: 'inline-flex',
            width: 14,
            height: 14,
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
          }}
        >
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
        <span
          key={label}
          style={{
            display: compact ? 'none' : undefined,
            whiteSpace: 'nowrap',
            // Fade-in with a slight slide when the status text ("check" ↔ "checking…" ↔ result)
            // switches; same animation as SelectLite's value switch (ol-select-value-in, global.css).
            animation: 'ol-select-value-in .16s var(--ol-motion-quick)',
          }}
        >
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
