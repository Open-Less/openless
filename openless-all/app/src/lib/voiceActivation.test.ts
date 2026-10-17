import {
  isVoiceActivationModelMissing,
  VOICE_ACTIVATION_MODEL_MISSING_MESSAGE,
} from './voiceActivation';

function assertEqual(actual: boolean, expected: boolean, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

assertEqual(
  isVoiceActivationModelMissing({
    phase: 'failed',
    message: VOICE_ACTIVATION_MODEL_MISSING_MESSAGE,
  }),
  true,
  'exact model-missing failure is actionable',
);
assertEqual(
  isVoiceActivationModelMissing({ phase: 'failed', message: 'permission denied' }),
  false,
  'unrelated failure is not a model-missing failure',
);
assertEqual(
  isVoiceActivationModelMissing({ phase: 'unavailable', message: 'host unsupported' }),
  false,
  'unavailable runtime is not a model-missing failure',
);
for (const phase of [
  'disabled',
  'starting',
  'listening_for_wake',
  'listening_for_finish',
] as const) {
  assertEqual(
    isVoiceActivationModelMissing({ phase }),
    false,
    `${phase} does not show a model action`,
  );
}
assertEqual(isVoiceActivationModelMissing(null), false, 'null state is ignored');
assertEqual(isVoiceActivationModelMissing(undefined), false, 'missing state is ignored');
