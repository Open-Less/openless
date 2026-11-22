export * from '../lib/ipc/selection-polish-preview';
export * from '../lib/ipc/selection-voice-preview';
export * from '../lib/ipc/expression';

import { listenTauriEvent } from '../runtime/transport';

export function listenSelectionPolishPreviewShown(onShown: () => void): Promise<() => void> {
  return listenTauriEvent('selection-polish-preview:shown', onShown);
}

export function listenSelectionPolishPreviewHidden(onHidden: () => void): Promise<() => void> {
  return listenTauriEvent('selection-polish-preview:hide', onHidden);
}

export function listenSelectionVoiceIntentShown(onShown: () => void): Promise<() => void> {
  return listenTauriEvent('selection-voice-intent:shown', onShown);
}
