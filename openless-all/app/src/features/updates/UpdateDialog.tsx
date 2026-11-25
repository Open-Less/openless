import { useId, useLayoutEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { restartApp } from '../../api/updates';
import { openExternal } from '../../api/files';
import { isStableChannelSwitch } from '../../lib/appVersion';
import { getRuntimeEnvironment } from '../../runtime/environment';
import { Button } from '../../ui/controls/Button';
import { PresenceModal } from '../../ui/overlays/Modal';
import { useAppViewport } from '../../ui/layout/AppViewport';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import type { UpdateStatus } from './updateController';
import './update-dialog.css';

const RELEASE_DOWNLOAD_URL = 'https://github.com/Open-Less/openless/releases';

export interface UpdateDialogProps {
  status: UpdateStatus;
  currentVersion: string;
  version: string;
  progress: number | null;
  downloaded: number;
  contentLength: number | null;
  errorMessage?: string | null;
  onInstall: () => void;
  onClose: () => void;
  deferPrompt?: boolean;
}

export function UpdateDialog(props: UpdateDialogProps) {
  const { t } = useTranslation();
  const { status, currentVersion, version, onClose, deferPrompt } = props;
  const { modalStack } = useAppViewport();
  const titleId = useId();
  const selected = status === 'available';
  const android = getRuntimeEnvironment().os === 'android';
  const androidInstalled = android && status === 'downloaded';
  const switching = selected && isStableChannelSwitch(currentVersion, version);
  const title = t(
    `settings.about.updateDialog.${switching ? 'stableChannelSwitch' : androidInstalled ? 'androidInstalled' : status}.title`,
  );
  const availableMessage = switching
    ? t('settings.about.updateDialog.stableChannelSwitch.desc', { currentVersion, version })
    : t('settings.about.updateDialog.available.desc', { version });
  const actions = useRef(props);
  actions.current = props;
  const { openDialog, notify } = useInteraction(
    selected ? `${currentVersion}:${version}` : 'activity',
  );

  useLayoutEffect(() => {
    if (!selected) return;
    let alive = true;
    let stopWaiting: (() => void) | undefined;
    const present = () => {
      stopWaiting?.();
      void openDialog<'install'>({
        title,
        description: availableMessage,
        width: 'min(360px, 100%)',
        render: ({ resolve, cancel }) => (
          <div className="ol-update-actions">
            <Button size="sm" onClick={cancel}>
              {t('common.cancel')}
            </Button>
            <Button size="sm" variant="blue" onClick={() => resolve('install')}>
              {t('settings.about.updateDialog.install')}
            </Button>
          </div>
        ),
      }).then((choice) => {
        if (!alive) return;
        if (choice === 'install') actions.current.onInstall();
        else actions.current.onClose();
      });
    };
    if (deferPrompt && modalStack.getActive()) {
      stopWaiting = modalStack.subscribe(() => {
        if (!modalStack.getActive()) present();
      });
    } else {
      present();
    }
    return () => {
      alive = false;
      stopWaiting?.();
    };
  }, [availableMessage, deferPrompt, modalStack, openDialog, selected, t, title]);

  const downloading = status === 'downloading';
  const installing = status === 'installing';
  const downloaded = status === 'downloaded';
  const installError = status === 'installError';
  const busy = downloading || installing;
  const actionInFlight = useRef(false);
  const showActionFailure = (error: unknown) => {
    notify({
      state: 'failed',
      message: t('settings.about.updateDialog.installError.desc', {
        error: error instanceof Error ? error.message : String(error),
      }),
    });
  };

  const runAction = (action: () => Promise<void>) => {
    if (actionInFlight.current) return;
    actionInFlight.current = true;
    void action()
      .catch(showActionFailure)
      .finally(() => {
        actionInFlight.current = false;
      });
  };

  return (
    <PresenceModal
      open={busy || downloaded || installError}
      onClose={busy ? () => {} : onClose}
      zIndex={40}
      width="min(360px, 100%)"
      style={{ padding: 18 }}
      labelledBy={titleId}
      render={() => (
        <>
          <h2 id={titleId} className="ol-update-title">
            {title}
          </h2>
          <p className="ol-update-detail">
            {androidInstalled
              ? t('settings.about.updateDialog.androidInstalled.desc', {
                  version,
                  defaultValue:
                    'The system installer opened. Finish the installation there, then reopen OpenLess to use {{version}}.',
                })
              : installError
                ? t('settings.about.updateDialog.installError.desc', {
                    error: props.errorMessage || t('settings.about.updateError'),
                  })
                : t(`settings.about.updateDialog.${status}.desc`, { version })}
          </p>
          {(busy || downloaded) && (
            <Progress
              status={status}
              progress={props.progress}
              downloaded={props.downloaded}
              contentLength={props.contentLength}
            />
          )}
          <div className="ol-update-actions">
            {busy && (
              <Button size="sm" disabled>
                {t(
                  `settings.about.updateDialog.${installing ? 'installingLabel' : 'downloadingLabel'}`,
                )}
              </Button>
            )}
            {downloaded && (
              <Button size="sm" onClick={onClose}>
                {t('settings.about.updateDialog.later')}
              </Button>
            )}
            {downloaded && !androidInstalled && (
              <Button variant="blue" size="sm" onClick={() => runAction(restartApp)}>
                {t('settings.about.updateDialog.restartNow')}
              </Button>
            )}
            {installError && (
              <>
                <Button size="sm" onClick={onClose}>
                  {t('common.cancel')}
                </Button>
                <Button
                  variant="blue"
                  size="sm"
                  onClick={() => runAction(() => openExternal(RELEASE_DOWNLOAD_URL))}
                >
                  {t('settings.about.updateDialog.manualDownload')}
                </Button>
              </>
            )}
          </div>
        </>
      )}
    />
  );
}

function Progress({
  status,
  progress,
  downloaded,
  contentLength,
}: Pick<UpdateDialogProps, 'status' | 'progress' | 'downloaded' | 'contentLength'>) {
  const { t } = useTranslation();
  const finished = status === 'downloaded' || status === 'installing';
  const label =
    status === 'installing'
      ? t('settings.about.updateDialog.installingLabel')
      : progress === null
        ? t('settings.about.updateDialog.progressUnknown', { downloaded: formatBytes(downloaded) })
        : t('settings.about.updateDialog.progress', {
            progress,
            downloaded: formatBytes(downloaded),
            total: formatBytes(contentLength ?? 0),
          });
  return (
    <div className="ol-update-progress">
      <progress max={100} value={finished ? 100 : (progress ?? undefined)} aria-label={label} />
      <small>{label}</small>
    </div>
  );
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
