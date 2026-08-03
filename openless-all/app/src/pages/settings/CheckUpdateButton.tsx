// 检查更新按钮 —— 关于页查正式版（channel='stable'）、高级页 Beta 区查测试版
// （channel='beta'），共用此组件。channel 显式传入，不受 prefs.updateChannel 影响。

import { useEffect } from 'react';
import { btnGhostStyle } from './shared';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from '../../components/AutoUpdate';
import type { UpdateChannel } from '../../lib/ipc';

export function CheckUpdateButton({ channel }: { channel: UpdateChannel }) {
  const { t } = useTranslation();
  const updater = useAutoUpdate();
  const { status, checking, busy } = updater;

  useEffect(() => {
    if (status === 'none' || status === 'error') {
      const id = window.setTimeout(() => { void updater.dismissDialog(); }, 2500);
      return () => window.clearTimeout(id);
    }
    return undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const upToDate = status === 'none';
  const failed = status === 'error';
  const iconName = upToDate ? 'check' : 'refresh';
  const color = upToDate ? 'var(--ol-ok)' : failed ? 'var(--ol-err)' : 'var(--ol-ink-2)';
  const labelKey = channel === 'beta'
    ? 'settings.about.checkBetaUpdateBtn'
    : 'settings.about.checkStableUpdateBtn';
  const label = checking ? t('settings.about.checkingUpdate') : t(labelKey);

  return (
    <>
      <button
        onClick={() => void updater.checkForUpdates(channel)}
        disabled={checking || busy}
        title={
          failed
            ? (updater.errorMessage ?? t('settings.about.updateError'))
            : upToDate
              ? t('settings.about.upToDate')
              : undefined
        }
        // 固定宽度 + 内容居中：图标（check ↔ refresh）与 label（"检查更新" ↔ "检查中…"）
        // 切换时按钮尺寸不再跳动；图标固定容器保证旋转时绕中心转、不位移。
        style={{ ...btnGhostStyle, color, opacity: checking || busy ? 0.7 : 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6, minWidth: 160 }}
      >
        <span style={{ display: 'inline-flex', width: 14, height: 14, alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
          <Icon
            name={iconName}
            size={12}
            style={checking ? { animation: 'ol-spin 0.8s linear infinite' } : undefined}
          />
        </span>
        <span style={{ whiteSpace: 'nowrap' }}>{label}</span>
      </button>
      {isDialogStatus(status) && (
        <UpdateDialog
          status={status}
          version={updater.version}
          progress={updater.progress}
          downloaded={updater.downloaded}
          contentLength={updater.contentLength}
          errorMessage={updater.errorMessage}
          onInstall={() => void updater.installUpdate()}
          onClose={() => void updater.dismissDialog()}
        />
      )}
    </>
  );
}
