export * from '../lib/ipc/voice-edit-session';

import { listenTauriEvent } from '../runtime/transport';

export function listenEmbeddedVoiceEditShown(onShown: () => void): Promise<() => void> {
  return listenTauriEvent('voice-edit:show', onShown);
}

export function listenEmbeddedVoiceEditDismissed(onDismissed: () => void): Promise<() => void> {
  return listenTauriEvent('voice-edit:dismiss', onDismissed);
}
