// 检查更新按钮 —— 关于页查正式版（channel='stable'）、高级页 Beta 区查测试版
// （channel='beta'），共用此组件。channel 显式传入，不受 prefs.updateChannel 影响。

import { useEffect } from 'react';
import { btnGhostStyle } from './shared';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from '../../components/AutoUpdate';
import type { UpdateChannel } from '../../lib/ipc';

export function CheckUpdateButton({ channel, compact = false }: { channel: UpdateChannel; compact?: boolean }) {
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
        // 桌面保留稳定文字宽度；紧凑布局使用带可访问名称的图标按钮。
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
        <span style={{ display: 'inline-flex', width: 14, height: 14, alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
          <Icon
            name={iconName}
            size={12}
            style={{
              // 状态图标（check ↔ refresh ↔ 错误）切换时颜色过渡；
              // 检查中旋转（ol-spin），旋转轴在图标容器中心（宽度锁死 14）。
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
            // 状态文案（"检查更新" ↔ "检查中…" ↔ 结果提示）切换时淡入微滑移，
            // 与 SelectLite 选中值切换动画同款（ol-select-value-in，global.css）。
            animation: 'ol-select-value-in .16s var(--ol-motion-quick)',
          }}
        >
          {label}
        </span>
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
