import {
  areProvidersConfigured,
  shouldShowProviderSetupPrompt,
} from './providerSetup';

function assertEqual(actual: boolean, expected: boolean, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

assertEqual(
  areProvidersConfigured({ volcengineConfigured: true, arkConfigured: true }),
  true,
  'configured when ASR and LLM are both ready',
);

assertEqual(
  areProvidersConfigured({ volcengineConfigured: false, arkConfigured: true }),
  false,
  'not configured when ASR provider is missing',
);

assertEqual(
  areProvidersConfigured({ volcengineConfigured: true, arkConfigured: false }),
  false,
  'not configured when LLM provider is missing',
);

assertEqual(
  shouldShowProviderSetupPrompt(
    { volcengineConfigured: false, arkConfigured: false },
    null,
  ),
  true,
  'show first-run prompt when providers are missing and no prompt was seen',
);

assertEqual(
  shouldShowProviderSetupPrompt(
    { volcengineConfigured: false, arkConfigured: false },
    '1',
  ),
  false,
  'do not repeat first-run prompt after the user has deferred it in this session',
);

assertEqual(
  shouldShowProviderSetupPrompt(
    { volcengineConfigured: true, arkConfigured: true },
    null,
  ),
  false,
  'do not show prompt when providers are already configured',
);
