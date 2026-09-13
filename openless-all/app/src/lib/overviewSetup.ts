import { areProvidersConfigured } from './providerSetup';
import type { CredentialsStatus } from './types';

export type OverviewSettingsSection = 'general' | 'services' | 'privacy' | 'shortcuts';
type SetupStep =
  | 'loading'
  | 'unavailable'
  | 'services'
  | 'permissions'
  | 'shortcuts'
  | 'recording'
  | 'tryDictation';

interface OverviewProvider {
  kind: 'asr' | 'llm' | 'omni';
  id: string | null;
  configured: boolean;
}

interface OverviewSetup {
  step: SetupStep;
  action: OverviewSettingsSection | 'refresh' | null;
  providers: OverviewProvider[];
}

/** Configuration is not a connectivity test or a permission check. Keep those
 * diagnostics in the existing settings pages instead of inferring success here. */
export function getOverviewSetup({
  credentials,
  loading,
  error,
  omniProvider,
  desktop,
  hotkeyAvailable,
  hasShortcut,
}: {
  credentials: CredentialsStatus | null;
  loading: boolean;
  error: boolean;
  omniProvider?: string | null;
  desktop: boolean;
  hotkeyAvailable: boolean | null;
  hasShortcut: boolean;
}): OverviewSetup {
  if (loading) return { step: 'loading', action: null, providers: [] };
  if (error || !credentials) return { step: 'unavailable', action: 'refresh', providers: [] };

  const providers: OverviewProvider[] =
    credentials.pipelineMode === 'multimodal'
      ? [
          {
            kind: 'omni',
            id: omniProvider || null,
            configured: credentials.omniConfigured === true,
          },
        ]
      : [
          {
            kind: 'asr',
            id: credentials.activeAsrProvider,
            configured: credentials.asrConfigured ?? credentials.volcengineConfigured,
          },
          {
            kind: 'llm',
            id: credentials.activeLlmProvider,
            configured: credentials.llmConfigured ?? credentials.arkConfigured,
          },
        ];

  if (!areProvidersConfigured(credentials))
    return { step: 'services', action: 'services', providers };
  if (!desktop) return { step: 'recording', action: 'general', providers };
  if (hotkeyAvailable === false) return { step: 'permissions', action: 'privacy', providers };
  if (hotkeyAvailable === null) return { step: 'recording', action: 'general', providers };
  if (!hasShortcut) return { step: 'shortcuts', action: 'shortcuts', providers };
  return { step: 'tryDictation', action: 'general', providers };
}
