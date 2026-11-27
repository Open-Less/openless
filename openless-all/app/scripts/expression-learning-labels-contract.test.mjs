import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (name) => readFileSync(new URL(`../src/i18n/${name}.ts`, import.meta.url), 'utf8');
const supported = read('index').match(/export const SUPPORTED_LOCALES = \[([^\]]+)\]/);
assert.ok(supported, 'expected the app’s supported locale list');
const locales = [...supported[1].matchAll(/'([^']+)'/g)].map(([, locale]) => locale);
assert.ok(locales.length > 0, 'expected at least one supported locale');

for (const locale of locales) {
  const section = read(locale).match(/^\s{4}expressionLearning: \{\n([\s\S]*?)^\s{4}\},/m);
  assert.ok(section, `${locale}: missing expression learning messages`);
  for (const feature of ['listUsage', 'courteousTone', 'conciseWording']) {
    for (const direction of ['More', 'Less']) {
      const key = `${feature}${direction}`;
      const entries = [...section[1].matchAll(new RegExp(`^\\s+${key}: '([^']*)',$`, 'gm'))];
      assert.equal(entries.length, 1, `${locale}: expected one ${key} label`);
      assert.ok(entries[0][1].trim(), `${locale}: empty ${key} label`);
    }
  }
}

console.log(`Expression learning labels are present in ${locales.length} locales`);
