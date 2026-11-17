import { listChannels, readCredential, setCredential } from './ipc';
import { isOfficialDeepSeekEndpoint } from './deepseekEndpoint';
export const DEEPSEEK_SUITE_ACCOUNT = 'deepseek_suite.config';
export interface DeepSeekSuiteConfig {
  apiKey: string;
  voice: 'stella' | 'tide';
}
export const defaultDeepSeekSuiteConfig: DeepSeekSuiteConfig = { apiKey: '', voice: 'stella' };
export function saveDeepSeekSuiteConfig(config: DeepSeekSuiteConfig): Promise<void> {
  return setCredential(DEEPSEEK_SUITE_ACCOUNT, JSON.stringify(config));
}
interface Reader {
  readCredential: typeof readCredential;
  listChannels: typeof listChannels;
  setCredential: typeof setCredential;
}
/** Seed a separate credential only once; later edits never follow a vendor channel. */
export async function ensureDeepSeekSuiteConfig(
  api: Reader = { readCredential, listChannels, setCredential },
): Promise<DeepSeekSuiteConfig> {
  const stored = await api.readCredential(DEEPSEEK_SUITE_ACCOUNT);
  if (stored !== null) return { ...defaultDeepSeekSuiteConfig, ...JSON.parse(stored) };
  const channels = (await api.listChannels('llm'))
    .filter((c) => c.enabled && c.providerType === 'deepseek')
    .sort((a, b) => a.order - b.order);
  for (const channel of channels) {
    const endpoint = (await api.readCredential('ark.endpoint', channel.id)) ?? '';
    if (!isOfficialDeepSeekEndpoint(endpoint)) continue;
    const key = (await api.readCredential('ark.api_key', channel.id))?.trim();
    if (!key) continue;
    const config = { ...defaultDeepSeekSuiteConfig, apiKey: key };
    await api.setCredential(DEEPSEEK_SUITE_ACCOUNT, JSON.stringify(config));
    return config;
  }
  return { ...defaultDeepSeekSuiteConfig };
}
