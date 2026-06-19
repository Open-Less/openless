// 主窗口启动 + 后台每 60 分钟自动检查更新。
// 受 prefs.autoUpdateCheck 开关控制；关闭时只走 Settings 手动按钮。
// 桌面：发现新版本弹 UpdateDialog 等用户确认。
// Android：发现新版本后自动下载、校验并打开系统安装器（进度仍走 UpdateDialog）。

import { useEffect, useRef, useState } from 'react';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from './AutoUpdate';
import { getPlatformCapabilities, isAndroid } from '../lib/ipc';
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

  const uRef = useRef(u);
  uRef.current = u;

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;

    const tick = () => {
      if (cancelled) return;
      const current = uRef.current;
      if (current.checking || current.busy || isDialogStatus(current.status)) return;
      void current.checkForUpdates(undefined, { autoInstallAndroid: isAndroid() }).catch(error => {
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
      errorMessage={u.errorMessage}
      onInstall={u.installUpdate}
      onClose={u.dismissDialog}
    />
  );
}
