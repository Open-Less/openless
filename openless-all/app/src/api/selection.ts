export * from '../lib/ipc/selection-voice-preview';
export * from '../lib/ipc/expression';

import { listenTauriEvent } from '../runtime/transport';

export function listenSelectionVoiceIntentShown(onShown: () => void): Promise<() => void> {
  return listenTauriEvent('selection-voice-intent:shown', onShown);
}
