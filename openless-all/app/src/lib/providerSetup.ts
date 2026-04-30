import type { CredentialsStatus } from './types';

export const PROVIDER_SETUP_PROMPT_DEFERRED_KEY = 'ol.providerSetupPromptDeferredThisSession';

export function areProvidersConfigured(credentials: CredentialsStatus): boolean {
  return credentials.volcengineConfigured && credentials.arkConfigured;
}

export function shouldShowProviderSetupPrompt(
  credentials: CredentialsStatus,
  promptDeferredValue: string | null,
): boolean {
  return !areProvidersConfigured(credentials) && promptDeferredValue !== '1';
}
