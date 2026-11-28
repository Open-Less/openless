const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`Expected ${expected}, received ${actual}`);
  },
};
import { parseSearchConfig, selectSearchProvider } from './webSearchConfig';

let config = parseSearchConfig('{"provider":"tavily","apiKey":"tavily-key","endpoint":""}');
config = selectSearchProvider(config, 'hybrid');
assert.equal(config.apiKey, '');
config.apiKey = 'official-deepseek-key';
config = selectSearchProvider(config, 'searxng');
config.endpoint = 'https://search.example/search';
for (const provider of ['free', 'off', 'brave', 'bing', 'free_serp'] as const) {
  config = selectSearchProvider(config, provider);
}
config.apiKey = 'another-key';
config = parseSearchConfig(JSON.stringify(config));
config = selectSearchProvider(config, 'tavily');
assert.equal(config.apiKey, 'tavily-key');
config = selectSearchProvider(config, 'hybrid');
assert.equal(config.apiKey, 'official-deepseek-key');
config = selectSearchProvider(config, 'searxng');
assert.equal(config.endpoint, 'https://search.example/search');
assert.equal(config.apiKey, '');
assert.equal(parseSearchConfig('{"provider":"auto","apiKey":"legacy"}').provider, 'auto');
assert.equal(parseSearchConfig(null).provider, 'free');
console.log('Web Search provider configuration roundtrip passed');
