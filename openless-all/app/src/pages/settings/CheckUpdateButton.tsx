// 检查更新按钮 —— 关于页查正式版（channel='stable'）、高级页 Beta 区查测试版
// （channel='beta'），共用此组件。
//
// 检查中：按钮内图标转圈。结果（已是最新 / 失败）只在按钮内以图标 + 颜色短暂
// 呈现 2.5s 后自动回到 idle，绝不另起文字块、不改变所在卡片高度 —— 杜绝
// 「渲染框突然变大 / 抽搐」。发现新版则弹出固定定位的 UpdateDialog。

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
    // 只按 status 触发：useAutoUpdate 每次渲染都返回新 updater 对象，把它放进
    // 依赖会让父组件每次重渲染都把 2.5s 自动收起计时器清掉重置。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const upToDate = status === 'none';
  const failed = status === 'error';
  const iconName = upToDate ? 'check' : 'refresh';
  const color = upToDate ? 'var(--ol-ok)' : failed ? 'var(--ol-err)' : 'var(--ol-ink-2)';
  const label = checking ? t('settings.about.checkingUpdate') : t('settings.about.checkUpdateBtn');

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
        style={{ ...btnGhostStyle, color, opacity: checking || busy ? 0.7 : 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6, minWidth: 84 }}
      >
        <Icon
          name={iconName}
          size={12}
          style={checking ? { animation: 'ol-spin 0.8s linear infinite' } : undefined}
        />
        <span style={{ whiteSpace: 'nowrap' }}>{label}</span>
      </button>
      {isDialogStatus(status) && (
        <UpdateDialog
          status={status}
          version={updater.version}
          progress={updater.progress}
          downloaded={updater.downloaded}
          contentLength={updater.contentLength}
          onInstall={() => void updater.installUpdate()}
          onClose={() => void updater.dismissDialog()}
        />
      )}
    </>
  );
}

