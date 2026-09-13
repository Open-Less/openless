import type { CredentialsStatus } from '../types';
import { invokeOrMock, isTauri } from './shared';
import { mockCredentialsStatus, mockCredentialValues } from './mock-data';
import { invalidateMockChannelTest } from './channels';
import { listProviderDescriptors, type LlmRequestFormat } from './providers';

export interface ProviderCheckResult {
  ok: boolean;
}

export interface ProviderModelsResult {
  models: string[];
}

interface OrcaRouterCatalogModel {
  id?: string;
  supported_endpoint_types?: string[];
  architecture?: {
    input_modalities?: string[];
  };
}

export function filterOrcaRouterModels(
  models: OrcaRouterCatalogModel[],
  kind: 'llm' | 'asr',
  requestFormat: LlmRequestFormat = 'chat_completions',
): string[] {
  const endpointType =
    kind === 'asr'
      ? 'openai'
      : {
          chat_completions: 'openai',
          responses: 'openai-response',
          messages: 'anthropic',
        }[requestFormat];
  return models
    .filter((model) => model.supported_endpoint_types?.includes(endpointType) === true)
    .filter(
      (model) =>
        kind === 'llm' ||
        (model.id?.toLowerCase().startsWith('google/gemini') === true &&
          model.architecture?.input_modalities?.includes('audio') === true),
    )
    .map((model) => model.id?.trim() ?? '')
    .filter(Boolean);
}

export function getCredentials(): Promise<CredentialsStatus> {
  return invokeOrMock('get_credentials', undefined, () => mockCredentialsStatus);
}

export function setCredential(account: string, value: string, provider?: string): Promise<void> {
  return invokeOrMock('set_credential', { account, value, provider }, () => {
    mockCredentialValues.set(`${provider ?? ''}:${account}`, value);
    if (provider && account.startsWith('ark.')) invalidateMockChannelTest(provider);
  });
}

export function setActiveAsrProvider(provider: string): Promise<void> {
  return invokeOrMock('set_active_asr_provider', { provider }, () => undefined);
}

export function setActiveLlmProvider(provider: string): Promise<void> {
  return invokeOrMock('set_active_llm_provider', { provider }, () => undefined);
}

export function setActiveOmniProvider(provider: string): Promise<void> {
  return invokeOrMock('set_active_omni_provider', { provider }, () => undefined);
}

export function readCredential(account: string, provider?: string): Promise<string | null> {
  return invokeOrMock<string | null>(
    'read_credential',
    { account, provider },
    () => mockCredentialValues.get(`${provider ?? ''}:${account}`) ?? null,
  );
}

/** `channelId` 省略时测当前生效的渠道；卡片上的「测试连通」会带上那张卡片的 id。 */
export function validateProviderCredentials(
  kind: 'llm' | 'asr' | 'omni',
  channelId?: string,
): Promise<ProviderCheckResult> {
  return invokeOrMock('validate_provider_credentials', { kind, channelId }, () => ({
    ok: true,
  }));
}

export async function listProviderModels(
  kind: 'llm' | 'asr' | 'omni',
  channelId?: string,
  providerType?: string,
): Promise<ProviderModelsResult> {
  if (!isTauri) {
    const descriptor = (await listProviderDescriptors(kind)).find(
      (item) => item.providerType === providerType,
    );
    if (descriptor?.staticModels.length) return { models: descriptor.staticModels };
  }
  if (!isTauri && providerType === 'orcarouter' && (kind === 'llm' || kind === 'asr')) {
    const endpointAccount = kind === 'llm' ? 'ark.endpoint' : 'asr.endpoint';
    const endpoint = mockCredentialValues.get(`${channelId ?? ''}:${endpointAccount}`);
    if (endpoint && new URL(endpoint).hostname.toLowerCase() === 'api.orcarouter.ai') {
      const response = await fetch('/__openless_dev/orcarouter/models');
      if (!response.ok) {
        throw new Error(`OrcaRouter /models returned ${response.status}`);
      }
      const payload = (await response.json()) as { data?: OrcaRouterCatalogModel[] };
      const storedFormat = mockCredentialValues.get(`${channelId ?? ''}:ark.request_format`);
      const requestFormat: LlmRequestFormat =
        storedFormat === 'responses' || storedFormat === 'messages'
          ? storedFormat
          : 'chat_completions';
      return { models: filterOrcaRouterModels(payload.data ?? [], kind, requestFormat) };
    }
  }
  return invokeOrMock('list_provider_models', { kind, channelId }, () => ({
    models:
      kind === 'llm'
        ? ['gpt-4o', 'deepseek-v4-flash', 'deepseek-v4-pro']
        : kind === 'omni'
          ? ['gpt-4o-audio-preview', 'qwen3-omni-flash']
          : ['whisper-1'],
  }));
}
