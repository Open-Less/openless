import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import {
  appCheckUpdateWithChannel,
  appDownloadAndInstallAndroidUpdate,
  appUpdateGithubFallback,
  setUpdateChannel,
} from '../../api/updates';
import { logClientError } from '../../api/files';
import type { UpdateChannel } from '../../api/contracts';
import { getRuntimeEnvironment } from '../../runtime/environment';
import { createDesktopUpdateResource, listenAndroidUpdateProgress } from '../../runtime/updates';
import {
  createUpdateController,
  idleUpdate,
  type CheckUpdateOptions,
  type UpdateSnapshot,
} from './updateController';

const controller = createUpdateController({
  target: () => {
    const { os, host } = getRuntimeEnvironment();
    return host !== 'tauri' ? 'unsupported' : os === 'android' ? 'android' : 'desktop';
  },
  check: (channel) => appCheckUpdateWithChannel(15_000, channel ?? null),
  githubFallback: appUpdateGithubFallback,
  desktopResource: createDesktopUpdateResource,
  installAndroid: appDownloadAndInstallAndroidUpdate,
  listenAndroid: listenAndroidUpdateProgress,
  selectStable: () => setUpdateChannel('stable'),
  log: (message) => {
    console.warn(`[updater] ${message}`);
    void logClientError(`[updater] ${message}`).catch(() => {});
  },
});

export interface UseAutoUpdate extends Omit<UpdateSnapshot, 'owner'> {
  progress: number | null;
  checking: boolean;
  busy: boolean;
  checkForUpdates: (channel?: UpdateChannel, options?: CheckUpdateOptions) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissDialog: () => Promise<void>;
}

export function useAutoUpdate(background = false): UseAutoUpdate {
  const [owner] = useState(() => Symbol('update presentation'));
  const current = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  );
  useEffect(() => controller.attach(owner, background), [owner, background]);
  const actions = useMemo(
    () => ({
      checkForUpdates: (channel?: UpdateChannel, options?: CheckUpdateOptions) =>
        controller.check(owner, channel, options),
      installUpdate: () => controller.install(owner),
      dismissDialog: () => controller.dismiss(owner),
    }),
    [owner],
  );
  const { owner: _owner, ...shown } = current.owner === owner ? current : idleUpdate;
  return {
    ...shown,
    ...actions,
    checking: current.status === 'checking',
    busy: current.status === 'downloading' || current.status === 'installing',
    progress:
      shown.contentLength && shown.contentLength > 0
        ? Math.min(100, Math.round((shown.downloaded / shown.contentLength) * 100))
        : null,
  };
}
