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
const realtime = omniDescriptors.find(
  (descriptor) => descriptor.providerType === 'openai-realtime',
);
const generatedRealtime = generated.omni.find(
  (descriptor) => descriptor.providerType === 'openai-realtime',
);
if (!realtime || !generatedRealtime)
  throw new Error('Native OpenAI Realtime Omni preset is missing');
if (
  Object.entries(realtime).some(
    ([key, value]) =>
      JSON.stringify(value) !==
      JSON.stringify(generatedRealtime[key as keyof typeof generatedRealtime]),
  )
)
  throw new Error('OpenAI Realtime preview catalogs must agree');
if (
  realtime.defaultEndpoint !== 'https://api.openai.com/v1' ||
  realtime.defaultModel !== 'gpt-realtime-2.1' ||
  realtime.authRequirement !== 'api_key' ||
  realtime.labelKey !== 'omniOpenaiRealtime'
)
  throw new Error('Realtime must use the explicit native Omni API-key preset');
const realtimeModels = await listProviderModels('omni', undefined, 'openai-realtime');
if (realtimeModels.models.join(',') !== 'gpt-realtime-2.1,gpt-realtime-2.1-mini')
  throw new Error('Realtime model picker must not expose unrelated OpenAI models as audio-capable');
if (
  omniDescriptors.find((descriptor) => descriptor.providerType === 'openai')?.defaultModel !==
  'gpt-4o-audio-preview'
)
  throw new Error('Adding Realtime must preserve existing OpenAI Omni configuration defaults');
if (
  (await listProviderDescriptors('asr')).some(
    (descriptor) => descriptor.providerType === 'openai-realtime',
  )
)
  throw new Error('Realtime must be offered as a native Omni provider, not a disguised ASR bridge');
console.log(
  'native Realtime Omni preview exposes scoped models and preserves OpenAI Chat defaults',
);
