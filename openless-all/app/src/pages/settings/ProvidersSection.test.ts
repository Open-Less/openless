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

const opencodePreset = LLM_PRESETS.find(p => p.id === 'opencode');

if (!opencodePreset) {
  throw new Error('OpenCode LLM preset is missing');
}

if (opencodePreset.baseUrl !== 'https://opencode.ai/zen/v1') {
  throw new Error(`unexpected OpenCode base URL: ${opencodePreset.baseUrl}`);
}

if (opencodePreset.modelPlaceholder !== 'deepseek-v4-flash') {
  throw new Error(`unexpected OpenCode default model: ${opencodePreset.modelPlaceholder}`);
}

const customPresetIndex = LLM_PRESETS.findIndex(p => p.id === 'custom');
const opencodePresetIndex = LLM_PRESETS.findIndex(p => p.id === 'opencode');
if (customPresetIndex < 0 || opencodePresetIndex < 0 || opencodePresetIndex >= customPresetIndex) {
  throw new Error('OpenCode preset must sit before the custom fallback preset');
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
