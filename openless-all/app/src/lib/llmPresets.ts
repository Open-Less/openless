export const DEFAULT_LLM_PRESET_ID = 'deepseek';

export const LLM_PRESETS = [
  {
    id: 'deepseek',
    nameKey: 'deepseek',
    baseUrl: 'https://api.deepseek.com/v1',
    modelPlaceholder: 'deepseek-v4-flash',
  },
  {
    id: 'siliconflow',
    nameKey: 'siliconflow',
    baseUrl: 'https://api.siliconflow.cn/v1',
    modelPlaceholder: 'Qwen/Qwen2.5-7B-Instruct',
  },
  {
    id: 'openai',
    nameKey: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    modelPlaceholder: 'gpt-4o',
  },
  {
    id: 'custom',
    nameKey: 'custom',
    baseUrl: '',
    modelPlaceholder: '',
  },
] as const;

export type LlmPresetId = typeof LLM_PRESETS[number]['id'];
