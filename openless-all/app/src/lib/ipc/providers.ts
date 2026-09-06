import { invokeOrMock } from './shared'

export type ProviderKind = 'asr' | 'llm' | 'omni'

export type AuthRequirement =
  | 'none'
  | 'api_key'
  | 'endpoint_model_optional_api_key'
  | 'api_key_unless_custom_endpoint'
  | 'volcengine'
  | 'xfyun'
  | 'o_auth'

export interface ProviderDescriptor {
  kind: ProviderKind
  providerType: string
  labelKey: string
  defaultEndpoint: string | null
  defaultModel: string | null
  authRequirement: AuthRequirement
  validationProbe: string
  staticModels: string[]
}

/** Core owns protocol, defaults, and credential requirements. */
export function listProviderDescriptors(kind: ProviderKind): Promise<ProviderDescriptor[]> {
  return invokeOrMock('list_provider_descriptors', { kind }, () => [])
}
