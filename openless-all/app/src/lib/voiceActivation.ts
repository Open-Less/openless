export type VoiceActivationPhase =
  | 'disabled'
  | 'paused'
  | 'pausing'
  | 'starting'
  | 'listening_for_wake'
  | 'wake_detected'
  | 'handing_off'
  | 'listening_for_finish'
  | 'finishing'
  | 'unavailable'
  | 'permission_denied'
  | 'mic_busy'
  | 'failed';

export interface VoiceActivationStateSnapshot {
  phase: VoiceActivationPhase;
  activationGeneration?: number;
  sessionId?: string | null;
  message?: string | null;
}

export const VOICE_ACTIVATION_MODEL_ALIAS = 'zipformer-bilingual-zh-en-streaming' as const;
export const VOICE_ACTIVATION_MODEL_MISSING_MESSAGE = 'voice activation model is not installed';

export function isVoiceActivationModelMissing(
  snapshot: VoiceActivationStateSnapshot | null | undefined,
): boolean {
  return (
    snapshot?.phase === 'failed' && snapshot.message === VOICE_ACTIVATION_MODEL_MISSING_MESSAGE
  );
}

export function voiceActivationStatusKey(snapshot: VoiceActivationStateSnapshot): string {
  switch (snapshot.phase) {
    case 'disabled': return 'settings.recording.voiceActivationDisabled';
    case 'paused': return 'settings.recording.voiceActivationPaused';
    case 'pausing': return 'settings.recording.voiceActivationPausing';
    case 'starting':
    case 'wake_detected':
    case 'handing_off': return 'settings.recording.voiceActivationStarting';
    case 'listening_for_wake': return 'settings.recording.voiceActivationListening';
    case 'listening_for_finish':
    case 'finishing': return 'settings.recording.voiceActivationDictating';
    default: return 'settings.recording.voiceActivationError';
  }
}
