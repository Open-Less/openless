import { Update } from '@tauri-apps/plugin-updater';
import { listenTauriEvent } from './transport';

export type UpdateDownloadEvent =
  | { kind: 'started'; size: number | null }
  | { kind: 'bytes'; chunk: number }
  | { kind: 'finished' };

export interface DesktopUpdateResource {
  rid: number;
  url: string;
  download: (progress: (event: UpdateDownloadEvent) => void) => Promise<void>;
  install: () => Promise<void>;
  close: () => Promise<void>;
}

export interface AndroidUpdateProgress {
  downloaded: number;
  contentLength: number | null;
  phase: 'downloading' | 'installing';
}

export function createDesktopUpdateResource(metadata: {
  rid: number;
  currentVersion: string;
  version: string;
  date?: string | null;
  body?: string | null;
  rawJson: Record<string, unknown>;
}): DesktopUpdateResource {
  const update = new Update({
    ...metadata,
    date: metadata.date ?? undefined,
    body: metadata.body ?? undefined,
  });
  return {
    rid: update.rid,
    url: String(update.rawJson?.url ?? ''),
    download: (progress) =>
      update.download((event) => {
        if (event.event === 'Started')
          progress({ kind: 'started', size: event.data.contentLength ?? null });
        else if (event.event === 'Progress')
          progress({ kind: 'bytes', chunk: event.data.chunkLength });
        else if (event.event === 'Finished') progress({ kind: 'finished' });
      }),
    install: () => update.install(),
    close: () => update.close(),
  };
}

export function listenAndroidUpdateProgress(onProgress: (progress: AndroidUpdateProgress) => void) {
  return listenTauriEvent<AndroidUpdateProgress>('android-update:progress', onProgress);
}
