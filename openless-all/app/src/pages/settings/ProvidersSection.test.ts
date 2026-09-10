import { LLM_LABELS } from './ProvidersSection';
import { ASR_LABELS } from './shared';
import { presetsFor } from './ChannelList';
import { filterOrcaRouterModels } from '../../lib/ipc/asr-credentials';

const atlascloudPreset = LLM_LABELS.find(p => p.id === 'atlascloud');
if (LLM_LABELS.find(p => p.id === 'opencode')?.nameKey !== 'opencode') {
  throw new Error('OpenCode LLM label is missing');
}
if (LLM_LABELS.find(p => p.id === 'tencentTokenHub')?.nameKey !== 'tencentTokenHub') {
  throw new Error('Tencent Cloud TokenHub LLM label is missing');
}
if (ASR_LABELS.find(p => p.id === 'tencent-cloud')?.nameKey !== 'asrTencentCloud') {
  throw new Error('Tencent Cloud ASR label is missing');
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

const protocolCatalog = [
  { id: 'model/chat', supported_endpoint_types: ['openai'] },
  { id: 'model/responses', supported_endpoint_types: ['openai-response'] },
  { id: 'model/messages', supported_endpoint_types: ['anthropic'] },
  { id: 'model/all', supported_endpoint_types: ['openai', 'openai-response', 'anthropic'] },
  { id: 'model/unknown' },
];
for (const [format, expected] of [
  ['chat_completions', ['model/chat', 'model/all']],
  ['responses', ['model/responses', 'model/all']],
  ['messages', ['model/messages', 'model/all']],
] as const) {
  const actual = filterOrcaRouterModels(protocolCatalog, 'llm', format);
  if (actual.join(',') !== expected.join(',')) {
    throw new Error(`unexpected ${format} catalog: ${actual.join(',')}`);
  }
}

const asrCatalog = [
  { id: 'google/gemini-audio', supported_endpoint_types: ['openai'], architecture: { input_modalities: ['text', 'audio'] } },
  { id: 'google/gemini-text', supported_endpoint_types: ['openai'], architecture: { input_modalities: ['text'] } },
  { id: 'google/gemini-unknown', supported_endpoint_types: ['openai'] },
  { id: 'meta/audio', supported_endpoint_types: ['openai'], architecture: { input_modalities: ['audio'] } },
];
if (filterOrcaRouterModels(asrCatalog, 'asr').join(',') !== 'google/gemini-audio') {
  throw new Error('OrcaRouter ASR catalog must require declared Gemini audio input');
}

for (const labels of [LLM_LABELS, ASR_LABELS]) {
  if (!labels.some(label => label.id === 'orcarouter' && label.nameKey === 'orcarouter')) {
    throw new Error('OrcaRouter provider label is missing');
  }
}
