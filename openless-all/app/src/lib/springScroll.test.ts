function assert(value: unknown, message = 'assertion failed'): asserts value {
  if (!value) throw new Error(message);
}
import { springStep } from './springScroll';

const one = springStep(0, 0, 100, 0.1);
assert(one.position > 0 && one.position < 100);
assert(one.velocity > 0);
const two = springStep(one.position, one.velocity, 100, 0.1);
const combined = springStep(0, 0, 100, 0.2);
assert(Math.abs(two.position - combined.position) < 1e-9);
assert(Math.abs(two.velocity - combined.velocity) < 1e-9);
const moving = springStep(0, 300, 100, 0.02);
assert(
  moving.position > springStep(0, 0, 100, 0.02).position,
  'release velocity must contribute to motion',
);
const rebound = springStep(-30, -100, 0, 1);
assert(Math.abs(rebound.position) < 0.01 && Math.abs(rebound.velocity) < 0.1);
console.log('critical spring scroll dynamics passed');
