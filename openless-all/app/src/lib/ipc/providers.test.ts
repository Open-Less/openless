import { listProviderDescriptors, type ProviderKind } from './providers';
import generated from './provider-descriptors.generated.json';
import { listProviderModels } from './asr-credentials';

const preview = generated.llm.find((descriptor) => descriptor.providerType === 'lmstudio');
const lmstudio = (await listProviderDescriptors('llm')).find(
  (descriptor) => descriptor.providerType === 'lmstudio',
);
if (
  !preview ||
  !lmstudio ||
  Object.entries(lmstudio).some(
    ([key, value]) =>
      JSON.stringify(value) !== JSON.stringify(preview[key as keyof typeof preview]),
  )
)
  throw new Error('LM Studio preview snapshots must agree');

for (const kind of ['asr', 'llm', 'omni'] as ProviderKind[]) {
  const descriptors = await listProviderDescriptors(kind);
  if (!descriptors.length)
    throw new Error(
      `${kind}: browser preview must expose the Core provider catalog so its editor can load`,
    );
  const ids = new Set<string>();
  for (const descriptor of descriptors) {
    if (
      descriptor.kind !== kind ||
      !descriptor.providerType ||
      !descriptor.labelKey ||
      !descriptor.authRequirement
    ) {
      throw new Error(`${kind}: incomplete provider descriptor`);
    }
    if (ids.has(descriptor.providerType))
      throw new Error(`${kind}: duplicate provider ${descriptor.providerType}`);
    ids.add(descriptor.providerType);
  }
}
console.log('browser provider catalog exposes ASR, LLM and Omni editors');

const omniDescriptors = await listProviderDescriptors('omni');
const realtimeProfiles = [
  {
    id: 'openai-realtime',
    labelKey: 'omniOpenaiRealtime',
    endpoint: 'https://api.openai.com/v1',
    models: ['gpt-realtime-2.1', 'gpt-realtime-2.1-mini'],
  },
  {
    id: 'dashscope-realtime',
    labelKey: 'omniDashscopeRealtime',
    endpoint: 'wss://dashscope.aliyuncs.com/api-ws/v1/realtime',
    models: [
      'qwen3.5-omni-flash-realtime',
      'qwen3.5-omni-plus-realtime',
      'qwen3.8-omni-flash-realtime',
    ],
  },
  {
    id: 'dashscope-realtime-legacy',
    labelKey: 'omniDashscopeRealtimeLegacy',
    endpoint: 'wss://dashscope.aliyuncs.com/api-ws/v1/realtime',
    models: ['qwen3-omni-flash-realtime-2025-09-15'],
  },
];
for (const profile of realtimeProfiles) {
  const realtime = omniDescriptors.find((descriptor) => descriptor.providerType === profile.id);
  const generatedRealtime = generated.omni.find(
    (descriptor) => descriptor.providerType === profile.id,
  );
  if (!realtime || !generatedRealtime)
    throw new Error(`${profile.id}: native Realtime Omni preset is missing`);
  if (
    Object.entries(realtime).some(
      ([key, value]) =>
        JSON.stringify(value) !==
        JSON.stringify(generatedRealtime[key as keyof typeof generatedRealtime]),
    )
  )
    throw new Error(`${profile.id}: Realtime preview catalogs must agree`);
  if (
    realtime.defaultEndpoint !== profile.endpoint ||
    realtime.defaultModel !== profile.models[0] ||
    realtime.authRequirement !== 'endpoint_model_optional_api_key' ||
    realtime.labelKey !== profile.labelKey ||
    realtime.validationProbe !== 'omni_text'
  )
    throw new Error(`${profile.id}: typed Realtime authentication and defaults must be explicit`);
  const realtimeModels = await listProviderModels('omni', undefined, profile.id);
  if (realtimeModels.models.join(',') !== profile.models.join(','))
    throw new Error(`${profile.id}: model candidates must remain scoped to the chosen protocol`);
  if (
    (await listProviderDescriptors('asr')).some(
      (descriptor) => descriptor.providerType === profile.id,
    )
  )
    throw new Error(`${profile.id}: Realtime must be native Omni, not a disguised ASR bridge`);
}
if (
  omniDescriptors.find((descriptor) => descriptor.providerType === 'openai')?.defaultModel !==
  'gpt-4o-audio-preview'
)
  throw new Error('Adding Realtime must preserve existing OpenAI Omni configuration defaults');
console.log(
  'three native Realtime Omni previews expose typed auth, scoped models and preserve Chat defaults',
);
