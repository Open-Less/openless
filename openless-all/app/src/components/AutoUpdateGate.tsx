// Main-window startup + background auto update check every 60 minutes.
// Controlled by the prefs.autoUpdateCheck switch; when off, only the manual Settings
// button runs. Desktop: a new version opens UpdateDialog for user confirmation.
// Android: a new version auto-downloads, verifies, and opens the system installer
// (progress still goes through UpdateDialog).

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
      void current
        .checkForUpdates(undefined, { autoInstallAndroid: isAndroid() })
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
