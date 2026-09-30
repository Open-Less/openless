const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`${actual} != ${expected}`);
  },
  deepEqual(actual: unknown, expected: unknown) {
    if (JSON.stringify(actual) !== JSON.stringify(expected))
      throw new Error(`${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
  },
};
import { PreferencesWriteGate, preferenceEdits } from './preferencesWriteGate';
const gate = new PreferencesWriteGate<{ theme: string; hotkey: { key: string; mode: string } }>();
const prefs = { theme: 'light', hotkey: { key: 'A', mode: 'hold' } };
gate.receiveIncoming({ preferences: prefs, revision: 1 });
const first = gate.beginWrite(prefs, { ...prefs, theme: 'dark' });
assert.deepEqual(first.edits, { '/theme': 'dark' });
gate.finishWrite(first.id, { preferences: { ...prefs, theme: 'dark' }, revision: 2 });
const second = gate.beginWrite(gate.current(), prefs);
gate.finishWrite(second.id, { preferences: prefs, revision: 3 });
assert.equal(
  gate.receiveIncoming({ preferences: { ...prefs, theme: 'dark' }, revision: 4 }).theme,
  'dark',
);
assert.equal(gate.receiveIncoming({ preferences: prefs, revision: 2 }).theme, 'dark');
const third = gate.beginWrite(gate.current(), {
  ...gate.current(),
  hotkey: { key: 'B', mode: 'hold' },
});
assert.deepEqual(third.edits, { '/hotkey/key': 'B' });
assert.deepEqual(
  gate.receiveIncoming({
    preferences: { ...prefs, hotkey: { key: 'A', mode: 'toggle' } },
    revision: 5,
  }).hotkey,
  { key: 'B', mode: 'toggle' },
);
const fourth = gate.beginWrite(gate.current(), { ...gate.current(), theme: 'dark' });
assert.deepEqual(gate.finishWrite(third.id), {
  theme: 'dark',
  hotkey: { key: 'A', mode: 'toggle' },
});
gate.finishWrite(fourth.id, {
  preferences: { ...prefs, theme: 'dark', hotkey: { key: 'A', mode: 'toggle' } },
  revision: 6,
});
assert.deepEqual(preferenceEdits({ items: [1] }, { items: [2] }), { '/items': [2] });
console.log('preference write gate tests passed');

const nullableGate = new PreferencesWriteGate<{ binding: { key: string } | null }>();
nullableGate.receiveIncoming({ preferences: { binding: { key: 'A' } }, revision: 1 });
const nullableWrite = nullableGate.beginWrite(nullableGate.current(), { binding: { key: 'B' } });
assert.deepEqual(nullableGate.receiveIncoming({ preferences: { binding: null }, revision: 2 }), {
  binding: null,
});
assert.deepEqual(nullableGate.finishWrite(nullableWrite.id), { binding: null });
