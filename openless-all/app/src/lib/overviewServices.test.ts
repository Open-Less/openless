import { loadOverviewServiceDetails } from './overviewServices';
import type { ProviderDescriptor, ProviderKind } from './ipc';
import type { CredentialsStatus, UserPreferences } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const credentials: CredentialsStatus = {
  activeAsrProvider: 'bailian',
  activeLlmProvider: 'deepseek',
  pipelineMode: 'traditional',
  asrConfigured: true,
  llmConfigured: true,
  omniConfigured: false,
  volcengineConfigured: false,
  arkConfigured: true,
};
const reads: string[] = [];
const models = new Map([
  ['asr.model', ' qwen3-asr-flash '],
  ['ark.model_id', 'deepseek-chat'],
  ['omni.model', 'gemini-2.5-flash'],
]);
const reader = {
  listProviderDescriptors: async (_kind: ProviderKind): Promise<ProviderDescriptor[]> => [],
  readCredential: async (account: string, provider?: string) => {
    assert(provider === undefined, 'let the backend resolve the actual active channel');
    reads.push(account);
    return models.get(account) ?? null;
  },
};

const traditional = await loadOverviewServiceDetails(credentials, null, reader);
assert(traditional.asr?.model === 'qwen3-asr-flash', 'show the selected speech model');
assert(traditional.llm?.model === 'deepseek-chat', 'show the active polish model');
assert(!traditional.omni, 'traditional mode must not expose an inactive Omni model');
assert(reads.length === 2, 'read only active model fields, never keys or backup credentials');

models.set('ark.model_id', 'deepseek-reasoner');
const switched = await loadOverviewServiceDetails(credentials, null, reader);
assert(switched.llm?.model === 'deepseek-reasoner', 'refresh even when the vendor does not change');

const descriptor = {
  providerType: 'deepseek',
  defaultModel: 'default-model',
} as ProviderDescriptor;
models.delete('ark.model_id');
const defaults = await loadOverviewServiceDetails(credentials, null, {
  ...reader,
  listProviderDescriptors: async () => [descriptor],
});
assert(defaults.llm?.model === 'default-model', 'use Core defaults for an unset model field');

reads.length = 0;
const omni = await loadOverviewServiceDetails(
  { ...credentials, pipelineMode: 'multimodal', omniConfigured: true },
  { activeOmniProvider: 'gemini' } as UserPreferences,
  reader,
);
assert(omni.omni?.model === 'gemini-2.5-flash', 'show the active Omni model');
assert(!omni.asr && !omni.llm, 'Omni mode must not display inactive traditional models');
assert(reads.join() === 'omni.model', 'Omni must use its own model namespace');

for (const [provider, field] of [
  ['local-qwen3-mlx', 'localAsrActiveModel'],
  ['local-whisper', 'localWhisperActiveModel'],
  ['foundry-local-whisper', 'foundryLocalAsrModel'],
  ['sherpa-onnx-local', 'sherpaOnnxModel'],
] as const) {
  reads.length = 0;
  const local = await loadOverviewServiceDetails(
    { ...credentials, activeAsrProvider: provider },
    { [field]: 'local-model', activeAsrProvider: 'stale-provider' } as unknown as UserPreferences,
    reader,
  );
  assert(local.asr?.model === 'local-model', `${provider} must use its global local model`);
  assert(!reads.includes('asr.model'), 'local models are not stored per channel');
}

const failure = await loadOverviewServiceDetails(credentials, null, {
  ...reader,
  readCredential: async () => {
    throw new Error('read failed');
  },
});
assert(
  failure.llm?.error === true && failure.llm.model === null,
  'failed reads must clear model data',
);
assert(failure.asr?.error === true, 'read errors must not masquerade as unset models');

const fixed = await loadOverviewServiceDetails(
  { ...credentials, activeAsrProvider: 'apple-speech' },
  null,
  reader,
);
assert(fixed.asr?.model === null && !fixed.asr.error, 'fixed engines must not invent a model id');

console.log('overview service details tests passed');
