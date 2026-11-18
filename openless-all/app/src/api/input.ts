export * from '../lib/ipc/devices';
export * from '../lib/ipc/expression';
export * from '../lib/ipc/android-audio';
export * from '../lib/ipc/remote-server';

import type { VoiceActivationStateSnapshot } from '../lib/voiceActivation';
import { invokeOrPreview, listenTauriEvent } from '../runtime/transport';

export function getAutostartEnabled(): Promise<boolean> {
  return invokeOrPreview('plugin:autostart|is_enabled', undefined, () => false);
}

export function setAutostartEnabled(enabled: boolean): Promise<void> {
  return invokeOrPreview(
    enabled ? 'plugin:autostart|enable' : 'plugin:autostart|disable',
    undefined,
    () => undefined,
  );
}

export function listenMicrophoneDeviceChanges(onChange: () => void): Promise<() => void> {
  return listenTauriEvent('microphone:devices-changed', onChange);
}

export function listenVoiceActivationChanges(
  onChange: (state: VoiceActivationStateSnapshot) => void,
): Promise<() => void> {
  return listenTauriEvent('voice-activation:state-changed', onChange);
}
