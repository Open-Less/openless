import { listenTauriEvent } from '../runtime/transport';

export * from '../lib/ipc/providers';
export * from '../lib/ipc/asr-credentials';
export * from '../lib/ipc/channels';
export * from '../lib/ipc/siwc';

export async function listenCredentialsChanges(onChange: () => void): Promise<() => void> {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener('ol-channels-changed', onChange);
  try {
    const stop = await listenTauriEvent('credentials:changed', onChange);
    return () => {
      stop();
      window.removeEventListener('ol-channels-changed', onChange);
    };
  } catch (error) {
    window.removeEventListener('ol-channels-changed', onChange);
    throw error;
  }
}
