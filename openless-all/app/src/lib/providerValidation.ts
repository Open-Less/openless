import type { ProviderKind } from './ipc/providers';
import { isRealtimeOmniProvider } from './realtimeOmni';

/** Validation must describe the request actually sent by the selected transport. */
export function providerValidationCopy(
  kind: ProviderKind,
  providerType?: string,
  audioValidation = false,
): { hintKey: string; successKey: string } {
  if (kind === 'omni' && isRealtimeOmniProvider(providerType ?? '')) {
    return {
      hintKey: 'settings.channels.validationHintRealtime',
      successKey: 'settings.providers.validateSuccessRealtime',
    };
  }
  return {
    hintKey:
      kind === 'omni' && !audioValidation
        ? 'settings.channels.validationHintOmni'
        : 'settings.channels.validationHint',
    successKey: audioValidation
      ? 'settings.providers.validateSuccessAudio'
      : kind === 'omni'
        ? 'settings.providers.validateSuccessOmni'
        : 'settings.providers.validateSuccess',
  };
}
