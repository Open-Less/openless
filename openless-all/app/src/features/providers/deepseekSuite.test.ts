import { ensureDeepSeekSuiteConfig, DEEPSEEK_SUITE_ACCOUNT } from './deepseekSuite';
const values = new Map<string, string>();
const writes: { account: string; provider?: string }[] = [];
const channel = (id: string, order: number, enabled = true) => ({
  id,
  name: id,
  providerType: 'deepseek',
  order,
  enabled,
  lastTest: null,
});
const api = {
  listChannels: async () => [
    channel('disabled', 0, false),
    channel('gateway', 1),
    channel('default', 2),
    channel('backup', 3),
  ],
  readCredential: async (account: string, provider?: string) =>
    values.get(`${provider ?? ''}:${account}`) ?? null,
  setCredential: async (account: string, value: string, provider?: string) => {
    writes.push({ account, provider });
    values.set(`${provider ?? ''}:${account}`, value);
  },
};
const assert = (value: unknown, message: string) => {
  if (!value) throw new Error(message);
};
values.set('gateway:ark.endpoint', 'https://gateway.example/v1');
values.set('gateway:ark.api_key', 'gateway-secret');
values.set('default:ark.endpoint', 'https://api.deepseek.com/v1');
values.set('default:ark.api_key', 'traditional-secret');
values.set('backup:ark.api_key', 'backup-secret');
const first = await ensureDeepSeekSuiteConfig(api);
assert(
  first.apiKey === 'traditional-secret',
  'copy the first enabled official DeepSeek credential',
);
assert(
  writes.length === 1 && writes[0].account === DEEPSEEK_SUITE_ACCOUNT && !writes[0].provider,
  'write only the independent application credential',
);
values.set('default:ark.api_key', 'changed-traditional-secret');
assert(
  (await ensureDeepSeekSuiteConfig(api)).apiKey === 'traditional-secret',
  'later vendor changes cannot replace the copied key',
);
values.set(`:${DEEPSEEK_SUITE_ACCOUNT}`, JSON.stringify({ apiKey: 'suite-secret', voice: 'tide' }));
assert(
  (await ensureDeepSeekSuiteConfig(api)).apiKey === 'suite-secret',
  'keep the suite key independent',
);
assert(
  values.get('default:ark.api_key') === 'changed-traditional-secret',
  'preserve traditional key',
);
values.set(`:${DEEPSEEK_SUITE_ACCOUNT}`, JSON.stringify({ apiKey: '', voice: 'stella' }));
assert(
  (await ensureDeepSeekSuiteConfig(api)).apiKey === '',
  'an explicitly cleared suite key must not be silently recopied',
);
console.log('DeepSeek suite credential isolation passed');
