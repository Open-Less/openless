export { getVoiceActivationState } from '../lib/ipc/settings';
export * from '../lib/ipc/devices';
export * from '../lib/ipc/expression';
export * from '../lib/ipc/android-audio';
export * from '../lib/ipc/remote-server';

import type { VoiceActivationStateSnapshot } from '../lib/voiceActivation';
import { getRuntimeEnvironment } from '../runtime/environment';
import { invokeOrPreview, listenTauriEvent } from '../runtime/transport';

export interface AndroidAudioSpectrum {
  raw: number[];
  processed: number[];
}

export interface RemoteInputStartError {
  reason: string;
  port: number;
}

export function listenRemoteInputStatusChanges(onChange: () => void): Promise<() => void> {
  // Pairing details belong to the explicit status snapshot, not the broadcast.
  return listenTauriEvent('remote-input:running', onChange);
}

export function listenRemoteInputErrors(
  onError: (error: RemoteInputStartError) => void,
): Promise<() => void> {
  return listenTauriEvent<Partial<RemoteInputStartError> | null>('remote-input:error', (error) =>
    onError({ reason: error?.reason ?? '', port: error?.port ?? 0 }),
  );
}

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

export function listenMicrophoneLevelChanges(
  onLevel: (level: number) => void,
): Promise<() => void> {
  if (getRuntimeEnvironment().host === 'harmony') {
    const listener = (event: Event) => onLevel((event as CustomEvent<number>).detail);
    window.addEventListener('openless:harmony-level', listener);
    return Promise.resolve(() => window.removeEventListener('openless:harmony-level', listener));
  }
  return listenTauriEvent<{ level: number }>('microphone:level', ({ level }) =>
    onLevel(level ?? 0),
  );
}

export function listenAndroidAudioSpectrum(
  onSpectrum: (spectrum: AndroidAudioSpectrum) => void,
): Promise<() => void> {
  return listenTauriEvent('android-audio-spectrum', onSpectrum);
}

export function listenVoiceActivationChanges(
  onChange: (state: VoiceActivationStateSnapshot) => void,
): Promise<() => void> {
  return listenTauriEvent('voice-activation:state-changed', onChange);
}
