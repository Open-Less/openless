import type { OS } from '../../components/WindowChrome';
import type { ProviderDescriptor } from '../../lib/ipc';
import {
  channelTestMode,
  defaultChannelNameForProvider,
  presetsFor,
  shouldRecycleDraft,
} from './ChannelList';

const localProviders = [
  'local-qwen3',
  'local-qwen3-mlx',
  'local-qwen3-c',
  'local-whisper',
  'apple-speech',
  'foundry-local-whisper',
  'sherpa-onnx-local',
] as const;

const expectedByPlatform: Record<OS, readonly string[]> = {
  mac: ['local-qwen3-mlx', 'local-qwen3-c', 'local-whisper', 'apple-speech'],
  win: ['foundry-local-whisper', 'sherpa-onnx-local'],
  linux: ['local-qwen3-c'],
  android: [],
};

// Production receives this catalog from Core. The test supplies only the
// fields needed to verify Host-specific visibility; it deliberately contains
// no endpoint/model defaults that could become a second provider truth.
const descriptors: ProviderDescriptor[] = [
  'volcengine',
  'bailian-qwen3-realtime',
  'bailian-fun-asr-flash',
  ...localProviders,
].map((providerType) => ({
  kind: 'asr',
  providerType,
  labelKey: providerType,
  defaultEndpoint: null,
  defaultModel: null,
  authRequirement: localProviders.includes(providerType as (typeof localProviders)[number])
    ? 'none'
    : 'api_key',
  validationProbe: 'unsupported',
  staticModels: [],
  defaultRequestFormat: null,
  supportedRequestFormats: [],
}));

const asrPresets = (os: OS, supportsQwen3Mlx = true, currentProviderId?: string) =>
  presetsFor('asr', os, supportsQwen3Mlx, currentProviderId, descriptors);

for (const os of Object.keys(expectedByPlatform) as OS[]) {
  const ids = new Set(asrPresets(os).map((preset) => preset.id));
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
  if (ids.has('local-qwen3')) {
    throw new Error('legacy local-qwen3 must remain hidden from new channels');
  }
}

const intelMacIds = new Set(asrPresets('mac', false).map((preset) => preset.id));
if (intelMacIds.has('local-qwen3-mlx') || !intelMacIds.has('local-qwen3-c')) {
  throw new Error('Intel macOS must expose C/CPU Qwen3 but not MLX');
}

const legacyQwenEditIds = new Set(
  asrPresets('mac', true, 'local-qwen3').map((preset) => preset.id),
);
if (!legacyQwenEditIds.has('local-qwen3')) {
  throw new Error('editing a legacy local-qwen3 channel must keep its current option visible');
}

const legacyBailianEditIds = new Set(
  asrPresets('linux', true, 'bailian-qwen3-realtime').map((preset) => preset.id),
);
if (!legacyBailianEditIds.has('bailian-qwen3-realtime')) {
  throw new Error('editing a legacy Bailian channel must keep its current option visible');
}

const unknownEditIds = new Set(
  asrPresets('mac', true, 'unknown-provider').map((preset) => preset.id),
);
if (unknownEditIds.has('unknown-provider')) {
  throw new Error('unknown provider ids must not be injected into preset options');
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

if (defaultChannelNameForProvider('orcarouter', '') !== 'OrcaRouter') {
  throw new Error('a blank OrcaRouter channel must receive the OrcaRouter default name');
}
if (defaultChannelNameForProvider('orcarouter', 'My primary key') !== 'My primary key') {
  throw new Error('the OrcaRouter default name must not overwrite a custom channel name');
}
if (defaultChannelNameForProvider('openai', '') !== '') {
  throw new Error('non-OrcaRouter channel naming must remain unchanged');
}

for (const provider of ['local-qwen3', 'local-qwen3-mlx', 'local-qwen3-c', 'local-whisper']) {
  if (channelTestMode('asr', provider, 'unsupported') !== 'local-model') {
    throw new Error(`${provider} must use the local model channel test`);
  }
}
if (channelTestMode('asr', 'apple-speech', 'unsupported') !== 'unavailable') {
  throw new Error('Apple Speech must not expose a verification action without a native probe');
}
if (channelTestMode('asr', 'apple-speech', undefined) !== 'unavailable') {
  throw new Error('Apple Speech must stay unavailable while provider descriptors are loading');
}
if (channelTestMode('asr', 'apple-speech', 'asr_native_silence') !== 'provider') {
  throw new Error('Apple Speech must use the native provider probe when Core exposes it');
}
if (channelTestMode('asr', 'volcengine', 'asr_silence') !== 'provider') {
  throw new Error('cloud ASR providers must keep the generic provider validation path');
}

console.log('ChannelList platform filtering and draft lifecycle tests passed');
