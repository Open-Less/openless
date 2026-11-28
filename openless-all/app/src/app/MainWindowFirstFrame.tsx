import { useEffect } from 'react';
import { withStartupDeadline } from '../lib/startupProbe';
import { initialThemeReady } from '../lib/themeMode';
import { IS_DEV_BUILD } from '../lib/appVersion';
import { isTauriHost, type OS } from '../runtime/environment';

const FIRST_FRAME_BUDGET_MS = 900;
const PENDING_OVERVIEW = '[data-ol-overview-pending]';

function waitForOverview(signal: AbortSignal): Promise<void> {
  if (!document.querySelector(PENDING_OVERVIEW)) return Promise.resolve();
  return new Promise((resolve) => {
    const finish = () => {
      observer.disconnect();
      signal.removeEventListener('abort', finish);
      resolve();
    };
    const observer = new MutationObserver(() => {
      if (!document.querySelector(PENDING_OVERVIEW)) finish();
    });
    observer.observe(document.getElementById('root') ?? document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['data-ol-overview-pending'],
    });
    signal.addEventListener('abort', finish, { once: true });
  });
}

/** Mounted inside the real destination's Suspense boundary, or on the recovery screen. */
export function MainWindowFirstFrame({ os }: { os: OS }) {
  useEffect(() => {
    if (os !== 'mac' || !isTauriHost()) return;
    const controller = new AbortController();
    const optionalReads = new AbortController();
    controller.signal.addEventListener('abort', () => optionalReads.abort(), { once: true });
    void (async () => {
      // Local history and theme usually settle while the window is hidden. Slow reads
      // must not block the shell; hidden WebKit views may also suspend animation frames.
      await withStartupDeadline(
        Promise.all([initialThemeReady(), waitForOverview(optionalReads.signal)]),
        controller.signal,
        FIRST_FRAME_BUDGET_MS,
      ).catch(() => undefined);
      optionalReads.abort();
      if (controller.signal.aborted) return;
      const { invoke } = await import('@tauri-apps/api/core');
      if (IS_DEV_BUILD) {
        const update = await invoke('app_check_update_with_channel', { timeoutMs: 1000 });
        if (update !== null) throw new Error('Dev updater must be disabled');
      }
      if (!controller.signal.aborted) await invoke('main_window_frame_ready');
    })().catch((error) => console.warn('[startup] main frame signal failed', error));
    return () => controller.abort();
  }, [os]);
  return null;
}
