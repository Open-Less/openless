import {
  DEFAULT_LLM_PRESET_ID,
  LLM_PRESETS,
} from './llmPresets';

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

function assert(condition: boolean, name: string) {
  if (!condition) {
    throw new Error(name);
  }
}

const presetIds: string[] = LLM_PRESETS.map(preset => preset.id);
assertEqual(DEFAULT_LLM_PRESET_ID, 'deepseek', 'DeepSeek is the default LLM preset');
assert(!presetIds.includes('ark'), 'Ark is not exposed as an LLM preset');

const deepSeek = LLM_PRESETS.find(preset => preset.id === 'deepseek');
assert(!!deepSeek, 'DeepSeek preset exists');
assertEqual(deepSeek?.baseUrl, 'https://api.deepseek.com/v1', 'DeepSeek uses the official base URL');
assertEqual(
  deepSeek?.modelPlaceholder,
  'deepseek-v4-flash',
  'DeepSeek default model tracks the current API model name',
);
