import type { OS } from '../../components/WindowChrome';
import { presetsFor, shouldRecycleDraft } from './ChannelList';

const localProviders = [
  'local-qwen3',
  'apple-speech',
  'foundry-local-whisper',
  'sherpa-onnx-local',
] as const;

const expectedByPlatform: Record<OS, readonly string[]> = {
  mac: ['local-qwen3', 'apple-speech'],
  win: ['foundry-local-whisper', 'sherpa-onnx-local'],
  linux: [],
  android: [],
};

for (const os of Object.keys(expectedByPlatform) as OS[]) {
  const ids = new Set(presetsFor('asr', os).map(preset => preset.id));
  const expected = new Set(expectedByPlatform[os]);

  for (const provider of localProviders) {
    if (ids.has(provider) !== expected.has(provider)) {
      throw new Error(`${provider} visibility is incorrect on ${os}`);
    }
  }

  if (!ids.has('volcengine')) {
    throw new Error(`cloud ASR providers must remain visible on ${os}`);
  }
  if (ids.has('bailian-qwen3-realtime') || ids.has('bailian-fun-asr-flash')) {
    throw new Error(`legacy Bailian aliases must remain hidden on ${os}`);
  }
}

if (!shouldRecycleDraft('draft-1', false)) {
  throw new Error('an untouched draft must be recycled');
}
if (shouldRecycleDraft('draft-1', true)) {
  throw new Error('a touched draft must be preserved');
}
if (shouldRecycleDraft(null, false)) {
  throw new Error('an existing channel must never enter draft cleanup');
}

console.log('ChannelList platform filtering and draft lifecycle tests passed');
