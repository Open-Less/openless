import { isLocalAsrModelSupportedOnOs } from './localAsr';

function assertEqual(actual: boolean, expected: boolean, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

for (const os of ['win', 'android'] as const) {
  assertEqual(isLocalAsrModelSupportedOnOs('qwen3-asr-0.6b', os), false, `Qwen is hidden on ${os}`);
  assertEqual(
    isLocalAsrModelSupportedOnOs('whisper-large-v3-turbo', os),
    false,
    `Whisper is hidden on ${os}`,
  );
}

assertEqual(
  isLocalAsrModelSupportedOnOs('qwen3-asr-0.6b', 'mac'),
  true,
  'Qwen is available on macOS',
);
assertEqual(
  isLocalAsrModelSupportedOnOs('qwen3-asr-0.6b', 'linux'),
  true,
  'Qwen is available on Linux',
);
assertEqual(
  isLocalAsrModelSupportedOnOs('whisper-large-v3-turbo', 'mac'),
  true,
  'Whisper is available on macOS',
);
