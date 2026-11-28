import { createUpdateController, type UpdateDependencies } from './updateController';
import type { AppUpdateMetadata } from '../../api/updates';
import type { AndroidUpdateProgress, UpdateDownloadEvent } from '../../runtime/updates';

function verify(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function gate<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

const update = (rid: number, url = 'https://github.com/example/release'): AppUpdateMetadata => ({
  rid,
  rawJson: { url },
  currentVersion: '2.0.1-beta.1',
  version: '2.0.1',
});

function dependencies(): UpdateDependencies {
  return {
    target: () => 'desktop',
    check: async () => null,
    githubFallback: async () => {
      throw new Error('unexpected fallback');
    },
    desktopResource: (metadata) => ({
      rid: metadata.rid,
      url: String(metadata.rawJson.url),
      download: async () => {},
      install: async () => {},
      close: async () => {},
    }),
    installAndroid: async () => {
      throw new Error('unexpected Android install');
    },
    listenAndroid: async () => () => {},
    selectStable: async () => {},
    log: () => {},
  };
}

{
  const deps = dependencies();
  const old = gate<AppUpdateMetadata>();
  const closed: number[] = [];
  let checks = 0;
  deps.check = () => (++checks === 1 ? old.promise : Promise.resolve(update(2)));
  const create = deps.desktopResource;
  deps.desktopResource = (metadata) => ({
    ...create(metadata),
    close: async () => {
      closed.push(metadata.rid);
    },
  });
  const controller = createUpdateController(deps);
  const owner = Symbol('previous route');
  const next = Symbol('new route');
  const detach = controller.attach(owner, false);
  const pending = controller.check(owner);
  detach();
  await controller.check(next);
  old.resolve(update(1));
  await pending;
  verify(
    controller.getSnapshot().owner === next && controller.getSnapshot().status === 'available',
    'a late check superseded the current route',
  );
  verify(closed.join(',') === '1', 'the orphaned native resource was not released exactly once');
  await controller.dismiss(next);
  verify(closed.join(',') === '1,2', 'dismissal did not release the pending update');
}

{
  const deps = dependencies();
  const downloadStarted = gate<void>();
  const permitInstall = gate<void>();
  const calls: string[] = [];
  deps.check = async () => update(10, 'https://dl.aeoform.com/releases/app.zip');
  deps.githubFallback = async (rid) => {
    calls.push(`fallback:${rid}`);
    return update(20);
  };
  deps.selectStable = async () => {
    calls.push('stable');
  };
  deps.desktopResource = (metadata) => ({
    rid: metadata.rid,
    url: String(metadata.rawJson.url),
    close: async () => {
      calls.push(`close:${metadata.rid}`);
    },
    install: async () => {
      calls.push(`install:${metadata.rid}`);
    },
    download: async (progress: (event: UpdateDownloadEvent) => void) => {
      calls.push(`download:${metadata.rid}`);
      if (metadata.rid === 10) throw new Error('mirror unavailable');
      progress({ kind: 'started', size: 100 });
      progress({ kind: 'bytes', chunk: 40 });
      progress({ kind: 'finished' });
      downloadStarted.resolve();
      await permitInstall.promise;
    },
  });
  const controller = createUpdateController(deps);
  const main = Symbol('window');
  const manual = Symbol('manual check');
  controller.attach(main, true);
  const detach = controller.attach(manual, false);
  await controller.check(manual, 'stable');
  const install = controller.install(manual);
  await downloadStarted.promise;
  verify(
    controller.getSnapshot().status === 'downloading',
    'progress alone cannot prove bytes are retained by the updater',
  );
  verify(
    controller.getSnapshot().downloaded === 40,
    'the retry failed to report its measured bytes',
  );
  detach();
  verify(
    controller.getSnapshot().owner === main,
    'an active installation did not transfer to the window',
  );
  await controller.check(main);
  permitInstall.resolve();
  await install;
  verify(
    controller.getSnapshot().status === 'downloaded',
    'successful installation was not published',
  );
  verify(
    calls.join(',') === 'stable,download:10,fallback:10,close:10,download:20,install:20,close:20',
    'an old or unready package was installed or leaked',
  );
}

{
  const deps = dependencies();
  const reachedInstaller = gate<void>();
  const installer = gate<void>();
  let emit: ((event: AndroidUpdateProgress) => void) | undefined;
  let detached = false;
  deps.target = () => 'android';
  deps.check = async () => ({
    ...update(0),
    rawJson: { url: 'https://example.test/app.apk', signature: 'signature' },
  });
  deps.listenAndroid = async (listener) => {
    emit = listener;
    return () => {
      detached = true;
    };
  };
  deps.installAndroid = async (candidate) => {
    verify(
      emit && candidate.signature === 'signature',
      'installer started before the progress subscription or lost its signature',
    );
    emit({ downloaded: 25, contentLength: 25, phase: 'installing' });
    reachedInstaller.resolve();
    await installer.promise;
  };
  const controller = createUpdateController(deps);
  const owner = Symbol('automatic Android check');
  const operation = controller.check(owner, undefined, { autoInstallAndroid: true });
  await reachedInstaller.promise;
  verify(
    controller.getSnapshot().status === 'installing',
    'installer progress was not scoped to the active job',
  );
  await controller.dismiss(owner);
  verify(
    controller.getSnapshot().status === 'installing',
    'dismissal interrupted an active install',
  );
  installer.resolve();
  await operation;
  emit?.({ downloaded: 1, contentLength: 25, phase: 'downloading' });
  verify(
    detached && controller.getSnapshot().status === 'downloaded',
    'late Android progress overwrote installer completion',
  );
}

console.log(
  'Update controller: scope cancellation, resource ownership, fallback and Android install passed',
);
