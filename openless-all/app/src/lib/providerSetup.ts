import type { CredentialsStatus } from './types';

export const PROVIDER_SETUP_PROMPT_DEFERRED_KEY = 'ol.providerSetupPromptDeferredThisSession';

export function areProvidersConfigured(credentials: CredentialsStatus): boolean {
  // Multimodal (Omni) mode: only requires the multimodal model to be configured; the legacy
  // ASR/LLM pair takes no part in this mode.
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
