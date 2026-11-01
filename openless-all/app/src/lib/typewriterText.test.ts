import { TYPEWRITER, planBirthDelays } from './typewriterText';

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

assert(planBirthDelays([]).length === 0, 'empty window has no delays');
assert(
  planBirthDelays([false, false]).every((delay) => delay === 0),
  'retained glyphs are not delayed',
);

const single = planBirthDelays([false, true]);
assert(single[1] === 0, 'a lone new glyph appears at once');

const few = planBirthDelays([false, true, true, true]);
assert(
  few.join(',') === `0,0,${TYPEWRITER.maxStepMs},${TYPEWRITER.maxStepMs * 2}`,
  `a short burst types at the slowest pace, got ${few.join(',')}`,
);

const many = planBirthDelays(Array(20).fill(true));
assert(many[1] === TYPEWRITER.minStepMs, 'a long burst types at the fastest pace');
assert(
  many.every((delay, index) => index === 0 || delay > many[index - 1]),
  'born glyphs enter strictly in order',
);

const mixed = planBirthDelays([true, false, true]);
assert(mixed[1] === 0 && mixed[2] > mixed[0], 'only born glyphs advance the typing order');

console.log('typewriterText.test.ts: ok');
