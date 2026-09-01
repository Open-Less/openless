import { LLM_PRESETS, prioritizeOrcaRouterModels } from './ProvidersSection';
import { ASR_PRESETS } from './shared';

const atlascloudPreset = LLM_PRESETS.find(p => p.id === 'atlascloud');

if (!atlascloudPreset) {
  throw new Error('Atlas Cloud LLM preset is missing');
}

if (atlascloudPreset.baseUrl !== 'https://api.atlascloud.ai/v1') {
  throw new Error(`unexpected Atlas Cloud base URL: ${atlascloudPreset.baseUrl}`);
}

if (atlascloudPreset.modelPlaceholder !== 'qwen/qwen3.5-flash') {
  throw new Error(`unexpected Atlas Cloud default model: ${atlascloudPreset.modelPlaceholder}`);
}

const orcarouterPreset = LLM_PRESETS.find(p => p.id === 'orcarouter');

if (!orcarouterPreset) {
  throw new Error('OrcaRouter LLM preset is missing');
}

if (orcarouterPreset.baseUrl !== 'https://api.orcarouter.ai/v1') {
  throw new Error(`unexpected OrcaRouter base URL: ${orcarouterPreset.baseUrl}`);
}

if (orcarouterPreset.modelPlaceholder !== 'orcarouter/fusion-flash') {
  throw new Error(`unexpected OrcaRouter default model: ${orcarouterPreset.modelPlaceholder}`);
}

const orcarouterAsrPreset = ASR_PRESETS.find(p => p.id === 'orcarouter');

if (!orcarouterAsrPreset) {
  throw new Error('OrcaRouter ASR preset is missing');
}

if (orcarouterAsrPreset.baseUrl !== 'https://api.orcarouter.ai/v1') {
  throw new Error(`unexpected OrcaRouter ASR base URL: ${orcarouterAsrPreset.baseUrl}`);
}

if (orcarouterAsrPreset.model !== 'google/gemini-2.5-flash') {
  throw new Error(`unexpected OrcaRouter ASR default model: ${orcarouterAsrPreset.model}`);
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

const openAiCompatiblePreset = ASR_PRESETS.find(p => p.id === 'openai-compatible');

if (!openAiCompatiblePreset) {
  throw new Error('Custom OpenAI-compatible ASR preset is missing');
}

if (openAiCompatiblePreset.baseUrl !== '' || openAiCompatiblePreset.model !== '') {
  throw new Error(
    `Custom OpenAI-compatible ASR preset must have no defaults (got baseUrl=${openAiCompatiblePreset.baseUrl}, model=${openAiCompatiblePreset.model})`,
  );
}

const zenmuxPreset = ASR_PRESETS.find(p => p.id === 'zenmux');

if (!zenmuxPreset) {
  throw new Error('ZenMux ASR preset is missing');
}

if (zenmuxPreset.baseUrl !== 'https://zenmux.ai/api/v1') {
  throw new Error(`unexpected ZenMux base URL: ${zenmuxPreset.baseUrl}`);
}

if (zenmuxPreset.model !== 'qwen/qwen3-asr-flash') {
  throw new Error(`unexpected ZenMux default model: ${zenmuxPreset.model}`);
}
