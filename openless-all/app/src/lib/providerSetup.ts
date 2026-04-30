import type { CredentialsStatus } from './types';

export const PROVIDER_SETUP_PROMPT_SEEN_KEY = 'ol.providerSetupPromptSeen';

export function areProvidersConfigured(credentials: CredentialsStatus): boolean {
  return credentials.volcengineConfigured && credentials.arkConfigured;
}

export function shouldShowProviderSetupPrompt(
  credentials: CredentialsStatus,
  promptSeenValue: string | null,
): boolean {
  return !areProvidersConfigured(credentials) && promptSeenValue !== '1';
}
