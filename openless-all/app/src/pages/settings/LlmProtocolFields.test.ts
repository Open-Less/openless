import { protocolValidationError, type ProtocolValues } from './LlmProtocolFields';
import { listProviderDescriptors } from '../../lib/ipc/providers';
import { createChannel, deleteChannel, deleteChannelIfBlank, listChannels, recordChannelTest, setChannelProviderType } from '../../lib/ipc/channels';
import { readCredential, setCredential } from '../../lib/ipc/asr-credentials';
import { getSettings, setSettings } from '../../lib/ipc/settings';
import { presetsFor } from './ChannelList';

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const values: ProtocolValues = { 'ark.request_format': '', 'ark.messages_thinking': '', 'ark.max_tokens': '', 'ark.thinking_budget': '' };
assert(protocolValidationError(values, 'messages') === null, 'Core-compatible defaults must be valid');
assert(protocolValidationError({ ...values, 'ark.request_format': 'invalid' }, 'messages') === 'llmRequestFormatInvalid', 'Unknown formats must not silently fall back');
assert(protocolValidationError({ ...values, 'ark.max_tokens': '0' }, 'messages') === 'llmTokenLimitInvalid', 'Zero output tokens must be rejected');
assert(protocolValidationError({ ...values, 'ark.messages_thinking': 'budget', 'ark.max_tokens': '1024' }, 'messages') === 'llmThinkingBudgetInvalid', 'Fixed thinking must leave room for output');
assert(protocolValidationError({ ...values, 'ark.messages_thinking': 'budget', 'ark.max_tokens': '4096', 'ark.thinking_budget': '2048' }, 'messages') === null, 'Valid fixed budget must be accepted');

const descriptors = await listProviderDescriptors('llm');
const presets = presetsFor('llm', 'win', true, undefined, descriptors);
assert(presets.filter(p => p.id.startsWith('custom')).length === 3, 'Browser catalog should expose three compatibility presets');
const opencode = presets.find(p => p.id === 'opencode');
assert(opencode?.defaultRequestFormat === 'chat_completions'
  && opencode.defaultEndpoint === 'https://opencode.ai/zen/v1'
  && opencode.defaultModel === 'deepseek-v4-flash', 'OpenCode browser preset must retain Core defaults');
const tokenhub = presets.find(p => p.id === 'tencentTokenHub');
assert(tokenhub?.defaultRequestFormat === null
  && tokenhub.supportedRequestFormats?.length === 0,
  'TokenHub browser preset must stay fixed to Chat Completions');
assert(presets.find(p => p.id === 'custom_messages')?.defaultRequestFormat === 'messages', 'Picker must retain Core protocol defaults');
for (const preset of presets) {
  assert(preset.supportedRequestFormats?.length === (preset.defaultRequestFormat ? 3 : 0), 'Picker must retain Core protocol capabilities');
}
assert(presets.find(p => p.id === 'gemini')?.defaultRequestFormat == null, 'Native Gemini must not gain compatibility formats');
assert(presets.find(p => p.id === 'codex_oauth')?.staticModels?.length, 'OAuth static models must be available without an API key');

const first = await createChannel('llm', 'opencode', 'first');
const second = await createChannel('llm', 'custom', 'second');
await setCredential('ark.request_format', 'messages', first);
await setCredential('ark.api_key', 'fixture-key', first);
await setCredential('ark.endpoint', 'https://opencode.ai/zen/go/v1', first);
await setCredential('ark.model_id', 'deepseek-v4-flash', first);
for (const format of ['chat_completions', 'responses', 'messages']) {
  await setCredential('ark.request_format', format, first);
  assert(await readCredential('ark.request_format', first) === format, 'OpenCode format must survive reload');
  assert(await readCredential('ark.api_key', first) === 'fixture-key', 'Format changes preserve the key');
  assert(await readCredential('ark.model_id', first) === 'deepseek-v4-flash', 'Format changes preserve the model');
  assert(await readCredential('ark.endpoint', first) === 'https://opencode.ai/zen/go/v1', 'Format changes preserve the endpoint');
}
await recordChannelTest('llm', first, true, 1, null);
await setCredential('ark.model_id', 'new-model', first);
assert(await readCredential('ark.request_format', first) === 'messages', 'Changing model must preserve format');
assert(await readCredential('ark.request_format', second) === null, 'Formats must be scoped by channel');
assert((await listChannels('llm')).find(c => c.id === first)?.lastTest === null, 'Credential mutation invalidates old validation');
await setChannelProviderType('llm', first, 'custom_responses');
assert(await readCredential('ark.request_format', first) === null, 'Changing preset resets the format override');
assert(await readCredential('ark.api_key', first) === 'fixture-key', 'Changing preset preserves the key');
await recordChannelTest('llm', first, true, 1, null);
const settings = await getSettings();
const asrTestAt = (await listChannels('asr'))[0].lastTest?.at;
await setSettings({ ...settings, llmThinkingEnabled: !settings.llmThinkingEnabled });
assert((await listChannels('llm')).find(c => c.id === first)?.lastTest === null, 'Changing thinking invalidates LLM tests');
assert((await listChannels('asr'))[0].lastTest?.at === asrTestAt, 'Changing thinking preserves ASR tests');
await setSettings(settings);

const blank = await createChannel('llm', 'custom', '');
assert(await deleteChannelIfBlank('llm', blank), 'An empty browser draft should be recycled');
assert(!(await listChannels('llm')).some(channel => channel.id === blank), 'A recycled browser draft must leave no channel');
const configured = await createChannel('llm', 'custom', '');
await setCredential('ark.api_key', 'keep-me', configured);
assert(!(await deleteChannelIfBlank('llm', configured)), 'A configured browser draft must be preserved');
assert((await listChannels('llm')).some(channel => channel.id === configured), 'A configured browser draft must remain visible');
await deleteChannel('llm', configured);
await deleteChannel('llm', first);
await deleteChannel('llm', second);
