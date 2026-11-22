export interface SearchServiceConfig {
  apiKey: string;
  endpoint: string;
}

export interface WebSearchConfig extends SearchServiceConfig {
  provider:
    'auto' | 'free' | 'hybrid' | 'free_serp' | 'bing' | 'tavily' | 'brave' | 'searxng' | 'off';
  providerConfigs?: Partial<Record<WebSearchConfig['provider'], SearchServiceConfig>>;
}

export const defaultSearchConfig: WebSearchConfig = {
  provider: 'free',
  apiKey: '',
  endpoint: '',
};

export function parseSearchConfig(raw: string | null): WebSearchConfig {
  const parsed = raw ? JSON.parse(raw) : {};
  return {
    ...parsed,
    provider: parsed.provider ?? 'free',
    apiKey: parsed.apiKey ?? '',
    endpoint: parsed.endpoint ?? '',
  };
}

export function selectSearchProvider(
  config: WebSearchConfig,
  provider: WebSearchConfig['provider'],
): WebSearchConfig {
  const providerConfigs = {
    ...config.providerConfigs,
    [config.provider]: { apiKey: config.apiKey, endpoint: config.endpoint },
  };
  return {
    ...config,
    provider,
    providerConfigs,
    ...(providerConfigs[provider] ?? { apiKey: '', endpoint: '' }),
  };
}
