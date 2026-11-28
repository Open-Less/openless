import type { LocalAsrEngineStatus } from '../lib/localAsr';
import { listenTauriEvent } from '../runtime/transport';

export * from '../lib/localAsr';

export function listenLocalAsrEngineStatus(
  onChange: (status: LocalAsrEngineStatus) => void,
): Promise<() => void> {
  return listenTauriEvent('local-asr:engine-changed', onChange);
}
