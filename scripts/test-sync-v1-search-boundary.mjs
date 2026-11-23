// Execute the real published UI codec with synthetic secrets and no native IPC.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { stripTypeScriptTypes } from 'node:module';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const root = fileURLToPath(new URL('../', import.meta.url));
const path = 'openless-all/app/src/features/settings/sections/WebSearchSection.tsx';
for (const revision of ['872c3293a8c8b10760017459a280526f3ebdf625', '51b65224']) {
  const source = execFileSync('git', ['show', `${revision}:${path}`], { cwd: root, encoding: 'utf8' });
  const codec = source.slice(source.indexOf('export interface WebSearchConfig'), source.indexOf('export function WebSearchSection'));
  const script = stripTypeScriptTypes(codec).replace(/^export /gm, '');
  const original = { provider: 'tavily', apiKey: 'fixture-active', endpoint: '',
    providerConfigs: { brave: { apiKey: 'fixture-inactive', endpoint: 'https://search.example.test' } } };
  let saved;
  const context = vm.createContext({
    readCredential: async () => JSON.stringify(original),
    setCredential: async (account, value) => { assert.equal(account, 'web_search.config'); saved = JSON.parse(value); },
  });
  vm.runInContext(script, context);
  const loaded = await context.loadSearchConfig();
  assert.equal(loaded.providerConfigs, undefined);
  loaded.endpoint = 'https://edited.example.test';
  await context.saveSearchConfig(loaded);
  assert.deepEqual(saved, { provider: 'tavily', apiKey: 'fixture-active', endpoint: 'https://edited.example.test' });
  console.log(`PASS expected extension loss in real UI codec: ${revision}`);
}
