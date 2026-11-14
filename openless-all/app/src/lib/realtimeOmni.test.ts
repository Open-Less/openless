import {
  defaultRealtimeOptions,
  fillEmptyOmniDefaults,
  isRealtimeOmniProvider,
  loadRealtimeOptions,
  parseRealtimeOptions,
  realtimeOmniProviders,
  realtimeOptionsAccount,
  saveRealtimeOptions,
  serializeRealtimeOptions,
} from './realtimeOmni';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';
import { en } from '../i18n/en';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { es } from '../i18n/es';
import { fr } from '../i18n/fr';
import { de } from '../i18n/de';
const assert = (condition: unknown, message: string) => {
  if (!condition) throw new Error(message);
};
for (const provider of realtimeOmniProviders)
  assert(isRealtimeOmniProvider(provider), 'Known profile');
assert(!isRealtimeOmniProvider('gemini'), 'Gemini must keep its dedicated protocol');
assert(!isRealtimeOmniProvider('custom'), 'HTTP custom must not acquire Realtime behavior');
for (const locale of [zhCN, zhTW, en, ja, ko, es, fr, de]) {
  const strings = locale.settings.providers.realtimeOptions;
  assert(
    Object.values(strings).every((value) => value.trim().length > 0),
    'Localized form has no empty copy',
  );
  assert(
    strings.none.includes('none') && strings.fallbackHint.includes('none'),
    'Experimental effort and fallback remain explicit',
  );
  assert(
    strings.openaiNotice.includes('24') && strings.dashscopeNotice.includes('16'),
    'Protocol-specific upload rates are explained',
  );
  assert(
    Boolean(locale.settings.providers.presets.omniDashscopeRealtimeLegacy),
    'Legacy protocol has a separate label',
  );
}
assert(
  JSON.stringify(parseRealtimeOptions(null)) === JSON.stringify(defaultRealtimeOptions),
  'Legacy default',
);
for (const bad of [
  'null',
  '[]',
  '{"authMode":null}',
  '{"reasoningFallback":0}',
  '{"extraHeaders":"secret-marker"}',
  '{"reasoningEffort":"enabled"}',
]) {
  let rejected = false;
  try {
    parseRealtimeOptions(bad);
  } catch (e) {
    rejected = true;
    assert(!String(e).includes('secret-marker'), 'No raw configuration in errors');
  }
  assert(rejected, 'Reject malformed or unknown configuration');
}
const options = { authMode: 'headers', reasoningEffort: 'none', reasoningFallback: false } as const;
assert(
  JSON.stringify(parseRealtimeOptions(serializeRealtimeOptions(options))) ===
    JSON.stringify(options),
  'Typed roundtrip',
);
async function main() {
  const values = new Map<string, string>();
  const read = async (account: string, provider?: string) =>
    values.get(`${provider}:${account}`) ?? null;
  const write = async (account: string, value: string, provider?: string) => {
    values.set(`${provider}:${account}`, value);
  };
  await saveRealtimeOptions('openai-realtime', options, write);
  assert(
    (await loadRealtimeOptions('openai-realtime', read)).authMode === 'headers',
    'Read explicit provider',
  );
  assert(
    (await loadRealtimeOptions('dashscope-realtime', read)).authMode === 'bearer',
    'No cross-profile leak',
  );
  assert(!values.has('openai-realtime:omni.extra_headers'), 'Options never become headers');
  await write('omni.endpoint', 'https://gateway.example/prefix', 'openai-realtime');
  await write('omni.model', 'custom-model', 'openai-realtime');
  await write('omni.api_key', 'fixture-key', 'openai-realtime');
  await write('omni.extra_headers', '{"x-tenant":"fixture"}', 'openai-realtime');
  await fillEmptyOmniDefaults(
    'openai-realtime',
    { baseUrl: 'https://api.openai.com/v1', modelPlaceholder: 'gpt-realtime-2.1' },
    read,
    write,
  );
  assert(
    (await read('omni.endpoint', 'openai-realtime')) === 'https://gateway.example/prefix',
    'Preserve gateway on return',
  );
  assert((await read('omni.model', 'openai-realtime')) === 'custom-model', 'Preserve custom model');
  assert((await read('omni.api_key', 'openai-realtime')) === 'fixture-key', 'Preserve main key');
  assert(
    (await read('omni.extra_headers', 'openai-realtime')) === '{"x-tenant":"fixture"}',
    'Preserve custom headers',
  );
  assert(
    (await read(realtimeOptionsAccount, 'openai-realtime')) === serializeRealtimeOptions(options),
    'Preserve typed options',
  );
  await fillEmptyOmniDefaults(
    'dashscope-realtime',
    { baseUrl: 'wss://dashscope.example/realtime', modelPlaceholder: 'qwen-model' },
    read,
    write,
  );
  assert(
    (await read('omni.model', 'dashscope-realtime')) === 'qwen-model',
    'Initialize empty profile independently',
  );
  console.log('realtimeOmni tests passed');
}
void main();
