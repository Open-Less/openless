import type { CredentialsStatus } from './types';

export const PROVIDER_SETUP_PROMPT_DEFERRED_KEY = 'ol.providerSetupPromptDeferredThisSession';

export function areProvidersConfigured(credentials: CredentialsStatus): boolean {
  // 多模态（Omni）模式：只要求多模态模型已配置；传统 ASR/LLM 两套在该模式下不参与。
  if (credentials.pipelineMode === 'multimodal') {
    return credentials.omniConfigured === true;
  }
  const asrConfigured = credentials.asrConfigured ?? credentials.volcengineConfigured;
  const llmConfigured = credentials.llmConfigured ?? credentials.arkConfigured;
  return asrConfigured && llmConfigured;
}

export function shouldShowProviderSetupPrompt(
  credentials: CredentialsStatus,
  promptDeferredValue: string | null,
): boolean {
  return !areProvidersConfigured(credentials) && promptDeferredValue !== '1';
}
