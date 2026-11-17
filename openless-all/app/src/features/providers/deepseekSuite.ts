import { listChannels, readCredential, setCredential } from '../../api/providers';
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
    // The channel can be edited while the asynchronous credential reads settle.
    // Check the identity and endpoint again before copying into the suite.
    const current = (await api.listChannels('llm')).find((item) => item.id === channel.id);
    if (
      !current?.enabled ||
      current.providerType !== 'deepseek' ||
      ((await api.readCredential('ark.endpoint', channel.id)) ?? '') !== endpoint
    )
      continue;
    const existing = await api.readCredential(DEEPSEEK_SUITE_ACCOUNT);
    if (existing !== null) return { ...defaultDeepSeekSuiteConfig, ...JSON.parse(existing) };
    const config = { ...defaultDeepSeekSuiteConfig, apiKey: key };
    await api.setCredential(DEEPSEEK_SUITE_ACCOUNT, JSON.stringify(config));
    return config;
  }
  return { ...defaultDeepSeekSuiteConfig };
}
