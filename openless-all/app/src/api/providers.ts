import { listenTauriEvent } from '../runtime/transport';
import type { CredentialsStatus } from './contracts';

export * from '../lib/ipc/providers';
export * from '../lib/ipc/asr-credentials';
export * from '../lib/ipc/channels';
export * from '../lib/ipc/siwc';

export function listenCredentialStatusChanges(
  onChange: (status: CredentialsStatus) => void,
): Promise<() => void> {
  return listenTauriEvent('credentials:changed', onChange);
}

export async function listenCredentialsChanges(onChange: () => void): Promise<() => void> {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener('ol-channels-changed', onChange);
  try {
    const stop = await listenCredentialStatusChanges(onChange);
    return () => {
      stop();
      window.removeEventListener('ol-channels-changed', onChange);
    };
  } catch (error) {
    window.removeEventListener('ol-channels-changed', onChange);
    throw error;
  }
}
