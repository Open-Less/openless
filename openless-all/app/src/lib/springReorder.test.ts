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
import { insertionSlot, displacedTop } from './useSpringReorder';
const slots = [12, 98, 212, 290];
assert.equal(insertionSlot(-20, slots), 0);
assert.equal(insertionSlot(12, slots), 0);
assert.equal(insertionSlot(55, slots), 0.5);
assert.equal(insertionSlot(400, slots), 3);
assert.equal(displacedTop(98, 1, 1, 86), 184);
assert.equal(displacedTop(98, 1, 1.5, 86), 141);
assert.equal(displacedTop(98, 1, 2, 86), 98);
let previous = Infinity;
for (let y = 0; y < 300; y++) {
  const position = displacedTop(98, 1, insertionSlot(y, slots), 86);
  assert.ok(position <= previous);
  previous = position;
}
console.log('Fractional gaps, unequal row heights, monotonic motion and bounds passed');
