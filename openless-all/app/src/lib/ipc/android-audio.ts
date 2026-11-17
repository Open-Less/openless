import { invokeOrMock } from './shared';

export interface AndroidAudioEffectsStatus {
  aecAvailable: boolean;
  noiseSuppressionAvailable: boolean;
  agcAvailable: boolean;
  runtime: null | {
    active: boolean;
    aec?: boolean;
    noiseSuppression?: boolean;
    agc?: boolean;
    error: string | null;
  };
}

export function getAndroidAudioEffectsStatus(): Promise<AndroidAudioEffectsStatus> {
  return invokeOrMock('get_android_audio_effects_status', undefined, () => ({
    aecAvailable: false,
    noiseSuppressionAvailable: false,
    agcAvailable: false,
    runtime: null,
  }));
}

export function setAndroidAudioMonitoring(enabled: boolean): Promise<void> {
  return invokeOrMock('set_android_audio_monitoring', { enabled }, () => undefined);
}
