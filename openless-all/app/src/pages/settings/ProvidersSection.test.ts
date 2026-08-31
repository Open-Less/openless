import { LLM_PRESETS } from './ProvidersSection';
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

const tokenHubPreset = LLM_PRESETS.find(p => p.id === 'tencentTokenHub');

if (!tokenHubPreset) {
  throw new Error('Tencent Cloud TokenHub LLM preset is missing');
}

if (tokenHubPreset.baseUrl !== 'https://tokenhub.tencentmaas.com/v1') {
  throw new Error(`unexpected TokenHub base URL: ${tokenHubPreset.baseUrl}`);
}

if (tokenHubPreset.modelPlaceholder !== 'hy3') {
  throw new Error(`unexpected TokenHub default model: ${tokenHubPreset.modelPlaceholder}`);
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

const tencentCloudPreset = ASR_PRESETS.find(p => p.id === 'tencent-cloud');

if (!tencentCloudPreset) {
  throw new Error('Tencent Cloud ASR preset is missing');
}

if (tencentCloudPreset.baseUrl !== '' || tencentCloudPreset.model !== 'Hy-ASR-3.0-preview') {
  throw new Error(
    `unexpected Tencent Cloud ASR defaults: baseUrl=${tencentCloudPreset.baseUrl}, model=${tencentCloudPreset.model}`,
  );
}
