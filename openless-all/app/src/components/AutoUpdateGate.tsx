// 主窗口启动 + 后台每 60 分钟自动调一次 plugin-updater check。
// 受 prefs.autoUpdateCheck 开关控制；关闭时只走 Settings → 关于 的手动按钮。
// 找到新版本时直接挂 UpdateDialog；不弹自定义通知，沿用既有 dialog 视觉。

import { useEffect, useRef, useState } from 'react';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from './AutoUpdate';
import { getPlatformCapabilities } from '../lib/ipc';
import type { PlatformCapabilities } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

const AUTO_CHECK_INTERVAL_MS = 60 * 60 * 1000;
const STARTUP_DELAY_MS = 4_000;

export function AutoUpdateGate() {
  const { prefs } = useHotkeySettings();
  const u = useAutoUpdate();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const enabled = (prefs?.autoUpdateCheck ?? true) && platformCaps?.supportsAutoUpdate === true;

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  // 用 ref 保持 tick 闭包始终读到最新的 useAutoUpdate 返回值。
  // 之前直接捕获 `u` 会让 60min interval 触发时读旧 status 闭包——例如用户已经
  // 手动打开 UpdateDialog 后，tick 仍可能错过 busy 检查触发并发 check。
  // 修 pr_agent "Stale closure" 反馈。
  const uRef = useRef(u);
  uRef.current = u;

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;

    const tick = () => {
      if (cancelled) return;
      const current = uRef.current;
      if (current.checking || current.busy || isDialogStatus(current.status)) return;
      void current.checkForUpdates().catch(error => {
        console.warn('[auto-update] background check failed', error);
      });
    };

    const startupTimer = window.setTimeout(tick, STARTUP_DELAY_MS);
    const intervalTimer = window.setInterval(tick, AUTO_CHECK_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(startupTimer);
      window.clearInterval(intervalTimer);
    };
  }, [enabled]);

  if (platformCaps?.supportsAutoUpdate !== true) return null;

  if (!isDialogStatus(u.status)) return null;
  return (
    <UpdateDialog
      status={u.status}
      version={u.version}
      progress={u.progress}
      downloaded={u.downloaded}
      contentLength={u.contentLength}
      onInstall={u.installUpdate}
      onClose={u.dismissDialog}
    />
  );
}
