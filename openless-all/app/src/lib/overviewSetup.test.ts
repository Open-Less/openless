import { getOverviewSetup } from './overviewSetup';
import type { CredentialsStatus } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const configured: CredentialsStatus = {
  activeAsrProvider: 'local-whisper',
  activeLlmProvider: 'deepseek',
  pipelineMode: 'traditional',
  asrConfigured: true,
  llmConfigured: true,
  omniConfigured: false,
  volcengineConfigured: false,
  arkConfigured: false,
};
const input = {
  credentials: configured,
  loading: false,
  error: false,
  desktop: true,
  hotkeyAvailable: true,
  hasShortcut: true,
};

// No service recommendation or success badge may be derived from an unfinished read.
for (const credentials of [null, configured]) {
  const loading = getOverviewSetup({ ...input, credentials, loading: true });
  assert(
    loading.step === 'loading' && loading.providers.length === 0,
    'loading must hide stale readiness',
  );
}
const failed = getOverviewSetup({ ...input, error: true });
assert(
  failed.step === 'unavailable' && failed.action === 'refresh',
  'failed reads must offer retry',
);
assert(failed.providers.length === 0, 'a failed read must not present cached providers as current');
assert(
  getOverviewSetup({ ...input, credentials: null }).action === 'refresh',
  'missing status is unknown, not unconfigured',
);

const traditional = getOverviewSetup(input);
assert(
  traditional.step === 'tryDictation',
  'active local ASR must not depend on Volcengine credentials',
);
assert(
  traditional.providers[0].id === 'local-whisper' && traditional.providers[0].configured,
  'preserve the active local model provider',
);
assert(traditional.providers[1].id === 'deepseek', 'preserve the actual LLM provider');
for (const missing of ['asrConfigured', 'llmConfigured'] as const) {
  const setup = getOverviewSetup({ ...input, credentials: { ...configured, [missing]: false } });
  assert(
    setup.step === 'services' && setup.action === 'services',
    'an incomplete traditional pipeline must lead to services',
  );
}

const omni = getOverviewSetup({
  ...input,
  credentials: {
    ...configured,
    pipelineMode: 'multimodal',
    asrConfigured: false,
    llmConfigured: false,
    omniConfigured: true,
  },
  omniProvider: 'dashscope-omni',
});
assert(omni.step === 'tryDictation', 'Omni must not require inactive ASR/LLM credentials');
assert(
  omni.providers.length === 1 && omni.providers[0].kind === 'omni',
  'Omni mode must show only its active pipeline',
);
assert(
  omni.providers[0].id === 'dashscope-omni' && omni.providers[0].configured,
  'Omni must keep its selected provider and status',
);
const missingOmni = getOverviewSetup({
  ...input,
  credentials: { ...configured, pipelineMode: 'multimodal' },
});
assert(
  missingOmni.action === 'services',
  'configured traditional providers cannot satisfy a missing Omni model',
);
assert(missingOmni.providers[0].id === null, 'an unknown Omni provider must not be invented');

assert(
  getOverviewSetup({ ...input, hotkeyAvailable: false }).action === 'privacy',
  'an unavailable hotkey adapter must lead to diagnostics',
);
assert(
  getOverviewSetup({ ...input, hasShortcut: false }).action === 'shortcuts',
  'a missing recording shortcut must lead to shortcuts',
);
assert(
  getOverviewSetup({ ...input, hotkeyAvailable: null }).action === 'general',
  'unknown hotkey capability must not promise a working shortcut',
);
assert(
  getOverviewSetup({ ...input, hotkeyAvailable: null, hasShortcut: false }).action === 'general',
  'unread preferences must not be mistaken for a missing shortcut',
);
const phone = getOverviewSetup({
  ...input,
  desktop: false,
  hotkeyAvailable: false,
  hasShortcut: false,
});
assert(
  phone.step === 'recording' && phone.action === 'general',
  'mobile must not be directed to desktop shortcuts',
);

console.log('overview setup behavior tests passed');
