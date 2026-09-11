import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(
  new URL('../assets/remote-input/app.js', import.meta.url),
  'utf8',
);
const prefix = source.slice(0, source.indexOf('  // 极简插值：'));
assert(prefix.includes('var L = I18N[LANG]'), 'read the actual locale dictionary and resolver');
function labels(injected, systemLanguage = 'en-US') {
  return vm.runInNewContext(
    `${prefix} return { dictionaries: I18N, locale: LANG, labels: L }; })();`,
    {
      window: { __OL_LANG__: injected },
      navigator: { language: systemLanguage },
    },
  );
}

const baseline = labels('en').labels;
for (const locale of ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'es', 'fr', 'de']) {
  const resolved = labels(locale);
  assert.equal(resolved.locale, locale);
  assert.deepEqual(
    Object.keys(resolved.labels).sort(),
    Object.keys(baseline).sort(),
    `${locale} covers every remote input control`,
  );
  for (const [key, value] of Object.entries(baseline)) {
    assert.equal(typeof resolved.labels[key], 'string');
    assert(resolved.labels[key].trim(), `${locale}.${key} has visible text`);
    assert.deepEqual(
      [...resolved.labels[key].matchAll(/\{\w+\}/g)].map(([value]) => value).sort(),
      [...value.matchAll(/\{\w+\}/g)].map(([value]) => value).sort(),
      `${locale}.${key} keeps its runtime placeholders`,
    );
  }
}
assert.equal(labels('', 'es-MX').locale, 'es');
assert.equal(labels('', 'fr-CA').locale, 'fr');
assert.equal(labels('', 'de-CH').locale, 'de');
assert.equal(labels('__proto__', 'en-US').locale, 'en');
console.log('remote input locale tests passed');
