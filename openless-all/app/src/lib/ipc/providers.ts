import mockDescriptors from './mock-provider-descriptors.json';
import { invokeOrMock } from './shared';

export type ProviderKind = 'asr' | 'llm' | 'omni';
export type LlmRequestFormat = 'chat_completions' | 'responses' | 'messages';

export type AuthRequirement =
  | 'none'
  | 'api_key'
  | 'endpoint_model_optional_api_key'
  | 'api_key_unless_custom_endpoint'
  | 'volcengine'
  | 'xfyun'
  | 'tencent_cloud'
  | 'o_auth';

export interface ProviderDescriptor {
  kind: ProviderKind;
  providerType: string;
  labelKey: string;
  defaultEndpoint: string | null;
  endpointPresets?: { name: string; endpoint: string; modelsUrl?: string }[];
  defaultModel: string | null;
  authRequirement: AuthRequirement;
  validationProbe: string;
  staticModels: string[];
  defaultRequestFormat: LlmRequestFormat | null;
  supportedRequestFormats: LlmRequestFormat[];
}

/** Core owns protocol, defaults, and credential requirements. */
export function listProviderDescriptors(kind: ProviderKind): Promise<ProviderDescriptor[]> {
  // Snapshot of Core provider_rules::provider_descriptors (all three kinds).
  return invokeOrMock('list_provider_descriptors', { kind }, () =>
    (mockDescriptors as ProviderDescriptor[]).filter((descriptor) => descriptor.kind === kind),
  );
}
