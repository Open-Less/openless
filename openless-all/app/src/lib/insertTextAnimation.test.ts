import {
  INSERT_TEXT_MOTION,
  appendedBirthAdvance,
  clampInsertContentWidth,
  diffInsertUnits,
  firstBornIndex,
  planCapsuleInsertMotion,
  planCharDelays,
  prefixWidths,
  propagationDelayMs,
  resetInsertUnitKeys,
  rightAnchoredPositions,
  segmentInsertUnits,
  type InsertUnit,
} from './insertTextAnimation';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function assertClose(actual: number, expected: number, name: string, epsilon = 0.01) {
  if (Math.abs(actual - expected) > epsilon) {
    throw new Error(`${name}: expected ~${expected}, got ${actual}`);
  }
}

resetInsertUnitKeys();
assertEqual(segmentInsertUnits('帮我').join(''), '帮我', 'CJK stays intact');
assertEqual(segmentInsertUnits('10六号').join(''), '10六号', 'ASCII + CJK mix');
assert(
  segmentInsertUnits('🙂a').length === 2,
  `emoji is one unit, got ${segmentInsertUnits('🙂a').length}`,
);

let units: InsertUnit[] = [];
units = diffInsertUnits(units, '帮我');
assertEqual(units.map((u) => u.text).join(''), '帮我', 'first insert text');
assert(
  units.every((u) => u.born),
  'first insert marks every glyph born',
);
assertEqual(firstBornIndex(units), 0, 'all-new origin is the first glyph');

const afterFirst = units;
units = diffInsertUnits(units, '帮我查找一');
assertEqual(units.map((u) => u.text).join(''), '帮我查找一', 'append keeps prefix');
assertEqual(units[0].key, afterFirst[0].key, 'prefix keys stay stable');
assertEqual(units[1].key, afterFirst[1].key, 'second prefix key stays stable');
assert(
  units.slice(0, 2).every((u) => !u.born),
  'prefix is no longer born',
);
assert(
  units.slice(2).every((u) => u.born),
  'appended glyphs are born',
);
assertEqual(firstBornIndex(units), 2, 'origin sits at the first new glyph');

const beforeReplace = units;
units = diffInsertUnits(units, '帮我查找一下');
assert(
  units.slice(0, 5).every((u) => !u.born),
  'longer prefix stays settled',
);
assertEqual(units[5].text, '下', 'suffix append');
assert(units[5].born, 'new trailing glyph is born');
assertEqual(units[0].key, beforeReplace[0].key, 'keys survive another append');

units = diffInsertUnits(units, '帮我X一下');
assertEqual(
  units.map((u) => u.text).join(''),
  '帮我X一下',
  'middle replace keeps prefix and suffix',
);
assertEqual(units[0].key, beforeReplace[0].key, 'prefix key survives replace');
assert(units[2].born, 'replacement glyph is born');
assertEqual(units[2].text, 'X', 'middle is the replacement');
assert(!units[3].born && units[3].text === '一', 'suffix stays settled');

units = diffInsertUnits(units, '');
assertEqual(units.length, 0, 'empty text clears units');

assertEqual(propagationDelayMs(0), 0, 'zero distance is immediate');
assertEqual(propagationDelayMs(-8), 0, 'negative distance is immediate');
assertClose(
  propagationDelayMs(INSERT_TEXT_MOTION.waveSpeedPxPerMs * 40),
  40,
  'delay is distance / wave speed',
);
assertEqual(
  propagationDelayMs(10_000),
  INSERT_TEXT_MOTION.maxDelayMs,
  'delay is capped so far glyphs still feel concurrent',
);

const widths = [16, 16, 16, 16, 16];
const starts = prefixWidths(widths);
assertEqual(starts.join(','), '0,16,32,48,64', 'prefix widths accumulate');
const delays = planCharDelays(widths, 3);
assert(
  delays[3] < delays[2] && delays[2] < delays[1] && delays[1] < delays[0],
  'wave travels left',
);
assert(delays[4] > delays[3], 'new glyphs to the right of origin stagger slightly');

const plan = planCapsuleInsertMotion({
  stageWidth: 460,
  contentWidth: 120,
  originX: 96,
  minWidth: 72,
  maxWidth: 440,
  padX: 18,
});
assertEqual(plan.width, 156, 'pill hugs text plus horizontal padding');
assertClose(plan.left, (460 - 156) / 2, 'final left is centered');
assertClose(plan.rightEdge, plan.left + plan.width, 'right edge is left + width');
assert(plan.widthDelayMs > INSERT_TEXT_MOTION.widthDelayMinMs, 'left edge waits for the wave');
assert(
  plan.recenterDelayMs > plan.widthDelayMs,
  'recenter lags the leftward expansion, like a second-order correction',
);

const clamped = clampInsertContentWidth(400, 156, 18);
assertEqual(clamped, 120, 'content width cannot exceed inner max');

const tiny = planCapsuleInsertMotion({
  stageWidth: 460,
  contentWidth: 8,
  originX: 0,
  minWidth: 72,
});
assertEqual(tiny.width, 72, 'empty-ish text still has a readable pill');
assertClose(tiny.left + tiny.width / 2, 230, 'tiny pill stays centered');

console.log('insertTextAnimation.test.ts passed');

const oldPositions = rightAnchoredPositions([16, 16]);
const newPositions = rightAnchoredPositions([16, 16, 16]);
assertClose(newPositions[0] - oldPositions[0], -16, 'append pushes existing glyph left');
assertClose(newPositions[1] - oldPositions[1], -16, 'retained glyph destination delta');
assertClose(newPositions[2], -16, 'new glyph ends at right anchor');
assert(
  Math.max(...planCharDelays(Array(200).fill(16), 0)) <= INSERT_TEXT_MOTION.maxDelayMs,
  'batch stagger bounded',
);
const emojiUnits = diffInsertUnits([], 'a👨‍👩‍👧‍👦é');
assertEqual(emojiUnits.length, 3, 'combined emoji and accent intact');
const corrected = diffInsertUnits(emojiUnits, 'b👨‍👩‍👧‍👦é');
assertEqual(corrected[1].key, emojiUnits[1].key, 'correction preserves emoji');
assertEqual(corrected[2].key, emojiUnits[2].key, 'correction preserves suffix');

const paired = planCharDelays(Array(10).fill(17.4), 8, 2);
assert(paired[7] < paired[5] && paired[5] < paired[3], 'pairs propagate from right to left');
assert(paired[6] - paired[7] < 5, 'two-glyph group moves together');
assert(paired[5] - paired[6] > 20, 'next group follows with a perceptible delay');
const triples = planCharDelays(Array(12).fill(17.4), 9, 3);
assert(triples[6] - triples[8] < 5, 'three-glyph batch pushes a three-glyph group');
assert(triples[5] - triples[6] > 20, 'next triple follows');
assert(
  plan.recenterDelayMs - plan.widthDelayMs >= 140,
  'shell is allowed to lean left before recentering',
);

const originalNumber = diffInsertUnits([], '10六号');
const correctedNumber = diffInsertUnits(originalNumber, '十六号');
assertEqual(
  appendedBirthAdvance(correctedNumber, [16, 16, 16]),
  0,
  'correction cannot enter over retained suffix',
);
const appendedNumber = diffInsertUnits(originalNumber, '10六号天气');
assertEqual(
  appendedBirthAdvance(appendedNumber, [16, 16, 16, 16, 16, 16]),
  32,
  'append enters beyond old tail',
);
