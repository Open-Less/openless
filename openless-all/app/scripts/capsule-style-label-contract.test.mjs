import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const locales = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'es', 'fr', 'de'];
for (const locale of locales) {
  const source = readFileSync(resolve(root, `src/i18n/${locale}.ts`), 'utf8');
  const match = source.match(/capsuleStyleClassic:\s*['"]([^'"]+)['"]/);
  assert.ok(match, `${locale} must define capsuleStyleClassic`);
  assert.doesNotMatch(
    match[1],
    /OpenLess/i,
    `${locale} default style label must not repeat the product name`,
  );
}
console.log('capsule default style labels omit the redundant product name');
