import { useEffect, useRef, useState } from 'react';
import { isDialogStatus } from './updateController';
import { UpdateDialog } from './UpdateDialog';
import { useAutoUpdate } from './useAutoUpdate';
import { getPlatformCapabilities } from '../../runtime/platform';
import { getRuntimeEnvironment } from '../../runtime/environment';
import type { PlatformCapabilities } from '../../api/contracts';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';

const AUTO_CHECK_INTERVAL_MS = 60 * 60 * 1000;
const STARTUP_DELAY_MS = 4_000;

export function AutoUpdateGate() {
  const { prefs } = useHotkeySettings();
  const u = useAutoUpdate(true);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const enabled = prefs?.autoUpdateCheck === true && platformCaps?.supportsAutoUpdate === true;

  useEffect(() => {
    let mounted = true;
    void getPlatformCapabilities()
      .then((caps) => {
        if (mounted) setPlatformCaps(caps);
      })
      .catch((error) => console.warn('[updater] capabilities unavailable', error));
    return () => {
      mounted = false;
    };
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
      void current
        .checkForUpdates(undefined, {
          autoInstallAndroid: getRuntimeEnvironment().os === 'android',
        })
        .catch((error) => {
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

  return (
    <UpdateDialog
      deferPrompt
      status={u.status}
      currentVersion={u.currentVersion}
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
