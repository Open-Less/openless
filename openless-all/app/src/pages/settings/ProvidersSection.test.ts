import { LLM_LABELS, prioritizeOrcaRouterModels } from './ProvidersSection';
import { ASR_LABELS } from './shared';
import { presetsFor } from './ChannelList';

const atlascloudPreset = LLM_LABELS.find(p => p.id === 'atlascloud');
if (LLM_LABELS.find(p => p.id === 'opencode')?.nameKey !== 'opencode') {
  throw new Error('OpenCode LLM label is missing');
}

if (!atlascloudPreset) {
  throw new Error('Atlas Cloud LLM preset is missing');
}

const openAiCompatiblePreset = ASR_LABELS.find(p => p.id === 'openai-compatible');

if (!openAiCompatiblePreset) {
  throw new Error('Custom OpenAI-compatible ASR preset is missing');
}

const zenmuxPreset = ASR_LABELS.find(p => p.id === 'zenmux');

if (!zenmuxPreset) {
  throw new Error('ZenMux ASR preset is missing');
}

const coreAsr = presetsFor('asr', 'win', true, undefined, [{
  kind: 'asr',
  providerType: 'openai-compatible',
  labelKey: 'asrOpenAiCompatible',
  defaultEndpoint: null,
  defaultModel: null,
  authRequirement: 'endpoint_model_optional_api_key',
  validationProbe: 'asr_silence',
  staticModels: [],
  defaultRequestFormat: null,
  supportedRequestFormats: [],
}]);

if (coreAsr.length !== 1 || coreAsr[0].authRequirement !== 'endpoint_model_optional_api_key') {
  throw new Error('Core provider descriptor must replace the browser fallback in the channel picker');
}

const prioritizedOrcaRouterModels = prioritizeOrcaRouterModels([
  'openai/gpt-5-mini',
  'orcarouter/fusion-mini',
  'anthropic/claude-haiku-4.5',
  'orcarouter/fusion-flash',
]);

if (prioritizedOrcaRouterModels.join(',') !== [
  'orcarouter/fusion-flash',
  'orcarouter/fusion-mini',
  'anthropic/claude-haiku-4.5',
  'openai/gpt-5-mini',
].join(',')) {
  throw new Error(`unexpected OrcaRouter model ordering: ${prioritizedOrcaRouterModels.join(',')}`);
}


for (const labels of [LLM_LABELS, ASR_LABELS]) {
  if (!labels.some(label => label.id === 'orcarouter' && label.nameKey === 'orcarouter')) {
    throw new Error('OrcaRouter provider label is missing');
  }
}
