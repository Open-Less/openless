import type { AppUpdateMetadata } from '../../api/updates';
import type { UpdateChannel } from '../../api/contracts';
import type { AndroidUpdateProgress, DesktopUpdateResource } from '../../runtime/updates';
import { isStableChannelSwitch } from '../../lib/appVersion';
import { downloadWithGithubFallback } from '../../lib/updateDownload';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'none'
  | 'downloading'
  | 'installing'
  | 'downloaded'
  | 'error'
  | 'installError';

export type CheckUpdateOptions = { autoInstallAndroid?: boolean };
type Owner = symbol;
type Target = 'android' | 'desktop' | 'unsupported';
type AndroidUpdate = { url: string; signature: string; version: string };
type Operation = {
  owner: Owner | null;
  io: 'check' | 'install' | null;
  currentVersion: string;
  version: string;
  desktop?: DesktopUpdateResource;
  android?: AndroidUpdate;
};

export interface UpdateSnapshot {
  owner: Owner | null;
  status: UpdateStatus;
  currentVersion: string;
  version: string;
  downloaded: number;
  contentLength: number | null;
  errorMessage: string | null;
}

export const idleUpdate: UpdateSnapshot = {
  owner: null,
  status: 'idle',
  currentVersion: '',
  version: '',
  downloaded: 0,
  contentLength: null,
  errorMessage: null,
};

export interface UpdateDependencies {
  target: () => Target;
  check: (channel?: UpdateChannel) => Promise<AppUpdateMetadata | null>;
  githubFallback: (rid: number) => Promise<AppUpdateMetadata>;
  desktopResource: (metadata: AppUpdateMetadata) => DesktopUpdateResource;
  installAndroid: (update: AndroidUpdate) => Promise<void>;
  listenAndroid: (progress: (event: AndroidUpdateProgress) => void) => Promise<() => void>;
  selectStable: () => Promise<void>;
  log: (message: string) => void;
}

export const isDialogStatus = (status: UpdateStatus) =>
  status === 'available' ||
  status === 'downloading' ||
  status === 'installing' ||
  status === 'downloaded' ||
  status === 'installError';

function readAndroidMetadata(metadata: AppUpdateMetadata): AndroidUpdate {
  const { url, signature } = metadata.rawJson ?? {};
  if (typeof url !== 'string' || !url || typeof signature !== 'string' || !signature) {
    throw new Error('The Android update manifest is missing a URL or signature');
  }
  return { url, signature, version: metadata.version };
}

