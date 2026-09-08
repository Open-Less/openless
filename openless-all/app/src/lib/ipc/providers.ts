import { invokeOrMock } from './shared'

export type ProviderKind = 'asr' | 'llm' | 'omni'
export type LlmRequestFormat = 'chat_completions' | 'responses' | 'messages'

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
  defaultRequestFormat: LlmRequestFormat | null
  supportedRequestFormats: LlmRequestFormat[]
}

/** Core owns protocol, defaults, and credential requirements. */
export function listProviderDescriptors(kind: ProviderKind): Promise<ProviderDescriptor[]> {
  return invokeOrMock('list_provider_descriptors', { kind }, () => kind === 'llm' ? [
    ['opencode', 'opencode', 'chat_completions'],
    ['orcarouter', 'orcarouter', 'chat_completions'],
    ['custom', 'customChatCompletions', 'chat_completions'],
    ['custom_responses', 'customResponses', 'responses'],
    ['custom_messages', 'customMessages', 'messages'],
  ].map(([providerType, labelKey, format]) => ({
    kind, providerType, labelKey,
    defaultEndpoint: providerType === 'orcarouter' ? 'https://api.orcarouter.ai/v1' : providerType === 'opencode' ? 'https://opencode.ai/zen/v1' : null,
    defaultModel: providerType === 'orcarouter' ? 'orcarouter/fusion-flash' : providerType === 'opencode' ? 'deepseek-v4-flash' : null,
    authRequirement: 'api_key_unless_custom_endpoint', validationProbe: 'llm_text', staticModels: [],
    defaultRequestFormat: format as LlmRequestFormat,
    supportedRequestFormats: ['chat_completions', 'responses', 'messages'],
  })) : kind === 'asr' ? [{
    kind, providerType: 'orcarouter', labelKey: 'orcarouter',
    defaultEndpoint: 'https://api.orcarouter.ai/v1', defaultModel: 'google/gemini-2.5-flash',
    authRequirement: 'api_key', validationProbe: 'asr_silence', staticModels: [],
    defaultRequestFormat: null, supportedRequestFormats: [],
  }] : [])
}
