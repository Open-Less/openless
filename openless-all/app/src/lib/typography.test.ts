const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`${actual} !== ${expected}`);
  },
  deepEqual(actual: unknown, expected: unknown) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('structures differ');
  },
  ok(value: unknown) {
    if (!value) throw new Error('assertion failed');
  },
  throws(work: () => unknown) {
    let threw = false;
    try {
      work();
    } catch {
      threw = true;
    }
    if (!threw) throw new Error('expected an error');
  },
};
import {
  DEFAULT_TYPOGRAPHY,
  parseTypography,
  FONT_CATALOG,
  TYPOGRAPHY_PRESETS,
} from './typography';
assert.deepEqual(parseTypography(JSON.stringify(DEFAULT_TYPOGRAPHY)), DEFAULT_TYPOGRAPHY);
for (const preset of Object.values(TYPOGRAPHY_PRESETS))
  assert.deepEqual(parseTypography(JSON.stringify(preset)), preset);
for (const invalid of [
  null,
  {},
  { ...DEFAULT_TYPOGRAPHY, version: 2 },
  { ...DEFAULT_TYPOGRAPHY, fonts: { ...DEFAULT_TYPOGRAPHY.fonts, ui: 'url(https://evil/font)' } },
  { ...DEFAULT_TYPOGRAPHY, width: '999999px' },
])
  assert.throws(() => parseTypography(JSON.stringify(invalid)));
assert.throws(() => parseTypography(' '.repeat(16_385)));
const safe = parseTypography(JSON.stringify({ ...DEFAULT_TYPOGRAPHY, extra: 'discarded' }));
assert.equal('extra' in safe, false);
assert.ok(FONT_CATALOG.every((font) => !font.family.includes('url(')));
console.log('Typography validation and local-only font catalog passed');