export function createUpdateController(deps: UpdateDependencies) {
  let operation: Operation | undefined;
  let background: Owner | undefined;
  let snapshot = idleUpdate;
  const subscribers = new Set<() => void>();

  function update(next: UpdateSnapshot) {
    snapshot = next;
    subscribers.forEach((subscriber) => subscriber());
  }

  function commit(task: Operation, changes: Partial<UpdateSnapshot>) {
    if (operation === task) update({ ...snapshot, ...changes, owner: task.owner });
  }

  async function release(task: Pick<Operation, 'desktop'>) {
    const resource = task.desktop;
    task.desktop = undefined;
    if (resource)
      await resource.close().catch((error) => deps.log(`close failed: ${String(error)}`));
  }

  function clear(task: Operation) {
    if (operation === task) {
      operation = undefined;
      update(idleUpdate);
    }
    return release(task);
  }

  async function install(task: Operation, automatic: boolean) {
    if (operation !== task || task.io === 'install' || (!task.android && !task.desktop)) return;
    task.io = 'install';
    commit(task, {
      status: 'downloading',
      currentVersion: task.currentVersion,
      version: task.version,
      downloaded: 0,
      contentLength: null,
      errorMessage: null,
    });
    let stopProgress: (() => void) | undefined;
    try {
      if (!automatic && isStableChannelSwitch(task.currentVersion, task.version)) {
        await deps.selectStable();
      }
      if (task.android) {
        // The Android event has no per-download ID. A single controller owns the
        // subscription and starts the installer only after it is listening.
        stopProgress = await deps.listenAndroid((progress) => {
          if (task.io === 'install')
            commit(task, {
              status: progress.phase,
              downloaded: progress.downloaded,
              contentLength: progress.contentLength,
            });
        });
        await deps.installAndroid(task.android);
        task.android = undefined;
      } else if (task.desktop) {
        const installed = await downloadWithGithubFallback(
          task.desktop,
          (candidate) =>
            candidate.download((event) => {
              if (task.io !== 'install' || task.desktop !== candidate) return;
              if (event.kind === 'started')
                commit(task, { downloaded: 0, contentLength: event.size });
              else if (event.kind === 'bytes')
                commit(task, { downloaded: snapshot.downloaded + event.chunk });
              // Download completion is reported when the SDK retains the bytes.
            }),
          (candidate) => candidate.url.startsWith('https://dl.aeoform.com/releases/'),
          async (previous) => {
            const retry = deps.desktopResource(await deps.githubFallback(previous.rid));
            task.desktop = retry;
            await release({ desktop: previous });
            commit(task, { status: 'downloading', downloaded: 0, contentLength: null });
            return retry;
          },
        );
        commit(task, { status: 'installing' });
        await installed.install();
        await release(task);
      }
      task.io = null;
      if (task.owner) commit(task, { status: 'downloaded' });
      else await clear(task);
    } catch (error) {
      task.io = null;
      const errorMessage = error instanceof Error ? error.message : String(error);
      deps.log(`install failed (v${task.version}): ${errorMessage}`);
      await release(task);
      if (!task.owner) {
        await clear(task);
      } else {
        commit(
          task,
          automatic ? { status: 'available' } : { status: 'installError', errorMessage },
        );
      }
    } finally {
      stopProgress?.();
    }
  }

  return {
    subscribe(listener: () => void) {
      subscribers.add(listener);
      return () => {
        subscribers.delete(listener);
      };
    },
    getSnapshot: () => snapshot,
    attach(owner: Owner, backgroundOwner: boolean) {
      if (backgroundOwner) background = owner;
      return () => {
        if (background === owner) background = undefined;
        const task = operation;
        if (!task || task.owner !== owner) return;
        if (task.io === 'install') {
          // A confirmed native install outlives a settings route. Only the
          // presentation owner changes while native resources are still in use.
          task.owner = background ?? null;
          commit(task, {});
        } else {
          void clear(task);
        }
      };
    },
    async check(owner: Owner, channel?: UpdateChannel, options?: CheckUpdateOptions) {
      const previous = operation;
      if (previous?.io || (isDialogStatus(snapshot.status) && previous?.owner !== owner)) return;
      const task: Operation = { owner, io: 'check', currentVersion: '', version: '' };
      operation = task;
      update({ ...idleUpdate, owner, status: 'checking' });
      try {
        if (previous) await release(previous);
        if (operation !== task) return;
        const target = deps.target();
        const metadata = target === 'unsupported' ? null : await deps.check(channel);
        if (metadata && target === 'desktop') task.desktop = deps.desktopResource(metadata);
        if (operation !== task) return await release(task);
        task.io = null;
        if (!metadata) return commit(task, { status: 'none' });
        if (target === 'android') task.android = readAndroidMetadata(metadata);
        task.currentVersion = metadata.currentVersion;
        task.version = metadata.version;
        if (task.android && options?.autoInstallAndroid) await install(task, true);
        else
          commit(task, {
            currentVersion: task.currentVersion,
            version: task.version,
            status: 'available',
          });
      } catch (error) {
        if (operation !== task) return;
        task.io = null;
        const errorMessage = error instanceof Error ? error.message : String(error);
        deps.log(`check failed: ${errorMessage}`);
        commit(task, { status: 'error', errorMessage });
      }
    },
    install(owner: Owner) {
      const task = operation;
      if (task?.owner !== owner || snapshot.status !== 'available') return Promise.resolve();
      return install(task, false);
    },
    dismiss(owner: Owner) {
      const task = operation;
      if (task?.owner !== owner || task.io === 'install') return Promise.resolve();
      return clear(task);
    },
  };
}
