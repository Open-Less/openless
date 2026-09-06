import { LLM_LABELS } from './ProvidersSection';
import { ASR_LABELS } from './shared';
import { presetsFor } from './ChannelList';

const atlascloudPreset = LLM_LABELS.find(p => p.id === 'atlascloud');

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
}]);

if (coreAsr.length !== 1 || coreAsr[0].authRequirement !== 'endpoint_model_optional_api_key') {
  throw new Error('Core provider descriptor must replace the browser fallback in the channel picker');
}
