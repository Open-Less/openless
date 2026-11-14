import { listProviderDescriptors, type ProviderKind } from './providers';
import generated from './provider-descriptors.generated.json';

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
const deepseekAsr = (await listProviderDescriptors('asr')).find(
  (descriptor) => descriptor.providerType === 'deepseek',
);
if (
  deepseekAsr?.authRequirement !== 'api_key_without_model' ||
  deepseekAsr.defaultModel !== null ||
  deepseekAsr.defaultEndpoint !== 'https://api.deepseek.com/v1' ||
  !generated.asr.some((descriptor) => descriptor.providerType === 'deepseek')
) {
  throw new Error('DeepSeek ASR preview must expose key-only configuration without a model');
}
console.log('browser provider catalog exposes ASR, LLM and Omni editors');
