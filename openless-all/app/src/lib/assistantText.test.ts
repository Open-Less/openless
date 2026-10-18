import {
  ANSWER_REVEAL,
  planTextReveal,
  proseTailStart,
  type TextRevealState,
} from './assistantText';

const assert = {
  equal(actual: unknown, expected: unknown, message = 'equal') {
    if (actual !== expected)
      throw new Error(`${message}: ${String(actual)} !== ${String(expected)}`);
  },
  deepEqual(actual: unknown, expected: unknown, message = 'deepEqual') {
    this.equal(JSON.stringify(actual), JSON.stringify(expected), message);
  },
  ok(value: boolean, message = 'expected true') {
    if (!value) throw new Error(message);
  },
};

function tail(text: string) {
  const start = proseTailStart(text);
  return start === null ? null : text.slice(start);
}
assert.equal(tail('A paragraph with a final word.'), 'final word.');
assert.equal(tail('不让最后一个字单独换行。'), '换行。');
assert.equal(tail('正文混排 OpenLess 很舒服！'), '舒服！');
assert.equal(tail('An ending with **formatting**'), 'with **formatting**');
assert.equal(tail('Meet the family 👨‍👩‍👧‍👦!'), 'family 👨‍👩‍👧‍👦!');
assert.equal(tail('Cafe\u0301 déjà.'), 'Cafe\u0301 déjà.');
assert.equal(tail('hello'), null);
assert.equal(tail(''), null);
assert.equal(tail('  \n'), null);

const empty: TextRevealState = { text: '', glyphs: [] };
const first = planTextReveal(empty, '你好 👨‍👩‍👧‍👦 e\u0301\n', 100, true);
assert.deepEqual(
  first.glyphs.map((glyph) => glyph.text),
  ['你', '好', '👨‍👩‍👧‍👦', 'e\u0301'],
);
assert.deepEqual(
  first.glyphs.map((glyph) => glyph.start),
  [100, 108, 116, 124],
);
assert.equal(ANSWER_REVEAL.durationMs, 90);
for (const glyph of first.glyphs) {
  assert.equal(first.text.slice(glyph.offset, glyph.offset + glyph.text.length), glyph.text);
}
const same = planTextReveal(first, first.text, 115, true);
assert.deepEqual(same.glyphs, first.glyphs, 'reparsing must not restart or duplicate reveals');
const next = planTextReveal(first, '你好 👨‍👩‍👧‍👦 e\u0301 又见面\n', 120, true);
assert.deepEqual(next.glyphs.slice(0, 4), first.glyphs, 'appending preserves original birth times');
assert.equal(next.glyphs[4].start, 124);
assert.deepEqual(planTextReveal(next, next.text, 1000, true).glyphs, []);

const rewrite = planTextReveal(first, '你好 family e\u0301\n', 130, true);
assert.deepEqual(rewrite.glyphs, [], 'formatting/correction rewrites must not flash old text');
const final = planTextReveal(next, next.text + ' Final!', 125, false);
assert.equal(final.text, next.text + ' Final!');
assert.deepEqual(final.glyphs, [], 'completion and reduced motion show everything immediately');
assert.deepEqual(
  planTextReveal(final, final.text, 126, true).glyphs,
  [],
  'enabling motion does not replay history',
);

const burst = planTextReveal(first, first.text.trimEnd() + '新'.repeat(10_000), 125, true);
assert.equal(burst.glyphs.length, ANSWER_REVEAL.maxGlyphs);
assert.ok(burst.glyphs.every((glyph) => glyph.start <= 125 + ANSWER_REVEAL.maxLeadMs));
assert.equal(
  burst.text.length,
  first.text.trimEnd().length + 10_000,
  'catch-up never drops content',
);
const reset = planTextReveal(burst, '', 126, true);
assert.deepEqual(reset, empty);
assert.equal(planTextReveal(reset, '新的回答', 127, true).glyphs.length, 4);
const intl = Intl as typeof Intl & { Segmenter?: unknown };
const segmenter = intl.Segmenter;
try {
  intl.Segmenter = undefined;
  assert.equal(proseTailStart('普通中文正文'), 4);
  assert.equal(proseTailStart('family 👨‍👩‍👧‍👦'), null);
  assert.deepEqual(planTextReveal(empty, 'e\u0301 👨‍👩‍👧‍👦', 0, true).glyphs, []);
} finally {
  intl.Segmenter = segmenter;
}
console.log('assistantText.test.ts passed');
